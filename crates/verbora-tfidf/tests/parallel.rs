//! Sequential-vs-parallel parity for [`TfIdf::par_add_documents_batch`]
//! (`parallel` feature only).
//!
//! Every other `parallel`-gated crate in this workspace fans a **pure**
//! sequential function out over `par_iter()` — the primitive itself never
//! changes, and the parity test proves the fan-out is a no-op transformation
//! on top of it. `TfIdf::add_document` cannot be fanned out that way at all:
//! it mutates `&mut self` shared corpus state (the interner, the incremental
//! document-frequency table, the idf cache) on every call, which is exactly
//! why a naive `docs.par_iter().for_each(|d| corpus.add_document(d))` cannot
//! compile, and why a `Mutex`-wrapped corpus would only serialize the real
//! work back onto one thread.
//!
//! `par_add_documents_batch` instead parallelizes only the part of ingestion
//! that touches no corpus state — lowercasing and tokenizing each text — and
//! replays the result through the *exact* sequential interning/counting loop
//! `add_document` already uses, in original order, on one thread. This suite
//! is the proof that the split is invisible from the outside: for every input
//! below, a corpus built by `N` sequential `add_document(Text, key, false)`
//! calls and a corpus built by one `par_add_documents_batch` call over the
//! same `(text, key)` pairs, in the same order, are asserted to agree on
//! *everything observable* — `to_json` (documents, keys, term/count maps, in
//! `for…in` order), the idf-cache prototype-identity flag, `idf`/`tfidfs` for
//! a broad set of probe terms (including ones absent from every document, and
//! `Object.prototype` member names that only diverge on a prototype-backed
//! cache), and `list_terms` for every document (term order, `tf`, `idf`,
//! `tfidf`).
//!
//! Inputs reuse the edge cases `src/tfidf.rs`'s own `#[cfg(test)] mod tests`
//! already knows to exercise — empty input, a single stop-worded character,
//! uppercase folding, accented Latin, Turkish dotted İ, Greek/Cyrillic, CJK,
//! astral characters, punctuation, digit-run hoisting, the `__proto__`/`__key`
//! term collisions, an `Object.prototype`-shadowing term name, and the
//! README's own four-document corpus — rather than inventing new ones, plus a
//! very long input at the same order of magnitude as `very_long_input`.

#![cfg(feature = "parallel")]

use std::sync::{Mutex, MutexGuard};

use verbora_tfidf::{DocKey, DocumentInput, Terms, TfIdf, globals};

/// Serialises every test in this file against the process-global tokenizer,
/// exactly like `src/tfidf.rs`'s own `#[cfg(test)] mod tests` and
/// `tests/parity.rs` do. Only one test here calls `set_tokenizer`, but Rust
/// test binaries run on multiple threads by default, so an unguarded test can
/// still observe that one's tokenizer mid-flight.
static GLOBALS: Mutex<()> = Mutex::new(());

fn lock() -> MutexGuard<'static, ()> {
    GLOBALS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

// ---------------------------------------------------------------------------
// Shared fixtures
// ---------------------------------------------------------------------------

/// Text documents already known, from `src/tfidf.rs`'s own test suite, to
/// exercise this crate's sharpest edges.
fn pathological_texts() -> Vec<String> {
    vec![
        "".to_owned(),
        "a".to_owned(),
        "q".to_owned(),
        "1".to_owned(),
        "Node NODE node".to_owned(),
        "naïve café crème brûlée".to_owned(),
        "İstanbul".to_owned(),
        "ΑΣ ΟΣ".to_owned(),
        "Москва и Ленинград".to_owned(),
        "ёлка ель".to_owned(),
        "日本語 test 中文测试".to_owned(),
        "😀abc😀".to_owned(),
        "a😀b".to_owned(),
        "𝕳𝖊𝖑𝖑𝖔".to_owned(),
        "e.g. U.S.A. don't hyphen-ated_word".to_owned(),
        "zeta 2020 alpha 10 beta 3.14".to_owned(),
        "__proto__ __proto__ alpha".to_owned(),
        "__key __key alpha".to_owned(),
        "toString toString".to_owned(),
        "constructor hasOwnProperty valueOf".to_owned(),
        "this document is about node.".to_owned(),
        "this document is about ruby.".to_owned(),
        "this document is about ruby and node.".to_owned(),
        "this document is about node. it has node examples".to_owned(),
        // Same order of magnitude as `very_long_input`'s corpus: a huge
        // run of two repeating terms, plus one huge single token.
        "lorem ipsum ".repeat(20_000),
        "a".repeat(100_000),
    ]
}

