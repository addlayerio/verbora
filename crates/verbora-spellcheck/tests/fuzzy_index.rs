//! Correctness of [`FuzzyIndex`] against a brute-force baseline.
//!
//! A BK-tree's pruning is a performance optimization, not a filter — it
//! must return exactly the same set of matches a linear scan computing
//! real Levenshtein distance against every dictionary word would. This is
//! the property that actually defines correctness here (matching this
//! workspace's own "verify against ground truth, not just a few hand-
//! picked examples" discipline for anything without a reference oracle to
//! replay), so it's checked directly rather than asserted through a
//! handful of individually-chosen cases.

use std::path::Path;

use verbora_distance::levenshtein;
use verbora_spellcheck::FuzzyIndexBuilder;

/// The same shared word list this workspace's other crates' benchmarks
/// use (`benches/data/words.json` at the workspace root), so this test
/// exercises the index against realistic, not hand-picked, strings.
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
    // `FuzzyIndex` indexes a *set* of distinct strings (see its own doc
    // comment) -- dedupe the raw word list the same way before computing
    // the brute-force baseline, so both sides are answering the same
    // question. The source list does contain real duplicates (it's
    // randomly generated), so skipping this would fail the comparison for
    // a reason that has nothing to do with the index's own correctness.
    let sample: Vec<String> = {
        let mut seen = std::collections::HashSet::new();
        dictionary
            .into_iter()
            .take(3_000)
            .filter(|w| seen.insert(w.clone()))
            .collect()
    };

    let mut builder = FuzzyIndexBuilder::new();
    for word in &sample {
        builder.insert(word);
    }
    let index = builder.build();
    assert_eq!(index.len(), sample.len());

    // A mix of: words genuinely in the dictionary, one-character
    // perturbations of dictionary words (the realistic "typo" shape), and
    // a handful of words definitely not present.
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

#[test]
fn empty_index_yields_no_neighbors() {
    let index = FuzzyIndexBuilder::new().build();
    assert!(index.is_empty());
    assert_eq!(index.neighbors("anything", 5).count(), 0);
}

#[test]
fn duplicate_inserts_collapse_to_one_entry() {
    let mut builder = FuzzyIndexBuilder::new();
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
