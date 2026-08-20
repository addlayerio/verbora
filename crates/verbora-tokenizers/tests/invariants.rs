//! The guarantees the crate documentation states, checked over a corpus that
//! deliberately reaches the classes an ASCII fixture set cannot.
//!
//! Every expected value here comes from UAX #29 or from arithmetic shown
//! inline. Nothing asserts "whatever the code currently does".

use std::collections::HashSet;

use verbora_tokenizers::{
    AbbreviationError, BorrowingTokenizer, SegmentTokenizer, SentenceTokenizer, Tokenizer,
    WordTokenizer,
};

/// The corpus every invariant below is swept over.
///
/// It spans ASCII, Latin-1, Greek, Cyrillic, Arabic, Hebrew, Devanagari, Thai,
/// Hangul, CJK and astral scalars, and carries the specific shapes that break
/// naive implementations: a lone `U+FEFF`, a lone `U+0085`, `CR LF`, text
/// beginning with a combining mark, text ending mid-combining-sequence, a ZWJ
/// emoji sequence, an unpaired regional indicator, and repeated identical
/// tokens (which is what makes `text.contains(tok)` a vacuous check).
const CORPUS: &[&str] = &[
    "",
    " ",
    "\t\n\r",
    "\r\n",
    "a\r\nb",
    "\u{feff}",
    "\u{85}",
    "a\u{85}b",
    "a\u{feff}b",
    "a",
    "Z",
    "0",
    "!",
    "...",
    "--",
    "don't",
    "well-known",
    "and/or",
    "3.14",
    "1,000",
    "node_js",
    "a:b",
    "___",
    "a a a",
    "the the the the",
    "ALLCAPS",
    "café naïve",
    "Äpfel Öl Übung",
    "a×b÷c",
    "straße",
    "Москва привет, мир",
    "Ελλάδα",
    "العربية",
    "עִבְרִית",
    "हिन्दी",
    "ภาษาไทย",
    "日本語",
    "すもももももも",
    "한국어",
    "e\u{301}",
    "\u{301}abc",
    "abc\u{301}",
    "\u{301}",
    "😀",
    "a😀b",
    "👍你好",
    "👨\u{200d}👩\u{200d}👧\u{200d}👦",
    "🇦🇧🇨",
    "🇦",
    "a🇦🇧🇨🇩b",
    "𝕳𝖊𝖑𝖑𝖔",
    "İstanbul",
    "\u{212a}\u{17f}",
    "١٢٣ and ٤٥٦",
    "\u{2160}\u{2161}",
    "½ ¾",
    "Dr. Smith arrived. He left.",
    "He works at Acme Inc. She does not.",
    "One.\u{85}Two.",
    "Visit www.a.b/$'x. Next.",
    "   ",
];

/// How the Unicode Character Database classifies one scalar, for the single
/// question `WordTokenizer`'s filter asks of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ucd {
    /// Has the `Alphabetic` property (UAX #44: `General_Category` in
    /// `{Lu, Ll, Lt, Lm, Lo, Nl}`, plus `Other_Alphabetic`).
    Alphabetic,
    /// `General_Category` is `Nd`, `Nl` or `No`.
    Numeric,
    /// Neither, so a segment made only of these scalars is not a word.
    Neither,
}

