//! [`FrozenTrie`]: the precomputed, read-only query representation built by
//! [`Trie::freeze`](crate::Trie::freeze).
//!
//! This is a Verbora-native extension — there is no reference `trie`
//! counterpart to port, so nothing here is bound by parity. Its only
//! obligation is to answer `contains`/`keys_with_prefix` identically to the
//! [`Trie`](crate::Trie) it was built from; see `Trie::freeze`'s own doc
//! comment for exactly why compression is exact and what it does not yet
//! cover.
//!
//! # Freeze = precompute everything
//!
//! Freezing does not merely path-compress the arena; it precomputes the
//! answers enumeration and membership queries would otherwise reconstruct
//! per call:
//!
//! * **The key table.** Every stored word, materialised once, in exactly the
//!   reference enumeration order. Because the build walk is pre-order, the
//!   words of *any* subtree form one contiguous range of that table, so each
//!   frozen node needs only a `(start, end)` pair for `keys_with_prefix` to
//!   become "descend the prefix, copy a range" — the copy being the measured
//!   allocation floor of the pinned `Vec<String>` return shape — and for a
//!   prefix *count* to become a subtraction. [`FrozenTrie::keys_slice`]
//!   exposes the range itself, borrowed, for callers who do not need owned
//!   `String`s at all.
//! * **The membership set.** `contains` answers from a hash set of the
//!   folded stored words (see `membership.rs` for why that is exactly the
//!   walk's answer), replacing the frozen walk's per-query unit buffer and
//!   label scans — the one query freezing used to make *slower* than the
//!   mutable arena.
//!
//! The compressed tree itself is kept for the prefix descent, which is the
//! only walking any query still does.

use std::iter::FusedIterator;

use smallvec::SmallVec;

use crate::membership::HashIndex;
use crate::trie::fold;

/// Arena index of the frozen root.
const ROOT: u32 = 0;

/// Inline child capacity for a [`FrozenNode`].
///
/// `FrozenChild` is three `u32`s (12 bytes, no padding), so unlike the
/// mutable [`Trie`](crate::Trie)'s `Child` there is no "free" inline size —
/// every inline slot costs real bytes. Two is kept anyway, matching the
/// mutable arena's own reasoning: the overwhelming majority of trie nodes
/// have one or two children, and that is exactly the shape a frozen,
/// read-only structure is built to serve fastest.
const INLINE_CHILDREN: usize = 2;

/// One compressed edge: a run of UTF-16 code units (a slice into
/// [`FrozenTrie::units`]) and the frozen node it leads to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FrozenChild {
    pub(crate) label_start: u32,
    pub(crate) label_end: u32,
    pub(crate) node: u32,
}

/// A node in the frozen arena — a real stopping point in the original trie:
/// the root, a stored word, or a branch.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct FrozenNode {
    /// Outgoing compressed edges, in the same order [`Trie::freeze`] read
    /// them from the original node's children — see that method's own doc
    /// comment for why this already reproduces the reference `for…in` order.
    pub(crate) children: SmallVec<[FrozenChild; INLINE_CHILDREN]>,
    pub(crate) is_word: bool,
}

/// A read-only, precomputed prefix tree, built once by
/// [`Trie::freeze`](crate::Trie::freeze).
///
/// ```
/// use verbora_trie::Trie;
///
/// let mut t = Trie::new();
/// t.add_strings(["a", "ab", "bc", "cd", "abc"]);
/// let frozen = t.freeze();
///
/// assert!(frozen.contains("abc"));
/// assert_eq!(frozen.keys_with_prefix("ab"), ["ab", "abc"]);
/// assert_eq!(frozen.keys_slice("ab"), ["ab", "abc"]); // borrowed, no copies
/// ```
#[derive(Clone, Debug)]
pub struct FrozenTrie {
    /// Kept (real) nodes, flat. Index 0 is always the frozen root.
    nodes: Vec<FrozenNode>,
    /// Shared buffer every edge's label slices into.
    units: Vec<u16>,
    /// Whether stored and queried strings are lowercased first — inherited
    /// unchanged from the [`Trie`] this was frozen from, since folding
    /// already happened at insertion time and freezing never re-derives it.
    folds: bool,
    /// Every stored word, in the reference enumeration order — the module
    /// doc's key table. `Vec<String>` rather than one contiguous blob with
    /// offsets because it is what lets [`FrozenTrie::keys_slice`] hand back a
    /// borrowed `&[String]` and lets `keys_with_prefix` be a straight
    /// `to_vec` (measured slightly faster to materialise than re-slicing a
    /// blob, at the cost of one allocation per key held here).
    keys: Vec<String>,
    /// Per frozen node: the `[start, end)` range of `keys` its subtree
    /// covers. Contiguity is a theorem, not luck: `keys` is emitted by a
    /// pre-order walk, so everything under one node is consecutive.
    word_range: Vec<(u32, u32)>,
    /// Hash membership set over the folded stored words, behind
    /// [`FrozenTrie::contains`] — see `membership.rs`.
    index: HashIndex,
}