/// Cycles through every `DocKey` shape `add_document` accepts, so the batch
/// also exercises `DocKey::clone()` across variants, not just `Num`.
fn key_for(i: usize) -> DocKey {
    #[expect(clippy::cast_precision_loss, reason = "test corpora are tiny")]
    match i % 4 {
        0 => DocKey::Undefined,
        1 => DocKey::Num(i as f64),
        2 => DocKey::string(format!("key{i}")),
        _ => DocKey::Bool(i % 2 == 0),
    }
}

fn docs_from(texts: &[String]) -> Vec<(&str, DocKey)> {
    texts
        .iter()
        .enumerate()
        .map(|(i, t)| (t.as_str(), key_for(i)))
        .collect()
}

/// Builds a corpus with `N` sequential `add_document(Text, key, false)`
/// calls — the ground truth `par_add_documents_batch` must reproduce exactly.
fn build_sequential(docs: &[(&str, DocKey)]) -> TfIdf {
    let mut t = TfIdf::new();
    for (text, key) in docs {
        t.add_document(DocumentInput::Text(text), key.clone(), false)
            .unwrap();
    }
    t
}

fn build_parallel(docs: &[(&str, DocKey)]) -> TfIdf {
    let mut t = TfIdf::new();
    t.par_add_documents_batch(docs).unwrap();
    t
}

/// Every distinct token across `texts` (via the same tokenizer/lowercasing
/// path the corpus itself uses), plus terms known to matter for this crate's
/// own reasons: absent entirely, or an `Object.prototype` member name that
/// only diverges on a prototype-backed idf cache.
fn probe_terms(texts: &[String]) -> Vec<String> {
    let mut terms: Vec<String> = Vec::new();
    for text in texts {
        let lowered = text.to_lowercase();
        globals::tokenize_global(&lowered).for_each(|t| {
            if !terms.iter().any(|existing| existing == t) {
                terms.push(t.to_owned());
            }
        });
    }
    for extra in [
        "toString",
        "constructor",
        "hasOwnProperty",
        "valueOf",
        "__proto__",
        "__key",
        "nowhere_to_be_found",
    ] {
        terms.push(extra.to_owned());
    }
    terms
}

/// `a == b`, treating two `NaN`s as equal — `idf` over an empty corpus is
/// `-Infinity` (fine, `==` handles that), but a `toString`-shadowed `tfidf`
/// is genuinely `NaN` on both sides and must not fail the comparison.
fn f64_eq(a: f64, b: f64) -> bool {
    a.to_bits() == b.to_bits() || (a.is_nan() && b.is_nan())
}

/// Asserts every observable surface of `a` and `b` agrees.
fn assert_full_parity(a: &mut TfIdf, b: &mut TfIdf, texts: &[String], label: &str) {
    assert_eq!(a.to_json(), b.to_json(), "{label}: to_json diverged");
    assert_eq!(
        a.idf_cache_is_prototype_backed(),
        b.idf_cache_is_prototype_backed(),
        "{label}: idf cache identity diverged"
    );

    let terms = probe_terms(texts);
    for term in &terms {
        let want = a.idf(term).unwrap();
        let got = b.idf(term).unwrap();
        assert!(
            f64_eq(want, got),
            "{label}: idf({term:?}) diverged: sequential={want:?} parallel={got:?}"
        );

        let want_s = a.tfidfs(Terms::Text(term)).unwrap();
        let got_s = b.tfidfs(Terms::Text(term)).unwrap();
        assert_eq!(
            want_s.len(),
            got_s.len(),
            "{label}: tfidfs({term:?}) length diverged"
        );
        for (i, (w, g)) in want_s.iter().zip(got_s.iter()).enumerate() {
            assert!(
                f64_eq(*w, *g),
                "{label}: tfidfs({term:?})[{i}] diverged: sequential={w:?} parallel={g:?}"
            );
        }
    }

    let len = a.documents().map(<[_]>::len).unwrap_or(0);
    assert_eq!(
        len,
        b.documents().map(<[_]>::len).unwrap_or(0),
        "{label}: document count diverged"
    );
    for d in 0..len {
        let want = a.list_terms(d).unwrap();
        let got = b.list_terms(d).unwrap();
        assert_eq!(
            want.len(),
            got.len(),
            "{label}: list_terms({d}) length diverged"
        );
        for (i, (w, g)) in want.iter().zip(got.iter()).enumerate() {
            assert_eq!(
                w.term, g.term,
                "{label}: list_terms({d})[{i}] term diverged"
            );
            assert_eq!(
                format!("{:?}", w.tf),
                format!("{:?}", g.tf),
                "{label}: list_terms({d})[{i}] ({:?}) tf diverged",
                w.term
            );
            assert_eq!(
                format!("{:?}", w.idf),
                format!("{:?}", g.idf),
                "{label}: list_terms({d})[{i}] ({:?}) idf diverged",
                w.term
            );
            assert!(
                f64_eq(w.tfidf, g.tfidf),
                "{label}: list_terms({d})[{i}] ({:?}) tfidf diverged: sequential={:?} parallel={:?}",
                w.term,
                w.tfidf,
                g.tfidf
            );
        }
    }
}

