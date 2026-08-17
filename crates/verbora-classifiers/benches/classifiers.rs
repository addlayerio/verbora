// `criterion_group!` expands to an undocumented function, and a benchmark
// harness is not public API, so the workspace-wide `missing_docs` lint is noise
// here.
#![allow(missing_docs)]

//! Criterion benchmarks for the classifiers.
//!
//! Four questions, because they have four different answers:
//!
//! * **Where does training time go?** Bayes accumulates counts in one pass;
//!   logistic regression runs gradient descent per class until convergence.
//!   Measured on the same corpus at the same sizes, the ratio between them is
//!   the cost of the model, not of the shared tokenising front end.
//! * **What does a single classification cost?** `text_to_features` is
//!   quadratic in the reference (`observation.indexOf(feature)` per feature);
//!   this crate inverts it to a hash-set probe. Benchmarking it separately from
//!   `classify` shows how much of a prediction is feature extraction.
//! * **How does maximum-entropy training scale?** Generalised iterative scaling
//!   is `O(iterations x features x contexts x classes)`, and every one of those
//!   factors grows with the corpus, so it is measured against corpus size.
//! * **What do the reference-compatibility primitives cost?** `Math.log` and
//!   `Math.exp` are hand-ported FDLIBM, and `safe-stable-stringify` runs once
//!   per context key. Both sit in the innermost loops, so a regression there is
//!   a regression everywhere.
//!
//! Inputs are synthesised deterministically rather than read from a corpus file:
//! the classifiers' cost depends on the *shape* of the data (documents, distinct
//! tokens, classes) far more than on its content, and generating it here keeps
//! the size sweep honest.

use std::cell::RefCell;
use std::rc::Rc;

use criterion::{BenchmarkId, Criterion, black_box, criterion_group, criterion_main};
use verbora_classifiers::{
    BayesClassifier, Context, DynValue, FeatureSet, LogisticRegressionClassifier, MECorpus,
    MaxEntClassifier, SEElement, Sample, TaggedWord, transcendental,
};

/// A deterministic pseudo-random source, so every run measures the same corpus.
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

/// `count` documents of `words` tokens each, drawn from a vocabulary of
/// `vocabulary` words and split across `classes` labels.
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

fn bayes_training(c: &mut Criterion) {
    let mut group = c.benchmark_group("bayes/train");
    for docs in [8usize, 32, 128] {
        let data = corpus(docs, 12, 200, 4);
        group.bench_with_input(BenchmarkId::from_parameter(docs), &data, |b, data| {
            b.iter(|| {
                let mut classifier = BayesClassifier::new();
                for (text, label) in data {
                    classifier.add_document(text.as_str(), label);
                }
                classifier.train().expect("Bayes training cannot fail");
                black_box(classifier.last_added())
            });
        });
    }
    group.finish();
}

fn logistic_training(c: &mut Criterion) {
    let mut group = c.benchmark_group("logistic/train");
    // Deliberately smaller: gradient descent runs to convergence per class, so
    // the 128-document case takes seconds rather than milliseconds.
    for docs in [4usize, 8, 16] {
        let data = corpus(docs, 8, 40, 3);
        group.bench_with_input(BenchmarkId::from_parameter(docs), &data, |b, data| {
            b.iter(|| {
                let mut classifier = LogisticRegressionClassifier::new();
                for (text, label) in data {
                    classifier.add_document(text.as_str(), label);
                }
                classifier.train().expect("the corpus has examples");
                black_box(classifier.engine().example_count())
            });
        });
    }
    group.finish();
}

fn classification(c: &mut Criterion) {
    let data = corpus(64, 12, 400, 6);
    let mut classifier = BayesClassifier::new();
    for (text, label) in &data {
        classifier.add_document(text.as_str(), label);
    }
    classifier.train().expect("Bayes training cannot fail");
    let probe = &data[0].0;

    let mut group = c.benchmark_group("bayes/predict");
    group.bench_function("text_to_features", |b| {
        b.iter(|| black_box(classifier.text_to_features(black_box(probe.as_str()))));
    });
    group.bench_function("get_classifications", |b| {
        b.iter(|| black_box(classifier.get_classifications(black_box(probe.as_str()))));
    });
    group.bench_function("classify", |b| {
        b.iter(|| black_box(classifier.classify(black_box(probe.as_str()))));
    });
    group.finish();
}

