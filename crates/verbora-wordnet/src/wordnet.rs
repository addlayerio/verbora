//! The dictionary itself: [`WordNet`], its lookups and its relation traversal.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};

use rustc_hash::FxHashSet;

use crate::data_file::DataFile;
use crate::error::{Error, Result};
use crate::index_file::{IndexEntry, IndexFile, index_key};
use crate::pointer::{Pointer, PointerSymbol};
use crate::pos::PartOfSpeech;
use crate::prebuilt::PrebuiltIndex;
use crate::sense::Sense;
use crate::source::Storage;
use crate::synset::{Synset, SynsetOffset, SynsetRef};

/// Environment variables [`WordNet::from_env`] consults, in order.
const ENV_VARS: [&str; 2] = ["VERBORA_WORDNET_DICT", "WORDNET_DB_PATH"];

/// The relative directory [`WordNet::from_env`] falls back to.
const FALLBACK_DIR: &str = "dict";

/// How a dictionary is opened.
///
/// ```
/// use verbora_wordnet::{Config, Storage};
///
/// // The default: every file read into memory when the dictionary is opened.
/// assert_eq!(Config::default().storage, Storage::Resident);
///
/// // Naming a sidecar selects `Storage::Indexed`, because that is the only
/// // strategy a line-start table is meaningful for.
/// let cfg = Config::default().with_prebuilt("wordnet.vbwnix");
/// assert_eq!(cfg.storage, Storage::Indexed);
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Config {
    /// Which byte-access strategy to use. See [`Storage`].
    pub storage: Storage,
    /// A prebuilt line-start index to load instead of scanning at startup.
    ///
    /// Consulted only when `storage` is [`Storage::Indexed`]. The dictionary
    /// text files remain the source of truth: the sidecar records each file's
    /// length and is rejected if it no longer matches.
    pub prebuilt: Option<PathBuf>,
}

impl Config {
    /// A configuration using `storage` and no prebuilt index.
    #[must_use]
    pub fn new(storage: Storage) -> Self {
        Self {
            storage,
            prebuilt: None,
        }
    }

    /// Loads line-start tables from `path` rather than scanning at startup.
    ///
    /// This also sets [`Config::storage`] to [`Storage::Indexed`], the only
    /// strategy that uses a line-start table.
    #[must_use]
    pub fn with_prebuilt(mut self, path: impl Into<PathBuf>) -> Self {
        self.storage = PrebuiltIndex::STORAGE;
        self.prebuilt = Some(path.into());
        self
    }
}

/// A WordNet dictionary: four index files and four data files.
///
/// Immutable after construction and `Send + Sync`, so one instance can serve
/// concurrent queries from any number of threads with no locking. Nothing is
/// cached per query, so a result never depends on what was looked up before.
///
/// # Data and licensing
///
/// This type *reads* the WordNet database; it does not contain it. The files
/// are covered by Princeton University's own licence — see the crate-level
/// documentation.
#[derive(Debug)]
pub struct WordNet {
    dict_dir: PathBuf,
    indexes: [IndexFile; 4],
    data: [DataFile; 4],
}

impl WordNet {
    /// Opens the dictionary in `dict_dir` with the default strategy
    /// ([`Storage::Resident`]).
    ///
    /// `dict_dir` is the directory holding `index.noun`, `data.noun` and their
    /// six siblings. All eight are opened now, so a missing or unreadable file
    /// is reported here rather than at the first query that happens to need it.
    ///
    /// # Errors
    ///
    /// [`Error::Io`] if any of the eight files cannot be opened.
    pub fn open(dict_dir: impl AsRef<Path>) -> Result<Self> {
        Self::open_with(dict_dir, &Config::default())
    }

