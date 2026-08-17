//! Lazy iterators over the trie.
//!
//! The reference builds its results by recursion into a result array; both
//! iterators here reproduce the same sequences without materialising them, so a
//! caller that wants the first match, or the first ten keys, pays only for those.

use std::borrow::Cow;
use std::iter::FusedIterator;

use crate::trie::{ROOT, Trie};

// ---------------------------------------------------------------------------
// keysWithPrefix
// ---------------------------------------------------------------------------

/// One level of the depth-first walk.
///
/// The restore fields are what turns the reference's `stringAgg + c` — a fresh
/// string per edge — into a single buffer that is pushed and truncated.
struct Frame {
    /// Arena index of the node this frame is visiting.
    node: u32,
    /// Index of the next child to descend into.
    next: usize,
    /// Length `buf` had before this node's edge label was appended.
    restore_len: usize,
    /// The half-pair carried before this node's edge label was appended.
    restore_pending: Option<u16>,
}

/// Iterator over the stored words beneath a prefix, in the reference order.
///
/// Created by [`Trie::iter_keys_with_prefix`] and [`Trie::keys`].
///
/// Pre-order: a node's own word is emitted before its children are visited, and
/// children are visited ASCII-digits-first (ascending) then in insertion order.
pub struct KeysWithPrefix<'t> {
    trie: &'t Trie,
    /// The word being spelled out, reused across the whole traversal.
    buf: String,
    /// A high surrogate whose partner has not arrived yet.
    ///
    /// Nodes are keyed by UTF-16 code unit, so a non-BMP character reaches this
    /// iterator as two separate edges. Holding the first half back until the
    /// second arrives is what lets `buf` stay a plain `String`.
    pending: Option<u16>,
    stack: Vec<Frame>,
    /// The node the prefix landed on, consumed by the first call to `next`.
    start: Option<u32>,
}

impl<'t> KeysWithPrefix<'t> {
    /// Positions the iterator at `prefix`.
    ///
    /// `prefix` is **not** case-folded, even on a case-insensitive trie — see
    /// the note on [`Trie::keys_with_prefix`] for why that is deliberate.
    pub(crate) fn new(trie: &'t Trie, prefix: &str) -> Self {
        let start = trie.descend_exact(prefix);
        Self {
            trie,
            // Results are the caller's prefix concatenated with stored edge
            // labels, so seeding the buffer with the prefix is exactly right —
            // and pointless work if the prefix matched nothing.
            buf: if start.is_some() {
                String::from(prefix)
            } else {
                String::new()
            },
            pending: None,
            stack: Vec::new(),
            start,
        }
    }

    /// Appends one edge label to `buf`, reassembling surrogate pairs.
    fn push_unit(&mut self, unit: u16) {
        if let Some(high) = self.pending.take() {
            if let Some(c) = char::decode_utf16([high, unit]).next().and_then(Result::ok) {
                self.buf.push(c);
                return;
            }
            // Unreachable through the public API: every stored word came from a
            // `&str`, so a high surrogate is always followed by its low half.
            // Kept total rather than panicking on a hand-built malformed tree.
            self.buf.push(char::REPLACEMENT_CHARACTER);
        }
        if (0xD800..0xDC00).contains(&unit) {
            self.pending = Some(unit);
            return;
        }
        self.buf
            .push(char::from_u32(u32::from(unit)).unwrap_or(char::REPLACEMENT_CHARACTER));
    }
}

impl Iterator for KeysWithPrefix<'_> {
    type Item = String;

    fn next(&mut self) -> Option<String> {
        // Copying the shared reference out detaches trie reads from the `&mut
        // self` borrows below.
        let trie = self.trie;

        if let Some(node) = self.start.take() {
            self.stack.push(Frame {
                node,
                next: 0,
                restore_len: self.buf.len(),
                restore_pending: None,
            });
            if trie.node(node).is_word {
                return Some(self.buf.clone());
            }
        }

        loop {
            let (node, next) = {
                let frame = self.stack.last()?;
                (frame.node, frame.next)
            };
            let children = &trie.node(node).children;

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
            self.push_unit(child.key);
            self.stack.push(Frame {
                node: child.node,
                next: 0,
                restore_len,
                restore_pending,
            });

            if trie.node(child.node).is_word {
                return Some(self.buf.clone());
            }
        }
    }
}

impl FusedIterator for KeysWithPrefix<'_> {}

impl std::fmt::Debug for KeysWithPrefix<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KeysWithPrefix")
            .field("depth", &self.stack.len())
            .field("current", &self.buf)
            .finish_non_exhaustive()
    }
}

// ---------------------------------------------------------------------------
// findMatchesOnPath
// ---------------------------------------------------------------------------

/// Iterator over the stored words that prefix a search string, shortest first.
///
/// Created by [`Trie::iter_matches_on_path`].
///
/// Items are cut from the search string, so on a case-sensitive trie they borrow
/// it and the whole traversal allocates nothing.
pub struct MatchesOnPath<'t, 'a> {
    trie: &'t Trie,
    /// The search string after folding: borrowed on a case-sensitive trie,
    /// owned when folding actually changed something.
    folded: Cow<'a, str>,
    /// Arena index of the node the walk has reached.
    node: u32,
    /// Byte offset of the next character to consume.
    pos: usize,
    /// Whether the root has been considered yet.
    started: bool,
    /// Set once the walk falls off the tree or runs out of input.
    done: bool,
}

impl<'t, 'a> MatchesOnPath<'t, 'a> {
    pub(crate) fn new(trie: &'t Trie, folded: Cow<'a, str>) -> Self {
        Self {
            trie,
            folded,
            node: ROOT,
            pos: 0,
            started: false,
            done: false,
        }
    }

    /// The first `end` bytes of the folded search, borrowed where possible.
    fn cut(&self, end: usize) -> Cow<'a, str> {
        match &self.folded {
            // Reborrowing at `'a` rather than at the `&self` borrow is what lets
            // the item outlive this call without a copy.
            Cow::Borrowed(s) => {
                let s: &'a str = s;
                Cow::Borrowed(&s[..end])
            }
            Cow::Owned(s) => Cow::Owned(s[..end].to_owned()),
        }
    }
}

impl<'a> Iterator for MatchesOnPath<'_, 'a> {
    type Item = Cow<'a, str>;

    fn next(&mut self) -> Option<Cow<'a, str>> {
        if !self.started {
            self.started = true;
            // The root is on every path, so an added empty string always matches.
            if self.trie.node(ROOT).is_word {
                return Some(self.cut(0));
            }
        }
        if self.done {
            return None;
        }

        while let Some(ch) = self.folded[self.pos..].chars().next() {
            // Non-BMP characters are two edges in the reference, and both must
            // exist for the walk to continue.
            let mut buf = [0u16; 2];
            for &unit in &*ch.encode_utf16(&mut buf) {
                match self.trie.child(self.node, unit) {
                    Some(next) => self.node = next,
                    None => {
                        self.done = true;
                        return None;
                    }
                }
            }
            self.pos += ch.len_utf8();
            if self.trie.node(self.node).is_word {
                return Some(self.cut(self.pos));
            }
        }

        self.done = true;
        None
    }
}

impl FusedIterator for MatchesOnPath<'_, '_> {}

impl std::fmt::Debug for MatchesOnPath<'_, '_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MatchesOnPath")
            .field("consumed", &self.pos)
            .field("done", &self.done)
            .finish_non_exhaustive()
    }
}
