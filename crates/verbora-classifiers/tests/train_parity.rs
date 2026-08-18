//! Differential parity for the hoisted `train()` path and the sparse logistic
//! fit.
//!
//! `train_with` no longer calls `text_to_features` per document — it computes
//! the enumeration order and a token→slot map once per call — and the logistic
//! engine no longer materialises a dense `f64` matrix or evaluates the
//! hypothesis twice per iteration. Neither restructuring may move a single bit
//! of observable state, so these tests keep the *old* computation alive as an
//! oracle:
//!
//! * observations are rebuilt through the still-shipped per-document path
//!   (`text_to_features`, which `classify` continues to use) and fed to a
//!   shadow engine through the public [`Engine`] trait;
//! * logistic theta is recomputed by a local replica of the dense `sylvester`
//!   gradient descent, exactly as `fit()` shipped before the sparse rewrite;
//!
//! and every comparison is on raw `f64` bits, not on formatted output, over
//! randomized op-sequences that include integer-like tokens and labels
//! (enumeration hoisting), `remove_document` (feature deletion), interleaved
//! incremental `train()` calls, `keep_stops` both ways, and corpora that
//! tokenise to nothing (the error paths).

use verbora_classifiers::transcendental;
use verbora_classifiers::{
    BayesClassifier, BayesEngine, ClassifierError, Engine, LogisticEngine,
    LogisticRegressionClassifier,
};

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

/// Tokens chosen to hit the delicate paths: integer-like strings (enumeration
/// hoisting), stop words (dropped documents), unicode, punctuation-bearing
/// words, and stems that exercise every Porter step.
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

/// A label that is sometimes integer-like, to exercise class-key hoisting in
/// the engines' own `OrderedMap`s.
fn random_label(rng: &mut Lcg) -> String {
    if rng.below(4) == 0 {
        format!("{}", rng.below(50))
    } else {
        format!("class{}", rng.below(4))
    }
}

/// Every observable bit of a Bayes engine, in a canonical string.
fn bayes_engine_state(e: &BayesEngine) -> String {
    use std::fmt::Write;
    let mut s = String::new();
    for (label, counts) in e.class_features().iter_insertion() {
        write!(s, "C{label}:").unwrap();
        for (i, v) in counts {
            write!(s, "{i}={:x},", v.to_bits()).unwrap();
        }
        s.push(';');
    }
    for (label, total) in e.class_totals().iter_insertion() {
        write!(s, "T{label}={:x};", total.to_bits()).unwrap();
    }
    write!(s, "N{:x}", e.total_examples().to_bits()).unwrap();
    s
}

/// The pre-hoist training loop: one `text_to_features` call per document, fed
/// through the public [`Engine`] trait into a shadow engine.
fn slow_bayes_train(c: &BayesClassifier, engine: &mut BayesEngine, last_added: &mut usize) {
    for doc in &c.docs()[*last_added..] {
        engine.add_example(&c.text_to_features(&doc.text), &doc.label);
        *last_added += 1;
    }
    engine.fit().expect("bayes fit is a no-op");
}

#[test]
fn hoisted_bayes_train_matches_the_per_document_path() {
    let mut rng = Lcg(0xDEAD_BEEF_1234_5678);
    for case in 0..500 {
        let mut c = BayesClassifier::new();
        if rng.below(3) == 0 {
            c.set_keep_stops(rng.below(2) == 1);
        }
        let mut oracle = BayesEngine::default();
        let mut oracle_last = 0usize;
        let n_ops = 1 + rng.below(24);
        for _ in 0..n_ops {
            match rng.below(10) {
                0..=5 => {
                    let t = random_text(&mut rng, 12);
                    let l = random_label(&mut rng);
                    c.add_document(t.as_str(), &l);
                }
                6 => {
                    // Token-slice documents bypass the stemmer entirely and can
                    // introduce case-sensitive vocabulary.
                    let n = rng.below(6);
                    let tokens: Vec<String> = (0..n)
                        .map(|_| WORD_POOL[rng.below(WORD_POOL.len())].to_owned())
                        .collect();
                    let l = random_label(&mut rng);
                    c.add_document(&tokens, &l);
                }
                7 => {
                    // Interleaved incremental train: the oracle replays the
                    // pending documents against the *current* vocabulary, which
                    // is exactly what the old loop did.
                    slow_bayes_train(&c, &mut oracle, &mut oracle_last);
                    c.train().expect("bayes train cannot fail");
                }
                8 => {
                    // Deletes the matched tokens' feature slots outright, so a
                    // later train sees a narrower — and re-hoisted — layout.
                    let t = random_text(&mut rng, 6);
                    let l = random_label(&mut rng);
                    c.remove_document(t.as_str(), &l);
                }
                _ => {
                    let t = random_text(&mut rng, 12);
                    c.add_document(t.as_str(), "9");
                }
            }
        }
        slow_bayes_train(&c, &mut oracle, &mut oracle_last);
        c.train().expect("bayes train cannot fail");
        assert_eq!(
            bayes_engine_state(c.engine()),
            bayes_engine_state(&oracle),
            "case {case}: hoisted train diverged from the per-document path"
        );
        assert_eq!(c.last_added(), oracle_last, "case {case}");
    }
}

