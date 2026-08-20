//! `index.{noun,verb,adj,adv}`: the lemma-to-synset map, and the search that
//! reads it.

use std::borrow::Cow;
use std::cmp::Ordering;
use std::path::{Path, PathBuf};

use crate::error::{Error, RecordError, Result};
use crate::parse;
use crate::pointer::PointerSymbol;
use crate::pos::PartOfSpeech;
use crate::sense::SenseNumber;
use crate::source::{Source, Storage};
use crate::synset::SynsetOffset;

/// Converts a word into the form an `index.*` file is keyed on.
///
/// `wndb(5WN)` states the key alphabet exactly: an index lemma is "lower case
/// ASCII text of word or collocation", and "collocations are formed by joining
/// individual words with an underscore (`_`) character". The transform that
/// follows from that, and the one this function applies, is:
///
/// 1. leading and trailing whitespace is removed;
/// 2. every remaining run of whitespace becomes a single `_`;
/// 3. ASCII letters are lowercased.
///
/// Nothing else changes. In particular **non-ASCII characters are left exactly
/// as they are**, and are not case-folded, transliterated or stripped. A word
/// containing one therefore cannot match, because no index lemma contains one;
/// silently mangling it into something that *does* match would be a guess, not
/// a lookup.
///
/// The transform is idempotent, and it is the identity on every string that is
/// already a legal index key — which is what makes every lemma in a dictionary
/// reachable through it. `verbora-wordnet`'s own test suite enumerates every
/// entry of every index file it is given and asserts exactly that.
///
/// Every entry point that takes a *word* applies this function; every entry
/// point that takes a *key* does not. See the crate-level "Choosing the right
/// API" section.
///
/// ```
/// use verbora_wordnet::index_key;
///
/// assert_eq!(index_key("entity"), "entity");
/// assert_eq!(index_key("New York"), "new_york");
/// assert_eq!(index_key("new  york"), "new_york");
/// assert_eq!(index_key("  entity  "), "entity");
/// assert_eq!(index_key("U.S.A."), "u.s.a.");
///
/// // Left alone, and therefore simply absent from an ASCII index.
/// assert_eq!(index_key("CAFÉ"), "cafÉ");
/// ```
#[must_use]
pub fn index_key(word: &str) -> Cow<'_, str> {
    let trimmed = word.trim();
    let needs_work = trimmed
        .chars()
        .any(|c| c.is_whitespace() || c.is_ascii_uppercase());
    if !needs_work {
        return Cow::Borrowed(trimmed);
    }

    let mut out = String::with_capacity(trimmed.len());
    let mut in_run = false;
    for c in trimmed.chars() {
        if c.is_whitespace() {
            if !in_run {
                out.push('_');
                in_run = true;
            }
        } else {
            in_run = false;
            out.push(c.to_ascii_lowercase());
        }
    }
    Cow::Owned(out)
}

/// One line of an `index.*` file: a lemma and the synsets that contain it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct IndexEntry {
    /// The lemma, exactly as the file spells it.
    pub lemma: String,
    /// The category, which is the same for every entry in one file.
    pub pos: PartOfSpeech,
    /// `ptr_symbol`: the relations that appear on some sense of this lemma, in
    /// file order.
    ///
    /// `wndb(5WN)` describes these as the `p_cnt` "different types of pointers"
    /// the lemma's senses use. Whether a particular file repeats one is a
    /// property of that file, not something this crate enforces or removes.
    pub pointer_symbols: Vec<PointerSymbol>,
    /// `tagsense_cnt`: how many of the senses below are semantically tagged in
    /// the WordNet sense-tagged corpus. They are the first `tagged_sense_count`
    /// entries, because `wndb(5WN)` orders senses by decreasing tag frequency.
    pub tagged_sense_count: u32,
    /// The synset offsets, in sense order: entry `0` is sense 1.
    pub synset_offsets: Vec<SynsetOffset>,
}

impl IndexEntry {
    /// How many senses this lemma has in this part of speech.
    #[must_use]
    pub fn sense_count(&self) -> usize {
        self.synset_offsets.len()
    }

    /// The offset of one numbered sense, or `None` if the lemma has fewer.
    ///
    /// Sense numbers are 1-based and run in the order the index line lists
    /// them, which `wndb(5WN)` defines as most-frequently-tagged first.
    #[must_use]
    pub fn offset_for_sense(&self, sense: SenseNumber) -> Option<SynsetOffset> {
        self.synset_offsets.get(sense.as_usize() - 1).copied()
    }
}