/// Equality compares the compressed tree (`nodes`/`units`) and the folding
/// flag only: the key table, word ranges, and membership set are all derived
/// deterministically from those in `from_parts`, so comparing them again
/// could never change the answer — it would only re-compare megabytes of
/// redundant data.
impl PartialEq for FrozenTrie {
    fn eq(&self, other: &Self) -> bool {
        self.nodes == other.nodes && self.units == other.units && self.folds == other.folds
    }
}

impl Eq for FrozenTrie {}

impl FrozenTrie {
    /// Assembles a `FrozenTrie` from already-computed parts, precomputing
    /// the key table, per-node word ranges, and membership set (the module
    /// doc's "freeze = precompute everything" step).
    ///
    /// Only [`Trie::freeze`](crate::Trie::freeze) calls this — it is the one
    /// place that can guarantee `nodes`/`units` satisfy this type's
    /// invariants (every `FrozenChild::node` is a valid index into `nodes`,
    /// every label range is a valid slice of `units`).
    pub(crate) fn from_parts(nodes: Vec<FrozenNode>, units: Vec<u16>, folds: bool) -> Self {
        let (keys, word_range) = key_table(&nodes, &units);
        let mut index = HashIndex::new();
        index.reserve(keys.len(), keys.iter().map(String::len).sum());
        for key in &keys {
            index.insert_folded(key.as_bytes());
        }
        Self {
            nodes,
            units,
            folds,
            keys,
            word_range,
            index,
        }
    }

    /// Whether this trie compares strings case-sensitively — the same
    /// answer [`Trie::is_case_sensitive`](crate::Trie::is_case_sensitive)
    /// gave on the trie this was frozen from.
    #[must_use]
    pub fn is_case_sensitive(&self) -> bool {
        !self.folds
    }

    /// The number of **frozen nodes** — root, stored words, and branches.
    ///
    /// This is *not* the same number [`Trie::get_size`](crate::Trie::get_size)
    /// reports on the trie this was frozen from: that counts one node per
    /// UTF-16 code unit, this counts only the positions compression could
    /// not remove. A long, branch-free chain (e.g. one word with no other
    /// word sharing any of its prefixes) is many original nodes but exactly
    /// one frozen node beyond the root.
    ///
    /// ```
    /// # use verbora_trie::Trie;
    /// let mut t = Trie::new();
    /// t.add_string("hello");
    /// assert_eq!(t.get_size(), 6); // root + 5 code units
    /// assert_eq!(t.freeze().get_size(), 2); // root + one compressed edge
    /// ```
    #[must_use]
    pub fn get_size(&self) -> usize {
        self.nodes.len()
    }

    /// Whether `string` was added as a complete word — see
    /// [`Trie::contains`](crate::Trie::contains).
    ///
    /// Answered from the precomputed membership set: one hash of the folded
    /// query plus a short probe, instead of walking compressed edges with a
    /// per-query unit buffer. That walk was this type's one *regression*
    /// against the mutable arena (wide-node label scans ate the hop savings);
    /// the hash set removes it and beats the mutable walk as well.
    #[must_use]
    pub fn contains(&self, string: &str) -> bool {
        let folded = fold(self.folds, string);
        self.index.contains_folded(folded.as_bytes())
    }

    /// Every stored word that starts with `prefix` — see
    /// [`Trie::keys_with_prefix`](crate::Trie::keys_with_prefix), including
    /// its "does not fold `prefix`" note, preserved here unchanged since
    /// this reads the same already-folded stored data.
    ///
    /// One descent plus one range copy out of the precomputed key table —
    /// the cost is the owned `Vec<String>` return shape itself. When a
    /// borrow is enough, [`FrozenTrie::keys_slice`] skips even that.
    #[must_use]
    pub fn keys_with_prefix(&self, prefix: &str) -> Vec<String> {
        self.keys_slice(prefix).to_vec()
    }

