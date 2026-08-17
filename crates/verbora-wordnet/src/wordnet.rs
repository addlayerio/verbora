//! The dictionary itself: `WordNet`, its lookups, and the pointer traversal.
//!
//! # Result order is defined by a bug, and it is asserted by the reference's own
//! test suite
//!
//! Every recursive helper in `wordnet` walks its work list with `pop()`, so
//! everything comes out **backwards**, at two levels:
//!
//! * the parts of speech are consulted in the order **adv, adj, verb, noun** —
//!   the reverse of the `[noun, verb, adj, adv]` array literal that produces
//!   them;
//! * within one part of speech, the synset offsets listed on the index line are
//!   visited **last to first**.
//!
//! `lookup("fast")` therefore yields `r:86892 r:86488 s:324771 … n:1071904`, and
//! `getSynonyms(1740, 'n')` yields `[4431553, 2137, 1930]` for an index line that
//! lists `1930, 2137, 4431553`. `io_spec/wordnet_spec` asserts the latter by
//! value. An idiomatic `for file in files` / `for offset in offsets` produces the
//! exact reverse for every single lookup, which is why every traversal below
//! iterates with `.rev()` and every `&mut Vec` helper drains with `pop`.
//!
//! # Callbacks become return values
//!
//! The reference API is callback-async throughout: `lookup`, `get`,
//! `lookupSynonyms` and `getSynonyms` all return `undefined` and deliver later.
//! Within a single operation, though, every read is strictly sequential — each
//! `fs` callback issues the next read — so a synchronous port is *order
//! equivalent*, and that is what this crate provides. The mapping is:
//!
//! | the reference | `verbora` |
//! |---|---|
//! | `wn.lookup(w, cb)` | [`WordNet::lookup`] → `Result<Vec<DataRecord>>` |
//! | | [`WordNet::lookup_iter`] → lazy iterator |
//! | `wn.get(off, pos, cb)` | [`WordNet::get`] |
//! | `wn.getDataFile(pos)` | [`WordNet::data_file`] → `Option` for `undefined` |
//! | `wn.lookupSynonyms(w, cb)` | [`WordNet::lookup_synonyms`] |
//! | `wn.getSynonyms(off, pos, cb)` | [`WordNet::get_synonyms`] |
//! | `wn.getSynonyms(record, cb)` | [`WordNet::get_synonyms_of`] |
//! | `wn.lookupFromFiles(f, r, w, cb)` | [`WordNet::lookup_from_files`] — takes `&mut Vec`, and drains them, because the reference does |
//! | `wn.pushResults(d, r, o, cb)` | [`WordNet::push_results`] |
//! | `wn.loadSynonyms(s, r, p, cb)` | [`WordNet::load_synonyms`] |
//! | `wn.loadResultSynonyms(s, r, cb)` | [`WordNet::load_result_synonyms`] |

use std::path::{Path, PathBuf};

use crate::data_file::{DataFile, DataRecord, Pointer};
use crate::error::{Error, Result};
use crate::index_file::IndexFile;
use crate::prebuilt::PrebuiltIndex;
use crate::source::Storage;
use crate::whitespace::normalize_lookup_word;

/// The four dictionary file pairs, in the order the reference's array literal
/// lists them. Traversal iterates this **in reverse**, because `files.pop()` does.
pub(crate) const POS_ORDER: [Pos; 4] = [Pos::Noun, Pos::Verb, Pos::Adj, Pos::Adv];

/// Which pair of dictionary files a part-of-speech tag routes to.
///
/// `getDataFile` is a five-arm `switch` with **no default clause**: `'a'` and
/// `'s'` (adjective head and satellite) both select the adjective files, and
/// anything else — including `'N'`, `'noun'` and `''` — yields `undefined`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Pos {
    /// `n` — `index.noun` / `data.noun`.
    Noun,
    /// `v` — `index.verb` / `data.verb`.
    Verb,
    /// `a` and `s` — `index.adj` / `data.adj`.
    Adj,
    /// `r` — `index.adv` / `data.adv`.
    Adv,
}

