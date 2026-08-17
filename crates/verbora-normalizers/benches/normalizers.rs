#![allow(missing_docs)]
//! Criterion benchmarks for `verbora-normalizers`.
//!
//! The groups are chosen to separate the two costs that actually matter:
//!
//! * **Rejection cost** — text the normalizer has nothing to do with. Latin prose
//!   handed to the katakana converter, ASCII handed to the diacritic folder. This
//!   is the common case in a pipeline that runs every normalizer over every
//!   document, and it should be close to a `memchr`-speed scan.
//! * **Work cost** — text that is entirely replacements, which measures the table
//!   lookup and the output buffer rather than the scan.
//!
//! `remove_diacritics` also carries a reference-shaped comparison: The reference
//! makes 86 sequential global-regex passes over the whole string, so its cost
//! scales with 86 x length while this port's scales with length.

use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};

use verbora_normalizers::ja::converters;
use verbora_normalizers::{
    normalize, normalize_ja, normalize_no, normalize_token, remove_diacritics,
};

/// English prose with no diacritics, no kana and no contractions: pure rejection.
fn ascii_prose(words: usize) -> String {
    "the quick brown fox jumps over the lazy dog "
        .repeat(words.div_ceil(9))
        .trim_end()
        .to_owned()
}

/// Latin text where roughly every fourth character folds.
fn accented_prose(repeats: usize) -> String {
    "crème brûlée à la française, naïve résumé of Ångström ".repeat(repeats)
}

/// Japanese text mixing halfwidth katakana, fullwidth alphanumerics and kanji.
fn japanese_prose(repeats: usize) -> String {
    "ｶﾀｶﾅのﾃｽﾄです。ＡＢＣ１２３と漢字、それに時々刻々の変化。".repeat(repeats)
}

fn bench_remove_diacritics(c: &mut Criterion) {
    let mut group = c.benchmark_group("remove_diacritics");
    for &len in &[64usize, 1_024, 16_384] {
        let ascii = ascii_prose(len / 5);
        let accented = accented_prose(len / 54 + 1);

        group.throughput(Throughput::Bytes(ascii.len() as u64));
        group.bench_with_input(BenchmarkId::new("ascii", ascii.len()), &ascii, |b, s| {
            b.iter(|| remove_diacritics(black_box(s)));
        });

        group.throughput(Throughput::Bytes(accented.len() as u64));
        group.bench_with_input(
            BenchmarkId::new("accented", accented.len()),
            &accented,
            |b, s| b.iter(|| remove_diacritics(black_box(s))),
        );
    }
    // Non-Latin text exercises the block bitmap: every character must be rejected
    // without touching the 820-entry table.
    let cyrillic = "Москва не сразу строилась ".repeat(40);
    group.throughput(Throughput::Bytes(cyrillic.len() as u64));
    group.bench_function("cyrillic-rejection", |b| {
        b.iter(|| remove_diacritics(black_box(&cyrillic)));
    });
    group.finish();
}

fn bench_nordic(c: &mut Criterion) {
    let mut group = c.benchmark_group("normalize_no");
    // Once every pair has fired the scan stops early, so a long tail after the
    // accents is essentially free — worth measuring separately from a short one.
    let short = "blåbærsyltetøy à la façon".to_owned();
    let long = format!("{short}{}", ascii_prose(400));
    for (name, input) in [("short", &short), ("accents-then-ascii", &long)] {
        group.throughput(Throughput::Bytes(input.len() as u64));
        group.bench_with_input(BenchmarkId::from_parameter(name), input, |b, s| {
            b.iter(|| normalize_no(black_box(s)));
        });
    }
    group.finish();
}

fn bench_english(c: &mut Criterion) {
    let mut group = c.benchmark_group("normalize");

    let no_contraction: Vec<&str> = "the quick brown fox jumps over the lazy dog"
        .split(' ')
        .collect();
    group.bench_function("tokens-without-contractions", |b| {
        b.iter(|| normalize(black_box(&no_contraction)));
    });

    let contractions = [
        "hasn't",
        "it's",
        "we'll",
        "how're",
        "we've",
        "I'd",
        "can't",
        "won't",
        "couldn't've",
        "i'm",
        "how'd",
    ];
    group.bench_function("tokens-with-contractions", |b| {
        b.iter(|| normalize(black_box(&contractions)));
    });

    // A single token that expands to 4,001 output tokens. This one is
    // allocator-bound rather than scanner-bound — the return type is
    // `Vec<String>` because the reference returns an array of strings, and one
    // small heap allocation per output token is what that costs. Kept honest and
    // separate so the realistic cases above are not read through it.
    let sentence = "hasn't ".repeat(500);
    group.throughput(Throughput::Bytes(sentence.len() as u64));
    group.bench_function("single-long-token", |b| {
        b.iter(|| normalize_token(black_box(&sentence)));
    });

    group.finish();
}

