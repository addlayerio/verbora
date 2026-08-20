//! Lazy iterators over the trie, and the straight-line bulk walk behind
//! [`Trie::keys_with_prefix`](crate::Trie::keys_with_prefix).
//!
//! Every enumeration here emits words in ascending order of their scalar
//! sequence — see [`Trie`]'s own "Order" section. Because one node label is one
//! whole Unicode scalar, spelling a word out is a plain `String::push`: there
//! is no half-character to hold back between edges.

use std::borrow::Cow;
use std::iter::FusedIterator;

use crate::trie::{Child, ROOT, Trie};

// ---------------------------------------------------------------------------
// keys with prefix
// ---------------------------------------------------------------------------

/// Runs the same depth-first walk [`KeysWithPrefix`] yields, handing each
/// stored word to `emit` as a borrow of the shared path buffer.
///
/// `folded` must already have been through
/// [`fold`](crate::trie::fold) — every caller of this is a `Trie` method that
/// folds its own argument first, which is what makes case handling uniform
/// across the whole API.
///
/// # Why this exists alongside the iterator
///
/// [`KeysWithPrefix`] has to *suspend* at every word: the arena cursor and the
/// path buffer live behind `&mut self` and are re-read from memory on each
/// `next`, and the traversal position has to be re-derived from the stack top
/// before any progress is made. A caller that wants the whole subtree pays that
/// per-word ceremony ~20 000 times for nothing. Here the same state is a set of
/// locals the optimiser keeps in registers across the entire subtree.
///
/// # Shape, and what each part of it buys
///
/// The stack holds *pending edges*, not open nodes. A node is therefore read
/// exactly once — the walk never returns to a parent to ask what its next child
/// was, which was the one read guaranteed to miss because a whole subtree had
/// been traversed since the last one.
///
/// On top of that the inner loop runs a **single-child chain in place**. Most
/// nodes in a natural-language trie mark no word and have exactly one child:
/// pure spelling, no decision. Those cost one label append and one arena hop
/// here instead of also a stack push, a stack pop, a buffer truncation and a
/// child-list copy. It is the same observation
/// [`Trie::freeze`](crate::Trie::freeze) makes structurally, applied at walk
/// time — `Trie` itself cannot compress those nodes away, because
/// [`Trie::node_count`](crate::Trie::node_count) counts them.
///
/// Iterative, not recursive, for the reason the rest of this crate is: the
/// chain depth is the *word length*, and a 100 kB key must not overflow.
pub(crate) fn walk_keys<F: FnMut(&str)>(trie: &Trie, folded: &str, mut emit: F) {
    let Some(start) = trie.descend_folded(folded) else {
        return;
    };
    // Results are the folded prefix concatenated with stored edge labels, so
    // seeding the buffer with it is exactly right.
    let mut buf = String::from(folded);
    let mut stack: Vec<PendingEdge> = Vec::new();

    let root = trie.node(start);
    if root.is_word {
        emit(&buf);
    }
    push_edges(&mut stack, &root.children, buf.len());

    while let Some(edge) = stack.pop() {
        // The buffer only ever grew past `restore_len` while the siblings
        // pushed after this edge were being walked, so truncating back to it
        // is always a shortening and always lands on the exact spelling this
        // edge's parent had.
        buf.truncate(edge.restore_len);
        let mut key = edge.key;
        let mut at = edge.node;

        loop {
            buf.push(key);
            let node = trie.node(at);
            if node.is_word {
                emit(&buf);
            }
            // A node with exactly one child has no sibling that could ever
            // need to rewind to it, so there is nothing to record and nobody
            // to come back for: stay in the loop with the buffer exactly as
            // it is. Only a fork or a leaf goes back through the stack.
            match node.children.as_slice() {
                [only] => {
                    key = only.key;
                    at = only.node;
                }
                children => {
                    push_edges(&mut stack, children, buf.len());
                    break;
                }
            }
        }
    }
}

/// Queues `children` so the depth-first walk reaches them in ascending label
/// order.
///
/// Reversed, because the stack is LIFO and the *smallest* label must come out
/// first.
#[inline]
fn push_edges(stack: &mut Vec<PendingEdge>, children: &[Child], restore_len: usize) {
    stack.extend(children.iter().rev().map(|c| PendingEdge {
        node: c.node,
        key: c.key,
        restore_len,
    }));
}

/// One queued edge of the depth-first walk: the child to visit and the buffer
/// length to rewind to before its label is appended.
struct PendingEdge {
    /// Arena index of the node this edge leads to.
    node: u32,
    /// The edge label: one Unicode scalar.
    key: char,
    /// Length `buf` had at the parent, before this edge's label.
    restore_len: usize,
}

