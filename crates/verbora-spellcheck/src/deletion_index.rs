//! A symmetric-delete index: build once with a **fixed maximum edit
//! distance**, query many — the second fuzzy-matching structure this crate
//! offers, alongside [`FuzzyIndex`]'s BK-tree, not a replacement for it.
//!
//! [`FuzzyIndex`]: crate::FuzzyIndex
//!
//! # The trade-off, stated up front
//!
//! A deletion index trades away exactly what a BK-tree offers for free:
//! `max_distance` must be fixed when the index is **built**, not chosen per
//! query. [`DeletionIndex::neighbors`] still takes a `max_distance` argument,
//! and rejects — with [`DistanceBeyondIndex`] — any value the index was not
//! built to answer. It does **not** silently narrow the question to one it can
//! answer: a query for distance 3 against a ceiling of 2 is a caller mistake,
//! and returning the distance-2 answer as though it were the distance-3 answer
//! would hide it.
//!
//! In exchange, construction is cheaper for a **fixed, small** `max_distance`
//! (candidate generation is O(word length choose distance) per word, not the
//! tree-shape-dependent cost a BK-tree insert pays), and query cost does not
//! grow with the size of the result the way generate-every-edit retrieval does.
//!
//! # Algorithm
//!
//! For every indexed word, generate every sequence reachable by deleting up to
//! `max_distance` Unicode scalars — including the word itself, at deletion
//! depth 0 — and record which word(s) each deletion sequence came from. A query
//! generates its *own* deletions up to its own distance and looks each one up;
//! every word found this way is a **candidate**, not a confirmed match —
//! deletion collisions are real (`"cats"` and `"cars"` both delete to `"cas"`)
//! — so every candidate's exact distance is computed with
//! [`verbora_distance::damerau_levenshtein`] before it is returned. See
//! `crate::deletions` for the completeness lemma that makes retrieval
//! exhaustive, and `tests/completeness.rs` for its exhaustive verification.
//!
//! # Metric and unit
//!
//! The whole crate uses one metric and one unit:
//! [`verbora_distance::damerau_levenshtein`] — unrestricted
//! Damerau–Levenshtein, unit cost, counted in **Unicode scalar values**
//! (`docs/design/distance-contract.md` §2). Generation uses that same unit,
//! because a generator and a verifier that disagree about what one unit is
//! produce an index that silently returns fewer matches with nothing failing to
//! compile.
//!
//! A consequence of the scalar unit worth naming: a deletion sequence is always
//! well-formed text, since deleting whole scalars can never split a surrogate
//! pair. Sequences are nonetheless kept as hashes of `[char]` rather than as
//! strings — there is never a need to render one back to text, only to compare
//! it.
//!
//! # Build → Freeze → Query
//!
//! [`DeletionIndexBuilder::insert`] as many times as you like, then
//! [`DeletionIndexBuilder::build`] freezes into an immutable, `Send + Sync`
//! [`DeletionIndex`].

use std::fmt;

use rustc_hash::FxHashMap;
use verbora_distance::damerau_levenshtein;

use crate::deletions::{distinct_deletions, to_scalars};
use crate::neighbor::Neighbor;

/// Returned by [`DeletionIndex::neighbors`] when the requested distance is
/// larger than the index was built to answer.
///
/// This is not a soft limit that could be lifted by trying harder at query
/// time: deletion sequences past the build-time depth were never generated for
/// any indexed word, so there is nothing for a deeper query to find. Rebuild
/// with a larger [`DeletionIndexBuilder::new`] ceiling, or use
/// [`FuzzyIndex`](crate::FuzzyIndex), whose `max_distance` is a query-time
/// parameter.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DistanceBeyondIndex {
    /// The distance the caller asked for.
    pub requested: u32,
    /// The largest distance this index can answer.
    pub ceiling: u32,
}

impl fmt::Display for DistanceBeyondIndex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "requested edit distance {} exceeds this deletion index's build-time ceiling of {}",
            self.requested, self.ceiling
        )
    }
}

