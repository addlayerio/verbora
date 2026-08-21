// `criterion_group!` expands to an undocumented function, and a benchmark
// harness is not public API, so the workspace-wide `missing_docs` lint is noise
// here.
#![allow(missing_docs)]

//! Benchmarks for `verbora-tfidf`.
//!
//! **Every figure this crate ever published was measured against the previous,
//! parity-shaped implementation, and that implementation no longer exists.**
//! The ingest pipeline changed (segment-then-fold rather than fold-then-
//! segment, integer counts, no per-token stop-word test by default), the idf
//! cache was deleted in favour of an incremental document-frequency table, and
//! the query path resolves a query once rather than re-tokenizing per document.
//! Nothing below carries a number, because carrying a stale one would be worse
//! than carrying none.
//!
//! Five questions, one group each:
//!
//! 1. **`build`** — what does ingesting a document cost, and does interning
//!    terms across a corpus beat the obvious `HashMap<String, u32>` per
//!    document? The baseline is deliberately the *informed* version of that
//!    map, so the comparison prices the representation rather than a straw man.
//! 2. **`idf`** — what does one inverse-document-frequency lookup cost? It is
//!    an array load behind a term-table probe, so the interesting question is
//!    whether it stays flat as the corpus grows.
//! 3. **`query`** — `tfidf`, `tfidfs`, `rank` and `list_terms`. `tfidf`
//!    resolves the query once per call and `tfidfs` once per corpus, which is
//!    the whole reason both exist; this group is where that shows up.
//! 4. **`persistence`** — `to_json` and `from_json`, which are the two
//!    operations whose cost scales with the vocabulary rather than the text.
//! 5. **`natural_log`** — what a specified, platform-independent logarithm
//!    costs against `f64::ln`. The crate pays this on every idf, so the price
//!    of determinism should be visible rather than assumed.
//!
//! With the `parallel` feature, a sixth group compares `par_add_documents`
//! against the sequential loop at two corpus shapes. Its crossover point is
//! **UNMEASURED**.

use std::hint::black_box;
use std::path::Path;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use verbora_tfidf::{TfIdf, natural_log};

/// A real document at a realistic scale: the ~167 kB English Wikipedia article
/// on the French Revolution that ships with the benchmark corpus.
///
/// Size matters more than it looks. An early version of this file used a 28-byte
/// fixture, and every "build" measurement was allocator noise. A synthetic
/// fallback keeps the benchmark runnable outside a full checkout, at the same
/// order of magnitude.
fn document() -> String {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crate is two levels below the workspace root")
        .join("benches/data/corpus");
    let article = std::fs::read_to_string(root.join("Wikipedia_EN_FrenchRevolution.json"))
        .ok()
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
        .and_then(|value| value.get("text")?.as_str().map(str::to_owned));
    article.unwrap_or_else(|| {
        "the quick brown fox jumps over the lazy dog while node and ruby argue ".repeat(2400)
    })
}

/// `n` rotations of `text`'s words, so terms genuinely overlap between
/// documents rather than being disjoint.
fn rotated_texts(text: &str, n: usize) -> Vec<String> {
    let words: Vec<&str> = text.split_whitespace().collect();
    (0..n)
        .map(|i| {
            let start = (i * 7) % words.len().max(1);
            words[start..].join(" ")
        })
        .collect()
}

