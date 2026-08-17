//! Correctness of [`DeletionIndex`] against a brute-force baseline, and
//! against [`FuzzyIndex`] itself where both can answer the same question.
//!
//! Same discipline as `tests/fuzzy_index.rs`: a deletion index's
//! over-generate-then-verify shape is a performance optimization, not a
//! filter, so it must return exactly the same set of matches a linear scan
//! computing real Levenshtein distance against every dictionary word would
//! — checked directly, not asserted through a handful of hand-picked cases.
//!
//! This file additionally stresses **astral (non-BMP) characters**
//! specifically: `crates/verbora-spellcheck/src/deletion_index.rs`'s own
//! doc comment explains why deletion generation must operate on UTF-16 code
//! units rather than `char`s (matching `verbora_distance::levenshtein`'s own
//! granularity) — a real, found-during-implementation risk class the
//! shared `benches/data/words.json` corpus (lowercase ASCII only) can never
//! exercise, so it needs its own coverage here rather than being assumed
//! safe by extension from the ASCII-only comparison.

use std::path::Path;

use verbora_distance::levenshtein;
use verbora_spellcheck::{DeletionIndexBuilder, FuzzyIndexBuilder};

fn words() -> Vec<String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crate is two levels below the workspace root")
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
        .expect("words array")
        .iter()
        .map(|w| w.as_str().expect("word is a string").to_owned())
        .collect()
}

fn brute_force_neighbors<'a>(
    dictionary: &'a [String],
    query: &str,
    max_distance: u32,
) -> Vec<&'a str> {
    dictionary
        .iter()
        .map(String::as_str)
        .filter(|w| (levenshtein(query, w, &Default::default()).round() as u32) <= max_distance)
        .collect()
}

#[test]
fn matches_brute_force_across_many_queries_and_distances() {
    let dictionary = words();
    let sample: Vec<String> = {
        let mut seen = std::collections::HashSet::new();
        dictionary
            .into_iter()
            .take(3_000)
            .filter(|w| seen.insert(w.clone()))
            .collect()
    };

    // Built to a cap of 3, matching `tests/fuzzy_index.rs`'s own tested
    // distance range, so both files' correctness claims cover the same
    // ground.
    let mut builder = DeletionIndexBuilder::new(3);
    for word in &sample {
        builder.insert(word);
    }
    let index = builder.build();
    assert_eq!(index.len(), sample.len());

    let mut queries: Vec<String> = sample.iter().take(30).cloned().collect();
    for w in sample.iter().skip(30).take(30) {
        let mut perturbed = w.clone();
        if !perturbed.is_empty() {
            perturbed.push('x');
        }
        queries.push(perturbed);
    }
    queries.push("zzzzzzzzzzzzzzzzzzzz".to_owned());
    queries.push(String::new());

    for query in &queries {
        for max_distance in [0, 1, 2, 3] {
            let mut expected = brute_force_neighbors(&sample, query, max_distance);
            let mut actual: Vec<&str> = index.neighbors(query, max_distance).collect();
            expected.sort_unstable();
            actual.sort_unstable();
            assert_eq!(
                actual, expected,
                "mismatch for query {query:?} at max_distance {max_distance}"
            );
        }
    }
}

/// The risk class `src/deletion_index.rs`'s own doc comment identifies:
/// astral (non-BMP) characters, where one `char` is two UTF-16 code units.
/// A dictionary and query set built entirely from such characters, checked
/// against the same brute-force baseline the ASCII test above uses.
#[test]
fn matches_brute_force_on_astral_heavy_input() {
    // A mix of astral emoji, astral mathematical alphanumerics, and BMP
    // text combined with both, so words vary in whether -- and where --
    // they contain a surrogate pair.
    let dictionary: Vec<String> = vec![
        "😀".to_owned(),
        "😀x".to_owned(),
        "x😀".to_owned(),
        "😀abc".to_owned(),
        "🙁abc".to_owned(),
        "😀😀".to_owned(),
        "😀😁".to_owned(),
        "𝕳𝖊𝖑𝖑𝖔".to_owned(),
        "𝕳𝖊𝖑𝖑𝖈".to_owned(),
        "hello".to_owned(),
        "hell😀".to_owned(),
        "a👍b".to_owned(),
        "a👍bc".to_owned(),
        "a👌bc".to_owned(),
        "".to_owned(),
    ];

    let mut builder = DeletionIndexBuilder::new(3);
    for word in &dictionary {
        builder.insert(word);
    }
    let index = builder.build();
    assert_eq!(index.len(), dictionary.len());

    let queries = dictionary.clone();
    for query in &queries {
        for max_distance in [0, 1, 2, 3] {
            let mut expected = brute_force_neighbors(&dictionary, query, max_distance);
            let mut actual: Vec<&str> = index.neighbors(query, max_distance).collect();
            expected.sort_unstable();
            actual.sort_unstable();
            assert_eq!(
                actual, expected,
                "astral mismatch for query {query:?} at max_distance {max_distance}"
            );
        }
    }
}

/// `DeletionIndex` and `FuzzyIndex` answer the exact same question
/// (candidate words within `k` edits) via entirely different mechanisms —
/// within `DeletionIndex`'s own build-time cap, they must agree with each
/// other too, not just with the brute-force baseline independently.
#[test]
fn agrees_with_fuzzy_index_within_the_deletion_indexs_cap() {
    let dictionary = words();
    let sample: Vec<String> = {
        let mut seen = std::collections::HashSet::new();
        dictionary
            .into_iter()
            .take(1_000)
            .filter(|w| seen.insert(w.clone()))
            .collect()
    };

    let mut deletion_builder = DeletionIndexBuilder::new(2);
    let mut fuzzy_builder = FuzzyIndexBuilder::new();
    for word in &sample {
        deletion_builder.insert(word);
        fuzzy_builder.insert(word);
    }
    let deletion_index = deletion_builder.build();
    let fuzzy_index = fuzzy_builder.build();

    for query in sample.iter().take(50) {
        for max_distance in [0, 1, 2] {
            let mut from_deletion: Vec<&str> =
                deletion_index.neighbors(query, max_distance).collect();
            let mut from_fuzzy: Vec<&str> = fuzzy_index.neighbors(query, max_distance).collect();
            from_deletion.sort_unstable();
            from_fuzzy.sort_unstable();
            assert_eq!(
                from_deletion, from_fuzzy,
                "DeletionIndex and FuzzyIndex disagree for query {query:?} at max_distance {max_distance}"
            );
        }
    }
}

#[test]
fn empty_index_yields_no_neighbors() {
    let index = DeletionIndexBuilder::new(2).build();
    assert!(index.is_empty());
    assert_eq!(index.neighbors("anything", 2).count(), 0);
}

#[test]
fn duplicate_inserts_collapse_to_one_entry() {
    let mut builder = DeletionIndexBuilder::new(2);
    builder.insert("hello");
    builder.insert("hello");
    builder.insert("hello");
    let index = builder.build();
    assert_eq!(index.len(), 1);
    assert_eq!(
        index.neighbors("hello", 0).collect::<Vec<_>>(),
        vec!["hello"]
    );
}
