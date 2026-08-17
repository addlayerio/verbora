//! Verbora vs. real, pinned third-party Rust competitors — TF-IDF.
//!
//! See `docs/COMPETITIVE_BENCHMARKS.md` §1.12 for the full research dossier;
//! summarized here:
//!
//! - **`tfidf` (afshinm) 0.3.0** — closest architectural match of any Rust
//!   candidate found (a stateful struct with `.add()`/`.idf()`/`.tf()`/
//!   `.tfidf()`, mirroring Verbora's own shape) but a genuinely different
//!   weighting formula: `idf = log10(N/df)` (unsmoothed) vs. Verbora's
//!   `1 + ln(N/(1+df))` (smoothed), and `tf = log10(count) + 1` vs. Verbora's
//!   raw count. Benchmarked for **build and query SPEED only** — the matrix is
//!   explicit that output values are not comparable, and this file never
//!   asserts they are.
//! - **`rust-tfidf` 1.1.1** — stateless: it has no ingestion/`add` step at
//!   all, only trait-based scoring over a caller-supplied, already-vectorized
//!   `Vec<(&str, usize)>` per document. It is therefore benchmarked for
//!   **query only**, never "build" — there is nothing to time there. Its
//!   crate's own `[lib] name` is `tfidf`, which collides with the `tfidf`
//!   (afshinm) crate above; `Cargo.toml` renames it to `rust_tfidf` on
//!   import (see that file's own comment — omitting the rename is a hard
//!   `E0464` compile error, not a style choice).
//! - **Tantivy** — excluded per the spec's own TANTIVY POLICY: BM25 scoring
//!   runs inside full query execution, not as a standalone queryable tf-idf
//!   table, so it would be doing categorically more (and different) work.
//!
//! Every document in every group below is a rotation of the same real ~163 kB
//! Wikipedia article (French Revolution) `crates/verbora-tfidf/benches/
//! tfidf.rs`'s own `document()`/`rotated_texts()` helpers use, read from the
//! same source file and rotated with the identical `words[(i*7) %
//! len..].join(" ")` formula — so this file's Verbora corpus and that
//! in-workspace bench's corpus are byte-identical without one importing the
//! other's harness. Every implementation within one Criterion group here
//! reads the exact same rotated document set.
//!
//! Sizes are `[4, 16, 64, 256, 1024]` documents, matching every other
//! competitive module's convention (`scripts/collect-results.py`'s `SIZES`).
//! Note this is a **different, larger and more numerous** document set than
//! the in-workspace bench's own `[1, 8, 64, 256]` — this file does not need
//! to match that bench's specific sizes, only its *derivation*, since the two
//! benches are never joined against each other (only Verbora-here vs.
//! Rust-competitors-here, and separately Verbora-in-workspace vs.
//! The reference).

use std::hint::black_box;
use std::path::Path;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};

use verbora_tfidf::{DocKey, DocumentInput, Terms, TfIdf};

// Capped at 256, not the `[4, 16, 64, 256, 1024]` every other competitive
// module uses (`scripts/collect-results.py`'s `SIZES`): every document here
// is a rotation of the *same* ~163 kB article ("few_large" per
// `crates/verbora-tfidf/benches/tfidf.rs`'s own naming), so a corpus of `n`
// documents is `O(n)` in ~163 kB units, not `O(n)` in a handful of bytes the
// way `distance.rs`'s string pairs are — a 1024-document corpus here is
// ~167 MB of text per `build` sample. Adapted down per the spec's own
// "DATASET SIZES" section ("para operaciones costosas adaptar tamaños
// razonablemente"); the in-workspace bench's own `idf_cold` group caps at
// the same 256 for the identical reason. `results/results.json` simply has
// no `1024` entry for this module — `scripts/collect-results.py` skips
// missing sizes rather than erroring.
const SIZES: [usize; 4] = [4, 16, 64, 256];

/// Byte-identical derivation to `crates/verbora-tfidf/benches/tfidf.rs`'s own
/// `document()`.
fn document() -> String {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("repo root is 3 levels up from rust-competitors/")
        .join("benches/data/corpus");
    let article = std::fs::read_to_string(root.join("Wikipedia_EN_FrenchRevolution.json"))
        .ok()
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
        .and_then(|v| {
            v.get("text")
                .and_then(|t| t.as_str())
                .map(ToOwned::to_owned)
        });
    article.unwrap_or_else(|| {
        "the quick brown fox jumps over the lazy dog while node and ruby argue ".repeat(2400)
    })
}

/// Byte-identical derivation to `crates/verbora-tfidf/benches/tfidf.rs`'s own
/// `rotated_texts()`.
fn rotated_texts(text: &str, n: usize) -> Vec<String> {
    let words: Vec<&str> = text.split_whitespace().collect();
    (0..n)
        .map(|i| {
            let start = (i * 7) % words.len().max(1);
            words[start..].join(" ")
        })
        .collect()
}

fn verbora_corpus(docs: &[String]) -> TfIdf {
    let mut t = TfIdf::new();
    #[expect(clippy::cast_precision_loss, reason = "benchmark corpora are tiny")]
    for (i, doc) in docs.iter().enumerate() {
        t.add_document(DocumentInput::Text(doc), DocKey::Num(i as f64), false)
            .expect("a fresh instance always has a documents array");
    }
    t
}

