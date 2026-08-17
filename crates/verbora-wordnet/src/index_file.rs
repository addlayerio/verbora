//! `index.{noun,verb,adj,adv}`: the lemma → synset-offset map, and the
//! deliberately incomplete binary search that reads it.
//!
//! # The search is wrong, and that is the specification
//!
//! `index_file` bisects **byte positions**, not lines. Each probe snaps
//! backwards to the start of the enclosing line, compares that line's first
//! token against the search key, and halves the step. Because the snap-back
//! changes which line a position denotes, the invariant a binary search needs
//! does not hold, and the search terminates with `{status:'miss'}` for words that
//! are unquestionably in the file.
//!
//! How wrong, measured against the shipped WordNet 3.1 database:
//!
//! | File | Lemmas | Reported missing |
//! |---|---:|---:|
//! | `index.adv` | 4,475 | 20 |
//! | `index.verb` | 11,540 | 183 |
//! | `index.adj` | 21,499 | 117 |
//! | `index.noun` | 117,953 | 624 |
//!
//! `index.verb` misses the entire head of the file — `aah`, `abandon`, `abase`,
//! `abate`, `abbreviate` — so a correct search is not a subtle improvement, it is
//! a different program. `awful`, `safely`, `such`, `bitter` and `firm` are all
//! adverbs the reference cannot find. Reproducing this is the single highest-value
//! parity requirement in the module, and it is why [`IndexFile::find`] is written
//! as a transcription of the reference arithmetic rather than as a search.
//!
//! # The other bug
//!
//! `lookupFromFile` fills [`IndexRecord::ptr_symbol`] from `tokens[0..p_cnt)`
//! instead of `tokens[4..4+p_cnt)`, so for `node` it reports
//! `["node", "n", "8", "5", "!"]` where the real pointer symbols are
//! `["!", "@", "~", "#p", ";"]`. Only the *content* is wrong: every later field
//! is indexed off `ptrs.length`, which is still exactly `p_cnt`, so `senseCnt`,
//! `tagsenseCnt` and `synsetOffset` all come out right.

use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::numfmt::{index_at, loop_count, parse_int, parse_int_10};
use crate::source::{Source, Storage};
use crate::whitespace::{path_join, split_ws, value_lt};

/// One `index.*` file.
///
/// Read-only and `Send + Sync`: share one instance across threads.
#[derive(Debug)]
pub struct IndexFile {
    file_path: String,
    source: Source,
}

/// The result of [`IndexFile::find`], mirroring the reference's two shapes.
///
/// On a miss the reference returns `{ status: 'miss' }` with no other keys at
/// all — `key`, `line` and `tokens` are `undefined`, not `null` — which is why
/// this is an enum rather than a struct with optional fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Find {
    /// The search key matched the first token of some line.
    Hit(IndexHit),
    /// The bisection gave up. Says nothing about whether the key is in the file.
    Miss,
}

impl Find {
    /// The matched line, if this is a hit.
    #[must_use]
    pub fn hit(&self) -> Option<&IndexHit> {
        match self {
            Self::Hit(h) => Some(h),
            Self::Miss => None,
        }
    }

    /// Whether the search reported a hit.
    #[must_use]
    pub fn is_hit(&self) -> bool {
        matches!(self, Self::Hit(_))
    }

    /// The reference's `status` string: `"hit"` or `"miss"`.
    #[must_use]
    pub fn status(&self) -> &'static str {
        match self {
            Self::Hit(_) => "hit",
            Self::Miss => "miss",
        }
    }
}

/// A matched index line.
///
/// Stores only the line; `key` and `tokens` are derived, because in the
/// reference they *are* derived — `key === tokens[0]` and
/// `tokens === line.split(/\s+/)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexHit {
    line: String,
}

impl IndexHit {
    /// The full line, with the trailing `\n` removed and nothing else.
    ///
    /// A CRLF file therefore keeps its `\r`, exactly as the reference does.
    #[must_use]
    pub fn line(&self) -> &str {
        &self.line
    }

    /// `tokens[0]`, the reference's `key`.
    #[must_use]
    pub fn key(&self) -> &str {
        self.tokens().next().unwrap_or("")
    }

    /// Lazy `line.split(/\s+/)`, empty edge fields included.
    ///
    /// Every WordNet index line ends with two spaces, so the last token is
    /// always `""`; a licence-header line starts with two spaces, so the first
    /// token is `""` too.
    pub fn tokens(&self) -> impl Iterator<Item = &str> {
        split_ws(&self.line)
    }

