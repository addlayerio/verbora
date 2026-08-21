//! The token contract, and the tokenizer coupling it makes explicit.
//!
//! `verbora-tagger` never tokenizes, so which lexicon keys a program can ever
//! reach is decided by whatever produced its tokens. That is a real trap and it
//! fails silently — a key the tokenizer never emits is not an error, it is just
//! an entry that never matches — so both halves of it are demonstrated here:
//!
//! * the **mismatch**: a lexicon keyed by whitespace-delimited corpus tokens
//!   paired with a [UAX #29] word tokenizer, which splits inside those keys;
//! * the **matched pair**: a lexicon built with
//!   `Corpus::build_lexicon` from a corpus tokenized by the same producer that
//!   will tokenize the text, where every key is reachable by construction.
//!
//! The tokenizer used below is `unicode_segmentation::unicode_words`, which is
//! exactly what `verbora_tokenizers::WordTokenizer` runs.
//!
//! [UAX #29]: https://www.unicode.org/reports/tr29/

use unicode_segmentation::UnicodeSegmentation;
use verbora_tagger::{Corpus, Lexicon, Tag, TaggedToken};

/// Keys of the shape a whitespace-delimited corpus produces: compounds,
/// abbreviations and symbol-bearing tokens that a UAX #29 tokenizer will not
/// hand back whole.
const CORPUS_STYLE_KEYS: &[&str] = &[
    "well-known",
    "government-to-government",
    "A&P",
    "A.A.U.",
    "%CHG",
    "Asia/Pacific",
];

fn tag(s: &'static str) -> Tag {
    Tag::new(s).expect("a conforming tag")
}

/// Every key a lexicon accepts is a conforming token: it survives
/// `split_whitespace` as exactly one field. This is the contract
/// `Lexicon::insert` enforces, checked here on the way back out.
#[test]
fn every_key_is_one_whitespace_delimited_token() {
    let mut lexicon = Lexicon::new(tag("NN"));
    for key in CORPUS_STYLE_KEYS {
        lexicon
            .insert(key, vec![tag("NN")])
            .expect("a conforming key");
    }
    let mut checked = 0usize;
    for (key, _) in lexicon.entries() {
        let fields: Vec<&str> = key.split_whitespace().collect();
        assert_eq!(fields, [key], "key {key:?} is not one token");
        checked += 1;
    }
    assert_eq!(checked, lexicon.len());

    // ...and a key that is not one token cannot be inserted at all.
    assert!(lexicon.insert("two words", vec![tag("NN")]).is_err());
    assert!(lexicon.insert("", vec![tag("NN")]).is_err());
}

/// The mismatch, on the concrete keys that cause it.
///
/// A key is *reachable* from a tokenizer only if the tokenizer emits it as a
/// single token when fed the key alone. Anything else — dropped because it is
/// pure punctuation, or split into pieces — can never be looked up, whatever the
/// surrounding text. Every key below fails that test under UAX #29 and passes it
/// under `split_whitespace`, which is the whole of the coupling.
#[test]
fn corpus_style_keys_are_unreachable_from_a_uax29_tokenizer() {
    let mut lexicon = Lexicon::new(tag("NN"));
    for key in CORPUS_STYLE_KEYS {
        lexicon
            .insert(key, vec![tag("JJ")])
            .expect("a conforming key");
    }

    for key in CORPUS_STYLE_KEYS {
        assert!(lexicon.contains(key), "{key:?} is a key of this lexicon");
        let tokens: Vec<&str> = key.unicode_words().collect();
        assert!(
            tokens.len() > 1 || tokens.first() != Some(key),
            "{key:?} unexpectedly survives UAX #29 whole: {tokens:?}"
        );
        // ...and it does survive the conforming producer.
        assert_eq!(key.split_whitespace().collect::<Vec<_>>(), [*key]);
    }

    // `U+002D` is `Word_Break=Other`, which is what splits the hyphenated keys —
    // the single most common cause, since hyphenated compounds are common in
    // corpus vocabularies.
    assert_eq!(
        "well-known".unicode_words().collect::<Vec<_>>(),
        ["well", "known"]
    );

    // The consequence, stated as behaviour rather than as arithmetic: tagging
    // the UAX #29 tokenization of a sentence never reaches the compound entry,
    // so its `JJ` is never assigned and both halves take the default.
    let text = "a well-known problem";
    for token in text.unicode_words() {
        assert_ne!(
            lexicon.tag_of(token),
            tag("JJ"),
            "{token:?} unexpectedly reached a compound entry"
        );
    }
    assert_eq!(
        lexicon.tag_of("well-known"),
        tag("JJ"),
        "whole, it is found"
    );
}

/// The matched pair: a lexicon built from a corpus tokenized by UAX #29 is fully
/// reachable from UAX #29 output — the escape hatch the crate documentation
/// points at, and the one this crate now recommends by default.
#[test]
fn a_corpus_built_lexicon_is_reachable_from_its_own_tokenizer() {
    let text = "The well-known and/or don't 3.14 node_js café";
    let sentence: Vec<TaggedToken<'_>> = text
        .unicode_words()
        .map(|t| TaggedToken::new(t, tag("NN")))
        .collect();
    let corpus = Corpus::from_sentences(vec![sentence]);
    let lexicon = corpus.build_lexicon(tag("NN")).expect("conforming tokens");

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