#[test]
fn integer_like_tokens_added_between_trains_shift_the_slots_identically() {
    // After the first train, adding "42" hoists it to slot 0 and shifts every
    // learned index; the second (incremental) train must build the *new*
    // documents' observations against the new layout — and only those.
    let mut c = BayesClassifier::new();
    let mut oracle = BayesEngine::default();
    let mut oracle_last = 0usize;
    c.add_document(&vec!["zebra".to_owned(), "appl".to_owned()], "A");
    slow_bayes_train(&c, &mut oracle, &mut oracle_last);
    c.train().unwrap();
    c.add_document(&vec!["42".to_owned(), "zebra".to_owned()], "B");
    slow_bayes_train(&c, &mut oracle, &mut oracle_last);
    c.train().unwrap();
    assert_eq!(c.feature_order(), vec!["42", "zebra", "appl"]);
    assert_eq!(bayes_engine_state(c.engine()), bayes_engine_state(&oracle));
}

#[test]
fn deleted_features_drop_out_of_later_observations_identically() {
    let mut c = BayesClassifier::new();
    let mut oracle = BayesEngine::default();
    let mut oracle_last = 0usize;
    let ab = vec!["alpha".to_owned(), "beta".to_owned()];
    c.add_document(&ab, "L");
    c.add_document(&vec!["gamma".to_owned(), "beta".to_owned()], "M");
    // Deletes 'alpha' and 'beta' from the vocabulary; the surviving second
    // document still *contains* 'beta', which must now silently miss.
    c.remove_document(&ab, "L");
    slow_bayes_train(&c, &mut oracle, &mut oracle_last);
    c.train().unwrap();
    assert_eq!(c.feature_order(), vec!["gamma"]);
    assert_eq!(bayes_engine_state(c.engine()), bayes_engine_state(&oracle));
}

/// The dense reference `fit()` exactly as shipped before the sparse rewrite:
/// `sylvester` row-major `f64` matrix, hypothesis evaluated both in the
/// gradient pass and again inside `cost`, contractions descending, cost sum
/// ascending. This is the oracle the sparse path must match bit-for-bit.
mod dense_reference {
    use super::*;

    fn hypothesis(theta: &[f64], examples: &[Vec<f64>]) -> Vec<f64> {
        examples
            .iter()
            .map(|row| {
                let mut sum = 0.0;
                let mut k = row.len();
                while k > 0 {
                    k -= 1;
                    sum += row[k] * theta[k];
                }
                transcendental::sigmoid(sum)
            })
            .collect()
    }

    fn cost(theta: &[f64], examples: &[Vec<f64>], y: &[f64]) -> f64 {
        let h = hypothesis(theta, examples);
        let m = examples.len();
        let mut sum = 0.0;
        for k in 0..m {
            let cost_1 = (0.0 - y[k]) * transcendental::log(h[k]);
            let cost_0 = (1.0 - y[k]) * transcendental::log(1.0 - h[k]);
            sum += cost_1 - cost_0;
        }
        (1.0 / m as f64) * sum
    }

    fn descend_gradient(
        theta_init: &[f64],
        examples: &[Vec<f64>],
        y: &[f64],
    ) -> Result<Vec<f64>, ClassifierError> {
        let m = examples.len();
        let max_it = 500 * m;
        let x: Vec<Vec<f64>> = examples
            .iter()
            .map(|row| {
                let mut r = Vec::with_capacity(row.len() + 1);
                r.push(1.0);
                r.extend_from_slice(row);
                r
            })
            .collect();
        let mut theta: Vec<f64> = theta_init.to_vec();
        theta.push(0.0);
        let n1 = theta.len();
        let mut learning_rate = 3.0f64;
        let mut learning_rate_found = false;
        let mut diff = vec![0.0; m];
        let mut gradient = vec![0.0; n1];
        while !learning_rate_found && learning_rate != 0.0 {
            let mut i = 0usize;
            let mut last = 0.0f64;
            loop {
                let h = hypothesis(&theta, &x);
                for k in 0..m {
                    diff[k] = h[k] - y[k];
                }
                for (col, g) in gradient.iter_mut().enumerate() {
                    let mut sum = 0.0;
                    let mut r = m;
                    while r > 0 {
                        r -= 1;
                        sum += x[r][col] * diff[r];
                    }
                    *g = sum;
                }
                for k in 0..n1 {
                    theta[k] -= (gradient[k] * (1.0 / m as f64)) * learning_rate;
                }
                let current = cost(&theta, &x, y);
                i += 1;
                if last != 0.0 && !last.is_nan() {
                    if current < last {
                        learning_rate_found = true;
                    } else {
                        break;
                    }
                    if last - current < 0.0001 {
                        break;
                    }
                }
                if i >= max_it {
                    return Err(ClassifierError::UnableToFindMinimum);
                }
                last = current;
            }
            learning_rate /= 3.0;
        }
        theta.remove(0);
        Ok(theta)
    }

