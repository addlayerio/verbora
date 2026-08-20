// `criterion_group!` expands to an undocumented function, and a benchmark
// harness is not public API, so the workspace-wide `missing_docs` lint is noise
// here.
#![allow(missing_docs)]

//! Criterion benchmarks for the three UAX #29 tokenizers.
//!
//! Three things are measured, because they answer three different questions:
//!
//! * **Scaling** — the same tokenizer over documents spanning three orders of
//!   magnitude, reported as throughput, so a per-byte regression is visible
//!   rather than hidden inside a per-call number.
//! * **Script** — the same byte budget of ASCII, Cyrillic and Japanese. The
//!   boundary rules take a documented ASCII fast path inside
//!   `unicode-segmentation`, so an all-ASCII row cannot show what the general
//!   automaton costs.
//! * **API shape** — `tokens` (lazy), `tokenize_borrowed` (collect),
//!   `tokenize_borrowed_into` (reused buffer) and `tokenize` (owned) on
//!   identical input. This is the crate's central design claim: the convenience
//!   APIs are built on the iterator, so the iterator must be the cheapest and
//!   the buffer-reusing form must beat the allocating one.
//!
//! Inputs come from `benches/data/words.json`, the shared word list every
//! tokenizer harness in the repo reads, so all of them are measured on
//! byte-identical data.
//!
//! **No number from this file may be published without a fresh full-precision
//! run**; the implementation changed wholesale in the Rust-native migration and
//! every previously published figure measured code that no longer exists.

use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use verbora_tokenizers::{
    BorrowingTokenizer, SegmentTokenizer, SentenceTokenizer, Tokenizer, WordTokenizer,
};

/// Document sizes, in words.
const SIZES: [usize; 4] = [16, 128, 1024, 8192];

/// Japanese text: the case UAX #29 §4 explicitly does not segment, so every
/// scalar is its own token and the per-token overhead is at its worst.
const JA_TEXT: &str = "計算機科学における字句解析とは、ソースコードを構成する文字の並びを、\
トークンの並びに変換することをいう。ここでいうトークンとは、意味を持つコードの最小単位のこと。";

/// Russian text, for the non-ASCII branch of the boundary automaton.
const RU_TEXT: &str = "Быстрая коричневая лиса перепрыгивает через ленивую собаку. \
Это тест кириллического текста для измерения производительности токенизатора. ";

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
    // The file is `{"words": ["...", ...]}`; a hand-rolled scan avoids a
    // `serde_json` dev-dependency for one benchmark input.
    body.split('"')
        .skip(3)
        .step_by(2)
        .map(str::to_owned)
        .filter(|w| !w.is_empty())
        .collect()
}

/// A plain document of `n` words.
fn document(words: &[String], n: usize) -> String {
    words
        .iter()
        .cycle()
        .take(n)
        .cloned()
        .collect::<Vec<_>>()
        .join(" ")
}

/// A document with the punctuation, contractions and sentence boundaries the
/// boundary rules actually branch on.
///
/// Punctuation-free text measures WB5 and nothing else — not the
/// `MidLetter`/`MidNum`/`MidNumLet` lookahead of WB6/WB7/WB11/WB12, and none of
/// the sentence rules.
fn prose(words: &[String], n: usize) -> String {
    let mut out = String::new();
    for (i, w) in words.iter().cycle().take(n).enumerate() {
        match i % 12 {
            0 if i > 0 => out.push_str(". "),
            3 => out.push_str(", "),
            7 => out.push_str("'s "),
            _ => out.push(' '),
        }
        out.push_str(w);
    }
    out.push('.');
    out
}

/// Repeats `seed` until it is at least `bytes` long.
fn repeat_to(seed: &str, bytes: usize) -> String {
    let mut s = String::with_capacity(bytes + seed.len());
    while s.len() < bytes {
        s.push_str(seed);
    }
    s
}

