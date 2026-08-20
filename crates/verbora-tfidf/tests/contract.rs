//! The published contract, exercised through the crate boundary.
//!
//! Everything here uses only what `verbora-tfidf` re-exports from its root, so
//! it also checks that the public surface is sufficient to use the crate — a
//! unit test inside the crate can reach private helpers and would not notice a
//! missing re-export.

use std::sync::Arc;

use verbora_core::{StopWordLanguage, StopWords};
use verbora_tfidf::{
    Analyzer, ArtifactStamp, CaseFold, Document, DocumentScore, ExportError, RestoreError,
    StampError, TermScore, TfIdf, Tokenize, natural_log,
};

/// Every text this suite scores against, one per input class the pipeline
/// treats differently.
const QUERIES: &[&str] = &[
    "",
    " ",
    "...",
    "node",
    "the quick brown fox",
    "The",
    "NODE",
    "Node.js",
    "node.js",
    "node, ruby and rails",
    "isn't",
    "don't",
    "3.14",
    "1,000",
    "a:b",
    "node_js",
    "über",
    "ÜBER",
    "straße",
    "İstanbul",
    "ΟΣ",
    "日本語",
    "😀",
    "a😀b",
    "hello  world",
    " leading",
    "trailing ",
    "a-b-c",
    "_",
    "0",
    "4294967295",
    "a-single-term-well-past-the-eight-byte-boundary",
];

fn sample_corpus() -> TfIdf {
    let mut corpus = TfIdf::new();
    corpus.add_document_with_key("this document is about node", "one");
    corpus.add_document_with_key("this document is about ruby", "two");
    corpus.add_document_with_key("this document is about ruby and node", "three");
    corpus.add_document_with_key("node_js and n0de and über and 3.14 and don't", "four");
    corpus.add_document_with_key("", "empty");
    corpus
}

// --- the numeric contract --------------------------------------------------

/// Every score, over every query and every document, must be finite — and the
/// three query entry points must agree to the last bit.
#[test]
fn every_query_is_finite_and_the_entry_points_agree() {
    let corpus = sample_corpus();
    for query in QUERIES {
        let analyzed = corpus.analyzer().terms(query);
        let all = corpus.tfidfs(query);
        assert_eq!(all.len(), corpus.len(), "{query:?}");
        assert_eq!(all, corpus.tfidfs_terms(&analyzed), "{query:?}");

        for (document, score) in all.iter().enumerate() {
            assert!(
                score.is_finite(),
                "tfidfs({query:?})[{document}] = {score:?}"
            );
            assert_eq!(
                corpus.tfidf(query, document).map(f64::to_bits),
                Some(score.to_bits()),
                "tfidf({query:?}, {document})"
            );
            assert_eq!(
                corpus.tfidf_terms(&analyzed, document).map(f64::to_bits),
                Some(score.to_bits()),
                "tfidf_terms({analyzed:?}, {document})"
            );
        }

        let mut from_rank = vec![f64::NAN; corpus.len()];
        for DocumentScore { document, score } in corpus.rank(query) {
            from_rank[document] = score;
        }
        assert_eq!(from_rank, all, "rank({query:?})");
    }
}

#[test]
fn idf_matches_the_published_formula_for_every_term_in_the_corpus() {
    let corpus = sample_corpus();
    let n = corpus.len();
    #[expect(clippy::cast_precision_loss, reason = "five documents")]
    let n_as_f64 = n as f64;
    for document in 0..n {
        for (term, count) in corpus
            .document_terms(document)
            .expect("document in range")
            .map(|(t, c)| (t.to_owned(), c))
            .collect::<Vec<_>>()
        {
            let df = corpus.document_frequency(&term);
            assert!(df >= 1, "a stored term occurs in at least one document");
            let want = 1.0 + natural_log(n_as_f64 / (1.0 + f64::from(df)));
            assert_eq!(corpus.idf(&term), Some(want), "idf({term:?})");
            assert_eq!(corpus.term_count(document, &term), Some(count));
        }
    }
}

#[test]
fn an_empty_corpus_answers_none_rather_than_a_sentinel() {
    let corpus = TfIdf::new();
    assert!(corpus.is_empty());
    assert_eq!(corpus.idf("node"), None);
    assert_eq!(corpus.tfidf("node", 0), None);
    assert_eq!(corpus.tfidf_terms(["node"], 0), None);
    assert_eq!(corpus.term_count(0, "node"), None);
    assert_eq!(corpus.list_terms(0), None);
    assert!(corpus.tfidfs("node").is_empty());
    assert!(corpus.rank("node").is_empty());
    assert!(corpus.documents().is_empty());
}

