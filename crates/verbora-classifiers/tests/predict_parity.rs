//! Differential parity for the prediction path and the memos behind it.
//!
//! Three restructurings share this file, because they are only safe together
//! and only observable through the same two calls (`text_to_features` and
//! `get_classifications`):
//!
//! * **`text_to_features` is token-driven.** It used to build the enumeration
//!   order, collect the observation's tokens into a hash set and ask "is this
//!   feature present?" once per *feature*; it now asks
//!   [`OrderedMap::slot_of`](verbora_classifiers::OrderedMap::slot_of) once
//!   per *token*. The 0/1 vector must be identical bit for bit, including for
//!   tokens outside the vocabulary, repeated tokens, integer-like tokens
//!   (which hoist the whole layout) and an empty vocabulary.
//! * **The enumeration order is memoized.** `OrderedMap` computes the
//!   reference's own-property order once per entry set instead of once per
//!   call. A memo that outlived a mutation would hand a *stable* feature
//!   layout to a model whose whole documented quirk is that its layout is not
//!   stable, so the oracle here recomputes the order from insertion order on
//!   every probe and compares.
//! * **Stems are memoized per classifier.** `add_document` and `classify` now
//!   route through `Stemmer::tokenize_and_stem_cached`. A classifier built on
//!   a stemmer that does *not* implement that method re-stems every token,
//!   which is exactly the pre-change behaviour — so the same op sequence run
//!   through both must produce the same serialised classifier and the same
//!   scores, including when the process-global stop-word list is mutated
//!   between calls.
//!
//! Every comparison is on raw `f64` bits or on the serialised bytes, never on
//! formatted output, over randomized op sequences that include integer-like
//! tokens and labels, `remove_document`, `keep_stops` both ways, and probes
//! that tokenise to nothing.

use std::sync::Arc;

use verbora_classifiers::{
    BayesClassifier, BayesEngine, Engine, LogisticRegressionClassifier, OrderedMap, Stemmer,
    StemmerOf,
};
use verbora_stemmers::PorterStemmer;

/// A tiny deterministic generator, so failures reproduce from the case number.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 11
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

/// The same pool `tests/train_parity.rs` uses: integer-like strings
/// (enumeration hoisting), stop words (dropped documents), unicode,
/// punctuation-bearing words, and stems that exercise every Porter step.
const WORD_POOL: &[&str] = &[
    "running",
    "jumps",
    "quickly",
    "the",
    "and",
    "of",
    "to",
    "a",
    "in",
    "is",
    "was",
    "42",
    "7",
    "0",
    "007",
    "4294967294",
    "don't",
    "unit-tests",
    "x/y",
    "Hello",
    "WORLD",
    "MiXeD",
    "café",
    "naïve",
    "日本語",
    "…",
    "--",
    "''",
    "programmer",
    "programming",
    "programs",
    "cats",
    "relational",
    "conditionally",
    "rational",
    "valency",
    "sky",
    "syzygy",
    "ties",
    "cries",
    "agreed",
    "feed",
    "plastered",
    "bled",
    "motoring",
    "sing",
    "conflated",
    "troubled",
    "sized",
    "hopping",
    "tanned",
    "falling",
    "hissing",
    "fizzed",
    "failing",
    "filing",
    "happy",
];

fn random_text(rng: &mut Lcg, max_words: usize) -> String {
    let n = rng.below(max_words + 1);
    (0..n)
        .map(|_| WORD_POOL[rng.below(WORD_POOL.len())])
        .collect::<Vec<_>>()
        .join(" ")
}

fn random_tokens(rng: &mut Lcg, max_words: usize) -> Vec<String> {
    let n = rng.below(max_words + 1);
    (0..n)
        .map(|_| WORD_POOL[rng.below(WORD_POOL.len())].to_owned())
        .collect()
}

fn random_label(rng: &mut Lcg) -> String {
    if rng.below(4) == 0 {
        format!("{}", rng.below(50))
    } else {
        format!("class{}", rng.below(4))
    }
}

/// A stemmer that deliberately does **not** implement
/// `tokenize_and_stem_cached`, so the default (cache-ignoring) body runs.
///
/// This is the pre-memo behaviour, kept alive as an oracle: every token is
/// re-stemmed on every call, exactly as the classifier used to do.
struct Uncached(StemmerOf<PorterStemmer>);

impl Stemmer for Uncached {
    fn tokenize_and_stem(&self, text: &str, keep_stops: bool) -> Vec<String> {
        self.0.tokenize_and_stem(text, keep_stops)
    }
}

fn uncached_stemmer() -> Arc<dyn Stemmer + Send + Sync> {
    Arc::new(Uncached(StemmerOf(PorterStemmer::new())))
}

