//! Verbora vs. real, pinned third-party Rust competitors — classifiers.
//!
//! See `docs/COMPETITIVE_BENCHMARKS.md` §1.13 for the full research dossier.
//! Both Naive Bayes and Logistic Regression are benchmarked against Rust
//! competitors here. No MaxEnt row: the matrix confirms **NO FAIR COMPETITOR
//! FOUND** in the Rust ecosystem for generalised iterative scaling.
//!
//! ## Naive Bayes competitors
//!
//! - **`classifier` (jackm321/Rust_Classifier) 0.0.3` is NOT pinned or
//!   benchmarked here**, despite the matrix marking it Yes/Yes (closest
//!   text-in/text-out API match). Verified during implementation that it does
//!   not compile on this workspace's pinned toolchain: its `Classifier`
//!   struct derives `RustcDecodable`/`RustcEncodable` from `rustc-serialize`,
//!   which needed the pre-1.15 compiler-plugin custom-derive mechanism long
//!   since removed from stable Rust (`rustc-serialize` itself has had no
//!   release since 2016 and never migrated to proc-macro derives). A minimal
//!   probe crate depending on `classifier = "=0.0.3"` alone fails to build
//!   with `error[E0277]: the trait bound Classifier: Encodable is not
//!   satisfied`. This is a real, reproducible build failure on the required
//!   toolchain, not a version-pinning choice — the matrix's optimistic
//!   build assessment was not re-verified by actually compiling it; this
//!   pass did, and found it broken.
//! - **smartcore 0.6.5** `naive_bayes::multinomial::MultinomialNB` and
//!   **linfa-bayes 0.8.1** `MultinomialNb` both operate on a pre-built dense
//!   count matrix, not raw text (matrix: Partial, selected cases). Verbora's
//!   own `BayesClassifier::add_document`/`.train()` tokenizes internally,
//!   *inside* the timed calls (see `crates/verbora-classifiers/benches/
//!   classifiers.rs`'s own `bayes_training`, which times `add_document` +
//!   `train()` together over raw `String`s) — to keep the same "raw text in,
//!   trained model out" boundary on every side, this file's smartcore/linfa
//!   adapter tokenizes and builds its vocabulary + count matrix *inside* the
//!   timed closure too (see [`Vocab::build`] and [`bayes_train`]), not ahead
//!   of it. The vectorizer itself (whitespace split, no stemming, no
//!   stop-word filtering) is real, additional documented daylight beyond
//!   the pre-built-matrix caveat: Verbora's own tokenizer stems and drops
//!   stop words, this one does neither — see [`Vocab`]'s own doc comment.
//! - `linfa-bayes` 0.8.1's `MultinomialNb::fit_with` contains a leftover
//!   `dbg!` call in its own published source
//!   (`linfa-bayes-0.8.1/src/multinomial_nb.rs:78`) that writes to stderr
//!   once per class on every `.fit()` call — confirmed by reading the crate
//!   source directly, not assumed. That I/O is intrinsic to calling the
//!   published crate as a normal user would (there is no way to disable it
//!   from outside the crate), so it is left in rather than patched around,
//!   and flagged here so a reader is not surprised by console spew during
//!   `cargo bench`.
//! - **naivebayes (ruivieira) 0.1.2** takes pre-tokenized `Vec<String>`
//!   input — [`tokenize`] does the same whitespace-split-and-lowercase job
//!   [`Vocab::build`] does for smartcore/linfa-bayes, so all three Bayes
//!   competitors do the identical, documented-simpler-than-Verbora amount of
//!   preprocessing. Its smoothing is not count-based additive smoothing at
//!   all: reading its published source
//!   (`naivebayes-0.1.2/src/lib.rs`'s `calculate_attr_prob`) shows an
//!   attribute unseen *under a given label* (but seen under some other
//!   label) gets a **fixed** `minimum_probability = 1e-9`, regardless of how
//!   much training data exists — never Verbora's
//!   `(count + smoothing) / (total + smoothing * |V|)`, which shrinks toward
//!   zero as the corpus grows. `tests/classifiers_naivebayes_logistic.rs`'s
//!   `naivebayes_smoothing_floor_is_fixed_not_count_based` demonstrates this
//!   concretely (not just asserted from reading the source) by showing the
//!   returned probability for an attribute unseen-under-label is exactly
//!   `1e-9`-scaled regardless of that label's document count. This is a real
//!   smoothing-*mechanics* difference, not just an API one — like smartcore
//!   and linfa-bayes above, benchmarked here for **speed only**, never
//!   output-value agreement beyond the narrow "same argmax on a clear-cut,
//!   unambiguous case" domain that same test file's
//!   `naivebayes_agrees_with_verbora_on_a_clear_cut_case` checks.
//!
//! ## Logistic Regression competitors
//!
//! - **smartcore 0.6.5** `linear::logistic_regression::LogisticRegression`
//!   and **linfa-logistic 0.8.1** `MultiLogisticRegression` are both dense-
//!   matrix/no-text-pipeline, exactly like their Naive Bayes siblings above —
//!   same [`Vocab`] tokenize+vectorize adapter, built inside the timed
//!   region, reused as-is. Both use a joint/softmax multiclass strategy via
//!   LBFGS, a different optimizer *and* a different multiclass strategy than
//!   Verbora's one-vs-rest plain gradient descent (`crates/
//!   verbora-classifiers/src/basic/logistic.rs`) — matrix: Partial, selected
//!   cases.
//! - **rustlearn 0.5.0** `linear_models::sgdclassifier::SGDClassifier`,
//!   wrapped in `multiclass::OneVsRestWrapper` — SGD (Adagrad) per
//!   one-vs-rest binary sub-model, the closest multiclass *strategy* match to
//!   Verbora's own one-vs-rest gradient descent of the three Logistic
//!   Regression competitors, but the matrix's own explicitly flagged
//!   **weakest/lowest-priority** candidate: unmaintained since 2018 (last
//!   push 2018-07-29). Included anyway per the spec's "do not limit to one
//!   rival, even a weak one, when the matrix already selected it" policy.
//!   Its own dense `Array` type only holds `f32`, so [`Vocab`]'s `u32` counts
//!   are cast down (matrix: Partial, selected cases; own optimizer, own
//!   float width — never an output-value comparison, same discipline as the
//!   other two).
//! - Corpus sizes for the Logistic Regression group are deliberately much
//!   smaller than the Bayes group's (`LOGISTIC_SIZES` vs. `SIZES`): Verbora's
//!   own gradient descent runs to convergence per class
//!   (`crates/verbora-classifiers/benches/classifiers.rs`'s own
//!   `logistic_training` already restricts itself to `[4, 8, 16]` documents,
//!   `words=8`, `vocabulary=40`, `classes=3` for exactly this reason — this
//!   file reuses that identical shape via [`logistic_corpus`] so the two
//!   suites' Verbora-side numbers stay comparable) — the competitors below
//!   are fast enough at these sizes that they are not the bottleneck.
//!
//! ## Corpus
//!
//! `bayes_train`/`bayes_predict` use a byte-for-byte port^Wcopy of
//! `crates/verbora-classifiers/benches/classifiers.rs`'s own `Lcg`/`corpus()`
//! generator (identical multiplier, increment, seed, shift) so Verbora's own
//! numbers here are directly comparable to that in-workspace bench's numbers
//! for the same corpus shape — this is shape-only synthetic data (see that
//! file's own doc comment for why: cost depends on document/vocabulary shape,
//! not content), so it is **not** used for accuracy. `logistic_train`/
//! `logistic_predict` reuse the identical `Lcg`, through [`logistic_corpus`],
//! at the much smaller sizes that same in-workspace file's own
//! `logistic_training` bench already established as tractable for gradient
//! descent to convergence (see this file's own "Logistic Regression
//! competitors" section above).
//!
//! ## Accuracy and correctness
//!
//! A real accuracy comparison needs real signal, which shape-only data does
//! not have. `tests/classifiers_accuracy.rs`'s `accuracy_report` (`cargo test
//! -p competitive-rust --test classifiers_accuracy -- --nocapture`) reads the
//! shared, signal-bearing `benches/data/classification-corpus.json`
//! (`tools/bench-data/generate.py`'s own doc comment explains its
//! construction) and reports accuracy for Verbora, smartcore and linfa-bayes
//! at every corpus size in that file, against its one fixed held-out test
//! set. `tests/classifiers_naivebayes_logistic.rs` is the equivalent
//! CORRECTNESS-BEFORE-PERFORMANCE check for this file's two new competitor
//! families (naivebayes; smartcore/linfa-logistic/rustlearn for Logistic
//! Regression) — real, lexically-meaningful clear-cut cases, not shape-only
//! data, since the point is agreement on a classification *decision*, not a
//! timing shape.

