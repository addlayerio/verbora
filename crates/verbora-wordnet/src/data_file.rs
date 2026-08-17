//! `data.{noun,verb,adj,adv}`: the synsets themselves, addressed by raw byte
//! offset, and the string surgery the reference performs on each one.
//!
//! # Four things a careful reading of the format gets wrong
//!
//! 1. **`wCnt` is hexadecimal.** `parseInt(tokens[3], 16)` — the only field in
//!    the whole format that is not base 10. `'0b'` is 11. A `str::parse` gives
//!    an error (or, worse, `0` after a `trim_start_matches('0')`), silently
//!    truncating the synonyms list and shifting the pointer offset for every
//!    synset with ten or more words.
//!
//! 2. **The gloss is split on `'| '` everywhere it occurs**, and only the first
//!    two pieces are used. `splitn(2, "| ")` would keep a gloss containing `'| '`
//!    intact and diverge. In the shipped database no line contains `'| '` twice,
//!    so this is latent — but a line with *no* `'| '` at all (every licence-header
//!    line) is reachable, and the reference throws there rather than defaulting.
//!
//! 3. **Examples are split on `'; '` with no awareness of quoting**, so a
//!    definition containing a semicolon is truncated and its tail becomes a bogus
//!    example. `data.verb` offset 4032 is the canonical case: the gloss
//!    `heave or utter a sigh; breathe deeply and heavily; "She sighed sadly"`
//!    yields the definition `heave or utter a sigh` and *two* examples.
//!
//! 4. **Example cleanup deletes whitespace runs rather than collapsing them.**
//!    `.replace(/\s\s+/g, '')` joins the words on either side. A single leading
//!    space survives, because one space is not a run. Meanwhile `def` and `gloss`
//!    are not cleaned at all and keep the record's two trailing spaces.
//!
//! Offsets are unvalidated byte positions. A mid-record offset produces a
//! well-formed record full of `NaN`s and `undefined`s rather than an error — see
//! [`DataRecord`].

use std::borrow::Cow;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::numfmt::{index_at, loop_count, parse_int_10, parse_int_16};
use crate::source::{Source, Storage};
use crate::whitespace::{clean_example, path_join, split_ws};

/// One `data.*` file.
///
/// Read-only and `Send + Sync`: share one instance across threads.
#[derive(Debug)]
pub struct DataFile {
    file_path: String,
    source: Source,
}

/// A relational pointer from one synset to another.
///
/// `synset_offset` is `f64` and the string fields are `Option` because a
/// mid-record offset can make the reference read past the end of the token list,
/// where the reference yields `undefined` and `NaN` instead of failing.
#[derive(Debug, Clone, PartialEq)]
pub struct Pointer {
    /// The relation symbol: `@` hypernym, `~` hyponym, `!` antonym, and so on.
    /// See [`crate::pointer`] for the catalogue.
    pub pointer_symbol: Option<String>,
    /// Byte offset of the target synset in the data file for `pos`.
    pub synset_offset: f64,
    /// Part-of-speech tag of the *target*: `n`, `v`, `a`, `s` or `r`.
    pub pos: Option<String>,
    /// Four hexadecimal digits locating the source and target words within their
    /// synsets. Left as the raw string; the reference never parses it.
    pub source_target: Option<String>,
}

