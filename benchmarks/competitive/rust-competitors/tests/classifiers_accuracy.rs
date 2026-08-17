//! ACCURACY + PERFORMANCE for `benches/classifiers.rs`'s Naive Bayes rows.
//!
//! Speed alone is not enough for a classifier comparison — the spec's own
//! `ACCURACY + PERFORMANCE` section is explicit that "a library 10× faster
//! but substantially less correct" must not be presented as simply faster.
//! This file trains Verbora's `BayesClassifier`, smartcore's
//! `MultinomialNB` and linfa-bayes's `MultinomialNb` at every size in the
//! shared, signal-bearing `benches/data/classification-corpus.json` (see
//! `tools/bench-data/generate.py`'s own comment for how it was built and why
//! it is synthetic-but-lexically-real rather than scraped), and scores each
//! against that file's one fixed 128-document held-out test set.
//!
//! `classifier` (jackm321) 0.0.3 is absent from this comparison, same reason
//! as `benches/classifiers.rs`: verified not to compile on this workspace's
//! toolchain (see that file's own doc comment for the exact error).
//!
//! Run with `cargo test -p competitive-rust --test classifiers_accuracy --
//! --nocapture` to see the accuracy-by-training-size table on stdout.
//!
//! A `tests/*.rs` binary cannot import private items from a `[[bench]]`
//! target, so `Vocab`/`label helpers` below are a deliberate, minimal
//! duplicate of `benches/classifiers.rs`'s own — see that file's doc comment
//! for the full rationale (same "raw text in, trained model out" boundary as
//! Verbora's own `add_document`+`train()`).

use std::collections::HashMap;

use ndarray::Array2;
use smartcore::linalg::basic::matrix::DenseMatrix;
use smartcore::naive_bayes::multinomial::MultinomialNB;
use verbora_classifiers::BayesClassifier;

#[derive(serde::Deserialize)]
struct Doc {
    text: String,
    label: String,
}

#[derive(serde::Deserialize)]
struct Corpus {
    classes: Vec<String>,
    sizes: Vec<usize>,
    train: HashMap<String, Vec<Doc>>,
    test: Vec<Doc>,
}

fn load_corpus() -> Corpus {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("repo root is 3 levels up from rust-competitors/")
        .join("benches/data/classification-corpus.json");
    let body = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "cannot read {}: {e}\nGenerate it with: python3 tools/bench-data/generate.py",
            path.display()
        )
    });
    serde_json::from_str(&body).expect("valid classification-corpus.json")
}

fn class_id(classes: &[String], label: &str) -> u32 {
    classes
        .iter()
        .position(|c| c == label)
        .expect("label is one of the corpus's own classes") as u32
}

/// Whitespace-split, lowercased vocabulary + count-matrix builder — the same
/// deliberately-simpler-than-Verbora adapter `benches/classifiers.rs` uses,
/// duplicated here (see that file's `Vocab` doc comment for the full
/// rationale: no stemming, no stop-word filtering, a real documented
/// difference beyond the pre-built-matrix architecture gap).
struct Vocab {
    index: HashMap<String, usize>,
}

impl Vocab {
    fn build(docs: &[Doc]) -> Self {
        let mut index = HashMap::new();
        for doc in docs {
            for tok in doc.text.split_whitespace() {
                let next_id = index.len();
                index.entry(tok.to_lowercase()).or_insert(next_id);
            }
        }
        Self { index }
    }

    fn len(&self) -> usize {
        self.index.len()
    }

    fn row(&self, text: &str) -> Vec<u32> {
        let mut row = vec![0u32; self.index.len()];
        for tok in text.split_whitespace() {
            if let Some(&id) = self.index.get(&tok.to_lowercase()) {
                row[id] += 1;
            }
        }
        row
    }

    fn matrix(&self, docs: &[Doc]) -> Vec<Vec<u32>> {
        docs.iter().map(|d| self.row(&d.text)).collect()
    }
}

