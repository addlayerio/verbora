//! An arena-backed prefix tree over `&str` keys.
//!
//! ```
//! use verbora_trie::Trie;
//!
//! let mut t = Trie::new();
//! t.insert_all(["a", "ab", "bc", "cd", "abc"]);
//!
//! assert!(t.contains("abc"));
//! assert_eq!(t.prefix_matches("abcd"), ["a", "ab", "abc"]);
//! assert_eq!(t.keys_with_prefix("ab"), ["ab", "abc"]);
//! ```
//!
//! # The contract, in four lines
//!
//! * **The unit is one Unicode scalar value.** A key is a `&str`; the smallest
//!   piece of one that is itself text is a scalar, so that is what labels an
//!   edge. Every node in the tree is therefore a position a caller can name,
//!   and every string this crate hands back is text the caller supplied — see
//!   [`Trie`]'s own "The unit" section.
//! * **Enumeration is ascending by scalar sequence**, which for well-formed
//!   Rust strings is exactly `<str as Ord>`. Nothing is left to "whatever order
//!   the structure happens to produce".
//! * **Case handling is fixed at construction and applies to every argument of
//!   every method**, with no exceptions — see [`CaseHandling`].
//! * **Nothing panics** for any input, short of capacity overflow on an arena
//!   larger than `u32::MAX` nodes.
//!
//! # Keys are owned on the way out
//!
//! A stored word exists nowhere contiguously in the mutable [`Trie`] — it is
//! spelled out one scalar per arena node — so every key-returning API either
//! allocates a `String` per word ([`Trie::keys_with_prefix`],
//! [`Trie::iter_keys_with_prefix`]) or lends a shared buffer for the duration
//! of one call ([`Trie::for_each_key_with_prefix`]). Only
//! [`FrozenTrie::keys_slice`], which reads a precomputed table, can hand back
//! `&[String]`.
//!
//! [`Trie::prefix_matches`] and [`Trie::longest_prefix`] are the exception:
//! their results are cut from the *caller's* search string, so on a
//! case-sensitive trie they are `Cow::Borrowed` and allocate nothing.
//!
//! # Choosing the Right API
//!
//! Three groups of paired APIs exist here. Each pair is a real trade, not a
//! style preference.
//!
//! ## Enumerating the keys under a prefix
//!
//! | API | Yields | Allocates | Reach for it when |
//! |---|---|---|---|
//! | [`Trie::keys_with_prefix`] | `Vec<String>` | one `Vec` + one `String` per word | you want them all and you want to keep them. **The default choice.** |
//! | [`Trie::iter_keys_with_prefix`] | `String` per `next` | one path buffer + one `String` per yielded word | you may stop early (`take`, `find`, `any`), or you only want the count |
//! | [`Trie::for_each_key_with_prefix`] | `&str` per call | one path buffer, nothing per word | each word is consumed and dropped — counted, summed, written to a sink |
//! | [`FrozenTrie::keys_slice`] | `&[String]` | nothing | the same trie is enumerated repeatedly and you can afford one [`Trie::freeze`] |
//!
//! Decision tree:
//!
//! ```text
//! need to keep the strings?
//!  ├── no  ── enumerating once?  ── yes ── for_each_key_with_prefix
//!  │                             └─ no  ── freeze() once, then keys_slice
//!  └── yes ─ might stop early?   ── yes ── iter_keys_with_prefix
//!                                └─ no  ── keys_with_prefix
//! ```
//!
//! Counting is a special case: `iter_keys_with_prefix(p).count()` is O(|p|) —
//! it reads a maintained subtree word count rather than walking anything — so
//! it is always the right way to ask "how many words start with `p`?", and
//! `keys_with_prefix(p).len()` is the wrong way.
//!
//! ```
//! # use verbora_trie::Trie;
//! let mut t = Trie::new();
//! t.insert_all(["cat", "cats", "car", "cart", "dog"]);
//!
//! assert_eq!(t.keys_with_prefix("ca"), ["car", "cart", "cat", "cats"]);
//! assert_eq!(t.iter_keys_with_prefix("ca").take(2).collect::<Vec<_>>(), ["car", "cart"]);
//! assert_eq!(t.iter_keys_with_prefix("ca").count(), 4); // no traversal
//!
//! let mut total = 0;
//! t.for_each_key_with_prefix("ca", |k| total += k.len()); // no allocation
//! assert_eq!(total, 3 + 4 + 3 + 4);
//! ```
//!
//! ## Words that prefix a search string
//!
//! | API | Answers | Reach for it when |
//! |---|---|---|
//! | [`Trie::prefix_matches`] | *every* stored word that prefixes the search, shortest first | you want them all — tokenisation against a dictionary, say |
//! | [`Trie::iter_prefix_matches`] | the same, lazily | you want the shortest, or a bounded number |
//! | [`Trie::longest_prefix`] | the **longest** one, plus the unconsumed remainder | you are consuming the input left to right — greedy segmentation |
//! | [`Trie::longest_prefix_lengths`] | the same split as scalar counts | you only need the offsets, and want zero allocation even on a folding trie |
//!
//! ## Mutable vs frozen
//!
//! [`Trie`] is the one you build with; it is the only one that can be inserted
//! into. [`Trie::freeze`] pays one linear pass to precompute a
//! [`FrozenTrie`] whose enumerations are range copies and whose `contains` is a
//! hash probe. Freeze when the same trie will be queried many times and never
//! changed again; do not freeze for a handful of lookups — the freeze itself
//! costs more than they do. `FrozenTrie` deliberately does not implement
//! [`Trie::prefix_matches`] or [`Trie::longest_prefix`]; call those on the
//! original.
//!
//! No performance figure appears anywhere in this crate's documentation that
//! has not been measured; where a trade-off is claimed without a number, the
//! claim is about allocation behaviour, which is visible in the signatures.

mod frozen;
mod iter;
mod membership;
mod trie;

pub use frozen::{FrozenKeysWithPrefix, FrozenTrie};
pub use iter::{KeysWithPrefix, PrefixMatches};
pub use trie::{CaseHandling, PrefixSplit, PrefixSplitLengths, Trie};