    /// `tokens[i]`, or `None` for the reference's `undefined`.
    #[must_use]
    pub fn token(&self, i: usize) -> Option<&str> {
        self.tokens().nth(i)
    }
}

/// The record `IndexFile#lookup` builds from a hit.
///
/// Fields are `f64` and `Option<String>` because the reference's `parseInt` can
/// produce `NaN` and its array indexing can produce `undefined`; both are
/// reachable (see [`IndexFile::lookup`]).
#[derive(Debug, Clone, PartialEq)]
pub struct IndexRecord {
    /// `tokens[0]`.
    pub lemma: Option<String>,
    /// `tokens[1]`.
    pub pos: Option<String>,
    /// **`tokens[0..p_cnt)`** — the reference's off-by-four slice, kept verbatim.
    /// Its length is `p_cnt`, which is what every later field is indexed from.
    pub ptr_symbol: Vec<Option<String>>,
    /// `parseInt(tokens[p_cnt + 4], 10)`.
    pub sense_cnt: f64,
    /// `parseInt(tokens[p_cnt + 5], 10)`.
    pub tagsense_cnt: f64,
    /// `parseInt(tokens[p_cnt + 6 + i], 10)` for `i` in `0..synset_cnt`.
    pub synset_offset: Vec<f64>,
}

/// One probe of the bisection: where it looked and what it found there.
///
/// Exposed so the search can be inspected and tested without re-implementing it;
/// [`IndexFile::find`] is a fold over exactly this iterator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Probe {
    /// The byte position probed, before snapping back to a line start.
    pub position: i64,
    /// The step size in effect for this probe.
    pub adjustment: i64,
    /// The first token of the line enclosing `position`.
    pub key: String,
}

impl IndexFile {
    /// Opens `dict_dir/index.<name>`.
    ///
    /// The path is built with Node's `path.join` semantics so that
    /// [`IndexFile::file_path`] matches the reference string for inputs like
    /// `"./test_data/"`.
    ///
    /// # Errors
    ///
    /// [`Error::Io`] if the file cannot be opened.
    pub fn open(dict_dir: &Path, name: &str, storage: Storage) -> Result<Self> {
        Self::open_path(dict_dir, &format!("index.{name}"), storage)
    }

    pub(crate) fn open_path(dict_dir: &Path, file_name: &str, storage: Storage) -> Result<Self> {
        let file_path = path_join(&dict_dir.to_string_lossy(), file_name);
        let source = Source::open(Path::new(&file_path), storage)?;
        Ok(Self { file_path, source })
    }

    pub(crate) fn from_source(file_path: String, source: Source) -> Self {
        Self { file_path, source }
    }

    /// The reference's `filePath`: `path.join(dataDir, 'index.' + name)`.
    #[must_use]
    pub fn file_path(&self) -> &str {
        &self.file_path
    }

    /// The path as a [`Path`].
    #[must_use]
    pub fn path(&self) -> PathBuf {
        PathBuf::from(&self.file_path)
    }

    /// The backing byte source.
    #[must_use]
    pub fn source(&self) -> &Source {
        &self.source
    }

    /// The lazy primitive: the sequence of probes the reference would make.
    ///
    /// Yields one [`Probe`] per `readLine`, in order, and stops when the search
    /// terminates. `find` is this iterator plus a decision about the last probe,
    /// so anything that changes the search changes both.
    #[must_use]
    pub fn probes<'a>(&'a self, search_key: &'a str) -> Probes<'a> {
        // size = fileSize - 1, and pos = adjustment = ceil(size / 2).
        // `ceil` on a float, not integer division: for size = -1 (an empty file)
        // the reference produces -0, which the `pos >= size` guard then rejects.
        let size = self.source.len() as i64 - 1;
        let pos = (size as f64 / 2.0).ceil() as i64;
        Probes {
            file: self,
            search_key,
            size,
            pos,
            adjustment: pos,
            last_pos: None,
            last_key: None,
            done: false,
            scratch: Vec::new(),
            last_line: String::new(),
        }
    }

    /// `IndexFile#find`: The reference's bisection, bug for bug.
    ///
    /// # Errors
    ///
    /// [`Error::Io`], [`Error::UnterminatedLine`] (the reference hangs) or
    /// [`Error::NegativeProbe`].
    pub fn find(&self, search_key: &str) -> Result<Find> {
        let mut probes = self.probes(search_key);
        loop {
            let Some(probe) = probes.next() else {
                return Ok(Find::Miss);
            };
            if probe?.key == search_key {
                return Ok(Find::Hit(IndexHit {
                    line: probes.take_line(),
                }));
            }
        }
    }

