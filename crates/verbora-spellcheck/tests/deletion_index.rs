//! Correctness of [`DeletionIndex`] against a brute-force baseline, and
//! against [`FuzzyIndex`] itself where both can answer the same question.
//!
//! Same discipline as `tests/fuzzy_index.rs`: a deletion index's
//! over-generate-then-verify shape is a performance optimization, not a
//! filter, so it must return exactly the same set of matches a linear scan
//! computing the crate's metric against every dictionary word would — checked
//! directly, not asserted through a handful of hand-picked cases.
//!
//! This file additionally stresses **astral (non-BMP) characters**
//! specifically: `crates/verbora-spellcheck/src/deletion_index.rs`'s own
//! doc comment explains why deletion generation must operate on the same
//! unit `verbora_distance::damerau_levenshtein` counts in — one Unicode
//! scalar, per `docs/design/distance-contract.md` §2 — rather than on UTF-16
//! code units. Generation and verification drifting apart is a *silent* failure:
//! `neighbors()` returns fewer matches and nothing fails to compile, so the
//! only thing standing between that bug and a release is this file. The
//! shared `benches/data/words.json` corpus (lowercase ASCII only) can never
//! exercise it, because on ASCII byte = scalar = UTF-16 unit.
//!
//! Three independent tripwires cover it, deliberately, because a single
//! fixed corpus is easy to weaken by accident:
//!
//! * `matches_brute_force_on_astral_heavy_input` — a hand-written corpus,
//!   every query against every distance.
//! * `astral_pairs_differing_in_both_surrogate_halves_are_found` — the
//!   *narrow* case, isolated: two one-character words whose UTF-16
//!   encodings share no code unit at all. Under a UTF-16 generator these sit
//!   at generation-distance 2 and verification-distance 1, so they are the
//!   minimal witness of the drift.
//! * `matches_brute_force_on_randomized_astral_corpora` — randomized, so a
//!   corpus that happens to share high surrogates (which is exactly what
//!   makes the two units agree) cannot hide the bug.

use std::path::Path;

use verbora_distance::damerau_levenshtein;
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
        .filter(|w| damerau_levenshtein(query, w) <= max_distance as usize)
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
            let mut actual: Vec<&str> = index
                .neighbors(query, max_distance)
                .expect("within the build-time ceiling")
                .map(|n| n.word)
                .collect();
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
/// astral (non-BMP) characters, where one scalar is two UTF-16 code units.
/// A dictionary and query set built largely from such characters, checked
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
        // Astral characters from *different* high-surrogate blocks, so a
        // one-scalar substitution is a two-code-unit substitution. The
        // corpus above happens to draw its emoji from one block (U+1F4xx /
        // U+1F6xx, high surrogate D83D) and its mathematical letters from
        // another (U+1D5xx, high surrogate D835); within a block the two
        // units agree, which is precisely the coincidence these rows
        // remove.
        "𝕳".to_owned(),
        "𝖊".to_owned(),
        "😀𝕳".to_owned(),
        "𝕳😀".to_owned(),
        "𝔸".to_owned(), // U+1D538, another D835 word
        "🜁".to_owned(), // U+1F701, high surrogate D83D
        "𐐷".to_owned(), // U+10437, high surrogate D801 — a third block
        "𐎠".to_owned(), // U+103A0, also D800-block but a different low half
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
            let mut actual: Vec<&str> = index
                .neighbors(query, max_distance)
                .expect("within the build-time ceiling")
                .map(|n| n.word)
                .collect();
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
            let mut from_deletion: Vec<&str> = deletion_index
                .neighbors(query, max_distance)
                .expect("within the build-time ceiling")
                .map(|n| n.word)
                .collect();
            let mut from_fuzzy: Vec<&str> = fuzzy_index
                .neighbors(query, max_distance)
                .map(|n| n.word)
                .collect();
            from_deletion.sort_unstable();
            from_fuzzy.sort_unstable();
            assert_eq!(
                from_deletion, from_fuzzy,
                "DeletionIndex and FuzzyIndex disagree for query {query:?} at max_distance {max_distance}"
            );
        }
    }
}

