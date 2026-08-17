//! The trie itself: arena storage, the code-unit walk, and the six public
//! operations that mirror the reference `trie`.

use std::borrow::Cow;

use smallvec::SmallVec;

use crate::iter::{KeysWithPrefix, MatchesOnPath};

/// Arena index of the root node.
pub(crate) const ROOT: u32 = 0;

/// Inline child capacity.
///
/// Two is not arbitrary: with the `union` representation a `SmallVec` reserves
/// `max(size_of::<[T; N]>(), 16)` bytes regardless, so `N = 2` for an 8-byte
/// `Child` is the largest inline capacity that is *free*. In a natural-language
/// trie the overwhelming majority of nodes have one child, so this keeps the
/// heap out of the picture for everything below the first level or two.
const INLINE_CHILDREN: usize = 2;

/// One edge: the UTF-16 code unit that labels it and the child it leads to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Child {
    /// The edge label, a single UTF-16 code unit — *not* a `char`.
    pub(crate) key: u16,
    /// Arena index of the node this edge leads to.
    pub(crate) node: u32,
}

/// A node in the arena.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Node {
    /// Outgoing edges, held in the reference `for…in` enumeration order so that
    /// iteration is a straight scan with no sorting or bookkeeping.
    pub(crate) children: SmallVec<[Child; INLINE_CHILDREN]>,
    /// The reference's `$` flag: a word ends here.
    pub(crate) is_word: bool,
}

/// True for the code units the reference treats as array-index-like object keys.
///
/// A key is an array index when it is the canonical decimal spelling of an
/// integer below `2^32 - 1`. Trie keys are single code units, so the only
/// qualifying ones are the ASCII digits: `'10'` is two units and never appears
/// as a key, and non-ASCII digits such as `'٣'` are not canonical spellings.
#[inline]
const fn is_index_key(unit: u16) -> bool {
    unit >= b'0' as u16 && unit <= b'9' as u16
}

/// Lowercases `s` the way the reference's `String#toLowerCase` does.
///
/// Returns a borrow when nothing changes, which is the common case for
/// already-folded corpora and for every call on a case-sensitive trie.
///
/// Rust's `str::to_lowercase` and the reference's `toLowerCase` agree on every
/// Unicode scalar value (verified by sweeping all 1.1M code points against
/// Node), including the multi-character expansions such as `'İ'` → `"i̇"` and
/// the context-sensitive Greek final sigma. Neither applies locale-specific
/// Turkish or Lithuanian rules.
pub(crate) fn fold(folds: bool, s: &str) -> Cow<'_, str> {
    if !folds {
        return Cow::Borrowed(s);
    }
    if s.is_ascii() {
        // `to_ascii_lowercase` is a byte-wise pass; `to_lowercase` would run the
        // full Unicode machinery to reach the same answer on ASCII.
        if s.as_bytes().iter().any(u8::is_ascii_uppercase) {
            return Cow::Owned(s.to_ascii_lowercase());
        }
        return Cow::Borrowed(s);
    }
    Cow::Owned(s.to_lowercase())
}

/// Where a walk stopped, in enough detail to rebuild every answer the reference
/// derives from it.
struct Walk {
    /// Byte offset, in the *folded* search string, one past the end of the
    /// longest stored word on the path. `None` when no node on the path was a
    /// word — the reference's `lastWord === null`.
    last_word: Option<usize>,
    /// UTF-16 length of that word, tracked alongside so callers that only want
    /// the split need not re-measure.
    last_word_units: usize,
    /// Byte offset of the character at which the walk stopped.
    stopped_at: usize,
    /// Set when the walk died *between* the halves of a surrogate pair, so the
    /// reference's remainder starts with an unpaired low surrogate.
    split_pair: bool,
}

/// A prefix tree, mirroring the reference `Trie` class.
///
/// Construct with [`Trie::new`] for the case-sensitive default, or
/// [`Trie::case_insensitive`] for the folding variant.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Trie {
    /// All nodes, flat. Index 0 is the root and is never removed, so a `u32`
    /// index is always valid and needs no `Option`.
    nodes: Vec<Node>,
    /// Whether stored and queried strings are lowercased first.
    ///
    /// Named for what it *does* rather than for the reference's `cs`, because
    /// The reference's own three-state flag (`undefined` / `null` / `false`)
    /// collapses to this single question: is it exactly `false`?
    folds: bool,
}

impl Default for Trie {
    fn default() -> Self {
        Self::new()
    }
}

impl Trie {
    /// Creates an empty, case-sensitive trie — the reference's `new Trie()`.
    #[must_use]
    pub fn new() -> Self {
        Self::with_case_sensitivity(true)
    }

    /// Creates an empty trie that lowercases everything it stores and every
    /// query it is given — the reference's `new Trie(false)`.
    ///
    /// [`Trie::keys_with_prefix`] is the one exception, and deliberately so; see
    /// its documentation.
    #[must_use]
    pub fn case_insensitive() -> Self {
        Self::with_case_sensitivity(false)
    }

    /// Creates an empty trie, mirroring `new Trie(caseSensitive)`.
    ///
    /// The reference defaults the flag when the argument is `undefined` but
    /// tests it with a strict `=== false` everywhere else, so `new Trie(null)`
    /// and `new Trie(0)` are both case-*sensitive*. Only a literal `false`
    /// enables folding, which is exactly what a Rust `bool` expresses.
    #[must_use]
    pub fn with_case_sensitivity(case_sensitive: bool) -> Self {
        Self {
            nodes: vec![Node::default()],
            folds: !case_sensitive,
        }
    }

    /// Whether this trie compares strings case-sensitively.
    #[must_use]
    pub fn is_case_sensitive(&self) -> bool {
        !self.folds
    }

    /// Reserves capacity for at least `additional` more nodes.
    ///
    /// A trie built from a known word list needs roughly one node per distinct
    /// prefix; reserving up front removes the arena's growth reallocations from
    /// a bulk load.
    pub fn reserve(&mut self, additional: usize) {
        self.nodes.reserve(additional);
    }