/// One `index.*` file.
///
/// Read-only after construction and `Send + Sync`: share one instance across
/// threads and query it concurrently.
#[derive(Debug)]
pub struct IndexFile {
    path: PathBuf,
    pos: PartOfSpeech,
    source: Source,
}

impl IndexFile {
    /// Opens one index file directly.
    ///
    /// `pos` must match the file: it is recorded on every [`IndexEntry`] the
    /// file yields, and a mismatch is reported as a malformed entry rather than
    /// silently relabelling the data.
    ///
    /// # Errors
    ///
    /// [`Error::Io`] if the file cannot be opened or read;
    /// [`Error::FileTooLarge`] for any storage but [`Storage::Pread`] on a file
    /// of 4 GiB or more.
    pub fn open(path: impl AsRef<Path>, pos: PartOfSpeech, storage: Storage) -> Result<Self> {
        let path = path.as_ref();
        Ok(Self {
            path: path.to_path_buf(),
            pos,
            source: Source::open(path, storage)?,
        })
    }

    pub(crate) fn from_source(path: PathBuf, pos: PartOfSpeech, source: Source) -> Self {
        Self { path, pos, source }
    }

    /// The file this was opened from.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The category this file indexes.
    #[must_use]
    pub fn part_of_speech(&self) -> PartOfSpeech {
        self.pos
    }

    /// The file's size in bytes, as recorded when it was opened.
    #[must_use]
    pub fn len_bytes(&self) -> u64 {
        self.source.len()
    }

    /// Looks up an already-normalised key.
    ///
    /// The argument is used verbatim: no lowercasing, no whitespace rewriting,
    /// no trimming. Pass a key you produced with [`index_key`], or one you took
    /// out of an [`IndexEntry`]. For a word typed by a user, prefer
    /// [`WordNet::lookup`](crate::WordNet::lookup), which applies [`index_key`]
    /// first.
    ///
    /// An empty key answers `None` without touching the file: an index lemma is
    /// never empty, and the copyright header lines that `wndb(5WN)` requires at
    /// the top of every index file are exactly the lines whose first field is
    /// empty.
    ///
    /// # Errors
    ///
    /// [`Error::Io`] if the file cannot be read, or
    /// [`Error::MalformedIndexEntry`] if the matched line does not follow
    /// `wndb(5WN)`'s format.
    pub fn entry(&self, key: &str) -> Result<Option<IndexEntry>> {
        let Some((line_start, line)) = self.find_line(key)? else {
            return Ok(None);
        };
        parse_index_line(&line, self.pos)
            .map(Some)
            .map_err(|kind| Error::index(&self.path, line_start, kind))
    }

    /// Every entry in the file, in file order, skipping the copyright header.
    ///
    /// This is a full sequential scan, not a search: it exists for building
    /// derived structures and for auditing a dictionary, and costs one pass
    /// over the file. Use [`IndexFile::entry`] to look one lemma up.
    #[must_use]
    pub fn entries(&self) -> Entries<'_> {
        Entries {
            file: self,
            at: 0,
            scratch: Vec::new(),
            done: false,
        }
    }

    /// Binary search over the file's lines for the one whose first field is
    /// `key`, returning the line's start offset and its text.
    ///
    /// `wndb(5WN)` specifies that index files are sorted so that they can be
    /// binary searched, and that the copyright header lines "all begin with two
    /// spaces … so they do not interfere with the binary search algorithm" —
    /// their first field is empty, which sorts below every real lemma.
    /// Comparison is therefore byte-wise over the first field, matching the
    /// ASCII collating sequence the files are sorted in.
    ///
    /// The invariant is: *if a line with this key exists, its first byte lies
    /// in `[lo, hi)`, and `lo` is itself the first byte of a line.* Each
    /// iteration snaps the midpoint back to the start of the line containing
    /// it, which is at or after `lo` by the invariant, and then either advances
    /// `lo` past that whole line or pulls `hi` down to its start. Both strictly
    /// shrink the window, so the loop terminates, and neither can step over a
    /// line without comparing it.
    fn find_line(&self, key: &str) -> Result<Option<(u64, String)>> {
        if key.is_empty() {
            return Ok(None);
        }
        let mut scratch = Vec::new();
        let mut lo = 0u64;
        let mut hi = self.source.len();

        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            // `line_start` answers within `[lo, mid]` for any table this crate
            // built. Clamping restores that invariant even for a sidecar whose
            // offsets do not describe this file, so a corrupt one produces a
            // miss rather than a loop that never narrows.
            let start = self.source.line_start(mid)?.clamp(lo, mid);
            let probe = self
                .source
                .with_line(start, &mut scratch, |line| probe(line, key))?;
            let Some(probe) = probe else { break };
            match probe {
                Probe::Found(text) => return Ok(Some((start, text))),
                Probe::TooSmall { next } => lo = next,
                Probe::TooLarge => {
                    if start > lo {
                        hi = start;
                    } else {
                        // The first line of the window is already past the key,
                        // so no line in the window can carry it.
                        return Ok(None);
                    }
                }
            }
        }
        Ok(None)
    }
}

