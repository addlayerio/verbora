//! An arena-backed prefix tree for Rust.
//!
//! ```
//! use verbora_trie::Trie;
//!
//! let mut t = Trie::new();
//! t.add_strings(["a", "ab", "bc", "cd", "abc"]);
//!
//! assert!(t.contains("abc"));
//! assert_eq!(t.find_matches_on_path("abcd"), ["a", "ab", "abc"]);
//! assert_eq!(t.keys_with_prefix("ab"), ["ab", "abc"]);
//! ```
//!
//! # What "parity" costs here
//!
//! Three properties of the reference are observable through its public API and
//! are reproduced exactly, even though none of them is what a fresh design would
//! choose:
//!
//! 1. **Nodes are keyed by UTF-16 code units, not characters.** `'😀'` occupies
//!    *two* levels of the tree, so `get_size` counts it twice and a walk can die
//!    between the halves of a surrogate pair. A `char`-keyed port reports
//!    different sizes and different [`Trie::find_prefix`] splits. See
//!    [`Trie::get_size`].
//!
//! 2. **Child iteration follows the reference's `for…in` order**, which is *not*
//!    insertion order: keys that look like array indices come first, in
//!    ascending numeric order, and only then the rest in insertion order. For
//!    single code units the array-index-like keys are exactly `'0'`–`'9'`, so a
//!    trie built from `["b1", "a1", "9x", "1x", "0x", "zz"]` enumerates as
//!    `["0x", "1x", "9x", "b1", "a1", "zz"]`. Neither a `HashMap` nor a
//!    `BTreeMap` nor a plain insertion-ordered list reproduces that; see
//!    [`Trie::keys_with_prefix`].
//!
//! 3. **`keysWithPrefix` never folds case**, because the reference tests a
//!    property (`this.caseSensitive`) that its constructor never sets — it
//!    stores the flag as `this.cs`. Every *other* method folds correctly. The
//!    bug is load-bearing for anyone relying on the recorded behaviour, so it is
//!    preserved and documented rather than fixed. See
//!    [`Trie::keys_with_prefix`].
//!
//! # How this port differs structurally
//!
//! The reference allocates one the reference object per node, each holding its own
//! hash map, and recurses once per code unit for every operation. This port:
//!
//! * stores all nodes in a single flat arena (`Vec<Node>`) addressed by `u32`,
//!   so a trie is two allocations rather than one per node and `get_size`
//!   becomes O(1) instead of a full traversal;
//! * keeps each node's children in a small inline vector, which holds the
//!   common one- and two-child cases without touching the heap at all, and
//!   maintains it in the reference enumeration order so that iteration needs no
//!   sort;
//! * folds case **once** at the entry point instead of re-lowercasing the
//!   remaining suffix at every level (the reference is quadratic in word
//!   length here; folding is idempotent, so one pass is observably identical);
//! * runs every operation iteratively, so a 100 KB input cannot overflow the
//!   stack the way the reference's per-code-unit recursion does.
//!
//! # Divergence: unpaired surrogates in `find_prefix`
//!
//! Because the walk advances one code unit at a time, the remainder
//! [`Trie::find_prefix`] returns can begin *inside* a surrogate pair — the reference
//! renders that as an unpaired surrogate, which a Rust `String` cannot hold. The
//! remainder is therefore returned with `U+FFFD` in that one position. The split
//! point itself is exact, and [`Trie::find_prefix_lengths`] reports it in code
//! units with no loss at all.

mod frozen;
mod iter;
mod trie;

pub use frozen::{FrozenKeysWithPrefix, FrozenTrie};
pub use iter::{KeysWithPrefix, MatchesOnPath};
pub use trie::Trie;