/// `Alphabetic` ranges, transcribed from the UCD, covering every scalar in
/// [`CORPUS`].
///
/// **This table is the point of the test.** The predicate it replaces was
/// `c.is_alphabetic() || c.is_numeric()`, which is not an independent oracle at
/// all: `unicode-segmentation`'s word filter is
/// `tables::util::is_alphanumeric`, and that function is *defined* as
/// `c.is_alphabetic() || c.is_numeric()` whenever its Unicode version equals
/// `char::UNICODE_VERSION` — which it does here, both being 17.0.0. The
/// assertion was `f(x) == f(x)` and could not fail, whatever the tokenizer did
/// with the property.
///
/// So these values come from the standard's own published data instead. Nothing
/// in this file may call `char::is_alphabetic`, `char::is_numeric` or
/// `char::is_alphanumeric` again; a new corpus entry is classified by looking
/// the scalar up in the UCD, and [`ucd_class`] fails loudly until it is.
const ALPHABETIC: &[(char, char)] = &[
    ('\u{0041}', '\u{005A}'),   // Lu  LATIN CAPITAL LETTER A..Z
    ('\u{0061}', '\u{007A}'),   // Ll  LATIN SMALL LETTER A..Z
    ('\u{00C0}', '\u{00D6}'),   // Lu  À..Ö  (U+00D7 is Sm, excluded)
    ('\u{00D8}', '\u{00F6}'),   // Lu/Ll Ø..ö  (U+00F7 is Sm, excluded)
    ('\u{00F8}', '\u{00FF}'),   // Ll  ø..ÿ
    ('\u{0100}', '\u{017F}'),   // Lu/Ll Latin Extended-A, incl. U+0130 İ, U+017F ſ
    ('\u{0386}', '\u{0386}'),   // Lu  GREEK CAPITAL LETTER ALPHA WITH TONOS
    ('\u{0388}', '\u{038A}'),   // Lu  GREEK CAPITAL LETTER EPSILON..IOTA WITH TONOS
    ('\u{038C}', '\u{038C}'),   // Lu  GREEK CAPITAL LETTER OMICRON WITH TONOS
    ('\u{038E}', '\u{03A1}'),   // Lu  GREEK UPSILON WITH TONOS..RHO
    ('\u{03A3}', '\u{03CE}'),   // Lu/Ll GREEK SIGMA..OMEGA WITH TONOS
    ('\u{0400}', '\u{045F}'),   // Lu/Ll Cyrillic
    ('\u{05B0}', '\u{05BD}'),   // Mn  Hebrew points — Other_Alphabetic
    ('\u{05D0}', '\u{05EA}'),   // Lo  Hebrew letters ALEF..TAV
    ('\u{0620}', '\u{064A}'),   // Lo/Lm Arabic letters (U+0640 TATWEEL is Lm)
    ('\u{0904}', '\u{0939}'),   // Lo  Devanagari letters
    ('\u{093E}', '\u{094C}'),   // Mc/Mn Devanagari vowel signs — Other_Alphabetic
    ('\u{0E01}', '\u{0E3A}'),   // Lo + Other_Alphabetic Mn — Thai
    ('\u{0E40}', '\u{0E46}'),   // Lo/Lm Thai SARA E..MAIYAMOK
    ('\u{212A}', '\u{212A}'),   // Lu  KELVIN SIGN
    ('\u{3041}', '\u{3096}'),   // Lo  Hiragana
    ('\u{4E00}', '\u{9FFF}'),   // Lo  CJK Unified Ideographs
    ('\u{AC00}', '\u{D7A3}'),   // Lo  Hangul syllables
    ('\u{1D56C}', '\u{1D59F}'), // Lu/Ll MATHEMATICAL BOLD FRAKTUR A..z
];

/// `General_Category` in `{Nd, Nl, No}`, transcribed from the UCD.
///
/// `U+2160..U+2182` is `Nl`, so it is *both* numeric and `Alphabetic`; it is
/// listed here because either answer gives the same verdict and the numeric
/// reading is the one the corpus is carrying it for.
const NUMERIC: &[(char, char)] = &[
    ('\u{0030}', '\u{0039}'), // Nd  DIGIT ZERO..NINE
    ('\u{00B2}', '\u{00B3}'), // No  SUPERSCRIPT TWO, THREE
    ('\u{00B9}', '\u{00B9}'), // No  SUPERSCRIPT ONE
    ('\u{00BC}', '\u{00BE}'), // No  VULGAR FRACTION ONE QUARTER..THREE QUARTERS
    ('\u{0660}', '\u{0669}'), // Nd  ARABIC-INDIC DIGIT ZERO..NINE
    ('\u{2160}', '\u{2182}'), // Nl  ROMAN NUMERAL ONE..TEN THOUSAND
];

/// Scalars that are neither `Alphabetic` nor `Nd`/`Nl`/`No`, transcribed from
/// the UCD. Listing them is what lets [`ucd_class`] reject a scalar it has
/// never been told about instead of silently answering "not a word".
const NEITHER: &[(char, char)] = &[
    ('\u{0009}', '\u{000D}'), // Cc  TAB..CARRIAGE RETURN
    ('\u{0020}', '\u{0020}'), // Zs  SPACE
    ('\u{0021}', '\u{002F}'), // P*/S* ! " # $ % & ' ( ) * + , - . /
    ('\u{003A}', '\u{0040}'), // P*/Sm : ; < = > ? @
    ('\u{005B}', '\u{0060}'), // P*/Sk [ \ ] ^ _ `  (U+005F LOW LINE is Pc)
    ('\u{007B}', '\u{007E}'), // P*/Sm { | } ~
    ('\u{0085}', '\u{0085}'), // Cc  NEXT LINE
    ('\u{00D7}', '\u{00D7}'), // Sm  MULTIPLICATION SIGN
    ('\u{00F7}', '\u{00F7}'), // Sm  DIVISION SIGN
    ('\u{0300}', '\u{0344}'), // Mn  combining marks, none Other_Alphabetic
    //                           (U+0345 YPOGEGRAMMENI is, and is excluded)
    ('\u{094D}', '\u{094D}'), // Mn  DEVANAGARI SIGN VIRAMA — not Other_Alphabetic
    ('\u{200D}', '\u{200D}'), // Cf  ZERO WIDTH JOINER
    ('\u{FEFF}', '\u{FEFF}'), // Cf  ZERO WIDTH NO-BREAK SPACE
    ('\u{1F1E6}', '\u{1F1FF}'), // So  REGIONAL INDICATOR SYMBOL LETTER A..Z
    ('\u{1F300}', '\u{1F5FF}'), // So  Miscellaneous Symbols and Pictographs
    ('\u{1F600}', '\u{1F64F}'), // So  Emoticons
];