impl std::error::Error for DistanceBeyondIndex {}

/// The mutable, build-time side of a [`DeletionIndex`] — see the module's own
/// doc comment for the full Build → Freeze → Query shape.
pub struct DeletionIndexBuilder {
    max_distance: u32,
    words: Vec<Box<str>>,
    deletions: FxHashMap<Box<[char]>, Vec<u32>>,
}

impl DeletionIndexBuilder {
    /// A builder with no entries yet, capped at `max_distance` edits.
    ///
    /// This cap is permanent for every [`DeletionIndex`] this builder ever
    /// produces — see [`DistanceBeyondIndex`]. Choose it as large as your real
    /// workload needs and no larger: construction cost grows combinatorially
    /// with it (`scalar length choose max_distance`, summed over every indexed
    /// word).
    #[must_use]
    pub fn new(max_distance: u32) -> Self {
        Self {
            max_distance,
            words: Vec::new(),
            deletions: FxHashMap::default(),
        }
    }

    /// Inserts `word`. A word already present is a no-op — this indexes a *set*
    /// of distinct strings, not a multiset.
    pub fn insert(&mut self, word: &str) {
        let units = to_scalars(word);
        if let Some(existing) = self.deletions.get(units.as_slice())
            && existing.iter().any(|&i| &*self.words[i as usize] == word)
        {
            return;
        }
        let index = self.words.len() as u32;
        for variant in distinct_deletions(&units, self.max_distance) {
            self.deletions
                .entry(variant.into_boxed_slice())
                .or_default()
                .push(index);
        }
        self.words.push(word.into());
    }

    /// Inserts every word in `words`.
    pub fn insert_all<I>(&mut self, words: I)
    where
        I: IntoIterator,
        I::Item: AsRef<str>,
    {
        for word in words {
            self.insert(word.as_ref());
        }
    }

    /// Freezes the builder into an immutable, `Send + Sync` [`DeletionIndex`].
    #[must_use]
    pub fn build(self) -> DeletionIndex {
        DeletionIndex {
            max_distance: self.max_distance,
            words: self.words,
            deletions: self.deletions,
        }
    }
}

/// An immutable, frozen symmetric-delete index — see the module's own doc
/// comment for the design, the build-time `max_distance` ceiling, and how it
/// compares with [`FuzzyIndex`](crate::FuzzyIndex).
///
/// ```
/// use verbora_spellcheck::DeletionIndexBuilder;
///
/// let mut builder = DeletionIndexBuilder::new(2);
/// builder.insert_all(["kitten", "sitting", "kitchen", "mitten", "hello"]);
/// let index = builder.build();
///
/// let within_two: Vec<&str> = index
///     .neighbors("kitten", 2)
///     .expect("2 is within the ceiling")
///     .map(|n| n.word)
///     .collect();
/// assert_eq!(within_two, ["kitten", "kitchen", "mitten"]);
///
/// // Asking for more than the index was built for is an error, not a
/// // silently narrowed answer.
/// assert!(index.neighbors("kitten", 3).is_err());
/// ```
#[derive(Default)]
pub struct DeletionIndex {
    max_distance: u32,
    /// Indexed words in insertion order; the index a word holds here *is* its
    /// enumeration rank, which is what makes result order specifiable.
    words: Vec<Box<str>>,
    deletions: FxHashMap<Box<[char]>, Vec<u32>>,
}

impl DeletionIndex {
    /// The number of distinct words indexed.
    #[must_use]
    pub fn len(&self) -> usize {
        self.words.len()
    }