/// `n` documents of `words_per_doc` words each, a rolling window over `text`'s
/// words — a fixed, realistic per-document size, so total corpus size scales
/// linearly with `n` the way a bulk-ingestion workload does.
#[cfg(feature = "parallel")]
fn chunked_texts(text: &str, words_per_doc: usize, n: usize) -> Vec<String> {
    let words: Vec<&str> = text.split_whitespace().collect();
    let len = words.len().max(1);
    (0..n)
        .map(|i| {
            let start = (i * words_per_doc) % len;
            words
                .iter()
                .cycle()
                .skip(start)
                .take(words_per_doc)
                .copied()
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect()
}

fn corpus(text: &str, n: usize) -> TfIdf {
    let mut c = TfIdf::new();
    c.add_documents(&rotated_texts(text, n));
    c
}

/// The obvious representation: one `HashMap<String, u32>` per document.
///
/// This is the *informed* version of that baseline, not a straw man — it uses
/// `get_mut` first, so a repeated term costs no allocation where the reflexive
/// `entry(token.to_owned())` would allocate on every occurrence. It runs the
/// same analyzer steps the crate does, so the comparison prices the
/// representation and nothing else.
fn naive_build(text: &str) -> std::collections::HashMap<String, u32> {
    use verbora_tokenizers::{BorrowingTokenizer, WordTokenizer};
    let mut map: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
    for token in WordTokenizer.tokens(text) {
        let folded = token.to_lowercase();
        match map.get_mut(&folded) {
            Some(count) => *count += 1,
            None => {
                map.insert(folded, 1);
            }
        }
    }
    map
}

fn bench_build(c: &mut Criterion) {
    let text = document();
    let terms: Vec<String> = verbora_tfidf::Analyzer::new().terms(&text);

    let mut group = c.benchmark_group("build");
    group.throughput(Throughput::Bytes(text.len() as u64));

    group.bench_function("add_document", |b| {
        b.iter(|| {
            let mut t = TfIdf::new();
            t.add_document(black_box(&text));
            black_box(t)
        });
    });

    // The pre-analyzed path: no tokenizing and no folding, so the difference
    // from `add_document` is the analyzer's own cost.
    group.bench_function("add_terms", |b| {
        b.iter(|| {
            let mut t = TfIdf::new();
            t.add_terms(black_box(&terms));
            black_box(t)
        });
    });

    group.bench_function("baseline/hashmap_of_owned_strings", |b| {
        b.iter(|| black_box(naive_build(black_box(&text))));
    });

    // Eight documents into ONE corpus versus eight independent naive maps.
    // This is where interning pays: after the first document the term table is
    // warm, so a repeated term costs a probe instead of a `String`.
    group.bench_function("corpus_of_8/interned", |b| {
        b.iter(|| {
            let mut t = TfIdf::new();
            for _ in 0..8 {
                t.add_document(black_box(&text));
            }
            black_box(t)
        });
    });

    group.bench_function("corpus_of_8/baseline", |b| {
        b.iter(|| {
            let maps: Vec<_> = (0..8).map(|_| naive_build(black_box(&text))).collect();
            black_box(maps)
        });
    });

    group.finish();
}

/// Does an idf lookup stay flat as the corpus grows?
///
/// It is a term-table probe plus one array load, with no cache to warm and
/// nothing to invalidate, so it should — but "should" is why this group exists.
fn bench_idf(c: &mut Criterion) {
    let text = document();
    let mut group = c.benchmark_group("idf");

    for n in [1usize, 8, 64, 256] {
        let built = corpus(&text, n);
        group.bench_with_input(BenchmarkId::new("present", n), &n, |b, _| {
            b.iter(|| black_box(built.idf(black_box("the"))));
        });
        group.bench_with_input(BenchmarkId::new("absent", n), &n, |b, _| {
            b.iter(|| black_box(built.idf(black_box("a-term-in-no-document"))));
        });
    }
    group.finish();
}

fn bench_query(c: &mut Criterion) {
    let text = document();
    let corpus64 = corpus(&text, 64);
    let terms = corpus64.analyzer().terms("the quick brown fox");
    let mut group = c.benchmark_group("query");

    group.bench_function("tfidf/one_document", |b| {
        b.iter(|| black_box(corpus64.tfidf(black_box("the quick brown fox"), 0)));
    });

    // The same work with the analyzer already run, so the difference is the
    // cost of analyzing a four-word query.
    group.bench_function("tfidf_terms/one_document", |b| {
        b.iter(|| black_box(corpus64.tfidf_terms(black_box(&terms), 0)));
    });

    // Resolves the query once for all 64 documents, which is the reason it
    // exists as a separate call rather than a loop over `tfidf`.
    group.bench_function("tfidfs/64_documents", |b| {
        b.iter(|| black_box(corpus64.tfidfs(black_box("the quick brown fox"))));
    });

    group.bench_function("tfidf_in_a_loop/64_documents", |b| {
        b.iter(|| {
            let scores: Vec<f64> = (0..64)
                .filter_map(|d| corpus64.tfidf(black_box("the quick brown fox"), d))
                .collect();
            black_box(scores)
        });
    });

    group.bench_function("rank/64_documents", |b| {
        b.iter(|| black_box(corpus64.rank(black_box("the quick brown fox"))));
    });

    group.bench_function("list_terms", |b| {
        b.iter(|| black_box(corpus64.list_terms(black_box(0))));
    });

    group.finish();
}

fn bench_persistence(c: &mut Criterion) {
    let text = document();
    let corpus64 = corpus(&text, 64);
    let json = corpus64.to_json().expect("the default tokenizer");

    let mut group = c.benchmark_group("persistence");
    group.throughput(Throughput::Bytes(json.len() as u64));

    group.bench_function("to_json", |b| {
        b.iter(|| black_box(black_box(&corpus64).to_json()));
    });

    group.bench_function("from_json", |b| {
        b.iter(|| black_box(TfIdf::from_json(black_box(&json))));
    });

    group.finish();
}

/// What a specified logarithm costs against the platform's.
///
/// `f64::ln` is not a drop-in alternative — it is a different function on every
/// platform, which is the whole reason `natural_log` exists — so this group
/// prices determinism rather than offering a choice.
fn bench_natural_log(c: &mut Criterion) {
    // The ratios an idf actually feeds to the logarithm: n / (1 + df).
    let inputs: Vec<f64> = (1..=256)
        .flat_map(|n| (0..8).map(move |df| f64::from(n) / (1.0 + f64::from(df))))
        .collect();

    let mut group = c.benchmark_group("natural_log");
    group.throughput(Throughput::Elements(inputs.len() as u64));

    group.bench_function("specified", |b| {
        b.iter(|| {
            let mut acc = 0.0;
            for x in black_box(&inputs) {
                acc += natural_log(*x);
            }
            black_box(acc)
        });
    });

    group.bench_function("platform_ln", |b| {
        b.iter(|| {
            let mut acc = 0.0;
            for x in black_box(&inputs) {
                acc += x.ln();
            }
            black_box(acc)
        });
    });

    group.finish();
}

/// One `(name, texts)` case for [`bench_parallel_batch`].
#[cfg(feature = "parallel")]
fn run_parallel_batch_case(
    group: &mut criterion::BenchmarkGroup<'_, criterion::measurement::WallTime>,
    name: &str,
    texts: &[String],
) {
    let total_bytes: u64 = texts.iter().map(|t| t.len() as u64).sum();
    group.throughput(Throughput::Bytes(total_bytes));

    group.bench_with_input(BenchmarkId::new("sequential", name), &texts, |b, texts| {
        b.iter(|| {
            let mut t = TfIdf::new();
            t.add_documents(black_box(texts));
            black_box(t)
        });
    });

    group.bench_with_input(
        BenchmarkId::new("par_add_documents", name),
        &texts,
        |b, texts| {
            b.iter(|| {
                let mut t = TfIdf::new();
                t.par_add_documents(black_box(texts));
                black_box(t)
            });
        },
    );
}

/// Sequential versus parallel ingestion, at two corpus shapes.
///
/// `few_large/N` is `N` documents each close to the full source article;
/// `many_small/N` is `N` documents of a fixed ~200 words — closer to "one
/// review, one ticket". Both matter, because the fork-join cost is amortized
/// very differently by the two.
#[cfg(feature = "parallel")]
fn bench_parallel_batch(c: &mut Criterion) {
    let text = document();
    let mut group = c.benchmark_group("parallel_batch");

    for n in [8usize, 64, 256] {
        let texts = rotated_texts(&text, n);
        run_parallel_batch_case(&mut group, &format!("few_large/{n}"), &texts);
    }

    for n in [128usize, 1024, 8192] {
        let texts = chunked_texts(&text, 200, n);
        run_parallel_batch_case(&mut group, &format!("many_small/{n}"), &texts);
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_build,
    bench_idf,
    bench_query,
    bench_persistence,
    bench_natural_log
);

#[cfg(feature = "parallel")]
criterion_group!(parallel_benches, bench_parallel_batch);

#[cfg(feature = "parallel")]
criterion_main!(benches, parallel_benches);
#[cfg(not(feature = "parallel"))]
criterion_main!(benches);