    /// `IndexFile#lookupFromFile` / `IndexFile#lookup` — the two are aliases.
    ///
    /// Returns `None` where the reference returns `null`. Note that this does
    /// **no** normalisation: lowercasing and the `\s+` → `_` rewrite happen only
    /// in [`crate::WordNet::lookup`].
    ///
    /// # Errors
    ///
    /// As [`IndexFile::find`], plus [`Error::CountTooLarge`] if `p_cnt` or
    /// `synset_cnt` parse to something absurd.
    pub fn lookup(&self, word: &str) -> Result<Option<IndexRecord>> {
        let Find::Hit(hit) = self.find(word)? else {
            return Ok(None);
        };
        Ok(Some(parse_index_line(hit.line())?))
    }
}

/// Builds an [`IndexRecord`] from a matched line.
///
/// Split out so the parse can be unit-tested against a literal line without a
/// file, and so the WordNet-level code can reuse it.
///
/// # Errors
///
/// [`Error::CountTooLarge`] when a count field parses to an implausible value.
pub fn parse_index_line(line: &str) -> Result<IndexRecord> {
    let tokens: Vec<&str> = split_ws(line).collect();

    // `parseInt(record.tokens[3])` — no radix argument, unlike everywhere else.
    let p_cnt = parse_int(tokens.get(3).copied(), None);
    let n_ptrs = loop_count(p_cnt).ok_or(Error::CountTooLarge {
        field: "p_cnt",
        value: p_cnt,
    })?;

    // THE BUG: The reference pushes tokens[i], not tokens[4 + i]. Only the
    // length matters downstream, which is why the rest of the record is correct.
    let ptr_symbol: Vec<Option<String>> = (0..n_ptrs)
        .map(|i| tokens.get(i).map(|s| (*s).to_owned()))
        .collect();

    let synset_cnt = parse_int(tokens.get(2).copied(), None);
    let n_offsets = loop_count(synset_cnt).ok_or(Error::CountTooLarge {
        field: "synset_cnt",
        value: synset_cnt,
    })?;
    let base = ptr_symbol.len() as f64 + 6.0;
    let synset_offset: Vec<f64> = (0..n_offsets)
        .map(|i| parse_int_10(index_at(&tokens, base + i as f64)))
        .collect();

    let after = ptr_symbol.len() as f64;
    Ok(IndexRecord {
        lemma: tokens.first().map(|s| (*s).to_owned()),
        pos: tokens.get(1).map(|s| (*s).to_owned()),
        sense_cnt: parse_int_10(index_at(&tokens, after + 4.0)),
        tagsense_cnt: parse_int_10(index_at(&tokens, after + 5.0)),
        ptr_symbol,
        synset_offset,
    })
}

/// Iterator over the bisection's probes; see [`IndexFile::probes`].
#[derive(Debug)]
pub struct Probes<'a> {
    file: &'a IndexFile,
    search_key: &'a str,
    size: i64,
    pos: i64,
    adjustment: i64,
    last_pos: Option<i64>,
    last_key: Option<String>,
    done: bool,
    /// Buffer reused across probes by the positioned-read backend.
    scratch: Vec<u8>,
    /// The line read by the most recent probe, so a hit can be returned whole
    /// without reading it a second time.
    last_line: String,
}

impl Probes<'_> {
    /// The line the most recent probe read, consumed.
    fn take_line(&mut self) -> String {
        std::mem::take(&mut self.last_line)
    }
}

