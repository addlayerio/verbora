//! Deletion-neighbourhood generation, shared by [`DeletionIndex`] and
//! [`Spellcheck`]'s own near-distance retrieval.
//!
//! [`DeletionIndex`]: crate::DeletionIndex
//! [`Spellcheck`]: crate::Spellcheck
//!
//! # The lemma this module exists to satisfy
//!
//! Symmetric-delete retrieval rests on one claim:
//!
//! > If `distance(a, b) <= k`, then deleting at most `k` units from `a` and at
//! > most `k` units from `b` can produce the *same* sequence.
//!
//! For the crate's metric — unrestricted Damerau–Levenshtein over Unicode
//! scalars ([`verbora_distance::damerau_levenshtein`], see
//! `docs/design/distance-contract.md`) — each of the four operations costs at
//! most one deletion on each side and leaves both sides equal at that position:
//!
//! | operation | delete from `a` | delete from `b` | both become |
//! |---|---|---|---|
//! | insertion (`b` has an extra scalar) | — | the inserted scalar | the shared text |
//! | deletion (`a` has an extra scalar) | the surplus scalar | — | the shared text |
//! | substitution (`x` ↔ `y`) | `x` | `y` | the surrounding text |
//! | transposition (`xy` ↔ `yx`) | `x` | `x` | `…y…` |
//!
//! So `k` operations need at most `k` deletions per side. The one case that
//! argument does not cover by itself is unrestricted Damerau–Levenshtein's
//! ability to edit the same substring twice — where the operations no longer
//! decompose into disjoint alignment regions. `tests/completeness.rs` closes
//! that gap by exhaustion rather than by assertion: it enumerates *every* pair
//! of strings over two complete small universes and checks the lemma for
//! `k = 1, 2, 3` on every pair the metric puts within `k`.
//!
//! # Why the unit is the Unicode scalar
//!
//! Generation and verification must agree on what an atomic unit is, or
//! retrieval silently returns fewer matches with nothing failing to compile.
//! [`verbora_distance::damerau_levenshtein`] counts Unicode scalars, so
//! generation counts Unicode scalars. Both directions of the mismatch are real
//! and this crate has seen both: generating by scalar against a UTF-16 metric
//! under-generates because one scalar is two code units there, and generating
//! by UTF-16 code unit against a scalar metric under-generates in the opposite
//! direction, because an astral scalar differing by one costs two code-unit
//! deletions.

/// The scalar sequence generation and lookup are keyed on.
pub(crate) fn to_scalars(word: &str) -> Vec<char> {
    word.chars().collect()
}

/// Every sequence reachable from `units` by deleting up to `max_distance`
/// Unicode scalars, **including `units` itself** (deletion depth 0).
///
/// May contain duplicates — different deleted positions can produce the same
/// result, e.g. deleting either `'a'` in `"aab"` — which callers dedupe
/// themselves rather than paying for a hash set here on every call.
fn deletions(units: &[char], max_distance: u32) -> Vec<Vec<char>> {
    let mut out = vec![units.to_vec()];
    let mut frontier: Vec<Vec<char>> = vec![units.to_vec()];
    for _ in 0..max_distance {
        let mut next = Vec::new();
        for s in &frontier {
            for i in 0..s.len() {
                let mut variant = s.clone();
                variant.remove(i);
                out.push(variant.clone());
                next.push(variant);
            }
        }
        if next.is_empty() {
            // Every unit has already been deleted (a word shorter than
            // `max_distance`) — nothing left to delete further.
            break;
        }
        frontier = next;
    }
    out
}

/// Deduplicated [`deletions`], sorted — the form both build and query time need
/// before touching a shared map.
pub(crate) fn distinct_deletions(units: &[char], max_distance: u32) -> Vec<Vec<char>> {
    let mut out = deletions(units, max_distance);
    out.sort_unstable();
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use verbora_distance::damerau_levenshtein;

    #[test]
    fn depth_zero_is_the_word_itself() {
        assert_eq!(deletions(&to_scalars("cat"), 0), [to_scalars("cat")]);
    }

    #[test]
    fn depth_one_is_every_single_unit_removal() {
        let mut expected: Vec<Vec<char>> = ["cat", "at", "ct", "ca"]
            .iter()
            .map(|s| to_scalars(s))
            .collect();
        expected.sort_unstable();
        assert_eq!(distinct_deletions(&to_scalars("cat"), 1), expected);
    }

    #[test]
    fn generation_stops_once_the_word_is_exhausted() {
        // "ab" has only 2 units; asking for depth 5 must not panic or loop
        // forever removing from an empty sequence.
        let mut expected: Vec<Vec<char>> =
            ["ab", "a", "b", ""].iter().map(|s| to_scalars(s)).collect();
        expected.sort_unstable();
        assert_eq!(distinct_deletions(&to_scalars("ab"), 5), expected);
    }

    #[test]
    fn the_unit_is_the_scalar_the_metric_counts() {
        // "😀" is **one** unit: deleting one unit from it yields the empty
        // sequence, not a lone surrogate half.
        let units = to_scalars("😀");
        assert_eq!(units.len(), 1);
        let mut expected = vec![units.clone(), Vec::new()];
        expected.sort_unstable();
        assert_eq!(distinct_deletions(&units, 1), expected);

        // A mixed word: "a😀b" is three units, so depth 1 removes each of the
        // three whole scalars.
        let units = to_scalars("a😀b");
        assert_eq!(units.len(), 3);
        let mut expected: Vec<Vec<char>> = ["a😀b", "😀b", "ab", "a😀"]
            .iter()
            .map(|s| to_scalars(s))
            .collect();
        expected.sort_unstable();
        assert_eq!(distinct_deletions(&units, 1), expected);

        // The depth of the connection retrieval relies on: two words one
        // astral substitution apart share a depth-1 deletion, which a UTF-16
        // generator would have missed under a scalar metric.
        assert_eq!(damerau_levenshtein("😀x", "😁x"), 1);
        let left: std::collections::BTreeSet<Vec<char>> = distinct_deletions(&to_scalars("😀x"), 1)
            .into_iter()
            .collect();
        let right: std::collections::BTreeSet<Vec<char>> =
            distinct_deletions(&to_scalars("😁x"), 1)
                .into_iter()
                .collect();
        assert!(
            left.intersection(&right).next().is_some(),
            "a distance-1 pair must share a depth-1 deletion"
        );
    }

    #[test]
    fn a_transposition_is_one_deletion_on_each_side() {
        // The row of the lemma's table that Levenshtein alone does not cover.
        assert_eq!(damerau_levenshtein("ab", "ba"), 1);
        let left: std::collections::BTreeSet<Vec<char>> = distinct_deletions(&to_scalars("ab"), 1)
            .into_iter()
            .collect();
        let right: std::collections::BTreeSet<Vec<char>> = distinct_deletions(&to_scalars("ba"), 1)
            .into_iter()
            .collect();
        assert!(left.intersection(&right).next().is_some());
    }
}