/// The UCD classification of `c`, from the transcribed tables above.
///
/// Panics for a scalar no table covers, which is the mechanism that keeps this
/// oracle honest: adding to [`CORPUS`] forces a property lookup in the standard
/// rather than a silent default.
fn ucd_class(c: char) -> Ucd {
    for &(lo, hi) in NUMERIC {
        if (lo..=hi).contains(&c) {
            return Ucd::Numeric;
        }
    }
    for &(lo, hi) in ALPHABETIC {
        if (lo..=hi).contains(&c) {
            return Ucd::Alphabetic;
        }
    }
    for &(lo, hi) in NEITHER {
        if (lo..=hi).contains(&c) {
            return Ucd::Neither;
        }
    }
    panic!(
        "U+{:04X} is in the corpus but in none of this file's transcribed UCD \
         tables. Look its Alphabetic and General_Category values up in the \
         standard and add it — never by calling char::is_alphabetic or \
         char::is_numeric, which are the implementation's own predicate.",
        c as u32
    )
}

/// Whether `segment` is a word under the crate's stated filter: at least one
/// scalar with the `Alphabetic` property, or with `General_Category` in
/// `{Nd, Nl, No}`.
///
/// Every scalar is classified *before* the verdict is taken. A short-circuiting
/// `.any(ucd_class(c) != Neither)` would let an unclassified scalar hide behind
/// an earlier match, which is exactly how the coverage this oracle depends on
/// would rot.
fn is_word_segment(segment: &str) -> bool {
    let classes: Vec<Ucd> = segment.chars().map(ucd_class).collect();
    classes
        .iter()
        .any(|class| matches!(class, Ucd::Alphabetic | Ucd::Numeric))
}

/// The byte range of `token` inside `text`, by pointer, so a repeated token
/// cannot be matched against the wrong occurrence.
fn range_of(text: &str, token: &str) -> std::ops::Range<usize> {
    let base = text.as_ptr() as usize;
    let at = token.as_ptr() as usize;
    assert!(
        at >= base && at + token.len() <= base + text.len(),
        "token {token:?} does not point into {text:?}"
    );
    (at - base)..(at - base + token.len())
}

// ---------------------------------------------------------------------------
// Concatenation, substring, order, emptiness
// ---------------------------------------------------------------------------

#[test]
fn segment_and_sentence_concatenation_reproduce_the_input() {
    let sentence = SentenceTokenizer::new();
    let tailored = SentenceTokenizer::with_abbreviations(["Dr.", "Inc.", "no."]).unwrap();
    for &text in CORPUS {
        assert_eq!(
            SegmentTokenizer.tokens(text).collect::<String>(),
            text,
            "segments of {text:?}"
        );
        assert_eq!(
            sentence.tokens(text).collect::<String>(),
            text,
            "sentences of {text:?}"
        );
        // Suppression joins segments; it must never drop text.
        assert_eq!(
            tailored.tokens(text).collect::<String>(),
            text,
            "tailored sentences of {text:?}"
        );
    }
}

/// Every token points into the input at a strictly increasing, non-overlapping
/// byte range.
///
/// The `text.contains(tok)` shape this replaces passes vacuously on `"a a a"`
/// and cannot detect an off-by-one range, which is why the check is by pointer.
#[test]
fn tokens_are_substrings_at_strictly_increasing_disjoint_ranges() {
    let sentence = SentenceTokenizer::new();
    for &text in CORPUS {
        for (name, tokens) in [
            ("word", WordTokenizer.tokenize_borrowed(text)),
            ("segment", SegmentTokenizer.tokenize_borrowed(text)),
            ("sentence", sentence.tokenize_borrowed(text)),
        ] {
            let mut previous_end = 0;
            for token in tokens {
                let range = range_of(text, token);
                assert!(
                    range.start >= previous_end,
                    "{name}: overlapping or out-of-order token {token:?} in {text:?}"
                );
                assert!(
                    range.end > range.start,
                    "{name}: empty range for {token:?} in {text:?}"
                );
                assert_eq!(
                    &text[range.clone()],
                    token,
                    "{name}: token does not equal its own range in {text:?}"
                );
                previous_end = range.end;
            }
        }
    }
}

