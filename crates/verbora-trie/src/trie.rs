//! The trie itself: arena storage, the scalar walk, and the public operations
//! built on it.

use std::borrow::Cow;

use smallvec::SmallVec;

use crate::iter::{KeysWithPrefix, PrefixMatches};
use crate::membership::HashIndex;

/// Arena index of the root node.
pub(crate) const ROOT: u32 = 0;

/// Inline child capacity.
///
/// Two is not arbitrary: with the `union` representation a `SmallVec` reserves
/// `max(size_of::<[T; N]>(), 16)` bytes regardless, so `N = 2` for an 8-byte
/// [`Child`] is the largest inline capacity that is *free*. In a
/// natural-language trie the overwhelming majority of nodes have one child, so
/// this keeps the heap out of the picture for everything below the first level
/// or two.
const INLINE_CHILDREN: usize = 2;

/// The child-list length at which lookup switches from a linear scan to a
/// binary search.
///
/// Below it the scan wins: the list is one cache line, already loaded, and the
/// branch predictor sees a short fixed trip count. Above it the comparisons
/// dominate. Both are correct at any size — the list is kept sorted either way
/// — so this constant is a speed knob, never a semantic one.
const BINARY_SEARCH_THRESHOLD: usize = 8;

/// How a trie treats letter case.
///
/// Case handling is fixed when the trie is constructed and then applies to
/// **every** argument of **every** method, with no exceptions: what is stored,
/// what [`Trie::contains`] is asked, and the prefix or search string handed to
/// any query. That uniformity is the contract — a method that folded some of
/// its arguments and not others would make results depend on which entry point
/// a caller happened to use.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CaseHandling {
    /// Words and arguments are compared exactly, scalar for scalar. The
    /// default, and the only mode in which a trie never rewrites its input.
    Sensitive,
    /// Words and arguments are lowercased with [`str::to_lowercase`] — full,
    /// locale-independent Unicode `Lowercase_Mapping` (Unicode 16.0 §5.18,
    /// `SpecialCasing.txt` included), so `'İ'` becomes the two scalars
    /// `"i\u{307}"` and a Greek capital sigma becomes `'ς'` or `'σ'`
    /// according to position.
    ///
    /// Folding is a *declared* transformation, not a silent one: it is chosen
    /// at construction, reported by [`Trie::case_handling`], and applied to
    /// every argument alike. Stored keys come back in their folded spelling,
    /// because that is what was stored.
    Folded,
}

/// One edge: the Unicode scalar that labels it and the child it leads to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Child {
    /// The edge label: exactly one Unicode scalar value.
    pub(crate) key: char,
    /// Arena index of the node this edge leads to.
    pub(crate) node: u32,
}

/// A node in the arena.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Node {
    /// Outgoing edges, kept sorted ascending by [`Child::key`] so that
    /// enumeration is a straight scan and lookup can binary-search.
    pub(crate) children: SmallVec<[Child; INLINE_CHILDREN]>,
    /// Whether a stored word ends at this node.
    pub(crate) is_word: bool,
    /// The number of stored words in this node's subtree, counting the node's
    /// own word.
    ///
    /// Not observable directly — it exists so a prefix count (`descend` +
    /// read) is O(|prefix|) instead of a subtree walk, which is what turns
    /// `iter_keys_with_prefix(p).count()` from ~1 ms into ~0.1 µs on a 20k
    /// word corpus (see [`KeysWithPrefix`]'s `count` override), and what makes
    /// [`Trie::len`] O(1). It occupies bytes that were already padding:
    /// `Node` stays 32 bytes, pinned by the `the_inline_child_capacity_is_free`
    /// test below. Maintained optimistically by [`Trie::insert`] —
    /// incremented while descending, rolled back by a re-walk only when the
    /// word turns out to be a duplicate — because duplicates are rare in real
    /// word lists and the non-duplicate path then needs no path buffer at all.
    pub(crate) word_count: u32,
}

/// Lowercases `s` when `handling` says to.
///
/// Returns a borrow when nothing changes, which is the common case for
/// already-folded corpora and for every call on a case-sensitive trie.
pub(crate) fn fold(handling: CaseHandling, s: &str) -> Cow<'_, str> {
    if handling == CaseHandling::Sensitive {
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

/// Where a walk stopped, in enough detail to answer both prefix queries.
struct Walk {
    /// Byte offset, in the *folded* search string, one past the end of the
    /// longest stored word on the path. `None` when no node on the path was a
    /// stored word.
    last_word: Option<usize>,
    /// Scalar count of that word, tracked alongside so callers that only want
    /// the split need not re-measure.
    last_word_scalars: usize,
    /// Byte offset of the scalar at which the walk stopped. Always a character
    /// boundary of the folded search string.
    stopped_at: usize,
}

/// The longest stored word that prefixes a search string, and the part of the
/// search the walk could not consume.
///
/// Returned by [`Trie::longest_prefix`]. Both fields are cut from the search
/// string itself (after folding), so on a case-sensitive trie they borrow it
/// and the whole operation allocates nothing.
///
/// `word` is `None` when no node on the path was a stored word.
/// `Some("")` — the trie stores the empty string — is a *different* answer
/// from `None`, and a `if let Some(w) = .. if !w.is_empty()` guard conflates
/// them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrefixSplit<'a> {
    /// The longest stored word that is a prefix of the search string.
    pub word: Option<Cow<'a, str>>,
    /// What was left when the **walk** died — not what was left after `word`
    /// ended. With only `["their", "and"]` stored, searching `"theyre"` walks
    /// as far as `"the"` and leaves `"yre"`, with `word` still `None`.
    pub rest: Cow<'a, str>,
}

/// The same split as [`PrefixSplit`], counted in Unicode scalars.
///
/// Returned by [`Trie::longest_prefix_lengths`], which allocates nothing at
/// all.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PrefixSplitLengths {
    /// Scalar count of the longest stored word prefixing the search, or `None`
    /// when there is none.
    pub word: Option<usize>,
    /// Scalar count of the unconsumed remainder.
    pub rest: usize,
}

/// A prefix tree over `&str` keys.
///
/// # The unit
///
/// **One Unicode scalar value is one node label.** A key is a Rust `&str`, so
/// the smallest piece of it that is itself text is a scalar; keying nodes by
/// anything smaller (UTF-8 bytes, UTF-16 code units) puts positions in the tree
/// that no `&str` can name, and makes a walk able to stop somewhere that is not
/// a character boundary. With the scalar unit every node in the tree is a
/// position a caller can hand in and get back:
///
/// * `'😀'` is one node, not two, and costs one position in every length this
///   crate reports;
/// * every remainder [`Trie::longest_prefix`] returns is a suffix of the
///   caller's own text — no `U+FFFD`, no invented scalar, ever;
/// * [`Trie::node_count`] counts scalars plus the root.
///
/// The unit is *not* the grapheme cluster. `"e\u{301}"` and `"é"` are two
/// different keys here, as they are for `str` equality itself, and normalising
/// them together is the caller's explicit choice — see `verbora-normalizers`.
///
/// # Order
///
/// Every enumeration this crate offers — [`Trie::keys`],
/// [`Trie::keys_with_prefix`], [`Trie::iter_keys_with_prefix`],
/// [`Trie::for_each_key_with_prefix`] and the [`FrozenTrie`](crate::FrozenTrie)
/// counterparts — yields words in **ascending order of their scalar sequence**.
/// For well-formed Rust strings that is byte-wise UTF-8 order, so it is exactly
/// `<str as Ord>`:
///
/// ```
/// use verbora_trie::Trie;
///
/// let mut t = Trie::new();
/// t.insert_all(["cat", "0x", "car", "Ångström", "日本", "😀"]);
///
/// let mut expected = ["cat", "0x", "car", "Ångström", "日本", "😀"];
/// expected.sort_unstable();
/// assert_eq!(t.keys_with_prefix(""), expected);
/// ```
///
/// A node's own word is emitted before its subtree, which is not an extra rule
/// but a consequence of the one above: a node's word is a proper prefix of
/// every word beneath it, and a proper prefix sorts first.
///
/// # Construction
///
/// [`Trie::new`] for the case-sensitive default, [`Trie::case_insensitive`] for
/// the folding variant, or [`Trie::with_case_handling`] to pick explicitly.
///
/// ```
/// use verbora_trie::Trie;
///
/// let mut t = Trie::new();
/// t.insert_all(["a", "ab", "bc", "cd", "abc"]);
///
/// assert!(t.contains("abc"));
/// assert_eq!(t.prefix_matches("abcd"), ["a", "ab", "abc"]);
/// assert_eq!(t.keys_with_prefix("ab"), ["ab", "abc"]);
/// ```
///
/// # Panics
///
/// Never, for any input, except on capacity overflow: a trie holding more than
/// `u32::MAX` nodes, or a membership index holding more than `u32::MAX`
/// distinct words, aborts the insertion with a panic the same way `Vec::push`
/// panics when it cannot grow. Both bounds need hundreds of gigabytes of live
/// keys to reach.
#[derive(Clone, Debug)]
pub struct Trie {
    /// All nodes, flat. Index 0 is the root and is never removed, so a `u32`
    /// index is always valid and needs no `Option`.
    nodes: Vec<Node>,
    handling: CaseHandling,
    /// Hash membership set over the stored words, maintained by
    /// [`Trie::insert`] and consulted by [`Trie::contains`] — see
    /// `membership.rs` for why it answers exactly what the arena walk did.
    index: HashIndex,
}