/// Trains Verbora, smartcore and linfa-bayes at every corpus size and
/// reports accuracy against the shared fixed 128-document test set.
#[test]
fn accuracy_report() {
    let corpus = load_corpus();
    println!(
        "\n{:<12}{:>8}{:>18}{:>18}{:>18}",
        "size", "n_test", "verbora", "smartcore", "linfa_bayes"
    );

    let mut last_verbora_acc = 0.0;
    for &n in &corpus.sizes {
        let train = &corpus.train[&n.to_string()];

        // Verbora: tokenizes/stems/filters stop words internally.
        let mut verbora = BayesClassifier::new();
        for doc in train {
            verbora.add_document(doc.text.as_str(), doc.label.as_str());
        }
        verbora.train().expect("Bayes training cannot fail");
        let verbora_correct = corpus
            .test
            .iter()
            .filter(|doc| verbora.classify(doc.text.as_str()).as_deref() == Ok(doc.label.as_str()))
            .count();

        // smartcore / linfa: vocabulary fit on `train` only (no test-set
        // leakage) — the discipline a real evaluation requires.
        let vocab = Vocab::build(train);
        let train_rows = vocab.matrix(train);
        let train_labels: Vec<u32> = train
            .iter()
            .map(|d| class_id(&corpus.classes, &d.label))
            .collect();
        let refs: Vec<&[u32]> = train_rows.iter().map(Vec::as_slice).collect();
        let x = DenseMatrix::<u32>::from_2d_array(&refs).expect("rectangular matrix");
        let sm_model = MultinomialNB::fit(&x, &train_labels, Default::default()).expect("fits");
        let sm_correct = corpus
            .test
            .iter()
            .filter(|doc| {
                let row = vocab.row(&doc.text);
                let xt = DenseMatrix::<u32>::from_2d_array(&[row.as_slice()]).expect("one row");
                let pred = sm_model.predict(&xt).expect("predicts")[0];
                pred == class_id(&corpus.classes, &doc.label)
            })
            .count();

        let flat: Vec<f64> = train_rows.iter().flatten().map(|&v| f64::from(v)).collect();
        let lx = Array2::from_shape_vec((train_rows.len(), vocab.len()), flat)
            .expect("rectangular matrix");
        // linfa 0.8.1's `Label` trait is implemented for `usize` (and `bool`,
        // `String`, `&str`, `()`) but not `i32`/`u32` — see
        // `linfa-0.8.1/src/dataset/mod.rs`.
        let ly: Vec<usize> = train
            .iter()
            .map(|d| class_id(&corpus.classes, &d.label) as usize)
            .collect();
        let lf_correct = {
            use linfa::prelude::*;
            use linfa_bayes::MultinomialNbParams;
            let ds = DatasetView::new(lx.view(), ndarray::ArrayView1::from(&ly));
            let model = MultinomialNbParams::new().fit(&ds).expect("fits");
            corpus
                .test
                .iter()
                .filter(|doc| {
                    let row = vocab.row(&doc.text);
                    let xt = Array2::from_shape_vec(
                        (1, vocab.len()),
                        row.iter().map(|&v| f64::from(v)).collect(),
                    )
                    .expect("one row");
                    let pred = model.predict(&xt)[0];
                    pred == class_id(&corpus.classes, &doc.label) as usize
                })
                .count()
        };

        let total = corpus.test.len();
        println!(
            "{:<12}{:>8}{:>17.1}%{:>17.1}%{:>17.1}%",
            n,
            total,
            100.0 * verbora_correct as f64 / total as f64,
            100.0 * sm_correct as f64 / total as f64,
            100.0 * lf_correct as f64 / total as f64,
        );
        last_verbora_acc = verbora_correct as f64 / total as f64;
    }

    // Sanity floor, not a tight assertion: random guessing across
    // `corpus.classes.len()` balanced classes scores ~1/len(); a working
    // classifier trained on the full corpus must clear that by a wide
    // margin.
    let random_baseline = 1.0 / corpus.classes.len() as f64;
    assert!(
        last_verbora_acc > random_baseline * 2.0,
        "Verbora's accuracy at the largest training size ({last_verbora_acc}) did not \
         clear twice the random baseline ({random_baseline}) — likely a harness bug, not \
         a real accuracy finding"
    );
}
