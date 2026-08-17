//! A SymSpell-style deletion index: build once with a **fixed maximum edit
//! distance**, query many — the second fuzzy-matching structure this crate
//! offers, alongside [`FuzzyIndex`](crate::FuzzyIndex)'s BK-tree, not a
//! replacement for it.
//!
//! This is a Verbora-native extension, exactly like [`FuzzyIndex`] — no
//! `docs/MIGRATION_MATRIX.md` entry references it, and it must never be
//! reported as parity. [`FuzzyIndex`]'s own doc comment explains why a
//! BK-tree was built first (`max_distance` as a query-time parameter, a
//! correctness argument that follows directly from the triangle
//! inequality) and explicitly flags a SymSpell-style deletion index as the
//! documented alternative, not built for lack of its own benchmark evidence
//! at the time. That evidence now exists: `docs/PERFORMANCE_GAPS.md` entry
//! 35 measured `fast_symspell` (a real, pinned third-party deletion-index
//! crate) beating this BK-tree's own query speed by a margin that **widens
//! with corpus size** (2.15× at 100 words to 66.7× at 20,000) — real
//! evidence that this architecture is worth having available, not a
//! speculative addition.
//!
//! # The trade-off, stated up front
//!
//! A deletion index trades away exactly what a BK-tree offers for free:
//! `max_distance` must be fixed when the index is **built**, not chosen per
//! query. [`DeletionIndex::neighbors`] still takes a `max_distance`
//! argument, but it is silently **capped** at the index's own build-time
//! maximum — a query asking for more than the index was built to answer
//! cannot get it, structurally, no matter what argument is passed. See
//! [`DeletionIndexBuilder::new`]'s own doc comment for why this is a hard
//! ceiling, not a soft default.
//!
//! In exchange, construction is far cheaper for a **fixed, small**
//! `max_distance` (candidate generation is O(word length choose distance)
//! per word, not the tree-shape-dependent cost a BK-tree insert pays), and
//! query cost does not grow with the size of the *result* the way a
//! generate-every-edit approach's does — both are the properties the
//! competitive audit measured `fast_symspell` winning on.
//!
//! # Algorithm
//!
//! For every indexed word, generate every unit sequence reachable by
//! deleting up to `max_distance` **UTF-16 code units** (including the word
//! itself, at deletion depth 0), and record which word(s) each deletion
//! sequence came from. A query generates its *own* deletions up to the same
//! (capped) distance and looks each one up; every dictionary word found
//! this way is a **candidate**, not yet a confirmed match — deletion
//! collisions are a real possibility (`"cats"` and `"cars"` both delete to
//! `"cas"`), so every candidate's *real* edit distance is computed with
//! [`verbora_distance::levenshtein`] before it is returned. This
//! over-generate-then-verify shape is exactly what [`FuzzyIndex`]'s own doc
//! comment names as one reason a BK-tree needs no separate verification
//! pass and a deletion index does — stated there as a cost, paid here
//! deliberately in exchange for the construction/query trade-off above.
//!
//! # Why UTF-16 code units, not `char`
//!
//! [`verbora_distance::levenshtein`] computes distance over UTF-16 code
//! units (this workspace's reference-parity convention — see
//! `crates/verbora-distance/src/units.rs`), not `char`s. The classical
//! SymSpell completeness argument — "if the real edit distance is ≤ `n`,
//! deleting up to `n` *of the same atomic unit* from each side is
//! guaranteed to produce a common string" — only holds when candidate
//! generation and distance verification agree on what an "atomic unit" is.
//! Generating deletions by `char` while verifying distance by UTF-16 code
//! unit would silently under-generate candidates for any astral-plane
//! (non-BMP) input — one `char` there is *two* code units, so a query and a
//! dictionary word could sit at true distance `n` without any `char`-level
//! deletion of depth `≤n` connecting them, even though a code-unit-level
//! one always does by construction. This crate's own `edits.rs`/`units.rs`
//! already documents exactly this class of bug for
//! [`Spellcheck`](crate::Spellcheck)'s own edit generator (`"a😀b"` edited
//! by `char` silently drops 83 of 238 real the reference candidates) — the
//! same reasoning applies here, so deletion generation below operates on
//! `Vec<u16>`, matching [`verbora_distance::levenshtein`]'s own granularity
//! exactly rather than approximating it.
//!
//! One consequence: a deletion sequence can end up cutting a surrogate pair
//! in half (the same way [`Spellcheck`]'s own edit generator's candidates
//! can — see `crate::units::EditUnit::as_str`'s own doc comment). Such a
//! sequence can never itself decode to a valid dictionary word, but it can
//! still be a genuine shared connection point between two *different*,
//! well-formed words' own deletion sets, so it is kept as a `Box<[u16]>`
//! map key throughout rather than being converted to (or rejected as) a
//! `String` — there is never a need to decode a deletion sequence back to
//! text at all, only to compare it for equality.
//!
//! # Build → Freeze → Query
//!
//! Same shape as [`FuzzyIndex`]: [`DeletionIndexBuilder::insert`] as many
//! times as you like, then [`DeletionIndexBuilder::build`] freezes into an
//! immutable, `Send + Sync` [`DeletionIndex`].
//!
//! # Metric: plain Levenshtein only
//!
//! Same reasoning as [`FuzzyIndex`]: candidate verification uses
//! [`verbora_distance::levenshtein`] with
//! [`verbora_distance::levenshtein::Options::default`] (unit-cost, no
//! caller-supplied weighting) — unlike a BK-tree's pruning, correctness
//! here does not strictly *require* a metric satisfying the triangle
//! inequality, but keeping the same fixed metric across both of this
//! crate's fuzzy-matching structures means a caller switching between them
//! never has to reason about two different notions of "distance".