    /// Whether no words are indexed.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.words.is_empty()
    }

    /// The build-time edit-distance ceiling — see [`DeletionIndexBuilder::new`]
    /// and [`DistanceBeyondIndex`].
    #[must_use]
    pub fn max_distance(&self) -> u32 {
        self.max_distance
    }

    /// Every indexed word within `max_distance` edits of `query`.
    ///
    /// # Order
    ///
    /// **Ascending insertion order**: word *i* precedes word *j* whenever *i*
    /// was inserted before *j*. It is neither alphabetical nor ranked by
    /// distance — this is candidate generation, not a search engine. Sort by
    /// [`Neighbor::distance`] at the call site when you want a ranking; the
    /// distance is already there, so ranking costs one sort and no
    /// recomputation.
    ///
    /// # Errors
    ///
    /// [`DistanceBeyondIndex`] when `max_distance` exceeds
    /// [`DeletionIndex::max_distance`].
    ///
    /// # Laziness
    ///
    /// Candidate *discovery* has already happened when this returns: every
    /// relevant deletion sequence was looked up and every hit collected, which
    /// cannot be streamed the way a BK-tree's traversal can. What stays lazy is
    /// the exact-distance *verification* of each candidate, so a caller who
    /// wants only the first few matches does not pay to verify the rest.
    pub fn neighbors(
        &self,
        query: &str,
        max_distance: u32,
    ) -> Result<DeletionNeighbors<'_>, DistanceBeyondIndex> {
        if max_distance > self.max_distance {
            return Err(DistanceBeyondIndex {
                requested: max_distance,
                ceiling: self.max_distance,
            });
        }
        let units = to_scalars(query);

        let mut candidates: Vec<u32> = Vec::new();
        for variant in distinct_deletions(&units, max_distance) {
            if let Some(indices) = self.deletions.get(variant.as_slice()) {
                candidates.extend_from_slice(indices);
            }
        }
        // Sorting by index is what makes the documented order *insertion*
        // order rather than "whichever deletion sequence happened to hash
        // first" — and the dedup that follows needs the sort anyway.
        candidates.sort_unstable();
        candidates.dedup();

        Ok(DeletionNeighbors {
            words: &self.words,
            query: query.to_owned(),
            max_distance,
            candidates: candidates.into_iter(),
        })
    }
}

/// Lazy iterator returned by [`DeletionIndex::neighbors`].
pub struct DeletionNeighbors<'a> {
    words: &'a [Box<str>],
    query: String,
    max_distance: u32,
    candidates: std::vec::IntoIter<u32>,
}

impl<'a> Iterator for DeletionNeighbors<'a> {
    type Item = Neighbor<'a>;

    fn next(&mut self) -> Option<Neighbor<'a>> {
        for index in self.candidates.by_ref() {
            let word = self.words[index as usize].as_ref();
            // A distance is bounded by the longer operand's scalar length, so
            // the narrowing is lossless for any input that fits in memory.
            let distance = damerau_levenshtein(&self.query, word) as u32;
            if distance <= self.max_distance {
                return Some(Neighbor { word, distance });
            }
        }
        None
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (0, Some(self.candidates.len()))
    }
}

impl std::iter::FusedIterator for DeletionNeighbors<'_> {}

