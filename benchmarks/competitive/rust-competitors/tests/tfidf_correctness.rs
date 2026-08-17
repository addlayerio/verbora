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
//! `benches/tfidf.rs`'s own module doc comment says the same. What this file
//! checks instead is that this crate's OWN corpus-construction helpers
//! (duplicated from `benches/tfidf.rs`, since a `tests/*.rs` binary cannot
//! import private items from a `[[bench]]` target) produce the values
//! `crates/verbora-tfidf/src/lib.rs`'s own doc-comment example promises, and
//! that the rotation formula genuinely nests as every benchmark group
//! implicitly assumes.

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