#[test]
fn no_tokenizer_ever_yields_an_empty_token() {
    let sentence = SentenceTokenizer::new();
    let tailored = SentenceTokenizer::with_abbreviations(["Dr.", "no."]).unwrap();
    for &text in CORPUS {
        for (name, tokens) in [
            ("word", WordTokenizer.tokenize_borrowed(text)),
            ("segment", SegmentTokenizer.tokenize_borrowed(text)),
            ("sentence", sentence.tokenize_borrowed(text)),
            ("tailored", tailored.tokenize_borrowed(text)),
        ] {
            assert!(
                tokens.iter().all(|t| !t.is_empty()),
                "{name} yielded an empty token for {text:?}"
            );
        }
    }
    // The empty input yields nothing at all — not one empty token.
    assert!(WordTokenizer.tokenize_borrowed("").is_empty());
    assert!(SegmentTokenizer.tokenize_borrowed("").is_empty());
    assert!(sentence.tokenize_borrowed("").is_empty());
    // Whitespace-only input is one *segment*, not a token that occurs nowhere.
    assert_eq!(sentence.tokenize_borrowed("   "), ["   "]);
    assert_eq!(SegmentTokenizer.tokenize_borrowed("   "), ["   "]);
    assert!(WordTokenizer.tokenize_borrowed("   ").is_empty());
}

/// `WordTokenizer`'s output is a pointer-identical subsequence of
/// `SegmentTokenizer`'s.
#[test]
fn words_are_a_pointer_identical_subsequence_of_segments() {
    for &text in CORPUS {
        let segments = SegmentTokenizer.tokenize_borrowed(text);
        let words = WordTokenizer.tokenize_borrowed(text);
        let mut segments = segments.into_iter();
        for word in &words {
            let found = segments.by_ref().find(|s| {
                std::ptr::eq(
                    std::ptr::from_ref::<str>(*s),
                    std::ptr::from_ref::<str>(*word),
                )
            });
            assert!(
                found.is_some(),
                "word {word:?} of {text:?} is not a segment at the same address"
            );
        }
    }
}

/// Every scalar in the corpus is classified by the transcribed UCD tables.
///
/// The sweep below folds over segments, so a scalar the tables do not know
/// would only be reached when it appears in a segment; this reaches all of
/// them, and fails the moment the corpus grows past the tables.
#[test]
fn the_oracle_classifies_every_scalar_in_the_corpus() {
    let mut seen = 0_usize;
    for &text in CORPUS {
        for c in text.chars() {
            let _ = ucd_class(c);
            seen += 1;
        }
    }
    assert!(seen > 0, "the corpus is empty");
}

/// Membership in `WordTokenizer`'s output equals the independently written
/// predicate, for every segment of the corpus.
///
/// "Independently written" is load-bearing and was previously false: the
/// oracle called `char::is_alphabetic() || char::is_numeric()`, which is the
/// body of the very function `unicode-segmentation` filters with. The oracle is
/// now the transcribed UCD tables above, so the two sides of this assertion are
/// computed from different things.
///
/// The corpus deliberately carries segments that are numeric but not
/// alphabetic (`"3.14"`, Arabic-Indic `"١٢٣"`, `U+2160` ROMAN NUMERAL ONE which
/// is `Nl`, `"½"` which is `No`) and segments made only of `Extend` marks
/// (`"\u{301}"`), which an alphabetic-only oracle gets wrong in both
/// directions.
#[test]
fn the_word_filter_is_exactly_alphabetic_or_numeric() {
    for &text in CORPUS {
        let words: HashSet<(usize, usize)> = WordTokenizer
            .tokenize_borrowed(text)
            .into_iter()
            .map(|w| {
                let r = range_of(text, w);
                (r.start, r.end)
            })
            .collect();
        for segment in SegmentTokenizer.tokenize_borrowed(text) {
            let r = range_of(text, segment);
            assert_eq!(
                words.contains(&(r.start, r.end)),
                is_word_segment(segment),
                "segment {segment:?} of {text:?}"
            );
        }
    }
    // The classes named above, asserted directly so the sweep cannot pass by
    // never reaching them.
    assert_eq!(WordTokenizer.tokenize_borrowed("3.14"), ["3.14"]);
    assert_eq!(WordTokenizer.tokenize_borrowed("١٢٣"), ["١٢٣"]);
    assert_eq!(WordTokenizer.tokenize_borrowed("\u{2160}"), ["\u{2160}"]);
    assert_eq!(WordTokenizer.tokenize_borrowed("½"), ["½"]);
    assert!(WordTokenizer.tokenize_borrowed("\u{301}").is_empty());
    assert_eq!(SegmentTokenizer.tokenize_borrowed("\u{301}"), ["\u{301}"]);
    assert!(WordTokenizer.tokenize_borrowed("___").is_empty());
    assert_eq!(SegmentTokenizer.tokenize_borrowed("___"), ["___"]);
}

