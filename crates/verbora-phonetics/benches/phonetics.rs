// `criterion_group!` expands to an undocumented function, and a benchmark
// harness is not public API, so the workspace-wide `missing_docs` lint is noise
// here.
#![allow(missing_docs)]

//! Criterion benchmarks for the phonetic encoders.
//!
//! Phonetic encoding is per-word work on short strings, so the costs that matter
//! are the ones a naive implementation pays *per call*:
//!
//! * **allocation** — every encoder here builds its key into one `String`,
//!   after collecting the input's letters into one reused `Vec<u8>`;
//! * **input preparation** — the letter scan skips every scalar outside
//!   `A`-`Z`, and the `ascii_vs_accented` group prices what an accented corpus
//!   costs that scan;
//! * **case folding**, which is simple ASCII folding done during that same
//!   scan.
//!
//! Word input comes from `benches/data/words.json`, the same list the other
//! crates' benchmarks use, so figures are comparable across the workspace.

use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use verbora_phonetics::{DaitchMokotoff, DoubleMetaphone, Metaphone, SoundEx};

#[cfg(feature = "parallel")]
use std::time::Duration;

#[cfg(feature = "parallel")]
use rayon::prelude::*;
#[cfg(feature = "parallel")]
use verbora_phonetics::{DEFAULT_CHUNK_SIZE, par_encode_batch};

/// How many words each throughput measurement runs over.
const BATCH: usize = 2_000;

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

/// Realistic surnames: the input phonetic algorithms actually run on, and the
/// one that reaches the special-case branches (`SCH`, `CZ`, `GN`, `MC`, `-WSKI`).
const SURNAMES: [&str; 24] = [
    "Smith",
    "Schmidt",
    "Johnson",
    "Williams",
    "Brown",
    "Jones",
    "Mueller",
    "Garcia",
    "Rodriguez",
    "Anderson",
    "Thompson",
    "Knuth",
    "Czech",
    "McDonald",
    "Wojcik",
    "Szymanski",
    "Kowalski",
    "Dvorak",
    "Gutierrez",
    "Schwarzenegger",
    "Tchaikovsky",
    "Pfeifer",
    "Hochmeier",
    "Jankowski",
];

/// One benchmark per encoder over the shared word list.
fn bench_encoders(c: &mut Criterion) {
    let corpus = words();
    let batch: Vec<&str> = corpus.iter().take(BATCH).map(String::as_str).collect();

    let soundex = SoundEx::new();
    let metaphone = Metaphone::new();
    let double = DoubleMetaphone::new();
    let dm = DaitchMokotoff::new();

    let mut g = c.benchmark_group("encoders");
    g.throughput(Throughput::Elements(BATCH as u64));

    g.bench_function("soundex", |b| {
        b.iter(|| {
            let mut n = 0;
            for w in black_box(&batch) {
                n += soundex.process(w).len();
            }
            n
        });
    });
    g.bench_function("metaphone", |b| {
        b.iter(|| {
            let mut n = 0;
            for w in black_box(&batch) {
                n += metaphone.process(w).len();
            }
            n
        });
    });
    g.bench_function("double_metaphone", |b| {
        b.iter(|| {
            let mut n = 0;
            for w in black_box(&batch) {
                let code = double.process(w);
                n += code.primary().len() + code.alternate().map_or(0, str::len);
            }
            n
        });
    });
    g.bench_function("daitch_mokotoff", |b| {
        b.iter(|| {
            let mut n = 0;
            for w in black_box(&batch) {
                n += dm.process(w).len();
            }
            n
        });
    });

    g.finish();
}

/// The cost of an accented corpus.
///
/// The same words, with and without an accent. An accented scalar is skipped
/// rather than coded, so the difference here is the cost of decoding a
/// multi-byte scalar during the letter scan -- not a different code path.
fn bench_ascii_vs_accented(c: &mut Criterion) {
    let metaphone = Metaphone::new();
    let double = DoubleMetaphone::new();

    let ascii: Vec<String> = SURNAMES.iter().map(|s| (*s).to_owned()).collect();
    let accented: Vec<String> = SURNAMES.iter().map(|s| format!("{s}é")).collect();

    let mut g = c.benchmark_group("ascii_vs_accented");
    g.throughput(Throughput::Elements(SURNAMES.len() as u64));

    for (label, input) in [("ascii", &ascii), ("accented", &accented)] {
        g.bench_with_input(BenchmarkId::new("metaphone", label), input, |b, input| {
            b.iter(|| {
                let mut n = 0;
                for w in black_box(input) {
                    n += metaphone.process(w).len();
                }
                n
            });
        });
        g.bench_with_input(
            BenchmarkId::new("double_metaphone", label),
            input,
            |b, input| {
                b.iter(|| {
                    let mut n = 0;
                    for w in black_box(input) {
                        n += double.process(w).primary().len();
                    }
                    n
                });
            },
        );
    }

    g.finish();
}