/// Equality deliberately ignores the membership index: the index's internal
/// layout records the order in which distinct words were *first* inserted,
/// which no query can observe — `["ab", "a"]` and `["a", "ab"]` build
/// byte-identical arenas (and answer every method identically) but opposite
/// index blobs. Comparing `nodes` (whose `word_count`s are fully determined by
/// the `is_word` flags) and `handling` is exactly the observable state.
impl PartialEq for Trie {
    fn eq(&self, other: &Self) -> bool {
        self.nodes == other.nodes && self.handling == other.handling
    }
}

impl Eq for Trie {}

impl Default for Trie {
    fn default() -> Self {
        Self::new()
    }
}

impl Trie {
    /// An empty, case-sensitive trie.
    #[must_use]
    pub fn new() -> Self {
        Self::with_case_handling(CaseHandling::Sensitive)
    }

    /// An empty trie that lowercases everything it stores and every argument
    /// it is given — see [`CaseHandling::Folded`].
    #[must_use]
    pub fn case_insensitive() -> Self {
        Self::with_case_handling(CaseHandling::Folded)
    }

    /// An empty trie with the given case handling.
    #[must_use]
    pub fn with_case_handling(handling: CaseHandling) -> Self {
        Self {
            nodes: vec![Node::default()],
            handling,
            index: HashIndex::new(),
        }
    }

    /// How this trie treats letter case.
    #[must_use]
    pub fn case_handling(&self) -> CaseHandling {
        self.handling
    }

    /// Reserves capacity for at least `additional` more nodes.
    ///
    /// A trie built from a known word list needs roughly one node per distinct
    /// prefix; reserving up front removes the arena's growth reallocations from
    /// a bulk load.
    pub fn reserve(&mut self, additional: usize) {
        self.nodes.reserve(additional);
    }

    // -----------------------------------------------------------------------
    // Mutation
    // -----------------------------------------------------------------------

    /// Inserts `string`, returning `true` when it was **not** already stored.
    ///
    /// The convention is `HashSet::insert`'s: `true` means "this call added
    /// it".
    ///
    /// Inserting the empty string marks the root as a word; it creates no node,
    /// so [`Trie::node_count`] does not change, but [`Trie::len`] does.
    ///
    /// Besides the arena path, each insertion maintains two query
    /// accelerators: the per-node subtree word counts (behind [`Trie::len`] and
    /// [`KeysWithPrefix`]'s O(1) `count`) and the hash membership set behind
    /// [`Trie::contains`]. Together they add roughly a third to a bulk load —
    /// a trade against `contains` running ~2× faster and prefix counts
    /// ~10,000× faster, disclosed here rather than hidden.
    ///
    /// ```
    /// # use verbora_trie::Trie;
    /// let mut t = Trie::new();
    /// assert!(t.insert("test"));
    /// assert!(!t.insert("test"));
    /// ```
    pub fn insert(&mut self, string: &str) -> bool {
        let folded = fold(self.handling, string);
        let mut node = ROOT;
        // Subtree word counts are bumped optimistically on the way down — see
        // `Node::word_count` for why the rollback below is the cheap branch.
        self.nodes[ROOT as usize].word_count += 1;
        for scalar in folded.chars() {
            node = self.child_or_insert(node, scalar);
            self.nodes[node as usize].word_count += 1;
        }
        let was_word = std::mem::replace(&mut self.nodes[node as usize].is_word, true);
        if was_word {
            // A duplicate stores nothing, so the optimistic increments were
            // wrong; re-walk the (now guaranteed present) path to undo them.
            let mut n = ROOT;
            self.nodes[ROOT as usize].word_count -= 1;
            for scalar in folded.chars() {
                // Present by construction: the forward walk above used
                // `child_or_insert` over this same `folded` string, which
                // creates any missing edge, and edges are only ever added —
                // never removed or relabelled — so the whole path exists.
                n = self.child(n, scalar).expect("path was just walked");
                self.nodes[n as usize].word_count -= 1;
            }
        } else {
            self.index.insert_folded(folded.as_bytes());
        }
        !was_word
    }

    /// Inserts every string in `list`.
    ///
    /// The iterator's `size_hint` is used to pre-size, but only as a hint: an
    /// iterator that overstates its length costs a few growth reallocations
    /// during the load and nothing else.
    ///
    /// ```
    /// # use verbora_trie::Trie;
    /// let mut t = Trie::new();
    /// t.insert_all(["alpha", "beta"]);
    /// t.insert_all(vec![String::from("gamma")]);
    /// assert_eq!(t.keys_with_prefix(""), ["alpha", "beta", "gamma"]);
    /// ```
    pub fn insert_all<I>(&mut self, list: I)
    where
        I: IntoIterator,
        I::Item: AsRef<str>,
    {
        let it = list.into_iter();
        // A word contributes at least one node unless it duplicates an existing
        // prefix entirely, so the item count is a good lower bound: enough to
        // skip the first few doublings of a bulk load without over-reserving for
        // callers who pass one string at a time.
        //
        // It is a *hint* and not a bound, though — `Iterator::size_hint` is
        // explicitly not a correctness contract, and a safe iterator may report
        // `usize::MAX`. Clamping to what the trie could *index* is not enough:
        // `u32::MAX` nodes is tens of gigabytes of real allocation, so an
        // overstated hint would trade a fast panic for exhausting the machine,
        // which is strictly worse. A hint only ever buys the first few
        // doublings — `Vec` growth is amortised past that — so it is clamped to
        // a size whose reservation is cheap whether or not the hint was honest.
        const MAX_HINT: usize = 4096;
        let hint = it.size_hint().0.min(MAX_HINT);
        // Fallible even after the clamp: an honest hint of a billion words is
        // in range and still larger than some machines can hand out, and a
        // pre-size that cannot be honoured must degrade to ordinary growth
        // rather than abort a load that would otherwise have fit.
        let _ = self.nodes.try_reserve(hint);
        // Pre-slotting the membership table the same way spares the bulk load
        // its incremental rehashes; key bytes stay amortized (their total
        // length is unknowable from a count).
        self.index.reserve(hint, 0);
        for s in it {
            self.insert(s.as_ref());
        }
    }

    // -----------------------------------------------------------------------
    // Queries
    // -----------------------------------------------------------------------