    /// Opens the dictionary with an explicit [`Config`].
    ///
    /// # Errors
    ///
    /// [`Error::Io`] if a file cannot be opened, [`Error::Prebuilt`] if a
    /// configured sidecar does not describe the dictionary on disk, or
    /// [`Error::FileTooLarge`] for any storage but [`Storage::Pread`] on a file
    /// of 4 GiB or more.
    pub fn open_with(dict_dir: impl AsRef<Path>, config: &Config) -> Result<Self> {
        let dir = dict_dir.as_ref();

        let prebuilt = match (&config.prebuilt, config.storage) {
            (Some(path), Storage::Indexed) => Some(PrebuiltIndex::load(path)?),
            _ => None,
        };

        let mut indexes = Vec::with_capacity(PartOfSpeech::ALL.len());
        let mut data = Vec::with_capacity(PartOfSpeech::ALL.len());
        for pos in PartOfSpeech::ALL {
            let index_name = format!("index.{}", pos.file_suffix());
            let data_name = format!("data.{}", pos.file_suffix());
            let index_path = dir.join(&index_name);
            let data_path = dir.join(&data_name);
            match &prebuilt {
                Some(pb) => {
                    indexes.push(IndexFile::from_source(
                        index_path.clone(),
                        pos,
                        pb.source_for(&index_name, &index_path)?,
                    ));
                    data.push(DataFile::from_source(
                        data_path.clone(),
                        pos,
                        pb.source_for(&data_name, &data_path)?,
                    ));
                }
                None => {
                    indexes.push(IndexFile::open(&index_path, pos, config.storage)?);
                    data.push(DataFile::open(&data_path, pos, config.storage)?);
                }
            }
        }

        Ok(Self {
            dict_dir: dir.to_path_buf(),
            // `PartOfSpeech::ALL` has exactly four members and the loop pushes
            // once per member, so both conversions succeed by construction.
            indexes: indexes
                .try_into()
                .unwrap_or_else(|_| unreachable!("one index file per part of speech")),
            data: data
                .try_into()
                .unwrap_or_else(|_| unreachable!("one data file per part of speech")),
        })
    }

    /// Locates a dictionary without being told where it is.
    ///
    /// Checks, in order:
    ///
    /// 1. `$VERBORA_WORDNET_DICT` — this crate's own override;
    /// 2. `$WORDNET_DB_PATH` — the variable WordNet distributions conventionally
    ///    set;
    /// 3. `./dict` under the current working directory.
    ///
    /// A candidate counts only if it actually contains `index.noun`, so a stale
    /// variable falls through to the next one instead of failing the whole call.
    ///
    /// # Errors
    ///
    /// [`Error::DictionaryNotFound`], naming every candidate that was tried,
    /// or anything [`WordNet::open_with`] can return once one is found.
    pub fn from_env() -> Result<Self> {
        Self::from_env_with(&Config::default())
    }

    /// [`WordNet::from_env`] with an explicit [`Config`].
    ///
    /// # Errors
    ///
    /// As [`WordNet::from_env`].
    pub fn from_env_with(config: &Config) -> Result<Self> {
        let mut tried = Vec::new();
        for var in ENV_VARS {
            if let Some(value) = std::env::var_os(var) {
                let dir = PathBuf::from(value);
                if dir.join("index.noun").is_file() {
                    return Self::open_with(&dir, config);
                }
                tried.push(dir);
            }
        }
        let local = PathBuf::from(FALLBACK_DIR);
        if local.join("index.noun").is_file() {
            return Self::open_with(&local, config);
        }
        tried.push(local);
        Err(Error::DictionaryNotFound { tried })
    }

    /// The directory the dictionary was opened from.
    #[must_use]
    pub fn dict_dir(&self) -> &Path {
        &self.dict_dir
    }

    /// The index file for a category.
    #[must_use]
    pub fn index_file(&self, pos: PartOfSpeech) -> &IndexFile {
        &self.indexes[pos as usize]
    }

    /// The data file for a category.
    #[must_use]
    pub fn data_file(&self, pos: PartOfSpeech) -> &DataFile {
        &self.data[pos as usize]
    }

    // -----------------------------------------------------------------------
    // lookup
    // -----------------------------------------------------------------------

    /// The index entry for `word` in one category, or `None` if it has none.
    ///
    /// `word` is converted with [`index_key`] first. To search for a key
    /// verbatim, call [`IndexFile::entry`] on [`WordNet::index_file`].
    ///
    /// # Errors
    ///
    /// [`Error::Io`] or [`Error::MalformedIndexEntry`].
    pub fn index_entry(&self, word: &str, pos: PartOfSpeech) -> Result<Option<IndexEntry>> {
        self.index_file(pos).entry(&index_key(word))
    }

    /// Every sense of `word` in one category, in sense order.
    ///
    /// Sense order is the order the index line lists its offsets, which
    /// `wndb(5WN)` defines as most-frequently-tagged first — so element `0` is
    /// sense 1. An unknown word gives an empty vector, not an error.
    ///
    /// # Errors
    ///
    /// [`Error::Io`], [`Error::MalformedIndexEntry`] or
    /// [`Error::MalformedSynset`].
    pub fn senses(&self, word: &str, pos: PartOfSpeech) -> Result<Vec<Synset>> {
        let Some(entry) = self.index_entry(word, pos)? else {
            return Ok(Vec::new());
        };
        let data = self.data_file(pos);
        entry
            .synset_offsets
            .iter()
            .map(|&offset| data.synset(offset))
            .collect()
    }