    /// Every stored word that starts with `prefix`, **borrowed** from the
    /// precomputed key table — same words, same order as
    /// [`FrozenTrie::keys_with_prefix`], with no per-key allocation and
    /// O(|prefix|) total cost.
    ///
    /// This is the shape to reach for when the strings are only read —
    /// counted, scanned, displayed — rather than kept: the subtree's words
    /// already sit consecutively in the table (the build walk is pre-order),
    /// so the whole answer is one descent and one range.
    ///
    /// Like `keys_with_prefix`, `prefix` is **not** case-folded — the
    /// preserved reference bug documented on
    /// [`Trie::keys_with_prefix`](crate::Trie::keys_with_prefix).
    ///
    /// ```
    /// use verbora_trie::Trie;
    ///
    /// let mut t = Trie::new();
    /// t.add_strings(["cat", "cats", "car", "cool"]);
    /// let frozen = t.freeze();
    ///
    /// let ca: &[String] = frozen.keys_slice("ca");
    /// assert_eq!(ca, ["cat", "cats", "car"]);
    /// assert_eq!(frozen.keys_slice("ca").len(), 3); // count without a copy
    /// assert!(frozen.keys_slice("dog").is_empty());
    /// ```
    #[must_use]
    pub fn keys_slice(&self, prefix: &str) -> &[String] {
        match self.descend_prefix(prefix) {
            Some(node) => {
                let (start, end) = self.word_range[node as usize];
                &self.keys[start as usize..end as usize]
            }
            None => &[],
        }
    }

    /// [`FrozenTrie::keys_with_prefix`] as a lazy iterator.
    ///
    /// A cursor over the precomputed key table's range: `count`, `nth`, and
    /// `size_hint` are all O(1), and each `next` costs exactly the one
    /// `String` clone the owned item type demands. To skip even those
    /// clones, iterate [`FrozenTrie::keys_slice`] directly.
    #[must_use]
    pub fn iter_keys_with_prefix<'t>(&'t self, prefix: &str) -> FrozenKeysWithPrefix<'t> {
        FrozenKeysWithPrefix::new(self, prefix)
    }

    /// Every stored word, in the reference's enumeration order — equivalent
    /// to `iter_keys_with_prefix("")`.
    #[must_use]
    pub fn keys(&self) -> FrozenKeysWithPrefix<'_> {
        self.iter_keys_with_prefix("")
    }

    #[inline]
    fn label(&self, child: &FrozenChild) -> &[u16] {
        &self.units[child.label_start as usize..child.label_end as usize]
    }

    /// The child of `node` whose edge label *starts* with `first_unit`, if
    /// any. At most one can exist: two children starting with the same unit
    /// would have been merged into one node during the original trie's own
    /// `child_or_insert`, long before freezing ever runs.
    #[inline]
    fn find_child(&self, node: u32, first_unit: u16) -> Option<&FrozenChild> {
        self.nodes[node as usize]
            .children
            .iter()
            .find(|c| self.units[c.label_start as usize] == first_unit)
    }

    /// The node whose subtree holds exactly the stored words starting with
    /// `prefix` (compared unit by unit, unfolded), or `None` when no stored
    /// word does.
    ///
    /// The walk may run out strictly *inside* a compressed edge. That is
    /// still a definite answer: a compressed edge has no branching inside it
    /// (that is exactly why it compressed), so once the consumed part of
    /// `prefix` is confirmed a genuine prefix of the edge label, every word
    /// in the edge's target subtree — and no other word — has `prefix` as a
    /// true prefix, and the target's word range is the answer. Nothing needs
    /// the label's unconsumed tail: the range indexes whole stored keys, not
    /// path reconstructions.
    fn descend_prefix(&self, prefix: &str) -> Option<u32> {
        let mut units = prefix.encode_utf16();
        let Some(mut unit) = units.next() else {
            return Some(ROOT);
        };
        let mut node = ROOT;
        loop {
            let child = self.find_child(node, unit)?;
            // `find_child` matched the label's first unit; the rest must
            // keep matching for as long as the query lasts.
            for &label_unit in &self.label(child)[1..] {
                match units.next() {
                    Some(u) if u == label_unit => {}
                    Some(_) => return None,
                    None => return Some(child.node),
                }
            }
            node = child.node;
            match units.next() {
                Some(u) => unit = u,
                None => return Some(node),
            }
        }
    }
}