/// Realistic names, where the special-case branches actually fire.
///
/// Random letter strings take the cheap path through most rules; surnames are
/// what the algorithms were designed for and what a search index feeds them.
fn bench_surnames(c: &mut Criterion) {
    let soundex = SoundEx::new();
    let metaphone = Metaphone::new();
    let double = DoubleMetaphone::new();
    let dm = DaitchMokotoff::new();

    let mut g = c.benchmark_group("surnames");
    g.throughput(Throughput::Elements(SURNAMES.len() as u64));

    g.bench_function("soundex", |b| {
        b.iter(|| {
            SURNAMES
                .iter()
                .map(|w| soundex.process(black_box(w)).len())
                .sum::<usize>()
        });
    });
    g.bench_function("metaphone", |b| {
        b.iter(|| {
            SURNAMES
                .iter()
                .map(|w| metaphone.process(black_box(w)).len())
                .sum::<usize>()
        });
    });
    g.bench_function("double_metaphone", |b| {
        b.iter(|| {
            SURNAMES
                .iter()
                .map(|w| double.process(black_box(w)).primary().len())
                .sum::<usize>()
        });
    });
    g.bench_function("daitch_mokotoff", |b| {
        b.iter(|| {
            SURNAMES
                .iter()
                .map(|w| dm.process(black_box(w)).len())
                .sum::<usize>()
        });
    });

    g.finish();
}

/// `compare`, the operation a deduplicating index actually calls.
///
/// Double Metaphone's `compare` encodes both sides and matches on *either* key,
/// so it does twice the work of the single-key encoders — worth seeing next to
/// them rather than inferring.
fn bench_compare(c: &mut Criterion) {
    let soundex = SoundEx::new();
    let metaphone = Metaphone::new();
    let double = DoubleMetaphone::new();

    let pairs: Vec<(&str, &str)> = SURNAMES
        .iter()
        .zip(SURNAMES.iter().cycle().skip(1))
        .map(|(a, b)| (*a, *b))
        .collect();

    let mut g = c.benchmark_group("compare");
    g.throughput(Throughput::Elements(pairs.len() as u64));

    g.bench_function("soundex", |b| {
        b.iter(|| {
            pairs
                .iter()
                .filter(|(x, y)| soundex.compare(black_box(x), black_box(y)))
                .count()
        });
    });
    g.bench_function("metaphone", |b| {
        b.iter(|| {
            pairs
                .iter()
                .filter(|(x, y)| metaphone.compare(black_box(x), black_box(y)))
                .count()
        });
    });
    g.bench_function("double_metaphone", |b| {
        b.iter(|| {
            pairs
                .iter()
                .filter(|(x, y)| double.compare(black_box(x), black_box(y)))
                .count()
        });
    });

    g.finish();
}

/// `process` against `process_into` over a reused buffer.
///
/// `process` allocates one `String` per word; `process_into` appends into a
/// buffer the caller owns, so this group prices exactly one allocation per
/// call -- the only difference between the two entry points.
fn bench_process_into(c: &mut Criterion) {
    let m = Metaphone::new();
    let soundex = SoundEx::new();
    let corpus = words();
    let batch: Vec<&str> = corpus.iter().take(256).map(String::as_str).collect();

    let mut g = c.benchmark_group("process_into");
    g.throughput(Throughput::Elements(batch.len() as u64));

    g.bench_function("metaphone_process", |b| {
        b.iter(|| {
            batch
                .iter()
                .map(|w| m.process(black_box(w)).len())
                .sum::<usize>()
        });
    });
    g.bench_function("metaphone_process_into", |b| {
        let mut buf = String::with_capacity(64);
        b.iter(|| {
            let mut n = 0;
            for w in &batch {
                buf.clear();
                m.process_into(black_box(w), &mut buf);
                n += buf.len();
            }
            n
        });
    });
    g.bench_function("soundex_process_into", |b| {
        let mut buf = String::with_capacity(8);
        b.iter(|| {
            let mut n = 0;
            for w in &batch {
                buf.clear();
                soundex.process_into(black_box(w), &mut buf);
                n += buf.len();
            }
            n
        });
    });

    g.finish();
}

