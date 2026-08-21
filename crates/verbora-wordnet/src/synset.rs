//! Synsets: the records of a `data.*` file.

use std::borrow::Cow;
use std::fmt;

use crate::error::RecordError;
use crate::parse;
use crate::pointer::{Pointer, PointerSymbol};
use crate::pos::{PartOfSpeech, SynsetType};

/// The byte position of a synset within its `data.*` file.
///
/// WordNet addresses synsets by where they start in the file, not by a serial
/// number, so an offset is only meaningful together with a part of speech —
/// offset `1740` names a different synset in `data.noun` and in `data.verb`.
/// Every API that takes one therefore takes the category as well.
///
/// The `Display` form is the eight-digit, zero-filled decimal spelling
/// `wndb(5WN)` uses in the files themselves, so an offset printed by this crate
/// can be searched for verbatim.
///
/// ```
/// use verbora_wordnet::SynsetOffset;
///
/// let off = SynsetOffset::new(3_832_647);
/// assert_eq!(off.get(), 3_832_647);
/// assert_eq!(off.to_string(), "03832647");
/// assert_eq!(SynsetOffset::new(0).to_string(), "00000000");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SynsetOffset(u32);

impl SynsetOffset {
    /// An offset at `byte_offset` bytes into a data file.
    #[must_use]
    pub const fn new(byte_offset: u32) -> Self {
        Self(byte_offset)
    }

    /// The byte position, as a plain integer.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}

impl fmt::Display for SynsetOffset {
    /// Writes the eight-digit, zero-filled form the dictionary files use.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:08}", self.0)
    }
}

impl From<SynsetOffset> for u64 {
    fn from(offset: SynsetOffset) -> Self {
        u64::from(offset.0)
    }
}

/// A syntactic restriction on where an adjective may appear.
///
/// `wndb(5WN)` allows an adjective's `word` field to carry one of three
/// parenthesised markers. They are part of the syntax of the field, not of the
/// lemma, so this crate parses them out: `awake(p)` yields the lemma `awake`
/// and [`SyntacticMarker::Predicate`], never a lemma no index contains.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SyntacticMarker {
    /// `(p)` — predicate position.
    Predicate,
    /// `(a)` — attributive; must appear before the noun it modifies.
    Attributive,
    /// `(ip)` — immediately postnominal.
    ImmediatelyPostnominal,
}

impl SyntacticMarker {
    /// All three, in the order `wndb(5WN)` lists them.
    pub const ALL: [Self; 3] = [
        Self::Predicate,
        Self::Attributive,
        Self::ImmediatelyPostnominal,
    ];

    /// The marker as written, parentheses included.
    #[must_use]
    pub fn suffix(self) -> &'static str {
        match self {
            Self::Predicate => "(p)",
            Self::Attributive => "(a)",
            Self::ImmediatelyPostnominal => "(ip)",
        }
    }

    /// Splits a `word` field into its lemma and its marker, if it has one.
    fn split(word: &str) -> (&str, Option<Self>) {
        for marker in Self::ALL {
            if let Some(lemma) = word.strip_suffix(marker.suffix()) {
                return (lemma, Some(marker));
            }
        }
        (word, None)
    }
}

impl fmt::Display for SyntacticMarker {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.suffix())
    }
}

/// One word of a synset, owned.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Word {
    /// The word as the lexicographer entered it: capitalisation preserved,
    /// spaces written as `_`, and any syntactic marker removed into
    /// [`Word::marker`].
    pub lemma: String,
    /// `lex_id`: a one-digit hexadecimal number that distinguishes senses of
    /// the same lemma within one lexicographer file.
    pub lex_id: u8,
    /// An adjective's syntactic restriction, when it has one.
    pub marker: Option<SyntacticMarker>,
}

/// One word of a synset, borrowing the line it was read from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WordRef<'a> {
    /// See [`Word::lemma`].
    pub lemma: &'a str,
    /// See [`Word::lex_id`].
    pub lex_id: u8,
    /// See [`Word::marker`].
    pub marker: Option<SyntacticMarker>,
}

impl WordRef<'_> {
    /// Copies into an owned [`Word`].
    #[must_use]
    pub fn to_word(self) -> Word {
        Word {
            lemma: self.lemma.to_owned(),
            lex_id: self.lex_id,
            marker: self.marker,
        }
    }
}