/// Appends one UTF-16 code unit to `buf`, holding a high surrogate in
/// `pending` until its partner arrives — the same reassembly
/// `KeysWithPrefix::push_unit` documents, needed here because a compressed
/// label *boundary* can fall between the halves of a surrogate pair even
/// though a label's interior never does.
fn push_unit(buf: &mut String, pending: &mut Option<u16>, unit: u16) {
    if let Some(high) = pending.take() {
        if let Some(c) = char::decode_utf16([high, unit]).next().and_then(Result::ok) {
            buf.push(c);
            return;
        }
        // Unreachable through the public API — every stored word came from a
        // well-formed `&str`; kept total rather than panicking.
        buf.push(char::REPLACEMENT_CHARACTER);
    }
    if (0xD800..0xDC00).contains(&unit) {
        *pending = Some(unit);
        return;
    }
    buf.push(char::from_u32(u32::from(unit)).unwrap_or(char::REPLACEMENT_CHARACTER));
}

/// Builds the key table and per-node word ranges in one pre-order walk —
/// the same traversal order the old per-query iterator used, so the table
/// *is* the reference enumeration order by construction.
///
/// Pre-order is also what makes each node's words contiguous: a node's range
/// opens when the walk first reaches it (`start = keys.len()`) and closes
/// when its frame pops (`end = keys.len()`), and every word emitted in
/// between lies in its subtree. Depends on `Trie::freeze` preserving child
/// order, which it does — children are carried over in their original
/// positions.
fn key_table(nodes: &[FrozenNode], units: &[u16]) -> (Vec<String>, Vec<(u32, u32)>) {
    /// One level of the walk; the restore fields turn a fresh string per
    /// path into one shared buffer that is pushed and truncated.
    struct Frame {
        node: u32,
        next: usize,
        restore_len: usize,
        restore_pending: Option<u16>,
    }

    let mut keys: Vec<String> = Vec::new();
    let mut word_range = vec![(0u32, 0u32); nodes.len()];
    let mut buf = String::new();
    let mut pending: Option<u16> = None;
    let mut stack: Vec<Frame> = Vec::with_capacity(32);

    // The root's range opens at 0 (already so) and its own word — the empty
    // string — is emitted before any descent, exactly as the iterator did.
    if nodes[ROOT as usize].is_word {
        keys.push(String::new());
    }
    stack.push(Frame {
        node: ROOT,
        next: 0,
        restore_len: 0,
        restore_pending: None,
    });

    while let Some(top) = stack.last_mut() {
        let node = top.node;
        let next = top.next;
        let children = &nodes[node as usize].children;

        if next >= children.len() {
            let frame = stack.pop().expect("frame checked above");
            word_range[frame.node as usize].1 = keys.len() as u32;
            buf.truncate(frame.restore_len);
            pending = frame.restore_pending;
            continue;
        }

        top.next += 1;
        let child = children[next];
        let restore_len = buf.len();
        let restore_pending = pending;
        for &unit in &units[child.label_start as usize..child.label_end as usize] {
            push_unit(&mut buf, &mut pending, unit);
        }
        word_range[child.node as usize].0 = keys.len() as u32;
        if nodes[child.node as usize].is_word {
            keys.push(buf.clone());
        }
        stack.push(Frame {
            node: child.node,
            next: 0,
            restore_len,
            restore_pending,
        });
    }

    (keys, word_range)
}

impl<'a> IntoIterator for &'a FrozenTrie {
    type Item = String;
    type IntoIter = FrozenKeysWithPrefix<'a>;

    /// Iterates every stored word, in the reference's enumeration order —
    /// see [`FrozenTrie::keys`].
    fn into_iter(self) -> Self::IntoIter {
        self.keys()
    }
}

// ---------------------------------------------------------------------------
// keysWithPrefix, frozen
// ---------------------------------------------------------------------------

/// Iterator over the stored words beneath a prefix, in the reference order —
/// the frozen counterpart of `verbora_trie::KeysWithPrefix`.
///
/// Created by [`FrozenTrie::iter_keys_with_prefix`] and [`FrozenTrie::keys`].
/// Same sequence as the mutable trie's iterator, but no walk happens here at
/// all: the prefix descent ran at construction and pinned down the subtree's
/// contiguous range of the precomputed key table, so this is a cursor over a
/// `&[String]` — which is why `count`, `nth`, and `size_hint` are O(1), and
/// why each item costs one clone rather than a path reconstruction.
pub struct FrozenKeysWithPrefix<'t> {
    /// The remaining words, narrowing from the front as the cursor advances.
    words: std::slice::Iter<'t, String>,
}