    /// One numbered sense, or `None` if the word has no such sense.
    ///
    /// # Errors
    ///
    /// As [`WordNet::senses`].
    pub fn sense(&self, sense: &Sense) -> Result<Option<Synset>> {
        let Some(entry) = self.index_entry(&sense.lemma, sense.pos)? else {
            return Ok(None);
        };
        let Some(offset) = entry.offset_for_sense(sense.number) else {
            return Ok(None);
        };
        self.data_file(sense.pos).synset(offset).map(Some)
    }

    /// The lazy primitive behind [`WordNet::lookup`].
    ///
    /// Yields every sense of `word` across all four categories — nouns, then
    /// verbs, then adjectives, then adverbs, and within each category in sense
    /// order — without materialising them all first. Reading one synset costs
    /// one line read, so stopping early genuinely saves the rest.
    ///
    /// The iterator stops after yielding its first [`Err`]: once a read has
    /// failed, continuing would report the same failure repeatedly.
    ///
    /// ```no_run
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// use verbora_wordnet::WordNet;
    ///
    /// let wn = WordNet::from_env()?;
    /// // Take the first two senses without reading the rest.
    /// for synset in wn.lookup_iter("run").take(2) {
    ///     println!("{}", synset?.gloss.definition);
    /// }
    /// # Ok(()) }
    /// ```
    #[must_use]
    pub fn lookup_iter<'a>(&'a self, word: &str) -> LookupIter<'a> {
        LookupIter {
            wordnet: self,
            key: index_key(word).into_owned(),
            next_pos: 0,
            offsets: Vec::new().into_iter(),
            data: None,
            failed: false,
        }
    }

    /// Every sense of `word`, across all four categories.
    ///
    /// `word` is converted with [`index_key`] first. Categories are consulted
    /// in the order noun, verb, adjective, adverb, and within each the senses
    /// come out in sense order.
    ///
    /// There is no deduplication: a lemma that exists as both a noun and a verb
    /// yields the senses of both.
    ///
    /// # Errors
    ///
    /// The first error from reading any index or data file.
    pub fn lookup(&self, word: &str) -> Result<Vec<Synset>> {
        self.lookup_iter(word).collect()
    }

    /// [`WordNet::lookup`] fanned out across a `rayon` thread pool. Requires
    /// the `parallel` feature.
    ///
    /// # Why this exists
    ///
    /// [`WordNet`] is immutable after construction and `Send + Sync`: nothing
    /// is cached per query and nothing is locked, so looking up many words is
    /// embarrassingly parallel with zero coordination between lookups. This
    /// function is exactly `words.par_iter().map(|w| self.lookup(w)).collect()`
    /// — a fan-out over the sequential primitive, not a second implementation
    /// of it. If you need [`WordNet::lookup_iter`]'s laziness in parallel,
    /// apply the same pattern at your own call site.
    ///
    /// # When to reach for it
    ///
    /// When the *batch*, not the single query, is the unit of work — resolving
    /// every distinct token of a corpus against WordNet as an offline step, for
    /// example. A single lookup is cheap enough that a small batch can be
    /// dominated by the cost of scheduling the tasks; prefer a plain
    /// `.iter().map(...)` loop for a handful of words.
    ///
    /// **The crossover point is currently unmeasured for this implementation.**
    /// Earlier figures were measured against a different search algorithm and
    /// have been retired rather than carried forward; reproduce with
    /// `cargo bench -p verbora-wordnet --features parallel -- par_lookup_batch`
    /// before relying on a number.
    ///
    /// # Allocation behaviour
    ///
    /// One `Vec` sized to `words.len()` for the output, plus whatever
    /// [`WordNet::lookup`] allocates per word. No additional buffering, no
    /// locking, and no per-call thread pool: this uses whichever global `rayon`
    /// pool is installed, so pool configuration stays the caller's business.
    ///
    /// # Order and errors
    ///
    /// Output order matches input order — `results[i]` is
    /// `self.lookup(words[i])` — and each element carries its own `Result`, so
    /// one word's failure does not abort the others.
    #[cfg(feature = "parallel")]
    #[cfg_attr(docsrs, doc(cfg(feature = "parallel")))]
    pub fn par_lookup_batch(&self, words: &[&str]) -> Vec<Result<Vec<Synset>>> {
        use rayon::prelude::*;
        words.par_iter().map(|word| self.lookup(word)).collect()
    }

    // -----------------------------------------------------------------------
    // direct synset access
    // -----------------------------------------------------------------------

    /// The synset at `offset` in the file for `pos`, owned.
    ///
    /// An offset is only meaningful together with a category: the same byte
    /// position names a different synset in each of the four data files.
    ///
    /// # Errors
    ///
    /// [`Error::OffsetOutOfRange`] if the offset lies outside the file,
    /// [`Error::MalformedSynset`] if it does not point at the start of a
    /// well-formed record of the right category, or [`Error::Io`].
    pub fn synset(&self, offset: SynsetOffset, pos: PartOfSpeech) -> Result<Synset> {
        self.data_file(pos).synset(offset)
    }

    /// [`WordNet::synset`] without copying: the record is handed to `f` as a
    /// [`SynsetRef`] borrowing the line it was parsed from.
    ///
    /// # Errors
    ///
    /// As [`WordNet::synset`].
    pub fn with_synset<R>(
        &self,
        offset: SynsetOffset,
        pos: PartOfSpeech,
        f: impl FnOnce(&SynsetRef<'_>) -> R,
    ) -> Result<R> {
        self.data_file(pos).with_synset(offset, f)
    }

    /// The synset a pointer points at.
    ///
    /// # Errors
    ///
    /// As [`WordNet::synset`].
    pub fn target(&self, pointer: &Pointer) -> Result<Synset> {
        self.synset(pointer.offset, pointer.part_of_speech())
    }

    // -----------------------------------------------------------------------
    // relation traversal
    // -----------------------------------------------------------------------

    /// Every synset `synset` points at, in file order.
    ///
    /// This is the lazy form of `synset.pointers.iter().map(|p| wn.target(p))`.
    #[must_use]
    pub fn pointers<'a>(&'a self, synset: &'a Synset) -> Pointers<'a> {
        Pointers {
            wordnet: self,
            pointers: synset.pointers.iter(),
            symbol: None,
        }
    }

    /// Only the synsets reached by pointers whose relation is `symbol`.
    ///
    /// ```no_run
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// use verbora_wordnet::{PartOfSpeech, PointerSymbol, WordNet};
    ///
    /// let wn = WordNet::from_env()?;
    /// for synset in wn.senses("node", PartOfSpeech::Noun)? {
    ///     for parent in wn.related(&synset, PointerSymbol::Hypernym) {
    ///         println!("{} -> {}", synset.lemma(), parent?.lemma());
    ///     }
    /// }
    /// # Ok(()) }
    /// ```
    #[must_use]
    pub fn related<'a>(&'a self, synset: &'a Synset, symbol: PointerSymbol) -> Pointers<'a> {
        Pointers {
            wordnet: self,
            pointers: synset.pointers.iter(),
            symbol: Some(symbol),
        }
    }

    /// The transitive closure of one relation from `synset`, breadth first.
    ///
    /// Each reachable synset is yielded at most once — the visited set is keyed
    /// on `(category, offset)`, because an offset alone is ambiguous across the
    /// four data files. The starting synset is **not** yielded, and is marked
    /// visited, so a relation that cycles back to it terminates.
    ///
    /// Following [`PointerSymbol::Hypernym`] repeatedly walks a noun up to
    /// `entity`; following [`PointerSymbol::Hyponym`] from a general synset can
    /// reach tens of thousands of descendants, so prefer `.take(n)` or a filter
    /// unless you want all of them.
    #[must_use]
    pub fn closure<'a>(&'a self, synset: &Synset, symbol: PointerSymbol) -> Closure<'a> {
        let mut seen = FxHashSet::default();
        seen.insert((synset.part_of_speech(), synset.offset));
        Closure {
            wordnet: self,
            symbol,
            queue: synset
                .pointers
                .iter()
                .filter(|p| p.symbol == symbol)
                .copied()
                .collect(),
            seen,
        }
    }
}

