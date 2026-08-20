//! The parts of the `verbora-core` trait contract this crate is the implementor
//! of.
//!
//! `Tokenizer` and `BorrowingTokenizer` promise four things that hold across
//! every implementation, whatever its boundaries are: tokens come in input
//! order, none of them is empty, empty input yields none at all, and — for
//! `BorrowingTokenizer` — every token is a contiguous slice of the input. A
//! claim on a trait is only real where the implementations are, which is here.
//!
//! # Enumerated, not sampled
//!
//! An empty token escapes at a boundary condition, not in the middle of a
//! sentence: a lone combining mark, a zero-width space, a soft hyphen, a
//! surrogate-adjacent astral scalar. Those are exactly the inputs a hand-picked
//! probe list leaves out. So this walks **every scalar in the Basic
//! Multilingual Plane** in three shapes — alone, padded with letters, and
//! doubled — through all four tokenizer configurations this crate ships, plus
//! astral samples above `U+FFFF`.

use verbora_tokenizers::{
    BorrowingTokenizer, SegmentTokenizer, SentenceTokenizer, Tokenizer, WordTokenizer,
};

/// Awkward shapes by hand, then every BMP scalar in three framings.
fn probes() -> Vec<String> {
    let mut v: Vec<String> = [
        "",
        " ",
        "  ",
        "\t\n\r",
        "\u{FEFF}",
        "a",
        "aa bb",
        "...",
        "-",
        "--",
        "a.b.c",
        "Dr. Smith went. He left.",
        "a\u{1F600}b",
        "e\u{0301}",
        "\u{0301}",
        "  a  ",
        "\n\n\n",
        "''",
        "1,000",
        "well-known",
        "and/or",
    ]
    .iter()
    .map(|s| (*s).to_owned())
    .collect();

    for u in 0u32..=0xFFFF {
        if let Some(c) = char::from_u32(u) {
            v.push(c.to_string());
            v.push(format!("a{c}b"));
            v.push(format!("{c}{c}"));
        }
    }
    for u in [0x1_0000u32, 0x1_F600, 0x10_FFFF] {
        let c = char::from_u32(u).expect("valid scalar");
        v.push(c.to_string());
        v.push(format!("a{c}b"));
    }
    v
}

fn assert_contract<T: BorrowingTokenizer>(tokenizer: &T, name: &str) {
    for probe in probes() {
        let borrowed: Vec<&str> = tokenizer.tokens(&probe).collect();
        for token in &borrowed {
            assert!(
                !token.is_empty(),
                "{name}: emitted an empty token for {probe:?}"
            );
            assert!(
                probe.contains(*token),
                "{name}: {token:?} is not a substring of {probe:?}"
            );
        }
        // The owned path is defined in terms of the borrowed one and must agree.
        assert_eq!(
            tokenizer.tokenize(&probe),
            borrowed,
            "{name}: the owned and borrowed paths disagree on {probe:?}"
        );
        assert_eq!(tokenizer.tokenize_borrowed(&probe), borrowed);
        if probe.is_empty() {
            assert!(borrowed.is_empty(), "{name}: empty input produced tokens");
        }
    }
}

#[test]
fn no_tokenizer_emits_an_empty_token_or_a_non_substring() {
    assert_contract(&WordTokenizer, "WordTokenizer");
    assert_contract(&SegmentTokenizer, "SegmentTokenizer");
    assert_contract(&SentenceTokenizer::new(), "SentenceTokenizer::new");
    assert_contract(
        &SentenceTokenizer::with_abbreviations(["Dr.", "Mr."]).expect("valid abbreviations"),
        "SentenceTokenizer::with_abbreviations",
    );
}

/// The buffer methods append rather than clear, which is the one place the
/// convention differs from `Stemmer::stem_into`.
#[test]
fn the_buffer_methods_append() {
    let mut owned = vec![String::from("kept")];
    WordTokenizer.tokenize_into("one two", &mut owned);
    assert_eq!(owned, ["kept", "one", "two"]);

    let mut borrowed = vec!["kept"];
    WordTokenizer.tokenize_borrowed_into("one two", &mut borrowed);
    assert_eq!(borrowed, ["kept", "one", "two"]);

    // ...and an empty document adds nothing rather than adding an empty token.
    WordTokenizer.tokenize_into("", &mut owned);
    assert_eq!(owned.len(), 3);
}

/// `tokenize_batch` is the same function applied per document, in order.
#[test]
fn tokenize_batch_agrees_with_tokenize() {
    let docs = ["one two", "", "  ", "three"];
    assert_eq!(
        WordTokenizer.tokenize_batch(&docs),
        docs.iter()
            .map(|d| WordTokenizer.tokenize(d))
            .collect::<Vec<_>>()
    );
}
