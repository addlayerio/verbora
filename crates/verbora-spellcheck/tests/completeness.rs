//! The lemma every symmetric-delete structure in this crate rests on, verified
//! by exhaustion rather than by assertion.
//!
//! > If `damerau_levenshtein(a, b) <= k`, then deleting at most `k` Unicode
//! > scalars from `a` and at most `k` from `b` can produce the **same**
//! > sequence.
//!
//! `src/deletions.rs` gives the per-operation argument — each of the four edit
//! operations costs at most one deletion on each side — but that argument
//! decomposes the edit script into disjoint alignment regions, and unrestricted
//! Damerau–Levenshtein is precisely the metric that may edit one substring
//! twice, so the decomposition is not automatic. This file closes the gap the
//! only way that does not require trusting a proof sketch: it enumerates
//! **every** pair of strings over two complete small universes and checks the
//! lemma on every pair the metric puts within `k`, for `k = 1, 2, 3`.
//!
//! The deletion generator here is written independently of the crate's own —
//! that is the point. If `src/deletions.rs` ever drifts (a different unit, an
//! off-by-one in the depth, an early exit that fires too soon), this file is
//! comparing against a definition, not against the code under test.
//!
//! If a counterexample is ever found, it is not a test to weaken: it is a
//! statement that [`DeletionIndex`](verbora_spellcheck::DeletionIndex) and
//! `Spellcheck`'s near-distance path can miss a match, and the retrieval depth
//! or the metric has to change.

use std::collections::HashSet;

use verbora_distance::damerau_levenshtein;

/// Every sequence reachable from `word` by deleting at most `depth` scalars,
/// including `word` itself. Written from the definition, breadth-first.
fn deletion_neighbourhood(word: &str, depth: u32) -> HashSet<Vec<char>> {
    let start: Vec<char> = word.chars().collect();
    let mut all: HashSet<Vec<char>> = HashSet::new();
    all.insert(start.clone());
    let mut frontier: HashSet<Vec<char>> = HashSet::new();
    frontier.insert(start);
    for _ in 0..depth {
        let mut next = HashSet::new();
        for s in &frontier {
            for i in 0..s.len() {
                let mut variant = s.clone();
                variant.remove(i);
                next.insert(variant);
            }
        }
        if next.is_empty() {
            break;
        }
        all.extend(next.iter().cloned());
        frontier = next;
    }
    all
}

/// Every string over `alphabet` of length at most `max_len`.
fn universe(alphabet: &[char], max_len: usize) -> Vec<String> {
    let mut out = vec![String::new()];
    let mut level = vec![String::new()];
    for _ in 0..max_len {
        let mut next = Vec::new();
        for s in &level {
            for &c in alphabet {
                let mut t = s.clone();
                t.push(c);
                out.push(t.clone());
                next.push(t);
            }
        }
        level = next;
    }
    out
}

/// Checks the lemma on every ordered pair of a complete universe.
fn assert_lemma_holds(alphabet: &[char], max_len: usize, max_k: u32) {
    let words = universe(alphabet, max_len);
    let neighbourhoods: Vec<Vec<HashSet<Vec<char>>>> = words
        .iter()
        .map(|w| (0..=max_k).map(|k| deletion_neighbourhood(w, k)).collect())
        .collect();

    let mut within = [0usize; 4];
    for (i, a) in words.iter().enumerate() {
        for (j, b) in words.iter().enumerate() {
            let d = damerau_levenshtein(a, b) as u32;
            if d > max_k {
                continue;
            }
            for k in d..=max_k {
                within[k as usize] += 1;
                let shared = neighbourhoods[i][k as usize]
                    .intersection(&neighbourhoods[j][k as usize])
                    .next()
                    .is_some();
                assert!(
                    shared,
                    "counterexample at k={k}: {a:?} and {b:?} are {d} edits apart \
                     but share no deletion sequence at depth {k}"
                );
            }
        }
    }

    for (k, &pairs) in within.iter().enumerate().take(max_k as usize + 1).skip(1) {
        assert!(
            pairs > 1_000,
            "k={k} checked only {pairs} pairs — the universe is too small to be evidence"
        );
    }
}

/// Every string of length 0–5 over a three-scalar alphabet: 364 words, 132,496
/// ordered pairs. Long enough for two edits to overlap in every way they can at
/// that length, which is the shape the per-operation argument does not cover.
#[test]
fn the_lemma_holds_over_a_complete_three_scalar_universe() {
    assert_lemma_holds(&['a', 'b', 'c'], 5, 3);
}

/// The same over a four-scalar alphabet at length 0–4: 341 words. A wider
/// alphabet makes substitutions between *distinct* scalars common, where the
/// narrower one repeats letters and hides them behind equal-character
/// coincidences.
#[test]
fn the_lemma_holds_over_a_complete_four_scalar_universe() {
    assert_lemma_holds(&['a', 'b', 'c', 'd'], 4, 3);
}

/// The same over an alphabet that is entirely non-ASCII, including astral
/// scalars from different planes. The lemma is about scalars, so it must not
/// depend on how wide their UTF-8 or UTF-16 encodings happen to be.
#[test]
fn the_lemma_holds_over_a_complete_astral_universe() {
    assert_lemma_holds(&['😀', '𝕳', '𐐷'], 5, 3);
}

/// The transposition row of the lemma's table, isolated: it is the one edit
/// operation plain Levenshtein does not have, and the reason a Levenshtein-only
/// completeness argument would not carry over.
#[test]
fn a_transposition_needs_one_deletion_on_each_side() {
    for (a, b) in [
        ("ab", "ba"),
        ("xaby", "xbay"),
        ("😀𝕳", "𝕳😀"),
        ("hte", "the"),
    ] {
        assert_eq!(damerau_levenshtein(a, b), 1, "{a:?} vs {b:?}");
        let left = deletion_neighbourhood(a, 1);
        let right = deletion_neighbourhood(b, 1);
        assert!(
            left.intersection(&right).next().is_some(),
            "{a:?} and {b:?} are one transposition apart but share no depth-1 deletion"
        );
    }
}