/// Iterator returned by [`WordNet::lookup_iter`].
#[derive(Debug)]
pub struct LookupIter<'a> {
    wordnet: &'a WordNet,
    key: String,
    /// Index into [`PartOfSpeech::ALL`] of the next category to consult.
    next_pos: usize,
    /// Offsets still to read for the current category, in sense order.
    offsets: std::vec::IntoIter<SynsetOffset>,
    data: Option<&'a DataFile>,
    /// Set once an error has been yielded, so the iterator stops rather than
    /// reporting the same failure forever.
    failed: bool,
}

impl Iterator for LookupIter<'_> {
    type Item = Result<Synset>;

    fn next(&mut self) -> Option<Result<Synset>> {
        loop {
            if self.failed {
                return None;
            }
            if let Some(offset) = self.offsets.next() {
                // `data` is set together with `offsets`, so it is `Some`
                // whenever an offset is pending.
                let data = self.data?;
                return Some(match data.synset(offset) {
                    Ok(record) => Ok(record),
                    Err(e) => {
                        self.failed = true;
                        Err(e)
                    }
                });
            }
            let pos = *PartOfSpeech::ALL.get(self.next_pos)?;
            self.next_pos += 1;
            match self.wordnet.index_file(pos).entry(&self.key) {
                Ok(Some(entry)) => {
                    self.offsets = entry.synset_offsets.into_iter();
                    self.data = Some(self.wordnet.data_file(pos));
                }
                Ok(None) => {}
                Err(e) => {
                    self.failed = true;
                    return Some(Err(e));
                }
            }
        }
    }
}