/// A synset's gloss, split into its definition and its example sentences.
///
/// `wndb(5WN)` describes the gloss as free text following a `|`, containing "a
/// definition, one or more example sentences, or both". WordNet's own
/// convention, which every distribution of the database follows, is that the
/// segments are separated by semicolons and that each example is written in
/// double quotes. This crate applies exactly that rule:
///
/// * the gloss is split on `;` **outside** double quotes;
/// * a segment that is wholly enclosed in double quotes is an example, with the
///   quotes removed;
/// * every other segment belongs to the definition, and the definition segments
///   are rejoined with `"; "` in their original order.
///
/// Splitting on every semicolon regardless of quoting — which is the tempting
/// simplification — truncates any definition that legitimately contains one.
/// `data.verb`'s "sigh" synset is the standing example: its gloss is
/// `heave or utter a sigh; breathe deeply and heavily; "She sighed sadly"`, one
/// definition of two clauses and one example, not one clause and two examples.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct Gloss {
    /// The definition, with surrounding whitespace removed.
    pub definition: String,
    /// The example sentences, in file order, with their quotes removed.
    pub examples: Vec<String>,
}

/// A [`Gloss`] borrowing the line it was read from.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct GlossRef<'a> {
    /// See [`Gloss::definition`]. Borrowed unless the definition spans more
    /// than one semicolon-separated segment and had to be rejoined.
    pub definition: Cow<'a, str>,
    /// See [`Gloss::examples`].
    pub examples: Vec<&'a str>,
}

impl GlossRef<'_> {
    /// Copies into an owned [`Gloss`].
    #[must_use]
    pub fn to_gloss(&self) -> Gloss {
        Gloss {
            definition: self.definition.as_ref().to_owned(),
            examples: self.examples.iter().map(|s| (*s).to_owned()).collect(),
        }
    }
}

/// A synset: one sense, shared by every word it contains.
///
/// This is the owned form. [`SynsetRef`] is the same record borrowing the line
/// it was parsed from; see the crate-level "Choosing the right API" section.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Synset {
    /// Where the record begins in its data file.
    pub offset: SynsetOffset,
    /// `lex_filenum`: which lexicographer file the synset came from.
    pub lex_filenum: u8,
    /// `ss_type`: the synset's own category, satellites included.
    pub synset_type: SynsetType,
    /// The synset's words, in file order. Never empty: `wndb(5WN)` requires
    /// `w_cnt` to be at least one, and a record claiming zero words is refused.
    pub words: Vec<Word>,
    /// The synset's relational pointers, in file order.
    pub pointers: Vec<Pointer>,
    /// The definition and examples.
    pub gloss: Gloss,
}

impl Synset {
    /// The first word of the synset — the one WordNet lists as the synset's
    /// name.
    #[must_use]
    pub fn lemma(&self) -> &str {
        // `words` is non-empty by construction: `parse_synset` rejects `w_cnt`
        // of zero, and every `Synset` this crate hands out came from it.
        self.words.first().map_or("", |w| w.lemma.as_str())
    }

    /// The file pair this synset lives in.
    #[must_use]
    pub fn part_of_speech(&self) -> PartOfSpeech {
        self.synset_type.part_of_speech()
    }

    /// The pointers whose relation is `symbol`.
    pub fn pointers_with(&self, symbol: PointerSymbol) -> impl Iterator<Item = &Pointer> {
        self.pointers.iter().filter(move |p| p.symbol == symbol)
    }
}

/// A [`Synset`] borrowing the line it was parsed from.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SynsetRef<'a> {
    /// See [`Synset::offset`].
    pub offset: SynsetOffset,
    /// See [`Synset::lex_filenum`].
    pub lex_filenum: u8,
    /// See [`Synset::synset_type`].
    pub synset_type: SynsetType,
    /// See [`Synset::words`].
    pub words: Vec<WordRef<'a>>,
    /// See [`Synset::pointers`].
    pub pointers: Vec<Pointer>,
    /// See [`Synset::gloss`].
    pub gloss: GlossRef<'a>,
}