/// The minimal witness that generation and verification share a unit.
///
/// `"😀"` (U+1F600) encodes to `D83D DE00` and `"𝕳"` (U+1D573) to
/// `D835 DD73`: **no** code unit in common. Their scalar distance is 1 — one
/// substitution — so a `max_distance` of 1 must find each from the other.
///
/// A UTF-16-granularity generator cannot: their UTF-16 distance is 2, their
/// longest common code-unit subsequence is empty, and connecting them
/// requires deleting both units from each side. The failure is silent, which
/// is why this is asserted on its own rather than left inside the larger
/// corpus comparison — an isolated two-word index has nowhere for the bug to
/// hide.
#[test]
fn astral_pairs_differing_in_both_surrogate_halves_are_found() {
    for (a, b) in [
        ("😀", "𝕳"),
        ("😀", "𐐷"),
        ("𝕳", "𐐷"),
        ("x😀y", "x𝕳y"),
        ("😀ab", "𐐷ab"),
    ] {
        assert_eq!(
            damerau_levenshtein(a, b),
            usize::from(a != b),
            "{a:?} and {b:?} must differ by one scalar substitution"
        );
        let mut builder = DeletionIndexBuilder::new(1);
        builder.insert(a);
        builder.insert(b);
        let index = builder.build();

        for (query, other) in [(a, b), (b, a)] {
            let found: Vec<&str> = index
                .neighbors(query, 1)
                .expect("within the build-time ceiling")
                .map(|n| n.word)
                .collect();
            assert!(
                found.contains(&other),
                "querying {query:?} at distance 1 missed {other:?}: got {found:?}"
            );
        }
    }
}

/// The same completeness property over *randomized* astral corpora.
///
/// The fixed corpora above can be defeated by a coincidence — draw every
/// astral character from one high-surrogate block and the UTF-16 unit and
/// the scalar agree on every distance, so a UTF-16 generator passes. Drawing
/// from several blocks at random removes that coincidence, and re-checking
/// against the brute force makes the assertion completeness, not similarity.
#[test]
fn matches_brute_force_on_randomized_astral_corpora() {
    // A deterministic PRNG, so a failure reproduces exactly.
    struct SplitMix64(u64);
    impl SplitMix64 {
        fn next_u64(&mut self) -> u64 {
            self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = self.0;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z ^ (z >> 31)
        }
        fn next_range(&mut self, bound: usize) -> usize {
            (self.next_u64() % bound as u64) as usize
        }
    }

    // Scalars from four different astral planes/blocks plus two BMP ones,
    // so consecutive draws routinely disagree in the high surrogate.
    const ALPHABET: &[char] = &[
        '\u{1F600}', // D83D DE00
        '\u{1F601}', // D83D DE01
        '\u{1D573}', // D835 DD73
        '\u{1D538}', // D835 DD38
        '\u{10437}', // D801 DC37
        '\u{103A0}', // D800 DFA0
        '\u{2070E}', // D841 DF0E
        'a',
        'é',
    ];

    let mut rng = SplitMix64(0x5EA2_D317_2026_0819);
    for round in 0..40 {
        let dictionary: Vec<String> = (0..14)
            .map(|_| {
                let len = rng.next_range(5);
                (0..len)
                    .map(|_| ALPHABET[rng.next_range(ALPHABET.len())])
                    .collect()
            })
            .collect();
        let mut builder = DeletionIndexBuilder::new(2);
        for word in &dictionary {
            builder.insert(word);
        }
        let index = builder.build();

        // Query with the dictionary words themselves and with fresh words,
        // so both "the query is indexed" and "the query is not" are covered.
        let mut queries = dictionary.clone();
        for _ in 0..6 {
            let len = rng.next_range(5);
            queries.push(
                (0..len)
                    .map(|_| ALPHABET[rng.next_range(ALPHABET.len())])
                    .collect(),
            );
        }

        for query in &queries {
            for max_distance in [0, 1, 2] {
                let mut expected = brute_force_neighbors(&dictionary, query, max_distance);
                let mut actual: Vec<&str> = index
                    .neighbors(query, max_distance)
                    .expect("within the build-time ceiling")
                    .map(|n| n.word)
                    .collect();
                expected.sort_unstable();
                expected.dedup();
                actual.sort_unstable();
                actual.dedup();
                assert_eq!(
                    actual, expected,
                    "round {round}: mismatch for query {query:?} at max_distance {max_distance}"
                );
            }
        }
    }
}

#[test]
fn empty_index_yields_no_neighbors() {
    let index = DeletionIndexBuilder::new(2).build();
    assert!(index.is_empty());
    assert_eq!(index.neighbors("anything", 2).unwrap().count(), 0);
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
        index
            .neighbors("hello", 0)
            .unwrap()
            .map(|n| n.word)
            .collect::<Vec<_>>(),
        vec!["hello"]
    );
}