/// Whether `key` is an array index — the rule `crate::ordmap` documents,
/// restated here so the oracle does not borrow the implementation it checks.
fn is_index(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 10
        && key.bytes().all(|b| b.is_ascii_digit())
        && (key.len() == 1 || !key.starts_with('0'))
        && key.parse::<u32>().is_ok_and(|n| n != u32::MAX)
}

/// The reference's own-property enumeration order, recomputed from scratch:
/// integer-index keys in ascending numeric order, then the rest in insertion
/// order.
fn oracle_order<V>(map: &OrderedMap<V>) -> Vec<String> {
    let mut indices: Vec<(u32, String)> = Vec::new();
    let mut rest: Vec<String> = Vec::new();
    for (k, _) in map.iter_insertion() {
        if is_index(k) {
            indices.push((k.parse().expect("checked"), k.to_owned()));
        } else {
            rest.push(k.to_owned());
        }
    }
    indices.sort_by_key(|(n, _)| *n);
    let mut out: Vec<String> = indices.into_iter().map(|(_, k)| k).collect();
    out.extend(rest);
    out
}

/// The pre-change `text_to_features` tail: one membership test per *feature*,
/// over a freshly computed enumeration order.
fn oracle_features(features: &OrderedMap<f64>, tokens: &[String]) -> Vec<u8> {
    let present: Vec<&str> = tokens.iter().map(String::as_str).collect();
    oracle_order(features)
        .into_iter()
        .map(|f| u8::from(present.contains(&f.as_str())))
        .collect()
}

/// The pre-change `probabilityOfClass`: a dense `while (i--)` walk of the
/// whole observation, re-scanned for every class.
fn oracle_probability(engine: &BayesEngine, observation: &[u8], label: &str) -> f64 {
    let total = *engine.class_totals().get(label).expect("trained label");
    let features = engine.class_features().get(label).expect("trained label");
    let mut prob = 0.0;
    let mut i = observation.len();
    while i > 0 {
        i -= 1;
        if observation[i] != 0 {
            let count = features
                .get(&(i as u32))
                .copied()
                .filter(|v| *v != 0.0 && !v.is_nan())
                .unwrap_or(engine.smoothing());
            prob += verbora_classifiers::log(count / total);
        }
    }
    (total / engine.total_examples()) * verbora_classifiers::exp(prob)
}

#[test]
fn token_driven_features_match_the_per_feature_probe() {
    let mut rng = Lcg(0x0BAD_C0DE_F00D_1234);
    for _ in 0..400 {
        let mut c = BayesClassifier::new();
        if rng.below(3) == 0 {
            c.set_keep_stops(rng.below(2) == 1);
        }
        for _ in 0..1 + rng.below(10) {
            match rng.below(8) {
                0 => {
                    let t = random_text(&mut rng, 6);
                    let l = random_label(&mut rng);
                    c.remove_document(t.as_str(), &l);
                }
                1 => {
                    let tokens = random_tokens(&mut rng, 5);
                    let l = random_label(&mut rng);
                    c.add_document(&tokens, &l);
                }
                _ => {
                    let t = random_text(&mut rng, 10);
                    let l = random_label(&mut rng);
                    c.add_document(t.as_str(), &l);
                }
            }
            // Probe after every op, so a memo that survived a mutation shows
            // up as a stale layout on the very next call.
            let tokens = random_tokens(&mut rng, 8);
            assert_eq!(
                c.text_to_features(&tokens),
                oracle_features(c.features(), &tokens),
                "token-slice observation"
            );
            let text = random_text(&mut rng, 8);
            let stemmed = c.docs().last().map(|d| d.text.clone()).unwrap_or_default();
            assert_eq!(
                c.text_to_features(text.as_str()).len(),
                c.features().len(),
                "one slot per feature"
            );
            // A document's own stored tokens must light up exactly the slots
            // the per-feature probe would.
            assert_eq!(
                c.text_to_features(&stemmed),
                oracle_features(c.features(), &stemmed),
                "stored document tokens"
            );
            // And the two directions of the enumeration must agree.
            let order = oracle_order(c.features());
            assert_eq!(c.feature_order(), order, "enumeration order");
            for (slot, key) in order.iter().enumerate() {
                assert_eq!(c.features().slot_of(key), Some(slot), "slot of {key}");
            }
            assert_eq!(c.features().slot_of("\u{0}never-a-token"), None);
        }
    }
}

