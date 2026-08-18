#![allow(missing_docs)]

//! Verbora vs. a real, pinned third-party Rust competitor — fuzzy/edit-
//! distance candidate lookup: `verbora_spellcheck::FuzzyIndex::neighbors`
//! vs. `fst::Set::search` composed with `fst::automaton::Levenshtein`.
//!
//! # Classification: NARROWED_EXACT on this file's ASCII word list, with a
//! disclosed correctness caveat for certain non-ASCII input
//!
//! Both sides answer the identical question -- "which stored words are
//! within edit distance `k` of this query?" -- under the identical metric
//! for the domain this file's shared corpus (`benches/data/words.json`,
//! lowercase ASCII, `a`-`z` only) actually exercises:
//!
//! * `FuzzyIndex` uses `verbora_distance::levenshtein` with
//!   `Options::default()` (unit insert/delete/substitute cost, no
//!   transposition -- see `crates/verbora-spellcheck/src/fuzzy_index.rs`'s
//!   own doc comment), computed over UTF-16 code units
//!   (`crates/verbora-distance/src/units.rs`).
//! * `fst::automaton::Levenshtein` computes unit-cost insert/delete/
//!   substitute distance over Unicode scalar values (`char`s) -- its own
//!   doc comment states this explicitly.
//!
//! For any word made only of Basic-Multilingual-Plane characters (which
//! includes every ASCII word in this corpus: one `char` is always exactly
//! one UTF-16 code unit in the BMP), the two metrics agree exactly. Verified
//! directly in `tests/fst_fuzzy_correctness.rs` (`BTreeSet`-equality across
//! a spread of real-corpus queries and `max_distance` values) before any
//! timing number below was trusted, per this workbench's own `CORRECTNESS
//! BEFORE PERFORMANCE` rule.
//!
//! **Known, real divergence outside this file's domain** (do not extend
//! this benchmark to non-ASCII input without re-reading this): `fst` 0.4.7's
//! `Levenshtein` automaton silently returns *incomplete* results for
//! same-byte-length multi-byte UTF-8 substitutions -- e.g.
//! `Set::search(&Levenshtein::new("аб", 1))` (Cyrillic) against a set
//! containing `"ав"` (one substitution away) returns nothing, at *any*
//! max_distance up to 4, even though `Set::contains` confirms both keys are
//! present. Latin BMP accented substitutions (e.g. `café`/`cafe`, a
//! 2-byte-to-1-byte substitution) are *not* affected -- only same-byte-
//! length multi-byte substitutions are. This reproduces on plain,
//! individually-constructed `fst::Set`s outside any Verbora code, matches
//! [a still-open upstream issue](https://github.com/BurntSushi/fst/issues/38)
//! ("levenshtein automata not matching Japanese Characters correctly",
//! opened 2017, still open as of this writing) -- a real, disclosed
//! upstream defect, not a fairness artifact of this comparison, and
//! consistent with the crate's own doc comment calling its Levenshtein
//! automaton "not speedy" and warning it "should [be] vastly improved in
//! the future". **This file's ASCII-only corpus never exercises that bug**,
//! which is exactly why the classification above is NARROWED_EXACT and not
//! plain EXACT -- the domain is real and disclosed, not silently dodged.
//!
//! # `fst`'s own size/memory caveat
//!
//! `fst::automaton::Levenshtein::new` errors (`Error::TooManyStates`) past a
//! default 10,000-DFA-state limit -- for an 8-character query, distance 1
//! and 2 build in under 1ms, distance 3 costs roughly an order of magnitude
//! more, and distance 4 can already exceed the default limit for some
//! queries. `new_with_limit` raises the ceiling at the cost of real, measured
//! memory -- a genuine robustness dimension `FuzzyIndex::neighbors` does not
//! share (it has no equivalent failure mode: `max_distance` is a plain
//! query-time integer with no automaton to construct).
//!
//! # Build asymmetry, measured not hidden
//!
//! `fst::Set` needs sorted, deduplicated input (`SetBuilder`/`Set::from_iter`
//! error otherwise); `FuzzyIndexBuilder::insert` accepts any order. The
//! sort+dedup step is included *inside* the timed `fst` construction closure
//! below, not hoisted out, matching `benches/trie.rs`'s own `build_fst`.
//!
//! # Measured grid
//!
//! Construction is measured at six corpus sizes; queries at the same six
//! sizes × two `max_distance` values. See `CORPUS_SIZES` and `MAX_DISTANCES`
//! below for exactly which cells are shared with the in-workspace
//! `crates/verbora-spellcheck/benches/fuzzy_index.rs` grid, why the
//! intermediate sizes and the distance-1 rows were added, and why higher
//! distances are deliberately not timed.

use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use fst::automaton::Levenshtein;
use fst::{IntoStreamer, Set, Streamer};
use verbora_spellcheck::{FuzzyIndex, FuzzyIndexBuilder};