fn bench_japanese(c: &mut Criterion) {
    let mut group = c.benchmark_group("normalize_ja");
    for &repeats in &[1usize, 16, 256] {
        let input = japanese_prose(repeats);
        group.throughput(Throughput::Bytes(input.len() as u64));
        group.bench_with_input(BenchmarkId::new("mixed", input.len()), &input, |b, s| {
            b.iter(|| normalize_ja(black_box(s)))
        });
    }
    // Rejection: Latin prose still walks all four stages.
    let ascii = ascii_prose(200);
    group.throughput(Throughput::Bytes(ascii.len() as u64));
    group.bench_function("ascii-rejection", |b| {
        b.iter(|| normalize_ja(black_box(&ascii)));
    });
    // The iteration-mark stage is gated on U+3005 being present at all; this is
    // the case that pays for the UTF-16 round trip.
    let marks = "時々刻々と甲斐々々しい".repeat(64);
    group.throughput(Throughput::Bytes(marks.len() as u64));
    group.bench_function("iteration-marks", |b| {
        b.iter(|| normalize_ja(black_box(&marks)));
    });
    group.finish();
}

fn bench_converters(c: &mut Criterion) {
    let mut group = c.benchmark_group("converters");
    let input = japanese_prose(16);
    group.throughput(Throughput::Bytes(input.len() as u64));

    type Conv = fn(&str) -> std::borrow::Cow<'_, str>;
    let cases: [(&str, Conv); 6] = [
        ("katakana_hf", converters::katakana_hf),
        ("katakana_fh", converters::katakana_fh),
        ("normalize", converters::normalize),
        ("fix_fullwidth_kana", converters::fix_fullwidth_kana),
        ("fix_composite_symbols", converters::fix_composite_symbols),
        ("hiragana_to_katakana", converters::hiragana_to_katakana),
    ];
    for (name, f) in cases {
        group.bench_with_input(BenchmarkId::from_parameter(name), &input, |b, s| {
            b.iter(|| f(black_box(s)));
        });
    }
    group.finish();
}

/// Sequential `remove_diacritics` vs. `par_remove_diacritics_batch`, at a few
/// realistic batch sizes. Requires the `parallel` feature; a no-op group
/// otherwise, so `criterion_group!` below stays a single, unconditional list.
fn bench_par_remove_diacritics_batch(c: &mut Criterion) {
    #[cfg(not(feature = "parallel"))]
    {
        let _ = c;
    }

    #[cfg(feature = "parallel")]
    {
        use verbora_normalizers::par_remove_diacritics_batch;

        let mut g = c.benchmark_group("par_remove_diacritics_batch");
        // One representative accented document (~1.2 KB, roughly the
        // `remove_diacritics` "work cost" case above) repeated out to a few
        // batch sizes: a small batch close to rayon's scheduling break-even
        // point, and two larger ones where the fan-out should win clearly.
        let doc = accented_prose(20);
        for &n in &[16usize, 256, 4096] {
            let docs: Vec<&str> = std::iter::repeat_n(doc.as_str(), n).collect();
            g.throughput(Throughput::Elements(n as u64));
            g.bench_with_input(BenchmarkId::new("sequential", n), &docs, |b, docs| {
                b.iter(|| {
                    let mut total = 0usize;
                    for s in docs {
                        total += remove_diacritics(black_box(s)).len();
                    }
                    total
                });
            });
            g.bench_with_input(BenchmarkId::new("parallel", n), &docs, |b, docs| {
                b.iter(|| par_remove_diacritics_batch(black_box(docs)));
            });
        }
        g.finish();
    }
}

criterion_group!(
    benches,
    bench_remove_diacritics,
    bench_nordic,
    bench_english,
    bench_japanese,
    bench_converters,
    bench_par_remove_diacritics_batch
);
criterion_main!(benches);