/// A synset, in the exact shape `DataFile#get` delivers.
///
/// # Why so many `Option`s and `f64`s
///
/// The reference performs no validation whatsoever on the offset it is given.
/// `get(1750, 'n')` — ten bytes into a real record — returns, quite happily:
///
/// ```text
/// { synsetOffset: 3, lexFilenum: NaN, pos: '01', wCnt: 14, lemma: '0',
///   synonyms: ['0','~','n','~','n','~','n','',undefined,undefined,…],
///   lexId: '003', ptrs: [], gloss: 'that which is perceived…' }
/// ```
///
/// Modelling those fields as `String` and `u32` would turn a value the reference
/// returns into an error. `f64` also carries `NaN` exactly, which matters because
/// `for (i = 0; i < NaN; i++)` runs zero times — the reason `synonyms` above is
/// truncated rather than absurd.
#[derive(Debug, Clone, PartialEq)]
pub struct DataRecord {
    /// `parseInt(tokens[0], 10)` — the record's own offset, as written in the
    /// file, which for a mid-line read is not the offset you asked for.
    pub synset_offset: f64,
    /// `parseInt(tokens[1], 10)`, the lexicographer file number.
    pub lex_filenum: f64,
    /// `tokens[2]`: `n`, `v`, `a`, `s` or `r`.
    pub pos: Option<String>,
    /// `parseInt(tokens[3], 16)` — **hexadecimal**.
    pub w_cnt: f64,
    /// `tokens[4]`: the first word only, adjective markers included (`awake(p)`).
    pub lemma: Option<String>,
    /// `tokens[4 + 2i]` for `i` in `0..w_cnt`; `None` where the reference read
    /// past the end of the line and got `undefined`.
    pub synonyms: Vec<Option<String>>,
    /// `tokens[5]`: the lex_id of the *first* word only, never parsed.
    pub lex_id: Option<String>,
    /// Relational pointers, in file order.
    pub ptrs: Vec<Pointer>,
    /// Everything after the first `'| '`, verbatim — trailing spaces included.
    pub gloss: String,
    /// `gloss.split('; ')[0]`. Not cleaned: keeps quotes and trailing spaces.
    pub def: String,
    /// `gloss.split('; ').slice(1)`, each with quotes and whitespace runs
    /// removed.
    pub exp: Vec<String>,
}

/// A borrowed view of a synset: the same fields, pointing into the caller's line.
///
/// The zero-copy primitive. Every string field except the examples is a
/// subslice of the line being parsed, so reading a synset allocates only for the
/// examples that actually contain a quote or a whitespace run. Use
/// [`DataFile::with_record`] to obtain one, and [`DataRecordRef::to_owned_record`] when
/// an owned [`DataRecord`] is needed.
#[derive(Debug, Clone, PartialEq)]
pub struct DataRecordRef<'a> {
    /// See [`DataRecord::synset_offset`].
    pub synset_offset: f64,
    /// See [`DataRecord::lex_filenum`].
    pub lex_filenum: f64,
    /// See [`DataRecord::pos`].
    pub pos: Option<&'a str>,
    /// See [`DataRecord::w_cnt`].
    pub w_cnt: f64,
    /// See [`DataRecord::lemma`].
    pub lemma: Option<&'a str>,
    /// See [`DataRecord::synonyms`].
    pub synonyms: Vec<Option<&'a str>>,
    /// See [`DataRecord::lex_id`].
    pub lex_id: Option<&'a str>,
    /// See [`DataRecord::ptrs`].
    pub ptrs: Vec<PointerRef<'a>>,
    /// See [`DataRecord::gloss`].
    pub gloss: &'a str,
    /// See [`DataRecord::def`].
    pub def: &'a str,
    /// See [`DataRecord::exp`]; borrowed unless the cleanup had work to do.
    pub exp: Vec<Cow<'a, str>>,
}

/// A borrowed [`Pointer`].
#[derive(Debug, Clone, PartialEq)]
pub struct PointerRef<'a> {
    /// See [`Pointer::pointer_symbol`].
    pub pointer_symbol: Option<&'a str>,
    /// See [`Pointer::synset_offset`].
    pub synset_offset: f64,
    /// See [`Pointer::pos`].
    pub pos: Option<&'a str>,
    /// See [`Pointer::source_target`].
    pub source_target: Option<&'a str>,
}

impl PointerRef<'_> {
    /// Copies into an owned [`Pointer`].
    #[must_use]
    pub fn to_owned_pointer(&self) -> Pointer {
        Pointer {
            pointer_symbol: self.pointer_symbol.map(str::to_owned),
            synset_offset: self.synset_offset,
            pos: self.pos.map(str::to_owned),
            source_target: self.source_target.map(str::to_owned),
        }
    }
}

