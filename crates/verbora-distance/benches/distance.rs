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
    LevenshteinCosts, PreparedPattern, damerau_levenshtein, damerau_levenshtein_weighted,
    dice_coefficient, hamming, jaro_winkler, levenshtein, levenshtein_search, levenshtein_weighted,
    osa,
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

    for (n, a, b) in &inputs.ascii {
        // Report the logical n×n DP grid as normalized work; the bit-vector
        // implementation deliberately does not evaluate those cells one by one.
        g.throughput(Throughput::Elements((n * n) as u64));
        g.bench_with_input(BenchmarkId::new("ascii", n), n, |bench, _| {
            bench.iter(|| levenshtein(black_box(a), black_box(b)));
        });
    }

    for (n, a, b) in &inputs.cyrillic {
        g.throughput(Throughput::Elements((n * n) as u64));
        g.bench_with_input(BenchmarkId::new("cyrillic", n), n, |bench, _| {
            bench.iter(|| levenshtein(black_box(a), black_box(b)));
        });
    }

    g.finish();
}

fn bench_levenshtein_shapes(c: &mut Criterion) {
    let inputs = load_inputs();
    let mut g = c.benchmark_group("levenshtein_shapes");
    let (_, near_a, _) = inputs
        .ascii
        .iter()
        .find(|(n, _, _)| *n == 1024)
        .expect("1024-char pair");
    let mut near_b = near_a.clone().into_bytes();
    near_b[512] = if near_b[512] == b'x' { b'y' } else { b'x' };
    let near_b = String::from_utf8(near_b).unwrap();
    g.bench_function("near/1024", |bench| {
        bench.iter(|| levenshtein(black_box(near_a), black_box(&near_b)));
    });
    let near_unicode_a = "абвг".repeat(256);
    let mut near_unicode_chars: Vec<char> = near_unicode_a.chars().collect();
    near_unicode_chars[512] = 'д';
    let near_unicode_b: String = near_unicode_chars.into_iter().collect();
    g.bench_function("near_unicode/1024", |bench| {
        bench.iter(|| levenshtein(black_box(&near_unicode_a), black_box(&near_unicode_b)));
    });
    g.bench_function("empty_ascii/1024", |bench| {
        bench.iter(|| levenshtein(black_box(""), black_box(near_a)));
    });
    g.bench_function("empty_unicode/1024", |bench| {
        bench.iter(|| levenshtein(black_box(""), black_box(&near_unicode_a)));
    });
    let disjoint_a = "a".repeat(1024);
    let disjoint_b = "z".repeat(1024);
    g.bench_function("disjoint/1024", |bench| {
        bench.iter(|| levenshtein(black_box(&disjoint_a), black_box(&disjoint_b)));
    });
    let late_overlap_a = "z".repeat(65);
    let late_overlap_b = format!("{}zb", "a".repeat(9_998));
    g.bench_function("late_overlap/65x10000", |bench| {
        bench.iter(|| levenshtein(black_box(&late_overlap_a), black_box(&late_overlap_b)));
    });
    g.finish();
}

fn bench_levenshtein_weighted(c: &mut Criterion) {
    let inputs = load_inputs();
    let weighted = LevenshteinCosts::new(1.0, 1.0, 0.5).expect("admissible costs");
    let mut g = c.benchmark_group("levenshtein_weighted");
    for (n, a, b) in &inputs.ascii {
        if !matches!(*n, 16 | 64 | 256) {
            continue;
        }
        g.bench_with_input(BenchmarkId::from_parameter(n), n, |bench, _| {
            bench.iter(|| levenshtein_weighted(black_box(a), black_box(b), &weighted));
        });
    }

    let (_, short_a, _) = inputs
        .ascii
        .iter()
        .find(|(n, _, _)| *n == 16)
        .expect("16-char pair");
    let (_, long_b, _) = inputs
        .ascii
        .iter()
        .find(|(n, _, _)| *n == 1024)
        .expect("1024-char pair");
    g.bench_function("rectangular/16x1024", |bench| {
        bench.iter(|| levenshtein_weighted(black_box(short_a), black_box(long_b), &weighted));
    });
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

    let weighted = LevenshteinCosts::new(1.0, 1.0, 1.5).expect("admissible costs");
    let weighted_damerau =
        verbora_distance::DamerauCosts::new(1.0, 1.0, 1.5, 1.0).expect("admissible");

    // The three unit-cost kernels (Myers, Hyyro-OSA, Zhao-Sahni) plus the
    // weighted full-matrix fallback each Damerau variant shares.
    g.bench_function("plain_myers_unit", |bench| {
        bench.iter(|| levenshtein(black_box(a), black_box(b)));
    });
    g.bench_function("osa_bit_vector", |bench| {
        bench.iter(|| osa(black_box(a), black_box(b)));
    });
    g.bench_function("damerau_zhao_sahni", |bench| {
        bench.iter(|| damerau_levenshtein(black_box(a), black_box(b)));
    });
    g.bench_function("damerau_weighted_matrix", |bench| {
        bench.iter(|| damerau_levenshtein_weighted(black_box(a), black_box(b), &weighted_damerau));
    });
    g.bench_function("plain_weighted_rows", |bench| {
        bench.iter(|| levenshtein_weighted(black_box(a), black_box(b), &weighted));
    });
    g.bench_function("search_matrix", |bench| {
        bench.iter(|| levenshtein_search(black_box(a), black_box(b)));
    });

    g.finish();
}