fn scaling(c: &mut Criterion) {
    let words = words();
    let mut group = c.benchmark_group("scaling");
    for n in SIZES {
        let doc = document(&words, n);
        group.throughput(Throughput::Bytes(doc.len() as u64));
        group.bench_with_input(BenchmarkId::new("word", doc.len()), &doc, |b, d| {
            b.iter(|| WordTokenizer.tokenize_borrowed(black_box(d)).len());
        });
        group.bench_with_input(BenchmarkId::new("segment", doc.len()), &doc, |b, d| {
            b.iter(|| SegmentTokenizer.tokenize_borrowed(black_box(d)).len());
        });

        let text = prose(&words, n);
        group.throughput(Throughput::Bytes(text.len() as u64));
        group.bench_with_input(BenchmarkId::new("word-prose", text.len()), &text, |b, d| {
            b.iter(|| WordTokenizer.tokenize_borrowed(black_box(d)).len());
        });
        group.bench_with_input(BenchmarkId::new("sentence", text.len()), &text, |b, d| {
            b.iter(|| {
                SentenceTokenizer::new()
                    .tokenize_borrowed(black_box(d))
                    .len()
            });
        });
    }
    group.finish();
}

fn scripts(c: &mut Criterion) {
    let words = words();
    let ascii = document(&words, 1024);
    let cyrillic = repeat_to(RU_TEXT, ascii.len());
    let japanese = repeat_to(JA_TEXT, ascii.len());

    let mut group = c.benchmark_group("scripts");
    group.throughput(Throughput::Bytes(ascii.len() as u64));
    group.bench_function("word-ascii", |b| {
        b.iter(|| WordTokenizer.tokenize_borrowed(black_box(&ascii)).len());
    });
    group.throughput(Throughput::Bytes(cyrillic.len() as u64));
    group.bench_function("word-cyrillic", |b| {
        b.iter(|| WordTokenizer.tokenize_borrowed(black_box(&cyrillic)).len());
    });
    group.throughput(Throughput::Bytes(japanese.len() as u64));
    group.bench_function("word-japanese", |b| {
        b.iter(|| WordTokenizer.tokenize_borrowed(black_box(&japanese)).len());
    });
    group.finish();
}

fn api_shape(c: &mut Criterion) {
    let words = words();
    let doc = document(&words, 1024);
    let t = WordTokenizer;

    let mut group = c.benchmark_group("api-shape");
    group.throughput(Throughput::Bytes(doc.len() as u64));

    // The primitive: no `Vec`, no allocation at all.
    group.bench_function("tokens-lazy", |b| {
        b.iter(|| t.tokens(black_box(&doc)).map(str::len).sum::<usize>());
    });
    // One growing `Vec` of `&str` per call.
    group.bench_function("tokenize-borrowed", |b| {
        b.iter(|| t.tokenize_borrowed(black_box(&doc)).len());
    });
    // The hot-loop API: the caller's buffer keeps its capacity.
    group.bench_function("tokenize-borrowed-into-reused", |b| {
        let mut buf = Vec::new();
        b.iter(|| {
            buf.clear();
            t.tokenize_borrowed_into(black_box(&doc), &mut buf);
            buf.len()
        });
    });
    // The owned path: one `String` per token.
    group.bench_function("tokenize-owned", |b| {
        b.iter(|| t.tokenize(black_box(&doc)).len());
    });
    group.finish();
}

/// Sequential vs. `par_tokenize_batch` over batches of independent documents.
///
/// Requires the `parallel` feature. The crossover this measures is currently
/// **unmeasured** and no figure from it is published anywhere.
#[cfg(feature = "parallel")]
fn parallel_batch(c: &mut Criterion) {
    use verbora_tokenizers::par_tokenize_batch;

    let words = words();
    let t = WordTokenizer;

    const N_DOCS: usize = 64;
    const WORDS_PER_DOC: [usize; 3] = [16, 1024, 8192];

    let mut group = c.benchmark_group("parallel-batch");
    for n_words in WORDS_PER_DOC {
        let doc = document(&words, n_words);
        let docs: Vec<&str> = std::iter::repeat_n(doc.as_str(), N_DOCS).collect();

        group.throughput(Throughput::Elements(N_DOCS as u64));
        group.bench_with_input(BenchmarkId::new("sequential", n_words), &docs, |b, docs| {
            b.iter(|| {
                docs.iter()
                    .map(|d| t.tokenize_borrowed(black_box(d)))
                    .collect::<Vec<_>>()
            });
        });
        group.bench_with_input(BenchmarkId::new("parallel", n_words), &docs, |b, docs| {
            b.iter(|| par_tokenize_batch(&t, black_box(docs)));
        });
    }
    group.finish();
}

criterion_group!(benches, scaling, scripts, api_shape);
#[cfg(feature = "parallel")]
criterion_group!(parallel_benches, parallel_batch);

#[cfg(feature = "parallel")]
criterion_main!(benches, parallel_benches);
#[cfg(not(feature = "parallel"))]
criterion_main!(benches);
