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

use std::hash::Hasher;

use rustc_hash::FxHasher;

/// The scalar sequence generation and lookup are keyed on.
pub(crate) fn to_scalars(word: &str) -> Vec<char> {
    word.chars().collect()
}

/// Hashes a scalar sequence, seeded with its length so that a sequence and its
/// own prefixes do not collide systematically.
///
/// This is what both deletion-keyed structures in the crate store *instead of*
/// the sequence — see [`for_each_deletion`]'s "Why this streams instead of
/// returning the set" for why keeping the sequence is not affordable.
pub(crate) fn hash_scalars(units: &[char]) -> u64 {
    let mut h = FxHasher::default();
    h.write_usize(units.len());
    for &c in units {
        h.write_u32(c as u32);
    }
    h.finish()
}

/// Calls `f` once with every sequence reachable from `units` by deleting up to
/// `max_distance` Unicode scalars, **including `units` itself** (deletion depth
/// 0).
///
/// The slice `f` receives is a scratch buffer this function reuses between
/// calls: it is valid for the duration of the call and no longer, and it is the
/// *same* buffer every time, so a caller that needs to keep a variant copies
/// it. None of the callers in this crate do — all four of them hash the variant
/// with [`hash_scalars`] and keep the hash.
///
/// May repeat a sequence when `units` repeats a scalar — deleting either `'a'`
/// of `"aab"` yields `"ab"` twice — which callers absorb themselves rather than
/// paying for a hash set here on every call.
///
/// # Why this streams instead of returning the set
///
/// The depth-2 deletion set of an `n`-scalar word is inherently cubic *as a
/// value*: it holds up to `n choose 2` sequences of `n - 2` scalars, so no
/// amount of care in producing it makes `Vec<Vec<char>>` smaller than `Θ(n³)`
/// bytes. Materialising it was therefore not merely wasteful but unbounded, and
/// on ordinary input: with a single long token in the corpus — a URL, a base64
/// blob, a mis-tokenised line — one near-distance query measured 1.0 GB of peak
/// RSS at 500 scalars, 4.0 GB at 800, and 63 GB at 2,000, which is an
/// allocation abort on any machine that cannot hold it.
///
/// The cubic factor is inherent to the *set*, but no caller in this crate wants
/// the set. Every one of them consumes a variant and forgets it: two hash a
/// variant to probe a map ([`DeletionIndex::neighbors`], [`Spellcheck`]'s
/// near-distance path) and two hash one to fill a map
/// ([`DeletionIndexBuilder::insert`], `Spellcheck::build_near_index`).
/// Streaming therefore holds one variant at a time — `O(n)` live bytes at any
/// instant, whatever the depth — while the number of variants, and so the
/// running time, is unchanged.
///
/// The same arithmetic is why neither map *stores* the sequence either. A map
/// keyed on the sequence pays the cubic bill once at build time and then keeps
/// paying it: `n choose 2` keys of `n - 2` scalars is the same `Θ(n³)` bytes,
/// retained for the life of the index rather than for the life of one call. So
/// both maps key on [`hash_scalars`] and verify the hit afterwards, which is
/// sound because a collision can only *add* a candidate that verification then
/// rejects. `tests::streaming_holds_one_variant_at_a_time` pins the call-time
/// half of that with a counting allocator;
/// `deletion_index::tests::an_index_of_one_long_word_stays_quadratic_in_its_length`
/// pins the retained half.
///
/// [`DeletionIndex::neighbors`]: crate::DeletionIndex::neighbors
/// [`DeletionIndexBuilder::insert`]: crate::DeletionIndexBuilder::insert
/// [`Spellcheck`]: crate::Spellcheck
///
/// # Why it enumerates position sets rather than a frontier
///
/// Deleting `d` scalars one after another is the same thing as removing a set
/// of `d` positions from the original: the intermediate sequences are not
/// results in their own right, they are only a path to one. Enumerating the
/// sets directly needs no frontier to hold the path, and it visits each
/// combination once instead of once per order the deletions could have been
/// applied in — `n choose 2` variants at depth 2 rather than `n(n - 1)`, so
/// half the work as well as none of the storage.
pub(crate) fn for_each_deletion(units: &[char], max_distance: u32, mut f: impl FnMut(&[char])) {
    f(units);

    let n = units.len();
    // Deleting more scalars than there are is the empty sequence, reached at
    // depth `n`; asking for a deeper deletion than the word is long is not an
    // error, it just has nothing further to remove.
    let deepest = (max_distance as usize).min(n);
    if deepest == 0 {
        return;
    }

    let mut variant: Vec<char> = Vec::with_capacity(n);
    let mut removed: Vec<usize> = Vec::with_capacity(deepest);
    for depth in 1..=deepest {
        removed.clear();
        removed.extend(0..depth);
        loop {
            variant.clear();
            let mut next = 0;
            for (position, &unit) in units.iter().enumerate() {
                if next < depth && removed[next] == position {
                    next += 1;
                } else {
                    variant.push(unit);
                }
            }
            f(&variant);
            if !advance(&mut removed, n) {
                break;
            }
        }
    }
}

