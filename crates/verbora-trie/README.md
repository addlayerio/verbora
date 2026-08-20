# verbora-trie

An arena-backed prefix tree over `&str` keys: membership, every key under a
prefix, every stored word that prefixes a search string, and the longest such
word with the remainder it left behind. For dictionary lookup, autocomplete,
greedy segmentation and anywhere a `HashSet` would answer *is this in the
set?* but not *what starts with this?*. Nodes live in one arena rather than
one heap allocation per node.

## The contract

**The unit is one Unicode scalar value**: a key is a `&str`, the smallest
piece of one that is itself text is a scalar, so that is what labels an edge.
**Enumeration order is specified, not incidental** — ascending by scalar
sequence, which for well-formed Rust strings is exactly `<str as Ord>`.
**Case handling is fixed at construction** (`CaseHandling`) and applies to
every argument of every method, with no exceptions. **Nothing panics** for
any input, short of capacity overflow on an arena larger than `u32::MAX`
nodes.

**Keys come back owned, except where they can honestly be borrowed.** A
stored word exists nowhere contiguously in a mutable `Trie` — it is spelled
out one scalar per arena node — so `keys_with_prefix` and
`iter_keys_with_prefix` allocate a `String` per word, and
`for_each_key_with_prefix` lends one shared buffer for the duration of the
call instead. `prefix_matches` and `longest_prefix` are the exception: their
results are cut from the *caller's* search string, so on a case-sensitive
trie they are `Cow::Borrowed` and allocate nothing. `Trie::freeze` pays one
linear pass for a `FrozenTrie` whose `keys_slice` hands back `&[String]`
directly.

## Example

```rust
use verbora_trie::Trie;

let mut t = Trie::new();
t.insert_all(["cat", "cats", "car", "cart", "dog"]);

assert!(t.contains("cart"));

// Ascending by scalar sequence, and owned on the way out.
assert_eq!(t.keys_with_prefix("ca"), ["car", "cart", "cat", "cats"]);

// Counting reads a maintained subtree count: O(|prefix|), no traversal.
assert_eq!(t.iter_keys_with_prefix("ca").count(), 4);

// Nothing allocated per word: the closure is lent a shared buffer.
let mut total = 0;
t.for_each_key_with_prefix("ca", |k| total += k.len());
assert_eq!(total, 3 + 4 + 3 + 4);

// Cut from the caller's own string, shortest first, so these borrow.
assert_eq!(t.prefix_matches("cartload"), ["car", "cart"]);
```

## See also

Full documentation, including the trade between the four ways to enumerate a
prefix and when freezing pays for itself:
<https://verbora.dev/features/trie>.

A trie answers prefix questions exactly. If what you wanted was *approximate*
lookup — words within some edit distance of a misspelling — that is
`verbora-spellcheck`; for the underlying metrics, `verbora-distance`; for
retrieving words that sound alike, `verbora-phonetics`.
