// `criterion_group!` expands to an undocumented function, and a benchmark
// harness is not public API, so the workspace-wide `missing_docs` lint is noise
// here.
#![allow(missing_docs)]

//! Criterion benchmarks for the spellchecker.
//!
//! Four things are measured, because they answer four different questions:
//!
//! * **Construction** — building the word table and the frequency counts over
//!   corpora spanning three orders of magnitude. This is paid once per instance
//!   and is linear in the corpus, so it is reported as throughput. The
//!   near-distance retrieval structure is built lazily on the first
//!   `corrections` call, so it is deliberately *not* in this group.
//! * **`is_correct`** — one hash lookup, on hits and on two shapes of miss.
//! * **`corrections`** — the whole pipeline, on both sides of the internal
//!   dispatch boundary: distance 1 and 2 go through the lazily built
//!   symmetric-delete index, distance 3 falls back to a corpus scan. Measuring
//!   both is what tells a caller what crossing that boundary costs.
//! * **`par_corrections_batch` vs. a sequential loop** (`parallel` feature
//!   only) — the same `corrections` calls, at the same distance and over the
//!   same corpus, at a few batch sizes. This is what answers whether the
//!   `rayon` fan-out is worth its scheduling overhead at a given batch size,
//!   not just that it compiles.
//!
//! ASCII and Cyrillic corpora are benchmarked side by side. They are the same
//! shape and size — the transliteration is a bijection on `a`–`z` — so the
//! difference between them is the cost of the wider scalars alone, now that the
//! crate has one unit (the Unicode scalar) and no ASCII-only alphabet.
//!
//! Inputs come from `benches/data/words.json`, the shared word list every
//! spellcheck harness in the repo reads. Every input is derived from it by a
//! stated rule (first word of a given length, first 4,000 words, …) rather than
//! by an index into a shuffled list, so each harness is provably measuring the
//! same work.
//!
//! **No result of this suite has been recorded since the crate's contract
//! changed.** Every figure that used to appear in these comments described the
//! previous candidate-generation design and is gone rather than adjusted.

use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use verbora_spellcheck::Spellcheck;

/// Corpus sizes, in words. The corpus decides both how many words share a
/// deletion sequence with a query and how often a candidate is a real match,
/// so it is the parameter that matters most.
const CORPUS_SIZES: [usize; 4] = [100, 1_000, 10_000, 20_000];

/// How many probes each membership benchmark walks.
///
/// One `is_correct` call is far below the resolution a per-call timing
/// baseline's timer can resolve honestly. Both sides therefore measure a fixed
/// batch, and the throughput annotation divides it back out.
const PROBE_COUNT: usize = 4_000;

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

/// The same words transliterated into Cyrillic, one code unit per letter.
///
/// The map is a bijection on `a`–`z`, so the corpus keeps its exact branching
/// structure and word lengths. The only thing that changes is that the ASCII
/// fast path can no longer be taken.
fn cyrillic(words: &[String]) -> Vec<String> {
    const RU: [char; 26] = [
        'а', 'б', 'в', 'г', 'д', 'е', 'ж', 'з', 'и', 'й', 'к', 'л', 'м', 'н', 'о', 'п', 'р', 'с',
        'т', 'у', 'ф', 'х', 'ц', 'ч', 'ш', 'щ',
    ];
    words
        .iter()
        .map(|w| {
            w.chars()
                .map(|c| {
                    if c.is_ascii_lowercase() {
                        RU[(c as usize) - ('a' as usize)]
                    } else {
                        c
                    }
                })
                .collect()
        })
        .collect()
}

/// The first word of exactly `len` characters.
///
/// A rule rather than an index, so any sibling harness can reproduce the
/// choice without either side having to hard-code a string.
fn word_of_length(words: &[String], len: usize) -> String {
    words
        .iter()
        .find(|w| w.chars().count() == len)
        .unwrap_or_else(|| panic!("no {len}-character word in the corpus"))
        .clone()
}

