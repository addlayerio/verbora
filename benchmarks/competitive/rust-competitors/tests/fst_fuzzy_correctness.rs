//! Correctness of `fst::Set::search` + `fst::automaton::Levenshtein` against
//! `verbora_spellcheck::FuzzyIndex::neighbors` — the property
//! `benches/fst_fuzzy.rs`'s own NARROWED_EXACT classification rests on.
//! Checked once, directly, before any timing number in that file is
//! trusted, per this workbench's `CORRECTNESS BEFORE PERFORMANCE` rule.
//!
//! Both sides must agree on the exact *set* of matches (never order — `fst`
//! returns matches in the automaton's own traversal order,
//! `FuzzyIndex::neighbors` in its BK-tree's own traversal order) for every
//! query and `max_distance` tested. The corpus is ASCII-only, the domain
//! `benches/fst_fuzzy.rs`'s own module doc comment documents as where the
//! two metrics (`verbora_distance::levenshtein`'s UTF-16-code-unit count vs.
//! `fst::automaton::Levenshtein`'s Unicode-scalar-value count) provably
//! agree — this file does not test non-ASCII input, on purpose.

use std::collections::BTreeSet;

use fst::automaton::Levenshtein;
use fst::{IntoStreamer, Set, Streamer};
use verbora_spellcheck::{FuzzyIndex, FuzzyIndexBuilder};

fn words() -> Vec<String> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("repo root is 3 levels up from rust-competitors/")
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
        .map(|w| w.as_str().expect("word").to_owned())
        .collect()
}

fn distinct_words(n: usize) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    words()
        .into_iter()
        .filter(|w| seen.insert(w.clone()))
        .take(n)
        .collect()
}

fn build_fuzzy_index(words: &[String]) -> FuzzyIndex {
    let mut b = FuzzyIndexBuilder::new();
    for w in words {
        b.insert(w);
    }
    b.build()
}

fn build_fst(words: &[String]) -> Set<Vec<u8>> {
    let mut sorted: Vec<&str> = words.iter().map(String::as_str).collect();
    sorted.sort_unstable();
    sorted.dedup();
    Set::from_iter(sorted).expect("sorted, deduplicated input builds an fst::Set")
}

fn fst_neighbors(set: &Set<Vec<u8>>, query: &str, max_distance: u32) -> BTreeSet<String> {
    let lev = Levenshtein::new(query, max_distance).expect("query within the default state limit");
    let mut stream = set.search(&lev).into_stream();
    let mut out = BTreeSet::new();
    while let Some(key) = stream.next() {
        out.insert(String::from_utf8(key.to_vec()).expect("ASCII corpus is valid UTF-8"));
    }
    out
}

#[test]
fn fst_and_fuzzy_index_agree_on_ascii_queries() {
    let corpus = distinct_words(3_000);
    let index = build_fuzzy_index(&corpus);
    let fst_set = build_fst(&corpus);

    let mut queries: Vec<String> = corpus.iter().take(30).cloned().collect();
    for w in corpus.iter().skip(30).take(30) {
        let mut perturbed = w.clone();
        if !perturbed.is_empty() {
            perturbed.push('x');
        }
        queries.push(perturbed);
    }
    queries.push("zzzzzzzzzzzzzzzzzzzz".to_owned());

    for query in &queries {
        for max_distance in [0, 1, 2] {
            let via_fuzzy_index: BTreeSet<String> = index
                .neighbors(query, max_distance)
                .map(str::to_owned)
                .collect();
            let via_fst = fst_neighbors(&fst_set, query, max_distance);
            assert_eq!(
                via_fuzzy_index, via_fst,
                "mismatch for query {query:?} at max_distance {max_distance}"
            );
        }
    }
}

#[test]
fn fst_levenshtein_automaton_size_limit_is_real() {
    // The module doc comment's own "size/memory caveat" claim, pinned down
    // as a real, reproducible test rather than left as prose: a wide enough
    // max_distance on a long enough query exceeds fst's default DFA-state
    // budget and errors, a failure mode `FuzzyIndex::neighbors` has no
    // equivalent of (its `max_distance` is a plain integer, no automaton to
    // build).
    let long_query = "supercalifragilisticexpialidocious";
    let result = Levenshtein::new(long_query, 20);
    assert!(
        result.is_err(),
        "expected a query this long at this distance to exceed fst's default state limit"
    );
}
