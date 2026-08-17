// `criterion_group!` expands to an undocumented function, and a benchmark
// harness is not public API, so the workspace-wide `missing_docs` lint is noise
// here.
#![allow(missing_docs)]

//! Benchmarks for `verbora-tfidf`.
//!
//! Four questions, one group each. Figures below are from one machine
//! (`--measurement-time 1.5`, 167 kB documents) and are recorded because two of
//! them changed a design decision.
//!
//! 1. **What does a document cost to ingest, and is the term interner worth
//!    it?** The design brief asked for the interning scheme to be *measured*
//!    rather than assumed, so `build` pits it against a `HashMap<String, f64>`
//!    per document. For a **single** document the two are indistinguishable
//!    (2.78 ms either way) — there is nothing to share yet. Across a corpus of
//!    eight the interner wins by ~17% (18.1 ms vs 21.8 ms), because a term is
//!    allocated once for the whole corpus instead of once per document. The
//!    larger payoff is not in this group at all: `u32` term ids are what make
//!    the document-frequency table in (2) possible.
//! 2. **What does a cache miss cost?** the reference recomputes `idf` by scanning
//!    every document. This port keeps an incremental document-frequency table,
//!    and `idf_cold/built` is flat at ~18 ns from 1 document to 256.
//!    `idf_cold/deserialized` is the fallback for corpora loaded from JSON,
//!    whose values a count table cannot represent: 24 ns at 1 document,
//!    2.6 µs at 256 — linear, like the reference. It was **3.4 ms** at 256
//!    before `RawDocument` grew a lookup index, which is how that O(terms)
//!    scan was found.
//! 3. **What do the queries cost?** `query` covers `tfidf`, `tfidfs` and
//!    `list_terms`, the last of which is quadratic-ish by construction: the
//!    reference asks for `idf` and then for `tfidf([term])`, which asks again.
//! 4. **What does exactness cost?** `math_log` prices the fdlibm port against
//!    the platform `f64::ln` it had to replace: ~7.6 µs vs ~5.1 µs per 2,048
//!    values, i.e. roughly 1.5× for a bit-exact logarithm. That is the whole
//!    price of the parity fix.
//! 5. **`parallel` feature only: does `par_add_documents_batch` beat the
//!    sequential loop, and at what size?** `parallel_batch` compares `N`
//!    sequential `add_document(Text, key, false)` calls against one
//!    `par_add_documents_batch` call over the same `N` `(text, key)` pairs, at
//!    two corpus shapes: `few_large` (8/64/256 documents near the full 167 kB
//!    article) and `many_small` (128/1,024/8,192 documents of ~1.2 kB each).
//!    On one 32-core machine the win is real but modest — ~5-8% faster for
//!    `few_large`, and for `many_small` it ranges from ~7% *slower* at 128
//!    documents (Rayon's fork-join cost not yet amortized) to ~13% faster at
//!    8,192. See `TfIdf::par_add_documents_batch`'s doc comment in
//!    `src/tfidf.rs` for the full table and why only the lowercasing/tokenizing
//!    step is parallelized — the sequential interning/counting replay this
//!    method deliberately leaves untouched is not free, which is exactly why
//!    the win is nowhere near linear in core count.

use std::hint::black_box;
use std::path::Path;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use verbora_tfidf::{DocKey, DocumentInput, JsonValue, Terms, TfIdf};

/// A real document at a realistic scale: the 167 kB English Wikipedia article
/// on the French Revolution that ships with the reference tree.
///
/// Size matters more than it looks. The first version of this file used
/// `spec/test_data/index.document1.txt`, which is **28 bytes** — every "build"
/// measurement was allocator noise, and the interning comparison it was meant
/// to settle came out backwards. A synthetic fallback keeps the benchmark
/// runnable outside a full checkout, at the same order of magnitude.
fn document() -> String {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crate is two levels below the workspace root")
        .join("benches/data/corpus");
    let article = std::fs::read_to_string(root.join("Wikipedia_EN_FrenchRevolution.json"))
        .ok()
        .and_then(|raw| serde_json::from_str::<JsonValue>(&raw).ok())
        .and_then(|v| match v.own("text") {
            Some(JsonValue::Str(s)) => Some(s.clone()),
            _ => None,
        });
    article.unwrap_or_else(|| {
        "the quick brown fox jumps over the lazy dog while node and ruby argue ".repeat(2400)
    })
}

