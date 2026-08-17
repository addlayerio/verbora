// `criterion_group!` expands to an undocumented function, and a benchmark
// harness is not public API, so the workspace-wide `missing_docs` lint is noise
// here.
#![allow(missing_docs)]

//! Criterion benchmarks for n-gram generation.
//!
//! N-gram generation is *positional*: what the elements contain never changes
//! the control flow, only how much is copied. The benchmarks are therefore
//! organised around the three things that do change cost —
//!
//! * **sequence length**, which sets how many windows there are;
//! * **whether the result is borrowed or owned**, which is the whole point of
//!   the `Cow`-yielding iterator; and
//! * **the work bolted on top**: padding, frequency statistics, tokenizing.
//!
//! Token input comes from `benches/data/words.json`, the same word list the
//! other crates' benchmarks use, so figures are comparable across the workspace.

use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use verbora_core::Tokenizer as _;
use verbora_ngrams::zh::{code_units, ngrams_zh, ngrams_zh_utf16, split_lossy};
use verbora_ngrams::{WordTokenizer, ngrams, ngrams_iter, ngrams_owned, ngrams_str, tokenize};

/// Sequence lengths spanning three orders of magnitude.
const SIZES: [usize; 4] = [16, 256, 4_096, 20_000];

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

/// The window loop, borrowed versus materialised.
///
/// The gap between these two is the cost the reference pays unconditionally:
/// `sequence.slice(i, i + n)` allocates a fresh array per window.
fn bench_windows(c: &mut Criterion) {
    let corpus = words();
    let view: Vec<&str> = corpus.iter().map(String::as_str).collect();
    let mut g = c.benchmark_group("bigrams");

    for size in SIZES {
        let seq = &view[..size];
        g.throughput(Throughput::Elements(size as u64));

        // Streaming: nothing is allocated at all.
        g.bench_with_input(BenchmarkId::new("iter", size), &size, |b, _| {
            b.iter(|| {
                let mut total = 0usize;
                for gram in ngrams_iter(black_box(seq), 2, None, None) {
                    total += gram.len();
                }
                total
            });
        });

        // One `Vec` of borrowed windows: the recommended default.
        g.bench_with_input(BenchmarkId::new("collect_borrowed", size), &size, |b, _| {
            b.iter(|| ngrams(black_box(seq), 2, None, None));
        });

        // Every window copied — the shape the reference returns.
        g.bench_with_input(BenchmarkId::new("collect_owned", size), &size, |b, _| {
            b.iter(|| ngrams_owned(black_box(seq), 2, None, None));
        });
    }
    g.finish();
}

/// How cost scales with `n`, and what padding adds.
fn bench_n_and_padding(c: &mut Criterion) {
    let corpus = words();
    let view: Vec<&str> = corpus.iter().map(String::as_str).collect();
    let seq = &view[..4_096];
    let mut g = c.benchmark_group("ngrams");
    g.throughput(Throughput::Elements(seq.len() as u64));

    for n in [1usize, 2, 3, 5, 8] {
        g.bench_with_input(BenchmarkId::new("unpadded", n), &n, |b, &n| {
            b.iter(|| ngrams(black_box(seq), n, None, None));
        });
        // Padding adds 2(n-1) owned tuples — a fixed cost, not a per-window one,
        // so the two curves should stay parallel.
        g.bench_with_input(BenchmarkId::new("padded", n), &n, |b, &n| {
            b.iter(|| ngrams(black_box(seq), n, Some("<s>"), Some("</s>")));
        });
    }
    g.finish();
}

/// The statistics path: one key string built per n-gram, plus a hash lookup.
fn bench_stats(c: &mut Criterion) {
    let corpus = words();
    let view: Vec<&str> = corpus.iter().map(String::as_str).collect();
    let mut g = c.benchmark_group("stats");

    for size in [256usize, 4_096] {
        let seq = &view[..size];
        g.throughput(Throughput::Elements(size as u64));
        g.bench_with_input(BenchmarkId::new("bigrams", size), &size, |b, _| {
            b.iter(|| verbora_ngrams::ngrams_with_stats(black_box(seq), 2, None, None));
        });
        // A high-repetition sequence: the frequency map stays small, so this
        // isolates key construction from map growth.
        let repeated: Vec<&str> = seq.iter().map(|_| "the").collect();
        g.bench_with_input(BenchmarkId::new("bigrams_repeated", size), &size, |b, _| {
            b.iter(|| verbora_ngrams::ngrams_with_stats(black_box(&repeated), 2, None, None));
        });
    }
    g.finish();
}

/// String input: tokenizing dominates, which is why the pre-tokenized API exists.
fn bench_string_input(c: &mut Criterion) {
    let corpus = words();
    let text: String = corpus[..4_096].join(" ");
    let mut g = c.benchmark_group("string_input");
    g.throughput(Throughput::Bytes(text.len() as u64));

    g.bench_function("tokenize", |b| b.iter(|| tokenize(black_box(&text))));
    g.bench_function("ngrams_str", |b| {
        b.iter(|| ngrams_str(black_box(&text), 2, None, None));
    });
    // The cheap path: tokenize once, then window without copying.
    let tokens = WordTokenizer.tokenize(&text);
    g.bench_function("pretokenized", |b| {
        b.iter(|| ngrams(black_box(&tokens), 2, None, None));
    });
    g.finish();
}

/// The Chinese variant, which splits per UTF-16 code unit.
fn bench_zh(c: &mut Criterion) {
    // Content is irrelevant to the control flow, so a repeated phrase is a fair
    // and perfectly reproducible input.
    let text = "中文文本测试自然语言处理".repeat(1_000);
    let units = code_units(&text);
    let mut g = c.benchmark_group("zh");
    g.throughput(Throughput::Elements(units.len() as u64));

    g.bench_function("ngrams_zh", |b| {
        b.iter(|| ngrams_zh(black_box(&text), 2, None, None));
    });
    g.bench_function("ngrams_zh_utf16", |b| {
        b.iter(|| ngrams_zh_utf16(black_box(&units), 2, None, None));
    });
    // The cheap path: split once, then window without copying. No the reference
    // counterpart exists, which is exactly the point.
    let split = split_lossy(&text);
    g.bench_function("split_then_window", |b| {
        b.iter(|| ngrams(black_box(&split), 2, None, None));
    });
    g.finish();
}

criterion_group!(
    benches,
    bench_windows,
    bench_n_and_padding,
    bench_stats,
    bench_string_input,
    bench_zh
);
criterion_main!(benches);