impl SynsetRef<'_> {
    /// The first word of the synset.
    #[must_use]
    pub fn lemma(&self) -> &str {
        self.words.first().map_or("", |w| w.lemma)
    }

    /// The file pair this synset lives in.
    #[must_use]
    pub fn part_of_speech(&self) -> PartOfSpeech {
        self.synset_type.part_of_speech()
    }

    /// Copies into an owned [`Synset`].
    #[must_use]
    pub fn to_synset(&self) -> Synset {
        Synset {
            offset: self.offset,
            lex_filenum: self.lex_filenum,
            synset_type: self.synset_type,
            words: self.words.iter().map(|w| w.to_word()).collect(),
            pointers: self.pointers.clone(),
            gloss: self.gloss.to_gloss(),
        }
    }
}

/// Parses one `data.*` line.
///
/// The record layout is `wndb(5WN)`'s, read strictly left to right:
///
/// ```text
/// synset_offset  lex_filenum  ss_type  w_cnt  word  lex_id  [word  lex_id...]
///     p_cnt  [ptr...]  [frames...]  |  gloss
/// ```
///
/// Verb frames sit between the pointers and the `|`; this crate does not parse
/// them, and does not need to, because the gloss is located by the delimiter
/// rather than by counting fields.
///
/// # Errors
///
/// A [`RecordError`] naming the first field that did not match the format.
pub(crate) fn parse_synset(line: &str) -> std::result::Result<SynsetRef<'_>, RecordError> {
    // `wndb(5WN)` puts the gloss after a vertical bar, and no field before it
    // may contain one, so the first `|` is the delimiter. Splitting at every
    // occurrence — or at the first `"| "` — would truncate a gloss containing a
    // bar and is not what the format says.
    let (head, gloss_text) = line.split_once('|').ok_or(RecordError::MissingGloss)?;

    let mut tokens = head.split_ascii_whitespace();
    let offset = parse::offset(
        "synset_offset",
        parse::required(tokens.next(), "synset_offset")?,
    )?;
    let lex_filenum = parse::decimal_u8(
        "lex_filenum",
        parse::required(tokens.next(), "lex_filenum")?,
    )?;
    let ss_type = parse::required(tokens.next(), "ss_type")?;
    let synset_type = SynsetType::from_tag(ss_type).ok_or(RecordError::InvalidField {
        field: "ss_type",
        value: ss_type.to_owned(),
    })?;

    let w_cnt_token = parse::required(tokens.next(), "w_cnt")?;
    let w_cnt = parse::hex_u8("w_cnt", w_cnt_token)?;
    if w_cnt == 0 {
        return Err(RecordError::InvalidField {
            field: "w_cnt",
            value: w_cnt_token.to_owned(),
        });
    }
    let mut words = Vec::with_capacity(usize::from(w_cnt));
    for _ in 0..w_cnt {
        let word = parse::required(tokens.next(), "word")?;
        let lex_id = parse::hex_u8("lex_id", parse::required(tokens.next(), "lex_id")?)?;
        let (lemma, marker) = SyntacticMarker::split(word);
        words.push(WordRef {
            lemma,
            lex_id,
            marker,
        });
    }

    let p_cnt = parse::decimal_u32("p_cnt", parse::required(tokens.next(), "p_cnt")?)?;
    // `p_cnt` is three decimal digits in the format, so it cannot name more
    // pointers than a line can hold; the capacity is still bounded explicitly
    // rather than trusted, and every pointer's four fields must actually be
    // present or the record is refused.
    let mut pointers = Vec::with_capacity(p_cnt.min(1024) as usize);
    for _ in 0..p_cnt {
        let symbol_token = parse::required(tokens.next(), "ptr_symbol")?;
        // A data record always writes the domain pointer's class letter; only an
        // index entry may omit it, because there the relation is summarised over
        // every sense of the lemma and the class belongs to the sense. Accepting
        // the unqualified form here would admit a malformed record as a
        // `Domain`/`Member` whose class nothing can later supply.
        let symbol = match PointerSymbol::from_symbol(symbol_token) {
            Some(PointerSymbol::Domain | PointerSymbol::Member) | None => {
                return Err(RecordError::InvalidField {
                    field: "ptr_symbol",
                    value: symbol_token.to_owned(),
                });
            }
            Some(symbol) => symbol,
        };
        let target = parse::offset(
            "synset_offset",
            parse::required(tokens.next(), "synset_offset")?,
        )?;
        let pos_token = parse::required(tokens.next(), "pos")?;
        let target_type = SynsetType::from_tag(pos_token).ok_or(RecordError::InvalidField {
            field: "pos",
            value: pos_token.to_owned(),
        })?;
        let scope = parse::pointer_scope(parse::required(tokens.next(), "source/target")?)?;
        pointers.push(Pointer {
            symbol,
            offset: target,
            synset_type: target_type,
            scope,
        });
    }

    Ok(SynsetRef {
        offset,
        lex_filenum,
        synset_type,
        words,
        pointers,
        gloss: parse_gloss(gloss_text),
    })
}

