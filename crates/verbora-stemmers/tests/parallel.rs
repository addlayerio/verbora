//! Sequential-vs-parallel parity for `TokenizeAndStem::par_tokenize_and_stem_batch`.
//!
//! The whole file is gated on the `parallel` feature (see the `#![cfg]` below),
//! so it compiles to nothing — and `cargo test -p verbora-stemmers` sees zero
//! tests from it — when the feature is off. `cargo test -p verbora-stemmers
//! --features parallel` is what actually runs these.
//!
//! Every case asserts the exact same thing: the `Vec<Vec<String>>` from
//! `par_tokenize_and_stem_batch` equals, element for element and in order, the
//! `Vec<Vec<String>>` from calling `tokenize_and_stem` in a plain sequential
//! `.iter().map()` loop — the same shape the crate's docs describe the
//! sequential API as. `Vec` equality is order-sensitive, so this single
//! assertion also rules out the parallel path silently reordering documents.
//!
//! The edge cases below are not invented for this file: they are the ones the
//! crate's own `src/base.rs` unit tests and `benches/stemmers.rs`'s
//! `worst_cases` group already exercise for the sequential path (emoji, mixed
//! separators, the Indonesian reduplication/miss/dictionary-hit cases, the long
//! run that saturates Lancaster's repeating-suffix rule, and Carry's
//! `Object.prototype`-name suffixes), plus the Unicode samples from
//! `base.rs`'s `run_scanning_agrees_with_the_verified_tokenizers` test.

#![cfg(feature = "parallel")]

use verbora_stemmers::{
    CarryStemmerFr, LancasterStemmer, PorterStemmer, PorterStemmerDe, PorterStemmerRu, StemmerId,
    TokenizeAndStem,
};

// `StemmerJa` is deliberately excluded: it does not implement `TokenizeAndStem`
// at all (its tokenizer is a TinySegmenter, not the shared character-class
// splitter — see `src/ja.rs`'s module docs), so it has its own inherent
// `tokenize_and_stem` and is out of scope for a method defined on the trait.

/// Runs both paths and asserts they agree exactly, including order.
fn assert_parity<S: TokenizeAndStem + Sync>(stemmer: &S, docs: &[&str], keep_stops: bool) {
    let sequential: Vec<Vec<String>> = docs
        .iter()
        .map(|d| stemmer.tokenize_and_stem(d, keep_stops))
        .collect();
    let parallel = stemmer.par_tokenize_and_stem_batch(docs, keep_stops);
    assert_eq!(
        parallel, sequential,
        "par_tokenize_and_stem_batch diverged from the sequential loop for {docs:?} \
         (keep_stops={keep_stops})"
    );
}

#[test]
fn empty_batch() {
    assert_parity(&PorterStemmer::new(), &[], false);
    assert_parity(&LancasterStemmer::new(), &[], true);
    assert_parity(&StemmerId::new(), &[], false);
}

#[test]
fn single_document() {
    assert_parity(
        &PorterStemmer::new(),
        &["My dog is very fun to play with"],
        false,
    );
    assert_parity(&LancasterStemmer::new(), &["maximum effort"], true);
}

#[test]
fn many_documents() {
    // A few hundred documents of varying, paragraph-ish length — enough that a
    // scheduling bug (e.g. an off-by-one chunk boundary) would show up as a
    // length or content mismatch somewhere in the batch, not just at the edges.
    let sentence = "The quick brown fox jumps over the lazy dog and runs away \
                     quickly into the forest before anyone notices";
    let docs: Vec<&str> = (0..500)
        .map(|i| {
            // Vary the document length deterministically so tasks are not all
            // doing identical work.
            let words = 1 + (i % 40);
            &sentence[..sentence
                .char_indices()
                .filter(|(_, c)| *c == ' ')
                .nth(words)
                .map_or(sentence.len(), |(idx, _)| idx)]
        })
        .collect();
    assert_parity(&PorterStemmer::new(), &docs, false);
    assert_parity(&PorterStemmer::new(), &docs, true);
    assert_parity(&LancasterStemmer::new(), &docs, false);
}

#[test]
fn unicode_documents() {
    // Reuses the exact samples `base.rs`'s tokenizer-agreement test checks,
    // plus a couple more from the crate's German/Indonesian doc examples.
    let docs = [
        "",
        "   ",
        "The quick brown fox",
        "it's a-b c/d",
        "Das Haus ist schön und die Häuser sind schöner.",
        "мама мыла раму",
        "buku-buku itu dibaca",
        "a😀b emoji",
        "tabs\tand\nnewlines",
        "!!!",
        "-leading and trailing-",
    ];
    assert_parity(&PorterStemmer::new(), &docs, false);
    assert_parity(&PorterStemmerDe::new(), &docs, false);
    assert_parity(&PorterStemmerRu::new(), &docs, false);
    assert_parity(&StemmerId::new(), &docs, false);
    assert_parity(&CarryStemmerFr::new(), &docs, false);
}

#[test]
fn indonesian_pathological_words() {
    // Reduplication (multiple `stemSingularWord` passes + prefix-restore loop),
    // an unanchored-regex miss, and a dictionary hit — the three shapes
    // `worst_cases` in benches/stemmers.rs measures because their *cost*, not
    // their result, is unusual.
    let docs = [
        "meniru-nirukan kesepersepuluhnya",
        "xxberkaerat mengkritik",
        "makan buku",
    ];
    assert_parity(&StemmerId::new(), &docs, false);
    assert_parity(&StemmerId::new(), &docs, true);
}

#[test]
fn long_and_repeating_runs() {
    // The same worst-case shapes as `benches/stemmers.rs::worst_cases`: a very
    // long word (stresses the UTF-16 buffer + region scan) and a long repeating
    // suffix (stresses Lancaster's repeating-suffix rule).
    let long = "x".repeat(1000);
    let long_ing = "ing".repeat(300);
    let docs = [long.as_str(), long_ing.as_str(), "ordinary text here"];
    assert_parity(&PorterStemmer::new(), &docs, false);
    assert_parity(&LancasterStemmer::new(), &docs, false);
}

#[test]
fn carry_prototype_suffixes() {
    // The crate's one documented behavioural divergence from the reference lives in
    // these exact suffixes (see tests/parity.rs::carry_prototype); the parallel
    // path must reproduce whatever the sequential path does for them, not
    // "the correct" reference-free-of-quirks answer.
    let docs = [
        "constructor __proto__ toString valueOf",
        "xxconstructor xx__proto__ xxtoString xxvalueOf",
        "hasOwnProperty isPrototypeOf propertyIsEnumerable",
    ];
    assert_parity(&CarryStemmerFr::new(), &docs, false);
}