    /// Compresses this trie into a [`FrozenTrie`]: a read-only, path-compressed
    /// representation built once for query-heavy workloads.
    ///
    /// # Why this exists
    ///
    /// `docs/PERFORMANCE_GAPS.md` entry 32 measured a real, disclosed loss:
    /// `fast_radix_trie` (a path-compressed radix map) beats this arena's
    /// `predictive_search`/`keys_with_prefix` by 1.64×–2.19×, specifically
    /// because a run of single-child, non-word nodes costs one node-hop each
    /// here but one edge-label comparison there. `Trie` already wins
    /// `build`/`contains` against the same competitor, so this is a targeted
    /// fix for the one operation that loses, not a replacement representation:
    /// `Trie` keeps building exactly as it does today, and this method is the
    /// "Freeze" step of the Build → Freeze → Query pattern this crate already
    /// follows for other Verbora-native query paths (see `AGENTS.md`'s
    /// "Verbora-Native Extensions" policy — `FrozenTrie` is the fifth entry
    /// there).
    ///
    /// # What compresses, and why it is exact
    ///
    /// An original node is kept as a real frozen node when it is the root, is
    /// itself a stored word (`is_word`), or has zero or more than one child —
    /// in every other case it has exactly one child and marks no word, so it
    /// cannot be an independently observable stopping point for any query and
    /// is folded into the edge label leading to the next kept node. This is
    /// exact, not approximate: every position a caller could ever land on
    /// (`contains` returning `true`, a word emitted by `keys_with_prefix`, a
    /// prefix argument ending exactly there) is preserved as a real frozen
    /// node; only the *unobservable* pass-through nodes disappear.
    ///
    /// Compression never reorders anything: a kept node's children are
    /// carried over in exactly their original position, which is what
    /// already encodes this crate's the reference `for…in` enumeration order
    /// (see this module's own `insert_position`) — so [`FrozenTrie`]
    /// reproduces that order with no extra bookkeeping. Surrogate-pair
    /// correctness is inherited the same way: a compressed edge is the exact
    /// same sequence of UTF-16 code units the original chain held, just
    /// stored inline instead of one node per unit.
    ///
    /// # What is *not* on `FrozenTrie` (yet)
    ///
    /// Only `contains` and `keys_with_prefix`/`iter_keys_with_prefix`/`keys`
    /// are implemented — the operations the measured gap and the benchmarked
    /// `predictive_search`/`contains_hit`/`contains_miss` groups are about.
    /// `find_matches_on_path`/`find_prefix`/`find_prefix_lengths` have no
    /// frozen counterpart: nothing in this crate's own benchmarks or the
    /// competitive audit found a loss there, so extending compression to
    /// those top-down, single-path walks (which would need byte-offset
    /// tracking across a multi-unit edge label, not just node-hop counting)
    /// is not attempted here. Call the equivalent method on the original
    /// `Trie` for those.
    ///
    /// # Cost
    ///
    /// One linear pass over every original node (`O(n)` in node count,
    /// `O(n)` extra space for the kept-node index map plus the compressed
    /// unit buffer) — the same shape of one-time cost `verbora-tagger`'s
    /// `build.rs` table-packing step already pays for its own frozen
    /// lexicon. Call this once after bulk-loading with [`Trie::add_strings`],
    /// not per query.
    ///
    /// ```
    /// # use verbora_trie::Trie;
    /// let mut t = Trie::new();
    /// t.add_strings(["cat", "cats", "car", "care", "careful"]);
    /// let frozen = t.freeze();
    /// assert!(frozen.contains("cats"));
    /// assert!(!frozen.contains("ca"));
    /// assert_eq!(frozen.keys_with_prefix("car"), t.keys_with_prefix("car"));
    /// ```
    #[must_use]
    pub fn freeze(&self) -> crate::frozen::FrozenTrie {
        let n = self.nodes.len();

        // Pass 1: which original nodes survive as real frozen nodes, and at
        // what index. `u32::MAX` marks "absorbed into some edge label".
        let mut new_index = vec![u32::MAX; n];
        let mut kept_count: u32 = 0;
        for (i, orig) in self.nodes.iter().enumerate() {
            let keep = i as u32 == ROOT || orig.is_word || orig.children.len() != 1;
            if keep {
                new_index[i] = kept_count;
                kept_count += 1;
            }
        }

        // Pass 2: for every kept node, walk each original child edge forward
        // through however many pass-through nodes it takes to reach the next
        // kept node, recording the whole run as one compressed edge.
        let mut frozen_nodes = Vec::with_capacity(kept_count as usize);
        let mut units: Vec<u16> = Vec::new();
        for (i, orig) in self.nodes.iter().enumerate() {
            if new_index[i] == u32::MAX {
                continue;
            }
            let mut children = SmallVec::new();
            for child in &orig.children {
                let label_start = units.len() as u32;
                units.push(child.key);
                let mut cur = child.node;
                while new_index[cur as usize] == u32::MAX {
                    // `cur` was not kept, so the keep check above guarantees
                    // it has exactly one child.
                    let only = self.nodes[cur as usize].children[0];
                    units.push(only.key);
                    cur = only.node;
                }
                let label_end = units.len() as u32;
                children.push(crate::frozen::FrozenChild {
                    label_start,
                    label_end,
                    node: new_index[cur as usize],
                });
            }
            frozen_nodes.push(crate::frozen::FrozenNode {
                children,
                is_word: orig.is_word,
            });
        }

        crate::frozen::FrozenTrie::from_parts(frozen_nodes, units, self.folds)
    }

    // -----------------------------------------------------------------------
    // Mutation
    // -----------------------------------------------------------------------