/// A misspelling of `word`: the middle scalar deleted.
///
/// A realistic probe. Feeding `corrections` a word that is already correct
/// measures something else, and feeding it noise measures the empty case.
fn typo(word: &str) -> String {
    let mut chars: Vec<char> = word.chars().collect();
    if chars.len() > 1 {
        chars.remove(chars.len() / 2);
    }
    chars.into_iter().collect()
}

fn bench_construction(c: &mut Criterion) {
    let ascii = words();
    let ru = cyrillic(&ascii);
    let mut g = c.benchmark_group("spellcheck_new");
    for (tag, corpus) in [("ascii", &ascii), ("cyrillic", &ru)] {
        for n in CORPUS_SIZES {
            let slice = &corpus[..n];
            g.throughput(Throughput::Elements(n as u64));
            g.bench_with_input(BenchmarkId::new(tag, n), &n, |b, _| {
                b.iter(|| Spellcheck::new(black_box(slice)));
            });
        }
    }
    g.finish();
}

fn bench_is_correct(c: &mut Criterion) {
    let words = words();
    let sc = Spellcheck::new(&words);
    let hits: Vec<&str> = words[..PROBE_COUNT].iter().map(String::as_str).collect();
    // Shares a long prefix with a real word but is not one: the expensive miss,
    // and the one the correction search actually generates.
    let near: Vec<String> = words[..PROBE_COUNT].iter().map(|w| typo(w)).collect();
    // Dies on the first character.
    let far: Vec<String> = words[..PROBE_COUNT]
        .iter()
        .map(|w| format!("Q{w}"))
        .collect();

    let mut g = c.benchmark_group("spellcheck_is_correct");
    g.throughput(Throughput::Elements(PROBE_COUNT as u64));
    g.bench_function("hit", |b| {
        b.iter(|| hits.iter().filter(|w| sc.is_correct(black_box(w))).count());
    });
    g.bench_function("miss_near", |b| {
        b.iter(|| near.iter().filter(|w| sc.is_correct(black_box(w))).count());
    });
    g.bench_function("miss_far", |b| {
        b.iter(|| far.iter().filter(|w| sc.is_correct(black_box(w))).count());
    });
    g.finish();
}