impl Pos {
    /// The reference's `switch (pos)`, exactly: `Some` for `n`, `v`, `a`, `s`,
    /// `r`, and `None` — the reference's `undefined` — for everything else.
    ///
    /// ```
    /// use verbora_wordnet::Pos;
    ///
    /// assert_eq!(Pos::from_tag("a"), Some(Pos::Adj));
    /// assert_eq!(Pos::from_tag("s"), Some(Pos::Adj)); // satellite adjective
    /// assert_eq!(Pos::from_tag("N"), None);           // matching is case sensitive
    /// assert_eq!(Pos::from_tag("noun"), None);
    /// ```
    #[must_use]
    pub fn from_tag(tag: &str) -> Option<Self> {
        match tag.as_bytes() {
            b"n" => Some(Self::Noun),
            b"v" => Some(Self::Verb),
            b"a" | b"s" => Some(Self::Adj),
            b"r" => Some(Self::Adv),
            _ => None,
        }
    }

    /// The file-name suffix: `noun`, `verb`, `adj`, `adv`.
    #[must_use]
    pub fn suffix(self) -> &'static str {
        match self {
            Self::Noun => "noun",
            Self::Verb => "verb",
            Self::Adj => "adj",
            Self::Adv => "adv",
        }
    }

    /// The canonical tag: `n`, `v`, `a`, `r`. Note that [`Pos::Adj`] answers
    /// `"a"`; the satellite tag `"s"` maps *in* but never *out*.
    #[must_use]
    pub fn tag(self) -> &'static str {
        match self {
            Self::Noun => "n",
            Self::Verb => "v",
            Self::Adj => "a",
            Self::Adv => "r",
        }
    }

    /// All four, in the reference's array-literal order.
    #[must_use]
    pub fn all() -> [Self; 4] {
        POS_ORDER
    }
}

/// How a dictionary is opened.
#[derive(Debug, Clone, Default)]
pub struct Config {
    /// Which byte-access strategy to use. See [`Storage`].
    pub storage: Storage,
    /// A prebuilt line-start index to load instead of scanning at startup.
    ///
    /// Only consulted when `storage` is [`Storage::Indexed`]. The dictionary
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
    #[must_use]
    pub fn with_prebuilt(mut self, path: impl Into<PathBuf>) -> Self {
        self.storage = Storage::Indexed;
        self.prebuilt = Some(path.into());
        self
    }
}

/// One index/data file pair, as `lookupFromFiles` receives it.
///
/// The reference lets the two halves disagree — nothing checks that the index
/// and the data file describe the same part of speech — and reading `index.adv`
/// offsets out of `data.noun` produces garbage records rather than an error. That
/// is preserved here.
#[derive(Debug, Clone, Copy)]
pub struct FilePair<'a> {
    /// The index file to search.
    pub index: &'a IndexFile,
    /// The data file to read the resulting offsets from.
    pub data: &'a DataFile,
}

/// A WordNet dictionary: four index files and four data files.
///
/// Immutable after construction and `Send + Sync`, so one instance can serve
/// concurrent queries from any number of threads with no locking. Nothing is
/// cached per query — the reference caches nothing either — so results never
/// depend on what was looked up before.
///
/// # Data and licensing
///
/// This type reads the WordNet database; it does not contain it. The files are
/// covered by Princeton University's own licence, reproduced in
/// `LICENSE-WORDNET` beside this crate. See the crate-level documentation.
#[derive(Debug)]
pub struct WordNet {
    dict_dir: PathBuf,
    indexes: [IndexFile; 4],
    datas: [DataFile; 4],
}

