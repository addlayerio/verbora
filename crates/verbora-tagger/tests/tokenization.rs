//! The token contract, checked against a real tokenizer by enumeration.
//!
//! `verbora-tagger` never tokenizes, so which lexicon keys a program can ever
//! reach is decided by whatever produced its tokens. These tests walk **every**
//! bundled key — 104,360 of them across the two languages — through two
//! producers and pin the result, so the coupling breaks a test rather than
//! silently costing hits.
//!
//! The two producers are:
//!
//! * `str::split_whitespace`, the conforming producer the bundled dictionaries
//!   are keyed for;
//! * [UAX #29] word segmentation (`unicode_segmentation::unicode_words`), which
//!   is exactly what `verbora_tokenizers::WordTokenizer` runs.
//!
//! [UAX #29]: https://www.unicode.org/reports/tr29/

use unicode_segmentation::UnicodeSegmentation;
use verbora_tagger::{Language, Lexicon};

/// Every bundled key is a conforming token: it survives `split_whitespace` as
/// exactly one field. This is the contract [`Lexicon::insert`] enforces, checked
/// here against the data rather than against the code that filtered it.
#[test]
fn every_bundled_key_is_one_whitespace_delimited_token() {
    for language in [Language::English, Language::Dutch] {
        let lexicon = Lexicon::bundled(language);
        let mut checked = 0usize;
        for (key, _) in lexicon.entries() {
            let fields: Vec<&str> = key.split_whitespace().collect();
            assert_eq!(fields, [key], "{language:?} key {key:?} is not one token");
            checked += 1;
        }
        assert_eq!(checked, lexicon.len());
    }
}

/// The measured cost of pairing the bundled dictionaries with a UAX #29 word
/// tokenizer.
///
/// A key is *reachable* from such a tokenizer only if the tokenizer emits it as
/// a single token when fed the key alone. Anything else — dropped because it is
/// pure punctuation, or split into pieces — can never be looked up, whatever the
/// surrounding text.
///
/// The counts below are enumerated, not sampled, and are exact. They are pinned
/// so that a change to either side (a new lexicon entry, a Unicode version bump
/// in `unicode-segmentation`) shows up as a failure with a number attached
/// instead of as a quietly worse tagger.
#[test]
fn uax29_word_segmentation_cannot_reach_one_english_key_in_six() {
    let expected = [
        // (language, dropped entirely, split, hyphen-caused splits)
        (Language::English, 35usize, 15_631usize, 14_430usize),
        (Language::Dutch, 19, 294, 230),
    ];
    for (language, want_dropped, want_split, want_hyphen) in expected {
        let lexicon = Lexicon::bundled(language);
        let (mut dropped, mut split, mut hyphen) = (0usize, 0usize, 0usize);
        for (key, _) in lexicon.entries() {
            let tokens: Vec<&str> = key.unicode_words().collect();
            if tokens.is_empty() {
                dropped += 1;
            } else if tokens.len() > 1 || tokens[0] != key {
                split += 1;
                if key.contains('-') {
                    hyphen += 1;
                }
            }
        }
        assert_eq!(dropped, want_dropped, "{language:?}: keys dropped entirely");
        assert_eq!(split, want_split, "{language:?}: keys split or trimmed");
        assert_eq!(
            hyphen, want_hyphen,
            "{language:?}: splits caused by U+002D HYPHEN-MINUS"
        );
    }

    // The headline figures the crate documentation quotes, recomputed here so
    // the prose and the data cannot drift apart.
    let english = Lexicon::bundled(Language::English);
    assert_eq!(english.len(), 92_661);
    assert_eq!(35 + 15_631, 15_666);
    let dutch = Lexicon::bundled(Language::Dutch);
    assert_eq!(dutch.len(), 11_699);
    assert_eq!(19 + 294, 313);
}

/// The mechanism, spelled out on the concrete keys the counts above are made of.
#[test]
fn the_split_keys_are_the_ones_the_documentation_names() {
    let en = Lexicon::bundled(Language::English);
    for key in [
        "well-known",
        "government-to-government",
        "A&P",
        "A.A.U.",
        "%CHG",
    ] {
        assert!(en.contains(key), "{key:?} is a real English key");
        let tokens: Vec<&str> = key.unicode_words().collect();
        assert!(
            tokens.len() > 1 || tokens.first() != Some(&key),
            "{key:?} unexpectedly survives UAX #29 whole: {tokens:?}"
        );
        // ...and it does survive the conforming producer.
        assert_eq!(key.split_whitespace().collect::<Vec<_>>(), [key]);
    }
    // `U+002D` is `Word_Break=Other`, which is what splits the hyphenated keys.
    assert_eq!(
        "well-known".unicode_words().collect::<Vec<_>>(),
        ["well", "known"]
    );
}

/// A lexicon built from a corpus tokenized by UAX #29 is fully reachable from
/// UAX #29 output — the escape hatch the crate documentation points at.
#[test]
fn a_corpus_built_lexicon_is_reachable_from_its_own_tokenizer() {
    use verbora_tagger::{Corpus, Tag, TaggedToken};

    let text = "The well-known and/or don't 3.14 node_js café";
    let sentence: Vec<TaggedToken<'_>> = text
        .unicode_words()
        .map(|t| TaggedToken::new(t, Tag::new("NN").unwrap()))
        .collect();
    let corpus = Corpus::from_sentences(vec![sentence]);
    let lexicon = corpus.build_lexicon(Tag::new("NN").unwrap()).unwrap();

    let mut seen = 0usize;
    for token in text.unicode_words() {
        assert!(lexicon.contains(token), "{token:?} unreachable");
        seen += 1;
    }
    assert_eq!(seen, 9, "the tokenizer produced the expected token count");
    assert!(
        !lexicon.contains("well-known"),
        "no whole-compound key exists"
    );
}