/// Sequential vs. naive per-word Rayon vs. chunked `par_encode_batch`, at
/// dictionary-index-sized inputs.
///
/// This is the benchmark the `parallel` feature's chunking strategy is
/// justified by. Six dispatch strategies are compared at each size:
///
/// * `sequential` — the plain `.iter().map(process).collect()` loop.
/// * `naive_per_word` — `tokens.par_iter().map(process).collect()`, one Rayon
///   task per word. This crate's own per-word costs (tens to a couple hundred
///   nanoseconds) are close to Rayon's per-task dispatch overhead, so this
///   measured as unreliable across repeated runs — sometimes markedly worse
///   than `sequential`, sometimes markedly better — rather than consistently
///   either. See the doc comment on [`verbora_phonetics::DEFAULT_CHUNK_SIZE`]
///   for what that run-to-run swing looked like.
/// * `chunked_<N>` — [`par_encode_batch`] with chunk size `N`, for a few
///   candidate chunk sizes including [`DEFAULT_CHUNK_SIZE`].
///
/// Sizes run from 10,000 (below where an index-building workload would
/// normally bother) to 100,000 (a realistic large dictionary), built by
/// cycling the shared word list so every size uses the same word
/// distribution.
#[cfg(feature = "parallel")]
fn bench_parallel_batch(c: &mut Criterion) {
    /// Below a typical index-build size, at a typical one, and above it.
    const SIZES: [usize; 3] = [10_000, 20_000, 100_000];

    /// Candidate chunk sizes for `par_encode_batch`, bracketing
    /// [`DEFAULT_CHUNK_SIZE`].
    const CHUNK_SIZES: [usize; 4] = [DEFAULT_CHUNK_SIZE, 256, 1024, 4096];

    let soundex = SoundEx::new();
    let corpus = words();

    let mut g = c.benchmark_group("parallel_batch");
    // This group's own point is the crossover, not tight confidence
    // intervals; keep total wall-clock time reasonable across 3 sizes x 6
    // strategies, while giving each enough samples to average out scheduler
    // noise from sharing a host with other work.
    g.sample_size(30);
    g.measurement_time(Duration::from_secs(3));
    g.warm_up_time(Duration::from_secs(1));

    for &n in &SIZES {
        let tokens: Vec<&str> = corpus.iter().map(String::as_str).cycle().take(n).collect();
        g.throughput(Throughput::Elements(n as u64));

        g.bench_with_input(BenchmarkId::new("sequential", n), &tokens, |b, tokens| {
            b.iter(|| {
                tokens
                    .iter()
                    .map(|t| soundex.process(black_box(t)))
                    .collect::<Vec<_>>()
            });
        });

        g.bench_with_input(
            BenchmarkId::new("naive_per_word", n),
            &tokens,
            |b, tokens| {
                b.iter(|| {
                    tokens
                        .par_iter()
                        .map(|t| soundex.process(black_box(*t)))
                        .collect::<Vec<_>>()
                });
            },
        );

        for &chunk_size in &CHUNK_SIZES {
            g.bench_with_input(
                BenchmarkId::new(format!("chunked_{chunk_size}"), n),
                &tokens,
                |b, tokens| {
                    b.iter(|| par_encode_batch(&soundex, black_box(tokens), chunk_size));
                },
            );
        }
    }

    g.finish();
}

criterion_group!(
    base_benches,
    bench_encoders,
    bench_ascii_vs_accented,
    bench_surnames,
    bench_compare,
    bench_process_into
);

#[cfg(feature = "parallel")]
criterion_group!(parallel_benches, bench_parallel_batch);

#[cfg(feature = "parallel")]
criterion_main!(base_benches, parallel_benches);
#[cfg(not(feature = "parallel"))]
criterion_main!(base_benches);
