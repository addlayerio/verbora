// `criterion_group!` expands to an undocumented function, and a benchmark
// harness is not public API, so the workspace-wide `missing_docs` lint is noise
// here.
#![allow(missing_docs)]

//! Criterion benchmarks for the sentence analyzer.
//!
//! Sentence lengths span 8 to 2 048 tags so that scaling — every method here is
//! a single linear pass — is visible rather than inferred. Tokens come from
//! `benches/data/words.json`, the shared corpus the rest of the workspace's
//! benchmarks use, so every harness describes byte-identical input.

use criterion::{
    BatchSize, BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main,
};
use verbora_analyzers::{Punct, SentenceAnalyzer, TaggedWord};

/// Sentence lengths, in tags.
const SIZES: [usize; 5] = [8, 32, 128, 512, 2048];

/// The POS tags assigned cyclically to the corpus words.
///
/// Chosen so every branch is exercised at a realistic frequency: `IN` opens a
/// prepositional phrase, `NN`/`NNS` close one, `VB` ends the subject, `PRP` is
/// the tag-question trigger. `DT` comes first so the sentence always has a
/// subject and `part` never appends an implicit 'You' — which keeps repeated
/// calls idempotent and the measurement meaningful.
///
/// Every sibling harness carries the same array; a mismatch would make them
/// measure different work, so keep them in step.
const TAG_CYCLE: [&str; 10] = [
    "DT", "JJ", "NN", "VB", "RB", "IN", "NNS", "PRP", "VBD", "CD",
];

/// The shared word list, read from `benches/data/words.json`.
fn load_words() -> Vec<String> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crate is two levels below the workspace root")
        .join("benches/data/words.json");
    let body = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "cannot read {}: {e}\nGenerate it with: python3 tools/bench-data/generate.py",
            path.display()
        )
    });
    let json: serde_json::Value = serde_json::from_str(&body).expect("valid bench data");
    json["words"]
        .as_array()
        .expect("a word list")
        .iter()
        .map(|w| w.as_str().expect("a word").to_owned())
        .collect()
}

/// Builds an `n`-tag sentence, borrowing every token from `words`.
fn sentence<'a>(words: &'a [String], n: usize) -> Vec<TaggedWord<'a>> {
    (0..n)
        .map(|i| {
            TaggedWord::new(
                words[i % words.len()].as_str(),
                TAG_CYCLE[i % TAG_CYCLE.len()],
            )
        })
        .collect()
}

fn bench_preposition_phrases(c: &mut Criterion) {
    let words = load_words();
    let mut g = c.benchmark_group("preposition_phrases");
    for n in SIZES {
        let tags = sentence(&words, n);
        g.throughput(Throughput::Elements(n as u64));
        g.bench_with_input(BenchmarkId::from_parameter(n), &n, |bench, _| {
            // `pp` is only ever set, so repeated calls on one analyzer do the
            // same work and leave the same state.
            let mut a = SentenceAnalyzer::new(tags.clone());
            bench.iter(|| {
                a.preposition_phrases();
                black_box(&a.pos_obj.tags);
            });
        });
    }
    g.finish();
}

fn bench_part(c: &mut Criterion) {
    let words = load_words();
    let mut g = c.benchmark_group("part");
    for n in SIZES {
        let tags = sentence(&words, n);
        g.throughput(Throughput::Elements(n as u64));
        g.bench_with_input(BenchmarkId::from_parameter(n), &n, |bench, _| {
            let mut a = SentenceAnalyzer::new(tags.clone());
            bench.iter(|| {
                a.part();
                black_box(&a.pos_obj.tags);
            });
        });
    }
    g.finish();
}

fn bench_type_of(c: &mut Criterion) {
    let words = load_words();
    let mut g = c.benchmark_group("type_of");
    for n in SIZES {
        let tags = sentence(&words, n);
        g.throughput(Throughput::Elements(n as u64));
        g.bench_with_input(BenchmarkId::from_parameter(n), &n, |bench, _| {
            // A non-empty `punct()` keeps the call non-destructive: the mark it
            // pops comes from the returned list, not from the sentence. With an
            // empty one this would consume a tag per iteration. The list is
            // rebuilt per call, exactly as the reference
            // `function () { return [{ token: '.', pos: '.' }] }` does.
            let mut a = SentenceAnalyzer::with_punct(tags.clone(), || {
                Punct::List(vec![TaggedWord::new(".", ".")])
            });
            a.part();
            bench.iter(|| black_box(a.type_of()));
        });
    }
    g.finish();
}

