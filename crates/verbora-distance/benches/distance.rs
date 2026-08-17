// `criterion_group!` expands to an undocumented function, and a benchmark
// harness is not public API, so the workspace-wide `missing_docs` lint is noise
// here.
#![allow(missing_docs)]

//! Criterion benchmarks for the distance metrics.
//!
//! Input sizes span four orders of magnitude so that scaling behaviour — not
//! just single-call latency — is visible. See `docs/PERFORMANCE.md` for the
//! paired results against the reference implementation.

use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use verbora_distance::{
    dice_coefficient, hamming, jaro_winkler,
    levenshtein::{Options, damerau_levenshtein, levenshtein, levenshtein_search},
};
#[cfg(feature = "parallel")]
use verbora_distance::{
    par_dice_coefficient_batch, par_hamming_batch, par_jaro_winkler_batch, par_levenshtein_batch,
};

/// The shared benchmark inputs, read from `benches/data/distance-pairs.json`.
///
/// Every harness that benchmarks string distance reads this same file, so all
/// numbers are measured on byte-identical data. Regenerate with
/// `python3 tools/bench-data/generate.py`.
struct Inputs {
    /// ASCII pairs keyed by length.
    ascii: Vec<(usize, String, String)>,
    /// BMP non-ASCII pairs, measuring the cost of leaving the ASCII fast path.
    cyrillic: Vec<(usize, String, String)>,
}

/// Reads the shared inputs, failing loudly if they have not been generated.
fn load_inputs() -> Inputs {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap()
        .join("benches/data/distance-pairs.json");
    let body = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "cannot read {}: {e}\nGenerate it with: python3 tools/bench-data/generate.py",
            path.display()
        )
    });
    let json: serde_json::Value = serde_json::from_str(&body).expect("valid bench data");

    let take = |key: &str| -> Vec<(usize, String, String)> {
        let mut v: Vec<(usize, String, String)> = json["pairs"][key]
            .as_object()
            .expect("pair map")
            .iter()
            .map(|(k, pair)| {
                (
                    k.parse().expect("numeric key"),
                    pair[0].as_str().unwrap().to_owned(),
                    pair[1].as_str().unwrap().to_owned(),
                )
            })
            .collect();
        v.sort_by_key(|(n, _, _)| *n);
        v
    };

    Inputs {
        ascii: take("ascii"),
        cyrillic: take("cyrillic"),
    }
}

fn bench_levenshtein(c: &mut Criterion) {
    let inputs = load_inputs();
    let mut g = c.benchmark_group("levenshtein");
    let opts = Options::default();

    for (n, a, b) in &inputs.ascii {
        // Throughput in "cells" makes the quadratic core comparable across sizes.
        g.throughput(Throughput::Elements((n * n) as u64));
        g.bench_with_input(BenchmarkId::new("ascii", n), n, |bench, _| {
            bench.iter(|| levenshtein(black_box(a), black_box(b), &opts));
        });
    }

    for (n, a, b) in &inputs.cyrillic {
        g.throughput(Throughput::Elements((n * n) as u64));
        g.bench_with_input(BenchmarkId::new("cyrillic", n), n, |bench, _| {
            bench.iter(|| levenshtein(black_box(a), black_box(b), &opts));
        });
    }

    g.finish();
}

fn bench_levenshtein_variants(c: &mut Criterion) {
    let inputs = load_inputs();
    let (_, a, b) = inputs
        .ascii
        .iter()
        .find(|(n, _, _)| *n == 64)
        .expect("64-char pair");
    let mut g = c.benchmark_group("levenshtein_variants");

    let plain = Options::default();
    let restricted = Options {
        restricted: true,
        ..Options::default()
    };
    let unrestricted = Options {
        restricted: false,
        ..Options::default()
    };

    // These exercise the two-row, three-row and full-matrix code paths
    // respectively; the gap between them is the payoff for not always
    // materialising the matrix.
    g.bench_function("plain_2row", |bench| {
        bench.iter(|| levenshtein(black_box(a), black_box(b), &plain));
    });
    g.bench_function("damerau_restricted_3row", |bench| {
        bench.iter(|| damerau_levenshtein(black_box(a), black_box(b), &restricted));
    });
    g.bench_function("damerau_unrestricted_matrix", |bench| {
        bench.iter(|| damerau_levenshtein(black_box(a), black_box(b), &unrestricted));
    });
    g.bench_function("search_matrix", |bench| {
        bench.iter(|| levenshtein_search(black_box(a), black_box(b), &plain));
    });

    g.finish();
}