/// The two orderings this crate publishes are total, so two runs of the same
/// query produce the same permutation and equal scores never shuffle.
#[test]
fn both_published_orderings_are_total_and_reproducible() {
    let mut corpus = TfIdf::new();
    // Deliberately many ties: five documents scoring 1, 1, 2, 0, 1.
    corpus.add_document("node alpha");
    corpus.add_document("node beta");
    corpus.add_document("node node gamma");
    corpus.add_document("delta");
    corpus.add_document("node epsilon");

    let ranked = corpus.rank("node");
    assert_eq!(
        ranked.iter().map(|r| r.document).collect::<Vec<_>>(),
        [2, 0, 1, 4, 3]
    );
    for _ in 0..8 {
        assert_eq!(corpus.rank("node"), ranked);
    }

    let terms = corpus.list_terms(2).expect("document in range");
    let names: Vec<&str> = terms.iter().map(|t| t.term.as_str()).collect();
    assert_eq!(names, ["node", "gamma"]);
    for _ in 0..8 {
        assert_eq!(corpus.list_terms(2), Some(terms.clone()));
    }
    // Descending by score, then ascending by term.
    for pair in terms.windows(2) {
        let (a, b) = (&pair[0], &pair[1]);
        assert!(a.tfidf > b.tfidf || (a.tfidf == b.tfidf && a.term < b.term));
    }
}

#[test]
fn list_terms_agrees_with_the_single_term_query() {
    let corpus = sample_corpus();
    for document in 0..corpus.len() {
        for TermScore {
            term,
            count,
            idf,
            tfidf,
        } in corpus.list_terms(document).expect("document in range")
        {
            assert_eq!(Some(count), corpus.term_count(document, &term));
            assert_eq!(Some(idf), corpus.idf(&term));
            assert_eq!(
                Some(tfidf.to_bits()),
                corpus.tfidf_terms([&term], document).map(f64::to_bits)
            );
        }
    }
}

// --- the analyzer ----------------------------------------------------------

#[test]
fn documents_and_queries_share_one_pipeline() {
    let analyzer = Analyzer::new().with_stop_words(StopWords::from_iter_of(["the", "is"]));
    let mut corpus = TfIdf::with_analyzer(analyzer);
    corpus.add_document("The document IS about Node");

    assert_eq!(
        corpus
            .document_terms(0)
            .expect("document in range")
            .map(|(t, _)| t)
            .collect::<Vec<_>>(),
        ["document", "about", "node"]
    );
    // A folded query finds the folded term…
    assert_eq!(corpus.tfidf("NODE", 0), corpus.tfidf("node", 0));
    // …and a filtered query term is filtered out of the query too, leaving the
    // empty query rather than a lookup that can never match.
    assert_eq!(corpus.tfidf("the is", 0), Some(0.0));
}

/// **Every** entry of the default English stop-word list, walked through the
/// pipeline that would have to produce it.
///
/// A stop-word list is a table looked up with the output of an earlier stage,
/// and the failure mode is that the table is spelled one way while the stage
/// produces another — so an entry silently filters nothing and nobody notices,
/// because the suite stays green. The only test that can see it is one that
/// enumerates the table.
///
/// Two entries of `StopWords::for_language(StopWordLanguage::En)` are unreachable through the default
/// analyzer, and both for the same reason: `WordTokenizer` keeps only the word
/// segments containing a letter or a digit, and neither `$` nor `_` contains
/// one, so no document text can ever produce them as terms. They are inert
/// rather than wrong — filtering something unproducible removes nothing — but
/// the set is pinned here so that a change to either the list or the pipeline
/// has to come past this assertion. The list itself belongs to `verbora-core`,
/// not to this crate.
#[test]
fn every_default_english_stop_word_is_reachable_or_is_a_named_exception() {
    let list = StopWords::for_language(StopWordLanguage::En);
    let plain = Analyzer::new();
    let filtering = Analyzer::new().with_stop_words(StopWords::for_language(StopWordLanguage::En));

    let mut unreachable = Vec::new();
    let mut reachable = 0usize;
    for word in list.words() {
        // What the analyzer produces from the entry spelled as its own document.
        let produced = plain.terms(word);
        if produced == [word.clone()] {
            reachable += 1;
            // …and with the list installed it must be filtered away entirely.
            assert!(
                filtering.terms(word).is_empty(),
                "{word:?} survives its own stop-word list"
            );
            // …in context as well as alone.
            assert_eq!(
                filtering.terms(&format!("alpha {word} omega")),
                ["alpha", "omega"],
                "{word:?} survives in context"
            );
        } else {
            unreachable.push((word.clone(), produced));
        }
    }

    assert_eq!(list.len(), 168, "the list this expectation was walked over");
    assert_eq!(reachable, 168);
    assert_eq!(
        unreachable,
        Vec::<(String, Vec<String>)>::new(),
        "the unreachable set changed"
    );
}