    /// Whether `string` was inserted as a complete word.
    ///
    /// A stored word's proper prefixes are *not* contained unless they were
    /// inserted in their own right: with only `"tested"` stored,
    /// `contains("test")` is `false`.
    ///
    /// Answered from the hash membership set rather than by walking the arena:
    /// the walk costs one dependent cache miss per scalar, which no arena
    /// layout recovers, while one hash of the folded bytes plus a short probe
    /// is about twice as fast on a 20k-word corpus. The two are exactly
    /// equivalent — see `membership.rs` — and this crate's tests hold them to
    /// that differentially.
    #[must_use]
    pub fn contains(&self, string: &str) -> bool {
        let folded = fold(self.handling, string);
        self.index.contains_folded(folded.as_bytes())
    }

    /// [`Trie::contains`] by the original arena walk — the oracle this
    /// crate's tests compare the membership index against.
    #[cfg(test)]
    pub(crate) fn contains_walk(&self, string: &str) -> bool {
        let folded = fold(self.handling, string);
        match self.descend_folded(&folded) {
            Some(node) => self.nodes[node as usize].is_word,
            None => false,
        }
    }

    /// The number of distinct stored words.
    ///
    /// O(1): the root's maintained subtree word count.
    ///
    /// ```
    /// # use verbora_trie::Trie;
    /// let mut t = Trie::new();
    /// t.insert_all(["a", "ab", "a", ""]);
    /// assert_eq!(t.len(), 3);
    /// assert_eq!(t.node_count(), 3); // root + 'a' + 'b'
    /// ```
    #[must_use]
    pub fn len(&self) -> usize {
        self.nodes[ROOT as usize].word_count as usize
    }

    /// Whether no word has been stored.
    ///
    /// Note that a trie storing only the empty string is **not** empty: the
    /// empty string is a word like any other.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The number of **nodes** from the root down, including the root itself.
    ///
    /// This counts tree nodes, not words: an empty trie is 1, inserting `"a"`
    /// makes it 2, and inserting `"ab"` afterwards makes it 3. Inserting the
    /// empty string never changes it. One Unicode scalar is one node, whatever
    /// plane it lives in:
    ///
    /// ```
    /// # use verbora_trie::Trie;
    /// let mut t = Trie::new();
    /// t.insert("a👍");
    /// assert_eq!(t.node_count(), 3); // root + 'a' + '👍'
    /// ```
    ///
    /// O(1): the arena's length *is* the node count.
    #[must_use]
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Every stored word that starts with `prefix`, in ascending scalar order.
    ///
    /// `prefix` is folded exactly like every other argument when the trie folds
    /// case:
    ///
    /// ```
    /// # use verbora_trie::Trie;
    /// let mut t = Trie::case_insensitive();
    /// t.insert_all(["thEIr", "And", "theY"]);
    /// assert_eq!(t.keys_with_prefix("th"), ["their", "they"]);
    /// assert_eq!(t.keys_with_prefix("TH"), ["their", "they"]);
    /// ```
    ///
    /// See the type-level "Choosing the Right API" section for when to reach
    /// for this rather than [`Trie::iter_keys_with_prefix`],
    /// [`Trie::for_each_key_with_prefix`] or [`Trie::freeze`].
    #[must_use]
    pub fn keys_with_prefix(&self, prefix: &str) -> Vec<String> {
        // Not `iter_keys_with_prefix(prefix).collect()`: the lazy iterator has
        // to suspend and resume the whole traversal state once per word, which
        // a caller who wants the entire subtree pays for and gets nothing
        // from. The straight-line walk emits the same sequence with the
        // cursor in registers; `word_count` sizes the vector exactly, so the
        // only allocations left are the one per returned `String` — the floor
        // for an owned result. See `for_each_key_with_prefix` to skip even
        // those.
        let folded = fold(self.handling, prefix);
        let mut out = Vec::with_capacity(
            self.descend_folded(&folded)
                .map_or(0, |n| self.node(n).word_count as usize),
        );
        crate::iter::walk_keys(self, &folded, |key| out.push(String::from(key)));
        out
    }

    /// Calls `f` with every stored word that starts with `prefix`, in the same
    /// order as [`Trie::keys_with_prefix`], **without allocating one `String`
    /// per word**.
    ///
    /// # Why a callback instead of borrows
    ///
    /// A stored word exists nowhere contiguously in this representation — it is
    /// spelled out one scalar per arena node — so there is no `&str` inside the
    /// trie to hand back, and an iterator of borrows is therefore impossible
    /// without materialising something first. Passing the shared path buffer
    /// down to `f` is the one shape that keeps the whole enumeration
    /// allocation-free: the argument is valid only for the duration of the
    /// call, which is exactly the lifetime the buffer has before the next edge
    /// overwrites it.
    ///
    /// ```
    /// # use verbora_trie::Trie;
    /// let mut t = Trie::new();
    /// t.insert_all(["cat", "cats", "car"]);
    /// let mut bytes = 0;
    /// t.for_each_key_with_prefix("ca", |k| bytes += k.len());
    /// assert_eq!(bytes, 3 + 3 + 4);
    /// ```
    pub fn for_each_key_with_prefix<F: FnMut(&str)>(&self, prefix: &str, f: F) {
        let folded = fold(self.handling, prefix);
        crate::iter::walk_keys(self, &folded, f);
    }