/// `n` rotations of `text`'s words, exactly the slices [`corpus`] feeds to
/// `add_document` — factored out so `bench_parallel_batch` can feed the same
/// text to both the sequential loop and `par_add_documents_batch`.
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
/// words (wrapping around) — a fixed, realistic per-document size (unlike
/// [`rotated_texts`], whose documents shrink towards the end of `text`
/// regardless of `n`), so total corpus size scales linearly with `n` the way
/// a real "many documents" ingestion workload does.
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

/// A corpus of `n` documents, each a rotation of the source text so that terms
/// genuinely overlap between documents rather than being disjoint.
fn corpus(text: &str, n: usize) -> TfIdf {
    let mut t = TfIdf::new();
    for (i, slice) in rotated_texts(text, n).into_iter().enumerate() {
        #[expect(clippy::cast_precision_loss, reason = "benchmark corpora are tiny")]
        t.add_document(DocumentInput::Text(&slice), DocKey::Num(i as f64), false)
            .expect("a fresh instance always has a documents array");
    }
    t
}

/// The obvious representation: one `HashMap<String, f64>` per document.
///
/// This is the *informed* version of that baseline, not a straw man — it uses
/// `get_mut` first so a repeated term costs no allocation, where the reflexive
/// `entry(token.to_owned())` would allocate on every occurrence. It is here to
/// price the interning scheme honestly, and it is not an alternative
/// implementation: it reproduces none of `__proto__`, `__key` or the
/// `Object.prototype` reset.
fn naive_build(text: &str) -> std::collections::HashMap<String, f64> {
    use verbora_tokenizers::WordTokenizer;
    let lowered = text.to_lowercase();
    let mut map: std::collections::HashMap<String, f64> = std::collections::HashMap::new();
    if let Some(tokens) = WordTokenizer::new().tokens(&lowered) {
        for token in tokens {
            if verbora_core::stopwords::is_default_stopword(token) {
                continue;
            }
            match map.get_mut(token) {
                Some(count) => *count += 1.0,
                None => {
                    map.insert(token.to_owned(), 1.0);
                }
            }
        }
    }
    map
}