use rustc_hash::FxHashMap;
use verbora_distance::levenshtein;

use crate::units::to_utf16;

/// Rounds [`levenshtein`]'s `f64` to the exact integer it always is under
/// this module's fixed unit-cost metric — identical helper to
/// `fuzzy_index::edit_distance`, kept separate rather than shared since the
/// two modules are independent Verbora-native extensions with no reason to
/// couple their internals.
fn edit_distance(a: &str, b: &str) -> u32 {
    levenshtein(a, b, &Default::default()).round() as u32
}

/// Every unit sequence reachable from `units` by deleting up to
/// `max_distance` UTF-16 code units, **including `units` itself** (deletion
/// depth 0) — may contain duplicate sequences (different deleted positions
/// can produce the same result, e.g. deleting either `'a'` in `"aab"`),
/// which callers dedupe themselves rather than paying for a `HashSet` here
/// on every call.
fn deletions(units: &[u16], max_distance: u32) -> Vec<Vec<u16>> {
    let mut out = vec![units.to_vec()];
    let mut frontier: Vec<Vec<u16>> = vec![units.to_vec()];
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

/// Deduplicated [`deletions`], as the sorted `Vec<Vec<u16>>` both build and
/// query time need before touching the shared map.
fn distinct_deletions(units: &[u16], max_distance: u32) -> Vec<Vec<u16>> {
    let mut out = deletions(units, max_distance);
    out.sort_unstable();
    out.dedup();
    out
}

/// The mutable, build-time side of a [`DeletionIndex`] — see the module's
/// own doc comment for the full Build → Freeze → Query shape.
pub struct DeletionIndexBuilder {
    max_distance: u32,
    words: Vec<Box<str>>,
    deletions: FxHashMap<Box<[u16]>, Vec<u32>>,
}

impl DeletionIndexBuilder {
    /// A builder with no entries yet, capped at `max_distance` edits.
    ///
    /// This cap is permanent for every [`DeletionIndex`] this builder ever
    /// produces: [`DeletionIndex::neighbors`] can never answer a query
    /// asking for more edits than this, because deletion sequences past
    /// this depth were never generated for any indexed word — there is
    /// nothing for a deeper query to find, structurally, not a limitation
    /// that could be lifted by trying harder at query time. Choose it the
    /// way you would choose `fast_symspell`'s own construction-time
    /// `max_edit_distance`: as large as your real correction workload
    /// needs, no larger — construction cost grows combinatorially with it
    /// (`unit length choose max_distance`, summed over every indexed word).
    #[must_use]
    pub fn new(max_distance: u32) -> Self {
        Self {
            max_distance,
            words: Vec::new(),
            deletions: FxHashMap::default(),
        }
    }

    /// Inserts `word`. A word already present is a no-op — like
    /// [`FuzzyIndex`](crate::FuzzyIndex), this indexes a *set* of distinct
    /// strings, not a multiset.
    pub fn insert(&mut self, word: &str) {
        let units = to_utf16(word);
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

/// An immutable, frozen SymSpell-style deletion index — see the module's
/// own doc comment for the full design, the build-time `max_distance`
/// ceiling, and why [`FuzzyIndex`](crate::FuzzyIndex) (a BK-tree) is this
/// crate's default rather than this structure.
///
/// ```
/// use verbora_spellcheck::DeletionIndexBuilder;
///
/// let mut builder = DeletionIndexBuilder::new(2);
/// for word in ["kitten", "sitting", "kitchen", "mitten", "hello"] {
///     builder.insert(word);
/// }
/// let index = builder.build();
///
/// let mut within_two: Vec<&str> = index.neighbors("kitten", 2).collect();
/// within_two.sort_unstable();
/// assert_eq!(within_two, ["kitchen", "kitten", "mitten"]);
///
/// // Asking for more than the index was built for (3) silently caps at 2 —
/// // "sitting" (real distance 3 from "kitten") is not found either way.
/// let mut within_three: Vec<&str> = index.neighbors("kitten", 3).collect();
/// within_three.sort_unstable();
/// assert_eq!(within_three, within_two);
/// ```
#[derive(Default)]
pub struct DeletionIndex {
    max_distance: u32,
    words: Vec<Box<str>>,
    deletions: FxHashMap<Box<[u16]>, Vec<u32>>,
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

    /// The build-time edit-distance ceiling — see
    /// [`DeletionIndexBuilder::new`]'s own doc comment. No query against
    /// this index can ever find a match beyond this distance.
    #[must_use]
    pub fn max_distance(&self) -> u32 {
        self.max_distance
    }

    /// Every indexed word within `max_distance` edits of `query`, lazily
    /// verified (matching `FuzzyIndex::neighbors`'s own laziness for the
    /// verification step — see the module doc comment for why the
    /// *candidate-gathering* step cannot be made lazy the same way a
    /// BK-tree's traversal can).
    ///
    /// `max_distance` is silently capped at
    /// [`DeletionIndex::max_distance`] — see that method's doc comment.
    /// Order is candidate-discovery order (effectively arbitrary — a
    /// `HashMap` iteration order over whichever deletion sequences the
    /// query happened to generate), not alphabetical and not ranked by
    /// distance; compose a ranking at the call site the same way
    /// `site/recipes/fuzzy-matching.md` already documents for
    /// [`FuzzyIndex`](crate::FuzzyIndex).
    #[must_use]
    pub fn neighbors(&self, query: &str, max_distance: u32) -> DeletionNeighbors<'_> {
        let capped = max_distance.min(self.max_distance);
        let units = to_utf16(query);

        let mut candidates: Vec<u32> = Vec::new();
        for variant in distinct_deletions(&units, capped) {
            if let Some(indices) = self.deletions.get(variant.as_slice()) {
                candidates.extend_from_slice(indices);
            }
        }
        candidates.sort_unstable();
        candidates.dedup();

        DeletionNeighbors {
            words: &self.words,
            query: query.to_owned(),
            max_distance: capped,
            candidates: candidates.into_iter(),
        }
    }
}

/// Lazy iterator returned by [`DeletionIndex::neighbors`].
///
/// Candidate *discovery* already happened by the time this is constructed
/// (every relevant deletion sequence was looked up and every hit collected
/// — see the module doc comment for why that step cannot be streamed); what
/// stays lazy here is the real-edit-distance *verification* of each
/// candidate, so a caller who only wants the first few matches does not pay
/// to verify the rest.
pub struct DeletionNeighbors<'a> {
    words: &'a [Box<str>],
    query: String,
    max_distance: u32,
    candidates: std::vec::IntoIter<u32>,
}

impl<'a> Iterator for DeletionNeighbors<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<&'a str> {
        for index in self.candidates.by_ref() {
            let word = self.words[index as usize].as_ref();
            if edit_distance(&self.query, word) <= self.max_distance {
                return Some(word);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deletions_include_the_word_itself_at_depth_zero() {
        let units = to_utf16("cat");
        let d = deletions(&units, 0);
        assert_eq!(d, [to_utf16("cat")]);
    }

    #[test]
    fn deletions_at_depth_one_are_every_single_unit_removal() {
        let units = to_utf16("cat");
        let d = distinct_deletions(&units, 1);
        let mut expected: Vec<Vec<u16>> = ["cat", "at", "ct", "ca"]
            .iter()
            .map(|s| to_utf16(s))
            .collect();
        expected.sort_unstable();
        assert_eq!(d, expected);
    }

    #[test]
    fn deletions_stop_early_once_the_word_is_exhausted() {
        // "ab" has only 2 units; asking for depth 5 must not panic or loop
        // forever removing from an empty sequence.
        let units = to_utf16("ab");
        let d = distinct_deletions(&units, 5);
        let mut expected: Vec<Vec<u16>> =
            ["ab", "a", "b", ""].iter().map(|s| to_utf16(s)).collect();
        expected.sort_unstable();
        assert_eq!(d, expected);
    }

    #[test]
    fn deletions_operate_on_utf16_units_not_chars() {
        // "😀" is 2 UTF-16 units (a surrogate pair). Deleting 1 unit from it
        // must yield each *half* of the pair separately, not "delete the
        // whole character" the way a `char`-based generator would.
        let units = to_utf16("😀");
        assert_eq!(units.len(), 2);
        let d = distinct_deletions(&units, 1);
        let mut expected = vec![units.clone(), vec![units[0]], vec![units[1]]];
        expected.sort_unstable();
        assert_eq!(d, expected);
    }

    #[test]
    fn empty_index_yields_no_neighbors() {
        let index = DeletionIndexBuilder::new(2).build();
        assert!(index.is_empty());
        assert_eq!(index.neighbors("anything", 2).count(), 0);
    }

    #[test]
    fn duplicate_inserts_collapse_to_one_entry() {
        let mut b = DeletionIndexBuilder::new(2);
        b.insert("hello");
        b.insert("hello");
        b.insert("hello");
        let index = b.build();
        assert_eq!(index.len(), 1);
    }

    #[test]
    fn exact_and_one_edit_matches() {
        let mut b = DeletionIndexBuilder::new(2);
        for w in ["kitten", "sitting", "kitchen", "mitten", "hello"] {
            b.insert(w);
        }
        let index = b.build();

        let mut within_two: Vec<&str> = index.neighbors("kitten", 2).collect();
        within_two.sort_unstable();
        assert_eq!(within_two, ["kitchen", "kitten", "mitten"]);

        assert!(index.neighbors("kitten", 0).eq(["kitten"]));
    }

    #[test]
    fn query_distance_is_capped_at_the_build_time_maximum() {
        let mut b = DeletionIndexBuilder::new(1);
        b.insert("kitten");
        b.insert("sitting"); // real distance 3 from "kitten"
        let index = b.build();

        assert_eq!(index.max_distance(), 1);
        // Asking for 10 is silently capped at 1 -- "sitting" is unreachable
        // no matter what, since its deletions past depth 1 were never
        // generated for it during build.
        let got: Vec<&str> = index.neighbors("kitten", 10).collect();
        assert_eq!(got, ["kitten"]);
    }

    #[test]
    fn deletion_collisions_are_verified_not_trusted() {
        // "cats" and "cars" both delete to "cas" at depth 1, but their real
        // edit distance from each other is 1 (substitute r/t), and from a
        // third, unrelated word "cabs" it is also 1 -- exercising that a
        // shared deletion sequence alone is not treated as a confirmed
        // match beyond what real Levenshtein distance actually allows.
        let mut b = DeletionIndexBuilder::new(1);
        for w in ["cats", "cars", "cabs", "unrelated"] {
            b.insert(w);
        }
        let index = b.build();
        let mut got: Vec<&str> = index.neighbors("cats", 1).collect();
        got.sort_unstable();
        assert_eq!(got, ["cabs", "cars", "cats"]);
    }

    /// The module doc comment claims `DeletionIndex`/`DeletionIndexBuilder`
    /// are `Send + Sync` (no locks, no `Rc`/`RefCell`/raw pointers, no
    /// `unsafe impl` anywhere in this file). A successful compile of this
    /// function *is* the check — it would fail to compile if a future
    /// change added interior mutability or a non-`Send`/`Sync` field.
    #[test]
    fn deletion_index_types_are_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<DeletionIndex>();
        assert_send_sync::<DeletionIndexBuilder>();
    }

    #[test]
    fn astral_characters_are_found_correctly() {
        // Two different astral (non-BMP) words at true distance 1
        // (substituting the whole character costs 2 in UTF-16-unit terms
        // when the two characters share no surrogate half, but sharing the
        // low or high surrogate can bring it to 1 -- either way, this must
        // match whatever `verbora_distance::levenshtein` itself reports,
        // not an independent char-level guess).
        let mut b = DeletionIndexBuilder::new(2);
        for w in ["😀abc", "🙁abc", "unrelated"] {
            b.insert(w);
        }
        let index = b.build();
        let d = edit_distance("😀abc", "🙁abc");
        let got: Vec<&str> = index.neighbors("😀abc", d).collect();
        assert!(got.contains(&"🙁abc"), "expected 🙁abc among {got:?}");
        assert!(got.contains(&"😀abc"));
    }
}
