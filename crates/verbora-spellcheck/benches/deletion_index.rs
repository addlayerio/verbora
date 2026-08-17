// `criterion_group!` expands to an undocumented function, and a benchmark
// harness is not public API, so the workspace-wide `missing_docs` lint is
// noise here.
#![allow(missing_docs)]

//! Criterion benchmarks for [`DeletionIndex`] — the evidence for its own
//! existence, mirroring `benches/fuzzy_index.rs`'s structure exactly so the
//! two files' numbers are directly comparable:
//!
//! * **Construction** — building the index over corpora spanning three
//!   orders of magnitude, at a fixed `max_distance` of 2 (this crate's own
//!   "expensive" `get_corrections` case, and `FuzzyIndex`'s own benchmarked
//!   query distance).
//! * **Query: `DeletionIndex::neighbors` vs. `FuzzyIndex::neighbors` vs. a
//!   brute-force linear scan** — the same "which of these words are within
//!   `k` edits of this query?" question, answered by three different
//!   mechanisms. This is the number `docs/PERFORMANCE_GAPS.md` entry 35's
//!   `fast_symspell`-vs-`FuzzyIndex` comparison motivated building this
//!   structure to close: a deletion index should win query speed at the
//!   cost of a fixed, build-time `max_distance` `FuzzyIndex` does not need.
//!
//! Inputs come from `benches/data/words.json`, the same shared word list
//! every other benchmark in this crate uses.

use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use verbora_distance::levenshtein;
use verbora_spellcheck::{DeletionIndex, DeletionIndexBuilder, FuzzyIndex, FuzzyIndexBuilder};

/// Corpus sizes, in words — matches `benches/fuzzy_index.rs`'s own range.
const CORPUS_SIZES: [usize; 4] = [100, 1_000, 10_000, 20_000];

/// How many queries each query benchmark runs per sample.
const QUERY_COUNT: usize = 200;

/// The `max_distance` both the deletion index's build-time cap and every
/// query in this file use — `FuzzyIndex` and brute-force are queried at the
/// same value, so all three answer the identical question.
const MAX_DISTANCE: u32 = 2;

fn words() -> Vec<String> {
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
        .expect("words array")
        .iter()
        .map(|w| w.as_str().expect("word is a string").to_owned())
        .collect()
}

/// The first `n` *distinct* words — both index types index a set, matching
/// `benches/fuzzy_index.rs`'s own `distinct_words`.
fn distinct_words(n: usize) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    words()
        .into_iter()
        .filter(|w| seen.insert(w.clone()))
        .take(n)
        .collect()
}

fn build_deletion_index(words: &[String]) -> DeletionIndex {
    let mut builder = DeletionIndexBuilder::new(MAX_DISTANCE);
    for w in words {
        builder.insert(w);
    }
    builder.build()
}

fn build_fuzzy_index(words: &[String]) -> FuzzyIndex {
    let mut builder = FuzzyIndexBuilder::new();
    for w in words {
        builder.insert(w);
    }
    builder.build()
}

fn bench_construction(c: &mut Criterion) {
    let mut g = c.benchmark_group("deletion_index_construction");
    for &size in &CORPUS_SIZES {
        let corpus = distinct_words(size);
        g.throughput(Throughput::Elements(corpus.len() as u64));
        g.bench_with_input(
            BenchmarkId::new("deletion_index", size),
            &corpus,
            |b, corpus| {
                b.iter(|| build_deletion_index(black_box(corpus)));
            },
        );
        g.bench_with_input(
            BenchmarkId::new("fuzzy_index", size),
            &corpus,
            |b, corpus| {
                b.iter(|| build_fuzzy_index(black_box(corpus)));
            },
        );
    }
    g.finish();
}

/// `DeletionIndex::neighbors` vs. `FuzzyIndex::neighbors` vs. a brute-force
/// linear scan, all three at `MAX_DISTANCE`, at the same corpus sizes and
/// query shape `benches/fuzzy_index.rs` already uses (queries are corpus
/// words themselves — guarantees at least one real hit per query, the
/// realistic "look up something close to a known word" shape).
fn bench_query_vs_brute_force(c: &mut Criterion) {
    let mut g = c.benchmark_group("deletion_index_query_vs_brute_force");
    for &size in &CORPUS_SIZES {
        let corpus = distinct_words(size);
        let deletion_index = build_deletion_index(&corpus);
        let fuzzy_index = build_fuzzy_index(&corpus);
        let queries: Vec<&str> = corpus
            .iter()
            .take(QUERY_COUNT)
            .map(String::as_str)
            .collect();

        g.throughput(Throughput::Elements(queries.len() as u64));

        g.bench_with_input(
            BenchmarkId::new("deletion_index", size),
            &queries,
            |b, queries| {
                b.iter(|| {
                    let mut n = 0usize;
                    for &q in queries {
                        n += deletion_index.neighbors(black_box(q), MAX_DISTANCE).count();
                    }
                    n
                });
            },
        );
        g.bench_with_input(
            BenchmarkId::new("fuzzy_index", size),
            &queries,
            |b, queries| {
                b.iter(|| {
                    let mut n = 0usize;
                    for &q in queries {
                        n += fuzzy_index.neighbors(black_box(q), MAX_DISTANCE).count();
                    }
                    n
                });
            },
        );
        g.bench_with_input(
            BenchmarkId::new("brute_force", size),
            &queries,
            |b, queries| {
                b.iter(|| {
                    let mut n = 0usize;
                    for &q in queries {
                        n += corpus
                            .iter()
                            .filter(|w| {
                                (levenshtein(black_box(q), w, &Default::default()).round() as u32)
                                    <= MAX_DISTANCE
                            })
                            .count();
                    }
                    n
                });
            },
        );
    }
    g.finish();
}

criterion_group!(
    deletion_index_benches,
    bench_construction,
    bench_query_vs_brute_force
);
criterion_main!(deletion_index_benches);
