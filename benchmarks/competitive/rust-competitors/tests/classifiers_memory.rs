//! Real allocation-counting memory report for `benches/classifiers.rs`'s two
//! NEW competitor families — `naivebayes` (ruivieira) for Naive Bayes, and
//! smartcore/linfa-logistic/rustlearn for Logistic Regression — which had no
//! memory numbers at all before this pass (the existing `smartcore`/
//! `linfa_bayes` Naive Bayes rows already have real allocator numbers, see
//! `rust-competitors/examples/memory_report.rs`'s own `classifiers_section`
//! and `results/memory-report.json`'s `classifiers` entries; this file does
//! not repeat those, only the genuinely new rows this pass adds).
//!
//! Uses [`competitive_rust::memory::measure`] — see that module's own doc
//! comment for what it counts and its single-threaded-measurement caveat.
//! Honored here by this file containing exactly one `#[test]`: Cargo compiles
//! every `tests/*.rs` file into its own separate process, so even though
//! `cargo test`'s default runner executes multiple `#[test]` fns from one
//! binary concurrently, a binary with only one test has no sibling thread to
//! contaminate the process-global allocator counters — the same reasoning
//! `memory::measure`'s own doc comment gives for "an `#[test]`" being a safe
//! call site.
//!
//! Run with: `cargo test -p competitive-rust --test classifiers_memory --
//! --nocapture` (from `benchmarks/competitive/`).

use std::collections::HashMap;

use competitive_rust::memory;
use naivebayes::NaiveBayes;
use smartcore::linalg::basic::matrix::DenseMatrix;
use smartcore::linear::logistic_regression::LogisticRegression;
use verbora_classifiers::{BayesClassifier, LogisticRegressionClassifier};

/// Byte-for-byte identical to `benches/classifiers.rs`'s own `Lcg`.
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

/// Byte-for-byte identical to `benches/classifiers.rs`'s own `corpus()`.
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

fn tokenize(text: &str) -> Vec<String> {
    text.split_whitespace().map(str::to_lowercase).collect()
}

/// Byte-for-byte identical to `benches/classifiers.rs`'s own `Vocab`.
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

    fn matrix(&self, docs: &[(String, String)]) -> Vec<Vec<u32>> {
        docs.iter().map(|(t, _)| self.row(t)).collect()
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
}

fn label_ids(docs: &[(String, String)]) -> Vec<usize> {
    docs.iter()
        .map(|(_, l)| {
            l.strip_prefix("class")
                .and_then(|n| n.parse::<usize>().ok())
                .expect("corpus() labels are always \"class<N>\"")
        })
        .collect()
}

fn label_ids_u32(docs: &[(String, String)]) -> Vec<u32> {
    label_ids(docs).into_iter().map(|v| v as u32).collect()
}

fn print_row(label: &str, r: &memory::Report) {
    println!(
        "{:<28}{:>12}{:>16}{:>14}{:>16}{:>14}",
        label,
        r.allocations,
        r.bytes_allocated,
        r.deallocations,
        r.bytes_deallocated,
        r.rss_kb_after
            .map_or_else(|| "n/a".to_string(), |v| v.to_string()),
    );
}

/// Naive Bayes training: Verbora's own `BayesClassifier` vs. `naivebayes`
/// (ruivieira), same 64-document/6-class corpus `benches/classifiers.rs`'s
/// own `bayes_predict` group trains on. Each must actually produce a working
/// classifier (a real assertion right after the measured closure), per this
/// project's "correctness before performance" discipline — a degenerate or
/// optimized-away measurement fails loudly rather than silently reporting a
/// suspiciously small number.
#[test]
fn naivebayes_training_memory() {
    let data = corpus(64, 12, 400, 6);

    let (verbora, r_verbora) = memory::measure(|| {
        let mut c = BayesClassifier::new();
        for (text, label) in &data {
            c.add_document(text.as_str(), label);
        }
        c.train().expect("Bayes training cannot fail");
        c
    });
    assert!(
        verbora.classify(data[0].0.as_str()).is_ok(),
        "a degenerate/optimized-away Verbora classifier must not pass silently"
    );

    let (nb, r_naivebayes) = memory::measure(|| {
        let mut nb = NaiveBayes::new();
        for (text, label) in &data {
            nb.train(&tokenize(text), label);
        }
        nb
    });
    let mut nb = nb;
    assert!(
        !nb.classify(&tokenize(data[0].0.as_str())).is_empty(),
        "a degenerate/optimized-away naivebayes classifier must not pass silently"
    );

    println!("\nNaive Bayes training memory (64 docs / 12 words / 400 vocab / 6 classes)");
    println!(
        "{:<28}{:>12}{:>16}{:>14}{:>16}{:>14}",
        "implementation", "allocs", "bytes_alloc", "deallocs", "bytes_dealloc", "rss_kb"
    );
    print_row("verbora", &r_verbora);
    print_row("naivebayes", &r_naivebayes);
}

