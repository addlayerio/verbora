// `criterion_group!` expands to an undocumented function, and a benchmark
// harness is not public API, so the workspace-wide `missing_docs` lint is noise
// here.
#![allow(missing_docs)]

//! Criterion benchmarks for the tokenizers.
//!
//! Three things are measured, because they answer three different questions:
//!
//! * **Scaling** — the same tokenizer over documents spanning three orders of
//!   magnitude, reported as throughput, so a per-byte regression is visible
//!   rather than hidden inside a per-call number.
//! * **Cross-tokenizer cost** — every family on one fixed document, which is
//!   what shows the price of leaving the character-class scanner for a rewriting
//!   pipeline (Treebank, the sentence splitter) or a statistical model
//!   (Japanese).
//! * **API shape** — `tokens` (lazy), `tokenize` (collect) and `tokenize_into`
//!   (reused buffer) on identical input. This is the crate's central design
//!   claim: the convenience APIs are built on the iterator, so the iterator must
//!   be the cheapest of the three and the buffer-reusing form must beat the
//!   allocating one.
//!
//! Inputs come from `benches/data/words.json`, the shared word list every
//! tokenizer harness in the repo reads, so all of them are measured on
//! byte-identical data.

use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use verbora_tokenizers::{
    AggressiveTokenizer, AggressiveTokenizerDe, AggressiveTokenizerHi, AggressiveTokenizerIt,
    AggressiveTokenizerNo, AggressiveTokenizerRu, AggressiveTokenizerVi, CaseTokenizer,
    SentenceTokenizer, Tokenize, TokenizerJa, TreebankWordTokenizer, WordPunctTokenizer,
    WordTokenizer,
};

/// Document sizes, in words.
const SIZES: [usize; 4] = [16, 128, 1024, 8192];

/// Japanese text for the segmenter, repeated to reach the target sizes.
///
/// A synthetic word list would not exercise the model: TinySegmenter's weights
/// are keyed by actual characters and character-type transitions, so random
/// kana would take a completely different set of branches from real prose.
const JA_TEXT: &str = "計算機科学における字句解析とは、ソースコードを構成する文字の並びを、\
トークンの並びに変換することをいう。ここでいうトークンとは、意味を持つコードの最小単位のこと。";

/// Russian text, for the non-ASCII branch of the character-class scanner.
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
    let json: serde_json::Value = serde_json::from_str(&body).expect("valid bench data");
    json["words"]
        .as_array()
        .expect("word list")
        .iter()
        .map(|w| w.as_str().expect("string").to_owned())
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

/// A document with the punctuation, contractions and sentence boundaries that
/// the rewriting tokenizers actually branch on.
///
/// Tokenizing punctuation-free text would measure the fast path of
/// `TreebankWordTokenizer` and none of its seventeen rules.
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

        group.bench_with_input(
            BenchmarkId::new("aggressive-en", doc.len()),
            &doc,
            |b, d| {
                b.iter(|| AggressiveTokenizer::new().tokenize(black_box(d)).len());
            },
        );
        group.bench_with_input(BenchmarkId::new("word", doc.len()), &doc, |b, d| {
            b.iter(|| WordTokenizer::new().tokenize(black_box(d)).map(|v| v.len()));
        });
        group.bench_with_input(BenchmarkId::new("wordpunct", doc.len()), &doc, |b, d| {
            b.iter(|| {
                WordPunctTokenizer::new()
                    .tokenize(black_box(d))
                    .map(|v| v.len())
            });
        });
        group.bench_with_input(BenchmarkId::new("case", doc.len()), &doc, |b, d| {
            b.iter(|| CaseTokenizer::new().tokenize(black_box(d)).len());
        });

        let text = prose(&words, n);
        group.throughput(Throughput::Bytes(text.len() as u64));
        group.bench_with_input(BenchmarkId::new("treebank", text.len()), &text, |b, d| {
            b.iter(|| TreebankWordTokenizer::new().tokenize(black_box(d)).len());
        });
        group.bench_with_input(BenchmarkId::new("sentence", text.len()), &text, |b, d| {
            b.iter(|| SentenceTokenizer::new().tokenize(black_box(d)).len());
        });
    }
    group.finish();
}