/// Sequential vs. `par_classify_batch` at a few batch sizes, over the same
/// trained classifier and probe corpus `classification` uses for its
/// single-item `classify` benchmark. This is the number to check before
/// reaching for `par_classify_batch`: at small sizes `rayon`'s scheduling
/// overhead can exceed the ~13 µs a single `classify` call costs, so the
/// sequential loop wins until the batch is big enough to amortise it.
#[cfg(feature = "parallel")]
fn par_classification(c: &mut Criterion) {
    let data = corpus(64, 12, 400, 6);
    let mut classifier = BayesClassifier::new();
    for (text, label) in &data {
        classifier.add_document(text.as_str(), label);
    }
    classifier.train().expect("Bayes training cannot fail");

    // Cycle the 64-document probe corpus out to each requested batch size, so
    // every task does independent, realistic work rather than repeating one
    // memoised call.
    let probes: Vec<&str> = data.iter().map(|(text, _)| text.as_str()).collect();

    let mut group = c.benchmark_group("bayes/predict_batch");
    for size in [8usize, 64, 512, 4096] {
        let batch: Vec<&str> = probes.iter().cycle().take(size).copied().collect();
        group.bench_with_input(BenchmarkId::new("sequential", size), &batch, |b, batch| {
            b.iter(|| {
                black_box(
                    batch
                        .iter()
                        .map(|t| classifier.classify(*t))
                        .collect::<Vec<_>>(),
                )
            });
        });
        group.bench_with_input(BenchmarkId::new("parallel", size), &batch, |b, batch| {
            b.iter(|| black_box(classifier.par_classify_batch(batch)));
        });
    }
    group.finish();
}

fn serialisation(c: &mut Criterion) {
    let data = corpus(64, 12, 400, 6);
    let mut classifier = BayesClassifier::new();
    for (text, label) in &data {
        classifier.add_document(text.as_str(), label);
    }
    classifier.train().expect("Bayes training cannot fail");
    let json = classifier.to_json();

    let mut group = c.benchmark_group("bayes/persist");
    group.bench_function("to_json", |b| b.iter(|| black_box(classifier.to_json())));
    group.bench_function("restore", |b| {
        b.iter(|| black_box(BayesClassifier::restore(black_box(&json)).map(|c| c.last_added())));
    });
    group.finish();
}

/// The ten-element SimpleExample sample the reference's own spec trains on.
fn simple_example() -> (Rc<RefCell<FeatureSet>>, Rc<RefCell<Sample>>) {
    let mut sample = Sample::new();
    let zero = Rc::new(Context::of_str("0"));
    let one = Rc::new(Context::of_str("1"));
    for _ in 0..3 {
        sample.add_element(SEElement::new("x", Rc::clone(&zero)));
    }
    for _ in 0..3 {
        sample.add_element(SEElement::new("y", Rc::clone(&zero)));
    }
    sample.add_element(SEElement::new("x", Rc::clone(&one)));
    for _ in 0..3 {
        sample.add_element(SEElement::new("y", Rc::clone(&one)));
    }
    let mut features = FeatureSet::new();
    sample
        .generate_features(&mut features)
        .expect("SEElement generates features");
    (
        Rc::new(RefCell::new(features)),
        Rc::new(RefCell::new(sample)),
    )
}

/// `count` four-word tagged sentences, for the POS model.
fn pos_corpus(count: usize) -> MECorpus {
    let tags = ["DT", "JJ", "NN", "VB"];
    let mut rng = Lcg(0x9E37_79B9_7F4A_7C15);
    MECorpus::from_tagged(
        (0..count)
            .map(|_| {
                tags.iter()
                    .map(|tag| TaggedWord::new(format!("w{}", rng.below(20)), *tag))
                    .collect()
            })
            .collect(),
    )
}

