//! Fuzzy string indexing with a query-time distance: build once, query many.
//!
//! [`FuzzyIndex`] answers one question: *which of these stored strings are
//! within edit distance `k` of this query?* — by pre-indexing the dictionary
//! once instead of computing a distance against every stored word on every
//! call.
//!
//! # What this is, and what it deliberately is not
//!
//! [`FuzzyIndex::neighbors`] is **candidate generation** — a blocking step —
//! not a search engine. It does not rank, does not pick a "best" match, and
//! does not accept a query language, the same boundary
//! [`verbora_phonetics::PhoneticIndex`](https://docs.rs/verbora-phonetics)
//! draws for phonetic candidate generation. Every neighbour comes back with the
//! exact distance already computed ([`Neighbor::distance`]), so ranking at the
//! call site is one sort and no recomputation.
//!
//! # Build → Freeze → Query
//!
//! [`FuzzyIndexBuilder`] is the mutable, convenient side: call
//! [`FuzzyIndexBuilder::insert`] as many times as you like, in any order.
//! [`FuzzyIndexBuilder::build`] freezes it into a [`FuzzyIndex`] — an
//! immutable, `Send + Sync` structure with no locks. Share it with `Arc`;
//! nothing about querying it needs `&mut`.
//!
//! # A BK-tree, and how it differs from [`DeletionIndex`]
//!
//! [`FuzzyIndex`] is a [BK-tree](https://en.wikipedia.org/wiki/BK-tree)
//! (Burkhard–Keller, 1973): every node is a word, and its children are keyed by
//! their *exact* edit distance from it. A query walks from the root, computing
//! the real distance to each visited node and using the triangle inequality to
//! skip whole subtrees that cannot contain a match — no subtree is visited
//! unless it could still hold one. This is **exhaustive and exact**: it returns
//! precisely the words a brute-force scan of the dictionary would, without
//! computing a distance against every one of them.
//!
//! [`DeletionIndex`] answers the same question by symmetric deletion. The two
//! differ in one respect that decides which to reach for: `FuzzyIndex` takes
//! `max_distance` per **query**, `DeletionIndex` fixes it at **build** time and
//! rejects anything larger. Pick per call site.
//!
//! [`DeletionIndex`]: crate::DeletionIndex
//!
//! # Metric and unit
//!
//! [`verbora_distance::damerau_levenshtein`] — unrestricted
//! Damerau–Levenshtein, unit cost, counted in Unicode scalar values. There is
//! no cost parameter, and there deliberately cannot be one: a BK-tree's pruning
//! is correct only under a true metric, and the weighted variants are not
//! metrics for arbitrary cost sets. Unrestricted Damerau–Levenshtein is
//! (`docs/design/distance-contract.md`: symmetry and the triangle inequality
//! are pinned there by property test), which is exactly why it, and not
//! optimal-string-alignment, is this crate's metric. A caller who genuinely
//! needs weighted-cost fuzzy matching composes
//! [`verbora_distance::damerau_levenshtein_weighted`] with a manual scan, the
//! same way this module asks callers to compose ranking.

use verbora_distance::damerau_levenshtein;

use crate::neighbor::Neighbor;

/// One word in the tree, plus the edges to its children keyed by exact edit
/// distance from this word — a BK-tree's defining structure.
///
/// Stored in a flat arena ([`FuzzyIndex::nodes`]) rather than recursive
/// `Box<Node>` links, the same choice `verbora-trie` makes, for cache
/// friendliness and to avoid a recursive `Drop` on a deep tree.
struct Node {
    word: Box<str>,
    /// `(edit distance from this node's word to the child's word, index of the
    /// child in the arena)`, kept **sorted ascending by distance**. At most one
    /// child can carry any given distance, so the order is total and it is what
    /// makes [`FuzzyIndex::neighbors`]'s traversal order specifiable rather
    /// than incidental.
    children: Vec<(u32, u32)>,
}

/// The mutable, build-time side of a [`FuzzyIndex`] — see the module's own doc
/// comment for the full Build → Freeze → Query shape.
#[derive(Default)]
pub struct FuzzyIndexBuilder {
    nodes: Vec<Node>,
}