fn languages(c: &mut Criterion) {
    let words = words();
    let ascii = document(&words, 1024);
    let cyrillic = repeat_to(RU_TEXT, ascii.len());
    let japanese = repeat_to(JA_TEXT, 4096);

    let mut group = c.benchmark_group("languages");
    group.throughput(Throughput::Bytes(ascii.len() as u64));
    // All-ASCII input: the scanner never leaves its byte fast path.
    group.bench_function("en", |b| {
        b.iter(|| AggressiveTokenizer::new().tokenize(black_box(&ascii)).len());
    });
    group.bench_function("it", |b| {
        b.iter(|| {
            AggressiveTokenizerIt::new()
                .tokenize(black_box(&ascii))
                .len()
        });
    });
    group.bench_function("de", |b| {
        b.iter(|| {
            AggressiveTokenizerDe::new()
                .tokenize(black_box(&ascii))
                .len()
        });
    });
    // Latin-1 and Latin Extended classes: wider `matches!` ladders.
    group.bench_function("vi", |b| {
        b.iter(|| {
            AggressiveTokenizerVi::new()
                .tokenize(black_box(&ascii))
                .len()
        });
    });
    // Rewriting variants: diacritic removal and punctuation deletion.
    group.bench_function("no", |b| {
        b.iter(|| {
            AggressiveTokenizerNo::new()
                .tokenize(black_box(&ascii))
                .len()
        });
    });
    group.bench_function("hi", |b| {
        b.iter(|| {
            AggressiveTokenizerHi::new()
                .tokenize(black_box(&ascii))
                .len()
        });
    });

    group.throughput(Throughput::Bytes(cyrillic.len() as u64));
    group.bench_function("ru-cyrillic", |b| {
        b.iter(|| {
            AggressiveTokenizerRu::new()
                .tokenize(black_box(&cyrillic))
                .len()
        });
    });
    // Regression guard for the block-mask scanner: WordTokenizer's class
    // *includes* Cyrillic, so on this input word runs live outside the ASCII
    // fast path entirely. A regression here means the block scan's non-ASCII
    // handoff got more expensive, which the all-ASCII rows cannot show.
    group.bench_function("word-cyrillic", |b| {
        b.iter(|| {
            WordTokenizer::new()
                .tokenize(black_box(&cyrillic))
                .map(|v| v.len())
        });
    });

    group.throughput(Throughput::Bytes(japanese.len() as u64));
    group.bench_function("ja-segmenter", |b| {
        b.iter(|| TokenizerJa::new().tokenize(black_box(&japanese)).len());
    });
    group.finish();
}

fn api_shape(c: &mut Criterion) {
    let words = words();
    let doc = document(&words, 1024);
    let t = AggressiveTokenizer::new();

    let mut group = c.benchmark_group("api-shape");
    group.throughput(Throughput::Bytes(doc.len() as u64));

    // The primitive: no `Vec`, no allocation at all.
    group.bench_function("tokens-lazy", |b| {
        b.iter(|| t.tokens(black_box(&doc)).map(str::len).sum::<usize>());
    });
    // The convenience API: one growing `Vec` per call.
    group.bench_function("tokenize", |b| {
        b.iter(|| t.tokenize(black_box(&doc)).len());
    });
    // The hot-loop API: the caller's buffer keeps its capacity.
    group.bench_function("tokenize-into-reused", |b| {
        let mut buf = Vec::new();
        b.iter(|| {
            buf.clear();
            t.tokenize_into(black_box(&doc), &mut buf);
            buf.len()
        });
    });
    group.finish();
}

/// Sequential vs. `par_tokenize_batch` over batches of independent documents.
///
/// Requires the `parallel` feature.
///
/// Three cases span the range that matters for the "when is this worth it"
/// question `Tokenize::par_tokenize_batch`'s doc comment answers: many small
/// documents (scheduling overhead can dominate), a middling size, and
/// documents around the ~8192-word size this crate's own benchmarks already
/// measure a single `tokenize` call on (see `scaling`, above). Every case
/// uses the same document count so `Throughput::Elements` reports a clean
/// per-document number and the sequential/parallel ratio at each size is
/// directly comparable.
#[cfg(feature = "parallel")]
fn parallel_batch(c: &mut Criterion) {
    let words = words();
    let t = AggressiveTokenizer::new();

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
                    .map(|d| t.tokenize(black_box(d)))
                    .collect::<Vec<_>>()
            });
        });
        group.bench_with_input(BenchmarkId::new("parallel", n_words), &docs, |b, docs| {
            b.iter(|| t.par_tokenize_batch(black_box(docs)));
        });
    }
    group.finish();
}

criterion_group!(benches, scaling, languages, api_shape);
#[cfg(feature = "parallel")]
criterion_group!(parallel_benches, parallel_batch);

#[cfg(feature = "parallel")]
criterion_main!(benches, parallel_benches);
#[cfg(not(feature = "parallel"))]
criterion_main!(benches);