/// Splits a gloss into its definition and its examples; see [`Gloss`].
fn parse_gloss(gloss: &str) -> GlossRef<'_> {
    let mut definition_parts: Vec<&str> = Vec::new();
    let mut examples: Vec<&str> = Vec::new();

    for segment in split_outside_quotes(gloss) {
        let segment = segment.trim();
        if segment.is_empty() {
            continue;
        }
        match strip_enclosing_quotes(segment) {
            Some(example) => examples.push(example),
            None => definition_parts.push(segment),
        }
    }

    let definition = match definition_parts.len() {
        0 => Cow::Borrowed(""),
        1 => Cow::Borrowed(definition_parts[0]),
        _ => Cow::Owned(definition_parts.join("; ")),
    };
    GlossRef {
        definition,
        examples,
    }
}

/// The segments of `s` separated by `;` characters that are not inside a
/// double-quoted run.
fn split_outside_quotes(s: &str) -> impl Iterator<Item = &str> {
    let mut in_quotes = false;
    let mut start = 0usize;
    let mut done = false;
    let mut chars = s.char_indices();
    std::iter::from_fn(move || {
        if done {
            return None;
        }
        for (i, c) in chars.by_ref() {
            match c {
                '"' => in_quotes = !in_quotes,
                ';' if !in_quotes => {
                    let piece = &s[start..i];
                    start = i + 1;
                    return Some(piece);
                }
                _ => {}
            }
        }
        done = true;
        Some(&s[start..])
    })
}

