//! Sequential-vs-parallel parity for `SentimentAnalyzer::par_get_sentiment_batch`.
//!
//! Every case here runs the same documents through both
//! `docs.iter().map(|d| analyzer.get_sentiment(d)).collect()` (the sequential
//! primitive, called in a loop — there is no sequential *batch* method to
//! compare against) and `analyzer.par_get_sentiment_batch(&docs)`, then
//! asserts the two `Vec<f64>`s are bit-for-bit identical, `NaN` included.
//! `par_get_sentiment_batch`'s whole body is `docs.par_iter().map(get_sentiment).collect()`,
//! so any mismatch here would mean Rayon's fan-out — not this crate's own
//! logic — reordered or altered a result.
//!
//! Only compiled when the `parallel` feature is enabled:
//! `cargo test -p verbora-sentiment --features parallel`.
#![cfg(feature = "parallel")]

use verbora_sentiment::{Language, SentimentAnalyzer, Stemmer, VocabularyKind};
use verbora_stemmers::PorterStemmer;

fn afinn_en() -> SentimentAnalyzer {
    SentimentAnalyzer::without_stemmer(Language::English, VocabularyKind::Afinn)
        .expect("English AFINN exists")
}

/// Runs both paths and asserts exact (bit-for-bit, `NaN`-including) equality.
///
/// Generic over the stemmer type `S` too, so the same helper covers both the
/// default `NoStemmer` and a real one — `par_get_sentiment_batch` must work
/// for any `S: Stemmer + Sync`, not just the default. `D` is fixed to
/// `Vec<&str>` (every case in this file) rather than left generic: the
/// trait solver cannot yet infer a higher-ranked `for<'d> &'d D:
/// IntoIterator` bound from a call site, only check it once named, and this
/// file has no need for a more polymorphic helper.
fn assert_parity<S>(analyzer: &SentimentAnalyzer<S>, docs: &[Vec<&str>])
where
    S: Stemmer + Sync,
{
    let sequential: Vec<Option<f64>> = docs.iter().map(|d| analyzer.get_sentiment(d)).collect();
    let parallel = analyzer.par_get_sentiment_batch(docs);
    assert_eq!(sequential.len(), parallel.len());
    let seq_bits: Vec<Option<u64>> = sequential.iter().map(|f| f.map(f64::to_bits)).collect();
    let par_bits: Vec<Option<u64>> = parallel.iter().map(|f| f.map(f64::to_bits)).collect();
    assert_eq!(seq_bits, par_bits, "sequential vs parallel diverged");
}

#[test]
fn empty_batch() {
    let a = afinn_en();
    let docs: Vec<Vec<&str>> = vec![];
    assert_parity(&a, &docs);
    assert!(a.par_get_sentiment_batch(&docs).is_empty());
}

#[test]
fn one_document() {
    let a = afinn_en();
    // `not happy` is not a shipped key, so this is the sticky-negation path:
    // -3 for each `happy`, over three units.
    let docs = vec![vec!["not", "happy", "happy"]];
    assert_parity(&a, &docs);
    assert_eq!(a.par_get_sentiment_batch(&docs), [Some(-2.0)]);
}

#[test]
fn many_documents_preserve_order() {
    let a = afinn_en();
    // Deliberately mixes empty (NaN), single-token and multi-token documents
    // so order-preservation is checkable by more than just length.
    let docs: Vec<Vec<&str>> = (0..500)
        .map(|i| match i % 5 {
            0 => vec![],
            1 => vec!["good"],
            2 => vec!["bad"],
            3 => vec!["not", "happy"],
            _ => vec!["good", "bad", "not", "happy", "terrible"],
        })
        .collect();
    assert_parity(&a, &docs);

    let scores = a.par_get_sentiment_batch(&docs);
    // The empty document scored nothing, so it has no mean.
    assert_eq!(scores[0], None);
    assert_eq!(scores[1], Some(3.0));
    assert_eq!(scores[2], Some(-3.0));
    assert_eq!(scores[3], Some(-1.5));
}

#[test]
fn unicode_documents() {
    // Reuses `accented_latin_cyrillic_greek_and_cjk` and
    // `astral_characters_and_emoji` from `src/analyzer.rs`'s own test suite.
    let a = afinn_en();
    let es = SentimentAnalyzer::without_stemmer(Language::Spanish, VocabularyKind::Afinn).unwrap();

    let docs = vec![
        vec!["Naïve"],
        vec!["café", "Ångström", "crème"],
        vec!["Москва", "Ελλάδα", "ΑΣΤΕΙΟ"],
        vec!["日本語", "中文测试", "한국어"],
        vec!["😀", "a😀b", "𝕳𝖊𝖑𝖑𝖔"],
    ];
    assert_parity(&a, &docs);

    let es_docs = vec![vec!["SUEÑO"], vec!["ÉXITO"], vec!["😂"], vec!["no", "😂"]];
    assert_parity(&es, &es_docs);
}

#[test]
fn pathological_inputs_from_the_sequential_suite() {
    // Reuses the edge cases `src/analyzer.rs` already exercises for
    // `get_sentiment` — no new edge cases invented here, per the project's
    // parallel-feature checklist.
    let a = afinn_en();

    let long_token = "a".repeat(100_000);
    let many_good: Vec<&str> = std::iter::repeat_n("good", 10_000).collect();
    let negated_many: Vec<&str> = std::iter::once("not")
        .chain(std::iter::repeat_n("happy", 10_000))
        .collect();

    let docs: Vec<Vec<&str>> = vec![
        vec![""],
        vec!["", "", ""],
        vec!["a"],
        vec!["0"],
        vec!["NOT", "GOOD"],
        vec!["cover", "up"],
        vec!["son", "of", "a", "bitch"],
        vec!["no"],
        vec!["no", "good"],
        vec![
            "constructor",
            "toString",
            "hasOwnProperty",
            "setPrototypeOf",
        ],
        vec!["!", "...", "--", "'", "?!", "«»", "—"],
        vec!["123", "3.14", "1,000", "-0"],
        vec!["not", ".", "good"],
        vec![long_token.as_str()],
        many_good,
        negated_many,
    ];
    assert_parity(&a, &docs);
}

#[test]
fn works_when_the_stemmer_type_parameter_is_not_the_default() {
    // `S` must be `Sync` for the batch method to exist at all; this pins that
    // it is *usable*, not just that `NoStemmer` happens to compile.
    let a = SentimentAnalyzer::with_stemmer(
        Language::English,
        VocabularyKind::Afinn,
        PorterStemmer::new(),
    )
    .unwrap();
    let docs = vec![
        vec!["running", "badly"],
        vec!["nationalization"],
        vec![],
        vec!["stemmed", "greatness", "not", "awful"],
    ];
    assert_parity(&a, &docs);
}