use std::collections::HashMap;
use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use naivebayes::NaiveBayes;
use ndarray::Array2;
use smartcore::linalg::basic::matrix::DenseMatrix;
use smartcore::linear::logistic_regression::LogisticRegression;
use smartcore::naive_bayes::multinomial::MultinomialNB;
use verbora_classifiers::{BayesClassifier, LogisticRegressionClassifier};

const SIZES: [usize; 5] = [4, 16, 64, 256, 1024];
/// Corpus sizes for the Logistic Regression group — see the module doc
/// comment's "Logistic Regression competitors" section for why these are so
/// much smaller than [`SIZES`].
const LOGISTIC_SIZES: [usize; 3] = [4, 8, 16];

/// Deterministic pseudo-random source, byte-for-byte identical to
/// `crates/verbora-classifiers/benches/classifiers.rs`'s own `Lcg`.
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

/// Byte-for-byte identical to `crates/verbora-classifiers/benches/
/// classifiers.rs`'s own `corpus()`.
fn corpus(count: usize, words: usize, vocabulary: usize, classes: usize) -> Vec<(String, String)> {
    let mut rng = Lcg(0x2545_F491_4F6C_DD1D);
    (0..count)
        .map(|i| {
            let text: Vec<String> = (0..words)
                .map(|_| format!("token{}", rng.below(vocabulary)))
                .collect();
            (text.join(" "), format!("class{}", i % classes))
        })
        .collect()
}