fn bench_corrections(c: &mut Criterion) {
    let ascii = words();
    let ru = cyrillic(&ascii);
    // Length is a dominant cost on both paths — it drives the deletion
    // neighbourhood's size on the indexed path and the metric's inner loop on
    // the scan — so the probes are pinned to a length rather than a position.
    let probe = typo(&word_of_length(&ascii, 8));
    let ru_probe = typo(&word_of_length(&ru, 8));

    let mut g = c.benchmark_group("spellcheck_corrections_d1");
    for (tag, corpus, probe) in [("ascii", &ascii, &probe), ("cyrillic", &ru, &ru_probe)] {
        for n in CORPUS_SIZES {
            let sc = Spellcheck::new(&corpus[..n]);
            // The retrieval structure is lazy; warm it so the group measures
            // querying rather than one-time construction.
            let _ = sc.corrections(probe, 1);
            g.bench_with_input(BenchmarkId::new(tag, n), &n, |b, _| {
                b.iter(|| sc.corrections(black_box(probe), 1));
            });
        }
    }
    g.finish();

    // Distance 2 widens the deletion neighbourhood quadratically in the
    // probe's length, so it gets its own group and a shorter probe.
    let probe = typo(&word_of_length(&ascii, 6));
    let ru_probe = typo(&word_of_length(&ru, 6));
    let mut g = c.benchmark_group("spellcheck_corrections_d2");
    for (tag, corpus, probe) in [("ascii", &ascii, &probe), ("cyrillic", &ru, &ru_probe)] {
        for n in [1_000usize, 20_000] {
            let sc = Spellcheck::new(&corpus[..n]);
            let _ = sc.corrections(probe, 2);
            g.bench_with_input(BenchmarkId::new(tag, n), &n, |b, _| {
                b.iter(|| sc.corrections(black_box(probe), 2));
            });
        }
    }
    g.finish();

    // Distance 3 is the other side of the dispatch boundary: no index, a scan
    // of the corpus with a scalar-length lower bound pruning it. Benchmarked
    // against the same probes so the two paths are directly comparable.
    let mut g = c.benchmark_group("spellcheck_corrections_d3_scan");
    g.sample_size(20);
    for (tag, corpus, probe) in [("ascii", &ascii, &probe), ("cyrillic", &ru, &ru_probe)] {
        for n in [1_000usize, 20_000] {
            let sc = Spellcheck::new(&corpus[..n]);
            g.bench_with_input(BenchmarkId::new(tag, n), &n, |b, _| {
                b.iter(|| sc.corrections(black_box(probe), 3));
            });
        }
    }
    g.finish();

    // `best_correction` does the same retrieval and skips the sort. Whether
    // that is worth a separate entry point is exactly what this group answers.
    let probe = typo(&word_of_length(&ascii, 8));
    let mut g = c.benchmark_group("spellcheck_best_correction_d2");
    for n in [1_000usize, 20_000] {
        let sc = Spellcheck::new(&ascii[..n]);
        let _ = sc.best_correction(&probe, 2);
        g.bench_with_input(BenchmarkId::new("ascii", n), &n, |b, _| {
            b.iter(|| sc.best_correction(black_box(&probe), 2));
        });
    }
    g.finish();
}

/// Sequential vs. `rayon`-parallel batch corrections, at the sizes a caller
/// batching a whole document's misspellings might actually hit.
///
/// The probes are one typo per corpus word, taken in corpus order — the same
/// construction `bench_is_correct`'s `near` uses — rather than one word
/// repeated, so the batch mixes short and long inputs the way a real document
/// would, instead of measuring `n` copies of a single cost.
#[cfg(feature = "parallel")]
fn bench_par_corrections_batch(c: &mut Criterion) {
    let ascii = words();
    let sc = Spellcheck::new(&ascii[..20_000]);
    let typos: Vec<String> = ascii[..512].iter().map(|w| typo(w)).collect();
    let probes: Vec<&str> = typos.iter().map(String::as_str).collect();

    // Distance 2, matching `bench_corrections`'s d2 group, so the per-word
    // cost the fan-out is amortising is measured in that group and this one
    // measures only what batching changes. No ratio is quoted here: the
    // crate's contract changed and nothing has been re-measured since.
    let mut g = c.benchmark_group("spellcheck_par_corrections_batch_d2");
    g.sample_size(10);
    // The retrieval structure is lazy; warm it outside the timing loop so
    // neither arm pays a one-time build the other does not.
    let _ = sc.corrections(probes[0], 2);
    for n in [8usize, 64, 512] {
        let batch = &probes[..n];
        g.throughput(Throughput::Elements(n as u64));
        g.bench_with_input(BenchmarkId::new("sequential", n), &n, |b, _| {
            b.iter(|| {
                batch
                    .iter()
                    .map(|w| sc.corrections(black_box(w), 2))
                    .collect::<Vec<_>>()
            });
        });
        g.bench_with_input(BenchmarkId::new("parallel", n), &n, |b, _| {
            b.iter(|| sc.par_corrections_batch(black_box(batch), 2));
        });
    }
    g.finish();
}

criterion_group!(
    benches,
    bench_construction,
    bench_is_correct,
    bench_corrections
);

#[cfg(feature = "parallel")]
criterion_group!(parallel_benches, bench_par_corrections_batch);

#[cfg(feature = "parallel")]
criterion_main!(benches, parallel_benches);

#[cfg(not(feature = "parallel"))]
criterion_main!(benches);