    /// Adds `string`, returning `true` if it was **already** present.
    ///
    /// The return value is the reference's, and it is the opposite of the
    /// "inserted?" convention most Rust set types use — `add_string` answers
    /// "was this already a word?", so a first insertion returns `false`.
    ///
    /// Adding the empty string marks the root as a word; it creates no node, so
    /// [`Trie::get_size`] does not change.
    ///
    /// ```
    /// # use verbora_trie::Trie;
    /// let mut t = Trie::new();
    /// assert!(!t.add_string("test"));
    /// assert!(t.add_string("test"));
    /// ```
    pub fn add_string(&mut self, string: &str) -> bool {
        let folded = fold(self.folds, string);
        let mut node = ROOT;
        // The reference re-lowercases the remaining suffix at every recursion
        // level. Folding is idempotent and the only context-sensitive rule
        // (Greek final sigma) cannot fire on already-folded text, so one pass
        // here is observably identical and linear instead of quadratic.
        for unit in folded.encode_utf16() {
            node = self.child_or_insert(node, unit);
        }
        std::mem::replace(&mut self.nodes[node as usize].is_word, true)
    }

    /// Adds every string in `list`.
    ///
    /// The reference iterates with `for…in`, which skips array holes and walks
    /// the *characters* of a string argument. A Rust iterator has neither
    /// behaviour, so this method is the faithful port of what that loop
    /// actually visits: pass the sequence you want inserted.
    ///
    /// ```
    /// # use verbora_trie::Trie;
    /// let mut t = Trie::new();
    /// t.add_strings(["alpha", "beta"]);
    /// t.add_strings(vec![String::from("gamma")]);
    /// assert_eq!(t.keys_with_prefix(""), ["alpha", "beta", "gamma"]);
    /// ```
    pub fn add_strings<I>(&mut self, list: I)
    where
        I: IntoIterator,
        I::Item: AsRef<str>,
    {
        let it = list.into_iter();
        // A word contributes at least one node unless it duplicates an existing
        // prefix entirely, so the item count is a safe lower bound: enough to
        // skip the first few doublings of a bulk load without over-reserving for
        // callers who pass one string at a time.
        self.nodes.reserve(it.size_hint().0);
        for s in it {
            self.add_string(s.as_ref());
        }
    }

    // -----------------------------------------------------------------------
    // Queries
    // -----------------------------------------------------------------------

    /// Whether `string` was added as a complete word.
    ///
    /// A stored word's proper prefixes are *not* contained unless they were
    /// added in their own right: with only `"tested"` stored, `contains("test")`
    /// is `false`.
    #[must_use]
    pub fn contains(&self, string: &str) -> bool {
        let folded = fold(self.folds, string);
        match self.descend(folded.encode_utf16()) {
            Some(node) => self.nodes[node as usize].is_word,
            None => false,
        }
    }

    /// The number of **nodes** from the root down, including the root itself.
    ///
    /// This counts tree nodes, not words: an empty trie is 1, adding `"a"` makes
    /// it 2, and adding `"ab"` afterwards makes it 3. Adding the empty string
    /// never changes it. A non-BMP character contributes **two** nodes, because
    /// The reference keys nodes by UTF-16 code unit:
    ///
    /// ```
    /// # use verbora_trie::Trie;
    /// let mut t = Trie::new();
    /// t.add_string("a👍");
    /// assert_eq!(t.get_size(), 4); // root + 'a' + high surrogate + low surrogate
    /// ```
    ///
    /// The reference traverses the whole structure on every call and warns
    /// against frequent use. Here the arena's length *is* the node count, so
    /// this is O(1) and the warning no longer applies.
    #[must_use]
    pub fn get_size(&self) -> usize {
        self.nodes.len()
    }

    /// Every stored word that starts with `prefix`, in the reference's order.
    ///
    /// # This method does not fold case — on purpose
    ///
    /// The reference guards its lowercasing with `if (this.caseSensitive ===
    /// false)`, but its constructor stores the flag as `this.cs`.
    /// `this.caseSensitive` is therefore always `undefined`, and the prefix is
    /// never folded — not even on a case-insensitive trie, where every stored
    /// word *has* been folded and an upper-case prefix consequently matches
    /// nothing:
    ///
    /// ```
    /// # use verbora_trie::Trie;
    /// let mut t = Trie::case_insensitive();
    /// t.add_strings(["thEIr", "And", "theY"]);
    /// assert_eq!(t.keys_with_prefix("th"), ["their", "they"]);
    /// assert!(t.keys_with_prefix("TH").is_empty()); // not a typo — see above
    /// ```
    ///
    /// Correcting this would silently change results for every caller who
    /// depends on the recorded behaviour, so the bug is preserved. Fold the
    /// prefix yourself if you want the intended semantics.
    ///
    /// # Order
    ///
    /// Depth-first, emitting a node's own word *before* descending, and visiting
    /// children in the reference object-key order: ASCII digits first in ascending
    /// numeric order, then every other key in insertion order.
    ///
    /// ```
    /// # use verbora_trie::Trie;
    /// let mut t = Trie::new();
    /// t.add_strings(["b1", "a1", "9x", "1x", "0x", "zz"]);
    /// assert_eq!(t.keys_with_prefix(""), ["0x", "1x", "9x", "b1", "a1", "zz"]);
    /// ```
    ///
    /// Use [`Trie::iter_keys_with_prefix`] to stream the same sequence without
    /// building the vector.
    #[must_use]
    pub fn keys_with_prefix(&self, prefix: &str) -> Vec<String> {
        self.iter_keys_with_prefix(prefix).collect()
    }