/// Steps `removed` to the next combination of its size in lexicographic order,
/// reporting `false` when it was already the last.
///
/// The last combination is the one holding the largest positions available —
/// `removed[i] == i + n - removed.len()` for every `i` — so the step is: find
/// the rightmost position not yet at its ceiling, raise it, and repack
/// everything to its right immediately after it.
fn advance(removed: &mut [usize], n: usize) -> bool {
    let depth = removed.len();
    for i in (0..depth).rev() {
        if removed[i] < i + n - depth {
            removed[i] += 1;
            for j in i + 1..depth {
                removed[j] = removed[j - 1] + 1;
            }
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::counting_alloc::measure;
    use verbora_distance::damerau_levenshtein;

    /// The streamed variants, deduplicated and sorted.
    ///
    /// Test-only: no caller wants the whole set, which is exactly why
    /// [`for_each_deletion`] does not build one. Every assertion below is about
    /// *what* is generated, and the set is the readable way to say that.
    fn distinct_deletions(units: &[char], max_distance: u32) -> Vec<Vec<char>> {
        let mut out = Vec::new();
        for_each_deletion(units, max_distance, |variant| out.push(variant.to_vec()));
        out.sort_unstable();
        out.dedup();
        out
    }

    /// The deletion neighbourhood written from the definition — breadth-first,
    /// one scalar at a time — independently of the position-set enumeration
    /// under test. `tests/completeness.rs` uses the same shape for the same
    /// reason: an assertion is only worth as much as the definition it is
    /// compared against.
    fn by_definition(units: &[char], max_distance: u32) -> BTreeSet<Vec<char>> {
        let mut all: BTreeSet<Vec<char>> = BTreeSet::new();
        all.insert(units.to_vec());
        let mut frontier: BTreeSet<Vec<char>> = all.clone();
        for _ in 0..max_distance {
            let mut next = BTreeSet::new();
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

    #[test]
    fn depth_zero_is_the_word_itself() {
        assert_eq!(
            distinct_deletions(&to_scalars("cat"), 0),
            [to_scalars("cat")]
        );
    }

    /// Enumerating position sets must produce exactly the sequences successive
    /// single deletions do — that equivalence is the whole licence for not
    /// keeping a frontier.
    #[test]
    fn the_stream_is_the_deletion_neighbourhood_by_definition() {
        for word in [
            "",
            "a",
            "ab",
            "aab",
            "aaaa",
            "cat",
            "abcdef",
            "café",
            "😀ab",
            "Москва",
        ] {
            let units = to_scalars(word);
            for k in 0u32..=4 {
                let streamed: BTreeSet<Vec<char>> =
                    distinct_deletions(&units, k).into_iter().collect();
                assert_eq!(streamed, by_definition(&units, k), "word {word:?} at k={k}");
            }
        }
    }

    /// One emission per position set — `sum of n choose d` for `d` up to the
    /// depth — so nothing is generated twice for having been reachable by two
    /// orders of deletion.
    #[test]
    fn each_position_set_is_emitted_exactly_once() {
        let units = to_scalars("abcdefgh");
        let n = units.len() as u64;
        let mut emitted = 0u64;
        for_each_deletion(&units, 2, |_| emitted += 1);
        assert_eq!(emitted, 1 + n + n * (n - 1) / 2);

        // Beyond the word's length there is nothing further to remove, so the
        // count stops growing rather than the generator spinning.
        let mut deep = 0u64;
        for_each_deletion(&to_scalars("ab"), 9, |_| deep += 1);
        assert_eq!(deep, 4, "\"ab\", \"b\", \"a\", \"\"");
    }

    /// The property that makes a long token affordable at all: generating a
    /// quadratic number of variants costs a *linear* amount of live memory,
    /// because there is one buffer and it is reused.
    ///
    /// Measured with [`measure`] rather than inferred from what the callback
    /// sees, because what the callback sees cannot distinguish the two designs:
    /// an implementation that collected every variant into a `Vec<Vec<char>>`
    /// and then replayed the collection through `f` would emit the same slices,
    /// of the same widths, in the same order, and satisfy every assertion about
    /// them — while allocating cubically. That implementation is what this test
    /// exists to fail against, and the allocator is what lets it.
    #[test]
    fn streaming_holds_one_variant_at_a_time() {
        let units = to_scalars(&"abcdefghij".repeat(12));
        let n = units.len();
        let mut widest = 0;
        let mut emitted = 0u64;
        // One buffer, reused, is the documented contract, so the callback can
        // check it directly: count how often the address it is handed changes.
        // A materialising generator hands out a different allocation each time.
        let mut buffers = 0u32;
        let mut previous: *const char = std::ptr::null();

        let (bytes, ()) = measure(|| {
            for_each_deletion(&units, 2, |variant| {
                widest = widest.max(variant.len());
                emitted += 1;
                if !std::ptr::eq(variant.as_ptr(), previous) {
                    buffers += 1;
                    previous = variant.as_ptr();
                }
            });
        });

        assert_eq!(widest, n, "no variant is wider than the word");
        assert_eq!(emitted, 1 + n as u64 + (n * (n - 1) / 2) as u64);

        // Two addresses over the whole run: `units` itself at depth 0, then the
        // single scratch buffer for every one of the 7,260 deeper variants.
        assert_eq!(
            buffers, 2,
            "the scratch buffer must be reused, not reallocated"
        );

        // Linear in the word, not in the number of variants. Measured at these
        // 120 scalars: 496 bytes live at the peak — the 480-byte variant buffer
        // plus the two-element position set — against 3,624,784 bytes for a
        // probe that materialised the set first. The budget sits between them
        // with two orders of magnitude of clearance on each side.
        let budget = 64 * n * size_of::<char>();
        assert!(
            bytes.peak < budget,
            "peak live bytes {} exceeded the linear budget {budget} for a {n}-scalar word",
            bytes.peak
        );
        assert_eq!(bytes.retained, 0, "the generator must keep nothing");
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