impl WordNet {
    /// Opens the dictionary in `dict_dir` with the default strategy.
    ///
    /// `dict_dir` is the directory holding `index.noun`, `data.noun` and their
    /// six siblings — the path `require('wordnet-db').path` returns in Node.
    ///
    /// Unlike the reference, which constructs successfully against a nonexistent
    /// directory and only stalls silently at the first lookup, this fails
    /// immediately if a file is missing.
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
    /// [`Error::Io`] if a file cannot be opened, or [`Error::Prebuilt`] if a
    /// configured sidecar index does not match the dictionary on disk.
    pub fn open_with(dict_dir: impl AsRef<Path>, config: &Config) -> Result<Self> {
        let dir = dict_dir.as_ref();
        let dir_str = dir.to_string_lossy().into_owned();

        let prebuilt = match (&config.prebuilt, config.storage) {
            (Some(path), Storage::Indexed) => Some(PrebuiltIndex::load(path)?),
            _ => None,
        };

        let mut indexes = Vec::with_capacity(4);
        let mut datas = Vec::with_capacity(4);
        for pos in POS_ORDER {
            let index_name = format!("index.{}", pos.suffix());
            let data_name = format!("data.{}", pos.suffix());
            match &prebuilt {
                Some(pb) => {
                    let ipath = crate::whitespace::path_join(&dir_str, &index_name);
                    let dpath = crate::whitespace::path_join(&dir_str, &data_name);
                    indexes.push(IndexFile::from_source(
                        ipath.clone(),
                        pb.source_for(&index_name, Path::new(&ipath))?,
                    ));
                    datas.push(DataFile::from_source(
                        dpath.clone(),
                        pb.source_for(&data_name, Path::new(&dpath))?,
                    ));
                }
                None => {
                    indexes.push(IndexFile::open_path(dir, &index_name, config.storage)?);
                    datas.push(DataFile::open_path(dir, &data_name, config.storage)?);
                }
            }
        }

        Ok(Self {
            dict_dir: dir.to_path_buf(),
            indexes: indexes
                .try_into()
                .unwrap_or_else(|_| unreachable!("exactly four parts of speech")),
            datas: datas
                .try_into()
                .unwrap_or_else(|_| unreachable!("exactly four parts of speech")),
        })
    }

    /// Locates a dictionary without being told where it is.
    ///
    /// Checks, in order:
    ///
    /// 1. `$WORDNET_DB_PATH` — the variable `wordnet-db` consumers conventionally
    ///    set;
    /// 2. `$VERBORA_WORDNET_DICT` — this crate's own override;
    /// 3. `./node_modules/wordnet-db/dict` under the current directory.
    ///
    /// The database is deliberately **not** vendored into this crate: it is
    /// separately licensed and 34 MB unpacked. Install it with
    /// `npm install wordnet-db` and point one of the variables at the `dict`
    /// directory.
    ///
    /// # Errors
    ///
    /// [`Error::Io`] with a `NotFound` source, naming the candidates tried, when
    /// none of them exists.
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
        for var in ["WORDNET_DB_PATH", "VERBORA_WORDNET_DICT"] {
            if let Some(v) = std::env::var_os(var) {
                let p = PathBuf::from(v);
                if p.join("index.noun").is_file() {
                    return Self::open_with(&p, config);
                }
                tried.push(format!("${var} = {}", p.display()));
            }
        }
        let local = PathBuf::from("node_modules/wordnet-db/dict");
        if local.join("index.noun").is_file() {
            return Self::open_with(&local, config);
        }
        tried.push(local.display().to_string());

