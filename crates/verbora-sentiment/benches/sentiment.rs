// `criterion_group!` expands to an undocumented function, and a benchmark
// harness is not public API, so the workspace-wide `missing_docs` lint is noise
// here.
#![allow(missing_docs)]

//! Criterion benchmarks for `verbora-sentiment`.
//!
//! Four costs are separated, because the decisions they inform are different:
//!
//! * **Cold start** — decoding one embedded blob into a lookup table, the cost
//!   this crate's data-shipping decision was made against. The reference
//!   `require`s ~7.5 MB of JSON at import time whether or not it is used; here
//!   1.2 MB of `key \0 polarity \0` is decoded lazily, once per process, and the
//!   benchmark says how much that is. Measured with the process-wide cache
//!   bypassed, since `Vocabulary::shared` would otherwise answer instantly after
//!   the first sample.
//! * **Construction** — building an analyzer. Without a stemmer this is a
//!   pointer copy; with one it re-stems the whole vocabulary, which the
//!   reference measured at 88 ms for English senticon and which is the single
//!   reason to build an analyzer once and keep it.
//! * **Scoring** — the hot loop, at several document sizes and against all three
//!   vocabulary families. Throughput is in tokens, since that is what the loop
//!   iterates.
//! * **Token shape** — lowercase ASCII (the allocation-free path), uppercase
//!   ASCII (one `to_ascii_lowercase` per token) and non-ASCII (the full Unicode
//!   `to_lowercase`, which is what correctness requires and what costs).

use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};

use verbora_sentiment::{SentimentAnalyzer, Stemmer, Vocabulary, VocabularyKind};
use verbora_stemmers::PorterStemmer;

/// A realistic English review, tokenized the way the reference's own spec does.
fn english_tokens(repeats: usize) -> Vec<String> {
    let text = "Sadly, stereotypes and caricatures of Native Americans are still \
                prevalent in our society and this film does not help. It is not a good \
                movie, but the acting is wonderful and the photography is beautiful. \
                I loved the ending, though the middle dragged terribly.";
    text.split_whitespace()
        .cycle()
        .take(text.split_whitespace().count() * repeats)
        .map(str::to_owned)
        .collect()
}

/// The same length of text with no uppercase and no accents: the fast path.
fn lowercase_tokens(n: usize) -> Vec<String> {
    [
        "the",
        "movie",
        "was",
        "not",
        "good",
        "but",
        "the",
        "acting",
        "was",
        "wonderful",
    ]
    .iter()
    .cycle()
    .take(n)
    .map(|s| (*s).to_owned())
    .collect()
}

/// Uppercase ASCII: every token allocates a lowercased copy.
fn uppercase_tokens(n: usize) -> Vec<String> {
    lowercase_tokens(n)
        .iter()
        .map(|s| s.to_uppercase())
        .collect()
}

/// Non-ASCII: the full-string Unicode lowercase on every token.
fn accented_tokens(n: usize) -> Vec<String> {
    [
        "película",
        "estupenda",
        "però",
        "não",
        "muito",
        "böse",
        "Straße",
        "ΑΣΤΕΙΟ",
        "café",
        "naïve",
    ]
    .iter()
    .cycle()
    .take(n)
    .map(|s| (*s).to_owned())
    .collect()
}

fn bench_cold_decode(c: &mut Criterion) {
    let mut group = c.benchmark_group("decode");
    for (kind, language) in [
        (VocabularyKind::Afinn, "English"),
        (VocabularyKind::Pattern, "German"),
        (VocabularyKind::Senticon, "English"),
    ] {
        let entries = Vocabulary::shared(kind, language)
            .expect("shipped pair")
            .len();
        group.throughput(Throughput::Elements(entries as u64));
        group.bench_function(BenchmarkId::new(kind.as_str(), language), |b| {
            // `Vocabulary::shared` is cached for the process, so re-measuring it
            // would time a pointer return. Rebuilding through `stemmed` with an
            // identity stemmer walks the same entries and does the same hashing,
            // which is the closest honest proxy for a cold decode.
            let base = Vocabulary::shared(kind, language).expect("shipped pair");
            b.iter(|| black_box(base.stemmed(&verbora_sentiment::NoStemmer).len()));
        });
    }
    group.finish();
}

fn bench_construction(c: &mut Criterion) {
    let mut group = c.benchmark_group("construct");
    group.sample_size(20);
    for (kind, language) in [
        (VocabularyKind::Afinn, "English"),
        (VocabularyKind::Senticon, "English"),
    ] {
        group.bench_function(BenchmarkId::new("no-stemmer", kind.as_str()), |b| {
            b.iter(|| {
                black_box(
                    SentimentAnalyzer::without_stemmer(language, kind.as_str())
                        .expect("shipped pair")
                        .vocabulary()
                        .len(),
                )
            });
        });
        group.bench_function(BenchmarkId::new("porter-stemmer", kind.as_str()), |b| {
            b.iter(|| {
                black_box(
                    SentimentAnalyzer::new(language, Some(PorterStemmer::new()), kind.as_str())
                        .expect("shipped pair")
                        .vocabulary()
                        .len(),
                )
            });
        });
    }
    group.finish();
}