// ---------------------------------------------------------------------------
// The moved-behaviour table
// ---------------------------------------------------------------------------

/// Every row of the table in `WordTokenizer`'s documentation, asserted
/// directly. Each expected value is derived from the named UAX #29 rule, not
/// from running the implementation.
#[test]
fn the_documented_word_boundary_table_holds() {
    let rows: &[(&str, &[&str])] = &[
        // U+002D HYPHEN-MINUS is Word_Break=Other; WB999 breaks on both sides.
        ("well-known", &["well", "known"]),
        // U+002F SOLIDUS is Word_Break=Other.
        ("and/or", &["and", "or"]),
        // WB6/WB7: ALetter x MidNumLetQ ALetter, U+0027 is Single_Quote.
        ("don't", &["don't"]),
        // WB11/WB12: Numeric x MidNumLet x Numeric, U+002E is MidNumLet.
        ("3.14", &["3.14"]),
        // WB11/WB12 with U+002C COMMA, which is MidNum.
        ("1,000", &["1,000"]),
        // WB13a/WB13b: U+005F LOW LINE is ExtendNumLet.
        ("node_js", &["node_js"]),
        // WB6/WB7 with U+003A COLON, which is MidLetter.
        ("a:b", &["a:b"]),
        // U+00C4 is ALetter, so the umlaut does not separate the word.
        ("Äpfel", &["Äpfel"]),
        // ASCII capitals are ALetter.
        ("A B", &["A", "B"]),
        // U+00D7 and U+00F7 are Word_Break=Other.
        ("a×b÷c", &["a", "b", "c"]),
        // Cyrillic is ALetter.
        ("привет, мир", &["привет", "мир"]),
        // U+00E9 and U+00EF are ALetter.
        ("café naïve", &["café", "naïve"]),
        // Han is Word_Break=Other, so WB999 breaks between every scalar. This
        // is UAX #29 §4's stated limitation, not a defect.
        ("日本語", &["日", "本", "語"]),
        (
            "すもももももも",
            &["す", "も", "も", "も", "も", "も", "も"],
        ),
        // No U+FFFD is ever fabricated: an astral scalar is Other, and is not a
        // word.
        ("a😀b", &["a", "b"]),
        // Nothing is folded, and no sentinel is spliced in.
        ("İstanbul", &["İstanbul"]),
    ];
    for (input, expected) in rows {
        assert_eq!(
            &WordTokenizer.tokenize_borrowed(input),
            expected,
            "{input:?}"
        );
    }
}

/// No token from any tokenizer contains `U+FFFD` unless the input did.
#[test]
fn replacement_characters_are_never_fabricated() {
    let sentence = SentenceTokenizer::new();
    for &text in CORPUS {
        if text.contains('\u{fffd}') {
            continue;
        }
        for (name, tokens) in [
            ("word", WordTokenizer.tokenize(text)),
            ("segment", SegmentTokenizer.tokenize(text)),
            ("sentence", sentence.tokenize(text)),
        ] {
            assert!(
                tokens.iter().all(|t| !t.contains('\u{fffd}')),
                "{name} fabricated U+FFFD for {text:?}"
            );
        }
    }
    // And the input's own replacement characters survive untouched.
    assert_eq!(SegmentTokenizer.tokenize_borrowed("\u{fffd}"), ["\u{fffd}"]);
}

// ---------------------------------------------------------------------------
// Totality
// ---------------------------------------------------------------------------

