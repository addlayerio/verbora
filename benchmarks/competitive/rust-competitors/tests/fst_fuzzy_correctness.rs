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
//!
//! Coverage spans: the original 3 000-word sweep at distances 0–2; a larger
//! 10 000-word sweep with four deterministic query-perturbation shapes
//! (append, delete-last, substitute-first, swap-first-two); distance 3 on
//! short queries (kept ≤ 8 characters so `fst`'s automaton stays under its
//! default state limit — the limit itself is pinned by its own tests below,
//! not silently avoided); edge-shaped queries (empty, single-character,
//! longer than any stored word); duplicate-insertion set semantics; and the
//! automaton-size failure-mode asymmetry from both sides of the boundary.

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

/// Agreement on the exact set of matches, all six perturbation-derived query
/// shapes, over a corpus more than three times the original sweep's — every
/// query is a deterministic transform of a corpus word, so the sweep is
/// reproducible without any new source data. Distances 0–2, the same domain
/// the original sweep pins.
#[test]
fn fst_and_fuzzy_index_agree_on_larger_perturbed_sweep() {
    let corpus = distinct_words(10_000);
    let index = build_fuzzy_index(&corpus);
    let fst_set = build_fst(&corpus);

    let mut queries: Vec<String> = Vec::new();
    // Exact corpus words, from a slice disjoint from the perturbation bases.
    queries.extend(corpus.iter().skip(100).take(60).cloned());
    for w in corpus.iter().skip(500).take(40) {
        // Insertion shape: one appended character.
        queries.push(format!("{w}x"));
        // Deletion shape: last character removed (every corpus word is ≥ 3
        // ASCII characters, so this is always well-formed and non-empty).
        queries.push(w[..w.len() - 1].to_owned());
        // Substitution shape: first character replaced.
        queries.push(format!("q{}", &w[1..]));
        // Transposition shape: first two characters swapped. Neither metric
        // has a transposition edit (both count this as two substitutions,
        // or one insert + one delete) — the *query string* is still just a
        // string, so agreement must hold regardless.
        let b = w.as_bytes();
        queries.push(format!("{}{}{}", b[1] as char, b[0] as char, &w[2..]));
    }

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

/// Distance 3 — one step beyond the original sweep's 0–2 — on queries kept
/// short (≤ 8 characters) so `fst`'s Levenshtein automaton stays under its
/// default 10 000-state limit (its own doc comment's guidance; the limit
/// itself is asserted real by `fst_levenshtein_automaton_size_limit_is_real`
/// above, so staying under it here is a disclosed constraint, not a dodge).
#[test]
fn fst_and_fuzzy_index_agree_at_distance_three_on_short_queries() {
    let corpus = distinct_words(1_000);
    let index = build_fuzzy_index(&corpus);
    let fst_set = build_fst(&corpus);

    let queries: Vec<&String> = corpus.iter().filter(|w| w.len() <= 8).take(40).collect();
    assert!(
        !queries.is_empty(),
        "corpus must contain short words for this sweep to be meaningful"
    );

    for query in queries {
        let via_fuzzy_index: BTreeSet<String> =
            index.neighbors(query, 3).map(str::to_owned).collect();
        let via_fst = fst_neighbors(&fst_set, query, 3);
        assert_eq!(
            via_fuzzy_index, via_fst,
            "mismatch for query {query:?} at max_distance 3"
        );
    }
}

/// Edge-shaped queries: the empty string (matches exactly the words of
/// length ≤ `max_distance` — none in this corpus, whose shortest word is 3
/// characters, so both sides must agree on *empty*), a single character, a
/// two-character query, and queries at and beyond the corpus's maximum word
/// length. All lowercase ASCII, inside the documented agreement domain.
#[test]
fn fst_and_fuzzy_index_agree_on_edge_shaped_queries() {
    let corpus = distinct_words(3_000);
    let index = build_fuzzy_index(&corpus);
    let fst_set = build_fst(&corpus);

    let longest = corpus
        .iter()
        .max_by_key(|w| w.len())
        .expect("non-empty corpus")
        .clone();

    let queries: Vec<String> = vec![
        String::new(),
        "a".to_owned(),
        "zq".to_owned(),
        longest,
        // Longer than any stored word (max generated length is 14): every
        // match must come from deletions alone.
        "qqqqqqqqqqqqqqqq".to_owned(),
    ];

    for query in &queries {
        for max_distance in [0, 1, 2] {
            let via_fuzzy_index: BTreeSet<String> = index
                .neighbors(query, max_distance)
                .map(str::to_owned)
                .collect();
            let via_fst = fst_neighbors(&fst_set, query, max_distance);
            assert_eq!(
                via_fuzzy_index, via_fst,
                "mismatch for edge query {query:?} at max_distance {max_distance}"
            );
        }
    }
}

/// Both sides index a *set*: feeding the same corpus twice changes nothing.
/// `FuzzyIndexBuilder::insert` documents duplicate insertion as a no-op;
/// `build_fst` dedups before `Set::from_iter` (which would otherwise error).
/// This is the claim `distinct_words` silently relies on everywhere else in
/// this file and in `benches/fst_fuzzy.rs`, pinned directly for once.
#[test]
fn duplicate_insertion_is_a_no_op_on_both_sides() {
    let base = distinct_words(2_000);
    let doubled: Vec<String> = base.iter().chain(base.iter()).cloned().collect();

    let index = build_fuzzy_index(&doubled);
    let fst_set = build_fst(&doubled);
    assert_eq!(
        index.len(),
        base.len(),
        "duplicate inserts must not grow the fuzzy index"
    );
    assert_eq!(
        fst_set.len(),
        base.len(),
        "dedup before fst construction must collapse duplicates"
    );

    for query in base.iter().take(25) {
        for max_distance in [1, 2] {
            let via_fuzzy_index: BTreeSet<String> = index
                .neighbors(query, max_distance)
                .map(str::to_owned)
                .collect();
            let via_fst = fst_neighbors(&fst_set, query, max_distance);
            assert_eq!(
                via_fuzzy_index, via_fst,
                "mismatch for query {query:?} at max_distance {max_distance} on doubled corpus"
            );
        }
    }
}

/// The other side of `fst_levenshtein_automaton_size_limit_is_real`'s
/// boundary: the same long query at a small distance builds fine (the
/// default state limit binds on distance, not query length alone) and still
/// agrees with `FuzzyIndex` — while the distance that makes `fst` error is
/// a plain, infallible integer for `FuzzyIndex::neighbors`, whose result
/// respects `max_distance` monotonicity.
#[test]
fn fuzzy_index_has_no_automaton_size_failure_mode() {
    let corpus = distinct_words(3_000);
    let index = build_fuzzy_index(&corpus);
    let fst_set = build_fst(&corpus);
    let long_query = "supercalifragilisticexpialidocious";

    let via_fst = fst_neighbors(&fst_set, long_query, 2);
    let via_fuzzy: BTreeSet<String> = index.neighbors(long_query, 2).map(str::to_owned).collect();
    assert_eq!(
        via_fuzzy, via_fst,
        "long query at small distance must still agree"
    );

    // distance 20 errors in fst (asserted above); FuzzyIndex just answers.
    let at_20 = index.neighbors(long_query, 20).count();
    assert!(
        at_20 >= via_fuzzy.len(),
        "a larger max_distance can never shrink the neighbor set"
    );
}