    /// [`Trie::keys_with_prefix`] as a lazy iterator.
    ///
    /// Yields the same strings in the same order while holding only one path
    /// buffer and one stack frame per level, so an early `take`/`find` stops
    /// paying for the rest of the subtree. Counting is better still:
    /// `iter_keys_with_prefix(p).count()` reads the maintained subtree word
    /// count after the `p`-length descent — O(|prefix|), and no part of the
    /// subtree is visited — so it is the way to ask "how many words start with
    /// `p`?".
    ///
    /// # What counting costs
    ///
    /// Not nothing, and the difference matters when choosing between this and
    /// [`Trie::keys_with_prefix`]. Constructing the iterator seeds its path
    /// buffer with the prefix, because every word it yields is that prefix plus
    /// stored edge labels — **one heap allocation, the size of the folded
    /// prefix**, made whether or not the iterator is then advanced. It is one
    /// and not two: the folded prefix is moved into the buffer rather than
    /// copied into it.
    ///
    /// So counting is one allocation and a descent, against the whole subtree
    /// plus a `String` per word for `keys_with_prefix(p).len()`. Two cases cost
    /// nothing at all: an empty prefix ([`Trie::keys`]), and a prefix no stored
    /// word starts with — neither can spell a word out, so neither takes a
    /// buffer.
    #[must_use]
    pub fn iter_keys_with_prefix(&self, prefix: &str) -> KeysWithPrefix<'_> {
        KeysWithPrefix::new(self, fold(self.handling, prefix))
    }

    /// Every stored word, in ascending scalar order.
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
    /// t.insert_all(["a", "ab", "bc", "cd", "abc"]);
    /// assert_eq!(t.prefix_matches("abcd"), ["a", "ab", "abc"]);
    /// ```
    ///
    /// If the empty string was stored it is the first result, since the root is
    /// on every path. Results are cut from `search` (after folding), not
    /// rebuilt from the stored keys, so on a case-sensitive trie they borrow
    /// rather than allocate.
    #[must_use]
    pub fn prefix_matches<'a>(&self, search: &'a str) -> Vec<Cow<'a, str>> {
        self.iter_prefix_matches(search).collect()
    }

    /// [`Trie::prefix_matches`] as a lazy iterator.
    ///
    /// The walk is linear and single-pass, so this stops the moment the caller
    /// does — useful when only the shortest or the first match is wanted.
    #[must_use]
    pub fn iter_prefix_matches<'a>(&self, search: &'a str) -> PrefixMatches<'_, 'a> {
        PrefixMatches::new(self, fold(self.handling, search))
    }

    /// The longest stored word that prefixes `search`, and the part of `search`
    /// the walk could not consume.
    ///
    /// ```
    /// # use verbora_trie::Trie;
    /// let mut t = Trie::new();
    /// t.insert_all(["their", "and", "they"]);
    /// let split = t.longest_prefix("theyre");
    /// assert_eq!(split.word.as_deref(), Some("they"));
    /// assert_eq!(split.rest, "re");
    /// ```
    ///
    /// See [`PrefixSplit`] for the two details that are easy to get wrong: the
    /// remainder is measured from where the *walk* died, and `Some("")` is a
    /// different answer from `None`.
    #[must_use]
    pub fn longest_prefix<'a>(&self, search: &'a str) -> PrefixSplit<'a> {
        let folded = fold(self.handling, search);
        let walk = self.walk(&folded);

        // Reborrowing through the `Cow` keeps the case-sensitive path
        // allocation free: `search` outlives the return value, so its slices
        // can be handed straight back.
        let cut = |start: usize, end: usize| -> Cow<'a, str> {
            match &folded {
                Cow::Borrowed(s) => Cow::Borrowed(&s[start..end]),
                Cow::Owned(s) => Cow::Owned(s[start..end].to_owned()),
            }
        };

        PrefixSplit {
            word: walk.last_word.map(|end| cut(0, end)),
            rest: cut(walk.stopped_at, folded.len()),
        }
    }

    /// The same split as [`Trie::longest_prefix`], counted in Unicode scalars
    /// and allocating nothing at all.
    ///
    /// ```
    /// # use verbora_trie::Trie;
    /// let mut t = Trie::new();
    /// t.insert_all(["their", "and", "they"]);
    /// let lengths = t.longest_prefix_lengths("theyre");
    /// assert_eq!(lengths.word, Some(4));
    /// assert_eq!(lengths.rest, 2);
    /// ```
    #[must_use]
    pub fn longest_prefix_lengths(&self, search: &str) -> PrefixSplitLengths {
        let folded = fold(self.handling, search);
        let walk = self.walk(&folded);
        PrefixSplitLengths {
            word: walk.last_word.is_some().then_some(walk.last_word_scalars),
            rest: folded[walk.stopped_at..].chars().count(),
        }
    }

    /// Compresses this trie into a [`FrozenTrie`](crate::FrozenTrie): a
    /// read-only, path-compressed representation built once for query-heavy
    /// workloads.
    ///
    /// # Why this exists
    ///
    /// A run of single-child, non-word nodes costs one node hop each here and
    /// one edge-label comparison in a path-compressed radix map — the measured
    /// gap `docs/PERFORMANCE_GAPS.md` entry 32 records against
    /// `fast_radix_trie` on `keys_with_prefix`. `Trie` already wins
    /// build and `contains` against the same competitor, so this is a targeted
    /// fix for the operation that loses, not a replacement representation:
    /// `Trie` keeps building exactly as it does today, and this method is the
    /// "Freeze" step of the Build → Freeze → Query pattern.
    ///
    /// # What compresses, and why it is exact
    ///
    /// An original node is kept as a real frozen node when it is the root, is
    /// itself a stored word, or has zero or more than one child — in every
    /// other case it has exactly one child and marks no word, so it cannot be
    /// an independently observable stopping point for any query and is folded
    /// into the edge label leading to the next kept node. This is exact, not
    /// approximate: every position a caller could ever land on (`contains`
    /// returning `true`, a word emitted by `keys_with_prefix`, a prefix
    /// argument ending exactly there) is preserved as a real frozen node; only
    /// the *unobservable* pass-through nodes disappear.
    ///
    /// Compression never reorders anything: a kept node's children are carried
    /// over in their original positions, which are already sorted ascending by
    /// label, and two edges from one node always differ in their first scalar
    /// — so the frozen tree enumerates in the same ascending scalar order.
    ///
    /// # What is *not* on `FrozenTrie`
    ///
    /// Only `contains` and the key enumerations are implemented — the
    /// operations the measured gap is about. `prefix_matches` /
    /// `longest_prefix` have no frozen counterpart: nothing found a loss
    /// there, so extending compression to those top-down single-path walks
    /// (which would need byte-offset tracking across a multi-scalar edge
    /// label) is not attempted. Call the equivalent method on the original
    /// `Trie` for those.
    ///
    /// # Cost
    ///
    /// One linear pass over every original node (`O(n)` in node count, `O(n)`
    /// extra space for the kept-node index map plus the compressed label
    /// buffer), then one pass over the compressed tree to precompute the
    /// enumeration-order key table with per-node word ranges and the hash
    /// membership set. Call this once after a bulk load, not per query.
    ///
    /// ```
    /// # use verbora_trie::Trie;
    /// let mut t = Trie::new();
    /// t.insert_all(["cat", "cats", "car", "care", "careful"]);
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
        let mut labels: Vec<char> = Vec::new();
        for (i, orig) in self.nodes.iter().enumerate() {
            if new_index[i] == u32::MAX {
                continue;
            }
            let mut children = SmallVec::new();
            for child in &orig.children {
                let label_start = labels.len() as u32;
                labels.push(child.key);
                let mut cur = child.node;
                while new_index[cur as usize] == u32::MAX {
                    // `cur` was not kept, so the keep check above guarantees
                    // it has exactly one child.
                    let only = self.nodes[cur as usize].children[0];
                    labels.push(only.key);
                    cur = only.node;
                }
                let label_end = labels.len() as u32;
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

        crate::frozen::FrozenTrie::from_parts(frozen_nodes, labels, self.handling)
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
    #[inline]
    pub(crate) fn child(&self, node: u32, key: char) -> Option<u32> {
        let children = &self.nodes[node as usize].children;
        if children.len() < BINARY_SEARCH_THRESHOLD {
            return children.iter().find(|c| c.key == key).map(|c| c.node);
        }
        children
            .binary_search_by(|c| c.key.cmp(&key))
            .ok()
            .map(|i| children[i].node)
    }

    /// Walks the scalars of an already-folded string from the root, returning
    /// the node reached or `None` if some edge is missing.
    #[inline]
    pub(crate) fn descend_folded(&self, folded: &str) -> Option<u32> {
        let mut node = ROOT;
        for scalar in folded.chars() {
            node = self.child(node, scalar)?;
        }
        Some(node)
    }

    /// Adds the edge labelled `key` if it is missing, and returns the child.
    fn child_or_insert(&mut self, node: u32, key: char) -> u32 {
        let at = match self.nodes[node as usize]
            .children
            .binary_search_by(|c| c.key.cmp(&key))
        {
            Ok(i) => return self.nodes[node as usize].children[i].node,
            Err(i) => i,
        };
        // One below `u32::MAX`, not at it: `freeze()` uses `u32::MAX` as its
        // "absorbed into an edge label" sentinel (see `FrozenTrie::from_parts`),
        // so a node carrying that index would be mis-compressed rather than
        // rejected. The ceiling is therefore `u32::MAX` nodes, and the check is
        // an assertion rather than a `try_from` because the conversion is not
        // the tight bound.
        assert!(
            self.nodes.len() < u32::MAX as usize,
            "trie exceeds its node ceiling of {} nodes",
            u32::MAX
        );
        let new = self.nodes.len() as u32;
        self.nodes.push(Node::default());
        self.nodes[node as usize]
            .children
            .insert(at, Child { key, node: new });
        new
    }

    /// The shared walk behind [`Trie::longest_prefix`] and
    /// [`Trie::longest_prefix_lengths`].
    fn walk(&self, folded: &str) -> Walk {
        let mut node = ROOT;
        let mut last_word = self.nodes[ROOT as usize].is_word.then_some(0usize);
        let mut last_word_scalars = 0usize;
        let mut scalars = 0usize;

        for (byte, scalar) in folded.char_indices() {
            match self.child(node, scalar) {
                Some(next) => node = next,
                None => {
                    return Walk {
                        last_word,
                        last_word_scalars,
                        stopped_at: byte,
                    };
                }
            }
            scalars += 1;
            if self.nodes[node as usize].is_word {
                last_word = Some(byte + scalar.len_utf8());
                last_word_scalars = scalars;
            }
        }

        Walk {
            last_word,
            last_word_scalars,
            stopped_at: folded.len(),
        }
    }
}

impl<S: AsRef<str>> Extend<S> for Trie {
    fn extend<I: IntoIterator<Item = S>>(&mut self, iter: I) {
        self.insert_all(iter);
    }
}

impl<S: AsRef<str>> FromIterator<S> for Trie {
    /// Builds a **case-sensitive** trie.
    ///
    /// For the folding variant, build with [`Trie::case_insensitive`] and
    /// [`Trie::insert_all`].
    fn from_iter<I: IntoIterator<Item = S>>(iter: I) -> Self {
        let mut t = Self::new();
        t.insert_all(iter);
        t
    }
}

impl<'a> IntoIterator for &'a Trie {
    type Item = String;
    type IntoIter = KeysWithPrefix<'a>;

    /// Iterates every stored word, in ascending scalar order.
    ///
    /// Each item is an owned `String` because a stored word exists nowhere
    /// contiguously — it is spelled out one scalar per node — so it has to be
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
        assert_eq!(INLINE_CHILDREN, 2);
        assert_eq!(std::mem::size_of::<Child>(), 8);
        assert_eq!(
            std::mem::size_of::<SmallVec<[Child; INLINE_CHILDREN]>>(),
            std::mem::size_of::<Vec<Child>>()
        );
        assert_eq!(std::mem::size_of::<Node>(), 32);
    }

    #[test]
    fn fold_borrows_when_nothing_changes() {
        assert!(matches!(
            fold(CaseHandling::Sensitive, "ABC"),
            Cow::Borrowed(_)
        ));
        assert!(matches!(
            fold(CaseHandling::Folded, "abc"),
            Cow::Borrowed(_)
        ));
        assert!(matches!(fold(CaseHandling::Folded, "ABC"), Cow::Owned(_)));
        assert_eq!(fold(CaseHandling::Folded, "ÜBER"), "über");
    }

    #[test]
    fn fold_applies_the_full_unicode_lowercase_mapping() {
        // U+0130 LATIN CAPITAL LETTER I WITH DOT ABOVE lowercases to two
        // scalars (Unicode `SpecialCasing.txt`).
        assert_eq!(fold(CaseHandling::Folded, "İ"), "i\u{307}");
        // Final sigma is positional (Unicode 16.0 §5.18, `SpecialCasing.txt`).
        assert_eq!(fold(CaseHandling::Folded, "ΟΣ"), "ος");
        assert_eq!(fold(CaseHandling::Folded, "Σ"), "σ");
    }

    // -----------------------------------------------------------------------
    // Structural invariants
    // -----------------------------------------------------------------------

    #[test]
    fn a_fresh_trie_has_one_node_and_contains_nothing() {
        let t = Trie::new();
        assert_eq!(t.node_count(), 1);
        assert_eq!(t.len(), 0);
        assert!(t.is_empty());
        assert!(!t.contains(""));
        assert!(!t.contains("a"));
        assert!(t.keys_with_prefix("").is_empty());
        assert!(t.prefix_matches("abc").is_empty());
        let split = t.longest_prefix("abc");
        assert_eq!(split.word, None);
        assert_eq!(split.rest, "abc");
    }

    #[test]
    fn every_node_lists_each_key_once() {
        let mut t = Trie::new();
        t.insert_all(["ab", "ab", "ac", "a", "b", "ab"]);
        // 'a' must not sprout a duplicate 'b' edge on the repeated insertions.
        assert_eq!(t.node_count(), 5); // root + a + b(under a) + c(under a) + b
        assert_eq!(t.len(), 4);
        assert_eq!(t.keys_with_prefix(""), ["a", "ab", "ac", "b"]);
    }

    #[test]
    fn child_lists_stay_sorted_at_every_node() {
        let mut t = Trie::new();
        t.insert_all([
            "zb", "z9", "za", "z0", "z1", "😀", "中", "A", "a", "0", "~", " ",
        ]);
        for node in &t.nodes {
            assert!(
                node.children.windows(2).all(|w| w[0].key < w[1].key),
                "children out of order: {:?}",
                node.children
            );
        }
    }

    // -----------------------------------------------------------------------
    // The empty string is a word like any other
    // -----------------------------------------------------------------------

    #[test]
    fn empty_string_is_a_word_that_creates_no_node() {
        let mut t = Trie::new();
        assert!(t.insert(""));
        assert!(!t.insert(""));
        assert_eq!(t.node_count(), 1, "the root already existed");
        assert_eq!(t.len(), 1);
        assert!(!t.is_empty());
        assert!(t.contains(""));
        assert_eq!(t.keys_with_prefix(""), [""]);
        assert_eq!(t.prefix_matches("abc"), [""]);
        // `Some("")` is a real answer, distinct from `None`.
        let split = t.longest_prefix("zzz");
        assert_eq!(split.word.as_deref(), Some(""));
        assert_eq!(split.rest, "zzz");
        assert_eq!(
            t.longest_prefix_lengths("zzz"),
            PrefixSplitLengths {
                word: Some(0),
                rest: 3
            }
        );
    }

    // -----------------------------------------------------------------------
    // Character-class coverage
    // -----------------------------------------------------------------------

    #[test]
    fn single_character_words() {
        let mut t = Trie::new();
        t.insert_all(["a", "z", "0", "!"]);
        assert_eq!(t.node_count(), 5);
        for w in ["a", "z", "0", "!"] {
            assert!(t.contains(w), "{w:?}");
        }
        // Ascending scalar order: '!' (0x21), '0' (0x30), 'a' (0x61), 'z'.
        assert_eq!(t.keys_with_prefix(""), ["!", "0", "a", "z"]);
    }

    #[test]
    fn all_uppercase_input() {
        let mut sensitive = Trie::new();
        sensitive.insert("ALLCAPS");
        assert!(sensitive.contains("ALLCAPS"));
        assert!(!sensitive.contains("allcaps"));

        let mut folding = Trie::case_insensitive();
        folding.insert("ALLCAPS");
        assert!(folding.contains("allcaps"));
        assert!(folding.contains("AllCaps"));
        // Stored folded, and every argument folds — including the prefix.
        assert_eq!(folding.keys_with_prefix("all"), ["allcaps"]);
        assert_eq!(folding.keys_with_prefix("ALL"), ["allcaps"]);
    }

    #[test]
    fn accented_latin() {
        let mut t = Trie::case_insensitive();
        t.insert_all(["café", "CAFÉ", "Ångström", "crème brûlée", "straße"]);
        assert!(t.contains("CafÉ"));
        assert!(t.contains("ångström"));
        assert!(t.contains("CRÈME BRÛLÉE"));
        // 'ß' has no single-scalar uppercase, so "STRASSE" is a different word.
        assert!(t.contains("straße"));
        assert!(!t.contains("strasse"));
        let mut plain = Trie::new();
        plain.insert("café");
        assert_eq!(plain.node_count(), 5);
    }

    #[test]
    fn cyrillic_and_greek() {
        let mut t = Trie::new();
        t.insert_all(["Москва", "Москвич", "Ελλάδα", "Ελλάς"]);
        assert!(t.contains("Москва"));
        assert!(!t.contains("Москв"));
        assert_eq!(t.keys_with_prefix("Москв"), ["Москва", "Москвич"]);
        assert_eq!(t.prefix_matches("Ελλάδας"), ["Ελλάδα"]);

        let mut folding = Trie::case_insensitive();
        folding.insert("ΟΣ");
        assert_eq!(folding.keys_with_prefix(""), ["ος"]);
        assert!(folding.contains("ΟΣ"));
        assert!(folding.contains("ος"));
    }

    #[test]
    fn cjk() {
        let mut t = Trie::new();
        t.insert_all(["日本語", "日本", "中文测试", "한국어"]);
        assert_eq!(t.prefix_matches("日本語です"), ["日本", "日本語"]);
        assert_eq!(t.keys_with_prefix("日本"), ["日本", "日本語"]);
        let mut one = Trie::new();
        one.insert("日本語");
        assert_eq!(one.node_count(), 4);
    }

    #[test]
    fn astral_characters_occupy_one_node_each() {
        let mut t = Trie::new();
        t.insert("😀");
        assert_eq!(t.node_count(), 2, "root + one scalar");
        assert!(t.contains("😀"));
        assert_eq!(t.keys_with_prefix(""), ["😀"]);

        let mut many = Trie::new();
        many.insert_all(["a👍", "a👍b", "𝕳𝖊𝖑𝖑𝖔"]);
        assert_eq!(many.keys_with_prefix("a"), ["a👍", "a👍b"]);
        assert_eq!(many.prefix_matches("a👍bc"), ["a👍", "a👍b"]);
        assert!(many.contains("𝕳𝖊𝖑𝖑𝖔"));
    }

    #[test]
    fn a_walk_that_stops_hands_back_the_callers_own_text() {
        let mut t = Trie::new();
        t.insert("a👍");

        // The scalar unit means the walk can only ever stop on a character
        // boundary, so every remainder is a suffix of the input.
        for (search, rest, rest_len) in [
            ("a👌", "👌", 1usize),
            ("a😀x", "😀x", 2),
            ("a𝕳x", "𝕳x", 2),
            ("a👍z", "z", 1),
        ] {
            let split = t.longest_prefix(search);
            assert_eq!(split.rest, rest, "{search:?}");
            assert!(!split.rest.contains('\u{FFFD}'));
            assert!(search.ends_with(&*split.rest), "{search:?}");
            assert_eq!(
                t.longest_prefix_lengths(search).rest,
                rest_len,
                "{search:?}"
            );
        }
    }

    #[test]
    fn punctuation_and_whitespace() {
        let mut t = Trie::new();
        t.insert_all(["e.g.", "e.g", "U.S.A.", "don't", "--", "a-b", "  double  "]);
        assert!(t.contains("e.g."));
        assert_eq!(t.prefix_matches("e.g.!"), ["e.g", "e.g."]);
        assert!(t.contains("  double  "));
        assert_eq!(t.keys_with_prefix("e.g"), ["e.g", "e.g."]);
        // Whitespace is an ordinary scalar here: nothing is trimmed.
        assert!(!t.contains("double"));
    }

    #[test]
    fn digits_and_numeric_looking_words() {
        let mut t = Trie::new();
        t.insert_all(["123", "3.14", "1,000", "12", "1"]);
        assert_eq!(t.prefix_matches("1234"), ["1", "12", "123"]);
        // Ascending scalar order: ',' (0x2C) sorts before '2' (0x32).
        assert_eq!(t.keys_with_prefix(""), ["1", "1,000", "12", "123", "3.14"]);
    }

    #[test]
    fn very_long_input_does_not_overflow_the_stack() {
        let long: String = "ab".repeat(100_000);
        let mut t = Trie::new();
        assert!(t.insert(&long));
        assert_eq!(t.node_count(), long.len() + 1);
        assert!(t.contains(&long));
        assert!(!t.contains(&long[..long.len() - 1]));

        let extended = format!("{long}tail");
        let split = t.longest_prefix(&extended);
        assert_eq!(split.word.as_deref(), Some(long.as_str()));
        assert_eq!(split.rest, "tail");
        assert_eq!(t.keys_with_prefix(&long[..1000]).len(), 1);
        assert_eq!(t.keys().count(), 1);
    }

    // -----------------------------------------------------------------------
    // Case handling
    // -----------------------------------------------------------------------

    #[test]
    fn case_sensitive_is_the_default() {
        assert_eq!(Trie::new().case_handling(), CaseHandling::Sensitive);
        assert_eq!(Trie::default().case_handling(), CaseHandling::Sensitive);
        assert_eq!(
            Trie::with_case_handling(CaseHandling::Sensitive).case_handling(),
            CaseHandling::Sensitive
        );
        assert_eq!(
            Trie::case_insensitive().case_handling(),
            CaseHandling::Folded
        );
    }

    #[test]
    fn folding_applies_to_every_method_without_exception() {
        let mut t = Trie::case_insensitive();
        t.insert_all(["thEIr", "And", "theY"]);

        assert!(t.contains("THEIR"));
        assert_eq!(t.prefix_matches("THEYRE"), ["they"]);
        let split = t.longest_prefix("ThEyRe");
        assert_eq!(split.word.as_deref(), Some("they"));
        assert_eq!(split.rest, "re");

        for prefix in ["th", "TH", "Th", "tH"] {
            assert_eq!(t.keys_with_prefix(prefix), ["their", "they"], "{prefix:?}");
            assert_eq!(
                t.iter_keys_with_prefix(prefix).collect::<Vec<String>>(),
                ["their", "they"],
                "{prefix:?}"
            );
            let mut seen = Vec::new();
            t.for_each_key_with_prefix(prefix, |k| seen.push(k.to_owned()));
            assert_eq!(seen, ["their", "they"], "{prefix:?}");
        }
    }

    #[test]
    fn folding_can_change_a_words_length() {
        // 'İ' U+0130 lowercases to two scalars, so the folded word is longer
        // than the input and occupies one extra node.
        let mut t = Trie::case_insensitive();
        t.insert("İ");
        assert_eq!(t.keys_with_prefix(""), ["i\u{307}"]);
        assert_eq!(t.node_count(), 3);
        assert!(t.contains("İ"));
        assert!(t.contains("i\u{307}"));
        // A folded prefix reaches it from either spelling.
        assert_eq!(t.keys_with_prefix("İ"), ["i\u{307}"]);
        assert_eq!(t.keys_with_prefix("i"), ["i\u{307}"]);
    }

    // -----------------------------------------------------------------------
    // Return-shape details that are easy to get wrong
    // -----------------------------------------------------------------------

    #[test]
    fn longest_prefix_reports_where_the_walk_died_not_where_the_word_ended() {
        let mut t = Trie::new();
        t.insert_all(["their", "and"]);
        // The walk reaches "the" before failing, so three scalars are gone
        // even though no word ever matched.
        let split = t.longest_prefix("theyre");
        assert_eq!(split.word, None);
        assert_eq!(split.rest, "yre");
        assert_eq!(
            t.longest_prefix_lengths("theyre"),
            PrefixSplitLengths {
                word: None,
                rest: 3
            }
        );
    }

    #[test]
    fn longest_prefix_on_an_exact_hit_leaves_nothing() {
        let mut t = Trie::new();
        t.insert("ab");
        let split = t.longest_prefix("ab");
        assert_eq!(split.word.as_deref(), Some("ab"));
        assert_eq!(split.rest, "");
        assert_eq!(
            t.longest_prefix_lengths("ab"),
            PrefixSplitLengths {
                word: Some(2),
                rest: 0
            }
        );
    }

    #[test]
    fn longest_prefix_borrows_when_no_folding_is_needed() {
        let mut t = Trie::new();
        t.insert("ab");
        let split = t.longest_prefix("abcd");
        assert!(matches!(split.word, Some(Cow::Borrowed(_))));
        assert!(matches!(split.rest, Cow::Borrowed(_)));

        let mut folding = Trie::case_insensitive();
        folding.insert("ab");
        let split = folding.longest_prefix("ABCD");
        assert!(matches!(split.word, Some(Cow::Owned(_))));
    }

    #[test]
    fn prefix_matches_borrow_when_no_folding_is_needed() {
        let mut t = Trie::new();
        t.insert_all(["a", "ab"]);
        let got = t.prefix_matches("abc");
        assert!(got.iter().all(|m| matches!(m, Cow::Borrowed(_))));
    }

    #[test]
    fn prefixes_and_extensions_of_a_stored_word_are_not_contained() {
        let mut t = Trie::new();
        t.insert_all(["test", "tested"]);
        assert!(t.contains("test"));
        assert!(!t.contains("tes"));
        assert!(!t.contains("teste"));
        assert!(!t.contains("est"));
    }

    // -----------------------------------------------------------------------
    // Iterator surface
    // -----------------------------------------------------------------------

    #[test]
    fn iterators_agree_with_their_eager_counterparts() {
        let mut t = Trie::new();
        t.insert_all(["a", "ab", "abc", "b", "0z", "9z", "😀", "日本"]);
        assert_eq!(
            t.keys_with_prefix("a"),
            t.iter_keys_with_prefix("a").collect::<Vec<_>>()
        );
        assert_eq!(t.keys_with_prefix(""), t.keys().collect::<Vec<_>>());
        assert_eq!(t.keys_with_prefix(""), (&t).into_iter().collect::<Vec<_>>());
        assert_eq!(
            t.prefix_matches("abcd"),
            t.iter_prefix_matches("abcd").collect::<Vec<_>>()
        );
    }

    #[test]
    fn iterators_stop_early_and_stay_fused() {
        let mut t = Trie::new();
        t.insert_all(["a", "ab", "abc", "abcd"]);
        assert_eq!(t.keys().take(2).collect::<Vec<_>>(), ["a", "ab"]);

        let mut keys = t.iter_keys_with_prefix("zz");
        assert_eq!(keys.next(), None);
        assert_eq!(keys.next(), None);

        let mut path = t.iter_prefix_matches("zz");
        assert_eq!(path.next(), None);
        assert_eq!(path.next(), None);
    }

    #[test]
    fn from_iterator_and_extend_build_case_sensitive_tries() {
        let mut t: Trie = ["a", "ab"].into_iter().collect();
        assert_eq!(t.case_handling(), CaseHandling::Sensitive);
        t.extend(["b", "bc"]);
        assert_eq!(t.keys_with_prefix(""), ["a", "ab", "b", "bc"]);
    }

    #[test]
    fn insert_all_accepts_owned_and_borrowed_items() {
        let owned = vec![String::from("alpha"), String::from("beta")];
        let mut t = Trie::new();
        t.insert_all(&owned);
        t.insert_all(owned.iter().map(String::as_str));
        t.insert_all(["gamma"]);
        assert_eq!(t.keys_with_prefix(""), ["alpha", "beta", "gamma"]);
    }

    #[test]
    fn clone_preserves_enumeration_order() {
        let mut t = Trie::case_insensitive();
        t.insert_all(["b1", "a1", "9x", "1x", "0x", "zz"]);
        let copy = t.clone();
        assert_eq!(t, copy);
        assert_eq!(copy.keys_with_prefix(""), t.keys_with_prefix(""));
        assert_eq!(copy.case_handling(), CaseHandling::Folded);
    }

    // -----------------------------------------------------------------------
    // The maintained accelerators: subtree word counts and the membership set
    // -----------------------------------------------------------------------

    /// A tiny, dependency-free PRNG — same Xorshift64 shape the frozen
    /// module's randomized tests use.
    pub(crate) struct Xorshift64(pub(crate) u64);

    impl Xorshift64 {
        pub(crate) fn next_u64(&mut self) -> u64 {
            let mut x = self.0;
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            self.0 = x;
            x
        }

        pub(crate) fn next_range(&mut self, bound: usize) -> usize {
            (self.next_u64() % bound as u64) as usize
        }
    }

    /// Uppercase letters exercise folding, digits exercise ordering across
    /// scalar classes, and the astral characters exercise the widest scalars.
    const ALPHABET: &[char] = &[
        'a', 'b', 'c', 'A', 'B', '0', '1', '9', 'ñ', 'É', '中', '😀', '👍',
    ];

    fn random_word(rng: &mut Xorshift64, max_len: usize) -> String {
        let len = rng.next_range(max_len + 1);
        (0..len)
            .map(|_| ALPHABET[rng.next_range(ALPHABET.len())])
            .collect()
    }

    /// Recomputes every node's subtree word count from nothing but `is_word`
    /// and the child edges — the definition `word_count` must match.
    ///
    /// A child is always created after its parent, so arena indices are a
    /// topological order and one reverse pass suffices.
    fn recount(t: &Trie) -> Vec<u32> {
        let mut counts = vec![0u32; t.nodes.len()];
        for i in (0..t.nodes.len()).rev() {
            let mut c = u32::from(t.nodes[i].is_word);
            for child in &t.nodes[i].children {
                c += counts[child.node as usize];
            }
            counts[i] = c;
        }
        counts
    }

    #[test]
    fn word_counts_survive_duplicates_folding_and_expansion() {
        let mut t = Trie::case_insensitive();
        for w in [
            "their", "THEIR", "they", "the", "", "İ", "i\u{307}", "a👍", "a👍", "a👍b", "0x", "9x",
        ] {
            t.insert(w);
        }
        let expected = recount(&t);
        let actual: Vec<u32> = t.nodes.iter().map(|n| n.word_count).collect();
        assert_eq!(actual, expected);
        assert_eq!(t.len(), t.keys().count());
    }

    #[test]
    fn randomized_word_counts_and_membership_agree_with_the_structure() {
        let mut rng = Xorshift64(0x0BAD_5EED_0BAD_5EED);
        for round in 0..40 {
            let case_insensitive = round % 3 == 0;
            let mut t = if case_insensitive {
                Trie::case_insensitive()
            } else {
                Trie::new()
            };
            // An independent membership oracle over folded strings.
            let mut oracle = std::collections::HashSet::new();

            let word_count = 5 + rng.next_range(60);
            let mut words: Vec<String> =
                (0..word_count).map(|_| random_word(&mut rng, 6)).collect();
            if round % 5 == 0 {
                words.push(String::new());
            }
            // Deliberate duplicates so the rollback path runs every round.
            for i in (0..words.len()).step_by(4) {
                words.push(words[i].clone());
            }

            for w in &words {
                let expected = oracle.insert(fold(t.handling, w).into_owned());
                assert_eq!(t.insert(w), expected, "r{round}: insert({w:?})");
            }

            // Counts: every node, against the structural recount.
            let expected = recount(&t);
            let actual: Vec<u32> = t.nodes.iter().map(|n| n.word_count).collect();
            assert_eq!(actual, expected, "r{round}: word_count diverged");
            assert_eq!(t.len(), oracle.len(), "r{round}: len");

            // Membership: every stored word, its prefixes (near misses), and
            // random probes, against the arena walk.
            let mut probes: Vec<String> = Vec::new();
            for w in &words {
                probes.push(w.clone());
                probes.push(w.to_uppercase());
                let mut acc = String::new();
                for ch in w.chars() {
                    acc.push(ch);
                    probes.push(acc.clone());
                }
                probes.push(format!("{w}zz"));
            }
            probes.push(String::new());
            for _ in 0..20 {
                probes.push(random_word(&mut rng, 6));
            }
            for p in &probes {
                assert_eq!(
                    t.contains(p),
                    t.contains_walk(p),
                    "r{round}: contains({p:?}) diverged from the walk"
                );
            }
        }
    }

    #[test]
    fn count_is_exact_before_during_and_after_iteration() {
        let mut t = Trie::new();
        t.insert_all(["a", "ab", "abc", "abd", "b", "0x", "😀", "😀a"]);
        for prefix in ["", "a", "ab", "abc", "b", "😀", "zz", "0"] {
            let full: Vec<String> = t.iter_keys_with_prefix(prefix).collect();
            assert_eq!(
                t.iter_keys_with_prefix(prefix).count(),
                full.len(),
                "fresh count for {prefix:?}"
            );
            for k in 0..=full.len() {
                let mut it = t.iter_keys_with_prefix(prefix);
                for expected in full.iter().take(k) {
                    assert_eq!(it.next().as_ref(), Some(expected));
                }
                assert_eq!(it.count(), full.len() - k, "after {k} of {prefix:?}");
            }
            let mut it = t.iter_keys_with_prefix(prefix);
            for _ in 0..full.len() {
                it.next();
            }
            assert_eq!(it.next(), None);
            assert_eq!(it.count(), 0);
        }
    }

    #[test]
    fn size_hint_is_exact_so_collect_allocates_once() {
        let mut t = Trie::new();
        t.insert_all(["a", "ab", "abc", "b"]);
        let mut it = t.iter_keys_with_prefix("a");
        assert_eq!(it.size_hint(), (3, Some(3)));
        it.next();
        assert_eq!(it.size_hint(), (2, Some(2)));
        let collected: Vec<String> = it.collect();
        assert_eq!(collected, ["ab", "abc"]);
        assert_eq!(t.iter_keys_with_prefix("zz").size_hint(), (0, Some(0)));
    }

    /// Every stored word, its prefixes, its case variants, near misses on
    /// both ends, and — the shape the first-byte gate exists for — misses
    /// that differ from every stored word in their *first* byte.
    fn membership_probes(words: &[String], rng: &mut Xorshift64) -> Vec<String> {
        let mut probes = vec![String::new()];
        for w in words {
            probes.push(w.clone());
            probes.push(w.to_uppercase());
            probes.push(format!("{w}zz"));
            for lead in ["Q", "~", "0", "中", "\u{7f}"] {
                probes.push(format!("{lead}{w}"));
            }
            let mut acc = String::new();
            for ch in w.chars() {
                acc.push(ch);
                probes.push(acc.clone());
            }
        }
        for _ in 0..20 {
            probes.push(random_word(rng, 6));
        }
        probes
    }

    #[test]
    fn the_first_byte_gate_never_changes_an_answer() {
        let mut rng = Xorshift64(0x51A7_1CE5_51A7_1CE5);
        for round in 0..40 {
            let mut t = if round % 3 == 0 {
                Trie::case_insensitive()
            } else {
                Trie::new()
            };
            let mut words: Vec<String> = (0..5 + rng.next_range(60))
                .map(|_| random_word(&mut rng, 6))
                .collect();
            if round % 5 == 0 {
                words.push(String::new());
            }
            t.insert_all(words.iter().map(String::as_str));

            for p in &membership_probes(&words, &mut rng) {
                assert_eq!(
                    t.contains(p),
                    t.contains_walk(p),
                    "r{round}: contains({p:?}) diverged from the walk"
                );
            }
        }
    }

    #[test]
    fn the_gate_rejects_only_bytes_no_stored_word_starts_with() {
        let mut t = Trie::new();
        t.insert("kilo");
        assert!(t.contains("kilo"));
        for lead in ['K', 'j', 'l', '0', ' '] {
            assert!(!t.contains(&format!("{lead}ilo")));
        }
        assert!(!t.contains(""));
        let mut with_empty = Trie::new();
        with_empty.insert("");
        assert!(with_empty.contains(""));
        assert!(!with_empty.contains("k"));
        let fresh = Trie::new();
        assert!(!fresh.contains(""));
        assert!(!fresh.contains("k"));
    }

    #[test]
    fn folding_reaches_the_gate_with_the_folded_first_byte() {
        let mut t = Trie::case_insensitive();
        t.insert_all(["kilo", "İstanbul", "Ångström"]);
        assert!(t.contains("KILO"));
        assert!(t.contains("İstanbul"));
        assert!(t.contains("i\u{307}stanbul"));
        assert!(t.contains("ÅNGSTRÖM"));
        assert!(t.contains("ångström"));
        assert!(!t.contains("Qilo"));
    }

    // -----------------------------------------------------------------------
    // The bulk walk behind `keys_with_prefix` / `for_each_key_with_prefix`
    // -----------------------------------------------------------------------

    /// The lazy iterator is the oracle: it is the implementation both bulk
    /// paths replaced.
    fn assert_walks_agree(t: &Trie, prefix: &str, whence: &str) {
        let oracle: Vec<String> = t.iter_keys_with_prefix(prefix).collect();
        assert_eq!(
            t.keys_with_prefix(prefix),
            oracle,
            "{whence}: keys_with_prefix({prefix:?})"
        );
        let mut borrowed: Vec<String> = Vec::new();
        t.for_each_key_with_prefix(prefix, |k| borrowed.push(k.to_owned()));
        assert_eq!(
            borrowed, oracle,
            "{whence}: for_each_key_with_prefix({prefix:?})"
        );
    }

    #[test]
    fn the_bulk_walk_matches_the_lazy_iterator_on_the_hard_shapes() {
        let mut t = Trie::new();
        t.insert_all([
            "",
            "a",
            "ab",
            "abc",
            "abcd",
            "abd",
            "b1",
            "a1",
            "9x",
            "1x",
            "0x",
            "zz",
            "😀",
            "😀a",
            "😀ab",
            "x😀y",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaz",
        ]);
        for prefix in [
            "", "a", "ab", "abc", "abcd", "abd", "b", "zz", "z", "0", "9", "x", "😀", "😀a",
            "nothing", "abcde", "aaaa",
        ] {
            assert_walks_agree(&t, prefix, "hard shapes");
        }
        assert!(t.keys_with_prefix("\u{fffd}").is_empty());
    }

    #[test]
    fn the_bulk_walk_matches_the_lazy_iterator_on_random_corpora() {
        let mut rng = Xorshift64(0xD00D_FEED_D00D_FEED);
        for round in 0..40 {
            let mut t = if round % 3 == 0 {
                Trie::case_insensitive()
            } else {
                Trie::new()
            };
            let words: Vec<String> = (0..5 + rng.next_range(80))
                .map(|_| random_word(&mut rng, 7))
                .collect();
            t.insert_all(words.iter().map(String::as_str));

            let mut prefixes = vec![String::new(), String::from("zzz")];
            for w in &words {
                let mut acc = String::new();
                for ch in w.chars() {
                    acc.push(ch);
                    prefixes.push(acc.clone());
                }
                prefixes.push(w.clone());
            }
            for p in &prefixes {
                assert_walks_agree(&t, p, &format!("r{round}"));
            }
        }
    }

    #[test]
    fn the_bulk_walk_agrees_with_the_frozen_representation() {
        let mut t = Trie::new();
        t.insert_all([
            "cat", "cats", "car", "care", "careful", "dog", "", "0a", "😀z",
        ]);
        let frozen = t.freeze();
        for prefix in ["", "c", "ca", "car", "care", "d", "0", "😀", "nope"] {
            let mut borrowed: Vec<String> = Vec::new();
            t.for_each_key_with_prefix(prefix, |k| borrowed.push(k.to_owned()));
            assert_eq!(borrowed, frozen.keys_slice(prefix), "{prefix:?}");
        }
    }

    #[test]
    fn a_deep_single_child_chain_neither_overflows_nor_reorders() {
        let deep = "a".repeat(100_000);
        let mut t = Trie::new();
        t.insert_all([deep.clone(), format!("{deep}b"), String::from("a")]);
        let keys = t.keys_with_prefix("");
        assert_eq!(keys.len(), 3);
        assert_eq!(keys[0], "a");
        assert_eq!(keys[1], deep);
        assert_eq!(keys[2], format!("{deep}b"));
        assert_eq!(keys, t.iter_keys_with_prefix("").collect::<Vec<_>>());
    }

    #[test]
    fn the_borrowed_key_is_only_the_current_word() {
        let mut t = Trie::new();
        t.insert_all(["ab", "abcd", "b"]);
        let mut seen = Vec::new();
        t.for_each_key_with_prefix("", |k| seen.push(k.to_owned()));
        assert_eq!(seen, ["ab", "abcd", "b"]);
    }

    #[test]
    fn equality_ignores_word_insertion_history() {
        let mut ab_first = Trie::new();
        ab_first.insert_all(["ab", "a"]);
        let mut a_first = Trie::new();
        a_first.insert_all(["a", "ab"]);
        assert_eq!(ab_first, a_first);
        for probe in ["a", "ab", "abc", "b", ""] {
            assert_eq!(ab_first.contains(probe), a_first.contains(probe));
        }
    }
}