/// Nothing panics, on any input, through any call shape.
///
/// The corpus is extended here with the bulk cases a fixture list cannot carry
/// inline: 64 KiB of a single combining mark, a long run of unpaired regional
/// indicators, and a long run of ZWJ.
#[test]
fn nothing_panics_on_pathological_input() {
    let mut inputs: Vec<String> = CORPUS.iter().map(|s| (*s).to_owned()).collect();
    inputs.push("\u{301}".repeat(64 * 1024 / 2));
    inputs.push("\u{1F1E6}".repeat(1024));
    inputs.push("a\u{200d}".repeat(1024));
    inputs.push("\u{200d}".repeat(1024));
    inputs.push(".".repeat(4096));

    let sentence = SentenceTokenizer::new();
    let tailored = SentenceTokenizer::with_abbreviations(["Dr.", "e.g.", "i.e."]).unwrap();
    let mut buf: Vec<&str> = Vec::new();
    for text in &inputs {
        // Four call shapes, three tokenizers.
        let _ = WordTokenizer.tokens(text).count();
        let _ = WordTokenizer.tokenize_borrowed(text);
        buf.clear();
        WordTokenizer.tokenize_borrowed_into(text, &mut buf);
        let _ = WordTokenizer.tokenize(text);

        let _ = SegmentTokenizer.tokens(text).count();
        let _ = SegmentTokenizer.tokenize_borrowed(text);
        buf.clear();
        SegmentTokenizer.tokenize_borrowed_into(text, &mut buf);
        let _ = SegmentTokenizer.tokenize(text);

        let _ = sentence.tokens(text).count();
        let _ = sentence.tokenize_borrowed(text);
        buf.clear();
        sentence.tokenize_borrowed_into(text, &mut buf);
        let _ = sentence.tokenize(text);

        let _ = tailored.tokenize_borrowed(text);
    }
}

/// `tokenize_borrowed_into` appends and does not clear — the documented
/// footgun, pinned so a "helpful" clear cannot be added silently.
#[test]
fn tokenize_borrowed_into_appends_without_clearing() {
    let mut buf = vec!["pre-existing"];
    WordTokenizer.tokenize_borrowed_into("one two", &mut buf);
    assert_eq!(buf, ["pre-existing", "one", "two"]);
    WordTokenizer.tokenize_borrowed_into("three", &mut buf);
    assert_eq!(buf, ["pre-existing", "one", "two", "three"]);
}

/// The owned path yields exactly the borrowed path's tokens. It is lossless
/// because the tokens are already valid `&str`.
#[test]
fn the_owned_path_agrees_with_the_borrowed_one() {
    let sentence = SentenceTokenizer::new();
    for &text in CORPUS {
        assert_eq!(
            WordTokenizer.tokenize(text),
            WordTokenizer.tokenize_borrowed(text)
        );
        assert_eq!(
            SegmentTokenizer.tokenize(text),
            SegmentTokenizer.tokenize_borrowed(text)
        );
        assert_eq!(sentence.tokenize(text), sentence.tokenize_borrowed(text));
    }
}

// ---------------------------------------------------------------------------
// Sentence abbreviations
// ---------------------------------------------------------------------------

/// Asserts a suppression fixture from both sides: what the untailored UAX #29
/// §5 rules produce, and what the tailoring produces — and that the two differ.
///
/// The `assert_ne!` is the point. A fixture whose tailored output equals its
/// untailored output demonstrates nothing about suppression however
/// abbreviation-shaped it looks, because it passes unchanged with the tailoring
/// switched off entirely. One such fixture was in this file: `"e.g."` over
/// `"Use a tool, e.g. a hammer. Then stop."`, where SB8 already suppresses both
/// interior breaks because the following scalar is `Lower`. It is now below,
/// as an explicit control, and the discriminating case sits beside it.
#[track_caller]
fn suppression_changes_the_split(
    abbreviations: &[&str],
    text: &str,
    untailored: &[&str],
    tailored: &[&str],
) {
    assert_ne!(
        untailored, tailored,
        "fixture {text:?} with {abbreviations:?} expects the same split either \
         way, so it cannot show that the tailoring did anything"
    );
    assert_eq!(
        SentenceTokenizer::new().tokenize_borrowed(text),
        untailored,
        "untailored {text:?}"
    );
    assert_eq!(
        SentenceTokenizer::with_abbreviations(abbreviations.iter().copied())
            .unwrap()
            .tokenize_borrowed(text),
        tailored,
        "tailored {text:?} with {abbreviations:?}"
    );
}

/// Asserts that a tailoring leaves the split alone — for the cases where *that*
/// is the contract, and labelled so it is never counted as suppression
/// coverage.
#[track_caller]
fn suppression_leaves_the_split_alone(abbreviations: &[&str], text: &str, expected: &[&str]) {
    assert_eq!(
        SentenceTokenizer::new().tokenize_borrowed(text),
        expected,
        "untailored {text:?}"
    );
    assert_eq!(
        SentenceTokenizer::with_abbreviations(abbreviations.iter().copied())
            .unwrap()
            .tokenize_borrowed(text),
        expected,
        "tailored {text:?} with {abbreviations:?}"
    );
}