fn bench_render(c: &mut Criterion) {
    let words = load_words();
    let mut g = c.benchmark_group("render");
    for n in SIZES {
        let tags = sentence(&words, n);
        let mut a = SentenceAnalyzer::new(tags);
        a.part();
        g.throughput(Throughput::Elements(n as u64));
        g.bench_with_input(BenchmarkId::new("subject_to_string", n), &n, |bench, _| {
            bench.iter(|| black_box(a.subject_to_string()));
        });
        g.bench_with_input(BenchmarkId::new("to_string", n), &n, |bench, _| {
            bench.iter(|| black_box(a.to_string()));
        });
    }
    g.finish();
}

fn bench_end_to_end(c: &mut Criterion) {
    let words = load_words();
    let mut g = c.benchmark_group("end_to_end");
    for n in SIZES {
        let tags = sentence(&words, n);
        g.throughput(Throughput::Elements(n as u64));
        g.bench_with_input(BenchmarkId::from_parameter(n), &n, |bench, _| {
            // The whole pipeline on a fresh sentence: annotate, split, classify,
            // render. `iter_batched` keeps the tag clone out of the measurement,
            // since a caller would already hold its tagger's output.
            bench.iter_batched(
                || tags.clone(),
                |fresh| {
                    let mut a = SentenceAnalyzer::new(fresh);
                    a.part();
                    let ty = a.type_of();
                    black_box((ty, a.subject_to_string(), a.predicate_to_string()))
                },
                BatchSize::SmallInput,
            );
        });
    }
    g.finish();
}

fn bench_build(c: &mut Criterion) {
    let words = load_words();
    let mut g = c.benchmark_group("build_tags");
    for n in SIZES {
        g.throughput(Throughput::Elements(n as u64));
        g.bench_with_input(BenchmarkId::from_parameter(n), &n, |bench, _| {
            // Tokens are borrowed, so this measures one `Vec` growth and nothing
            // else — the payoff for `Cow` fields over `String` ones.
            bench.iter(|| black_box(sentence(&words, n)));
        });
    }
    g.finish();
}

/// Sequential `SentenceAnalyzer` pipeline vs. `par_analyze_batch`, at a few
/// realistic batch sizes. Requires the `parallel` feature; a no-op group
/// otherwise, so `criterion_group!` below stays a single, unconditional list.
fn bench_par_analyze_batch(c: &mut Criterion) {
    #[cfg(not(feature = "parallel"))]
    {
        let _ = c;
    }

    #[cfg(feature = "parallel")]
    {
        use verbora_analyzers::par_analyze_batch;

        let words = load_words();
        // A mid-range sentence length from `SIZES` above (~6-7 µs per
        // `bench_end_to_end`), repeated out to a few batch sizes: a small
        // batch close to rayon's scheduling break-even point, and larger ones
        // where the fan-out should win clearly.
        const TAGS_PER_SENTENCE: usize = 32;
        let mut g = c.benchmark_group("par_analyze_batch");
        for &n in &[16usize, 256, 4096] {
            g.throughput(Throughput::Elements(n as u64));
            g.bench_with_input(BenchmarkId::new("sequential", n), &n, |bench, &n| {
                bench.iter_batched(
                    || {
                        (0..n)
                            .map(|_| sentence(&words, TAGS_PER_SENTENCE))
                            .collect::<Vec<_>>()
                    },
                    |fresh: Vec<Vec<TaggedWord<'_>>>| {
                        let out: Vec<_> = fresh
                            .into_iter()
                            .map(|tags| {
                                let mut a = SentenceAnalyzer::new(tags);
                                a.part();
                                let ty = a.type_of();
                                (ty, a.subject_to_string(), a.predicate_to_string())
                            })
                            .collect();
                        black_box(out)
                    },
                    BatchSize::SmallInput,
                );
            });
            g.bench_with_input(BenchmarkId::new("parallel", n), &n, |bench, &n| {
                bench.iter_batched(
                    || {
                        (0..n)
                            .map(|_| sentence(&words, TAGS_PER_SENTENCE))
                            .collect::<Vec<_>>()
                    },
                    |fresh| black_box(par_analyze_batch(fresh)),
                    BatchSize::SmallInput,
                );
            });
        }
        g.finish();
    }
}

criterion_group!(
    benches,
    bench_preposition_phrases,
    bench_part,
    bench_type_of,
    bench_render,
    bench_end_to_end,
    bench_build,
    bench_par_analyze_batch
);
criterion_main!(benches);
