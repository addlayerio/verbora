//! `CORRECTNESS BEFORE PERFORMANCE` for `benches/tfidf.rs`.
//!
//! Verifies the harness code in `benches/tfidf.rs` — not `verbora-tfidf`
//! itself, which already has its own parity suite against the reference (see
//! `crates/verbora-tfidf/tests/parity.rs`) — before any timing number from
//! that file is trusted.
//!
//! `afshinm`'s `tfidf` and `rust-tfidf` are deliberately **not** checked
//! against Verbora's output values here: `docs/COMPETITIVE_BENCHMARKS.md`
//! §1.12 documents both as genuinely different weighting formulas (unsmoothed
//! `log10(N/df)` and augmented/normalized-TF variants vs. Verbora's
//! `1 + ln(N/(1+df))`), so there is nothing correct for them to agree on —
//! `benches/tfidf.rs`'s own module doc comment says the same. That exclusion
//! stands unchanged. What IS checked across implementations here is strictly
//! **ordinal** agreement — that all three formulas rank a rarer term's idf
//! above a commoner term's, and (at equal document frequency) a
//! more-frequent-in-document term's tfidf above a less-frequent one's. Both
//! orderings are mathematical consequences every one of the three formulas
//! shares by construction (each idf variant is strictly decreasing in `df`;
//! each tf variant is strictly increasing in in-document count), so ordinal
//! disagreement — unlike value disagreement — genuinely would mean one of the
//! implementations, or this crate's own adapter code, is broken. No test
//! below ever compares a value from one implementation to a value from
//! another.
//!
//! Everything else this file checks is harness verification: that this
//! crate's OWN corpus-construction helpers (duplicated from
//! `benches/tfidf.rs`, since a `tests/*.rs` binary cannot import private
//! items from a `[[bench]]` target) produce the values
//! `crates/verbora-tfidf/src/lib.rs`'s own doc-comment example promises, and
//! that the rotation/chunking formulas genuinely nest, wrap, and size
//! themselves as every benchmark group implicitly assumes.

use verbora_tfidf::{DocKey, DocumentInput, Terms, TfIdf};

/// Duplicated from `benches/tfidf.rs` — kept in exact lock-step; see that
/// file for why (byte-identical to `crates/verbora-tfidf/benches/tfidf.rs`'s
/// own `rotated_texts()`).
fn rotated_texts(text: &str, n: usize) -> Vec<String> {
    let words: Vec<&str> = text.split_whitespace().collect();
    (0..n)
        .map(|i| {
            let start = (i * 7) % words.len().max(1);
            words[start..].join(" ")
        })
        .collect()
}

/// Duplicated from `benches/tfidf.rs` — kept in exact lock-step, same
/// discipline as [`rotated_texts`] above (and byte-identical to
/// `crates/verbora-tfidf/benches/tfidf.rs`'s own `chunked_texts()`).
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

/// Duplicated from `benches/tfidf.rs` — kept in exact lock-step, same
/// discipline as [`rotated_texts`] above (the naive whitespace+lowercase
/// vectorizer the `rust_tfidf` query rows feed on).
fn vectorize(doc: &str) -> Vec<(String, usize)> {
    let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for word in doc.split_whitespace() {
        *counts.entry(word.to_lowercase()).or_insert(0) += 1;
    }
    counts.into_iter().collect()
}

fn verbora_corpus(docs: &[String]) -> TfIdf {
    let mut t = TfIdf::new();
    #[expect(clippy::cast_precision_loss, reason = "test corpora are tiny")]
    for (i, doc) in docs.iter().enumerate() {
        t.add_document(DocumentInput::Text(doc), DocKey::Num(i as f64), false)
            .expect("a fresh instance always has a documents array");
    }
    t
}

/// The exact example from `crates/verbora-tfidf/src/lib.rs`'s own module doc
/// comment, replayed through this file's own `verbora_corpus` builder to
/// confirm the harness reproduces it, not just that the library does.
#[test]
fn verbora_corpus_matches_lib_doc_example() {
    let docs = [
        "this document is about node.".to_owned(),
        "this document is about ruby.".to_owned(),
        "this document is about ruby and node.".to_owned(),
        "this document is about node. it has node examples".to_owned(),
    ];
    let mut t = verbora_corpus(&docs);
    assert_eq!(t.idf("node").unwrap(), 1.0 + (4.0f64 / 4.0).ln());
    assert_eq!(t.tfidfs(Terms::Text("node")).unwrap(), [1.0, 0.0, 1.0, 2.0]);
}

/// The property every `bench_build`/`bench_idf`/`bench_tfidf` group in
/// `benches/tfidf.rs` relies on implicitly: rotating for a larger `n` never
/// changes the documents a smaller `n` already produced, so every size in
/// `SIZES` sees a genuine nested-prefix nesting of the same underlying
/// article rather than an unrelated resample.
#[test]
fn rotated_texts_are_nested_prefixes() {
    let text = "one two three four five six seven eight nine ten";
    let small = rotated_texts(text, 4);
    let large = rotated_texts(text, 16);
    assert_eq!(small, &large[..4]);
}