fn check(texts: &[String], label: &str) {
    let docs = docs_from(texts);
    let mut sequential = build_sequential(&docs);
    let mut parallel = build_parallel(&docs);
    assert_full_parity(&mut sequential, &mut parallel, texts, label);
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn empty_batch_touches_nothing() {
    let _guard = lock();
    let mut t = TfIdf::new();
    t.par_add_documents_batch(&[]).unwrap();
    assert_eq!(t.to_json(), r#"{"documents":[],"_idfCache":{}}"#);
    // A fresh corpus starts prototype-backed; an empty batch must not swap
    // that identity, exactly as zero `add_document` calls would not.
    assert!(t.idf_cache_is_prototype_backed());
}

#[test]
fn one_item_matches_the_sequential_call() {
    let _guard = lock();
    let texts = vec!["this document is about node.".to_owned()];
    check(&texts, "one_item");
}

#[test]
fn empty_string_document_matches() {
    let _guard = lock();
    let texts = vec![String::new()];
    check(&texts, "empty_string_document");
}

#[test]
fn pathological_and_unicode_texts_match() {
    let _guard = lock();
    let texts = pathological_texts();
    check(&texts, "pathological_and_unicode");
}

#[test]
fn many_items_preserve_order_and_match_the_sequential_loop() {
    let _guard = lock();
    // Cycled out past any reasonable Rayon task-splitting boundary, so a
    // reordering or off-by-one merge bug would show up as a mismatch rather
    // than passing by accident.
    let base = pathological_texts();
    let mut texts: Vec<String> = Vec::new();
    while texts.len() < 600 {
        texts.extend(base.iter().cloned());
    }
    check(&texts, "many_items");
}

#[test]
fn readme_corpus_matches_including_undefined_keys() {
    let _guard = lock();
    // The README's own four-document corpus, all with `DocKey::Undefined` —
    // the common case, and distinct from `key_for`'s mixed-variant cycling.
    let texts = vec![
        "this document is about node.".to_owned(),
        "this document is about ruby.".to_owned(),
        "this document is about ruby and node.".to_owned(),
        "this document is about node. it has node examples".to_owned(),
    ];
    let docs: Vec<(&str, DocKey)> = texts
        .iter()
        .map(|t| (t.as_str(), DocKey::Undefined))
        .collect();
    let mut sequential = build_sequential(&docs);
    let mut parallel = build_parallel(&docs);
    assert_full_parity(
        &mut sequential,
        &mut parallel,
        &texts,
        "readme_undefined_keys",
    );
}

#[test]
fn string_keys_clone_correctly_through_the_batch() {
    let _guard = lock();
    let texts = vec!["alpha beta".to_owned(), "beta gamma".to_owned()];
    let docs: Vec<(&str, DocKey)> = texts
        .iter()
        .map(|t| (t.as_str(), DocKey::string("shared-looking-key")))
        .collect();
    let mut sequential = build_sequential(&docs);
    let mut parallel = build_parallel(&docs);
    // Two independently-constructed `DocKey::string` values are NOT `===`
    // (object/string keys still compare by value here since `Str` holds an
    // `Arc<str>` compared structurally) — this just proves the clone carries
    // the right *value* through the parallel path.
    assert_full_parity(&mut sequential, &mut parallel, &texts, "string_keys");
}

#[test]
fn a_custom_tokenizer_is_honoured_by_the_parallel_path() {
    let _guard = lock();
    // `par_add_documents_batch` reads the same process-global tokenizer
    // `add_document` does. `GLOBALS` is what keeps this from leaking into
    // every other test in this file while it is installed.
    verbora_tfidf::TfIdf::set_tokenizer(std::sync::Arc::new(
        verbora_tokenizers::TreebankWordTokenizer::new(),
    ));
    let texts = vec!["this isn't node".to_owned(), "won't you join us".to_owned()];
    check(&texts, "custom_tokenizer");
    globals::reset_tokenizer();
}