fn bench_scoring(c: &mut Criterion) {
    let mut group = c.benchmark_group("get_sentiment");
    let afinn = SentimentAnalyzer::without_stemmer("English", "afinn").expect("English AFINN");
    let senticon =
        SentimentAnalyzer::without_stemmer("English", "senticon").expect("English senticon");
    let stemmed = SentimentAnalyzer::new("English", Some(PorterStemmer::new()), "afinn")
        .expect("English AFINN");

    for repeats in [1_usize, 8, 64] {
        let tokens = english_tokens(repeats);
        group.throughput(Throughput::Elements(tokens.len() as u64));
        group.bench_with_input(
            BenchmarkId::new("afinn", tokens.len()),
            &tokens,
            |b, tokens| b.iter(|| black_box(afinn.get_sentiment(tokens.iter()))),
        );
        group.bench_with_input(
            BenchmarkId::new("senticon", tokens.len()),
            &tokens,
            |b, tokens| b.iter(|| black_box(senticon.get_sentiment(tokens.iter()))),
        );
        // With a stemmer every miss costs a stem, so this is the worst case for
        // text the vocabulary does not cover.
        group.bench_with_input(
            BenchmarkId::new("afinn+porter", tokens.len()),
            &tokens,
            |b, tokens| b.iter(|| black_box(stemmed.get_sentiment(tokens.iter()))),
        );
    }
    group.finish();
}

fn bench_token_shape(c: &mut Criterion) {
    let mut group = c.benchmark_group("token_shape");
    let afinn = SentimentAnalyzer::without_stemmer("English", "afinn").expect("English AFINN");
    for (name, tokens) in [
        ("lowercase-ascii", lowercase_tokens(512)),
        ("uppercase-ascii", uppercase_tokens(512)),
        ("non-ascii", accented_tokens(512)),
    ] {
        group.throughput(Throughput::Elements(tokens.len() as u64));
        group.bench_with_input(BenchmarkId::from_parameter(name), &tokens, |b, tokens| {
            b.iter(|| black_box(afinn.get_sentiment(tokens.iter())));
        });
    }
    group.finish();
}

/// The lazy primitive against the convenience wrapper, to show that composing a
/// tokenizer costs nothing extra.
fn bench_contributions(c: &mut Criterion) {
    let mut group = c.benchmark_group("contributions");
    let afinn = SentimentAnalyzer::without_stemmer("English", "afinn").expect("English AFINN");
    let tokens = english_tokens(16);
    group.throughput(Throughput::Elements(tokens.len() as u64));
    group.bench_function("sum", |b| {
        b.iter(|| black_box(afinn.contributions(tokens.iter()).sum::<f64>()));
    });
    group.bench_function("first-nonzero", |b| {
        b.iter(|| black_box(afinn.contributions(tokens.iter()).position(|d| d != 0.0)));
    });
    group.finish();
}

/// The stemmer bridge in isolation: one virtual call plus the algorithm.
fn bench_stem_bridge(c: &mut Criterion) {
    let stemmer: Box<dyn Stemmer> = Box::new(PorterStemmer::new());
    // A group rather than a bare `bench_function("stem_bridge/porter")`:
    // Criterion flattens a slash in a bare id into an underscore, which would
    // stop the result collector from joining this row against its siblings.
    let mut group = c.benchmark_group("stem_bridge");
    group.bench_function("porter", |b| {
        b.iter(|| black_box(stemmer.stem("nationalization").len()));
    });
    group.finish();
}

/// Sequential vs `parallel`-feature batch scoring, at a few `(document
/// count, tokens per document)` combinations spanning the three token counts
/// (44/352/2816) the `get_sentiment` benchmark above already uses, so this
/// group is directly comparable to it: each `parallel/*` per-document cost
/// should roughly match `get_sentiment/afinn/<n>` divided by core count, once
/// the batch is big enough to amortise Rayon's fan-out.
///
/// Only compiled with `--features parallel`; see `src/parallel.rs` for what
/// this API is for.
#[cfg(feature = "parallel")]
fn bench_par_batch(c: &mut Criterion) {
    let afinn = SentimentAnalyzer::without_stemmer("English", "afinn").expect("English AFINN");

    let mut group = c.benchmark_group("par_batch");
    group.sample_size(30);

    // (documents in the batch, repeats per document — 44/352/2816 tokens).
    for (doc_count, repeats) in [
        (16_usize, 1_usize),
        (256, 1),
        (16, 8),
        (128, 8),
        (16, 64),
        (128, 64),
    ] {
        let doc = english_tokens(repeats);
        let docs: Vec<Vec<String>> = std::iter::repeat_n(doc, doc_count).collect();
        let id = format!("docs{doc_count}_tokens{}", 44 * repeats);
        group.throughput(Throughput::Elements((doc_count * 44 * repeats) as u64));

        group.bench_with_input(BenchmarkId::new("sequential", &id), &docs, |b, docs| {
            b.iter(|| {
                black_box(
                    docs.iter()
                        .map(|d| afinn.get_sentiment(d.iter()))
                        .collect::<Vec<f64>>(),
                )
            });
        });
        group.bench_with_input(BenchmarkId::new("parallel", &id), &docs, |b, docs| {
            b.iter(|| black_box(afinn.par_get_sentiment_batch(docs)));
        });
    }
    group.finish();
}

#[cfg(not(feature = "parallel"))]
criterion_group!(
    benches,
    bench_cold_decode,
    bench_construction,
    bench_scoring,
    bench_token_shape,
    bench_contributions,
    bench_stem_bridge
);

#[cfg(feature = "parallel")]
criterion_group!(
    benches,
    bench_cold_decode,
    bench_construction,
    bench_scoring,
    bench_token_shape,
    bench_contributions,
    bench_stem_bridge,
    bench_par_batch
);
criterion_main!(benches);