impl DataRecordRef<'_> {
    /// Copies into an owned [`DataRecord`].
    #[must_use]
    pub fn to_owned_record(&self) -> DataRecord {
        DataRecord {
            synset_offset: self.synset_offset,
            lex_filenum: self.lex_filenum,
            pos: self.pos.map(str::to_owned),
            w_cnt: self.w_cnt,
            lemma: self.lemma.map(str::to_owned),
            synonyms: self.synonyms.iter().map(|s| s.map(str::to_owned)).collect(),
            lex_id: self.lex_id.map(str::to_owned),
            ptrs: self.ptrs.iter().map(PointerRef::to_owned_pointer).collect(),
            gloss: self.gloss.to_owned(),
            def: self.def.to_owned(),
            exp: self.exp.iter().map(|c| c.as_ref().to_owned()).collect(),
        }
    }
}

impl DataFile {
    /// Opens `dict_dir/data.<name>`.
    ///
    /// # Errors
    ///
    /// [`Error::Io`] if the file cannot be opened.
    pub fn open(dict_dir: &Path, name: &str, storage: Storage) -> Result<Self> {
        Self::open_path(dict_dir, &format!("data.{name}"), storage)
    }

    pub(crate) fn open_path(dict_dir: &Path, file_name: &str, storage: Storage) -> Result<Self> {
        let file_path = path_join(&dict_dir.to_string_lossy(), file_name);
        let source = Source::open(Path::new(&file_path), storage)?;
        Ok(Self { file_path, source })
    }

    pub(crate) fn from_source(file_path: String, source: Source) -> Self {
        Self { file_path, source }
    }

    /// The reference's `filePath`: `path.join(dataDir, 'data.' + name)`.
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

    /// The zero-copy primitive: reads the record at `offset` and hands a
    /// borrowed view to `f`.
    ///
    /// Unlike the index files, no snapping back to a line start happens here —
    /// The reference reads *forward* from exactly the byte it is given, which is
    /// why a mid-record offset yields a garbage record instead of the enclosing
    /// one.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidOffset`] for a non-integral or negative offset (the
    /// reference throws from `fs.read`), [`Error::MissingGloss`] for a line with
    /// no `'| '`, [`Error::UnterminatedLine`] past the end of the file (the
    /// reference loops forever), or [`Error::Io`].
    pub fn with_record<R>(
        &self,
        offset: f64,
        f: impl FnOnce(&DataRecordRef<'_>) -> R,
    ) -> Result<R> {
        if !offset.is_finite() || offset < 0.0 || offset.fract() != 0.0 {
            return Err(Error::InvalidOffset {
                path: self.path(),
                offset,
            });
        }
        let at = offset as u64;
        let mut scratch = Vec::new();
        let bytes = self.source.line_at(at, &mut scratch)?;
        // `buff.slice(...).toString('UTF-8')` substitutes U+FFFD for invalid
        // bytes rather than failing, so lossy decoding is the faithful choice.
        let line = String::from_utf8_lossy(bytes);
        let record = parse_data_line(&line).map_err(|e| match e {
            ParseError::MissingGloss => Error::MissingGloss {
                path: self.path(),
                offset: at,
            },
            ParseError::CountTooLarge { field, value } => Error::CountTooLarge { field, value },
        })?;
        Ok(f(&record))
    }

    /// `DataFile#get`: the record at `offset`, owned.
    ///
    /// # Errors
    ///
    /// As [`DataFile::with_record`].
    pub fn get(&self, offset: f64) -> Result<DataRecord> {
        self.with_record(offset, |r| r.to_owned_record())
    }
}

/// Why [`parse_data_line`] refused a line.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ParseError {
    /// The line has no `'| '`; the reference throws `TypeError … reading 'split'`.
    MissingGloss,
    /// A count field parsed to an implausible value.
    CountTooLarge {
        /// `"wCnt"` or `"p_cnt"`.
        field: &'static str,
        /// The parsed value, as an integer where possible.
        value: f64,
    },
}