    /// The dense `fit()`, run against a recorded engine's examples.
    pub fn fit(e: &LogisticEngine) -> Result<Vec<Vec<f64>>, ClassifierError> {
        let num_classes = e.examples().len();
        let mut targets = vec![vec![0.0f64; num_classes]; e.example_count()];
        let mut matrix: Vec<Vec<f64>> = Vec::with_capacity(e.example_count());
        let mut d = 0usize;
        for (c, label) in e.examples().enumeration_order().into_iter().enumerate() {
            for row in e.examples().get(label).expect("key came from this map") {
                matrix.push(row.iter().map(|&b| f64::from(b)).collect());
                targets[d][c] = 1.0;
                d += 1;
            }
        }
        if matrix.is_empty() {
            return Err(ClassifierError::NoExamples);
        }
        let width = matrix[0].len();
        let zeros = vec![0.0f64; width];
        let mut theta = Vec::with_capacity(e.classifications().len());
        for i in 0..e.classifications().len() {
            let column: Vec<f64> = targets.iter().map(|row| row[i]).collect();
            theta.push(descend_gradient(&zeros, &matrix, &column)?);
        }
        Ok(theta)
    }
}

/// The recorded (pre-fit) side of a logistic engine, in a canonical string.
fn logistic_examples_state(e: &LogisticEngine) -> String {
    use std::fmt::Write;
    let mut s = String::new();
    for (label, rows) in e.examples().iter_insertion() {
        write!(s, "E{label}:{rows:?};").unwrap();
    }
    write!(s, "C{:?};N{}", e.classifications(), e.example_count()).unwrap();
    s
}

fn theta_bits(theta: &[Vec<f64>]) -> Vec<Vec<u64>> {
    theta
        .iter()
        .map(|t| t.iter().map(|v| v.to_bits()).collect())
        .collect()
}

#[test]
fn sparse_logistic_fit_matches_the_dense_reference() {
    let mut rng = Lcg(0xFACE_FEED_0BAD_F00D);
    for case in 0..200 {
        let mut c = LogisticRegressionClassifier::new();
        if rng.below(3) == 0 {
            c.set_keep_stops(rng.below(2) == 1);
        }
        // `below(15)` includes 0 documents, which must reproduce the
        // `NoExamples` error path on both sides.
        let n_docs = rng.below(15);
        for _ in 0..n_docs {
            let t = random_text(&mut rng, 8);
            let l = if rng.below(5) == 0 {
                format!("{}", rng.below(9))
            } else {
                format!("class{}", rng.below(3))
            };
            c.add_document(t.as_str(), &l);
        }

        // Oracle engine: the per-document observation path, recorded through
        // the public trait — this checks the hoisted logistic re-add loop.
        let mut oracle = LogisticEngine::default();
        for doc in c.docs() {
            oracle.add_example(&c.text_to_features(&doc.text), &doc.label);
        }
        let expected = dense_reference::fit(&oracle);

        let got = c.train();
        match (&expected, &got) {
            (Ok(theta), Ok(())) => {
                assert_eq!(
                    logistic_examples_state(c.engine()),
                    logistic_examples_state(&oracle),
                    "case {case}: hoisted re-add diverged from the per-document path"
                );
                assert_eq!(
                    theta_bits(c.engine().theta().expect("trained")),
                    theta_bits(theta),
                    "case {case}: sparse descent diverged from the dense reference"
                );
            }
            (Err(a), Err(b)) => assert_eq!(a, b, "case {case}: error paths diverged"),
            _ => panic!("case {case}: oracle {expected:?} vs shipped {got:?}"),
        }
    }
}

#[test]
fn retraining_after_new_vocabulary_matches_the_dense_reference() {
    // A second train() after the vocabulary grew re-adds *every* document
    // against the widened, re-hoisted layout — the RESETS_ON_TRAIN path.
    let mut c = LogisticRegressionClassifier::new();
    c.add_document("i am long qqqq", "buy");
    c.add_document("i am short qqqq", "sell");
    c.train().unwrap();
    c.add_document("the 42 index dropped today", "sell");
    c.train().unwrap();

    let mut oracle = LogisticEngine::default();
    for doc in c.docs() {
        oracle.add_example(&c.text_to_features(&doc.text), &doc.label);
    }
    let expected = dense_reference::fit(&oracle).expect("fits");
    assert_eq!(
        logistic_examples_state(c.engine()),
        logistic_examples_state(&oracle)
    );
    assert_eq!(
        theta_bits(c.engine().theta().expect("trained")),
        theta_bits(&expected)
    );
}
