//! Sequential-vs-parallel parity for [`Classifier::par_classify_batch`].
//!
//! Only compiled with the `parallel` feature; with it off this test binary
//! has no items and runs zero tests.
//!
//! Every case here reruns the exact inputs already exercised by this crate's
//! own sequential test suite — nothing here is a new edge case, only a
//! side-by-side rerun through `par_classify_batch` next to the plain
//! `texts.iter().map(Classifier::classify)` loop it must match exactly:
//!
//! * the boundary categories from `tests/edge_cases.rs`'s
//!   `categories()`/`bayes_survives_every_category` (accented Latin,
//!   Cyrillic, Greek, CJK, astral-plane emoji, punctuation, digits, combining
//!   marks), given here as the raw text `classify` tokenises rather than
//!   pre-split tokens;
//! * the empty/stop-word-only inputs from
//!   `empty_and_stop_word_only_documents_vanish`, which tokenise to nothing
//!   and so exercise `ClassifierError::NotTrained` per item, not just for a
//!   whole untrained classifier;
//! * the `BayesClassifier`/`LogisticRegressionClassifier` doc example corpus
//!   from `src/basic/bayes.rs`/`src/basic/logistic.rs`.

#![cfg(feature = "parallel")]

use verbora_classifiers::{BayesClassifier, ClassifierError, LogisticRegressionClassifier};

/// The crate's own canonical fixture — the doc example on
/// [`BayesClassifier`](verbora_classifiers::BayesClassifier) and the
/// `bayes.rs` unit tests train on exactly this corpus.
fn trained_bayes() -> BayesClassifier {
    let mut c = BayesClassifier::new();
    c.add_document("my unit-tests failed.", "software");
    c.add_document("tried the program, but it was buggy.", "software");
    c.add_document("tomorrow we will do standard tests", "other");
    c.add_document("the drive has a 2TB capacity", "other");
    c.train().expect("Bayes training cannot fail");
    c
}

/// The doc example on
/// [`LogisticRegressionClassifier`](verbora_classifiers::LogisticRegressionClassifier).
fn trained_logistic() -> LogisticRegressionClassifier {
    let mut c = LogisticRegressionClassifier::new();
    c.add_document("i am long qqqq", "buy");
    c.add_document("i am short qqqq", "sell");
    c.train().expect("the corpus has examples");
    c
}

/// Runs `texts` through the sequential loop and through
/// [`Classifier::par_classify_batch`](verbora_classifiers::Classifier::par_classify_batch),
/// and asserts the two agree exactly, element for element, including which
/// ones are `Err`.
fn assert_parity<E>(classifier: &verbora_classifiers::Classifier<E>, texts: &[&str])
where
    E: verbora_classifiers::Engine + Sync,
{
    let sequential: Vec<Result<String, ClassifierError>> =
        texts.iter().map(|t| classifier.classify(*t)).collect();
    let parallel = classifier.par_classify_batch(texts);
    assert_eq!(
        sequential, parallel,
        "sequential and parallel outputs diverged for {texts:?}"
    );
}

#[test]
fn empty_batch() {
    assert_parity(&trained_bayes(), &[]);
    assert_parity(&trained_logistic(), &[]);
}

#[test]
fn one_item() {
    assert_parity(&trained_bayes(), &["did the tests pass?"]);
    assert_parity(&trained_logistic(), &["i am short qqqq"]);
}

#[test]
fn many_items() {
    let base = [
        "did the tests pass?",
        "the program is buggy",
        "tomorrow's forecast",
        "a drive with huge capacity",
    ];
    let texts: Vec<&str> = std::iter::repeat(base).flatten().take(500).collect();
    assert_parity(&trained_bayes(), &texts);
}

/// Each `rayon` worker warms its own stem memo, so a batch big enough to be
/// split across workers must still agree with a sequential loop that shares
/// the classifier's single memo — and with a run where the classifier's memo
/// is cold, warm, or contended.
///
/// The batch mixes texts that repeat (memo hits) with texts unique to one
/// position (memo misses), so both sides of every worker's memo are exercised
/// rather than only the hot path.
#[test]
fn per_worker_memos_agree_with_the_shared_one() {
    let classifier = trained_bayes();
    let unique: Vec<String> = (0..600)
        .map(|i| format!("document number {i} about testing programs and drives"))
        .collect();
    let mut texts: Vec<&str> = Vec::new();
    for (i, u) in unique.iter().enumerate() {
        texts.push(u.as_str());
        // Interleave a repeated probe, which every worker will see.
        texts.push(if i % 2 == 0 {
            "did the tests pass?"
        } else {
            "the program is buggy"
        });
    }
    // Cold: neither the shared memo nor any worker's has seen these tokens.
    assert_parity(&classifier, &texts);
    // Warm: the sequential half of `assert_parity` has now filled the shared
    // memo, so the second run compares a warm shared memo against fresh
    // per-worker ones.
    assert_parity(&classifier, &texts);
}

/// Reruns `tests/edge_cases.rs`'s `categories()` (accented Latin, Cyrillic,
/// Greek, CJK, astral-plane emoji, punctuation, digits, combining marks) as
/// text `classify` tokenises itself, instead of the pre-split token slices
/// that test uses.
#[test]
fn unicode_categories_from_edge_cases() {
    let texts = [
        "q",
        "ALLCAPS MiXeD",
        "café naïve Ångström",
        "Москва Ленинград",
        "Ελλάδα Αθήνα",
        "日本語 中文测试 한국어",
        "😀 a😀b 𝕳𝖊𝖑𝖑𝖔",
        "... ! -- don't",
        "0 42 3.14 1000",
        "e\u{301} é",
    ];
    assert_parity(&trained_bayes(), &texts);
    assert_parity(&trained_logistic(), &texts);
}

/// Reruns `tests/edge_cases.rs`'s
/// `empty_and_stop_word_only_documents_vanish` inputs, which tokenise to
/// nothing — so on a freshly-constructed (untrained) classifier every one of
/// them hits `ClassifierError::NotTrained` via `classify`, exercising the
/// error path through the fan-out rather than just the success path.
#[test]
fn empty_and_stop_word_only_texts_report_not_trained_consistently() {
    let texts = ["", " ", "   ", "\t\n\r", "the a of", "!!! ...", "x", "7"];
    let untrained = BayesClassifier::new();
    assert_parity(&untrained, &texts);
    // The same inputs against a trained classifier: they still tokenise to
    // nothing, so `text_to_features` is the empty vector and Bayes falls back
    // to the class prior for every one of them — still exercised identically
    // by both paths.
    assert_parity(&trained_bayes(), &texts);
}