/// The inside of `s` when it is wholly enclosed in double quotes.
fn strip_enclosing_quotes(s: &str) -> Option<&str> {
    let inner = s.strip_prefix('"')?.strip_suffix('"')?;
    Some(inner)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pointer::PointerScope;

    /// A real `data.noun` record, transcribed from WordNet 3.1. Every expected
    /// value below is read off the record by applying `wndb(5WN)`'s field
    /// layout by hand.
    const NODE: &str = "03832647 06 n 03 node 0 client 0 guest 0 003 @ 03086983 n 0000 #p 03089375 n 0000 ;c 06138021 n 0000 | (computer science) any computer that is hooked up to a computer network  ";

    #[test]
    fn parses_a_real_synset_field_by_field() {
        let s = parse_synset(NODE).unwrap();
        assert_eq!(s.offset, SynsetOffset::new(3_832_647));
        assert_eq!(s.lex_filenum, 6);
        assert_eq!(s.synset_type, SynsetType::Noun);
        assert_eq!(s.part_of_speech(), PartOfSpeech::Noun);
        assert_eq!(
            s.words.iter().map(|w| w.lemma).collect::<Vec<_>>(),
            ["node", "client", "guest"]
        );
        assert!(s.words.iter().all(|w| w.lex_id == 0 && w.marker.is_none()));
        assert_eq!(s.lemma(), "node");

        assert_eq!(s.pointers.len(), 3);
        assert_eq!(s.pointers[0].symbol, PointerSymbol::Hypernym);
        assert_eq!(s.pointers[0].offset, SynsetOffset::new(3_086_983));
        assert_eq!(s.pointers[0].synset_type, SynsetType::Noun);
        assert_eq!(s.pointers[0].scope, PointerScope::Semantic);
        assert_eq!(s.pointers[2].symbol, PointerSymbol::DomainOfTopic);

        // The gloss is trimmed; the record's two trailing spaces are padding.
        assert_eq!(
            s.gloss.definition,
            "(computer science) any computer that is hooked up to a computer network"
        );
        assert!(s.gloss.examples.is_empty());
        assert_eq!(s.to_synset().gloss.definition, s.gloss.definition);
    }

    /// `w_cnt` is two hexadecimal digits (`wndb(5WN)`), so `0b` is eleven.
    /// Reading it as decimal would stop at ten words and then find `k` where
    /// `p_cnt` should be.
    #[test]
    fn word_count_is_hexadecimal() {
        let line = "00000001 06 n 0b a 0 b 0 c 0 d 0 e 0 f 0 g 0 h 0 i 0 j 0 k 0 000 | g  ";
        let s = parse_synset(line).unwrap();
        assert_eq!(s.words.len(), 11);
        assert_eq!(s.words[10].lemma, "k");
        assert!(s.pointers.is_empty());
    }

    #[test]
    fn a_zero_word_count_is_refused() {
        let line = "00000001 06 n 00 000 | g  ";
        assert_eq!(
            parse_synset(line).unwrap_err(),
            RecordError::InvalidField {
                field: "w_cnt",
                value: "00".to_owned()
            }
        );
    }

    /// The standing example from `data.verb`: a two-clause definition and one
    /// example, not a one-clause definition and two examples.
    #[test]
    fn a_semicolon_in_the_definition_does_not_become_an_example() {
        let line = "00004032 09 v 02 sigh 0 suspire 0 000 | heave or utter a sigh; breathe deeply and heavily; \"She sighed sadly\"  ";
        let s = parse_synset(line).unwrap();
        assert_eq!(
            s.gloss.definition,
            "heave or utter a sigh; breathe deeply and heavily"
        );
        assert_eq!(s.gloss.examples, ["She sighed sadly"]);
    }

    #[test]
    fn a_semicolon_inside_a_quoted_example_does_not_split_it() {
        let line = "00000001 06 n 01 x 0 000 | a definition; \"one clause; another clause\"; \"second example\"  ";
        let s = parse_synset(line).unwrap();
        assert_eq!(s.gloss.definition, "a definition");
        assert_eq!(
            s.gloss.examples,
            ["one clause; another clause", "second example"]
        );
    }

    #[test]
    fn a_gloss_may_be_examples_only_or_definition_only() {
        let only_def = parse_synset("00000001 06 n 01 x 0 000 | just a definition  ").unwrap();
        assert_eq!(only_def.gloss.definition, "just a definition");
        assert!(only_def.gloss.examples.is_empty());

        let only_ex = parse_synset("00000001 06 n 01 x 0 000 | \"just an example\"  ").unwrap();
        assert_eq!(only_ex.gloss.definition, "");
        assert_eq!(only_ex.gloss.examples, ["just an example"]);

        let empty = parse_synset("00000001 06 n 01 x 0 000 |  ").unwrap();
        assert_eq!(empty.gloss.definition, "");
        assert!(empty.gloss.examples.is_empty());
    }

    #[test]
    fn a_gloss_containing_a_bar_keeps_it() {
        let s = parse_synset("00000300 03 a 01 pipes 0 000 | first| second| third  ").unwrap();
        assert_eq!(s.gloss.definition, "first| second| third");
    }

    #[test]
    fn a_record_without_a_gloss_delimiter_is_refused() {
        assert_eq!(
            parse_synset("00000001 06 n 01 x 0 000").unwrap_err(),
            RecordError::MissingGloss
        );
    }

    #[test]
    fn adjective_markers_are_split_out_of_the_lemma() {
        let s = parse_synset("00000001 00 a 03 awake 0 alert(p) 1 sole(a) 2 000 | x  ").unwrap();
        assert_eq!(s.words[0].lemma, "awake");
        assert_eq!(s.words[0].marker, None);
        assert_eq!(s.words[1].lemma, "alert");
        assert_eq!(s.words[1].marker, Some(SyntacticMarker::Predicate));
        assert_eq!(s.words[2].lemma, "sole");
        assert_eq!(s.words[2].marker, Some(SyntacticMarker::Attributive));

        let ip = parse_synset("00000001 00 a 01 elect(ip) 0 000 | x  ").unwrap();
        assert_eq!(ip.words[0].lemma, "elect");
        assert_eq!(
            ip.words[0].marker,
            Some(SyntacticMarker::ImmediatelyPostnominal)
        );
    }

    /// A truncated record names the first field that is missing rather than
    /// inventing a value for it.
    #[test]
    fn a_truncated_record_names_the_missing_field() {
        let cases: [(&str, RecordError); 5] = [
            (
                "|g",
                RecordError::MissingField {
                    field: "synset_offset",
                },
            ),
            (
                "1 |g",
                RecordError::MissingField {
                    field: "lex_filenum",
                },
            ),
            ("1 06 |g", RecordError::MissingField { field: "ss_type" }),
            ("1 06 n |g", RecordError::MissingField { field: "w_cnt" }),
            ("1 06 n 01 |g", RecordError::MissingField { field: "word" }),
        ];
        for (line, want) in cases {
            assert_eq!(parse_synset(line).unwrap_err(), want, "{line:?}");
        }
        assert_eq!(
            parse_synset("1 06 n 01 x 0 |g").unwrap_err(),
            RecordError::MissingField { field: "p_cnt" }
        );
        assert_eq!(
            parse_synset("1 06 n 01 x 0 001 @ 2 n |g").unwrap_err(),
            RecordError::MissingField {
                field: "source/target"
            }
        );
    }

    #[test]
    fn an_unknown_pointer_symbol_is_refused() {
        assert_eq!(
            parse_synset("1 06 n 01 x 0 001 ?? 2 n 0000 |g").unwrap_err(),
            RecordError::InvalidField {
                field: "ptr_symbol",
                value: "??".to_owned()
            }
        );
    }

    #[test]
    fn a_lexical_pointer_carries_both_word_numbers() {
        let s = parse_synset("1 06 n 01 x 0 001 ! 2 n 0102 |g").unwrap();
        match s.pointers[0].scope {
            PointerScope::Lexical {
                source_word,
                target_word,
            } => {
                assert_eq!(source_word.get(), 1);
                assert_eq!(target_word.get(), 2);
            }
            PointerScope::Semantic => panic!("expected a lexical pointer"),
        }
    }

    #[test]
    fn non_ascii_and_astral_glosses_survive_intact() {
        for text in [
            "café",
            "Москва",
            "日本語",
            "😀 emoji",
            "Ελλάδα",
            "a\u{0301}",
        ] {
            let line = format!("00000001 06 n 01 w 0 000 | {text}  ");
            let s = parse_synset(&line).unwrap();
            assert_eq!(s.gloss.definition, text);
        }
    }

    #[test]
    fn the_borrowed_and_owned_records_agree() {
        let borrowed = parse_synset(NODE).unwrap();
        let owned = borrowed.to_synset();
        assert_eq!(owned.offset, borrowed.offset);
        assert_eq!(owned.lemma(), borrowed.lemma());
        assert_eq!(owned.pointers, borrowed.pointers);
        assert_eq!(owned.gloss.definition, borrowed.gloss.definition);
        assert_eq!(owned.gloss.examples, borrowed.gloss.examples);
        assert_eq!(owned.words.len(), borrowed.words.len());
        assert_eq!(
            owned.pointers_with(PointerSymbol::Hypernym).count(),
            1,
            "node has exactly one hypernym pointer"
        );
    }

    #[test]
    fn offsets_display_as_the_files_write_them() {
        assert_eq!(SynsetOffset::new(83).to_string(), "00000083");
        assert_eq!(SynsetOffset::new(0).to_string(), "00000000");
        assert_eq!(SynsetOffset::new(123_456_789).to_string(), "123456789");
        assert_eq!(u64::from(SynsetOffset::new(83)), 83u64);
    }

    #[test]
    fn every_syntactic_marker_round_trips() {
        for m in SyntacticMarker::ALL {
            let word = format!("lemma{}", m.suffix());
            assert_eq!(SyntacticMarker::split(&word), ("lemma", Some(m)));
            assert_eq!(m.to_string(), m.suffix());
        }
        assert_eq!(SyntacticMarker::split("lemma"), ("lemma", None));
        // A parenthesised suffix that is not one of the three stays put.
        assert_eq!(SyntacticMarker::split("lemma(x)"), ("lemma(x)", None));
    }
}