#[test]
fn abbreviation_suppression_fixtures() {
    // Untailored, UAX #29 §5 breaks after the ATerm run following "Dr" because
    // the next scalar is `Upper`; with "Dr." that boundary is suppressed.
    suppression_changes_the_split(
        &["Dr."],
        "Dr. Smith arrived. He left.",
        &["Dr. ", "Smith arrived. ", "He left."],
        &["Dr. Smith arrived. ", "He left."],
    );
    // Suppressing the only interior boundary leaves one sentence.
    suppression_changes_the_split(
        &["Inc."],
        "He works at Acme Inc. She does not.",
        &["He works at Acme Inc. ", "She does not."],
        &["He works at Acme Inc. She does not."],
    );
    // Documented over-suppression: matching is a plain suffix test, so "no."
    // fires on "casino.". Pinned so it cannot be silently "fixed" into a
    // word-boundary rule that would then break "e.g." and "Ph.D.".
    suppression_changes_the_split(
        &["no."],
        "Visit the casino. Then leave.",
        &["Visit the casino. ", "Then leave."],
        &["Visit the casino. Then leave."],
    );
    // An interior-period abbreviation that actually changes the result. SB8
    // suppresses the break inside "e.g." on its own, because "g" is `Lower` —
    // but not the break after it, because "A" is `Upper`. That second break is
    // the tailoring's, and it is the case the deleted fixture could not reach:
    // this is why the word-boundary qualification (§4.3) was rejected, since a
    // word boundary would refuse to match an abbreviation whose own interior
    // period is a break opportunity.
    suppression_changes_the_split(
        &["e.g."],
        "Use a tool, e.g. A hammer works.",
        &["Use a tool, e.g. ", "A hammer works."],
        &["Use a tool, e.g. A hammer works."],
    );
    // "Ph.D." is the same class under a different rule: the interior period is
    // held by SB7, `(Upper | Lower) ATerm × Upper`, because "h" precedes it and
    // "D" follows immediately. The final period has a space before "Later", so
    // SB7 does not reach it and SB11 breaks — and that break is the
    // tailoring's to suppress.
    suppression_changes_the_split(
        &["Ph.D."],
        "She has a Ph.D. Later she left.",
        &["She has a Ph.D. ", "Later she left."],
        &["She has a Ph.D. Later she left."],
    );

    // --- Cases whose contract is that nothing changes ----------------------

    // The final boundary at text.len() is never suppressed, so the last
    // sentence is never lost — the case a naive implementation drops entirely.
    suppression_leaves_the_split_alone(
        &["Inc."],
        "Ends with an abbreviation Inc.",
        &["Ends with an abbreviation Inc."],
    );
    // Comparison is case-sensitive, so "No." does not fire on "casino.".
    suppression_leaves_the_split_alone(
        &["No."],
        "Visit the casino. Then leave.",
        &["Visit the casino. ", "Then leave."],
    );
    // The control. Both interior breaks are already suppressed by SB8 — the
    // scalar after each period is `Lower` — so the tailoring has nothing to do
    // and this fixture says nothing about suppression. It is kept to pin that
    // fact, not to demonstrate the tailoring.
    suppression_leaves_the_split_alone(
        &["e.g."],
        "Use a tool, e.g. a hammer. Then stop.",
        &["Use a tool, e.g. a hammer. ", "Then stop."],
    );
}

/// Suppression fires when **some** abbreviation matches, not when every one
/// does — the rule `SentenceTokenizer::with_abbreviations`' documentation
/// states ("suppression asks only whether *some* abbreviation matches") and the
/// reason duplicates and order are irrelevant.
///
/// Every other split assertion in this file uses a one-element set, where
/// "some" and "all" are the same predicate; mutating the implementation's
/// `.any` to `.all` leaves all of them green. These sets have two and three
/// elements, and exactly one element matches at each suppressed boundary.
#[test]
fn suppression_needs_only_one_abbreviation_to_match() {
    let two = SentenceTokenizer::with_abbreviations(["Dr.", "Inc."]).unwrap();

    // "Dr." matches at the boundary and "Inc." does not: one is enough.
    assert_eq!(
        two.tokenize_borrowed("Dr. Smith arrived. He left."),
        ["Dr. Smith arrived. ", "He left."]
    );
    // The other way round, so neither element is privileged by its position.
    assert_eq!(
        two.tokenize_borrowed("He works at Acme Inc. She does not."),
        ["He works at Acme Inc. She does not."]
    );
    // Both fire, at different boundaries of one text, each with the other
    // abbreviation not matching there.
    assert_eq!(
        two.tokenize_borrowed("Dr. Smith works at Acme Inc. She left."),
        ["Dr. Smith works at Acme Inc. She left."]
    );
    // A set none of whose elements match suppresses nothing.
    assert_eq!(two.tokenize_borrowed("One. Two."), ["One. ", "Two."]);

    // Duplicates and order are irrelevant, which is only true under "some":
    // under "every" a repeated entry would still have to match at once.
    let shuffled = SentenceTokenizer::with_abbreviations(["Inc.", "Dr.", "Dr."]).unwrap();
    assert_eq!(
        shuffled.tokenize_borrowed("Dr. Smith works at Acme Inc. She left."),
        two.tokenize_borrowed("Dr. Smith works at Acme Inc. She left.")
    );
}