fn bench_build(c: &mut Criterion) {
    let text = document();
    let tokens: Vec<String> = text
        .split_whitespace()
        .map(str::to_ascii_lowercase)
        .collect();
    let token_refs: Vec<&str> = tokens.iter().map(String::as_str).collect();

    let mut group = c.benchmark_group("build");
    group.throughput(Throughput::Bytes(text.len() as u64));

    group.bench_function("add_document/text", |b| {
        b.iter(|| {
            let mut t = TfIdf::new();
            t.add_document(
                DocumentInput::Text(black_box(&text)),
                DocKey::Undefined,
                false,
            )
            .unwrap();
            black_box(t)
        });
    });

    // The token path skips lowercasing and stop-word filtering entirely, which
    // is a behavioural difference before it is a performance one.
    group.bench_function("add_document/tokens", |b| {
        b.iter(|| {
            let mut t = TfIdf::new();
            t.add_document(
                DocumentInput::Tokens(black_box(&token_refs)),
                DocKey::Undefined,
                false,
            )
            .unwrap();
            black_box(t)
        });
    });

    group.bench_function("baseline/hashmap_of_owned_strings", |b| {
        b.iter(|| black_box(naive_build(black_box(&text))));
    });

    // Eight documents into ONE corpus versus eight independent naive maps.
    // This is where interning pays: after the first document the interner is
    // warm, so a repeated term costs a hash lookup instead of a `String`.
    // Measuring a single `add_document` into a pre-built corpus would not show
    // it, because the setup clone would dominate the sample.
    group.bench_function("corpus_of_8/interned", |b| {
        b.iter(|| {
            let mut t = TfIdf::new();
            for i in 0..8 {
                t.add_document(
                    DocumentInput::Text(black_box(&text)),
                    DocKey::Num(f64::from(i)),
                    false,
                )
                .unwrap();
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

/// What a cache miss costs, isolated from any corpus copying.
///
/// `idf_value(term, true)` is the reference's `force` flag: it skips the cache
/// probe and recomputes, which is exactly the miss path and needs no fresh
/// instance per sample. An earlier version of this benchmark cloned the corpus
/// in `iter_batched` setup and measured the clone instead — the "built" numbers
/// came out *slower* than the scanning path, which is how the mistake surfaced.
fn bench_idf(c: &mut Criterion) {
    let text = document();
    let mut group = c.benchmark_group("idf_cold");

    for n in [1usize, 8, 64, 256] {
        let mut built = corpus(&text, n);
        group.bench_with_input(BenchmarkId::new("built", n), &n, |b, _| {
            b.iter(|| black_box(built.idf_value(black_box("the"), true).unwrap()));
        });

        // The same corpus, deserialized: every document is an opaque value, so
        // the incremental table cannot be used and the reference's full scan is
        // what runs. The gap between the two lines is the optimization.
        let mut exotic = TfIdf::from_json(&built.to_json()).expect("round-trips");
        group.bench_with_input(BenchmarkId::new("deserialized", n), &n, |b, _| {
            b.iter(|| black_box(exotic.idf_value(black_box("the"), true).unwrap()));
        });
    }
    group.finish();
}

fn bench_query(c: &mut Criterion) {
    let text = document();
    let corpus64 = corpus(&text, 64);
    let mut group = c.benchmark_group("query");

    group.bench_function("tfidf/warm_cache", |b| {
        let mut t = corpus64.clone();
        // Prime the cache so this measures the query, not the first miss.
        t.tfidf(Terms::Text("the quick brown fox"), 0).unwrap();
        b.iter(|| {
            black_box(
                t.tfidf(black_box(Terms::Text("the quick brown fox")), 0)
                    .unwrap(),
            )
        });
    });

    group.bench_function("tfidf/cold_cache", |b| {
        b.iter_batched(
            || corpus64.clone(),
            |mut t| {
                black_box(
                    t.tfidf(black_box(Terms::Text("the quick brown fox")), 0)
                        .unwrap(),
                )
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.bench_function("tfidfs/64_documents", |b| {
        b.iter_batched(
            || corpus64.clone(),
            |mut t| black_box(t.tfidfs(black_box(Terms::Text("node"))).unwrap()),
            criterion::BatchSize::SmallInput,
        );
    });

    group.bench_function("list_terms", |b| {
        b.iter_batched(
            || corpus64.clone(),
            |mut t| black_box(t.list_terms(black_box(0)).unwrap()),
            criterion::BatchSize::SmallInput,
        );
    });

    group.bench_function("to_json", |b| {
        b.iter(|| black_box(black_box(&corpus64).to_json()));
    });

    let json = corpus64.to_json();
    group.throughput(Throughput::Bytes(json.len() as u64));
    group.bench_function("from_json", |b| {
        b.iter(|| black_box(TfIdf::from_json(black_box(&json)).unwrap()));
    });

    group.finish();
}

fn bench_math_log(c: &mut Criterion) {
    // The ratios an idf actually feeds to the logarithm.
    let inputs: Vec<f64> = (1..=256)
        .flat_map(|n| (1..=8).map(move |d| f64::from(n) / f64::from(d)))
        .collect();

    let mut group = c.benchmark_group("math_log");
    group.throughput(Throughput::Elements(inputs.len() as u64));

    group.bench_function("fdlibm_port", |b| {
        b.iter(|| {
            let mut acc = 0.0;
            for x in black_box(&inputs) {
                acc += verbora_tfidf::math_log(*x);
            }
            black_box(acc)
        });
    });

    // What the crate would have used if the fixtures had not caught the
    // one-ULP disagreement. Kept so the cost of exactness stays visible.
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

fn bench_documents(c: &mut Criterion) {
    let mut group = c.benchmark_group("documents");

    // A raw document is stored verbatim, so ingestion is a clone rather than a
    // tokenize — but it makes every later idf take the scanning path.
    let raw = JsonValue::Obj(
        (0..64)
            .map(|i| (format!("term{i}"), JsonValue::Num(f64::from(i))))
            .collect(),
    );
    group.bench_function("add_document/raw", |b| {
        b.iter(|| {
            let mut t = TfIdf::new();
            t.add_document(
                DocumentInput::Raw(black_box(raw.clone())),
                DocKey::Undefined,
                false,
            )
            .unwrap();
            black_box(t)
        });
    });

    let text = document();
    let corpus64 = corpus(&text, 64);
    group.bench_function("remove_document", |b| {
        b.iter_batched(
            || corpus64.clone(),
            |mut t| black_box(t.remove_document(black_box(&DocKey::Num(32.0))).unwrap()),
            criterion::BatchSize::SmallInput,
        );
    });

    group.finish();
}

/// One `(name, docs)` benchmark case for [`bench_parallel_batch`], comparing
/// `N` sequential `add_document(Text, key, false)` calls against one
/// `par_add_documents_batch` call over the same `N` `(text, key)` pairs.
#[cfg(feature = "parallel")]
fn run_parallel_batch_case(
    group: &mut criterion::BenchmarkGroup<'_, criterion::measurement::WallTime>,
    name: &str,
    docs: &[(&str, DocKey)],
) {
    let total_bytes: u64 = docs.iter().map(|(t, _)| t.len() as u64).sum();
    group.throughput(Throughput::Bytes(total_bytes));

    group.bench_with_input(BenchmarkId::new("sequential", name), &docs, |b, docs| {
        b.iter(|| {
            let mut t = TfIdf::new();
            for (text, key) in *docs {
                t.add_document(DocumentInput::Text(black_box(text)), key.clone(), false)
                    .unwrap();
            }
            black_box(t)
        });
    });

    group.bench_with_input(
        BenchmarkId::new("par_add_documents_batch", name),
        &docs,
        |b, docs| {
            b.iter(|| {
                let mut t = TfIdf::new();
                t.par_add_documents_batch(black_box(docs)).unwrap();
                black_box(t)
            });
        },
    );
}

/// Sequential vs. `par_add_documents_batch`, at two different corpus shapes.
///
/// `few_large/N` uses [`rotated_texts`] — `N` = 8, 64, 256 documents each
/// close to the *full* 167 kB source text (the same documents `corpus` and
/// `bench_idf`'s `idf_cold` group use). `many_small/N` uses [`chunked_texts`]
/// at a fixed 200 words (roughly 1.2 kB) per document — closer to "one
/// review, one ticket, one paragraph" — at `N` = 128, 1024, 8192, so total
/// corpus size scales with `N` the way a real bulk-ingestion workload does,
/// rather than staying pinned near one document's size regardless of `N`.
#[cfg(feature = "parallel")]
fn bench_parallel_batch(c: &mut Criterion) {
    let text = document();
    let mut group = c.benchmark_group("parallel_batch");

    for n in [8usize, 64, 256] {
        let texts = rotated_texts(&text, n);
        #[expect(clippy::cast_precision_loss, reason = "benchmark corpora are tiny")]
        let docs: Vec<(&str, DocKey)> = texts
            .iter()
            .enumerate()
            .map(|(i, t)| (t.as_str(), DocKey::Num(i as f64)))
            .collect();
        run_parallel_batch_case(&mut group, &format!("few_large/{n}"), &docs);
    }

    for n in [128usize, 1024, 8192] {
        let texts = chunked_texts(&text, 200, n);
        #[expect(clippy::cast_precision_loss, reason = "benchmark corpora are tiny")]
        let docs: Vec<(&str, DocKey)> = texts
            .iter()
            .enumerate()
            .map(|(i, t)| (t.as_str(), DocKey::Num(i as f64)))
            .collect();
        run_parallel_batch_case(&mut group, &format!("many_small/{n}"), &docs);
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_build,
    bench_idf,
    bench_query,
    bench_math_log,
    bench_documents
);

#[cfg(feature = "parallel")]
criterion_group!(parallel_benches, bench_parallel_batch);

#[cfg(feature = "parallel")]
criterion_main!(benches, parallel_benches);
#[cfg(not(feature = "parallel"))]
criterion_main!(benches);