#[test]
fn sparse_bayes_scoring_matches_the_dense_walk() {
    let mut rng = Lcg(0x5EED_1234_9876_ABCD);
    for _ in 0..300 {
        let mut c = BayesClassifier::new();
        if rng.below(3) == 0 {
            c.set_keep_stops(rng.below(2) == 1);
        }
        for _ in 0..1 + rng.below(8) {
            let t = random_text(&mut rng, 10);
            let l = random_label(&mut rng);
            c.add_document(t.as_str(), &l);
        }
        c.train().expect("bayes train cannot fail");
        if c.engine().class_features().is_empty() {
            continue;
        }
        for _ in 0..6 {
            // Observations of every width, including narrower and wider than
            // the vocabulary, all-zero and all-one.
            let width = rng.below(2 * c.features().len() + 3);
            let observation: Vec<u8> = (0..width)
                .map(|_| match rng.below(4) {
                    0 => 0,
                    1 => 1,
                    2 => u8::try_from(rng.below(256)).expect("below 256"),
                    _ => u8::from(rng.below(2) == 1),
                })
                .collect();
            for label in c.engine().class_features().enumeration_order() {
                assert_eq!(
                    c.engine()
                        .probability_of_class(&observation, label)
                        .map(f64::to_bits),
                    Some(oracle_probability(c.engine(), &observation, label).to_bits()),
                    "probability_of_class({label})"
                );
            }
            let got = c
                .engine()
                .classifications(&observation)
                .expect("bayes never fails");
            let mut want: Vec<(String, u64)> = c
                .engine()
                .class_features()
                .enumeration_order()
                .into_iter()
                .map(|l| {
                    (
                        l.to_owned(),
                        oracle_probability(c.engine(), &observation, l).to_bits(),
                    )
                })
                .collect();
            want.sort_by(|x, y| {
                let d = f64::from_bits(y.1) - f64::from_bits(x.1);
                if d > 0.0 {
                    std::cmp::Ordering::Greater
                } else if d < 0.0 {
                    std::cmp::Ordering::Less
                } else {
                    std::cmp::Ordering::Equal
                }
            });
            let got: Vec<(String, u64)> = got
                .into_iter()
                .map(|c| (c.label, c.value.to_bits()))
                .collect();
            assert_eq!(got, want, "getClassifications");
        }
    }
}

/// Runs the same randomized script against a memoizing and a re-stemming
/// classifier and demands identical serialised bytes and identical scores.
#[test]
fn the_stem_memo_changes_no_observable_bit() {
    let mut rng = Lcg(0xFEED_FACE_CAFE_0001);
    for _ in 0..200 {
        let mut memo = BayesClassifier::new();
        let mut fresh = BayesClassifier::with_stemmer(uncached_stemmer());
        let mut logistic_memo = LogisticRegressionClassifier::new();
        let mut logistic_fresh = LogisticRegressionClassifier::with_stemmer(uncached_stemmer());
        if rng.below(3) == 0 {
            let keep = rng.below(2) == 1;
            memo.set_keep_stops(keep);
            fresh.set_keep_stops(keep);
            logistic_memo.set_keep_stops(keep);
            logistic_fresh.set_keep_stops(keep);
        }
        let mut probes: Vec<String> = Vec::new();
        for _ in 0..1 + rng.below(10) {
            let label = random_label(&mut rng);
            match rng.below(8) {
                0 => {
                    let t = random_text(&mut rng, 6);
                    memo.remove_document(t.as_str(), &label);
                    fresh.remove_document(t.as_str(), &label);
                    logistic_memo.remove_document(t.as_str(), &label);
                    logistic_fresh.remove_document(t.as_str(), &label);
                }
                _ => {
                    let t = random_text(&mut rng, 10);
                    memo.add_document(t.as_str(), &label);
                    fresh.add_document(t.as_str(), &label);
                    logistic_memo.add_document(t.as_str(), &label);
                    logistic_fresh.add_document(t.as_str(), &label);
                    probes.push(t);
                }
            }
            assert_eq!(memo.to_json(), fresh.to_json(), "vocabulary and documents");
        }
        memo.train().expect("bayes train cannot fail");
        fresh.train().expect("bayes train cannot fail");
        assert_eq!(memo.to_json(), fresh.to_json(), "trained bayes model");
        let l1 = logistic_memo.train();
        let l2 = logistic_fresh.train();
        assert_eq!(l1, l2, "logistic train outcome");
        assert_eq!(
            logistic_memo.to_json(),
            logistic_fresh.to_json(),
            "trained logistic model"
        );
        probes.push(String::new());
        probes.push(random_text(&mut rng, 12));
        for probe in &probes {
            // Run each probe twice: the first call fills the memo, the second
            // reads it, and both must agree with the re-stemming classifier.
            for _ in 0..2 {
                assert_eq!(
                    memo.text_to_features(probe.as_str()),
                    fresh.text_to_features(probe.as_str()),
                    "features for {probe:?}"
                );
                let a = memo.get_classifications(probe.as_str());
                let b = fresh.get_classifications(probe.as_str());
                match (a, b) {
                    (Ok(a), Ok(b)) => {
                        assert_eq!(a.len(), b.len());
                        for (x, y) in a.iter().zip(&b) {
                            assert_eq!(x.label, y.label);
                            assert_eq!(x.value.to_bits(), y.value.to_bits(), "{probe:?}");
                        }
                    }
                    (a, b) => assert_eq!(a.is_err(), b.is_err(), "{probe:?}"),
                }
                assert_eq!(
                    logistic_memo.classify(probe.as_str()),
                    logistic_fresh.classify(probe.as_str()),
                    "logistic label for {probe:?}"
                );
            }
        }
    }
}