fn bench_jaro_winkler(c: &mut Criterion) {
    let inputs = load_inputs();
    let mut g = c.benchmark_group("jaro_winkler");
    let opts = Default::default();
    for (n, a, b) in &inputs.ascii {
        g.throughput(Throughput::Elements((n * n) as u64));
        g.bench_with_input(BenchmarkId::from_parameter(n), n, |bench, _| {
            bench.iter(|| jaro_winkler(black_box(a), black_box(b), &opts));
        });
    }
    g.finish();
}

fn bench_dice(c: &mut Criterion) {
    let inputs = load_inputs();
    let mut g = c.benchmark_group("dice");
    for (n, a, b) in &inputs.ascii {
        g.throughput(Throughput::Bytes(2 * *n as u64));
        g.bench_with_input(BenchmarkId::from_parameter(n), n, |bench, _| {
            bench.iter(|| dice_coefficient(black_box(a), black_box(b)));
        });
    }
    g.finish();
}

fn bench_hamming(c: &mut Criterion) {
    let inputs = load_inputs();
    let mut g = c.benchmark_group("hamming");
    for (n, a, b) in &inputs.ascii {
        g.throughput(Throughput::Bytes(*n as u64));
        g.bench_with_input(BenchmarkId::from_parameter(n), n, |bench, _| {
            bench.iter(|| hamming(black_box(a), black_box(b), false));
        });
    }
    g.finish();
}

// ---------------------------------------------------------------------------
// Parallel batch (feature = "parallel")
// ---------------------------------------------------------------------------
//
// Each `par_*_batch` function is architecturally required to be a thin
// `par_iter().map(<sequential fn>).collect()` fan-out (see each function's
// doc comment) — so these benchmarks are not proving the parallel path is
// *correct* (`tests/parallel.rs` does that), they are measuring where it
// pays off. `site/performance/parallelism.md` states plainly that "no
// crossover point has been measured for Verbora"; this is what measuring it
// looks like for this crate's own metrics.
//
// Two axes are varied, deliberately kept separate:
// - `par_levenshtein_by_length`: batch size fixed, pair length swept — shows
//   how per-pair cost (tens of ns to single-digit ms, see `docs/PERFORMANCE.md`)
//   changes the trade-off.
// - `par_levenshtein_by_batch_size`: pair length fixed at a representative 64
//   characters, batch size swept — shows the crossover in batch count at one
//   fixed per-pair cost.
//
// Batch sizes here are the benchmark's own choice of "a few realistic sizes",
// not part of the library's API: `par_*_batch` takes whatever slice the
// caller passes, with no chunking or size-based dispatch inside it.

#[cfg(feature = "parallel")]
fn bench_par_levenshtein_by_length(c: &mut Criterion) {
    let inputs = load_inputs();
    let opts = Options::default();
    const BATCH: usize = 300;

    let mut g = c.benchmark_group("par_levenshtein_by_length");
    // 1024-char pairs cost single-digit milliseconds each (see
    // `docs/PERFORMANCE.md`); a lower sample count keeps this benchmark's
    // total run time reasonable without weakening the comparison, since both
    // sides are measured with the same sample count.
    g.sample_size(10);

    for (n, a, b) in &inputs.ascii {
        let pairs: Vec<(&str, &str)> =
            std::iter::repeat_n((a.as_str(), b.as_str()), BATCH).collect();
        g.throughput(Throughput::Elements(BATCH as u64));

        g.bench_with_input(BenchmarkId::new("sequential", n), n, |bench, _| {
            bench.iter(|| {
                black_box(&pairs)
                    .iter()
                    .map(|(a, b)| levenshtein(a, b, &opts))
                    .collect::<Vec<_>>()
            });
        });
        g.bench_with_input(BenchmarkId::new("parallel", n), n, |bench, _| {
            bench.iter(|| par_levenshtein_batch(black_box(&pairs), &opts));
        });
    }
    g.finish();
}

