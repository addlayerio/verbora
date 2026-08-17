//! Fuzzy string indexing: build once, query many, for edit-distance
//! candidate retrieval — a companion to [`Spellcheck`](crate::Spellcheck),
//! not a replacement for it.
//!
//! This is a Verbora-native extension, **not** part of the reference
//! parity surface this crate otherwise is — no `docs/MIGRATION_MATRIX.md`
//! entry references it, and it must never be reported as one. It exists
//! because [`Spellcheck::get_corrections`](crate::Spellcheck::get_corrections)
//! generates every candidate string within `n` edits of the input on
//! *every call* (Norvig's own algorithm, ported faithfully — see this
//! crate's own top-level doc comment) and checks each against a trie —
//! fine for a misspelling-correction workload, where `n` stays small (1–2)
//! and the dictionary is only ever the membership filter. [`FuzzyIndex`]
//! answers a related but different question: *which of these thousands of
//! stored strings are within edit distance `k` of this query?* — by
//! pre-indexing the dictionary once instead of generating edits on every
//! query, which pays off when the same dictionary is queried repeatedly
//! and `k` is not small enough for edit-generation to stay cheap.
//!
//! # What this is, and what it deliberately is not
//!
//! [`FuzzyIndex::neighbors`] answers exactly one question: *which stored
//! entries are within a given edit distance of this query?* It is
//! **candidate generation** — a blocking step — not a search engine. It
//! does not rank, does not pick a "best" match, and does not accept a
//! query language, the same boundary
//! [`verbora_phonetics::PhoneticIndex`](https://docs.rs/verbora-phonetics)
//! draws for phonetic candidate generation. Rank the output with
//! [`verbora_distance`](https://docs.rs/verbora-distance) at the call site
//! when you need that — the exact composition
//! `site/recipes/fuzzy-matching.md` already documents for phonetic
//! candidates applies here unchanged, just with a different candidate
//! source.
//!
//! # Build → Freeze → Query
//!
//! [`FuzzyIndexBuilder`] is the mutable, convenient side: call
//! [`FuzzyIndexBuilder::insert`] as many times as you like, in any order.
//! [`FuzzyIndexBuilder::build`] freezes it into a [`FuzzyIndex`] — an
//! immutable, `Send + Sync` structure with no locks. Share it with `Arc`;
//! nothing about querying it needs `&mut`.
//!
//! # A BK-tree, and why not SymSpell's deletion index
//!
//! [`FuzzyIndex`] is a [BK-tree](https://en.wikipedia.org/wiki/BK-tree)
//! (Burkhard–Keller, 1973): every node is a word, and its children are
//! keyed by their *exact* edit distance from it. A query walks from the
//! root, computing the real distance to each visited node and using the
//! triangle inequality to skip whole subtrees that cannot contain a match
//! — no subtree is visited unless it could still hold one. This is
//! **exhaustive and exact**: it returns precisely the same set
//! [`Spellcheck`](crate::Spellcheck)'s own brute-force edit-generation
//! would (verified directly — see `tests/fuzzy_index.rs`), just without
//! generating every edit string up front.
//!
//! SymSpell's deletion-based index (precompute every string reachable by
//! *deleting* up to `k` characters from each dictionary word, then do the
//! same to the query and intersect) is the faster design in the published
//! literature for small, fixed `k` — but it requires `k` to be fixed at
//! build time, needs a real edit-distance computation as a verification
//! pass anyway (the deletion trick alone over-generates), and has more
//! subtle correctness edge cases to get exactly right. A BK-tree needs
//! none of that: `max_distance` is a query-time parameter, not a
//! build-time one, and its correctness follows directly from the triangle
//! inequality rather than from a symmetric-deletion argument. Given this
//! module's own bar (`# Verbora-Native Extensions` in `AGENTS.md`:
//! benchmark-justified, not built speculatively ahead of real evidence), a
//! BK-tree was built and benchmarked first because it is the simpler
//! design to prove correct.
//!
//! **Update:** a SymSpell-style deletion index now exists too —
//! [`crate::DeletionIndex`] — added once real evidence justified it: the
//! competitive audit measured a pinned third-party deletion-index crate
//! (`fast_symspell`) beating this BK-tree's own query speed by a margin
//! that *widens* with corpus size (`docs/PERFORMANCE_GAPS.md` entry 35).
//! `DeletionIndex` does not replace `FuzzyIndex` — it trades away the
//! query-time-`max_distance` flexibility documented above for faster
//! queries at a fixed, build-time-chosen distance; see its own doc comment
//! for the real, measured trade-off between the two and when each wins.
//!
//! # Metric: plain Levenshtein only, not a caller-supplied `Options`
//!
//! [`verbora_distance::levenshtein`]'s `Options` lets a caller assign
//! arbitrary per-operation costs. A BK-tree's pruning is only *correct* —
//! never missing a real match — when the underlying distance function is a
//! proper metric, specifically when it satisfies the triangle inequality;
//! uniform-cost Levenshtein (insertion, deletion, and substitution all
//! costing 1, [`verbora_distance::levenshtein::Options::default`]) is a
//! proper metric, but an arbitrary caller-supplied cost assignment is not
//! guaranteed to be. Rather than accept an `Options` that could silently
//! make queries return incomplete results, [`FuzzyIndex`] fixes the metric
//! to the default and does not expose it as a parameter — a caller who
//! genuinely needs weighted-cost fuzzy matching can compose
//! `verbora_distance::levenshtein` directly with a manual scan, the same
//! way this module's own doc comment asks callers to compose ranking.