/// The memo must not outlive the stop-word list it was filled under.
///
/// `tokenizeAndStem` consults the stop-word list *before* stemming, so the
/// memo — which is keyed on the token that reached the stemmer — can never
/// answer the "is this a stop word?" question. Mutating the process-global
/// list between calls must therefore keep taking effect on a classifier whose
/// memo is already warm.
#[test]
fn a_warm_memo_still_sees_stop_word_list_mutations() {
    let stemmer = PorterStemmer::new();
    // A nonsense token no other test uses, so the process-global mutation
    // this test performs cannot change any other test's tokenisation.
    let word = "zzqqxxparityprobe";
    let text = format!("{word} running cats");

    let mut warm = BayesClassifier::new();
    let mut oracle = BayesClassifier::with_stemmer(uncached_stemmer());
    for c in [&mut warm, &mut oracle] {
        c.add_document(text.as_str(), "A");
        c.add_document("jumps quickly", "B");
        c.train().expect("bayes train cannot fail");
    }
    // The memo now holds a stem for `word`, learned while it was not a stop
    // word.
    let before = warm.text_to_features(text.as_str());
    assert_eq!(before, oracle.text_to_features(text.as_str()));
    assert_eq!(before.iter().filter(|&&v| v == 1).count(), 3);

    stemmer.add_stop_word(word);
    let after_add = warm.text_to_features(text.as_str());
    assert_eq!(
        after_add,
        oracle.text_to_features(text.as_str()),
        "the newly-added stop word must drop out despite the warm memo"
    );
    assert_eq!(after_add.iter().filter(|&&v| v == 1).count(), 2);

    stemmer.remove_stop_word(word);
    assert_eq!(
        warm.text_to_features(text.as_str()),
        before,
        "removing it again restores the original observation"
    );
    assert_eq!(
        warm.text_to_features(text.as_str()),
        oracle.text_to_features(text.as_str())
    );
}

/// The enumeration memo must be dropped by every mutation that can reorder
/// the keys — which is what makes an integer-like token still able to
/// invalidate a trained model.
#[test]
fn an_integer_like_token_still_reshuffles_a_warm_layout() {
    let mut c = BayesClassifier::new();
    let alpha = vec!["alpha".to_owned()];
    let beta = vec!["beta".to_owned()];
    c.add_document(&alpha, "A");
    c.add_document(&beta, "B");
    c.train().expect("bayes train cannot fail");
    // Warm the memo through the prediction path.
    assert_eq!(c.text_to_features(&alpha), vec![1, 0]);
    assert_eq!(c.feature_order(), vec!["alpha", "beta"]);

    c.add_document(&vec!["99".to_owned()], "C");
    assert_eq!(c.feature_order(), vec!["99", "alpha", "beta"]);
    assert_eq!(c.text_to_features(&alpha), vec![0, 1, 0]);
    assert_eq!(c.features().slot_of("99"), Some(0));

    c.remove_document(&vec!["99".to_owned()], "C");
    assert_eq!(c.feature_order(), vec!["alpha", "beta"]);
    assert_eq!(c.text_to_features(&alpha), vec![1, 0]);
    assert_eq!(c.features().slot_of("99"), None);
}

/// Overwriting an existing key keeps its position, so the memo survives — and
/// the counts still move.
#[test]
fn repeated_tokens_update_counts_without_moving_slots() {
    let mut c = BayesClassifier::new();
    let doc = vec!["7".to_owned(), "alpha".to_owned(), "alpha".to_owned()];
    c.add_document(&doc, "A");
    assert_eq!(c.feature_order(), vec!["7", "alpha"]);
    assert_eq!(c.features().get("alpha"), Some(&2.0));
    c.add_document(&doc, "A");
    assert_eq!(c.feature_order(), vec!["7", "alpha"]);
    assert_eq!(c.features().get("alpha"), Some(&4.0));
    assert_eq!(c.features().get("7"), Some(&2.0));
    assert_eq!(c.text_to_features(&doc), vec![1, 1]);
}