/// One level of the depth-first walk.
struct Frame {
    /// Arena index of the node this frame is visiting.
    node: u32,
    /// Index of the next child to descend into.
    next: usize,
    /// Length `buf` had before this node's edge label was appended.
    restore_len: usize,
}

/// Iterator over the stored words beneath a prefix, in ascending scalar order.
///
/// Created by [`Trie::iter_keys_with_prefix`](crate::Trie::iter_keys_with_prefix)
/// and [`Trie::keys`](crate::Trie::keys).
///
/// A node's own word is emitted before its subtree, which follows from the
/// order rather than adding to it: a node's word is a proper prefix of every
/// word beneath it, and a proper prefix sorts first.
pub struct KeysWithPrefix<'t> {
    trie: &'t Trie,
    /// The word being spelled out, reused across the whole traversal.
    buf: String,
    stack: Vec<Frame>,
    /// The node the prefix landed on, consumed by the first call to `next`.
    start: Option<u32>,
    /// How many words this iterator has yet to yield.
    ///
    /// Initialised from the start node's maintained subtree word count and
    /// decremented per item. The trie is borrowed for the iterator's whole
    /// lifetime, so the subtree cannot change underneath it and the number is
    /// *exact* at every point — which is what lets `count` be O(1) instead of
    /// walking the rest of the subtree, and `size_hint` be tight enough that
    /// `collect::<Vec<_>>()` allocates its result exactly once.
    remaining: usize,
}

impl<'t> KeysWithPrefix<'t> {
    /// Positions the iterator at an already-folded `prefix`.
    pub(crate) fn new(trie: &'t Trie, folded: &str) -> Self {
        let start = trie.descend_folded(folded);
        Self {
            trie,
            buf: if start.is_some() {
                String::from(folded)
            } else {
                String::new()
            },
            stack: Vec::new(),
            start,
            remaining: start.map_or(0, |n| trie.node(n).word_count as usize),
        }
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
            });
            if trie.node(node).is_word {
                self.remaining -= 1;
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
                continue;
            }

            let child = children[next];
            self.stack.last_mut().expect("frame checked above").next += 1;

            let restore_len = self.buf.len();
            self.buf.push(child.key);
            self.stack.push(Frame {
                node: child.node,
                next: 0,
                restore_len,
            });

            if trie.node(child.node).is_word {
                self.remaining -= 1;
                return Some(self.buf.clone());
            }
        }
    }

    /// O(1): the maintained subtree word count, minus what was already
    /// yielded — no walk, no string reconstruction. This is exact because the
    /// borrow rules freeze the trie for the iterator's lifetime; see the
    /// `remaining` field.
    fn count(self) -> usize {
        self.remaining
    }

    /// Exact bounds, for the same reason `count` is exact — so `collect`
    /// pre-reserves the whole result vector in one allocation.
    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, Some(self.remaining))
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
// prefix matches
// ---------------------------------------------------------------------------

/// Iterator over the stored words that prefix a search string, shortest first.
///
/// Created by [`Trie::iter_prefix_matches`](crate::Trie::iter_prefix_matches).
///
/// Items are cut from the search string, so on a case-sensitive trie they
/// borrow it and the whole traversal allocates nothing.
pub struct PrefixMatches<'t, 'a> {
    trie: &'t Trie,
    /// The search string after folding: borrowed on a case-sensitive trie,
    /// owned when folding actually changed something.
    folded: Cow<'a, str>,
    /// Arena index of the node the walk has reached.
    node: u32,
    /// Byte offset of the next scalar to consume.
    pos: usize,
    /// Whether the root has been considered yet.
    started: bool,
    /// Set once the walk falls off the tree or runs out of input.
    done: bool,
}

impl<'t, 'a> PrefixMatches<'t, 'a> {
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

impl<'a> Iterator for PrefixMatches<'_, 'a> {
    type Item = Cow<'a, str>;

    fn next(&mut self) -> Option<Cow<'a, str>> {
        if !self.started {
            self.started = true;
            // The root is on every path, so a stored empty string always matches.
            if self.trie.node(ROOT).is_word {
                return Some(self.cut(0));
            }
        }
        if self.done {
            return None;
        }

        while let Some(scalar) = self.folded[self.pos..].chars().next() {
            match self.trie.child(self.node, scalar) {
                Some(next) => self.node = next,
                None => {
                    self.done = true;
                    return None;
                }
            }
            self.pos += scalar.len_utf8();
            if self.trie.node(self.node).is_word {
                return Some(self.cut(self.pos));
            }
        }

        self.done = true;
        None
    }
}

impl FusedIterator for PrefixMatches<'_, '_> {}

impl std::fmt::Debug for PrefixMatches<'_, '_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PrefixMatches")
            .field("consumed", &self.pos)
            .field("done", &self.done)
            .finish_non_exhaustive()
    }
}