/// Same generator, restricted to [`LOGISTIC_SIZES`]'s much smaller shape —
/// see the module doc comment's "Logistic Regression competitors" section.
fn logistic_corpus(count: usize) -> Vec<(String, String)> {
    corpus(count, 8, 40, 3)
}

/// Whitespace-split, lowercased tokens — the pre-tokenized `Vec<String>`
/// input the `naivebayes` (ruivieira) row's `NaiveBayes::train`/`classify`
/// need (matrix: Partial, "pre-tokenized input"). Identical tokenization to
/// [`Vocab::build`]/[`Vocab::row`] (no stemming, no stop-word filtering), so
/// every Bayes competitor in this file does the same, documented amount of
/// preprocessing — see the module doc comment's `naivebayes` bullet.
fn tokenize(text: &str) -> Vec<String> {
    text.split_whitespace().map(str::to_lowercase).collect()
}

/// The smartcore/linfa adapter: a whitespace-split, lowercased vocabulary and
/// count-matrix builder.
///
/// Deliberately simpler than Verbora's own tokenizer: no stemming, no
/// stop-word filtering, no punctuation handling — a real, documented
/// difference in *how much work* each side does, beyond the pre-built-matrix
/// architecture gap the competitive matrix already flags. `[build]` is called
/// **inside** every timed benchmark iteration in [`bayes_train`], matching
/// the boundary Verbora's own `add_document`+`train()` times (raw text in,
/// nothing pre-computed).
struct Vocab {
    index: HashMap<String, usize>,
}

