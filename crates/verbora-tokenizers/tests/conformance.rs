//! UAX #29 conformance: the UCD's own break-test files, replayed as data.
//!
//! `tests/data/WordBreakTest.txt` and `tests/data/SentenceBreakTest.txt` are
//! taken verbatim from the Unicode Character Database for the version this
//! crate is built against (17.0.0), and are redistributed under the Unicode
//! terms of use recorded in each file's own header
//! (<https://www.unicode.org/terms_of_use.html>).
//!
//! This is what pins the Unicode version Verbora claims. A dependency upgrade
//! that moves any boundary fails here, loudly and by name, which is the
//! mechanism the "stamp your persisted keys" obligation in the crate
//! documentation depends on — a silent boundary change is exactly the failure
//! that corrupts an index built before the upgrade.
//!
//! Both files reach the input classes no hand-written fixture set does: the
//! whole `Extend`/`Format`/`ZWJ` machinery of WB4, regional-indicator pairs
//! (WB15/WB16), and ZWJ emoji sequences (WB3c) — the places a boundary
//! implementation drops or duplicates text.

use verbora_tokenizers::{BorrowingTokenizer, SegmentTokenizer, SentenceTokenizer};

/// One parsed row: the sample string, and the byte offsets at which the file
/// says a break occurs (excluding the leading `0`).
struct Case {
    text: String,
    breaks: Vec<usize>,
    line: usize,
    source: String,
}

/// Parses a UCD break-test file.
///
/// Each data line is a `÷`/`×`-separated sequence of hex code points, `÷`
/// marking a break opportunity and `×` marking its absence, terminated by `÷`
/// and optionally followed by a `#` comment.
fn parse(body: &str) -> Vec<Case> {
    let mut cases = Vec::new();
    for (i, raw) in body.lines().enumerate() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let mut text = String::new();
        let mut breaks = Vec::new();
        for token in line.split_whitespace() {
            match token {
                "÷" => {
                    if !text.is_empty() {
                        breaks.push(text.len());
                    }
                }
                "×" => {}
                hex => {
                    let cp = u32::from_str_radix(hex, 16)
                        .unwrap_or_else(|e| panic!("line {}: bad code point {hex:?}: {e}", i + 1));
                    let c = char::from_u32(cp).unwrap_or_else(|| {
                        panic!("line {}: {hex:?} is not a Unicode scalar value", i + 1)
                    });
                    text.push(c);
                }
            }
        }
        cases.push(Case {
            text,
            breaks,
            line: i + 1,
            source: raw.split('#').next().unwrap_or("").trim().to_owned(),
        });
    }
    assert!(!cases.is_empty(), "the conformance file parsed to nothing");
    cases
}

/// The byte offsets at which `tokens` ends a segment, excluding the leading
/// `0` and including the final `text.len()` — the same convention the test
/// files use for their interior and trailing `÷`.
fn boundaries<'a>(tokens: impl Iterator<Item = &'a str>) -> Vec<usize> {
    let mut out = Vec::new();
    let mut at = 0;
    for tok in tokens {
        assert!(!tok.is_empty(), "a tokenizer yielded an empty segment");
        at += tok.len();
        out.push(at);
    }
    out
}

fn word_cases() -> Vec<Case> {
    parse(include_str!("data/WordBreakTest.txt"))
}

fn sentence_cases() -> Vec<Case> {
    parse(include_str!("data/SentenceBreakTest.txt"))
}

#[test]
fn segment_tokenizer_matches_word_break_test() {
    let cases = word_cases();
    assert!(
        cases.len() > 1000,
        "expected the full file, got {}",
        cases.len()
    );
    for case in cases {
        let got = boundaries(SegmentTokenizer.tokens(&case.text));
        assert_eq!(
            got, case.breaks,
            "WordBreakTest.txt line {}: {}",
            case.line, case.source
        );
    }
}

#[test]
fn sentence_tokenizer_matches_sentence_break_test() {
    let cases = sentence_cases();
    assert!(
        cases.len() > 400,
        "expected the full file, got {}",
        cases.len()
    );
    let tokenizer = SentenceTokenizer::new();
    for case in cases {
        let got = boundaries(tokenizer.tokens(&case.text));
        assert_eq!(
            got, case.breaks,
            "SentenceBreakTest.txt line {}: {}",
            case.line, case.source
        );
    }
}

/// Concatenation must reproduce every conformance sample exactly, for both
/// boundary tokenizers. The samples are where text gets dropped or duplicated
/// if a rule mishandles `Extend`, `Format` or `ZWJ`.
#[test]
fn conformance_samples_round_trip() {
    let sentence = SentenceTokenizer::new();
    for case in word_cases() {
        assert_eq!(
            SegmentTokenizer.tokens(&case.text).collect::<String>(),
            case.text,
            "WordBreakTest.txt line {}",
            case.line
        );
    }
    for case in sentence_cases() {
        assert_eq!(
            sentence.tokens(&case.text).collect::<String>(),
            case.text,
            "SentenceBreakTest.txt line {}",
            case.line
        );
    }
}

/// The Unicode version the boundaries above were checked against, stated in
/// the crate's own rustdoc and stamped into anything that persists
/// tokenizer-derived keys.
///
/// This is the tripwire for a dependency bump: the conformance data in
/// `tests/data/` is version-specific, so a change here without a matching data
/// refresh means the suite above is silently checking the wrong standard.
#[test]
fn unicode_version_matches_the_conformance_data() {
    assert_eq!(
        verbora_tokenizers::unicode_version(),
        (17, 0, 0),
        "tests/data/*BreakTest.txt are the 17.0.0 files; refresh them from \
         https://www.unicode.org/Public/<version>/ucd/auxiliary/ and update \
         the version recorded in the crate documentation"
    );
}