fn maxent_training(c: &mut Criterion) {
    let mut group = c.benchmark_group("maxent/train");
    group.bench_function("simpleExample", |b| {
        b.iter(|| {
            let (features, sample) = simple_example();
            let mut classifier = MaxEntClassifier::new(features, sample);
            classifier.train(20, 0.01).expect("SE features are finite");
            black_box(classifier.scaler().expect("trained").iteration())
        });
    });
    for sentences in [1usize, 2, 4] {
        let corpus = pos_corpus(sentences);
        group.bench_with_input(BenchmarkId::new("pos", sentences), &corpus, |b, corpus| {
            b.iter(|| {
                let sample = corpus.generate_sample();
                let mut features = FeatureSet::new();
                sample
                    .generate_features(&mut features)
                    .expect("POS elements generate features");
                let mut classifier = MaxEntClassifier::new(
                    Rc::new(RefCell::new(features)),
                    Rc::new(RefCell::new(sample)),
                );
                classifier.train(2, 0.01).expect("POS features are finite");
                black_box(classifier.scaler().expect("trained").c())
            });
        });
    }
    group.finish();
}

fn maxent_predict(c: &mut Criterion) {
    let (features, sample) = simple_example();
    let mut classifier = MaxEntClassifier::new(features, sample);
    classifier.train(20, 0.01).expect("SE features are finite");
    let probe = Rc::new(Context::of_str("0"));
    let unseen = Rc::new(Context::of_str("unseen"));

    let mut group = c.benchmark_group("maxent/predict");
    group.bench_function("classify/memoised", |b| {
        b.iter(|| black_box(classifier.classify(black_box(&probe))));
    });
    group.bench_function("classify/unseen", |b| {
        b.iter(|| black_box(classifier.classify(black_box(&unseen))));
    });
    group.finish();
}

fn reference_primitives(c: &mut Criterion) {
    let mut group = c.benchmark_group("referenceprimitives");

    // The two functions in the innermost loop of every model.
    group.bench_function("log", |b| {
        b.iter(|| black_box(transcendental::log(black_box(0.123_456_789))));
    });
    group.bench_function("exp", |b| {
        b.iter(|| black_box(transcendental::exp(black_box(-3.25))));
    });
    group.bench_function("sigmoid", |b| {
        b.iter(|| black_box(transcendental::sigmoid(black_box(0.75))));
    });

    // One context key per element per training iteration.
    let window = |offsets: &[(&str, &str)]| {
        DynValue::Obj(
            offsets
                .iter()
                .map(|(k, v)| ((*k).to_owned(), DynValue::Str((*v).to_owned())))
                .collect(),
        )
    };
    let data = DynValue::Obj(vec![
        (
            "wordWindow".to_owned(),
            window(&[("0", "dog"), ("-2", "the"), ("-1", "big"), ("1", "runs")]),
        ),
        (
            "tagWindow".to_owned(),
            window(&[("0", "NN"), ("-2", "DT"), ("-1", "JJ"), ("1", "VB")]),
        ),
    ]);
    group.bench_function("stable_stringify/posContext", |b| {
        b.iter(|| black_box(black_box(&data).stable_stringify()));
    });
    group.bench_function("stable_stringify/string", |b| {
        let s = DynValue::Str("café😀".to_owned());
        b.iter(|| black_box(black_box(&s).stable_stringify()));
    });
    group.finish();
}

criterion_group!(
    benches,
    bayes_training,
    logistic_training,
    classification,
    serialisation,
    maxent_training,
    maxent_predict,
    reference_primitives
);

#[cfg(feature = "parallel")]
criterion_group!(par_benches, par_classification);

#[cfg(not(feature = "parallel"))]
criterion_main!(benches);
#[cfg(feature = "parallel")]
criterion_main!(benches, par_benches);
