// `criterion_group!` expands to an undocumented function, and a benchmark
// harness is not public API, so the workspace-wide `missing_docs` lint is noise
// here.
#![allow(missing_docs)]

//! Criterion benchmarks for `verbora-transliterators`.
//!
//! Three costs are separated, because they have completely different shapes:
//!
//! * **Rejection** — text the romanizer has nothing to do with: Latin prose,
//!   kanji, halfwidth katakana. This is the common case for any pipeline that
//!   runs the romanizer over mixed-language documents, and it should cost one
//!   vectorised scan of the input and no allocation at all.
//! * **Work** — kana throughout, so the scan both matches and rewrites. This
//!   measures the index lookups and the output buffer.
//! * **The lazy primitive on its own** — `Rewrites` counted rather than
//!   spliced, which allocates nothing and is the floor the splicing numbers
//!   are measured against.
//!
//! None of these has been run against the current implementation. No figure
//! from an earlier one is repeated anywhere in this crate.

use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};

use verbora_transliterators::{
    Rewrites, transliterate_ja, transliterate_ja_into, transliterate_ja_normalized,
};

/// Kana-only prose: every character is rewritten by some phase.
fn kana_prose(repeats: usize) -> String {
    "とうきょうとっきょきょかきょくのぼーじょれーぬーゔぉー".repeat(repeats)
}

/// Katakana loanwords: the digraph and small-vowel paths, which are where the
/// two- and three-character keys live.
fn katakana_prose(repeats: usize) -> String {
    "アヴァンギャルドなドキュメントィンドウとフューチャー".repeat(repeats)
}

/// Realistic mixed text: kana, kanji, ASCII and punctuation interleaved. Kanji
/// and ASCII are pure scan cost; only the kana is rewritten.
fn mixed_prose(repeats: usize) -> String {
    "これは日本語のテストです。ABC 123 と漢字、それに時々刻々の変化。".repeat(repeats)
}

/// English prose: no kana at all, so the whole-input gate should reject it.
fn ascii_prose(repeats: usize) -> String {
    "the quick brown fox jumps over the lazy dog ".repeat(repeats)
}

/// Halfwidth katakana: Japanese, but invisible to every table — the case that
/// motivates `transliterate_normalized`.
fn halfwidth_prose(repeats: usize) -> String {
    "ｺﾚﾊﾆﾎﾝｺﾞﾉﾃｽﾄﾃﾞｽ｡".repeat(repeats)
}

fn bench_transliterate(c: &mut Criterion) {
    let mut group = c.benchmark_group("transliterate");

    for &repeats in &[1usize, 16, 256] {
        for (label, input) in [
            ("kana", kana_prose(repeats)),
            ("katakana", katakana_prose(repeats)),
            ("mixed", mixed_prose(repeats)),
        ] {
            group.throughput(Throughput::Bytes(input.len() as u64));
            group.bench_with_input(BenchmarkId::new(label, input.len()), &input, |b, s| {
                b.iter(|| transliterate_ja(black_box(s)))
            });
        }
    }

    // The rejection paths. Both return `Cow::Borrowed` without allocating.
    for (id, input) in [
        ("ascii-rejection", ascii_prose(64)),
        ("halfwidth-rejection", halfwidth_prose(64)),
    ] {
        group.throughput(Throughput::Bytes(input.len() as u64));
        group.bench_with_input(BenchmarkId::from_parameter(id), &input, |b, s| {
            b.iter(|| transliterate_ja(black_box(s)))
        });
    }

    group.finish();
}

fn bench_rewrites(c: &mut Criterion) {
    let mut group = c.benchmark_group("transliterate_rewrites");

    // The lazy primitive with nothing built on top. `Rewrites` allocates
    // nothing at all, so the difference between this and the matching
    // `transliterate/*` row is exactly what the output `String` costs.
    for (label, input) in [
        ("kana", kana_prose(16)),
        ("mixed", mixed_prose(16)),
        ("ascii", ascii_prose(16)),
    ] {
        group.throughput(Throughput::Bytes(input.len() as u64));
        group.bench_with_input(BenchmarkId::from_parameter(label), &input, |b, s| {
            b.iter(|| Rewrites::new(black_box(s)).count())
        });
    }

    group.finish();
}

fn bench_buffered(c: &mut Criterion) {
    let mut group = c.benchmark_group("transliterate_buffered");
    let words: Vec<String> = (0..64).map(|i| kana_prose(1 + i % 3)).collect();
    let total: usize = words.iter().map(String::len).sum();
    group.throughput(Throughput::Bytes(total as u64));

    // A caller that reuses one buffer across many inputs: the shape a document
    // pipeline actually has, and the reason `transliterate_ja_into` exists.
    group.bench_function("reused_buffer", |b| {
        let mut buf = String::new();
        b.iter(|| {
            buf.clear();
            for w in &words {
                transliterate_ja_into(black_box(w), &mut buf);
            }
            buf.len()
        });
    });

    // The same work with a fresh `String` per input, for the difference.
    group.bench_function("fresh_allocation", |b| {
        b.iter(|| {
            let mut n = 0;
            for w in &words {
                n += transliterate_ja(black_box(w)).len();
            }
            n
        });
    });

    group.finish();
}

fn bench_normalized(c: &mut Criterion) {
    let mut group = c.benchmark_group("transliterate_normalized");
    for (id, input) in [
        ("halfwidth", halfwidth_prose(16)),
        ("mixed", mixed_prose(16)),
        ("ascii", ascii_prose(16)),
    ] {
        group.throughput(Throughput::Bytes(input.len() as u64));
        group.bench_with_input(BenchmarkId::from_parameter(id), &input, |b, s| {
            b.iter(|| transliterate_ja_normalized(black_box(s)))
        });
    }
    group.finish();
}

/// Sequential `transliterate_ja` vs. `par_transliterate_ja_batch`, at a few
/// realistic batch sizes. Requires the `parallel` feature; a no-op group
/// otherwise, so `criterion_group!` below stays a single, unconditional list.
fn bench_par_transliterate_ja_batch(c: &mut Criterion) {
    #[cfg(not(feature = "parallel"))]
    {
        let _ = c;
    }

    #[cfg(feature = "parallel")]
    {
        use verbora_transliterators::par_transliterate_ja_batch;

        let mut g = c.benchmark_group("par_transliterate_ja_batch");
        // One representative all-kana, document-scale input repeated out to a
        // few batch sizes: a small batch near rayon's scheduling break-even
        // point, and larger ones where the fan-out should win more clearly.
        // Where the break-even actually falls is unmeasured on this
        // implementation, which is why the group spans three sizes rather
        // than asserting one.
        let doc = kana_prose(290);
        for &n in &[4usize, 32, 256] {
            let docs: Vec<&str> = std::iter::repeat_n(doc.as_str(), n).collect();
            g.throughput(Throughput::Bytes(doc.len() as u64 * n as u64));
            g.bench_with_input(BenchmarkId::new("sequential", n), &docs, |b, docs| {
                b.iter(|| {
                    let mut total = 0usize;
                    for s in docs {
                        total += transliterate_ja(black_box(s)).len();
                    }
                    total
                });
            });
            g.bench_with_input(BenchmarkId::new("parallel", n), &docs, |b, docs| {
                b.iter(|| par_transliterate_ja_batch(black_box(docs)));
            });
        }
        g.finish();
    }
}

criterion_group!(
    benches,
    bench_transliterate,
    bench_rewrites,
    bench_buffered,
    bench_normalized,
    bench_par_transliterate_ja_batch
);
criterion_main!(benches);