use verbora_distance::levenshtein;

/// One word in the tree, plus the edges to its children keyed by exact
/// edit distance from this word — a BK-tree's own defining structure.
/// Stored in a flat arena ([`FuzzyIndex::nodes`]) rather than recursive
/// `Box<Node>` links, the same "flat arena over `Box`-linked nodes" choice
/// `verbora-trie` already made for cache-friendliness and to avoid a
/// recursive `Drop` on a deep tree.
struct Node {
    word: Box<str>,
    /// `(edit distance from this node's word to the child's word, index of
    /// the child in the arena)`. Order within this `Vec` doesn't affect
    /// query correctness or the *set* of results, only which subtrees a
    /// query happens to visit first.
    children: Vec<(u32, u32)>,
}

/// Rounds [`levenshtein`]'s `f64` to the exact integer it always is under
/// [`FuzzyIndex`]'s fixed unit-cost metric — see the module's own doc
/// comment for why the metric isn't a caller-supplied `Options`.
fn edit_distance(a: &str, b: &str) -> u32 {
    levenshtein(a, b, &Default::default()).round() as u32
}

/// The mutable, build-time side of a [`FuzzyIndex`] — see the module's own
/// doc comment for the full Build → Freeze → Query shape.
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

    /// Inserts `word` into the tree. A word already present (edit distance
    /// 0 from an existing node) is a no-op — [`FuzzyIndex`] indexes a
    /// *set* of distinct strings, not a multiset with per-entry identity
    /// (unlike `verbora_phonetics::PhoneticIndex`'s `EntryId`-per-insert
    /// model, which exists there to map back to caller-supplied entries
    /// that may legitimately repeat; a fuzzy-matched *dictionary* has no
    /// equivalent need).
    ///
    /// Where a new word lands in the tree depends on insertion order —
    /// this affects query *speed* (a well-balanced tree prunes better) but
    /// never query *correctness*: [`FuzzyIndex::neighbors`] is an
    /// exhaustive, triangle-inequality-pruned search, not a heuristic, so
    /// it returns the same set of matches regardless of how the tree
    /// happened to grow.
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
            let distance = edit_distance(word, &self.nodes[current].word);
            if distance == 0 {
                return;
            }
            match self.nodes[current]
                .children
                .iter()
                .find(|&&(d, _)| d == distance)
            {
                Some(&(_, child)) => current = child as usize,
                None => {
                    let new_index = self.nodes.len() as u32;
                    self.nodes.push(Node {
                        word: word.into(),
                        children: Vec::new(),
                    });
                    self.nodes[current].children.push((distance, new_index));
                    return;
                }
            }
        }
    }

    /// Freezes the builder into an immutable, `Send + Sync` [`FuzzyIndex`].
    #[must_use]
    pub fn build(self) -> FuzzyIndex {
        FuzzyIndex { nodes: self.nodes }
    }
}

/// An immutable, frozen fuzzy-matching index — see the module's own doc
/// comment for the full Build → Freeze → Query shape and what this
/// deliberately does not do (rank, apply a threshold beyond the caller's
/// own `max_distance`, accept a query language).
///
/// ```
/// use verbora_spellcheck::FuzzyIndexBuilder;
///
/// let mut builder = FuzzyIndexBuilder::new();
/// for word in ["kitten", "sitting", "kitchen", "mitten", "hello"] {
///     builder.insert(word);
/// }
/// let index = builder.build();
///
/// // "kitten" -> "sitting" is the textbook edit-distance-3 example
/// // (substitute k/s, substitute e/i, insert g); at distance 2 it's not
/// // yet a neighbor.
/// let mut within_two: Vec<&str> = index.neighbors("kitten", 2).collect();
/// within_two.sort_unstable();
/// assert_eq!(within_two, ["kitchen", "kitten", "mitten"]);
///
/// let mut within_three: Vec<&str> = index.neighbors("kitten", 3).collect();
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
    /// `.take(n)` never visits more of the tree than it has to (matching
    /// `PhoneticIndex::neighbors`'s own laziness). Order is the tree's own
    /// traversal order (depth-first through the branches the triangle
    /// inequality didn't prune), not alphabetical and not ranked by
    /// distance — see the module's own doc comment for composing a
    /// ranking at the call site.
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
    type Item = &'a str;

    fn next(&mut self) -> Option<&'a str> {
        while let Some(index) = self.stack.pop() {
            let node = &self.nodes[index as usize];
            let distance = edit_distance(&self.query, &node.word);
            // Triangle inequality: a match reachable through a child can be
            // within `max_distance` of `query` only if that child's own
            // edge distance to this node is within `max_distance` of
            // `distance` -- every other subtree is skipped, unvisited.
            for &(edge_distance, child) in &node.children {
                if edge_distance.abs_diff(distance) <= self.max_distance {
                    self.stack.push(child);
                }
            }
            if distance <= self.max_distance {
                return Some(&node.word);
            }
        }
        None
    }
}
