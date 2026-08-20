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
//!   this crate inverts it to one `OrderedMap::slot_of` lookup per *token*,
//!   which makes it linear in the probe rather than in the vocabulary.
//!   Benchmarking it separately from `classify` shows how much of a
//!   prediction is feature extraction — on a 342-feature vocabulary the
//!   answer is now "very little": the remainder is Porter stemming, which no
//!   restructuring can remove because the stems *are* the features.
//! * **How does maximum-entropy training scale?** One generalised-iterative-
//!   scaling iteration costs `O(events x predicates-per-event x outcomes)`, and
//!   the iteration count is itself data-dependent, so training is measured
//!   against sample size at a fixed iteration budget.
//! * **What do the shared numeric primitives cost?** `log` and `exp` are
//!   in-tree FDLIBM ports and sit in the innermost loop of every model, and
//!   `stable_stringify` runs once per persisted value. A regression in either
//!   is a regression everywhere.
//!
//! Inputs are synthesised deterministically rather than read from a corpus file:
//! the classifiers' cost depends on the *shape* of the data (documents, distinct
//! tokens, classes) far more than on its content, and generating it here keeps
//! the size sweep honest.

use criterion::{BenchmarkId, Criterion, black_box, criterion_group, criterion_main};
use verbora_classifiers::{
    BayesClassifier, DynValue, Gis, LogisticRegressionClassifier, MaxEntClassifier, Sample,
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
/// overhead can exceed the ~2 µs a single `classify` call costs, so the
/// sequential loop wins until the batch is big enough to amortise both that
/// and each worker's own stem-memo warm-up (the sequential side answers from
/// the classifier's single memo, which the repeated probe corpus below keeps
/// hot from the first iteration on).
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

/// A deterministic maximum-entropy sample: `events` events over `outcomes`
/// outcomes, each context carrying `width` predicates drawn from a shared pool.
///
/// The shape, not the content, is what the cost depends on: how many events,
/// how many predicates fire per event, and how many outcomes each of them has
/// to be scored against.
fn maxent_sample(events: usize, outcomes: usize, width: usize) -> Sample {
    let mut rng = Lcg(0x9E37_79B9_7F4A_7C15);
    let mut sample = Sample::new();
    for _ in 0..events {
        let outcome = format!("o{}", rng.below(outcomes));
        let predicates: Vec<String> = (0..width)
            .map(|slot| format!("f{slot}={}", rng.below(16)))
            .collect();
        sample.add(outcome, predicates);
    }
    sample
}

/// The worked four-event example the module documentation derives by hand.
fn worked_example() -> Sample {
    let mut sample = Sample::new();
    sample.add("x", ["a"]);
    sample.add("x", ["a"]);
    sample.add("y", ["a"]);
    sample.add("y", ["b"]);
    sample
}

fn maxent_training(c: &mut Criterion) {
    let mut group = c.benchmark_group("maxent/train");
    group.bench_function("workedExample", |b| {
        b.iter(|| {
            let mut classifier = MaxEntClassifier::from_sample(worked_example());
            let report = *classifier
                .train_with(Gis::new(20, 1e-6).expect("a valid tolerance"))
                .expect("four events");
            black_box(report.iterations)
        });
    });
    // A fixed iteration budget, so the sweep measures the per-iteration cost
    // rather than how many iterations each sample happens to need.
    let budget = Gis::new(8, 0.0).expect("a valid tolerance");
    for events in [16usize, 64, 256] {
        let sample = maxent_sample(events, 4, 6);
        group.bench_with_input(BenchmarkId::new("gis", events), &sample, |b, sample| {
            b.iter(|| {
                let mut classifier = MaxEntClassifier::from_sample(sample.clone());
                let report = *classifier.train_with(budget).expect("a non-empty sample");
                black_box(report.log_likelihood)
            });
        });
    }
    group.finish();
}

fn maxent_predict(c: &mut Criterion) {
    let mut classifier = MaxEntClassifier::from_sample(maxent_sample(256, 4, 6));
    classifier
        .train_with(Gis::new(8, 0.0).expect("a valid tolerance"))
        .expect("a non-empty sample");
    let model = classifier.model().expect("just trained").clone();
    let known: Vec<String> = classifier.sample().events()[0].predicates().to_vec();
    let unknown: Vec<String> = (0..6).map(|i| format!("f{i}=never")).collect();

    let mut group = c.benchmark_group("maxent/predict");
    group.bench_function("classify/known", |b| {
        b.iter(|| black_box(classifier.classify(black_box(&known))));
    });
    group.bench_function("classify/unknown", |b| {
        b.iter(|| black_box(classifier.classify(black_box(&unknown))));
    });
    // The allocating convenience against the reusable-buffer primitive, which
    // is the only difference between them.
    group.bench_function("distribution/allocating", |b| {
        b.iter(|| black_box(model.distribution(black_box(&known))));
    });
    group.bench_function("distribution/reused", |b| {
        let mut out = Vec::new();
        b.iter(|| {
            model.distribution_into(black_box(&known), &mut out);
            black_box(out.len())
        });
    });
    group.finish();
}

fn reference_primitives(c: &mut Criterion) {
    let mut group = c.benchmark_group("referenceprimitives");

    // The two functions in the innermost loop of every model.
    group.bench_function("log", |b| {
        b.iter(|| black_box(verbora_classifiers::log(black_box(0.123_456_789))));
    });
    group.bench_function("exp", |b| {
        b.iter(|| black_box(verbora_classifiers::exp(black_box(-3.25))));
    });
    group.bench_function("sigmoid", |b| {
        b.iter(|| black_box(verbora_classifiers::sigmoid(black_box(0.75))));
    });

    // One stringify per persisted value.
    let nested = DynValue::Obj(vec![
        (
            "counts".to_owned(),
            DynValue::Obj(
                (0..8)
                    .map(|i| (format!("k{i}"), DynValue::Num(f64::from(i))))
                    .collect(),
            ),
        ),
        (
            "labels".to_owned(),
            DynValue::Arr(
                ["the", "big", "dog", "runs"]
                    .into_iter()
                    .map(|s| DynValue::Str(s.to_owned()))
                    .collect(),
            ),
        ),
    ]);
    group.bench_function("stable_stringify/nested", |b| {
        b.iter(|| black_box(black_box(&nested).stable_stringify()));
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