/// Sanity check on the real shared source `benches/tfidf.rs`'s `document()`
/// reads: if the Wikipedia fixture is present, rotation actually produces
/// *different* documents (not `n` copies of the same slice), which is what
/// makes "build an n-document corpus" a meaningfully different workload at
/// each size rather than the same single document added `n` times.
#[test]
fn rotated_texts_differ_on_real_article() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("repo root is 3 levels up from rust-competitors/")
        .join("benches/data/corpus/Wikipedia_EN_FrenchRevolution.json");
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return; // Fixture not present in this checkout; nothing to check.
    };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return;
    };
    let Some(text) = json.get("text").and_then(|t| t.as_str()) else {
        return;
    };
    let docs = rotated_texts(text, 4);
    let unique: std::collections::HashSet<&String> = docs.iter().collect();
    assert_eq!(unique.len(), docs.len(), "rotated documents should differ");
}

/// The wrap-around behaviour `rotated_texts`'s `% words.len()` exists for:
/// once `i * 7` passes the word count, the rotation cycle repeats exactly —
/// document `i` and document `i + len` are the same string (for `len`
/// coprime with 7, one full period covers every offset). The benchmark
/// groups never rely on uniqueness beyond one period (the real article has
/// ~25k words, far more than `7 * 256`), but the formula must still be
/// well-defined past it — this pins that down instead of assuming it.
#[test]
fn rotated_texts_wrap_modulo_word_count() {
    let text = "one two three four five six seven eight nine ten"; // 10 words
    let docs = rotated_texts(text, 20);
    assert_eq!(
        &docs[..10],
        &docs[10..],
        "rotation offsets are (i * 7) % 10, so the cycle repeats with period 10"
    );
    // And within one period, every document is distinct (gcd(7, 10) = 1).
    let unique: std::collections::HashSet<&String> = docs[..10].iter().collect();
    assert_eq!(unique.len(), 10);
}

/// What `benches/tfidf.rs`'s `build_many_small` group assumes of
/// [`chunked_texts`]: every document has exactly `words_per_doc` words
/// (including when the window wraps past the end of the source text), a
/// larger `n` never changes the documents a smaller `n` already produced
/// (same nested-prefix property `rotated_texts_are_nested_prefixes` checks
/// for the other shape), and total corpus size therefore scales linearly
/// with `n` — the property that makes its per-`n` `Throughput::Bytes` values
/// meaningful.
#[test]
fn chunked_texts_have_fixed_size_and_nest() {
    let text = "one two three four five six seven eight nine ten"; // 10 words
    let words_per_doc = 4;
    let small = chunked_texts(text, words_per_doc, 3);
    let large = chunked_texts(text, words_per_doc, 12);
    assert_eq!(small, &large[..3], "chunked corpora nest as prefixes");
    for doc in &large {
        assert_eq!(
            doc.split_whitespace().count(),
            words_per_doc,
            "every chunk has exactly words_per_doc words, wrap-around included"
        );
    }
    // Wrap-around produces a real rolling window, not truncation: the third
    // chunk starts at word 8 of 10 and wraps to the front.
    assert_eq!(large[2], "nine ten one two");
}

/// The other property every benchmark group implicitly assumes of the
/// corpus builder: a corpus built from `n` texts really contains `n`
/// documents (one `tfidfs` score per document, in insertion order), across
/// the whole span of sizes the groups sweep.
#[test]
fn verbora_corpus_has_one_score_per_document() {
    let text = "falcon meadow copper violet ember quartz willow harbor tundra maple \
                cedar onyx prairie garnet"
        .repeat(4);
    for n in [1usize, 2, 4, 8, 16, 32] {
        let docs = rotated_texts(&text, n);
        let mut t = verbora_corpus(&docs);
        let scores = t
            .tfidfs(Terms::Text("falcon"))
            .expect("corpus was built with a documents array");
        assert_eq!(
            scores.len(),
            n,
            "a corpus built from {n} texts must score exactly {n} documents"
        );
    }
}

/// The `rust_tfidf` adapter check: [`vectorize`]'s term counts must sum to
/// the whitespace token count of the input (nothing dropped, nothing double
/// counted), fold case exactly like `benches/tfidf.rs`'s query probes assume
/// ("The" and "the" are one term), and emit each term once.
#[test]
fn vectorize_counts_match_whitespace_tokens() {
    let doc = "The falcon saw the FALCON near the meadow";
    let vec = vectorize(doc);

    let total: usize = vec.iter().map(|&(_, c)| c).sum();
    assert_eq!(total, doc.split_whitespace().count());

    let unique: std::collections::HashSet<&String> = vec.iter().map(|(t, _)| t).collect();
    assert_eq!(unique.len(), vec.len(), "each term appears exactly once");

    let count_of = |term: &str| {
        vec.iter()
            .find(|(t, _)| t == term)
            .map(|&(_, c)| c)
            .unwrap_or(0)
    };
    assert_eq!(count_of("the"), 3, "The/the fold to one lowercased term");
    assert_eq!(count_of("falcon"), 2, "falcon/FALCON fold together");
    assert_eq!(count_of("meadow"), 1);
    assert_eq!(count_of("The"), 0, "no unfolded variant survives");
}

