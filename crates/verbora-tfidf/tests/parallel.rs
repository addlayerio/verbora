//! `par_add_documents` must be indistinguishable from the sequential loop.
//!
//! The method exists to run the analyzer on more than one core; it is only
//! legitimate if the corpus it leaves behind is byte-for-byte the one
//! `add_documents` would have left. These tests hold it to that over corpora
//! chosen to stress every part of the pipeline: case folding (including the
//! mappings that change a token's length and the context-sensitive one), the
//! UAX #29 interior-punctuation rules, non-ASCII scripts, astral scalars,
//! documents that yield no terms at all, terms on both sides of the term
//! table's eight-byte short/long boundary, and stop-word filtering.
//!
//! The comparison is [`TfIdf::to_json`], which is the widest observable
//! surface the crate has: it carries every document's key, every term, every
//! count, and their order.

#![cfg(feature = "parallel")]

use verbora_core::{StopWordLanguage, StopWords};
use verbora_tfidf::{Analyzer, CaseFold, TfIdf};

/// The vocabulary the randomized corpora are drawn from: one entry per input
/// class the ingest pipeline treats differently.
const VOCABULARY: &[&str] = &[
    "the",
    "quick",
    "node",
    "ruby",
    "2020",
    "10",
    "0",
    "4294967295",
    "İstanbul",
    "naïve",
    "ΑΣ",
    "ΟΣ",
    "Σ",
    "ёлка",
    "Москва",
    "don't",
    "3.14",
    "1,000",
    "a:b",
    "node_js",
    "a",
    "_",
    "___",
    "e.g.",
    "😀abc😀",
    "𝕳𝖊𝖑𝖑𝖔",
    "MiXeD",
    "CASE",
    "ẞ",
    "ǅungla",
    "日本語",
    "exactly8",
    "morethan8bytes",
    "",
];

/// A deterministic xorshift64, so a failure is reproducible.
fn xorshift(state: &mut u64) -> u64 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    *state
}

/// `count` documents built by joining random vocabulary entries with random
/// separators.
fn random_corpus(state: &mut u64, count: usize) -> Vec<String> {
    (0..count)
        .map(|_| {
            let terms = (xorshift(state) % 40) as usize;
            let mut text = String::new();
            for i in 0..terms {
                if i > 0 {
                    match xorshift(state) % 4 {
                        0 => text.push(' '),
                        1 => text.push_str("  "),
                        2 => text.push('.'),
                        _ => text.push_str(", "),
                    }
                }
                let index = (xorshift(state) % VOCABULARY.len() as u64) as usize;
                text.push_str(VOCABULARY[index]);
            }
            text
        })
        .collect()
}

fn assert_parallel_matches_sequential(analyzer: &Analyzer, texts: &[String], label: &str) {
    let mut parallel = TfIdf::with_analyzer(analyzer.clone());
    let added = parallel.par_add_documents(texts);
    assert_eq!(added, 0..texts.len(), "{label}: wrong positions");

    let mut sequential = TfIdf::with_analyzer(analyzer.clone());
    for text in texts {
        sequential.add_document(text);
    }

    assert_eq!(
        parallel.to_json().expect("default tokenizer"),
        sequential.to_json().expect("default tokenizer"),
        "{label}: serialized state diverged"
    );

    // …and the derived numbers, which the JSON does not carry.
    for term in VOCABULARY {
        assert_eq!(
            parallel.document_frequency(term),
            sequential.document_frequency(term),
            "{label}: df({term:?})"
        );
        assert_eq!(
            parallel.tfidfs(term),
            sequential.tfidfs(term),
            "{label}: tfidfs({term:?})"
        );
    }
}

#[test]
fn parallel_ingestion_matches_the_sequential_loop_on_random_corpora() {
    let analyzers = [
        Analyzer::new(),
        Analyzer::new().with_case_fold(CaseFold::None),
        Analyzer::new().with_stop_words(StopWords::for_language(StopWordLanguage::En)),
        Analyzer::new().with_stop_words(StopWords::from_iter_of(["the", "a", "node"])),
    ];
    let mut state = 0x243F_6A88_85A3_08D3_u64;
    for (index, analyzer) in analyzers.iter().enumerate() {
        for case in 0..40u32 {
            let count = 1 + (xorshift(&mut state) % 9) as usize;
            let texts = random_corpus(&mut state, count);
            assert_parallel_matches_sequential(
                analyzer,
                &texts,
                &format!("analyzer {index}, case {case}"),
            );
        }
    }
}

#[test]
fn parallel_ingestion_matches_the_sequential_loop_on_the_awkward_shapes() {
    let texts: Vec<String> = [
        "",
        " ",
        "   ,,,   ",
        "İstanbul İstanbul",
        "ΑΣ ΟΣ Σ ΑΣΑ",
        "don't 3.14 1,000 a:b node_js",
        "日本語 test 中文测试",
        "😀abc😀 a😀b 𝕳𝖊𝖑𝖑𝖔",
        "exactly8 exactly8b morethan8bytes",
        "MiXeD CASE mixed case",
        "one",
    ]
    .iter()
    .map(|s| (*s).to_owned())
    .collect();
    assert_parallel_matches_sequential(&Analyzer::new(), &texts, "awkward shapes");
    assert_parallel_matches_sequential(
        &Analyzer::new().with_case_fold(CaseFold::None),
        &texts,
        "awkward shapes, unfolded",
    );
}

#[test]
fn an_empty_batch_changes_nothing() {
    let mut corpus = TfIdf::new();
    corpus.add_document("node");
    let before = corpus.to_json().expect("default tokenizer");
    let empty: [&str; 0] = [];
    assert_eq!(corpus.par_add_documents(&empty), 1..1);
    assert_eq!(corpus.to_json().expect("default tokenizer"), before);
}

#[test]
fn a_parallel_batch_appends_after_existing_documents() {
    let mut corpus = TfIdf::new();
    corpus.add_document("first");
    let added = corpus.par_add_documents(&["second", "third"]);
    assert_eq!(added, 1..3);
    assert_eq!(corpus.len(), 3);
    assert_eq!(corpus.term_count(0, "first"), Some(1));
    assert_eq!(corpus.term_count(2, "third"), Some(1));
}

#[test]
fn a_large_batch_still_matches_the_sequential_loop() {
    // Enough documents to make Rayon actually split the work across tasks.
    let texts: Vec<String> = (0..2_000)
        .map(|i| format!("document number {i} about node and ruby and {}", i % 37))
        .collect();
    assert_parallel_matches_sequential(&Analyzer::new(), &texts, "2000 documents");
}