#[test]
fn the_text_unit_is_the_tokenizers_and_is_not_re_derived_here() {
    let mut corpus = TfIdf::new();
    corpus.add_document("don't 3.14 1,000 a:b node_js well-known and/or");
    let terms: Vec<&str> = corpus
        .document_terms(0)
        .expect("document in range")
        .map(|(t, _)| t)
        .collect();
    assert_eq!(
        terms,
        [
            "don't", "3.14", "1,000", "a:b", "node_js", "well", "known", "and", "or"
        ]
    );
    // One document, one occurrence of each: 1 + ln(1 / (1 + 1)).
    let single = 1.0 + natural_log(0.5);
    for term in ["don't", "3.14", "1,000", "a:b", "node_js"] {
        assert_eq!(corpus.term_count(0, term), Some(1), "{term}");
        assert_eq!(corpus.tfidf(term, 0), Some(single), "{term}");
    }
}

#[test]
fn a_custom_tokenizer_is_reachable_and_blocks_serialization() {
    #[derive(Debug)]
    struct SplitOnApostrophe;
    impl Tokenize for SplitOnApostrophe {
        fn tokenize_into(&self, text: &str, out: &mut Vec<String>) {
            out.extend(
                text.split(|c: char| c.is_whitespace() || c == '\'')
                    .filter(|piece| !piece.is_empty())
                    .map(str::to_owned),
            );
        }
    }

    let analyzer = Analyzer::new().with_tokenizer(Arc::new(SplitOnApostrophe));
    assert!(!analyzer.uses_default_tokenizer());
    let mut corpus = TfIdf::with_analyzer(analyzer);
    corpus.add_document("this isn't node");
    assert_eq!(corpus.term_count(0, "isn"), Some(1));
    assert_eq!(corpus.term_count(0, "isn't"), Some(0));
    assert!(matches!(
        corpus.to_json(),
        Err(ExportError::CustomTokenizer)
    ));

    // Two corpora in one program do not share an analyzer.
    let mut plain = TfIdf::new();
    plain.add_document("this isn't node");
    assert_eq!(plain.term_count(0, "isn't"), Some(1));
    assert!(plain.to_json().is_ok());
}

// --- persistence -----------------------------------------------------------

#[test]
fn a_corpus_round_trips_and_its_output_is_stable() {
    let corpus = sample_corpus();
    let json = corpus.to_json().expect("default tokenizer");
    let restored = TfIdf::from_json(&json).expect("its own output");

    assert_eq!(restored.len(), corpus.len());
    assert_eq!(restored.to_json().expect("default tokenizer"), json);
    for query in QUERIES {
        assert_eq!(restored.tfidfs(query), corpus.tfidfs(query), "{query:?}");
    }
    for document in 0..corpus.len() {
        assert_eq!(
            restored.document(document).and_then(Document::key),
            corpus.document(document).and_then(Document::key)
        );
        assert_eq!(
            restored.list_terms(document),
            corpus.list_terms(document),
            "document {document}"
        );
    }
}

#[test]
fn the_analyzer_travels_with_the_artifact() {
    let analyzer = Analyzer::new()
        .with_case_fold(CaseFold::None)
        .with_stop_words(StopWords::from_iter_of(["Skip"]));
    let mut corpus = TfIdf::with_analyzer(analyzer);
    corpus.add_document("Keep Skip keep");

    let restored =
        TfIdf::from_json(&corpus.to_json().expect("default tokenizer")).expect("round-trips");
    assert_eq!(restored.analyzer().case_fold(), CaseFold::None);
    assert_eq!(
        restored.analyzer().stop_words().map(StopWords::words),
        Some(["Skip".to_owned()].as_slice())
    );
    assert_eq!(restored.term_count(0, "Keep"), Some(1));
    assert_eq!(restored.term_count(0, "keep"), Some(1));
    assert_eq!(restored.term_count(0, "Skip"), Some(0));
}

