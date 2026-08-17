//! [`FrozenTrie`]: the path-compressed query representation built by
//! [`Trie::freeze`](crate::Trie::freeze).
//!
//! This is a Verbora-native extension — there is no reference `trie`
//! counterpart to port, so nothing here is bound by parity. Its only
//! obligation is to answer `contains`/`keys_with_prefix` identically to the
//! [`Trie`](crate::Trie) it was built from; see `Trie::freeze`'s own doc
//! comment for exactly why compression is exact and what it does not yet
//! cover.

use std::iter::FusedIterator;

use smallvec::SmallVec;

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

/// A read-only, path-compressed prefix tree, built once by
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
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrozenTrie {
    /// Kept (real) nodes, flat. Index 0 is always the frozen root.
    nodes: Vec<FrozenNode>,
    /// Shared buffer every edge's label slices into.
    units: Vec<u16>,
    /// Whether stored and queried strings are lowercased first — inherited
    /// unchanged from the [`Trie`] this was frozen from, since folding
    /// already happened at insertion time and freezing never re-derives it.
    folds: bool,
}

impl FrozenTrie {
    /// Assembles a `FrozenTrie` from already-computed parts.
    ///
    /// Only [`Trie::freeze`](crate::Trie::freeze) calls this — it is the one
    /// place that can guarantee `nodes`/`units` satisfy this type's
    /// invariants (every `FrozenChild::node` is a valid index into `nodes`,
    /// every label range is a valid slice of `units`).
    pub(crate) fn from_parts(nodes: Vec<FrozenNode>, units: Vec<u16>, folds: bool) -> Self {
        Self {
            nodes,
            units,
            folds,
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
    #[must_use]
    pub fn contains(&self, string: &str) -> bool {
        let folded = fold(self.folds, string);
        let units: Vec<u16> = folded.encode_utf16().collect();
        match self.descend_exact(&units) {
            Some(node) => self.nodes[node as usize].is_word,
            None => false,
        }
    }

    /// Every stored word that starts with `prefix` — see
    /// [`Trie::keys_with_prefix`](crate::Trie::keys_with_prefix), including
    /// its "does not fold `prefix`" note, preserved here unchanged since
    /// this walks the same already-folded stored data.
    #[must_use]
    pub fn keys_with_prefix(&self, prefix: &str) -> Vec<String> {
        self.iter_keys_with_prefix(prefix).collect()
    }

    /// [`FrozenTrie::keys_with_prefix`] as a lazy iterator.
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

    /// Consumes every unit in `units`, landing precisely on a node — used by
    /// [`FrozenTrie::contains`], which needs a real stopping point, not a
    /// position strictly inside a compressed edge.
    fn descend_exact(&self, units: &[u16]) -> Option<u32> {
        let mut node = ROOT;
        let mut pos = 0usize;
        while pos < units.len() {
            let child = self.find_child(node, units[pos])?;
            let label = self.label(child);
            let end = pos + label.len();
            if end > units.len() || units[pos..end] != *label {
                return None;
            }
            pos = end;
            node = child.node;
        }
        Some(node)
    }

    /// Consumes every unit in `units`, but may land strictly *inside* an
    /// edge — used by [`FrozenKeysWithPrefix`], which only needs "does a
    /// word start with this", not an exact node.
    ///
    /// A compressed edge has no branching inside it (that is exactly why it
    /// compressed), so once `units` is confirmed to be a genuine prefix of
    /// the edge label, every word reachable from the edge's target node
    /// still has the full original prefix argument as a true prefix — the
    /// caller can start enumerating from `child.node` directly. Returns that
    /// node together with whatever *tail* of the winning edge's label was
    /// not part of `units` — the caller must append it before enumerating,
    /// since `child.node` is only reached by consuming the label in full,
    /// not just the part `units` happened to cover.
    fn descend_prefix(&self, units: &[u16]) -> Option<(u32, &[u16])> {
        let mut node = ROOT;
        let mut pos = 0usize;
        while pos < units.len() {
            let child = self.find_child(node, units[pos])?;
            let label = self.label(child);
            let remaining = units.len() - pos;
            let overlap = remaining.min(label.len());
            if units[pos..pos + overlap] != label[..overlap] {
                return None;
            }
            pos += overlap;
            if overlap < label.len() {
                // `pos == units.len()` here: the query ran out inside this
                // edge. Everything past `overlap` is real path that was
                // never optional — it must be appended to reach `child.node`.
                return Some((child.node, &label[overlap..]));
            }
            node = child.node;
        }
        Some((node, &[]))
    }
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

/// One level of the depth-first walk over a [`FrozenTrie`].
struct Frame {
    node: u32,
    next: usize,
    restore_len: usize,
    restore_pending: Option<u16>,
}

/// Iterator over the stored words beneath a prefix, in the reference order —
/// the frozen counterpart of `verbora_trie::KeysWithPrefix`.
///
/// Created by [`FrozenTrie::iter_keys_with_prefix`] and [`FrozenTrie::keys`].
/// Same pre-order, same the reference `for…in`-derived child order as the
/// mutable trie's iterator; the only structural difference is that each step
/// here pushes a whole compressed edge label instead of one code unit.
pub struct FrozenKeysWithPrefix<'t> {
    trie: &'t FrozenTrie,
    buf: String,
    /// A high surrogate whose partner has not arrived yet — see
    /// `verbora_trie::KeysWithPrefix`'s own field of the same name for why
    /// this makes `buf` stay a plain `String` across a label that happens to
    /// end mid-surrogate-pair (never true within one edge, since an edge is
    /// exactly the code units of whole characters — but a *label boundary*
    /// falls between two nodes at an arbitrary unit, so the pair can still
    /// split there).
    pending: Option<u16>,
    stack: Vec<Frame>,
    start: Option<u32>,
}

impl<'t> FrozenKeysWithPrefix<'t> {
    fn new(trie: &'t FrozenTrie, prefix: &str) -> Self {
        // No folding — mirrors `verbora_trie::KeysWithPrefix::new`'s own
        // preserved reference bug; see `Trie::keys_with_prefix`'s doc.
        let units: Vec<u16> = prefix.encode_utf16().collect();
        let found = trie.descend_prefix(&units);
        let mut this = Self {
            trie,
            buf: if found.is_some() {
                String::from(prefix)
            } else {
                String::new()
            },
            pending: None,
            stack: Vec::new(),
            start: found.map(|(node, _)| node),
        };
        // `prefix` is a well-formed `&str`, so its own UTF-16 encoding can
        // never end mid-surrogate-pair — `pending` is still `None` here, and
        // this is the only place `tail` needs pushing before `next()` runs.
        if let Some((_, tail)) = found {
            this.push_label(tail);
        }
        this
    }

    fn push_unit(&mut self, unit: u16) {
        if let Some(high) = self.pending.take() {
            if let Some(c) = char::decode_utf16([high, unit]).next().and_then(Result::ok) {
                self.buf.push(c);
                return;
            }
            // Unreachable through the public API — see the identical branch
            // in `verbora_trie::KeysWithPrefix::push_unit`.
            self.buf.push(char::REPLACEMENT_CHARACTER);
        }
        if (0xD800..0xDC00).contains(&unit) {
            self.pending = Some(unit);
            return;
        }
        self.buf
            .push(char::from_u32(u32::from(unit)).unwrap_or(char::REPLACEMENT_CHARACTER));
    }

    fn push_label(&mut self, label: &[u16]) {
        for &unit in label {
            self.push_unit(unit);
        }
    }
}

impl Iterator for FrozenKeysWithPrefix<'_> {
    type Item = String;

    fn next(&mut self) -> Option<String> {
        let trie = self.trie;

        if let Some(node) = self.start.take() {
            self.stack.push(Frame {
                node,
                next: 0,
                restore_len: self.buf.len(),
                restore_pending: None,
            });
            if trie.nodes[node as usize].is_word {
                return Some(self.buf.clone());
            }
        }

        loop {
            let (node, next) = {
                let frame = self.stack.last()?;
                (frame.node, frame.next)
            };
            let children = &trie.nodes[node as usize].children;

            if next >= children.len() {
                let frame = self.stack.pop().expect("frame checked above");
                self.buf.truncate(frame.restore_len);
                self.pending = frame.restore_pending;
                continue;
            }

            let child = children[next];
            self.stack.last_mut().expect("frame checked above").next += 1;

            let restore_len = self.buf.len();
            let restore_pending = self.pending;
            self.push_label(trie.label(&child));
            self.stack.push(Frame {
                node: child.node,
                next: 0,
                restore_len,
                restore_pending,
            });

            if trie.nodes[child.node as usize].is_word {
                return Some(self.buf.clone());
            }
        }
    }
}

impl FusedIterator for FrozenKeysWithPrefix<'_> {}

impl std::fmt::Debug for FrozenKeysWithPrefix<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FrozenKeysWithPrefix")
            .field("depth", &self.stack.len())
            .field("current", &self.buf)
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
}