#[cfg(feature = "parallel")]
fn bench_par_levenshtein_by_batch_size(c: &mut Criterion) {
    let inputs = load_inputs();
    let opts = Options::default();
    let (_, a, b) = inputs
        .ascii
        .iter()
        .find(|(n, _, _)| *n == 64)
        .expect("64-char pair");

    let mut g = c.benchmark_group("par_levenshtein_by_batch_size");
    for &batch in &[16usize, 128, 1024, 8192] {
        let pairs: Vec<(&str, &str)> =
            std::iter::repeat_n((a.as_str(), b.as_str()), batch).collect();
        g.throughput(Throughput::Elements(batch as u64));

        g.bench_with_input(BenchmarkId::new("sequential", batch), &batch, |bench, _| {
            bench.iter(|| {
                black_box(&pairs)
                    .iter()
                    .map(|(a, b)| levenshtein(a, b, &opts))
                    .collect::<Vec<_>>()
            });
        });
        g.bench_with_input(BenchmarkId::new("parallel", batch), &batch, |bench, _| {
            bench.iter(|| par_levenshtein_batch(black_box(&pairs), &opts));
        });
    }
    g.finish();
}

#[cfg(feature = "parallel")]
fn bench_par_jaro_winkler(c: &mut Criterion) {
    let inputs = load_inputs();
    let opts = Default::default();
    const BATCH: usize = 1000;

    let mut g = c.benchmark_group("par_jaro_winkler");
    g.sample_size(10); // 1024-char pairs still cost ~170 µs each; keep runtime bounded.

    for (n, a, b) in &inputs.ascii {
        let pairs: Vec<(&str, &str)> =
            std::iter::repeat_n((a.as_str(), b.as_str()), BATCH).collect();
        g.throughput(Throughput::Elements(BATCH as u64));

        g.bench_with_input(BenchmarkId::new("sequential", n), n, |bench, _| {
            bench.iter(|| {
                black_box(&pairs)
                    .iter()
                    .map(|(a, b)| jaro_winkler(a, b, &opts))
                    .collect::<Vec<_>>()
            });
        });
        g.bench_with_input(BenchmarkId::new("parallel", n), n, |bench, _| {
            bench.iter(|| par_jaro_winkler_batch(black_box(&pairs), &opts));
        });
    }
    g.finish();
}

#[cfg(feature = "parallel")]
fn bench_par_dice(c: &mut Criterion) {
    let inputs = load_inputs();
    const BATCH: usize = 1000;

    let mut g = c.benchmark_group("par_dice");
    for (n, a, b) in &inputs.ascii {
        let pairs: Vec<(&str, &str)> =
            std::iter::repeat_n((a.as_str(), b.as_str()), BATCH).collect();
        g.throughput(Throughput::Elements(BATCH as u64));

        g.bench_with_input(BenchmarkId::new("sequential", n), n, |bench, _| {
            bench.iter(|| {
                black_box(&pairs)
                    .iter()
                    .map(|(a, b)| dice_coefficient(a, b))
                    .collect::<Vec<_>>()
            });
        });
        g.bench_with_input(BenchmarkId::new("parallel", n), n, |bench, _| {
            bench.iter(|| par_dice_coefficient_batch(black_box(&pairs)));
        });
    }
    g.finish();
}

#[cfg(feature = "parallel")]
fn bench_par_hamming(c: &mut Criterion) {
    let inputs = load_inputs();
    const BATCH: usize = 1000;

    let mut g = c.benchmark_group("par_hamming");
    for (n, a, b) in &inputs.ascii {
        let pairs: Vec<(&str, &str)> =
            std::iter::repeat_n((a.as_str(), b.as_str()), BATCH).collect();
        g.throughput(Throughput::Elements(BATCH as u64));

        g.bench_with_input(BenchmarkId::new("sequential", n), n, |bench, _| {
            bench.iter(|| {
                black_box(&pairs)
                    .iter()
                    .map(|(a, b)| hamming(a, b, false))
                    .collect::<Vec<_>>()
            });
        });
        g.bench_with_input(BenchmarkId::new("parallel", n), n, |bench, _| {
            bench.iter(|| par_hamming_batch(black_box(&pairs), false));
        });
    }
    g.finish();
}

criterion_group!(
    benches,
    bench_levenshtein,
    bench_levenshtein_variants,
    bench_jaro_winkler,
    bench_dice,
    bench_hamming
);

#[cfg(feature = "parallel")]
criterion_group!(
    parallel_benches,
    bench_par_levenshtein_by_length,
    bench_par_levenshtein_by_batch_size,
    bench_par_jaro_winkler,
    bench_par_dice,
    bench_par_hamming,
);

#[cfg(feature = "parallel")]
criterion_main!(benches, parallel_benches);
#[cfg(not(feature = "parallel"))]
criterion_main!(benches);