/// Corpus sizes. `100`, `1_000`, `10_000` and `20_000` match
/// `crates/verbora-spellcheck/benches/fuzzy_index.rs`'s own `CORPUS_SIZES`,
/// so those four columns stay directly comparable across the two files;
/// `300` and `3_000` are this file's own added intermediate points — a
/// *double* crossover (`docs/PERFORMANCE_GAPS.md` entry 37) cannot be
/// located from four points spaced a decade apart, so each decade gets an
/// interior sample. Both extra sizes are plain prefixes of the same shared
/// `words.json` word list (`distinct_words`), not new source data.
const CORPUS_SIZES: [usize; 6] = [100, 300, 1_000, 3_000, 10_000, 20_000];

/// `max_distance` values measured per corpus size in `bench_query`. `1` is
/// the dominant real spellcheck case (a single typo) and the cheapest
/// automaton `fst` can be asked to build; `2` is the file's original,
/// headline configuration. Both distances sit squarely inside the
/// NARROWED_EXACT ASCII agreement domain pinned in
/// `tests/fst_fuzzy_correctness.rs` (which sweeps distances 0–3). Higher
/// distances are deliberately not timed: distance 3+ automata approach
/// `fst`'s own default state limit for long queries (see the module doc
/// comment's size/memory caveat), which would make the comparison a
/// robustness question, not a throughput one.
const MAX_DISTANCES: [u32; 2] = [1, 2];

fn words() -> Vec<String> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("repo root is 3 levels up from rust-competitors/")
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
        .expect("word list")
        .iter()
        .map(|w| w.as_str().expect("word").to_owned())
        .collect()
}

/// Distinct words only -- both `FuzzyIndex` and `fst::Set` index a set, not
/// a multiset; matches `crates/verbora-spellcheck/benches/fuzzy_index.rs`'s
/// own `distinct_words`.
fn distinct_words(n: usize) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    words()
        .into_iter()
        .filter(|w| seen.insert(w.clone()))
        .take(n)
        .collect()
}

fn build_fuzzy_index(words: &[String]) -> FuzzyIndex {
    let mut b = FuzzyIndexBuilder::new();
    for w in words {
        b.insert(w);
    }
    b.build()
}

/// Sort + dedup happens inside this function -- see the module doc comment's
/// "Build asymmetry" section for why that is deliberate.
fn build_fst(words: &[String]) -> Set<Vec<u8>> {
    let mut sorted: Vec<&str> = words.iter().map(String::as_str).collect();
    sorted.sort_unstable();
    sorted.dedup();
    Set::from_iter(sorted).expect("sorted, deduplicated input builds an fst::Set")
}

fn bench_construction(c: &mut Criterion) {
    let mut g = c.benchmark_group("fst_fuzzy_construction");
    for &size in &CORPUS_SIZES {
        let corpus = distinct_words(size);
        g.throughput(Throughput::Elements(corpus.len() as u64));
        g.bench_with_input(
            BenchmarkId::new("fuzzy_index", size),
            &corpus,
            |b, corpus| {
                b.iter(|| black_box(build_fuzzy_index(black_box(corpus))));
            },
        );
        g.bench_with_input(BenchmarkId::new("fst", size), &corpus, |b, corpus| {
            b.iter(|| black_box(build_fst(black_box(corpus))));
        });
    }
    g.finish();
}

fn bench_query(c: &mut Criterion) {
    const QUERY_COUNT: usize = 200;

    let mut g = c.benchmark_group("fst_fuzzy_query");
    for &size in &CORPUS_SIZES {
        let corpus = distinct_words(size);
        let index = build_fuzzy_index(&corpus);
        let fst_set = build_fst(&corpus);
        let queries: Vec<&str> = corpus
            .iter()
            .take(QUERY_COUNT)
            .map(String::as_str)
            .collect();

        g.throughput(Throughput::Elements(queries.len() as u64));

        for &max_distance in &MAX_DISTANCES {
            // `max_distance == 2` keeps the file's original, unsuffixed
            // benchmark IDs (`fuzzy_index`/`fst`) — `results/results.json`'s
            // raw-file mapping references them, so they must stay stable;
            // the added distance gets a `_d1` suffix instead.
            let (fuzzy_id, fst_id) = if max_distance == 2 {
                ("fuzzy_index".to_owned(), "fst".to_owned())
            } else {
                (
                    format!("fuzzy_index_d{max_distance}"),
                    format!("fst_d{max_distance}"),
                )
            };

            g.bench_with_input(BenchmarkId::new(fuzzy_id, size), &queries, |b, queries| {
                b.iter(|| {
                    let mut n = 0usize;
                    for &q in queries {
                        n += index.neighbors(black_box(q), max_distance).count();
                    }
                    n
                });
            });
            g.bench_with_input(BenchmarkId::new(fst_id, size), &queries, |b, queries| {
                b.iter(|| {
                    let mut n = 0usize;
                    for &q in queries {
                        let lev = Levenshtein::new(black_box(q), max_distance)
                            .expect("automaton within the default state limit for these queries");
                        let mut stream = fst_set.search(&lev).into_stream();
                        while stream.next().is_some() {
                            n += 1;
                        }
                    }
                    n
                });
            });
        }
    }
    g.finish();
}

criterion_group!(fst_fuzzy_benches, bench_construction, bench_query);
criterion_main!(fst_fuzzy_benches);