impl<'t> FrozenKeysWithPrefix<'t> {
    fn new(trie: &'t FrozenTrie, prefix: &str) -> Self {
        // No folding — `keys_slice` preserves the reference bug documented
        // on `Trie::keys_with_prefix`.
        Self {
            words: trie.keys_slice(prefix).iter(),
        }
    }
}

impl Iterator for FrozenKeysWithPrefix<'_> {
    type Item = String;

    fn next(&mut self) -> Option<String> {
        self.words.next().cloned()
    }

    /// O(1): the words left are a slice, and a slice knows its length.
    fn count(self) -> usize {
        self.words.len()
    }

    /// O(1): skips without cloning the skipped words.
    fn nth(&mut self, n: usize) -> Option<String> {
        self.words.nth(n).cloned()
    }

    /// Exact — so `collect` allocates the result vector once.
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.words.size_hint()
    }
}

impl FusedIterator for FrozenKeysWithPrefix<'_> {}

impl std::fmt::Debug for FrozenKeysWithPrefix<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FrozenKeysWithPrefix")
            .field("remaining", &self.words.len())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use crate::Trie;

    /// Compares `Trie` and `FrozenTrie` across every probe both directly:
    /// same `contains` answer, same `keys_with_prefix` sequence.
    fn assert_agrees(t: &Trie, probes_contains: &[&str], probes_prefix: &[&str]) {
        let frozen = t.freeze();
        for &s in probes_contains {
            assert_eq!(
                t.contains(s),
                frozen.contains(s),
                "contains({s:?}) disagrees"
            );
        }
        for &p in probes_prefix {
            assert_eq!(
                t.keys_with_prefix(p),
                frozen.keys_with_prefix(p),
                "keys_with_prefix({p:?}) disagrees"
            );
        }
    }

    #[test]
    fn empty_trie_freezes_to_one_node_and_matches() {
        let t = Trie::new();
        let frozen = t.freeze();
        assert_eq!(frozen.get_size(), 1);
        assert!(!frozen.contains(""));
        assert!(!frozen.contains("a"));
        assert!(frozen.keys_with_prefix("").is_empty());
        assert_agrees(&t, &["", "a", "anything"], &["", "a"]);
    }

    #[test]
    fn empty_string_is_a_word_that_creates_no_frozen_node_either() {
        let mut t = Trie::new();
        t.add_string("");
        let frozen = t.freeze();
        assert_eq!(frozen.get_size(), 1, "the root already existed");
        assert!(frozen.contains(""));
        assert_eq!(frozen.keys_with_prefix(""), [""]);
        assert_agrees(&t, &[""], &[""]);
    }

    #[test]
    fn a_lone_branch_free_word_compresses_to_one_edge() {
        let mut t = Trie::new();
        t.add_string("hello");
        let frozen = t.freeze();
        // root + one compressed edge, vs. 6 original per-code-unit nodes.
        assert_eq!(t.get_size(), 6);
        assert_eq!(frozen.get_size(), 2);
        assert!(frozen.contains("hello"));
        assert!(!frozen.contains("hell"));
        assert!(!frozen.contains("helloo"));
        assert_agrees(
            &t,
            &["hello", "hell", "helloo", "h", "", "xyz"],
            &["hel", "help", "hello", "hellox", "", "x"],
        );
    }

    #[test]
    fn a_shorter_word_along_the_chain_forces_a_kept_node_there() {
        // "car" is a real word AND a prefix of "care"/"careful", so freeze
        // must NOT compress across the "car" node -- it has to stay a real,
        // independently reachable stopping point.
        let mut t = Trie::new();
        t.add_strings(["cat", "cats", "car", "care", "careful"]);
        assert_agrees(
            &t,
            &["cat", "cats", "car", "care", "careful", "ca", "cares", ""],
            &["ca", "car", "care", "cat", "c", "", "z"],
        );
    }

    #[test]
    fn branching_and_shared_prefixes_agree() {
        let mut t = Trie::new();
        t.add_strings([
            "a", "ab", "abc", "abcd", "abx", "b", "bc", "bcd", "z", "zz", "zzz",
        ]);
        assert_agrees(
            &t,
            &[
                "a", "ab", "abc", "abcd", "abx", "b", "bc", "bcd", "z", "zz", "zzz", "abcde", "",
            ],
            &["", "a", "ab", "abc", "abx", "b", "z", "zz", "abcdefgh", "q"],
        );
    }

    #[test]
    fn digit_hoisted_children_survive_freezing_in_order() {
        let mut t = Trie::new();
        t.add_strings(["zb", "z9", "za", "z0", "z1"]);
        let frozen = t.freeze();
        assert_eq!(frozen.keys_with_prefix("z"), ["z0", "z1", "z9", "zb", "za"]);
        assert_eq!(t.keys_with_prefix("z"), frozen.keys_with_prefix("z"));
    }

    #[test]
    fn digit_hoisting_and_compression_interact_correctly() {
        // Every branch here is itself a multi-character run, so freezing
        // compresses each one into an edge -- the digit-vs-letter ordering
        // rule applies to the *first* unit of each compressed edge.
        let mut t = Trie::new();
        t.add_strings(["banana", "9lives", "1derful", "0zone", "zebra"]);
        assert_agrees(
            &t,
            &["banana", "9lives", "1derful", "0zone", "zebra", "b"],
            &["", "b", "9", "1", "0", "z"],
        );
        assert_eq!(t.keys_with_prefix(""), t.freeze().keys_with_prefix(""));
    }

    #[test]
    fn case_insensitive_folding_and_the_keys_with_prefix_bug_both_survive() {
        let mut t = Trie::case_insensitive();
        t.add_strings(["thEIr", "And", "theY"]);
        let frozen = t.freeze();

        assert!(frozen.contains("THEIR"));
        assert!(frozen.contains("their"));
        assert_eq!(frozen.keys_with_prefix("th"), ["their", "they"]);
        // The preserved reference bug: an uppercase prefix matches nothing,
        // even on a case-insensitive trie.
        assert!(frozen.keys_with_prefix("TH").is_empty());
        assert!(!frozen.is_case_sensitive());

        assert_agrees(
            &t,
            &["their", "THEIR", "and", "AND", "they", "THEY", "the"],
            &["th", "TH", "an", "AND", ""],
        );
    }

    #[test]
    fn astral_characters_and_surrogate_pair_boundaries_agree() {
        let mut t = Trie::new();
        t.add_strings(["a👍", "a👍b", "😀", "𝕳𝖊𝖑𝖑𝖔"]);
        let frozen = t.freeze();

        assert!(frozen.contains("a👍"));
        assert!(frozen.contains("a👍b"));
        assert!(frozen.contains("😀"));
        assert!(frozen.contains("𝕳𝖊𝖑𝖑𝖔"));
        // U+1F44C shares 👍's high surrogate but not its low one.
        assert!(!frozen.contains("a👌"));

        assert_agrees(
            &t,
            &["a👍", "a👍b", "😀", "𝕳𝖊𝖑𝖑𝖔", "a👌", "a", ""],
            &["a", "a👍", "", "😀", "𝕳"],
        );
    }

    #[test]
    fn cjk_and_cyrillic_and_greek_agree() {
        let mut t = Trie::new();
        t.add_strings([
            "日本語",
            "日本",
            "中文测试",
            "한국어",
            "Москва",
            "Москвич",
            "Ελλάδα",
            "Ελλάς",
        ]);
        assert_agrees(
            &t,
            &[
                "日本語",
                "日本",
                "中文测试",
                "한국어",
                "Москва",
                "Ελλάδα",
                "日本語です",
            ],
            &["日本", "中文", "Москв", "Ελλ", "", "한"],
        );
    }

    #[test]
    fn prototype_polluting_keys_freeze_like_ordinary_words() {
        let mut t = Trie::new();
        t.add_strings(["__proto__", "constructor", "toString"]);
        assert_agrees(
            &t,
            &["__proto__", "constructor", "toString", "valueOf"],
            &["", "__", "constr", "to"],
        );
    }

    #[test]
    fn very_long_input_freezes_and_queries_without_overflowing_the_stack() {
        // freeze()'s pass-through walk and every FrozenTrie query here are
        // plain loops, not recursion -- this pins that down instead of
        // trusting it.
        let long: String = "ab".repeat(100_000);
        let mut t = Trie::new();
        t.add_string(&long);
        let frozen = t.freeze();
        assert!(frozen.contains(&long));
        assert!(!frozen.contains(&long[..long.len() - 1]));
        assert_eq!(frozen.keys_with_prefix(&long[..1000]).len(), 1);
        assert_eq!(frozen.keys().count(), 1);
        // One word, no other branch: root + exactly one compressed edge.
        assert_eq!(frozen.get_size(), 2);
    }

    #[test]
    fn into_iterator_matches_keys() {
        let mut t = Trie::new();
        t.add_strings(["a", "ab", "abc", "b", "0z", "9z", "😀", "日本"]);
        let frozen = t.freeze();
        assert_eq!(
            frozen.keys().collect::<Vec<_>>(),
            (&frozen).into_iter().collect::<Vec<_>>()
        );
        assert_eq!(t.keys_with_prefix(""), frozen.keys().collect::<Vec<_>>());
    }

    #[test]
    fn iter_keys_with_prefix_matches_its_eager_counterpart() {
        let mut t = Trie::new();
        t.add_strings(["a", "ab", "abc", "abcd", "b"]);
        let frozen = t.freeze();
        assert_eq!(
            frozen.keys_with_prefix("a"),
            frozen.iter_keys_with_prefix("a").collect::<Vec<_>>()
        );
    }

    // -----------------------------------------------------------------------
    // Randomized cross-check: many tries, many probes, no shortcuts.
    // -----------------------------------------------------------------------

    /// A tiny, dependency-free PRNG — this crate has no `rand` dev-dependency
    /// and does not need one just for this. Same shape as the Xorshift64
    /// generator `verbora-distance`'s own randomized Levenshtein tests use.
    struct Xorshift64(u64);

    impl Xorshift64 {
        fn next_u64(&mut self) -> u64 {
            let mut x = self.0;
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            self.0 = x;
            x
        }

        fn next_range(&mut self, bound: usize) -> usize {
            (self.next_u64() % bound as u64) as usize
        }
    }

    const ALPHABET: &[char] = &[
        'a', 'b', 'c', 'd', 'e', '0', '1', '9', 'ñ', 'é', '中', '😀', '👍',
    ];

    fn random_word(rng: &mut Xorshift64, max_len: usize) -> String {
        let len = rng.next_range(max_len + 1);
        (0..len)
            .map(|_| ALPHABET[rng.next_range(ALPHABET.len())])
            .collect()
    }

    #[test]
    fn randomized_tries_agree_on_contains_and_keys_with_prefix() {
        let mut rng = Xorshift64(0x5EED_C0FF_EE15_5AFE);

        for round in 0..40 {
            let case_insensitive = round % 3 == 0;
            let mut t = if case_insensitive {
                Trie::case_insensitive()
            } else {
                Trie::new()
            };

            let word_count = 5 + rng.next_range(60);
            let mut words: Vec<String> =
                (0..word_count).map(|_| random_word(&mut rng, 6)).collect();
            // Every so often, include the empty string too.
            if round % 5 == 0 {
                words.push(String::new());
            }
            t.add_strings(&words);
            let frozen = t.freeze();

            // Probe every stored word, plus every prefix of every stored
            // word (the cases most likely to land mid-compressed-edge), plus
            // a handful of pure-random strings that likely miss entirely.
            let mut probes: Vec<String> = Vec::new();
            for w in &words {
                probes.push(w.clone());
                let mut acc = String::new();
                for ch in w.chars() {
                    acc.push(ch);
                    probes.push(acc.clone());
                }
            }
            for _ in 0..20 {
                probes.push(random_word(&mut rng, 6));
            }

            for p in &probes {
                assert_eq!(
                    t.contains(p),
                    frozen.contains(p),
                    "round {round}: contains({p:?}) disagrees (case_insensitive={case_insensitive})"
                );
                assert_eq!(
                    t.keys_with_prefix(p),
                    frozen.keys_with_prefix(p),
                    "round {round}: keys_with_prefix({p:?}) disagrees (case_insensitive={case_insensitive})"
                );
            }
        }
    }

    // -----------------------------------------------------------------------
    // The precomputed key table: keys_slice, ranges, and the range cursor
    // -----------------------------------------------------------------------

    #[test]
    fn keys_slice_agrees_with_keys_with_prefix_everywhere() {
        let mut t = Trie::new();
        t.add_strings(["cat", "cats", "car", "care", "0x", "9x", "a👍", "a👍b", ""]);
        let frozen = t.freeze();
        for p in [
            "", "c", "ca", "car", "care", "cares", "cat", "0", "a", "a👍", "z",
        ] {
            assert_eq!(
                frozen.keys_slice(p),
                frozen.keys_with_prefix(p).as_slice(),
                "keys_slice({p:?})"
            );
            assert_eq!(
                frozen.keys_slice(p),
                t.keys_with_prefix(p).as_slice(),
                "keys_slice({p:?}) vs the mutable walk"
            );
        }
        assert!(frozen.keys_slice("nothing-here").is_empty());
    }

    #[test]
    fn keys_slice_preserves_the_no_folding_bug() {
        let mut t = Trie::case_insensitive();
        t.add_strings(["thEIr", "And", "theY"]);
        let frozen = t.freeze();
        assert_eq!(frozen.keys_slice("th"), ["their", "they"]);
        // The preserved reference bug: the prefix argument is never folded,
        // so an uppercase prefix matches nothing even here.
        assert!(frozen.keys_slice("TH").is_empty());
    }

    #[test]
    fn frozen_iterator_count_nth_and_size_hint_are_exact() {
        let mut t = Trie::new();
        t.add_strings(["a", "ab", "abc", "abd", "b", "😀", "😀a"]);
        let frozen = t.freeze();
        for p in ["", "a", "ab", "😀", "zz"] {
            let full: Vec<String> = frozen.iter_keys_with_prefix(p).collect();
            assert_eq!(frozen.iter_keys_with_prefix(p).count(), full.len());
            assert_eq!(
                frozen.iter_keys_with_prefix(p).size_hint(),
                (full.len(), Some(full.len()))
            );
            // count after partial consumption, at every stopping point.
            for k in 0..=full.len() {
                let mut it = frozen.iter_keys_with_prefix(p);
                for expected in full.iter().take(k) {
                    assert_eq!(it.next().as_ref(), Some(expected));
                }
                assert_eq!(it.count(), full.len() - k, "after {k} of {p:?}");
            }
            // nth skips exactly, without disturbing what follows.
            for n in 0..full.len() {
                let mut it = frozen.iter_keys_with_prefix(p);
                assert_eq!(it.nth(n).as_ref(), Some(&full[n]), "nth({n}) of {p:?}");
                assert_eq!(it.next().as_ref(), full.get(n + 1), "after nth({n})");
            }
        }
        // Fused after exhaustion.
        let mut it = frozen.iter_keys_with_prefix("zz");
        assert_eq!(it.next(), None);
        assert_eq!(it.next(), None);
    }

    #[test]
    fn randomized_key_table_and_membership_agree_with_the_mutable_walk() {
        // Same differential shape as the older randomized test above, but
        // aimed at the precomputed surfaces it does not cover: `keys_slice`,
        // the O(1) iterator count, and `contains` against the *arena walk*
        // oracle (both `Trie::contains` and `FrozenTrie::contains` answer
        // from hash sets now, so agreeing with each other would not prove
        // the sets right — the walk is the independent witness).
        let mut rng = Xorshift64(0xFEED_FACE_CAFE_BEEF);
        for round in 0..40 {
            let case_insensitive = round % 3 == 0;
            let mut t = if case_insensitive {
                Trie::case_insensitive()
            } else {
                Trie::new()
            };
            let word_count = 5 + rng.next_range(60);
            let mut words: Vec<String> =
                (0..word_count).map(|_| random_word(&mut rng, 6)).collect();
            if round % 5 == 0 {
                words.push(String::new());
            }
            t.add_strings(&words);
            let frozen = t.freeze();

            let mut probes: Vec<String> = Vec::new();
            for w in &words {
                probes.push(w.clone());
                probes.push(w.to_uppercase());
                let mut acc = String::new();
                for ch in w.chars() {
                    acc.push(ch);
                    probes.push(acc.clone());
                }
            }
            probes.push(String::new());
            for _ in 0..20 {
                probes.push(random_word(&mut rng, 6));
            }

            for p in &probes {
                let want_keys = t.keys_with_prefix(p);
                assert_eq!(
                    frozen.keys_slice(p),
                    want_keys.as_slice(),
                    "r{round}: keys_slice({p:?})"
                );
                assert_eq!(
                    frozen.iter_keys_with_prefix(p).count(),
                    want_keys.len(),
                    "r{round}: frozen count({p:?})"
                );
                assert_eq!(
                    t.iter_keys_with_prefix(p).count(),
                    want_keys.len(),
                    "r{round}: mutable count({p:?})"
                );
                let want_contains = t.contains_walk(p);
                assert_eq!(t.contains(p), want_contains, "r{round}: contains({p:?})");
                assert_eq!(
                    frozen.contains(p),
                    want_contains,
                    "r{round}: frozen contains({p:?})"
                );
            }
        }
    }
}
