// `criterion_group!` expands to an undocumented function, and a benchmark
// harness is not public API, so the workspace-wide `missing_docs` lint is noise
// here.
#![allow(missing_docs)]

//! Criterion benchmarks for n-gram windows.
//!
//! N-gram generation is *positional*: what the elements contain never changes
//! the control flow, only how much is copied. The benchmarks are therefore
//! organised around the three things that do change cost —
//!
//! * **sequence length**, which sets how many windows there are;
//! * **the window width `n`**, which for [`Padded`] also sets how much padding
//!   is materialised; and
//! * **whether the windowed sequence is the caller's slice or a padded copy of
//!   it**, which is the one allocation this crate can make.
//!
//! Token input comes from `benches/data/words.json`, the same word list the
//! other crates' benchmarks use, so figures are comparable across the
//! workspace. The character benchmark uses a fixed mixed-script string:
//! `char_ngrams`' cost is driven by scalar width, not by content.

use std::hint::black_box;
use std::num::NonZeroUsize;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use verbora_ngrams::{Padded, char_ngrams, ngrams};

/// Sequence lengths spanning three orders of magnitude.
const SIZES: [usize; 4] = [16, 256, 4_096, 20_000];

/// Window widths: unigrams through the widest window anybody uses in practice.
const WIDTHS: [usize; 5] = [1, 2, 3, 5, 8];

/// A `NonZeroUsize` from a literal that is known not to be zero.
fn width(n: usize) -> NonZeroUsize {
    NonZeroUsize::new(n).expect("benchmark widths are non-zero")
}

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

/// The window loop itself: streamed, versus collected into a `Vec` of borrows.
///
/// Neither shape allocates per window — the gap between them is one `Vec`
/// growth for `len - n + 1` pointer-and-length pairs, and nothing else.
fn bench_windows(c: &mut Criterion) {
    let corpus = words();
    let view: Vec<&str> = corpus.iter().map(String::as_str).collect();
    let n = width(2);
    let mut g = c.benchmark_group("bigrams");

    for size in SIZES {
        let seq = &view[..size];
        g.throughput(Throughput::Elements(size as u64));

        // Streaming: nothing is allocated at all.
        g.bench_with_input(BenchmarkId::new("iter", size), &size, |b, _| {
            b.iter(|| {
                let mut total = 0usize;
                for window in ngrams(black_box(seq), n) {
                    total += window.len();
                }
                total
            });
        });

        // One `Vec` of borrowed windows: the recommended default.
        g.bench_with_input(BenchmarkId::new("collect", size), &size, |b, _| {
            b.iter(|| ngrams(black_box(seq), n).collect::<Vec<_>>());
        });
    }
    g.finish();
}

/// How cost scales with `n`, unpadded.
///
/// The window count falls as `n` rises, so the curve should be flat-to-falling;
/// what it must not do is grow, because no per-window work depends on `n`.
fn bench_width(c: &mut Criterion) {
    let corpus = words();
    let view: Vec<&str> = corpus.iter().map(String::as_str).collect();
    let seq = &view[..4_096];
    let mut g = c.benchmark_group("width");
    g.throughput(Throughput::Elements(seq.len() as u64));

    for n in WIDTHS {
        g.bench_with_input(BenchmarkId::new("unpadded", n), &n, |b, &n| {
            b.iter(|| ngrams(black_box(seq), width(n)).collect::<Vec<_>>());
        });
    }
    g.finish();
}

/// What padding costs: one `Vec` of `len + 2(n - 1)` clones, built once.
///
/// Construction and iteration are timed separately, because the whole design
/// claim is that the cost is in `Padded::new` and not in the windows.
fn bench_padding(c: &mut Criterion) {
    let corpus = words();
    let view: Vec<&str> = corpus.iter().map(String::as_str).collect();
    let seq = &view[..4_096];
    let mut g = c.benchmark_group("padding");
    g.throughput(Throughput::Elements(seq.len() as u64));

    for n in WIDTHS {
        g.bench_with_input(BenchmarkId::new("build", n), &n, |b, &n| {
            b.iter(|| Padded::new(black_box(seq), width(n), Some(&"<s>"), Some(&"</s>")));
        });

        let padded = Padded::new(seq, width(n), Some(&"<s>"), Some(&"</s>"));
        g.bench_with_input(BenchmarkId::new("windows", n), &n, |b, _| {
            b.iter(|| black_box(&padded).ngrams().collect::<Vec<_>>());
        });
    }
    g.finish();
}

/// Character windows over ASCII, over Latin-1 and over CJK.
///
/// `char_ngrams` counts the input's scalars once when it is created, so a
/// wider encoding means fewer windows over the same number of bytes; the three
/// inputs are sized in bytes so the throughput figures are comparable.
fn bench_char_ngrams(c: &mut Criterion) {
    let inputs: [(&str, String); 3] = [
        ("ascii", "the quick brown fox ".repeat(1_000)),
        ("latin1", "café naïve jamón crème ".repeat(1_000)),
        ("cjk", "中文文本测试自然语言处理".repeat(1_000)),
    ];
    let mut g = c.benchmark_group("char_ngrams");

    for (label, text) in &inputs {
        g.throughput(Throughput::Bytes(text.len() as u64));
        for n in [2usize, 3, 5] {
            g.bench_with_input(BenchmarkId::new(*label, n), &n, |b, &n| {
                b.iter(|| char_ngrams(black_box(text.as_str()), width(n)).count());
            });
        }
    }
    g.finish();
}

criterion_group!(
    benches,
    bench_windows,
    bench_width,
    bench_padding,
    bench_char_ngrams
);
criterion_main!(benches);