/// Logistic Regression training: Verbora's own `LogisticRegressionClassifier`
/// vs. smartcore, linfa-logistic and rustlearn, same small corpus
/// `benches/classifiers.rs`'s own `logistic_train`/`logistic_predict` groups
/// use (`LOGISTIC_SIZES`'s largest size, 16 documents) — see that file's own
/// doc comment for why Logistic Regression's corpus is so much smaller than
/// Naive Bayes'.
#[test]
fn logistic_regression_training_memory() {
    let data = corpus(16, 8, 40, 3);

    let (verbora, r_verbora) = memory::measure(|| {
        let mut c = LogisticRegressionClassifier::new();
        for (text, label) in &data {
            c.add_document(text.as_str(), label);
        }
        c.train().expect("the corpus has examples");
        c
    });
    assert!(
        verbora.classify(data[0].0.as_str()).is_ok(),
        "a degenerate/optimized-away Verbora classifier must not pass silently"
    );

    let (sm_model, r_smartcore) = memory::measure(|| {
        let vocab = Vocab::build(&data);
        let rows = vocab.matrix(&data);
        let rows_f64: Vec<Vec<f64>> = rows
            .iter()
            .map(|r| r.iter().map(|&v| f64::from(v)).collect())
            .collect();
        let refs: Vec<&[f64]> = rows_f64.iter().map(Vec::as_slice).collect();
        let x = DenseMatrix::<f64>::from_2d_array(&refs).expect("rectangular matrix");
        let y = label_ids_u32(&data);
        LogisticRegression::fit(&x, &y, Default::default()).expect("fits")
    });
    let probe_vocab = Vocab::build(&data);
    let probe_row = probe_vocab.row(data[0].0.as_str());
    let probe_row_f64: Vec<f64> = probe_row.iter().map(|&v| f64::from(v)).collect();
    let xt = DenseMatrix::<f64>::from_2d_array(&[probe_row_f64.as_slice()]).expect("one row");
    assert!(
        sm_model.predict(&xt).is_ok(),
        "a degenerate/optimized-away smartcore model must not pass silently"
    );

    let (_, r_linfa) = memory::measure(|| {
        use linfa::prelude::*;
        use linfa_logistic::MultiLogisticRegression;
        let vocab = Vocab::build(&data);
        let rows = vocab.matrix(&data);
        let flat: Vec<f64> = rows.iter().flatten().map(|&v| f64::from(v)).collect();
        let x = ndarray::Array2::from_shape_vec((rows.len(), vocab.len()), flat)
            .expect("rectangular matrix");
        let y = label_ids(&data);
        let y = ndarray::Array1::from(y);
        let ds = DatasetView::new(x.view(), y.view());
        MultiLogisticRegression::default().fit(&ds).expect("fits")
    });

    let (_, r_rustlearn) = memory::measure(|| {
        use rustlearn::linear_models::sgdclassifier::Hyperparameters;
        use rustlearn::prelude::*;
        let vocab = Vocab::build(&data);
        let rows = vocab.matrix(&data);
        let rows_f32: Vec<Vec<f32>> = rows
            .iter()
            .map(|r| r.iter().map(|&v| v as f32).collect())
            .collect();
        let x = Array::from(&rows_f32);
        let y: Vec<f32> = label_ids(&data).into_iter().map(|v| v as f32).collect();
        let y = Array::from(y);
        let mut model = Hyperparameters::new(vocab.len()).one_vs_rest();
        model.fit(&x, &y).expect("fits");
        model
    });

    println!("\nLogistic Regression training memory (16 docs / 8 words / 40 vocab / 3 classes)");
    println!(
        "{:<28}{:>12}{:>16}{:>14}{:>16}{:>14}",
        "implementation", "allocs", "bytes_alloc", "deallocs", "bytes_dealloc", "rss_kb"
    );
    print_row("verbora", &r_verbora);
    print_row("smartcore", &r_smartcore);
    print_row("linfa_logistic", &r_linfa);
    print_row("rustlearn", &r_rustlearn);
}