fn afshinm_corpus<'a>(docs: &'a [String]) -> tfidf::tfidf::TfIdf<'a> {
    let mut t = tfidf::tfidf::TfIdf::new();
    for doc in docs {
        t.add(doc);
    }
    t
}

/// A naive whitespace+lowercase vectorizer, for `rust-tfidf`'s
/// `ProcessedDocument` shape (`Vec<(String, usize)>` term-count pairs) —
/// query-only per the matrix, so this is deliberately never timed as
/// "build". Not Verbora's real tokenizer (no punctuation handling, no
/// stop-word filtering): the source text is plain prose from a real article,
/// so this is a reasonable, documented approximation for a crate that has no
/// tokenizer of its own to begin with.
fn vectorize(doc: &str) -> Vec<(String, usize)> {
    let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for word in doc.split_whitespace() {
        *counts.entry(word.to_lowercase()).or_insert(0) += 1;
    }
    counts.into_iter().collect()
}

fn bench_build(c: &mut Criterion) {
    let text = document();
    let mut g = c.benchmark_group("build");
    for n in SIZES {
        let docs = rotated_texts(&text, n);
        let total_bytes: u64 = docs.iter().map(|d| d.len() as u64).sum();
        g.throughput(Throughput::Bytes(total_bytes));

        g.bench_with_input(BenchmarkId::new("verbora", n), &docs, |b, docs| {
            b.iter(|| black_box(verbora_corpus(black_box(docs))));
        });
        g.bench_with_input(BenchmarkId::new("afshinm", n), &docs, |b, docs| {
            b.iter(|| black_box(afshinm_corpus(black_box(docs))));
        });
        // No `rust_tfidf` row: it has no ingestion step (matrix confirms) —
        // nothing to benchmark here.
    }
    g.finish();
}

fn bench_idf(c: &mut Criterion) {
    let text = document();
    let mut g = c.benchmark_group("idf");
    for n in SIZES {
        let docs = rotated_texts(&text, n);
        let mut verbora = verbora_corpus(&docs);
        let afshinm = afshinm_corpus(&docs);
        let vectors: Vec<Vec<(String, usize)>> = docs.iter().map(|d| vectorize(d)).collect();

        g.bench_with_input(BenchmarkId::new("verbora", n), &n, |b, _| {
            b.iter(|| black_box(verbora.idf_value(black_box("the"), true).unwrap()));
        });
        g.bench_with_input(BenchmarkId::new("afshinm", n), &n, |b, _| {
            b.iter(|| black_box(afshinm.idf(black_box(&tfidf::tfidf::Term("the")))));
        });
        g.bench_with_input(BenchmarkId::new("rust_tfidf", n), &n, |b, _| {
            use rust_tfidf::{Idf, idf::InverseFrequencyIdf};
            let probe = "the".to_owned();
            b.iter(|| {
                black_box(InverseFrequencyIdf::idf(
                    black_box(&probe),
                    black_box(vectors.iter()),
                ))
            });
        });
    }
    g.finish();
}

fn bench_tfidf(c: &mut Criterion) {
    let text = document();
    let mut g = c.benchmark_group("tfidf");
    for n in SIZES {
        let docs = rotated_texts(&text, n);
        let mut verbora = verbora_corpus(&docs);
        let afshinm = afshinm_corpus(&docs);
        let vectors: Vec<Vec<(String, usize)>> = docs.iter().map(|d| vectorize(d)).collect();

        g.bench_with_input(BenchmarkId::new("verbora", n), &n, |b, _| {
            b.iter(|| black_box(verbora.tfidf(black_box(Terms::Text("the")), 0).unwrap()));
        });
        g.bench_with_input(BenchmarkId::new("afshinm", n), &n, |b, _| {
            b.iter(|| black_box(afshinm.tfidf(black_box(&tfidf::tfidf::Term("the")), 0)));
        });
        g.bench_with_input(BenchmarkId::new("rust_tfidf", n), &n, |b, _| {
            use rust_tfidf::{TfIdf as _, TfIdfDefault};
            let probe = "the".to_owned();
            b.iter(|| {
                black_box(TfIdfDefault::tfidf(
                    black_box(&probe),
                    black_box(&vectors[0]),
                    black_box(vectors.iter()),
                ))
            });
        });
    }
    g.finish();
}

criterion_group!(benches, bench_build, bench_idf, bench_tfidf);
criterion_main!(benches);

// CORRECTNESS BEFORE PERFORMANCE: see `../tests/tfidf_correctness.rs`, not a
// `#[cfg(test)] mod` in this file. A Criterion `[[bench]]` target compiles
// with `harness = false`, which replaces the standard libtest runner with
// `criterion_main!`'s own `fn main` — an in-file `#[test]` here would be
// dead code that `cargo test` never invokes (there is no libtest harness left
// to collect it). `tests/*.rs` files are their own real integration-test
// binaries with the standard harness intact, which is where every other
// competitive-module correctness check in this crate already lives (see
// `tests/trie_correctness.rs`, `tests/tokenizers_correctness.rs`, ...).