/// Ordinal idf agreement — see the module doc comment for why this is the
/// one cross-implementation dimension that IS checkable despite the
/// documented value-level exclusion: every idf variant here (Verbora's
/// `1 + ln(N/(1+df))`, afshinm's `log10(N/df)`, rust-tfidf's smoothed
/// inverse frequency) is strictly decreasing in document frequency, so all
/// three must rank `df=1 > df=2 > df=3 > df=4` identically. Values are never
/// compared across implementations.
#[test]
fn all_implementations_order_idf_by_rarity_identically() {
    // df: falcon=4, meadow=3, copper=2, violet=1. Plain lowercase words, no
    // punctuation (afshinm splits on ' ' only), none in Verbora's default
    // stop-word list.
    let docs = [
        "falcon meadow copper violet".to_owned(),
        "falcon meadow copper".to_owned(),
        "falcon meadow".to_owned(),
        "falcon".to_owned(),
    ];

    let mut verbora = verbora_corpus(&docs);
    let v: Vec<f64> = ["violet", "copper", "meadow", "falcon"]
        .iter()
        .map(|t| verbora.idf(t).expect("corpus has a documents array"))
        .collect();
    assert!(
        v[0] > v[1] && v[1] > v[2] && v[2] > v[3],
        "verbora idf must strictly decrease with df, got {v:?}"
    );

    let mut afshinm = tfidf::tfidf::TfIdf::new();
    for doc in &docs {
        afshinm.add(doc);
    }
    let a: Vec<f32> = ["violet", "copper", "meadow", "falcon"]
        .iter()
        .map(|t| afshinm.idf(&tfidf::tfidf::Term(t)))
        .collect();
    assert!(
        a[0] > a[1] && a[1] > a[2] && a[2] > a[3],
        "afshinm idf must strictly decrease with df, got {a:?}"
    );

    let vectors: Vec<Vec<(String, usize)>> = docs.iter().map(|d| vectorize(d)).collect();
    let r: Vec<f64> = ["violet", "copper", "meadow", "falcon"]
        .iter()
        .map(|t| {
            use rust_tfidf::{Idf, idf::InverseFrequencyIdf};
            InverseFrequencyIdf::idf((*t).to_owned(), vectors.iter())
        })
        .collect();
    assert!(
        r[0] > r[1] && r[1] > r[2] && r[2] > r[3],
        "rust_tfidf idf must strictly decrease with df, got {r:?}"
    );
}

/// Ordinal tfidf agreement, the tf side of the same argument: at EQUAL
/// document frequency (so every implementation's idf factor for the two
/// terms is identical within itself), a term occurring three times in the
/// probed document must out-score a term occurring once — Verbora's raw
/// count, afshinm's `log10(count) + 1`, and rust-tfidf's augmented
/// `0.5 + 0.5·count/max` are all strictly increasing in count. Values are
/// never compared across implementations.
#[test]
fn all_implementations_order_tfidf_by_term_frequency_at_equal_df() {
    // df(wolf) = df(moon) = 2 of 3 documents; counts in document 0: wolf=3,
    // moon=1. The third document keeps df < N so every idf factor is > 0.
    let docs = [
        "wolf wolf wolf moon".to_owned(),
        "wolf moon".to_owned(),
        "river".to_owned(),
    ];

    let mut verbora = verbora_corpus(&docs);
    let v_wolf = verbora.tfidf(Terms::Text("wolf"), 0).expect("has docs");
    let v_moon = verbora.tfidf(Terms::Text("moon"), 0).expect("has docs");
    assert!(
        v_wolf > v_moon,
        "verbora: tfidf(wolf, d0)={v_wolf} must exceed tfidf(moon, d0)={v_moon}"
    );

    let mut afshinm = tfidf::tfidf::TfIdf::new();
    for doc in &docs {
        afshinm.add(doc);
    }
    let a_wolf = afshinm.tfidf(&tfidf::tfidf::Term("wolf"), 0);
    let a_moon = afshinm.tfidf(&tfidf::tfidf::Term("moon"), 0);
    assert!(
        a_wolf > a_moon,
        "afshinm: tfidf(wolf, d0)={a_wolf} must exceed tfidf(moon, d0)={a_moon}"
    );

    let vectors: Vec<Vec<(String, usize)>> = docs.iter().map(|d| vectorize(d)).collect();
    let (r_wolf, r_moon) = {
        use rust_tfidf::{TfIdf as _, TfIdfDefault};
        (
            TfIdfDefault::tfidf("wolf".to_owned(), &vectors[0], vectors.iter()),
            TfIdfDefault::tfidf("moon".to_owned(), &vectors[0], vectors.iter()),
        )
    };
    assert!(
        r_wolf > r_moon,
        "rust_tfidf: tfidf(wolf, d0)={r_wolf} must exceed tfidf(moon, d0)={r_moon}"
    );
}