/// Parses one `data.*` line into a borrowed record.
///
/// Exposed so the parse can be exercised against literal lines, and so callers
/// with their own byte source can reuse it.
///
/// # Errors
///
/// [`ParseError::MissingGloss`] when the line has no `'| '` separator.
pub fn parse_data_line(line: &str) -> std::result::Result<DataRecordRef<'_>, ParseError> {
    // `line.split('| ')` splits at EVERY occurrence, not the first; only the
    // first two pieces are ever used.
    let mut pieces = line.split("| ");
    let head = pieces.next().unwrap_or("");
    let gloss = pieces.next().ok_or(ParseError::MissingGloss)?;

    let tokens: Vec<&str> = split_ws(head).collect();

    let w_cnt = parse_int_16(tokens.get(3).copied());
    let n_words = loop_count(w_cnt).ok_or(ParseError::CountTooLarge {
        field: "wCnt",
        value: w_cnt,
    })?;
    let synonyms: Vec<Option<&str>> = (0..n_words)
        .map(|i| index_at(&tokens, 4.0 + (i as f64) * 2.0))
        .collect();

    // (wCnt - 1) * 2 + 6 == 2 * wCnt + 4: the index of p_cnt. NaN propagates,
    // and `index_at` turns a NaN index into `undefined` rather than a panic.
    let ptr_offset = (w_cnt - 1.0) * 2.0 + 6.0;
    let p_cnt = parse_int_10(index_at(&tokens, ptr_offset));
    let n_ptrs = loop_count(p_cnt).ok_or(ParseError::CountTooLarge {
        field: "p_cnt",
        value: p_cnt,
    })?;
    let ptrs: Vec<PointerRef<'_>> = (0..n_ptrs)
        .map(|i| {
            let base = ptr_offset + (i as f64) * 4.0;
            PointerRef {
                pointer_symbol: index_at(&tokens, base + 1.0),
                synset_offset: parse_int_10(index_at(&tokens, base + 2.0)),
                pos: index_at(&tokens, base + 3.0),
                source_target: index_at(&tokens, base + 4.0),
            }
        })
        .collect();

    // The gloss is split on '; ' with no quote awareness, so a definition that
    // legitimately contains a semicolon is truncated into "examples".
    let mut gloss_parts = gloss.split("; ");
    let def = gloss_parts.next().unwrap_or("");
    let exp: Vec<Cow<'_, str>> = gloss_parts.map(clean_example).collect();

    Ok(DataRecordRef {
        synset_offset: parse_int_10(tokens.first().copied()),
        lex_filenum: parse_int_10(tokens.get(1).copied()),
        pos: tokens.get(2).copied(),
        w_cnt,
        lemma: tokens.get(4).copied(),
        synonyms,
        lex_id: tokens.get(5).copied(),
        ptrs,
        gloss,
        def,
        exp,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const NODE: &str = "03832647 06 n 03 node 0 client 0 guest 0 003 @ 03086983 n 0000 #p 03089375 n 0000 ;c 06138021 n 0000 | (computer science) any computer that is hooked up to a computer network  ";

    #[test]
    fn parses_a_real_synset() {
        let r = parse_data_line(NODE).unwrap();
        assert_eq!(r.synset_offset, 3_832_647.0);
        assert_eq!(r.lex_filenum, 6.0);
        assert_eq!(r.pos, Some("n"));
        assert_eq!(r.w_cnt, 3.0);
        assert_eq!(r.lemma, Some("node"));
        assert_eq!(r.synonyms, [Some("node"), Some("client"), Some("guest")]);
        assert_eq!(r.lex_id, Some("0"));
        assert_eq!(r.ptrs.len(), 3);
        assert_eq!(r.ptrs[0].pointer_symbol, Some("@"));
        assert_eq!(r.ptrs[0].synset_offset, 3_086_983.0);
        assert_eq!(r.ptrs[2].pointer_symbol, Some(";c"));
        assert_eq!(r.ptrs[2].source_target, Some("0000"));
        // `def` is NOT cleaned: the record's two trailing spaces survive.
        assert!(r.def.ends_with("network  "));
        assert_eq!(r.def, r.gloss);
        assert!(r.exp.is_empty());
    }

    #[test]
    fn word_count_is_read_as_hexadecimal() {
        // 0x0b = 11 words. Base 10 would give 0 and an empty synonyms list.
        let line = "00000001 06 n 0b a 0 b 0 c 0 d 0 e 0 f 0 g 0 h 0 i 0 j 0 k 0 000 | g  ";
        let r = parse_data_line(line).unwrap();
        assert_eq!(r.w_cnt, 11.0);
        assert_eq!(r.synonyms.len(), 11);
        assert_eq!(r.synonyms[10], Some("k"));
    }

    #[test]
    fn a_semicolon_in_the_definition_truncates_it() {
        let line = "00004032 09 v 02 sigh 0 suspire 0 000 | heave or utter a sigh; breathe deeply and heavily; \"She sighed sadly\"  ";
        let r = parse_data_line(line).unwrap();
        assert_eq!(r.def, "heave or utter a sigh");
        assert_eq!(r.exp, ["breathe deeply and heavily", "She sighed sadly"]);
        // The gloss keeps everything, quotes and trailing spaces included.
        assert!(r.gloss.contains('"'));
    }

    #[test]
    fn examples_lose_whitespace_runs_but_keep_a_single_leading_space() {
        let line = "00000001 06 n 01 x 0 000 | d; \"a b\";  \"c  d\"  ";
        let r = parse_data_line(line).unwrap();
        assert_eq!(r.def, "d");
        assert_eq!(r.exp[0], "a b");
        // Leading single space kept; the internal double space deleted.
        assert_eq!(r.exp[1], " cd");
    }

    #[test]
    fn a_line_without_a_gloss_separator_is_refused() {
        assert_eq!(
            parse_data_line("  1 no separator here  ").unwrap_err(),
            ParseError::MissingGloss
        );
    }

    #[test]
    fn the_gloss_splits_at_every_separator_not_just_the_first() {
        let r = parse_data_line("00000300 03 a 01 pipes 0 000 | first| second| third  ").unwrap();
        // splitn(2, "| ") would have produced "first| second| third".
        assert_eq!(r.gloss, "first");
        assert_eq!(r.def, "first");
    }

    #[test]
    fn a_mid_record_offset_yields_nan_and_undefined_not_an_error() {
        // Ten bytes into the node record: the fields shift and stop making sense.
        let r = parse_data_line(&NODE[10..]).unwrap();
        assert!(r.lex_filenum.is_nan() || r.lex_filenum.is_finite());
        // What matters is that it parses at all rather than erroring.
        assert!(r.gloss.contains("computer"));
    }

    #[test]
    fn a_short_line_produces_undefined_entries_rather_than_panicking() {
        let r = parse_data_line("1 2 n 05 a | g  ").unwrap();
        assert_eq!(r.w_cnt, 5.0);
        assert_eq!(r.synonyms.len(), 5);
        assert_eq!(r.synonyms[0], Some("a"));
        assert_eq!(r.synonyms[4], None);
    }

    #[test]
    fn non_ascii_and_astral_glosses_survive() {
        for gloss in ["café", "Москва", "日本語", "😀 emoji", "Ελλάδα"] {
            let line = format!("00000001 06 n 01 w 0 000 | {gloss}  ");
            let r = parse_data_line(&line).unwrap();
            assert_eq!(r.def, format!("{gloss}  "));
        }
    }
}