        Err(Error::Io {
            path: local,
            source: std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!(
                    "no WordNet dictionary found (tried: {}); \
                     install it with `npm install wordnet-db` and set \
                     $WORDNET_DB_PATH to its dict/ directory",
                    tried.join(", ")
                ),
            ),
        })
    }

    /// The directory the dictionary was opened from.
    #[must_use]
    pub fn dict_dir(&self) -> &Path {
        &self.dict_dir
    }

    /// The index file for a part of speech.
    #[must_use]
    pub fn index_file(&self, pos: Pos) -> &IndexFile {
        &self.indexes[pos as usize]
    }

    /// The data file for a part of speech.
    #[must_use]
    pub fn data_file_for(&self, pos: Pos) -> &DataFile {
        &self.datas[pos as usize]
    }

    /// `getDataFile(pos)`: the data file for a *tag*, or `None` for the
    /// reference's `undefined`.
    #[must_use]
    pub fn data_file(&self, tag: &str) -> Option<&DataFile> {
        Pos::from_tag(tag).map(|p| self.data_file_for(p))
    }

    /// All four index/data pairs, in the reference's array-literal order.
    #[must_use]
    pub fn file_pairs(&self) -> [FilePair<'_>; 4] {
        [
            self.pair(Pos::Noun),
            self.pair(Pos::Verb),
            self.pair(Pos::Adj),
            self.pair(Pos::Adv),
        ]
    }

    /// One index/data pair.
    #[must_use]
    pub fn pair(&self, pos: Pos) -> FilePair<'_> {
        FilePair {
            index: self.index_file(pos),
            data: self.data_file_for(pos),
        }
    }

    // -----------------------------------------------------------------------
    // lookup
    // -----------------------------------------------------------------------

    /// The lazy primitive behind [`WordNet::lookup`].
    ///
    /// Yields synsets in the reference's order — adv, adj, verb, noun, and within
    /// each part of speech the index line's offsets last-to-first — without
    /// materialising them all first. Reading a single synset out of `data.noun`
    /// costs one line read, so stopping early genuinely saves the rest.
    ///
    /// ```no_run
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// use verbora_wordnet::WordNet;
    ///
    /// let wn = WordNet::from_env()?;
    /// // "run" has 57 senses; take the first two without reading the other 55.
    /// for record in wn.lookup_iter("run").take(2) {
    ///     println!("{}", record?.def);
    /// }
    /// # Ok(()) }
    /// ```
    #[must_use]
    pub fn lookup_iter<'a>(&'a self, word: &str) -> LookupIter<'a> {
        LookupIter {
            wordnet: self,
            word: normalize_lookup_word(word),
            // `files.pop()` visits the array literal backwards.
            next_pos: 4,
            offsets: Vec::new(),
            data: None,
            failed: false,
        }
    }

    /// `WordNet#lookup`: every synset for `word`, across all four parts of
    /// speech.
    ///
    /// `word` is lowercased and its whitespace runs are replaced with single
    /// underscores first, so `"New York"` and `"new  york"` both become
    /// `"new_york"` — and `"  entity  "` becomes `"_entity_"`, which misses.
    ///
    /// No deduplication: a synset reachable from two parts of speech appears
    /// twice, exactly as in the reference.
    ///
    /// # Errors
    ///
    /// Any [`Error`] from reading the index or data files.
    pub fn lookup(&self, word: &str) -> Result<Vec<DataRecord>> {
        self.lookup_iter(word).collect()
    }

    /// [`WordNet::lookup`], fanned out across a `rayon` thread pool. Requires
    /// the `parallel` feature.
    ///
    /// # Why this exists
    ///
    /// [`WordNet`] is immutable after construction and `Send + Sync` (see the
    /// [module-level "Concurrency" section](crate::wordnet) and
    /// `queries_run_concurrently_on_a_shared_dictionary` in this crate's own
    /// test suite): nothing is cached per query and nothing is locked, so
    /// looking up many words is embarrassingly parallel with zero coordination
    /// cost between lookups. This function is exactly
    /// `words.par_iter().map(|w| self.lookup(w)).collect()` — a thin fan-out
    /// over the existing sequential primitive, not a second implementation of
    /// it. `lookup_iter`'s laziness, `lookup_from_files`'s in-place draining,
    /// and every other entry point are untouched; if you need those shapes in
    /// parallel, apply the same `par_iter().map(...)` pattern at your own call
    /// site (see `site/performance/parallelism.md`).
    ///
    /// # When to reach for it vs. the sequential loop
    ///
    /// Reach for this only when the *batch*, not the single query, is the unit
    /// of work — for example, resolving every distinct token in a large corpus
    /// against WordNet as an offline step. Measured per-item costs on this
    /// crate's own benchmarks (`Storage::Resident`, warm dictionary):
    ///
    /// | Case | Cost |
    /// |---|--:|
    /// | a common entry (`entity`) | ~5.7–6.9 µs |
    /// | the worst case (`run`, 57 senses) | ~150–206 µs |
    /// | a miss (`zzzzz`) | ~4–5.4 µs |
    ///
    /// Measured on `benches/wordnet.rs`'s own `par_lookup_batch` group
    /// (`Storage::Resident`, 32 hardware threads, `WORDS` — the same 16-word
    /// mix `bench_repeat` uses — repeated out to each size), sequential vs.
    /// `par_lookup_batch`:
    ///
    /// | Batch size | Sequential | Parallel | Speedup |
    /// |--:|--:|--:|--:|
    /// | 16 | 633.5 µs | 600.6 µs | ~1.05× (noise-level) |
    /// | 160 | 7.03 ms | 2.10 ms | ~3.3× |
    /// | 1600 | 100.2 ms | 24.95 ms | ~4.0× |
    ///
    /// A batch of 16 common words is close to the break-even point — a `rayon`
    /// task costs on the order of a microsecond to schedule (see
    /// `site/performance/parallelism.md`), which is comparable to a single
    /// lookup's own cost, so the win there is within noise. Prefer a plain
    /// `.iter().map(WordNet::lookup)` loop at that scale. From a few hundred
    /// words up — where `run`-like high-sense-count words recur often enough
    /// to matter, or the batch itself is simply large — the scheduling cost
    /// amortises and the win is real. Reproduce with
    /// `cargo bench -p verbora-wordnet --features parallel -- par_lookup_batch`.
    ///
    /// # Allocation behaviour
    ///
    /// One `Vec<Result<Vec<DataRecord>>>` sized to `words.len()` for the
    /// output, plus whatever [`WordNet::lookup`] itself allocates per word (one
    /// `Vec<DataRecord>` per successful lookup, growing to the sense count).
    /// No additional buffering, no locking, no per-call thread-pool
    /// construction — this uses whichever global `rayon` pool is already
    /// installed (or `rayon`'s default one), so pool configuration remains the
    /// caller's responsibility, not this crate's.
    ///
    /// # Order and errors
    ///
    /// Output order matches input order — `results[i]` is `self.lookup(words[i])`
    /// — via `rayon`'s order-preserving `map` + `collect`, unlike this crate's
    /// own traversal order for the *senses within* a single lookup (see the
    /// module-level "Result order" section above, which this function does not
    /// change). Each element carries its own `Result`, exactly as a sequential
    /// `words.iter().map(|w| self.lookup(w)).collect::<Vec<_>>()` would: one
    /// word's [`Error`] does not abort the others.
    #[cfg(feature = "parallel")]
    pub fn par_lookup_batch(&self, words: &[&str]) -> Vec<Result<Vec<DataRecord>>> {
        use rayon::prelude::*;
        words.par_iter().map(|word| self.lookup(word)).collect()
    }

    /// `WordNet#lookupFromFiles`, mutation included.
    ///
    /// `files` is drained to empty and `results` is appended to, because that is
    /// what the reference's `files.pop()` and `results.push()` do — and both are
    /// observable, since the caller supplies the arrays. Prefer
    /// [`WordNet::lookup`] unless you are reproducing that.
    ///
    /// # Errors
    ///
    /// Any [`Error`] from reading the index or data files.
    pub fn lookup_from_files(
        &self,
        files: &mut Vec<FilePair<'_>>,
        results: &mut Vec<DataRecord>,
        word: &str,
    ) -> Result<()> {
        while let Some(pair) = files.pop() {
            let Some(record) = pair.index.lookup(word)? else {
                continue;
            };
            let mut offsets = record.synset_offset;
            self.push_results(pair.data, results, &mut offsets)?;
        }
        Ok(())
    }

    /// `WordNet#pushResults`, mutation included: drains `offsets` from the back
    /// and appends each record to `results`.
    ///
    /// # Errors
    ///
    /// Any [`Error`] from reading the data file.
    pub fn push_results(
        &self,
        data: &DataFile,
        results: &mut Vec<DataRecord>,
        offsets: &mut Vec<f64>,
    ) -> Result<()> {
        while let Some(offset) = offsets.pop() {
            results.push(data.get(offset)?);
        }
        Ok(())
    }

    // -----------------------------------------------------------------------
    // get
    // -----------------------------------------------------------------------

    /// `WordNet#get`: the synset at a byte offset in the file for `tag`.
    ///
    /// `synset_offset` is a **byte offset**, not a record index, and is not
    /// validated: a mid-record offset yields a garbage record rather than an
    /// error, and an offset inside the 29-line licence header yields
    /// [`Error::MissingGloss`] (where the reference throws asynchronously).
    ///
    /// # Errors
    ///
    /// [`Error::UnknownPos`] for a tag outside `n`, `v`, `a`, `s`, `r` — where
    /// The reference throws `TypeError … reading 'get'` — plus anything
    /// [`DataFile::get`] can return.
    pub fn get(&self, synset_offset: f64, tag: &str) -> Result<DataRecord> {
        let data = self
            .data_file(tag)
            .ok_or_else(|| Error::UnknownPos(tag.to_owned()))?;
        data.get(synset_offset)
    }

    /// [`WordNet::get`] with a typed part of speech, which cannot fail to resolve.
    ///
    /// # Errors
    ///
    /// Anything [`DataFile::get`] can return.
    pub fn get_at(&self, synset_offset: f64, pos: Pos) -> Result<DataRecord> {
        self.data_file_for(pos).get(synset_offset)
    }

    // -----------------------------------------------------------------------
    // synonyms
    // -----------------------------------------------------------------------

    /// `WordNet#loadSynonyms`, mutation included.
    ///
    /// Drains `ptrs` from the back, appending the synset each one points at to
    /// `synonyms`, then hands `results` to [`WordNet::load_result_synonyms`],
    /// which drains that too. The reference's two functions are mutually
    /// recursive; the loops here are the same traversal without the recursion.
    ///
    /// # Errors
    ///
    /// [`Error::UnknownPos`] if a pointer's `pos` is not one of `n`, `v`, `a`,
    /// `s`, `r` — which does not occur in the shipped database — plus anything
    /// [`DataFile::get`] can return.
    pub fn load_synonyms(
        &self,
        synonyms: &mut Vec<DataRecord>,
        results: &mut Vec<DataRecord>,
        ptrs: &mut Vec<Pointer>,
    ) -> Result<()> {
        while let Some(ptr) = ptrs.pop() {
            synonyms.push(self.follow(&ptr)?);
        }
        self.load_result_synonyms(synonyms, results)
    }

    /// `WordNet#loadResultSynonyms`, mutation included.
    ///
    /// Drains `results` from the back; for each record, drains that record's own
    /// pointer list from the back too. So the output order is: last result
    /// first, and within it, last pointer first.
    ///
    /// # Errors
    ///
    /// As [`WordNet::load_synonyms`].
    pub fn load_result_synonyms(
        &self,
        synonyms: &mut Vec<DataRecord>,
        results: &mut Vec<DataRecord>,
    ) -> Result<()> {
        while let Some(mut result) = results.pop() {
            while let Some(ptr) = result.ptrs.pop() {
                synonyms.push(self.follow(&ptr)?);
            }
        }
        Ok(())
    }

    /// `WordNet#lookupSynonyms`: every synset one pointer hop from any sense of
    /// `word`.
    ///
    /// Duplicates are possible and are kept: two senses pointing at the same
    /// synset yield it twice.
    ///
    /// # Errors
    ///
    /// As [`WordNet::lookup`] and [`WordNet::load_result_synonyms`].
    pub fn lookup_synonyms(&self, word: &str) -> Result<Vec<DataRecord>> {
        let mut results = self.lookup(word)?;
        let mut synonyms = Vec::new();
        self.load_result_synonyms(&mut synonyms, &mut results)?;
        Ok(synonyms)
    }

    /// `WordNet#getSynonyms(synsetOffset, pos, cb)`.
    ///
    /// Re-reads the synset from disk and follows its pointers, last to first.
    ///
    /// # Errors
    ///
    /// As [`WordNet::get`] and [`WordNet::load_synonyms`].
    pub fn get_synonyms(&self, synset_offset: f64, tag: &str) -> Result<Vec<DataRecord>> {
        let record = self.get(synset_offset, tag)?;
        let mut ptrs = record.ptrs;
        let mut synonyms = Vec::new();
        let mut results = Vec::new();
        self.load_synonyms(&mut synonyms, &mut results, &mut ptrs)?;
        Ok(synonyms)
    }

    /// `WordNet#getSynonyms(record, cb)`.
    ///
    /// Note that the reference **re-reads** the synset from disk using the
    /// record's own offset and tag rather than using the pointers it already
    /// holds, so the caller's record is left untouched. This does the same, which
    /// is why it takes `&DataRecord` rather than consuming it.
    ///
    /// # Errors
    ///
    /// [`Error::UnknownPos`] if `record.pos` is absent or unrecognised, plus
    /// anything [`WordNet::get_synonyms`] can return.
    pub fn get_synonyms_of(&self, record: &DataRecord) -> Result<Vec<DataRecord>> {
        let tag = record
            .pos
            .as_deref()
            .ok_or_else(|| Error::UnknownPos(String::new()))?;
        self.get_synonyms(record.synset_offset, tag)
    }

    /// Reads the synset a pointer points at.
    fn follow(&self, ptr: &Pointer) -> Result<DataRecord> {
        let tag = ptr.pos.as_deref().unwrap_or("");
        self.get(ptr.synset_offset, tag)
    }

    // -----------------------------------------------------------------------
    // relation traversal
    // -----------------------------------------------------------------------

    /// Every synset `record` points at, in the reference's order (last pointer
    /// first).
    ///
    /// This is [`WordNet::get_synonyms_of`] made lazy, and without the re-read:
    /// it uses the pointers already on `record`.
    #[must_use]
    pub fn pointers<'a>(&'a self, record: &'a DataRecord) -> Pointers<'a> {
        Pointers {
            wordnet: self,
            ptrs: &record.ptrs,
            next: record.ptrs.len(),
            symbol: None,
        }
    }

    /// Only the synsets reached by pointers whose symbol is `symbol`.
    ///
    /// ```no_run
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// use verbora_wordnet::{WordNet, pointer};
    ///
    /// let wn = WordNet::from_env()?;
    /// let node = wn.get(3_832_647.0, "n")?;
    /// for parent in wn.relation(&node, pointer::HYPERNYM) {
    ///     println!("{}", parent?.def);
    /// }
    /// # Ok(()) }
    /// ```
    #[must_use]
    pub fn relation<'a>(&'a self, record: &'a DataRecord, symbol: &'a str) -> Pointers<'a> {
        Pointers {
            wordnet: self,
            ptrs: &record.ptrs,
            next: record.ptrs.len(),
            symbol: Some(symbol),
        }
    }

    /// The transitive closure of `symbol` from `record`, breadth first.
    ///
    /// Cycle safe: each synset offset is visited at most once. Useful for
    /// hypernym chains, where following `@` repeatedly walks up to `entity`.
    ///
    /// The starting record is **not** yielded.
    #[must_use]
    pub fn closure<'a>(&'a self, record: &DataRecord, symbol: &'a str) -> Closure<'a> {
        let queue: std::collections::VecDeque<Pointer> =
            record.ptrs.iter().rev().cloned().collect();
        Closure {
            wordnet: self,
            symbol,
            queue,
            seen: vec![record.synset_offset.to_bits()],
        }
    }
}