impl Vocab {
    fn build(docs: &[(String, String)]) -> Self {
        let mut index = HashMap::new();
        for (text, _) in docs {
            for tok in text.split_whitespace() {
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

    fn matrix(&self, docs: &[(String, String)]) -> Vec<Vec<u32>> {
        docs.iter().map(|(t, _)| self.row(t)).collect()
    }
}

/// `classN` -> `N`, the integer label linfa needs (linfa 0.8.1's `Label`
/// trait is implemented for `usize`, not `u32`). Specific to this file's own
/// `corpus()` label format, not a general parser.
fn label_ids(docs: &[(String, String)]) -> Vec<usize> {
    docs.iter()
        .map(|(_, l)| {
            l.strip_prefix("class")
                .and_then(|n| n.parse::<usize>().ok())
                .expect("corpus() labels are always \"class<N>\"")
        })
        .collect()
}

/// Same mapping, as `u32` — what smartcore's `MultinomialNB` needs.
fn label_ids_u32(docs: &[(String, String)]) -> Vec<u32> {
    label_ids(docs).into_iter().map(|v| v as u32).collect()
}

fn bench_train(c: &mut Criterion) {
    let mut g = c.benchmark_group("bayes_train");
    for n in SIZES {
        let data = corpus(n, 12, 200, 4);

        g.bench_with_input(BenchmarkId::new("verbora", n), &data, |b, data| {
            b.iter(|| {
                let mut classifier = BayesClassifier::new();
                for (text, label) in data {
                    classifier.add_document(black_box(text.as_str()), label);
                }
                classifier.train().expect("Bayes training cannot fail");
                black_box(classifier.last_added())
            });
        });

        g.bench_with_input(BenchmarkId::new("smartcore", n), &data, |b, data| {
            b.iter(|| {
                let vocab = Vocab::build(black_box(data));
                let rows = vocab.matrix(data);
                let refs: Vec<&[u32]> = rows.iter().map(Vec::as_slice).collect();
                let x = DenseMatrix::<u32>::from_2d_array(&refs).expect("rectangular matrix");
                let y = label_ids_u32(data);
                black_box(MultinomialNB::fit(&x, &y, Default::default()).expect("fits"))
            });
        });

        g.bench_with_input(BenchmarkId::new("linfa_bayes", n), &data, |b, data| {
            use linfa::prelude::*;
            use linfa_bayes::MultinomialNbParams;
            b.iter(|| {
                let vocab = Vocab::build(black_box(data));
                let rows = vocab.matrix(data);
                let flat: Vec<f64> = rows.iter().flatten().map(|&v| f64::from(v)).collect();
                let x = Array2::from_shape_vec((rows.len(), vocab.len()), flat)
                    .expect("rectangular matrix");
                let y = label_ids(data);
                let y = ndarray::Array1::from(y);
                let ds = DatasetView::new(x.view(), y.view());
                black_box(MultinomialNbParams::new().fit(&ds).expect("fits"))
            });
        });

        g.bench_with_input(BenchmarkId::new("naivebayes", n), &data, |b, data| {
            b.iter(|| {
                let mut nb = NaiveBayes::new();
                for (text, label) in black_box(data) {
                    nb.train(&tokenize(text), label);
                }
                black_box(nb)
            });
        });
    }
    g.finish();
}

fn bench_predict(c: &mut Criterion) {
    // Fixed corpus, same shape as the in-workspace bench's own
    // `classification` group (`corpus(64, 12, 400, 6)`) — training happens
    // once, outside the timed region, matching that every model here is
    // already-trained before `classify`/`predict` is called.
    let data = corpus(64, 12, 400, 6);
    let probe = data[0].0.clone();

    let mut verbora = BayesClassifier::new();
    for (text, label) in &data {
        verbora.add_document(text.as_str(), label);
    }
    verbora.train().expect("Bayes training cannot fail");

    let vocab = Vocab::build(&data);
    let rows = vocab.matrix(&data);
    let refs: Vec<&[u32]> = rows.iter().map(Vec::as_slice).collect();
    let x = DenseMatrix::<u32>::from_2d_array(&refs).expect("rectangular matrix");
    let y = label_ids_u32(&data);
    let y_linfa = label_ids(&data);
    let sm_model = MultinomialNB::fit(&x, &y, Default::default()).expect("fits");

    let mut g = c.benchmark_group("bayes_predict");
    g.bench_function("verbora", |b| {
        b.iter(|| black_box(verbora.classify(black_box(probe.as_str())).unwrap()));
    });
    g.bench_function("smartcore", |b| {
        b.iter(|| {
            let row = vocab.row(black_box(probe.as_str()));
            let xt = DenseMatrix::<u32>::from_2d_array(&[row.as_slice()]).expect("one row");
            black_box(sm_model.predict(&xt).expect("predicts"))
        });
    });
    g.bench_function("linfa_bayes", |b| {
        use linfa::prelude::*;
        use linfa_bayes::MultinomialNbParams;
        let flat: Vec<f64> = rows.iter().flatten().map(|&v| f64::from(v)).collect();
        let x = Array2::from_shape_vec((rows.len(), vocab.len()), flat).expect("rectangular");
        let y_arr = ndarray::Array1::from(y_linfa.clone());
        let ds = DatasetView::new(x.view(), y_arr.view());
        let model = MultinomialNbParams::new().fit(&ds).expect("fits");
        b.iter(|| {
            let row = vocab.row(black_box(probe.as_str()));
            let xt = Array2::from_shape_vec(
                (1, vocab.len()),
                row.iter().map(|&v| f64::from(v)).collect(),
            )
            .expect("one row");
            black_box(model.predict(&xt))
        });
    });
    g.bench_function("naivebayes", |b| {
        let mut nb = NaiveBayes::new();
        for (text, label) in &data {
            nb.train(&tokenize(text), label);
        }
        b.iter(|| black_box(nb.classify(&tokenize(black_box(probe.as_str())))));
    });
    g.finish();
}

/// `classN` -> `N` as `f32` — what smartcore's dense logistic-regression `x`
/// matrix and rustlearn's `Array` both need (smartcore's `TX: FloatNumber`
/// bound rules out the `u32` counts [`Vocab::matrix`] returns directly,
/// unlike [`MultinomialNB`] above).
fn counts_to_f64(rows: &[Vec<u32>]) -> Vec<Vec<f64>> {
    rows.iter()
        .map(|row| row.iter().map(|&v| f64::from(v)).collect())
        .collect()
}

/// Same conversion, to `f32` — rustlearn's `Array` only ever holds `f32`.
fn counts_to_f32(rows: &[Vec<u32>]) -> Vec<Vec<f32>> {
    rows.iter()
        .map(|row| row.iter().map(|&v| v as f32).collect())
        .collect()
}

fn bench_logistic_train(c: &mut Criterion) {
    let mut g = c.benchmark_group("logistic_train");
    for n in LOGISTIC_SIZES {
        let data = logistic_corpus(n);

        g.bench_with_input(BenchmarkId::new("verbora", n), &data, |b, data| {
            b.iter(|| {
                let mut classifier = LogisticRegressionClassifier::new();
                for (text, label) in data {
                    classifier.add_document(black_box(text.as_str()), label);
                }
                classifier.train().expect("the corpus has examples");
                black_box(classifier.engine().example_count())
            });
        });

        g.bench_with_input(BenchmarkId::new("smartcore", n), &data, |b, data| {
            b.iter(|| {
                let vocab = Vocab::build(black_box(data));
                let rows = counts_to_f64(&vocab.matrix(data));
                let refs: Vec<&[f64]> = rows.iter().map(Vec::as_slice).collect();
                let x = DenseMatrix::<f64>::from_2d_array(&refs).expect("rectangular matrix");
                let y = label_ids_u32(data);
                black_box(LogisticRegression::fit(&x, &y, Default::default()).expect("fits"))
            });
        });

        g.bench_with_input(BenchmarkId::new("linfa_logistic", n), &data, |b, data| {
            use linfa::prelude::*;
            use linfa_logistic::MultiLogisticRegression;
            b.iter(|| {
                let vocab = Vocab::build(black_box(data));
                let rows = vocab.matrix(data);
                let flat: Vec<f64> = rows.iter().flatten().map(|&v| f64::from(v)).collect();
                let x = Array2::from_shape_vec((rows.len(), vocab.len()), flat)
                    .expect("rectangular matrix");
                let y = label_ids(data);
                let y = ndarray::Array1::from(y);
                let ds = DatasetView::new(x.view(), y.view());
                black_box(MultiLogisticRegression::default().fit(&ds).expect("fits"))
            });
        });

        g.bench_with_input(BenchmarkId::new("rustlearn", n), &data, |b, data| {
            use rustlearn::linear_models::sgdclassifier::Hyperparameters;
            use rustlearn::prelude::*;
            b.iter(|| {
                let vocab = Vocab::build(black_box(data));
                let rows = counts_to_f32(&vocab.matrix(data));
                let x = Array::from(&rows);
                let y: Vec<f32> = label_ids(data).into_iter().map(|v| v as f32).collect();
                let y = Array::from(y);
                let mut model = Hyperparameters::new(vocab.len()).one_vs_rest();
                model.fit(&x, &y).expect("fits");
                black_box(model)
            });
        });
    }
    g.finish();
}

fn bench_logistic_predict(c: &mut Criterion) {
    // Fixed corpus, the same shape [`LOGISTIC_SIZES`]'s largest size uses —
    // training happens once, outside the timed region, matching every model
    // here already being trained before `classify`/`predict` is called.
    let data = logistic_corpus(16);
    let probe = data[0].0.clone();

    let mut verbora = LogisticRegressionClassifier::new();
    for (text, label) in &data {
        verbora.add_document(text.as_str(), label);
    }
    verbora.train().expect("the corpus has examples");

    let vocab = Vocab::build(&data);
    let rows = vocab.matrix(&data);
    let rows_f64 = counts_to_f64(&rows);
    let refs: Vec<&[f64]> = rows_f64.iter().map(Vec::as_slice).collect();
    let x = DenseMatrix::<f64>::from_2d_array(&refs).expect("rectangular matrix");
    let y = label_ids_u32(&data);
    let y_linfa = label_ids(&data);
    let sm_model = LogisticRegression::fit(&x, &y, Default::default()).expect("fits");

    let mut g = c.benchmark_group("logistic_predict");
    g.bench_function("verbora", |b| {
        b.iter(|| black_box(verbora.classify(black_box(probe.as_str())).unwrap()));
    });
    g.bench_function("smartcore", |b| {
        b.iter(|| {
            let row = vocab.row(black_box(probe.as_str()));
            let row_f64: Vec<f64> = row.iter().map(|&v| f64::from(v)).collect();
            let xt = DenseMatrix::<f64>::from_2d_array(&[row_f64.as_slice()]).expect("one row");
            black_box(sm_model.predict(&xt).expect("predicts"))
        });
    });
    g.bench_function("linfa_logistic", |b| {
        use linfa::prelude::*;
        use linfa_logistic::MultiLogisticRegression;
        let flat: Vec<f64> = rows.iter().flatten().map(|&v| f64::from(v)).collect();
        let x = Array2::from_shape_vec((rows.len(), vocab.len()), flat).expect("rectangular");
        let y_arr = ndarray::Array1::from(y_linfa.clone());
        let ds = DatasetView::new(x.view(), y_arr.view());
        let model = MultiLogisticRegression::default().fit(&ds).expect("fits");
        b.iter(|| {
            let row = vocab.row(black_box(probe.as_str()));
            let xt = Array2::from_shape_vec(
                (1, vocab.len()),
                row.iter().map(|&v| f64::from(v)).collect(),
            )
            .expect("one row");
            black_box(model.predict(&xt))
        });
    });
    g.bench_function("rustlearn", |b| {
        use rustlearn::linear_models::sgdclassifier::Hyperparameters;
        use rustlearn::prelude::*;
        let rows_f32 = counts_to_f32(&rows);
        let x_train = Array::from(&rows_f32);
        let y: Vec<f32> = y_linfa.iter().map(|&v| v as f32).collect();
        let y_train = Array::from(y);
        let mut model = Hyperparameters::new(vocab.len()).one_vs_rest();
        model.fit(&x_train, &y_train).expect("fits");
        b.iter(|| {
            let row = vocab.row(black_box(probe.as_str()));
            let row_f32: Vec<f32> = row.iter().map(|&v| v as f32).collect();
            let xt = Array::from(&vec![row_f32]);
            black_box(model.predict(&xt).expect("predicts"))
        });
    });
    g.finish();
}

criterion_group!(
    benches,
    bench_train,
    bench_predict,
    bench_logistic_train,
    bench_logistic_predict
);
criterion_main!(benches);

// ACCURACY + PERFORMANCE: see `../tests/classifiers_accuracy.rs`, not a
// `#[cfg(test)] mod` in this file — same reason as `benches/tfidf.rs`'s
// pointer comment above its own `criterion_main!`: `harness = false` leaves
// no libtest runner in this binary for `cargo test` to collect `#[test]`s
// with, so they would be dead code here. That file trains Verbora, smartcore
// and linfa-bayes at every size in the shared, signal-bearing
// `benches/data/classification-corpus.json` and reports accuracy against its
// fixed held-out test set — the speed numbers in this file are shape-only
// synthetic data (see the module doc comment above) and were never going to
// answer the accuracy question the spec's own `ACCURACY + PERFORMANCE`
// section asks for.
