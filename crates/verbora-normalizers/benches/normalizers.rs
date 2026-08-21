#![allow(missing_docs)]
//! Criterion benchmarks for `verbora-normalizers`.
//!
//! The groups separate the two costs that decide whether the `Cow` guarantee
//! is worth its existence:
//!
//! * **Rejection cost** — text a function has nothing to do with, which returns
//!   `Cow::Borrowed`. Every call in this crate pays a quick check (UAX #15 §9)
//!   to decide that, and the point of measuring it separately is to find out
//!   whether that check is cheap enough to be free in a pipeline that runs a
//!   normalizer over every document.
//! * **Work cost** — text that is substantially replacements, which measures
//!   the decomposition, the filter and the output buffer instead.
//!
//! No number from these benchmarks is published anywhere until a
//! full-precision campaign has been run on settled code; see
//! `docs/design/text-shaping-contract.md` §7.

use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};

use verbora_normalizers::{nfc, nfd, nfkc, nfkd, remove_diacritics};

/// English prose with no diacritics and no compatibility characters: pure
/// rejection, and the ASCII fast path in `remove_diacritics`.
fn ascii_prose(words: usize) -> String {
    "the quick brown fox jumps over the lazy dog "
        .repeat(words.div_ceil(9))
        .trim_end()
        .to_owned()
}

/// Latin text where roughly every fourth character carries a mark, precomposed.
fn accented_prose(repeats: usize) -> String {
    "crème brûlée à la française, naïve résumé of Ångström ".repeat(repeats)
}

/// The same text already decomposed, which skips the quick check's `Yes` arm
/// for NFD and takes the work path for NFC.
fn decomposed_prose(repeats: usize) -> String {
    nfd(&accented_prose(repeats)).into_owned()
}

/// Japanese mixing halfwidth katakana, fullwidth alphanumerics and kanji: the
/// input NFKC is for, and which NFC leaves alone.
fn japanese_prose(repeats: usize) -> String {
    "ｶﾀｶﾅのﾃｽﾄです。ＡＢＣ１２３と漢字、それに時々刻々の変化。".repeat(repeats)
}

/// Non-Latin text with no marks: every scalar must be rejected without work.
fn cyrillic_prose(repeats: usize) -> String {
    "Москва не сразу строилась ".repeat(repeats)
}

fn bench_forms(c: &mut Criterion) {
    let mut group = c.benchmark_group("forms");
    type Form = fn(&str) -> std::borrow::Cow<'_, str>;
    let forms: [(&str, Form); 4] = [("nfd", nfd), ("nfc", nfc), ("nfkd", nfkd), ("nfkc", nfkc)];

    let inputs = [
        ("ascii", ascii_prose(200)),
        ("accented-precomposed", accented_prose(20)),
        ("accented-decomposed", decomposed_prose(20)),
        ("japanese", japanese_prose(16)),
        ("cyrillic", cyrillic_prose(40)),
    ];
    for (shape, input) in &inputs {
        group.throughput(Throughput::Bytes(input.len() as u64));
        for (name, f) in forms {
            group.bench_with_input(BenchmarkId::new(name, shape), input, |b, s| {
                b.iter(|| f(black_box(s)));
            });
        }
    }
    group.finish();
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
    // Text with no marks at all still walks a full decomposition scan to prove
    // it: this is the cost the `Cow::Borrowed` guarantee charges non-Latin
    // callers who get nothing back for it.
    let cyrillic = cyrillic_prose(40);
    group.throughput(Throughput::Bytes(cyrillic.len() as u64));
    group.bench_function("cyrillic-rejection", |b| {
        b.iter(|| remove_diacritics(black_box(&cyrillic)));
    });
    // Hebrew with niqqud: dense non-zero-class marks, so almost every scalar
    // is dropped and the output is much shorter than the input.
    let hebrew = "שָׁלוֹם עֲלֵיכֶם ".repeat(60);
    group.throughput(Throughput::Bytes(hebrew.len() as u64));
    group.bench_function("hebrew-niqqud", |b| {
        b.iter(|| remove_diacritics(black_box(&hebrew)));
    });
    group.finish();
}

/// Sequential `remove_diacritics` vs. `par_remove_diacritics_batch`, at a few
/// batch sizes. Requires the `parallel` feature; a no-op group otherwise, so
/// `criterion_group!` below stays a single, unconditional list.
fn bench_par_remove_diacritics_batch(c: &mut Criterion) {
    #[cfg(not(feature = "parallel"))]
    {
        let _ = c;
    }

    #[cfg(feature = "parallel")]
    {
        use verbora_normalizers::par_remove_diacritics_batch;

        let mut g = c.benchmark_group("par_remove_diacritics_batch");
        // One representative accented document (~1.2 KB), repeated out to a
        // small batch near rayon's scheduling break-even point and two larger
        // ones where the fan-out should win.
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
    bench_forms,
    bench_remove_diacritics,
    bench_par_remove_diacritics_batch
);
criterion_main!(benches);