#[test]
fn an_artifact_without_this_builds_stamp_is_refused() {
    let stamp = ArtifactStamp::current();
    assert_eq!(stamp.schema, verbora_tfidf::SCHEMA);
    assert!(stamp.lowercase.is_some());

    // No stamp at all.
    assert!(matches!(
        TfIdf::from_json(r#"{"analyzer":{"case_fold":"none"},"documents":[]}"#),
        Err(RestoreError::Stamp(StampError::Missing))
    ));
    // Not JSON.
    assert!(matches!(
        TfIdf::from_json("]["),
        Err(RestoreError::Parse(_))
    ));
    // A stamp from another build, on a body this build could have written.
    let (major, minor, update) = stamp.unicode;
    let foreign = format!(
        r#"{{"{}":{{"schema":{},"unicode":"{}.{minor}.{update}","lowercase":"{:016x}"}},"analyzer":{{"case_fold":"lowercase"}},"documents":[]}}"#,
        verbora_tfidf::STAMP_PROPERTY,
        stamp.schema,
        major + 1,
        stamp.lowercase.expect("this build stamps a fingerprint"),
    );
    let Err(RestoreError::Stamp(StampError::Incompatible { found, expected })) =
        TfIdf::from_json(&foreign)
    else {
        panic!("a foreign Unicode version must be refused");
    };
    assert_eq!(found.unicode, (major + 1, minor, update));
    assert_eq!(expected, stamp);
    assert!(found.to_string().contains("Unicode"));
}

// --- ingestion and removal -------------------------------------------------

#[test]
fn add_terms_stores_exactly_what_it_is_given() {
    let mut corpus = TfIdf::new();
    corpus.add_terms_with_key(["Node", "node", "NODE", "with space"], "verbatim");
    assert_eq!(corpus.find_document("verbatim"), Some(0));
    assert_eq!(
        corpus
            .document_terms(0)
            .expect("document in range")
            .collect::<Vec<_>>(),
        [("Node", 1), ("node", 1), ("NODE", 1), ("with space", 1)]
    );
    assert_eq!(
        corpus.tfidf_terms(["with space"], 0),
        Some(1.0 + natural_log(0.5))
    );
    // …and none of it is reachable from a text query, because the analyzer
    // would never produce those spellings.
    assert_eq!(corpus.tfidf("with space", 0), Some(0.0));
}

#[test]
fn removal_updates_every_derived_number_and_shifts_positions() {
    let mut corpus = TfIdf::new();
    corpus.add_document_with_key("node ruby", "a");
    corpus.add_document_with_key("node perl", "b");
    corpus.add_document_with_key("node", "c");

    assert_eq!(corpus.document_frequency("node"), 3);
    let removed = corpus.remove_document(0).expect("index in range");
    assert_eq!(removed.key(), Some("a"));
    assert_eq!(removed.distinct_terms(), 2);
    assert_eq!(removed.total_terms(), 2);

    assert_eq!(corpus.len(), 2);
    assert_eq!(corpus.find_document("b"), Some(0));
    assert_eq!(corpus.document_frequency("node"), 2);
    assert_eq!(corpus.document_frequency("ruby"), 0);
    assert_eq!(corpus.idf("ruby"), Some(1.0 + natural_log(2.0)));
    assert!(corpus.remove_document(2).is_none());
}

#[test]
fn a_utf8_file_can_be_ingested_and_a_non_utf8_one_cannot() {
    let dir = std::env::temp_dir().join(format!(
        "verbora-tfidf-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&dir).expect("temp dir");

    let good = dir.join("good.txt");
    std::fs::write(&good, "node and ruby").expect("write");
    let mut corpus = TfIdf::new();
    assert_eq!(corpus.add_document_from_path(&good).expect("readable"), 0);
    assert_eq!(corpus.term_count(0, "node"), Some(1));

    let bad = dir.join("bad.bin");
    std::fs::write(&bad, [0xffu8, 0xfe, 0xfd]).expect("write");
    let error = corpus
        .add_document_from_path(&bad)
        .expect_err("not UTF-8, so not text");
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
    // The failed read added nothing.
    assert_eq!(corpus.len(), 1);

    let missing = dir.join("missing.txt");
    assert_eq!(
        corpus
            .add_document_from_path(&missing)
            .expect_err("absent")
            .kind(),
        std::io::ErrorKind::NotFound
    );

    std::fs::remove_dir_all(&dir).expect("clean up");
}