/// Iterator returned by [`WordNet::lookup_iter`].
#[derive(Debug)]
pub struct LookupIter<'a> {
    wordnet: &'a WordNet,
    word: String,
    /// Index into [`POS_ORDER`], counted down: `files.pop()` walks it backwards.
    next_pos: usize,
    /// Offsets still to read for the current part of speech, already reversed.
    offsets: Vec<f64>,
    data: Option<&'a DataFile>,
    /// Set once an error has been yielded, so the iterator stops rather than
    /// re-reporting the same failure forever.
    failed: bool,
}

impl Iterator for LookupIter<'_> {
    type Item = Result<DataRecord>;

    fn next(&mut self) -> Option<Result<DataRecord>> {
        loop {
            if self.failed {
                return None;
            }
            if let Some(offset) = self.offsets.pop() {
                let data = self.data?;
                return Some(match data.get(offset) {
                    Ok(r) => Ok(r),
                    Err(e) => {
                        self.failed = true;
                        Err(e)
                    }
                });
            }
            if self.next_pos == 0 {
                return None;
            }
            self.next_pos -= 1;
            let pos = POS_ORDER[self.next_pos];
            let pair = self.wordnet.pair(pos);
            match pair.index.lookup(&self.word) {
                Ok(Some(record)) => {
                    // `offsets.pop()` visits the index line's offsets backwards;
                    // popping a forward Vec does the same thing.
                    self.offsets = record.synset_offset;
                    self.data = Some(pair.data);
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

/// Iterator returned by [`WordNet::pointers`] and [`WordNet::relation`].
#[derive(Debug)]
pub struct Pointers<'a> {
    wordnet: &'a WordNet,
    ptrs: &'a [Pointer],
    /// One past the next pointer to visit; counted down, because the reference
    /// drains the list from the back.
    next: usize,
    symbol: Option<&'a str>,
}

impl Iterator for Pointers<'_> {
    type Item = Result<DataRecord>;

    fn next(&mut self) -> Option<Result<DataRecord>> {
        while self.next > 0 {
            self.next -= 1;
            let ptr = &self.ptrs[self.next];
            if let Some(want) = self.symbol {
                if ptr.pointer_symbol.as_deref() != Some(want) {
                    continue;
                }
            }
            return Some(self.wordnet.follow(ptr));
        }
        None
    }
}

/// Iterator returned by [`WordNet::closure`].
#[derive(Debug)]
pub struct Closure<'a> {
    wordnet: &'a WordNet,
    symbol: &'a str,
    queue: std::collections::VecDeque<Pointer>,
    /// Offsets already emitted, as bit patterns so `NaN` compares by identity.
    seen: Vec<u64>,
}

impl Iterator for Closure<'_> {
    type Item = Result<DataRecord>;

    fn next(&mut self) -> Option<Result<DataRecord>> {
        while !self.queue.is_empty() {
            let ptr = self.queue.pop_front()?;
            if ptr.pointer_symbol.as_deref() != Some(self.symbol) {
                continue;
            }
            let key = ptr.synset_offset.to_bits();
            if self.seen.contains(&key) {
                continue;
            }
            self.seen.push(key);
            return Some(match self.wordnet.follow(&ptr) {
                Ok(record) => {
                    // Children enter the queue in the same reversed order the
                    // root's own pointers did, so the walk is consistent with
                    // `pointers()` at every level.
                    self.queue.extend(record.ptrs.iter().rev().cloned());
                    Ok(record)
                }
                Err(e) => {
                    self.queue.clear();
                    Err(e)
                }
            });
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pos_dispatch_matches_the_reference_switch() {
        assert_eq!(Pos::from_tag("n"), Some(Pos::Noun));
        assert_eq!(Pos::from_tag("v"), Some(Pos::Verb));
        assert_eq!(Pos::from_tag("a"), Some(Pos::Adj));
        assert_eq!(Pos::from_tag("s"), Some(Pos::Adj));
        assert_eq!(Pos::from_tag("r"), Some(Pos::Adv));
        for bad in ["N", "V", "A", "S", "R", "", "noun", "x", "nn", " n", "ن"] {
            assert_eq!(Pos::from_tag(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn pos_round_trips_through_its_tag() {
        for p in Pos::all() {
            assert_eq!(Pos::from_tag(p.tag()), Some(p));
        }
        assert_eq!(Pos::all(), [Pos::Noun, Pos::Verb, Pos::Adj, Pos::Adv]);
    }

    #[test]
    fn wordnet_is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<WordNet>();
        assert_send_sync::<IndexFile>();
        assert_send_sync::<DataFile>();
    }
}