impl Iterator for Probes<'_> {
    type Item = Result<Probe>;

    fn next(&mut self) -> Option<Result<Probe>> {
        if self.done {
            return None;
        }
        // Termination clause 1: the probe did not move, or ran off the end.
        // `size` is fileSize - 1, so the last byte is never probed.
        if self.last_pos == Some(self.pos) || self.pos >= self.size {
            self.done = true;
            return None;
        }

        let start = match self.file.source.prev_eol(self.pos) {
            Ok(s) => s,
            Err(e) => {
                self.done = true;
                return Some(Err(e));
            }
        };
        {
            // Decode straight into `last_line`'s existing buffer instead of
            // building a fresh `String` every probe and dropping the old one:
            // ~18 probes per `index.noun` lookup otherwise means ~18 alloc/free
            // pairs for a value only the *final* probe's caller ever reads.
            let mut scratch = std::mem::take(&mut self.scratch);
            let bytes = match self.file.source.line_at(start, &mut scratch) {
                Ok(bytes) => bytes,
                Err(e) => {
                    self.done = true;
                    self.scratch = scratch;
                    return Some(Err(e));
                }
            };
            self.last_line.clear();
            // `toString('UTF-8')` on invalid bytes yields U+FFFD rather than
            // failing, so the lossy conversion is the faithful one. Valid UTF-8
            // (the overwhelming common case) is `Cow::Borrowed`, so this is a
            // single memcpy into the reused buffer, not a fresh allocation.
            self.last_line.push_str(&String::from_utf8_lossy(bytes));
            self.scratch = scratch;
        }

        let key = split_ws(&self.last_line).next().unwrap_or("").to_owned();
        let position = self.pos;
        let adjustment = self.adjustment;

        if key == self.search_key {
            // The caller reads `last_line`; stop here so a second `next()` is a
            // no-op rather than a further probe.
            self.done = true;
            return Some(Ok(Probe {
                position,
                adjustment,
                key,
            }));
        }
        // Termination clause 2: the step has shrunk to one, or the probe landed
        // on the same line as last time.
        if adjustment == 1 || self.last_key.as_deref() == Some(key.as_str()) {
            self.done = true;
            return Some(Ok(Probe {
                position,
                adjustment,
                key,
            }));
        }

        // ceil(adjustment * 0.5) on a float — for an integer that is (a + 1) / 2,
        // NOT a / 2. Halving with integer division walks a different sequence of
        // positions and therefore finds a different set of words.
        self.adjustment = (adjustment + 1) / 2;
        self.last_pos = Some(position);
        self.pos = if value_lt(&key, self.search_key) {
            position + self.adjustment
        } else {
            position - self.adjustment
        };
        // One clone instead of two: `key` moves into the returned `Probe` and
        // `last_key` gets the copy, rather than cloning from `key` for both.
        self.last_key = Some(key.clone());
        Some(Ok(Probe {
            position,
            adjustment,
            key,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NODE_LINE: &str = "node n 8 5 ! @ ~ #p ; 8 0 13934060 13918679 13174985 08515608 08515452 05437672 05272412 03832647  ";

    #[test]
    fn index_record_reproduces_the_ptr_symbol_bug() {
        let r = parse_index_line(NODE_LINE).unwrap();
        assert_eq!(r.lemma.as_deref(), Some("node"));
        assert_eq!(r.pos.as_deref(), Some("n"));
        // What a reading of the WordNet file format would have produced:
        //   ["!", "@", "~", "#p", ";"]
        let syms: Vec<&str> = r
            .ptr_symbol
            .iter()
            .map(|s| s.as_deref().unwrap_or_default())
            .collect();
        assert_eq!(syms, ["node", "n", "8", "5", "!"]);
        assert_eq!(r.sense_cnt, 8.0);
        assert_eq!(r.tagsense_cnt, 0.0);
        assert_eq!(
            r.synset_offset,
            [
                13_934_060.0,
                13_918_679.0,
                13_174_985.0,
                8_515_608.0,
                8_515_452.0,
                5_437_672.0,
                5_272_412.0,
                3_832_647.0
            ]
        );
    }

    #[test]
    fn a_licence_header_line_parses_to_nan_counts() {
        // `find('')` hits one of these, and every parseInt then yields NaN,
        // which makes both loops run zero times rather than error.
        let r = parse_index_line(
            "  20 ABILITY OR FITNESS FOR ANY PARTICULAR PURPOSE OR THAT THE USE  ",
        )
        .unwrap();
        assert_eq!(r.lemma.as_deref(), Some(""));
        assert_eq!(r.pos.as_deref(), Some("20"));
        assert!(r.ptr_symbol.is_empty());
        assert!(r.sense_cnt.is_nan());
        assert!(r.tagsense_cnt.is_nan());
        assert!(r.synset_offset.is_empty());
    }

    #[test]
    fn a_short_line_yields_undefined_fields_not_a_panic() {
        let r = parse_index_line("").unwrap();
        assert_eq!(r.lemma.as_deref(), Some(""));
        assert_eq!(r.pos, None);
        assert!(r.sense_cnt.is_nan());
    }

    #[test]
    fn ceil_halving_is_not_integer_division() {
        // The reference's Math.ceil(adjustment * 0.5). Getting this wrong walks
        // a different probe sequence and finds a different set of words.
        for (a, want) in [(5, 3), (4, 2), (3, 2), (2, 1), (1, 1)] {
            assert_eq!((a + 1) / 2, want, "adjustment {a}");
            assert_eq!(((a as f64) * 0.5).ceil() as i64, want);
        }
    }
}