/// What one probe of the binary search found.
enum Probe {
    /// The line's first field is the search key; here is the whole line.
    Found(String),
    /// The line sorts below the key; the search continues at `next`.
    TooSmall { next: u64 },
    /// The line sorts above the key.
    TooLarge,
}

fn probe(line: crate::source::Line<'_>, key: &str) -> Probe {
    // The lemma is the first space-delimited field. Comparing the raw bytes
    // rather than a decoded string keeps the ordering identical to the one the
    // file is sorted in even if a byte is not valid UTF-8.
    let field = match memchr::memchr(b' ', line.bytes) {
        Some(i) => &line.bytes[..i],
        None => line.bytes,
    };
    match field.cmp(key.as_bytes()) {
        Ordering::Equal => Probe::Found(String::from_utf8_lossy(line.bytes).into_owned()),
        Ordering::Less => Probe::TooSmall { next: line.next },
        Ordering::Greater => Probe::TooLarge,
    }
}

/// Iterator returned by [`IndexFile::entries`].
#[derive(Debug)]
pub struct Entries<'a> {
    file: &'a IndexFile,
    at: u64,
    scratch: Vec<u8>,
    done: bool,
}

impl Iterator for Entries<'_> {
    type Item = Result<IndexEntry>;

    fn next(&mut self) -> Option<Result<IndexEntry>> {
        while !self.done {
            let start = self.at;
            let mut scratch = std::mem::take(&mut self.scratch);
            let read = self.file.source.with_line(start, &mut scratch, |line| {
                (
                    is_header(line.bytes),
                    String::from_utf8_lossy(line.bytes).into_owned(),
                    line.next,
                )
            });
            self.scratch = scratch;
            match read {
                Err(e) => {
                    self.done = true;
                    return Some(Err(e));
                }
                Ok(None) => {
                    self.done = true;
                    return None;
                }
                Ok(Some((header, text, next))) => {
                    self.at = next;
                    if header || text.is_empty() {
                        continue;
                    }
                    return Some(
                        parse_index_line(&text, self.file.pos)
                            .map_err(|kind| Error::index(&self.file.path, start, kind)),
                    );
                }
            }
        }
        None
    }
}

impl std::iter::FusedIterator for Entries<'_> {}

/// `wndb(5WN)`: every copyright-header line begins with two spaces.
fn is_header(line: &[u8]) -> bool {
    line.starts_with(b"  ")
}