impl std::iter::FusedIterator for LookupIter<'_> {}

/// Iterator returned by [`WordNet::pointers`] and [`WordNet::related`].
#[derive(Debug)]
pub struct Pointers<'a> {
    wordnet: &'a WordNet,
    pointers: std::slice::Iter<'a, Pointer>,
    symbol: Option<PointerSymbol>,
}

impl Iterator for Pointers<'_> {
    type Item = Result<Synset>;

    fn next(&mut self) -> Option<Result<Synset>> {
        for pointer in self.pointers.by_ref() {
            if self.symbol.is_some_and(|want| pointer.symbol != want) {
                continue;
            }
            return Some(self.wordnet.target(pointer));
        }
        None
    }
}

impl std::iter::FusedIterator for Pointers<'_> {}

/// Iterator returned by [`WordNet::closure`].
#[derive(Debug)]
pub struct Closure<'a> {
    wordnet: &'a WordNet,
    symbol: PointerSymbol,
    queue: VecDeque<Pointer>,
    /// Every `(category, offset)` already emitted, plus the starting synset.
    seen: FxHashSet<(PartOfSpeech, SynsetOffset)>,
}

impl Iterator for Closure<'_> {
    type Item = Result<Synset>;

    fn next(&mut self) -> Option<Result<Synset>> {
        while let Some(pointer) = self.queue.pop_front() {
            if !self.seen.insert((pointer.part_of_speech(), pointer.offset)) {
                continue;
            }
            return Some(match self.wordnet.target(&pointer) {
                Ok(synset) => {
                    self.queue
                        .extend(synset.pointers.iter().filter(|p| p.symbol == self.symbol));
                    Ok(synset)
                }
                Err(e) => {
                    // Stop rather than continuing past a file that cannot be
                    // read: every later answer would be from the same file.
                    self.queue.clear();
                    Err(e)
                }
            });
        }
        None
    }
}

impl std::iter::FusedIterator for Closure<'_> {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wordnet_is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<WordNet>();
        assert_send_sync::<IndexFile>();
        assert_send_sync::<DataFile>();
    }

    #[test]
    fn naming_a_sidecar_selects_the_indexed_strategy() {
        let cfg = Config::new(Storage::Pread).with_prebuilt("x.vbwnix");
        assert_eq!(cfg.storage, Storage::Indexed);
        assert_eq!(cfg.prebuilt.as_deref(), Some(Path::new("x.vbwnix")));
        assert_eq!(Config::default().prebuilt, None);
    }

    #[test]
    fn the_index_and_data_arrays_are_addressed_by_category() {
        // `index_file`/`data_file` index by `pos as usize`, which is only
        // correct if the enum's discriminants match `PartOfSpeech::ALL`'s order.
        for (i, pos) in PartOfSpeech::ALL.iter().enumerate() {
            assert_eq!(*pos as usize, i, "{pos:?}");
        }
    }

    #[test]
    fn a_missing_dictionary_fails_at_open() {
        assert!(matches!(
            WordNet::open("/no/such/dir"),
            Err(Error::Io { .. })
        ));
    }
}