impl FuzzyIndexBuilder {
    /// A builder with no entries yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Inserts `word` into the tree. A word already present (distance 0 from an
    /// existing node) is a no-op — [`FuzzyIndex`] indexes a *set* of distinct
    /// strings, not a multiset.
    ///
    /// Where a new word lands in the tree depends on insertion order. That
    /// affects query *speed* (a well-balanced tree prunes better) and the order
    /// results come back in (documented on [`FuzzyIndex::neighbors`]), but
    /// never *which* words come back: the search is exhaustive and
    /// triangle-inequality-pruned, not heuristic.
    pub fn insert(&mut self, word: &str) {
        if self.nodes.is_empty() {
            self.nodes.push(Node {
                word: word.into(),
                children: Vec::new(),
            });
            return;
        }
        let mut current = 0usize;
        loop {
            // A distance is bounded by the longer operand's scalar length, so
            // the narrowing to `u32` is lossless for any input that fits in
            // memory, and holds a child edge at 8 bytes.
            let distance = damerau_levenshtein(word, &self.nodes[current].word) as u32;
            if distance == 0 {
                return;
            }
            match self.nodes[current]
                .children
                .binary_search_by_key(&distance, |&(d, _)| d)
            {
                Ok(i) => current = self.nodes[current].children[i].1 as usize,
                Err(i) => {
                    let new_index = self.nodes.len() as u32;
                    self.nodes.push(Node {
                        word: word.into(),
                        children: Vec::new(),
                    });
                    self.nodes[current]
                        .children
                        .insert(i, (distance, new_index));
                    return;
                }
            }
        }
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

    /// Freezes the builder into an immutable, `Send + Sync` [`FuzzyIndex`].
    #[must_use]
    pub fn build(self) -> FuzzyIndex {
        FuzzyIndex { nodes: self.nodes }
    }
}

/// An immutable, frozen fuzzy-matching index — see the module's own doc comment
/// for the Build → Freeze → Query shape and what this deliberately does not do
/// (rank, apply a threshold beyond the caller's own `max_distance`, accept a
/// query language).
///
/// ```
/// use verbora_spellcheck::FuzzyIndexBuilder;
///
/// let mut builder = FuzzyIndexBuilder::new();
/// builder.insert_all(["kitten", "sitting", "kitchen", "mitten", "hello"]);
/// let index = builder.build();
///
/// let mut within_two: Vec<&str> = index.neighbors("kitten", 2).map(|n| n.word).collect();
/// within_two.sort_unstable();
/// assert_eq!(within_two, ["kitchen", "kitten", "mitten"]);
///
/// // "kitten" -> "sitting" is the textbook distance-3 example.
/// let mut within_three: Vec<&str> = index.neighbors("kitten", 3).map(|n| n.word).collect();
/// within_three.sort_unstable();
/// assert_eq!(within_three, ["kitchen", "kitten", "mitten", "sitting"]);
/// assert!(!within_three.contains(&"hello"));
/// ```
#[derive(Default)]
pub struct FuzzyIndex {
    nodes: Vec<Node>,
}

impl FuzzyIndex {
    /// The number of distinct words indexed.
    #[must_use]
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Whether no words are indexed.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Every indexed word within `max_distance` edits of `query`, lazily —
    /// `.take(n)` never visits more of the tree than it has to.
    ///
    /// # Order
    ///
    /// Depth-first from the root. A visited node's own word is yielded before
    /// any of its subtrees, and its subtrees are visited in **ascending order
    /// of the edge distance labelling them** (a node has at most one child per
    /// exact distance, so that order is total). Subtrees the triangle
    /// inequality rules out are skipped entirely and contribute nothing.
    ///
    /// That makes the sequence fully determined by the insertion sequence and
    /// the query — deterministic, and written down here rather than left to
    /// whatever the traversal happens to produce. It is *not* alphabetical and
    /// *not* ranked by distance: sort by [`Neighbor::distance`] at the call
    /// site when you want a ranking.
    #[must_use]
    pub fn neighbors(&self, query: &str, max_distance: u32) -> Neighbors<'_> {
        Neighbors {
            nodes: &self.nodes,
            query: query.to_owned(),
            max_distance,
            stack: if self.nodes.is_empty() {
                Vec::new()
            } else {
                vec![0]
            },
        }
    }
}