/// The 1-vs-N screening shape: one query term against a candidate set, with
/// and without a prepared pattern.
///
/// Not read from `distance-pairs.json`, which holds *pairs* keyed by length
/// and has no candidate-set shape to offer. The tokens below are generated
/// from a fixed seed instead, so the corpus is still byte-identical between
/// runs, and the two arms see exactly the same one.
///
/// Construction is deliberately **inside** the timed closure. The question a
/// screening workload asks is not "how fast is a query once the table exists"
/// but "what does the whole scan cost", so the table build has to be paid for
/// out of the same budget and amortized over the candidates the way it would
/// be in the caller's loop.
fn bench_prepared_pattern(c: &mut Criterion) {
    /// Deterministic PRNG — a fixed corpus, not a random one.
    struct SplitMix64(u64);
    impl SplitMix64 {
        fn next_u64(&mut self) -> u64 {
            self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = self.0;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z ^ (z >> 31)
        }
    }

    let mut g = c.benchmark_group("prepared_pattern");
    const CANDIDATES: usize = 64;

    for len in [3usize, 7, 12, 24, 48] {
        let mut rng = SplitMix64(0x0DDB_A110_C0FF_EE00 ^ len as u64);
        let token = |rng: &mut SplitMix64| -> String {
            (0..len)
                .map(|_| (b'a' + (rng.next_u64() % 26) as u8) as char)
                .collect()
        };
        let pattern = token(&mut rng);
        let candidates: Vec<String> = (0..CANDIDATES).map(|_| token(&mut rng)).collect();

        g.throughput(Throughput::Elements(CANDIDATES as u64));
        g.bench_with_input(BenchmarkId::new("per_call", len), &len, |bench, _| {
            bench.iter(|| {
                let mut total = 0usize;
                for candidate in &candidates {
                    total += levenshtein(black_box(&pattern), black_box(candidate));
                }
                total
            });
        });
        g.bench_with_input(BenchmarkId::new("prepared", len), &len, |bench, _| {
            bench.iter(|| {
                let prepared = PreparedPattern::new(black_box(&pattern));
                let mut total = 0usize;
                for candidate in &candidates {
                    total += prepared.levenshtein(black_box(candidate));
                }
                total
            });
        });
    }

    g.finish();
}

fn bench_jaro_winkler(c: &mut Criterion) {
    let inputs = load_inputs();
    let mut g = c.benchmark_group("jaro_winkler");
    for (n, a, b) in &inputs.ascii {
        g.throughput(Throughput::Elements((n * n) as u64));
        g.bench_with_input(BenchmarkId::from_parameter(n), n, |bench, _| {
            bench.iter(|| jaro_winkler(black_box(a), black_box(b)));
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
            bench.iter(|| hamming(black_box(a), black_box(b)));
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
                    .map(|(a, b)| levenshtein(a, b))
                    .collect::<Vec<_>>()
            });
        });
        g.bench_with_input(BenchmarkId::new("parallel", n), n, |bench, _| {
            bench.iter(|| par_levenshtein_batch(black_box(&pairs)));
        });
    }
    g.finish();
}

#[cfg(feature = "parallel")]
fn bench_par_levenshtein_by_batch_size(c: &mut Criterion) {
    let inputs = load_inputs();
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
                    .map(|(a, b)| levenshtein(a, b))
                    .collect::<Vec<_>>()
            });
        });
        g.bench_with_input(BenchmarkId::new("parallel", batch), &batch, |bench, _| {
            bench.iter(|| par_levenshtein_batch(black_box(&pairs)));
        });
    }
    g.finish();
}

#[cfg(feature = "parallel")]
fn bench_par_jaro_winkler(c: &mut Criterion) {
    let inputs = load_inputs();
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
                    .map(|(a, b)| jaro_winkler(a, b))
                    .collect::<Vec<_>>()
            });
        });
        g.bench_with_input(BenchmarkId::new("parallel", n), n, |bench, _| {
            bench.iter(|| par_jaro_winkler_batch(black_box(&pairs)));
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
                    .map(|(a, b)| hamming(a, b))
                    .collect::<Vec<_>>()
            });
        });
        g.bench_with_input(BenchmarkId::new("parallel", n), n, |bench, _| {
            bench.iter(|| par_hamming_batch(black_box(&pairs)));
        });
    }
    g.finish();
}

criterion_group!(
    benches,
    bench_levenshtein,
    bench_levenshtein_shapes,
    bench_levenshtein_weighted,
    bench_levenshtein_variants,
    bench_prepared_pattern,
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
