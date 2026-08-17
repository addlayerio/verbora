// `criterion_group!` expands to an undocumented function, and a benchmark
// harness is not public API, so the workspace-wide `missing_docs` lint is
// noise here.
#![allow(missing_docs)]

//! Criterion benchmarks for [`FuzzyIndex`] — specifically, the evidence
//! `crates/verbora-spellcheck/src/fuzzy_index.rs`'s own doc comment cites
//! for choosing a BK-tree over a brute-force scan (the approach
//! `Spellcheck::get_corrections` uses instead, appropriate there since it
//! also has to generate the edit strings themselves; a pre-built index has
//! no such requirement). Two things are measured:
//!
//! * **Construction** — building the tree over corpora spanning three
//!   orders of magnitude. Paid once per index, so reported as throughput.
//! * **Query: `FuzzyIndex::neighbors` vs. a brute-force linear scan** —
//!   the same "which of these words are within `k` edits of this query?"
//!   question, answered by walking the tree (with triangle-inequality
//!   pruning) versus computing real Levenshtein distance against every
//!   dictionary word. This is the number that justifies the tree's own
//!   existence: if it isn't faster than the scan at realistic sizes, there
//!   is no reason to prefer it over `verbora_distance::levenshtein` in a
//!   plain loop.
//!
//! Inputs come from `benches/data/words.json`, the same shared word list
//! `benches/spellcheck.rs` uses, so figures are comparable across this
//! crate's own benchmarks.

use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use verbora_distance::levenshtein;
use verbora_spellcheck::{FuzzyIndex, FuzzyIndexBuilder};

/// Corpus sizes, in words — matches `benches/spellcheck.rs`'s own range.
const CORPUS_SIZES: [usize; 4] = [100, 1_000, 10_000, 20_000];

/// How many queries each query benchmark runs per sample.
const QUERY_COUNT: usize = 200;

/// Reads the shared word list, failing loudly if it has not been generated.
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

/// The first `n` *distinct* words from the shared list — `FuzzyIndex`
/// indexes a set, so duplicates would understate `n`'s real effect on tree
/// shape and depth.
fn distinct_words(n: usize) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    words()
        .into_iter()
        .filter(|w| seen.insert(w.clone()))
        .take(n)
        .collect()
}

fn build_index(words: &[String]) -> FuzzyIndex {
    let mut builder = FuzzyIndexBuilder::new();
    for w in words {
        builder.insert(w);
    }
    builder.build()
}

fn bench_construction(c: &mut Criterion) {
    let mut g = c.benchmark_group("fuzzy_index_construction");
    for &size in &CORPUS_SIZES {
        let corpus = distinct_words(size);
        g.throughput(Throughput::Elements(corpus.len() as u64));
        g.bench_with_input(BenchmarkId::from_parameter(size), &corpus, |b, corpus| {
            b.iter(|| build_index(black_box(corpus)));
        });
    }
    g.finish();
}

/// `FuzzyIndex::neighbors` vs. a brute-force linear scan, at the same
/// corpus sizes and a realistic `max_distance` (2 — the same value
/// `benches/spellcheck.rs`'s own "expensive" `get_corrections` case uses).
fn bench_query_vs_brute_force(c: &mut Criterion) {
    const MAX_DISTANCE: u32 = 2;

    let mut g = c.benchmark_group("fuzzy_index_query_vs_brute_force");
    for &size in &CORPUS_SIZES {
        let corpus = distinct_words(size);
        let index = build_index(&corpus);
        // Queries are corpus words themselves -- guarantees at least one
        // real hit per query (the word itself, distance 0), which is the
        // realistic "look up something close to a known word" shape;
        // `Spellcheck::get_corrections`'s own benchmark makes the same
        // choice for the same reason.
        let queries: Vec<&str> = corpus
            .iter()
            .take(QUERY_COUNT)
            .map(String::as_str)
            .collect();

        g.throughput(Throughput::Elements(queries.len() as u64));

        g.bench_with_input(BenchmarkId::new("tree", size), &queries, |b, queries| {
            b.iter(|| {
                let mut n = 0usize;
                for &q in queries {
                    n += index.neighbors(black_box(q), MAX_DISTANCE).count();
                }
                n
            });
        });

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
    fuzzy_index_benches,
    bench_construction,
    bench_query_vs_brute_force
);
criterion_main!(fuzzy_index_benches);