/// Parses one `index.*` line.
///
/// The layout is `wndb(5WN)`'s:
///
/// ```text
/// lemma  pos  synset_cnt  p_cnt  [ptr_symbol...]  sense_cnt  tagsense_cnt
///     synset_offset  [synset_offset...]
/// ```
///
/// `sense_cnt` is documented as "same as `synset_cnt` … retained for
/// compatibility", so a line where the two disagree is malformed: believing
/// either one would silently drop or invent senses.
pub(crate) fn parse_index_line(
    line: &str,
    pos: PartOfSpeech,
) -> std::result::Result<IndexEntry, RecordError> {
    let mut tokens = line.split_ascii_whitespace();

    let lemma = parse::required(tokens.next(), "lemma")?;
    let pos_token = parse::required(tokens.next(), "pos")?;
    let found_pos = PartOfSpeech::from_tag(pos_token).ok_or(RecordError::InvalidField {
        field: "pos",
        value: pos_token.to_owned(),
    })?;
    if found_pos != pos {
        return Err(RecordError::InvalidField {
            field: "pos",
            value: pos_token.to_owned(),
        });
    }

    let synset_cnt =
        parse::decimal_u32("synset_cnt", parse::required(tokens.next(), "synset_cnt")?)?;
    let p_cnt = parse::decimal_u32("p_cnt", parse::required(tokens.next(), "p_cnt")?)?;

    let mut pointer_symbols = Vec::with_capacity(p_cnt.min(64) as usize);
    for _ in 0..p_cnt {
        let token = parse::required(tokens.next(), "ptr_symbol")?;
        pointer_symbols.push(PointerSymbol::from_symbol(token).ok_or(
            RecordError::InvalidField {
                field: "ptr_symbol",
                value: token.to_owned(),
            },
        )?);
    }

    let sense_cnt = parse::decimal_u32("sense_cnt", parse::required(tokens.next(), "sense_cnt")?)?;
    if sense_cnt != synset_cnt {
        return Err(RecordError::SenseCountMismatch {
            synset_cnt,
            sense_cnt,
        });
    }
    let tagged_sense_count = parse::decimal_u32(
        "tagsense_cnt",
        parse::required(tokens.next(), "tagsense_cnt")?,
    )?;

    let mut synset_offsets = Vec::with_capacity(synset_cnt.min(1024) as usize);
    for _ in 0..synset_cnt {
        synset_offsets.push(parse::offset(
            "synset_offset",
            parse::required(tokens.next(), "synset_offset")?,
        )?);
    }

    Ok(IndexEntry {
        lemma: lemma.to_owned(),
        pos,
        pointer_symbols,
        tagged_sense_count,
        synset_offsets,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A real `index.noun` line, transcribed from WordNet 3.1. Every expected
    /// value below is read off the line by applying `wndb(5WN)`'s field layout
    /// by hand: `node n 8 5 ! @ ~ #p ; …` is lemma `node`, category `n`,
    /// eight synsets, five pointer symbols, then the redundant sense count,
    /// the tagged-sense count and eight offsets.
    const NODE_LINE: &str = "node n 8 5 ! @ ~ #p ;c 8 0 13934060 13918679 13174985 08515608 08515452 05437672 05272412 03832647  ";

    #[test]
    fn parses_a_real_index_line_field_by_field() {
        let e = parse_index_line(NODE_LINE, PartOfSpeech::Noun).unwrap();
        assert_eq!(e.lemma, "node");
        assert_eq!(e.pos, PartOfSpeech::Noun);
        assert_eq!(
            e.pointer_symbols,
            [
                PointerSymbol::Antonym,
                PointerSymbol::Hypernym,
                PointerSymbol::Hyponym,
                PointerSymbol::PartHolonym,
                PointerSymbol::DomainOfTopic,
            ]
        );
        assert_eq!(e.tagged_sense_count, 0);
        assert_eq!(e.sense_count(), 8);
        assert_eq!(
            e.synset_offsets.first().copied(),
            Some(SynsetOffset::new(13_934_060))
        );
        assert_eq!(
            e.synset_offsets.last().copied(),
            Some(SynsetOffset::new(3_832_647))
        );
        // Sense 1 is the first offset on the line, not the last.
        let sense = |n: u16| SenseNumber::from_u16(n).unwrap();
        assert_eq!(
            e.offset_for_sense(sense(1)),
            Some(SynsetOffset::new(13_934_060))
        );
        assert_eq!(
            e.offset_for_sense(sense(8)),
            Some(SynsetOffset::new(3_832_647))
        );
        assert_eq!(e.offset_for_sense(sense(9)), None);
    }

    #[test]
    fn a_copyright_header_line_is_recognised_and_never_parsed_as_an_entry() {
        let header = "  20 ABILITY OR FITNESS FOR ANY PARTICULAR PURPOSE OR THAT THE USE  ";
        assert!(is_header(header.as_bytes()));
        assert!(!is_header(NODE_LINE.as_bytes()));
        // Its first field is empty, which is what keeps it below every lemma.
        assert_eq!(header.split(' ').next(), Some(""));
    }

    #[test]
    fn a_disagreeing_redundant_sense_count_is_refused() {
        let line = "node n 8 0 7 0 1 2 3 4 5 6 7 8  ";
        assert_eq!(
            parse_index_line(line, PartOfSpeech::Noun).unwrap_err(),
            RecordError::SenseCountMismatch {
                synset_cnt: 8,
                sense_cnt: 7
            }
        );
    }

    #[test]
    fn a_line_from_the_wrong_file_is_refused() {
        assert!(matches!(
            parse_index_line(NODE_LINE, PartOfSpeech::Verb).unwrap_err(),
            RecordError::InvalidField { field: "pos", .. }
        ));
    }

    #[test]
    fn a_truncated_line_names_the_missing_field() {
        let cases: [(&str, RecordError); 6] = [
            ("", RecordError::MissingField { field: "lemma" }),
            ("node", RecordError::MissingField { field: "pos" }),
            (
                "node n",
                RecordError::MissingField {
                    field: "synset_cnt",
                },
            ),
            ("node n 1", RecordError::MissingField { field: "p_cnt" }),
            (
                "node n 1 0",
                RecordError::MissingField { field: "sense_cnt" },
            ),
            (
                "node n 1 0 1",
                RecordError::MissingField {
                    field: "tagsense_cnt",
                },
            ),
        ];
        for (line, want) in cases {
            assert_eq!(
                parse_index_line(line, PartOfSpeech::Noun).unwrap_err(),
                want,
                "{line:?}"
            );
        }
        assert_eq!(
            parse_index_line("node n 2 0 2 0 00000001", PartOfSpeech::Noun).unwrap_err(),
            RecordError::MissingField {
                field: "synset_offset"
            }
        );
    }

    #[test]
    fn a_header_line_does_not_parse_as_an_entry() {
        let header = "  20 ABILITY OR FITNESS FOR ANY PARTICULAR PURPOSE OR THAT THE USE  ";
        assert!(parse_index_line(header, PartOfSpeech::Noun).is_err());
    }

    #[test]
    fn index_key_is_the_identity_on_every_legal_key_character() {
        // `wndb(5WN)`'s lemma alphabet: lower case ASCII plus the punctuation
        // WordNet 3.1 actually uses in lemmas. Enumerated, not sampled.
        let alphabet: Vec<char> = ('a'..='z')
            .chain('0'..='9')
            .chain(['_', '-', '.', '\'', '/'])
            .collect();
        assert_eq!(alphabet.len(), 41);
        for &c in &alphabet {
            let one = c.to_string();
            assert_eq!(index_key(&one), one, "{c:?}");
            for &d in &alphabet {
                let two: String = [c, d].iter().collect();
                assert_eq!(index_key(&two), two, "{two:?}");
            }
        }
    }

    #[test]
    fn index_key_lowercases_ascii_and_joins_words_with_one_underscore() {
        assert_eq!(index_key(""), "");
        assert_eq!(index_key("entity"), "entity");
        assert_eq!(index_key("ENTITY"), "entity");
        assert_eq!(index_key("EnTiTy"), "entity");
        assert_eq!(index_key("New York"), "new_york");
        assert_eq!(index_key("new  york"), "new_york");
        assert_eq!(index_key("NEW\tYORK"), "new_york");
        assert_eq!(index_key("  entity  "), "entity");
        assert_eq!(index_key("\t\n\r"), "");
        assert_eq!(index_key("a b c"), "a_b_c");
        // Non-ASCII is left exactly as it is.
        assert_eq!(index_key("café"), "café");
        assert_eq!(index_key("Москва"), "Москва");
        assert_eq!(index_key("日本語"), "日本語");
        assert_eq!(index_key("😀"), "😀");
        assert_eq!(index_key("İSTANBUL"), "İstanbul");
    }

    #[test]
    fn index_key_is_idempotent() {
        for word in [
            "",
            " ",
            "entity",
            "ENTITY",
            "  New   York  ",
            "CAFÉ",
            "日本 語",
            "😀 😀",
            "a\u{00A0}b",
            "İSTANBUL",
        ] {
            let once = index_key(word).into_owned();
            assert_eq!(index_key(&once), once, "{word:?}");
        }
    }

    #[test]
    fn index_key_borrows_when_there_is_nothing_to_do() {
        assert!(matches!(index_key("entity"), Cow::Borrowed(_)));
        assert!(matches!(index_key("new_york"), Cow::Borrowed(_)));
        assert!(matches!(index_key("café"), Cow::Borrowed(_)));
        assert!(matches!(index_key("Entity"), Cow::Owned(_)));
        assert!(matches!(index_key("new york"), Cow::Owned(_)));
    }
}