    /// [`Trie::keys_with_prefix`] as a lazy iterator.
    ///
    /// Yields the same strings in the same order while holding only one path
    /// buffer and one stack frame per level, so an early `take`/`find` stops
    /// paying for the rest of the subtree.
    #[must_use]
    pub fn iter_keys_with_prefix(&self, prefix: &str) -> KeysWithPrefix<'_> {
        // No folding here — see the note on `keys_with_prefix`.
        KeysWithPrefix::new(self, prefix)
    }

    /// Every stored word, in the reference's enumeration order.
    ///
    /// Equivalent to `iter_keys_with_prefix("")`.
    #[must_use]
    pub fn keys(&self) -> KeysWithPrefix<'_> {
        self.iter_keys_with_prefix("")
    }

    /// Every stored word that is a prefix of `search`, shortest first.
    ///
    /// ```
    /// # use verbora_trie::Trie;
    /// let mut t = Trie::new();
    /// t.add_strings(["a", "ab", "bc", "cd", "abc"]);
    /// assert_eq!(t.find_matches_on_path("abcd"), ["a", "ab", "abc"]);
    /// ```
    ///
    /// If the empty string was added it is the first result, since the root is
    /// on every path. Results are cut from `search` (after folding), not
    /// rebuilt from the stored keys, so on a case-sensitive trie they borrow
    /// rather than allocate.
    #[must_use]
    pub fn find_matches_on_path<'a>(&self, search: &'a str) -> Vec<Cow<'a, str>> {
        self.iter_matches_on_path(search).collect()
    }

    /// [`Trie::find_matches_on_path`] as a lazy iterator.
    ///
    /// The walk is linear and single-pass, so this stops the moment the caller
    /// does — useful when only the longest or the first match is wanted.
    #[must_use]
    pub fn iter_matches_on_path<'a>(&self, search: &'a str) -> MatchesOnPath<'_, 'a> {
        MatchesOnPath::new(self, fold(self.folds, search))
    }

    /// The longest stored word that prefixes `search`, and the part of `search`
    /// the walk could not consume.
    ///
    /// Mirrors the reference's two-element array `[lastWord, remainder]`:
    ///
    /// ```
    /// # use verbora_trie::Trie;
    /// # use std::borrow::Cow;
    /// let mut t = Trie::new();
    /// t.add_strings(["their", "and", "they"]);
    /// let (word, rest) = t.find_prefix("theyre");
    /// assert_eq!(word.as_deref(), Some("they"));
    /// assert_eq!(rest, "re");
    /// ```
    ///
    /// Two details are easy to get wrong:
    ///
    /// * The remainder is what was left when the **walk** died, not what was
    ///   left after the last word ended. With only `["their", "and"]` stored,
    ///   `find_prefix("theyre")` is `(None, "yre")` — the walk got as far as
    ///   `"the"`.
    /// * `Some("")` and `None` are different answers. A trie containing the
    ///   empty string returns `(Some(""), "zzz")` for `find_prefix("zzz")`,
    ///   so a `if let Some(w) = .. if !w.is_empty()` guard diverges.
    ///
    /// # Unpaired surrogates
    ///
    /// The walk advances one UTF-16 code unit at a time, so it can stop between
    /// the halves of a surrogate pair; the reference then returns a remainder
    /// beginning with an unpaired surrogate. Rust strings cannot represent one,
    /// so that single position becomes `U+FFFD`. The split itself is exact —
    /// [`Trie::find_prefix_lengths`] reports it losslessly.
    #[must_use]
    pub fn find_prefix<'a>(&self, search: &'a str) -> (Option<Cow<'a, str>>, Cow<'a, str>) {
        let folded = fold(self.folds, search);
        let walk = self.walk(&folded);

        // Reborrowing through the `Cow` keeps the case-sensitive path allocation
        // free: `search` outlives the return value, so its slices can be handed
        // straight back.
        let cut = |start: usize, end: usize| -> Cow<'a, str> {
            match &folded {
                Cow::Borrowed(s) => Cow::Borrowed(&s[start..end]),
                Cow::Owned(s) => Cow::Owned(s[start..end].to_owned()),
            }
        };

        let word = walk.last_word.map(|end| cut(0, end));
        let remainder = if walk.split_pair {
            // The dead unit is the low half of a 4-byte character; everything
            // after that character is intact.
            let mut s = String::with_capacity(3 + folded.len() - walk.stopped_at - 4);
            s.push(char::REPLACEMENT_CHARACTER);
            s.push_str(&folded[walk.stopped_at + 4..]);
            Cow::Owned(s)
        } else {
            cut(walk.stopped_at, folded.len())
        };
        (word, remainder)
    }

    /// The same split as [`Trie::find_prefix`], measured in UTF-16 code units.
    ///
    /// Returns `(length of the longest matching word, length of the remainder)`.
    /// Unlike [`Trie::find_prefix`] this is exact even when the walk stops
    /// inside a surrogate pair, and it allocates nothing at all.
    ///
    /// ```
    /// # use verbora_trie::Trie;
    /// let mut t = Trie::new();
    /// t.add_strings(["their", "and", "they"]);
    /// assert_eq!(t.find_prefix_lengths("theyre"), (Some(4), 2));
    /// ```
    #[must_use]
    pub fn find_prefix_lengths(&self, search: &str) -> (Option<usize>, usize) {
        let folded = fold(self.folds, search);
        let walk = self.walk(&folded);
        // Only the unconsumed tail needs measuring; when the walk stopped inside
        // a surrogate pair its high half is already gone, hence the -1.
        let tail = utf16_len(&folded[walk.stopped_at..]) - usize::from(walk.split_pair);
        let word = walk.last_word.is_some().then_some(walk.last_word_units);
        (word, tail)
    }

    // -----------------------------------------------------------------------
    // Internals
    // -----------------------------------------------------------------------

    /// Node at `node`. Kept crate-visible so the iterators can read the arena
    /// without exposing it.
    #[inline]
    pub(crate) fn node(&self, node: u32) -> &Node {
        &self.nodes[node as usize]
    }

    /// Follows the edge labelled `key`, if there is one.
    ///
    /// A linear scan, not a hash lookup: in a natural-language trie almost every
    /// node has one or two children, and scanning a contiguous inline array
    /// matches hashing plus a pointer chase at those sizes while costing nothing
    /// to build. The `contains_hit` and `contains_miss` benchmark groups measure
    /// this against a `HashMap`-per-node baseline.
    #[inline]
    pub(crate) fn child(&self, node: u32, key: u16) -> Option<u32> {
        self.nodes[node as usize]
            .children
            .iter()
            .find(|c| c.key == key)
            .map(|c| c.node)
    }

    /// Walks `units` from the root, returning the node reached or `None` if some
    /// edge is missing.
    #[inline]
    fn descend(&self, units: impl Iterator<Item = u16>) -> Option<u32> {
        let mut node = ROOT;
        for unit in units {
            node = self.child(node, unit)?;
        }
        Some(node)
    }

    /// Walks the code units of `prefix` from the root without folding.
    ///
    /// Used only by [`Trie::iter_keys_with_prefix`], which must not fold; see
    /// the note there.
    pub(crate) fn descend_exact(&self, prefix: &str) -> Option<u32> {
        self.descend(prefix.encode_utf16())
    }

    /// Adds the edge labelled `key` if it is missing, and returns the child.
    fn child_or_insert(&mut self, node: u32, key: u16) -> u32 {
        if let Some(existing) = self.child(node, key) {
            return existing;
        }
        let new = u32::try_from(self.nodes.len()).expect("trie exceeds 2^32 nodes");
        self.nodes.push(Node::default());
        let at = self.insert_position(node, key);
        self.nodes[node as usize]
            .children
            .insert(at, Child { key, node: new });
        new
    }

    /// The slot a new `key` takes so the child list stays in the reference's
    /// `for…in` order.
    ///
    /// Array-index-like keys (the ASCII digits) are hoisted into a
    /// numerically-sorted block at the front; everything else is appended, which
    /// is insertion order. Maintaining the invariant on write means iteration
    /// never has to sort — and iteration is the only place the order is
    /// observable.
    fn insert_position(&self, node: u32, key: u16) -> usize {
        let children = &self.nodes[node as usize].children;
        if !is_index_key(key) {
            return children.len();
        }
        children
            .iter()
            .position(|c| !is_index_key(c.key) || c.key > key)
            .unwrap_or(children.len())
    }

    /// The shared walk behind `find_prefix` and `find_prefix_lengths`.
    ///
    /// Iterates characters rather than code units so byte offsets stay in step,
    /// but consumes astral characters as the *two* units the reference sees, and
    /// reports which half the walk died on.
    fn walk(&self, folded: &str) -> Walk {
        let mut node = ROOT;
        let mut last_word = self.nodes[ROOT as usize].is_word.then_some(0usize);
        let mut last_word_units = 0usize;
        let mut units = 0usize;

        for (byte, ch) in folded.char_indices() {
            let mut buf = [0u16; 2];
            let encoded = ch.encode_utf16(&mut buf);
            let mut consumed = 0usize;
            for &unit in &*encoded {
                match self.child(node, unit) {
                    Some(next) => {
                        node = next;
                        consumed += 1;
                    }
                    None => {
                        return Walk {
                            last_word,
                            last_word_units,
                            stopped_at: byte,
                            // Dying on the second unit means the remainder opens
                            // with an unpaired low surrogate.
                            split_pair: consumed == 1,
                        };
                    }
                }
            }
            units += consumed;
            // A node reached between two surrogate halves can never be a word:
            // every stored word came from a `&str` and so is well-formed. That
            // makes checking only at character boundaries exact, not an
            // approximation.
            if self.nodes[node as usize].is_word {
                last_word = Some(byte + ch.len_utf8());
                last_word_units = units;
            }
        }

        Walk {
            last_word,
            last_word_units,
            stopped_at: folded.len(),
            split_pair: false,
        }
    }
}