impl std::fmt::Debug for DeletionNeighbors<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeletionNeighbors")
            .field("unverified_candidates", &self.candidates.len())
            .field("max_distance", &self.max_distance)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words<'a>(it: impl Iterator<Item = Neighbor<'a>>) -> Vec<&'a str> {
        it.map(|n| n.word).collect()
    }

    #[test]
    fn empty_index_yields_no_neighbors() {
        let index = DeletionIndexBuilder::new(2).build();
        assert!(index.is_empty());
        assert_eq!(index.neighbors("anything", 2).unwrap().count(), 0);
    }

    #[test]
    fn duplicate_inserts_collapse_to_one_entry() {
        let mut b = DeletionIndexBuilder::new(2);
        b.insert_all(["hello", "hello", "hello"]);
        assert_eq!(b.build().len(), 1);
    }

    #[test]
    fn results_come_back_in_insertion_order() {
        let mut b = DeletionIndexBuilder::new(1);
        // Deliberately not alphabetical, so insertion order and alphabetical
        // order disagree and the test can tell them apart.
        b.insert_all(["mitten", "kitten", "bitten"]);
        let index = b.build();
        assert_eq!(
            words(index.neighbors("kitten", 1).unwrap()),
            ["mitten", "kitten", "bitten"]
        );
    }

    #[test]
    fn the_reported_distance_is_the_real_one() {
        let mut b = DeletionIndexBuilder::new(2);
        b.insert_all(["kitten", "mitten", "kitchen"]);
        let index = b.build();
        for n in index.neighbors("kitten", 2).unwrap() {
            assert_eq!(
                n.distance as usize,
                damerau_levenshtein("kitten", n.word),
                "{:?}",
                n.word
            );
        }
    }

    #[test]
    fn exact_and_near_matches() {
        let mut b = DeletionIndexBuilder::new(2);
        b.insert_all(["kitten", "sitting", "kitchen", "mitten", "hello"]);
        let index = b.build();

        let mut within_two = words(index.neighbors("kitten", 2).unwrap());
        within_two.sort_unstable();
        assert_eq!(within_two, ["kitchen", "kitten", "mitten"]);

        assert_eq!(words(index.neighbors("kitten", 0).unwrap()), ["kitten"]);
    }

    #[test]
    fn a_transposition_is_one_edit_under_this_crate_s_metric() {
        // The whole reason the metric is Damerau–Levenshtein and not plain
        // Levenshtein: a transposed pair is the commonest typo class, and it
        // costs one edit, not two.
        assert_eq!(damerau_levenshtein("hte", "the"), 1);
        let mut b = DeletionIndexBuilder::new(1);
        b.insert("the");
        let index = b.build();
        assert_eq!(words(index.neighbors("hte", 1).unwrap()), ["the"]);
    }

    #[test]
    fn a_query_beyond_the_ceiling_is_an_error() {
        let mut b = DeletionIndexBuilder::new(1);
        b.insert_all(["kitten", "sitting"]);
        let index = b.build();

        assert_eq!(index.max_distance(), 1);
        let err = index.neighbors("kitten", 10).unwrap_err();
        assert_eq!(
            err,
            DistanceBeyondIndex {
                requested: 10,
                ceiling: 1
            }
        );
        assert!(err.to_string().contains("ceiling of 1"));
        // The distances it *was* built for still answer.
        assert_eq!(words(index.neighbors("kitten", 1).unwrap()), ["kitten"]);
    }

    #[test]
    fn deletion_collisions_are_verified_not_trusted() {
        // "cats" and "cars" both delete to "cas" at depth 1; retrieval finds
        // both, and verification decides.
        let mut b = DeletionIndexBuilder::new(1);
        b.insert_all(["cats", "cars", "cabs", "unrelated"]);
        let index = b.build();
        let mut got = words(index.neighbors("cats", 1).unwrap());
        got.sort_unstable();
        assert_eq!(got, ["cabs", "cars", "cats"]);
    }

    /// The module doc comment claims `DeletionIndex`/`DeletionIndexBuilder`
    /// are `Send + Sync` (no locks, no `Rc`/`RefCell`/raw pointers, no
    /// `unsafe impl` anywhere in this file). A successful compile *is* the
    /// check.
    #[test]
    fn deletion_index_types_are_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<DeletionIndex>();
        assert_send_sync::<DeletionIndexBuilder>();
    }

    #[test]
    fn astral_scalars_are_found_at_their_real_distance() {
        // '😀' and '𝕳' share no UTF-16 half, so a code-unit generator would
        // need depth 2 to connect them; at scalar granularity it is depth 1,
        // which is exactly what the metric reports.
        assert_eq!(damerau_levenshtein("😀abc", "𝕳abc"), 1);
        let mut b = DeletionIndexBuilder::new(1);
        b.insert_all(["😀abc", "𝕳abc", "unrelated"]);
        let index = b.build();
        let got = words(index.neighbors("😀abc", 1).unwrap());
        assert!(got.contains(&"𝕳abc"), "expected 𝕳abc among {got:?}");
        assert!(got.contains(&"😀abc"));
    }
}