/// Lazy iterator returned by [`FuzzyIndex::neighbors`].
pub struct Neighbors<'a> {
    nodes: &'a [Node],
    query: String,
    max_distance: u32,
    stack: Vec<u32>,
}

impl<'a> Iterator for Neighbors<'a> {
    type Item = Neighbor<'a>;

    fn next(&mut self) -> Option<Neighbor<'a>> {
        while let Some(index) = self.stack.pop() {
            let node = &self.nodes[index as usize];
            let distance = damerau_levenshtein(&self.query, &node.word) as u32;
            // Triangle inequality: a match reachable through a child can be
            // within `max_distance` of `query` only if that child's own edge
            // distance to this node is within `max_distance` of `distance` --
            // every other subtree is skipped, unvisited. Pushed in descending
            // edge order so the LIFO stack pops them ascending, which is the
            // order this iterator documents.
            for &(edge_distance, child) in node.children.iter().rev() {
                if edge_distance.abs_diff(distance) <= self.max_distance {
                    self.stack.push(child);
                }
            }
            if distance <= self.max_distance {
                return Some(Neighbor {
                    word: &node.word,
                    distance,
                });
            }
        }
        None
    }
}

impl std::iter::FusedIterator for Neighbors<'_> {}

impl std::fmt::Debug for Neighbors<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Neighbors")
            .field("pending_subtrees", &self.stack.len())
            .field("max_distance", &self.max_distance)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_index_yields_no_neighbors() {
        let index = FuzzyIndexBuilder::new().build();
        assert!(index.is_empty());
        assert_eq!(index.neighbors("anything", 5).count(), 0);
    }

    #[test]
    fn duplicates_collapse() {
        let mut b = FuzzyIndexBuilder::new();
        b.insert_all(["hello", "hello", "hello"]);
        assert_eq!(b.build().len(), 1);
    }

    #[test]
    fn the_reported_distance_is_the_real_one() {
        let mut b = FuzzyIndexBuilder::new();
        b.insert_all(["kitten", "sitting", "kitchen", "mitten"]);
        let index = b.build();
        for n in index.neighbors("kitten", 3) {
            assert_eq!(
                n.distance as usize,
                damerau_levenshtein("kitten", n.word),
                "{:?}",
                n.word
            );
        }
    }

    #[test]
    fn subtrees_are_visited_in_ascending_edge_distance() {
        // Root "aaaa"; three children at edge distances 1, 2 and 3. Inserted
        // furthest-first, so insertion order and edge order disagree and the
        // documented order is distinguishable from "whatever was inserted
        // first".
        let mut b = FuzzyIndexBuilder::new();
        b.insert_all(["aaaa", "bbba", "bbaa", "baaa"]);
        let index = b.build();
        let got: Vec<(&str, u32)> = index
            .neighbors("aaaa", 4)
            .map(|n| (n.word, n.distance))
            .collect();
        assert_eq!(
            got,
            [("aaaa", 0), ("baaa", 1), ("bbaa", 2), ("bbba", 3)],
            "children must be visited ascending by edge distance"
        );
    }

    #[test]
    fn a_transposition_is_one_edit() {
        assert_eq!(damerau_levenshtein("hte", "the"), 1);
        let mut b = FuzzyIndexBuilder::new();
        b.insert_all(["the", "then"]);
        let index = b.build();
        let got: Vec<&str> = index.neighbors("hte", 1).map(|n| n.word).collect();
        assert_eq!(got, ["the"]);
    }

    #[test]
    fn fuzzy_index_types_are_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<FuzzyIndex>();
        assert_send_sync::<FuzzyIndexBuilder>();
    }
}