/// The reference's `String#length`: the UTF-16 code-unit count of `s`.
#[inline]
fn utf16_len(s: &str) -> usize {
    if s.is_ascii() {
        return s.len();
    }
    s.chars().map(char::len_utf16).sum()
}

impl<S: AsRef<str>> Extend<S> for Trie {
    fn extend<I: IntoIterator<Item = S>>(&mut self, iter: I) {
        self.add_strings(iter);
    }
}

impl<S: AsRef<str>> FromIterator<S> for Trie {
    /// Builds a **case-sensitive** trie, matching `new Trie()`.
    ///
    /// For the folding variant, build with [`Trie::case_insensitive`] and
    /// [`Trie::add_strings`].
    fn from_iter<I: IntoIterator<Item = S>>(iter: I) -> Self {
        let mut t = Self::new();
        t.add_strings(iter);
        t
    }
}

impl<'a> IntoIterator for &'a Trie {
    type Item = String;
    type IntoIter = KeysWithPrefix<'a>;

    /// Iterates every stored word, in the reference's enumeration order.
    ///
    /// Each item is an owned `String` because a stored word exists nowhere
    /// contiguously — it is spelled out one code unit per node — so it has to be
    /// materialised. That cost is visible in the item type rather than hidden:
    /// the iterator itself allocates only one reusable path buffer and one
    /// stack, both proportional to the depth, not the number of words.
    fn into_iter(self) -> Self::IntoIter {
        self.keys()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_inline_child_capacity_is_free() {
        // The `union` representation reserves `max(size_of::<[Child; N]>(), 16)`
        // bytes whatever `N` is, so capacity 2 costs exactly what a bare `Vec`
        // would while keeping one- and two-child nodes off the heap entirely.
        // Raising `INLINE_CHILDREN` to 4 would grow every node by 16 bytes.
        assert_eq!(INLINE_CHILDREN, 2);
        assert_eq!(
            std::mem::size_of::<SmallVec<[Child; INLINE_CHILDREN]>>(),
            std::mem::size_of::<Vec<Child>>()
        );
        assert_eq!(std::mem::size_of::<Node>(), 32);
    }

    #[test]
    fn index_keys_are_exactly_the_ascii_digits() {
        for u in 0u16..=0xFF {
            assert_eq!(
                is_index_key(u),
                (b'0' as u16..=b'9' as u16).contains(&u),
                "for {u:#06x}"
            );
        }
        // '٣' ARABIC-INDIC DIGIT THREE is a digit but not a canonical index.
        assert!(!is_index_key(0x0663));
    }

    #[test]
    fn fold_borrows_when_nothing_changes() {
        assert!(matches!(fold(false, "ABC"), Cow::Borrowed(_)));
        assert!(matches!(fold(true, "abc"), Cow::Borrowed(_)));
        assert!(matches!(fold(true, "ABC"), Cow::Owned(_)));
        assert_eq!(fold(true, "ÜBER"), "über");
    }

    #[test]
    fn fold_matches_the_reference_on_expanding_and_contextual_cases() {
        // 'İ' lowercases to two code points in both languages.
        assert_eq!(fold(true, "İ"), "i\u{307}");
        // Greek final sigma is context sensitive; both languages apply it.
        assert_eq!(fold(true, "ΟΣ"), "ος");
        assert_eq!(fold(true, "Σ"), "σ");
    }

    #[test]
    fn utf16_len_counts_code_units() {
        for s in ["", "abc", "café", "Москва", "😀", "a😀b", "𝕳𝖊𝖑𝖑𝖔"] {
            assert_eq!(utf16_len(s), s.encode_utf16().count(), "for {s:?}");
        }
    }

    #[test]
    fn digit_children_are_hoisted_and_sorted() {
        let mut t = Trie::new();
        t.add_strings(["zb", "z9", "za", "z0", "z1"]);
        assert_eq!(t.keys_with_prefix("z"), ["z0", "z1", "z9", "zb", "za"]);
    }

    // -----------------------------------------------------------------------
    // Structural invariants
    // -----------------------------------------------------------------------

    #[test]
    fn a_fresh_trie_has_one_node_and_contains_nothing() {
        let t = Trie::new();
        assert_eq!(t.get_size(), 1);
        assert!(!t.contains(""));
        assert!(!t.contains("a"));
        assert!(t.keys_with_prefix("").is_empty());
        assert!(t.find_matches_on_path("abc").is_empty());
        let (word, rest) = t.find_prefix("abc");
        assert_eq!(word, None);
        assert_eq!(rest, "abc");
    }

    #[test]
    fn every_node_lists_each_key_once() {
        let mut t = Trie::new();
        t.add_strings(["ab", "ab", "ac", "a", "b", "ab"]);
        // 'a' must not sprout a duplicate 'b' edge on the repeated insertions.
        assert_eq!(t.get_size(), 5); // root + a + b(under a) + c(under a) + b
        assert_eq!(t.keys_with_prefix(""), ["a", "ab", "ac", "b"]);
    }

    // -----------------------------------------------------------------------
    // The empty string is a word like any other
    // -----------------------------------------------------------------------

    #[test]
    fn empty_string_is_a_word_that_creates_no_node() {
        let mut t = Trie::new();
        assert!(!t.add_string(""));
        assert!(t.add_string(""));
        assert_eq!(t.get_size(), 1, "the root already existed");
        assert!(t.contains(""));
        assert_eq!(t.keys_with_prefix(""), [""]);
        assert_eq!(t.find_matches_on_path("abc"), [""]);
        // `Some("")` is a real answer, distinct from `None`.
        let (word, rest) = t.find_prefix("zzz");
        assert_eq!(word.as_deref(), Some(""));
        assert_eq!(rest, "zzz");
        assert_eq!(t.find_prefix_lengths("zzz"), (Some(0), 3));
    }

    // -----------------------------------------------------------------------
    // Character-class coverage
    // -----------------------------------------------------------------------

    #[test]
    fn single_character_words() {
        let mut t = Trie::new();
        t.add_strings(["a", "z", "0", "!"]);
        assert_eq!(t.get_size(), 5);
        for w in ["a", "z", "0", "!"] {
            assert!(t.contains(w), "{w:?}");
        }
        // '0' is array-index-like and so sorts ahead of the insertion order.
        assert_eq!(t.keys_with_prefix(""), ["0", "a", "z", "!"]);
    }

    #[test]
    fn all_uppercase_input() {
        let mut sensitive = Trie::new();
        sensitive.add_string("ALLCAPS");
        assert!(sensitive.contains("ALLCAPS"));
        assert!(!sensitive.contains("allcaps"));

        let mut folding = Trie::case_insensitive();
        folding.add_string("ALLCAPS");
        assert!(folding.contains("allcaps"));
        assert!(folding.contains("AllCaps"));
        // Stored folded, and `keys_with_prefix` does not fold its argument.
        assert_eq!(folding.keys_with_prefix("all"), ["allcaps"]);
        assert!(folding.keys_with_prefix("ALL").is_empty());
    }

    #[test]
    fn accented_latin() {
        let mut t = Trie::case_insensitive();
        t.add_strings(["café", "CAFÉ", "Ångström", "crème brûlée", "straße"]);
        assert!(t.contains("CafÉ"));
        assert!(t.contains("ångström"));
        assert!(t.contains("CRÈME BRÛLÉE"));
        // 'ß' has no single-character uppercase, so "STRASSE" is a different word.
        assert!(t.contains("straße"));
        assert!(!t.contains("strasse"));
        // Each accented character is one BMP code unit, hence one node.
        let mut plain = Trie::new();
        plain.add_string("café");
        assert_eq!(plain.get_size(), 5);
    }

    #[test]
    fn cyrillic_and_greek() {
        let mut t = Trie::new();
        t.add_strings(["Москва", "Москвич", "Ελλάδα", "Ελλάς"]);
        assert!(t.contains("Москва"));
        assert!(!t.contains("Москв"));
        assert_eq!(t.keys_with_prefix("Москв"), ["Москва", "Москвич"]);
        assert_eq!(t.find_matches_on_path("Ελλάδας"), ["Ελλάδα"]);

        // Greek final sigma: folding is context-sensitive in both languages.
        let mut folding = Trie::case_insensitive();
        folding.add_string("ΟΣ");
        assert_eq!(folding.keys_with_prefix(""), ["ος"]);
        assert!(folding.contains("ΟΣ"));
        assert!(folding.contains("ος"));
    }

    #[test]
    fn cjk() {
        let mut t = Trie::new();
        t.add_strings(["日本語", "日本", "中文测试", "한국어"]);
        assert_eq!(t.find_matches_on_path("日本語です"), ["日本", "日本語"]);
        assert_eq!(t.keys_with_prefix("日本"), ["日本", "日本語"]);
        // Every CJK character here is in the BMP: one code unit, one node.
        let mut one = Trie::new();
        one.add_string("日本語");
        assert_eq!(one.get_size(), 4);
    }

    #[test]
    fn astral_characters_occupy_two_nodes_each() {
        let mut t = Trie::new();
        t.add_string("😀");
        assert_eq!(t.get_size(), 3, "root + high surrogate + low surrogate");
        assert!(t.contains("😀"));
        assert_eq!(t.keys_with_prefix(""), ["😀"]);

        let mut many = Trie::new();
        many.add_strings(["a👍", "a👍b", "𝕳𝖊𝖑𝖑𝖔"]);
        assert_eq!(many.keys_with_prefix("a"), ["a👍", "a👍b"]);
        assert_eq!(many.find_matches_on_path("a👍bc"), ["a👍", "a👍b"]);
        assert!(many.contains("𝕳𝖊𝖑𝖑𝖔"));
    }

    #[test]
    fn a_walk_can_die_between_surrogate_halves() {
        let mut t = Trie::new();
        t.add_string("a👍"); // U+1F44D = D83D DC4D

        // U+1F44C = D83D DC4C shares the high surrogate but not the low one, so
        // the reference consumes three of the four code units and hands back a
        // remainder that begins with an unpaired surrogate.
        let (word, rest) = t.find_prefix("a👌");
        assert_eq!(word, None);
        assert_eq!(rest, "\u{FFFD}");
        // The split itself is exact: three units in, one unit left.
        assert_eq!(t.find_prefix_lengths("a👌"), (None, 1));

        // '😀' U+1F600 = D83D DE00 shares the same high surrogate, so it dies the
        // same way — one code unit of the two-unit character survives.
        let (_, rest) = t.find_prefix("a😀x");
        assert_eq!(rest, "\u{FFFD}x");
        assert_eq!(t.find_prefix_lengths("a😀x"), (None, 2));

        // '𝕳' U+1D573 = D835 DD73 differs in the *first* half, so the walk dies
        // cleanly on a character boundary and the remainder is ordinary text.
        let (_, rest) = t.find_prefix("a𝕳x");
        assert_eq!(rest, "𝕳x");
        assert_eq!(t.find_prefix_lengths("a𝕳x"), (None, 3));
    }

    #[test]
    fn punctuation_and_whitespace() {
        let mut t = Trie::new();
        t.add_strings(["e.g.", "e.g", "U.S.A.", "don't", "--", "a-b", "  double  "]);
        assert!(t.contains("e.g."));
        assert_eq!(t.find_matches_on_path("e.g.!"), ["e.g", "e.g."]);
        assert!(t.contains("  double  "));
        assert_eq!(t.keys_with_prefix("e.g"), ["e.g", "e.g."]);
        // Whitespace is an ordinary code unit here: nothing is trimmed.
        assert!(!t.contains("double"));
    }

    #[test]
    fn digits_and_numeric_looking_words() {
        let mut t = Trie::new();
        t.add_strings(["123", "3.14", "1,000", "12", "1"]);
        assert_eq!(t.find_matches_on_path("1234"), ["1", "12", "123"]);
        // The whole first level is digits, so it enumerates in numeric order
        // regardless of when each was inserted.
        assert_eq!(t.keys_with_prefix(""), ["1", "12", "123", "1,000", "3.14"]);
    }

    #[test]
    fn very_long_input_does_not_overflow_the_stack() {
        // The reference recurses once per code unit and would need ~200k frames
        // for this; every operation here is a loop.
        let long: String = "ab".repeat(100_000);
        let mut t = Trie::new();
        assert!(!t.add_string(&long));
        assert_eq!(t.get_size(), long.len() + 1);
        assert!(t.contains(&long));
        assert!(!t.contains(&long[..long.len() - 1]));

        let extended = format!("{long}tail");
        let (word, rest) = t.find_prefix(&extended);
        assert_eq!(word.as_deref(), Some(long.as_str()));
        assert_eq!(rest, "tail");
        assert_eq!(t.keys_with_prefix(&long[..1000]).len(), 1);
        assert_eq!(t.keys().count(), 1);
    }

    // -----------------------------------------------------------------------
    // Case sensitivity
    // -----------------------------------------------------------------------

    #[test]
    fn case_sensitive_is_the_default() {
        assert!(Trie::new().is_case_sensitive());
        assert!(Trie::default().is_case_sensitive());
        assert!(Trie::with_case_sensitivity(true).is_case_sensitive());
        assert!(!Trie::case_insensitive().is_case_sensitive());
    }

    #[test]
    fn folding_applies_to_every_method_except_keys_with_prefix() {
        let mut t = Trie::case_insensitive();
        t.add_strings(["thEIr", "And", "theY"]);

        assert!(t.contains("THEIR"));
        assert_eq!(t.find_matches_on_path("THEYRE"), ["they"]);
        let (word, rest) = t.find_prefix("ThEyRe");
        assert_eq!(word.as_deref(), Some("they"));
        assert_eq!(rest, "re");

        // The reference's bug: `keysWithPrefix` reads `this.caseSensitive`,
        // which the constructor never sets.
        assert_eq!(t.keys_with_prefix("th"), ["their", "they"]);
        assert!(t.keys_with_prefix("TH").is_empty());
    }

    #[test]
    fn folding_can_change_a_words_length() {
        // 'İ' U+0130 lowercases to two code points, so the folded word is longer
        // than the input and occupies one extra node.
        let mut t = Trie::case_insensitive();
        t.add_string("İ");
        assert_eq!(t.keys_with_prefix(""), ["i\u{307}"]);
        assert_eq!(t.get_size(), 3);
        assert!(t.contains("İ"));
        assert!(t.contains("i\u{307}"));
    }

    // -----------------------------------------------------------------------
    // Return-shape details that are easy to get wrong
    // -----------------------------------------------------------------------

    #[test]
    fn find_prefix_reports_where_the_walk_died_not_where_the_word_ended() {
        let mut t = Trie::new();
        t.add_strings(["their", "and"]);
        // The walk reaches "the" before failing, so three characters are gone
        // even though no word ever matched.
        let (word, rest) = t.find_prefix("theyre");
        assert_eq!(word, None);
        assert_eq!(rest, "yre");
        assert_eq!(t.find_prefix_lengths("theyre"), (None, 3));
    }

    #[test]
    fn find_prefix_on_an_exact_hit_leaves_nothing() {
        let mut t = Trie::new();
        t.add_string("ab");
        let (word, rest) = t.find_prefix("ab");
        assert_eq!(word.as_deref(), Some("ab"));
        assert_eq!(rest, "");
        assert_eq!(t.find_prefix_lengths("ab"), (Some(2), 0));
    }

    #[test]
    fn find_prefix_borrows_when_no_folding_is_needed() {
        let mut t = Trie::new();
        t.add_string("ab");
        let (word, rest) = t.find_prefix("abcd");
        assert!(matches!(word, Some(Cow::Borrowed(_))));
        assert!(matches!(rest, Cow::Borrowed(_)));

        let mut folding = Trie::case_insensitive();
        folding.add_string("ab");
        let (word, _) = folding.find_prefix("ABCD");
        assert!(matches!(word, Some(Cow::Owned(_))));
    }

    #[test]
    fn matches_on_path_borrows_when_no_folding_is_needed() {
        let mut t = Trie::new();
        t.add_strings(["a", "ab"]);
        let got = t.find_matches_on_path("abc");
        assert!(got.iter().all(|m| matches!(m, Cow::Borrowed(_))));
    }

    #[test]
    fn prefixes_and_extensions_of_a_stored_word_are_not_contained() {
        let mut t = Trie::new();
        t.add_strings(["test", "tested"]);
        assert!(t.contains("test"));
        assert!(!t.contains("tes"));
        assert!(!t.contains("teste"));
        assert!(!t.contains("est"));
    }

    #[test]
    fn prototype_polluting_keys_are_ordinary_words() {
        // The reference's node map is `Object.create(null)`, so these are safe
        // there too; a Rust port has no way to get this wrong, which is exactly
        // why it is worth pinning.
        let mut t = Trie::new();
        t.add_strings(["__proto__", "constructor", "toString"]);
        assert!(t.contains("__proto__"));
        assert!(t.contains("constructor"));
        assert!(!t.contains("valueOf"));
        assert_eq!(t.get_size(), 1 + 9 + 11 + 8);
    }

    // -----------------------------------------------------------------------
    // Iterator surface
    // -----------------------------------------------------------------------

    #[test]
    fn iterators_agree_with_their_eager_counterparts() {
        let mut t = Trie::new();
        t.add_strings(["a", "ab", "abc", "b", "0z", "9z", "😀", "日本"]);
        assert_eq!(
            t.keys_with_prefix("a"),
            t.iter_keys_with_prefix("a").collect::<Vec<_>>()
        );
        assert_eq!(t.keys_with_prefix(""), t.keys().collect::<Vec<_>>());
        assert_eq!(t.keys_with_prefix(""), (&t).into_iter().collect::<Vec<_>>());
        assert_eq!(
            t.find_matches_on_path("abcd"),
            t.iter_matches_on_path("abcd").collect::<Vec<_>>()
        );
    }

    #[test]
    fn iterators_stop_early_and_stay_fused() {
        let mut t = Trie::new();
        t.add_strings(["a", "ab", "abc", "abcd"]);
        assert_eq!(t.keys().take(2).collect::<Vec<_>>(), ["a", "ab"]);

        let mut keys = t.iter_keys_with_prefix("zz");
        assert_eq!(keys.next(), None);
        assert_eq!(keys.next(), None);

        let mut path = t.iter_matches_on_path("zz");
        assert_eq!(path.next(), None);
        assert_eq!(path.next(), None);
    }

    #[test]
    fn from_iterator_and_extend_build_case_sensitive_tries() {
        let mut t: Trie = ["a", "ab"].into_iter().collect();
        assert!(t.is_case_sensitive());
        t.extend(["b", "bc"]);
        assert_eq!(t.keys_with_prefix(""), ["a", "ab", "b", "bc"]);
    }

    #[test]
    fn add_strings_accepts_owned_and_borrowed_items() {
        let owned = vec![String::from("alpha"), String::from("beta")];
        let mut t = Trie::new();
        t.add_strings(&owned);
        t.add_strings(owned.iter().map(String::as_str));
        t.add_strings(["gamma"]);
        assert_eq!(t.keys_with_prefix(""), ["alpha", "beta", "gamma"]);
    }

    #[test]
    fn clone_preserves_enumeration_order() {
        let mut t = Trie::case_insensitive();
        t.add_strings(["b1", "a1", "9x", "1x", "0x", "zz"]);
        let copy = t.clone();
        assert_eq!(t, copy);
        assert_eq!(copy.keys_with_prefix(""), t.keys_with_prefix(""));
        assert!(!copy.is_case_sensitive());
    }
}