/// The whitespace skipped before matching is Unicode `White_Space`, so
/// `U+0085` NEXT LINE counts and `U+FEFF` does not.
///
/// Those two code points are the exact complement of the ECMAScript `\s` set
/// the deleted tokenizers used, and both directions are reachable, so this is
/// the test that distinguishes the two sets rather than merely asserting one.
#[test]
fn abbreviation_matching_skips_unicode_whitespace() {
    let t = SentenceTokenizer::with_abbreviations(["Dr."]).unwrap();

    // U+0085 is White_Space. The untailored rules break after it...
    assert_eq!(
        SentenceTokenizer::new().tokenize_borrowed("Dr.\u{85}Smith left."),
        ["Dr.\u{85}", "Smith left."]
    );
    // ...and trimming it exposes "Dr.", so the boundary is suppressed. Under
    // ECMAScript `\s`, which excludes U+0085, it would not be.
    assert_eq!(
        t.tokenize_borrowed("Dr.\u{85}Smith left."),
        ["Dr.\u{85}Smith left."]
    );

    // U+FEFF is not White_Space, so it is not trimmed: the tail is
    // "Dr. \u{feff}", which does not end with "Dr.", and the boundary stands.
    // Under ECMAScript `\s`, which includes U+FEFF, it would be suppressed.
    let with_bom = "Dr. \u{feff}Smith arrived. He left.";
    assert_eq!(
        SentenceTokenizer::new().tokenize_borrowed(with_bom),
        ["Dr. \u{feff}", "Smith arrived. ", "He left."]
    );
    assert_eq!(
        t.tokenize_borrowed(with_bom),
        ["Dr. \u{feff}", "Smith arrived. ", "He left."]
    );
}

#[test]
fn an_empty_abbreviation_is_unrepresentable() {
    assert_eq!(
        SentenceTokenizer::with_abbreviations([""]),
        Err(AbbreviationError::Empty { index: 0 })
    );
    assert_eq!(
        SentenceTokenizer::with_abbreviations(["Dr.", "Inc.", ""]),
        Err(AbbreviationError::Empty { index: 2 })
    );
    // And the error says which one.
    let err = SentenceTokenizer::with_abbreviations(["a", ""]).unwrap_err();
    assert_eq!(
        err.to_string(),
        "abbreviation at index 1 is the empty string"
    );
}

#[test]
fn abbreviations_are_reported_in_supply_order() {
    let t = SentenceTokenizer::with_abbreviations(["Dr.", "Inc."]).unwrap();
    assert_eq!(t.abbreviations(), ["Dr.", "Inc."]);
    assert!(SentenceTokenizer::new().abbreviations().is_empty());
    assert_eq!(SentenceTokenizer::default(), SentenceTokenizer::new());
}

// ---------------------------------------------------------------------------
// Parallelism
// ---------------------------------------------------------------------------

#[cfg(feature = "parallel")]
#[test]
fn par_tokenize_batch_matches_a_sequential_loop_and_preserves_order() {
    use verbora_tokenizers::par_tokenize_batch;

    let sentence = SentenceTokenizer::new();
    for texts in [&[][..], &CORPUS[..1], CORPUS] {
        assert_eq!(
            par_tokenize_batch(&WordTokenizer, texts),
            texts
                .iter()
                .map(|t| WordTokenizer.tokenize_borrowed(t))
                .collect::<Vec<_>>()
        );
        assert_eq!(
            par_tokenize_batch(&SegmentTokenizer, texts),
            texts
                .iter()
                .map(|t| SegmentTokenizer.tokenize_borrowed(t))
                .collect::<Vec<_>>()
        );
        assert_eq!(
            par_tokenize_batch(&sentence, texts),
            texts
                .iter()
                .map(|t| sentence.tokenize_borrowed(t))
                .collect::<Vec<_>>()
        );
    }
}
