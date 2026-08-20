# Prefix autocomplete

A search box that suggests completions as the user types. The index is a
[`Trie`](../features/trie.md); the interesting decision is which query API to
call.

## Build the index once

```rust
use verbora_trie::Trie;

fn build(words: &[&str]) -> Trie {
    let mut trie = Trie::new();
    trie.reserve(words.len());        // grow the arena once, not repeatedly
    trie.insert_all(words.iter().copied());
    trie
}

let index = build(&["rust", "rustic", "rusty", "rustle", "ruse", "rush"]);

assert!(index.contains("rusty"));
assert!(!index.contains("rustling"));
```

`Trie` is `Send + Sync`, so an `Arc<Trie>` serves every request. Construction
takes `&mut self` and cannot be parallelised — build it at startup.

## Query: the lazy call is the right one

A suggestion box shows ten results. `keys_with_prefix` builds **all** of them:

```rust
use verbora_trie::Trie;
let mut index = Trie::new();
index.insert_all(["rust", "rustic", "rusty", "rustle", "ruse", "rush"]);

// Materialises every match, then you throw most away.
let all: Vec<String> = index.keys_with_prefix("rust");
assert_eq!(all, ["rust", "rustic", "rustle", "rusty"]);

// Walks only as far as it needs to.
let top: Vec<String> = index.iter_keys_with_prefix("rust").take(3).collect();
assert_eq!(top, ["rust", "rustic", "rustle"]);
```

With four matches the difference is nothing. With a prefix like `"a"` over a
300,000-word dictionary, `keys_with_prefix` reconstructs tens of thousands of
`String`s to show you ten.

<a class="badge badge-lazy" href="../performance/iterator-vs-into">LAZY</a>
<a class="badge badge-owned" href="../performance/allocation">OWNED</a>

Either way the output is a `String` per key — a stored word exists nowhere
contiguously in the arena, so it has to be spelled out one scalar at a time and
there is nothing to borrow. The lazy form allocates one only for each key you
actually take, which is what makes it right for a top-*k* suggestion list. When
you want the whole subtree but not the strings — a count, a checksum, a filter
that keeps only a few — `for_each_key_with_prefix` hands each key to a closure
as a `&str` into a shared path buffer and allocates nothing at all:

```rust
use verbora_trie::Trie;

let mut index = Trie::new();
index.insert_all(["rust", "rustic", "rusty", "rustle", "ruse", "rush"]);

let mut widest = 0;
index.for_each_key_with_prefix("rust", |key| widest = widest.max(key.len()));
assert_eq!(widest, 6);
```

The borrow is valid only for the duration of the call — the next edge overwrites
the buffer — so copy out what you need inside the closure.

## The complete handler

```rust
use verbora_trie::Trie;

fn suggest(index: &Trie, prefix: &str, limit: usize) -> Vec<String> {
    if prefix.is_empty() {
        return Vec::new();          // an empty prefix matches everything
    }

    index.iter_keys_with_prefix(prefix).take(limit).collect()
}

let mut index = Trie::new();
index.insert_all(["rust", "rustic", "rusty", "rustle", "ruse", "rush"]);
assert_eq!(suggest(&index, "rus", 2), ["ruse", "rush"]);
assert!(suggest(&index, "", 10).is_empty());
```

## Case handling is declared once, then uniform

<div class="callout callout-note">
<strong>Case handling is fixed when the trie is constructed, and then applies to
every argument of every method.</strong> A case-insensitive trie folds what you
store, what you ask <code>contains</code>, and the prefix or search string handed
to any query — with no exceptions. A method that folded some of its arguments and
not others would make results depend on which entry point you happened to use.
</div>

```rust
use verbora_trie::Trie;

let mut index = Trie::case_insensitive();
index.insert_all(["Rust", "Rusty"]);

assert!(index.contains("RUST"));
assert_eq!(index.keys_with_prefix("RUST"), ["rust", "rusty"]);
assert_eq!(index.keys_with_prefix("rust"), ["rust", "rusty"]);
```

The one thing to plan for is that a case-insensitive trie stores the *folded*
keys, so what comes back is lowercase. If you need the original casing, keep a
side map from folded key to display form. Folding is
[`str::to_lowercase`](https://doc.rust-lang.org/std/primitive.str.html#method.to_lowercase)'s
full Unicode mapping, so it is a declared transformation of your input, not an
ASCII shortcut: `Trie::case_handling()` reports which mode an index is in.

## Result ordering

Keys come back in **ascending order of their scalar sequence**, which for
well-formed Rust strings is exactly `<str as Ord>` — not insertion order.

```rust
use verbora_trie::Trie;

let mut t = Trie::new();
t.insert_all(["b1", "a1", "9x", "1x", "0x", "zz"]);

// `keys()` is itself lazy — collect to inspect the order.
assert_eq!(
    t.keys().collect::<Vec<_>>(),
    ["0x", "1x", "9x", "a1", "b1", "zz"],
);
```

A node's own word is emitted before its subtree, which is not a second rule but a
consequence of the first: a node's word is a proper prefix of every word beneath
it, and a proper prefix sorts first. The order is specified, so you can rely on
it — but it is a *lexical* order, not a relevance one.

**If you want relevance ordering, sort after taking.** Take more than you need,
score them, then truncate:

```rust
use verbora_distance::jaro_winkler;
use verbora_trie::Trie;

fn ranked(index: &Trie, prefix: &str, limit: usize) -> Vec<String> {
    let mut candidates: Vec<String> = index
        .iter_keys_with_prefix(prefix)
        .take(limit * 10)                       // a bounded superset
        .collect();

    // Descending: Jaro–Winkler is a similarity, so higher is closer. The score
    // is always finite, so `total_cmp` gives a total order with no NaN case.
    candidates.sort_by(|a, b| {
        jaro_winkler(prefix, b).total_cmp(&jaro_winkler(prefix, a))
    });
    candidates.truncate(limit);
    candidates
}

let mut index = Trie::new();
index.insert_all(["rust", "rustic", "rusty", "rustle", "ruse", "rush"]);
assert_eq!(ranked(&index, "rust", 2), ["rust", "rusty"]);
```

`take(limit * 10)` keeps the cost bounded: you never reconstruct the whole
subtree, and you still have enough candidates for the ranking to mean something.

## Related trie queries

```rust
use verbora_trie::Trie;

let mut t = Trie::new();
t.insert_all(["rust", "rusty"]);

// Every stored key that is a prefix of the search string, shortest first —
// dictionary matching.
assert_eq!(t.prefix_matches("rusty"), ["rust", "rusty"]);

// Split a string into (longest stored prefix, unconsumed remainder).
let split = t.longest_prefix("rustyx");
assert_eq!(split.word.as_deref(), Some("rusty"));
assert_eq!(split.rest, "x");

// The same split counted in Unicode scalars, allocating nothing at all.
let lengths = t.longest_prefix_lengths("rustyx");
assert_eq!(lengths.word, Some(5));
assert_eq!(lengths.rest, 1);
```

Two details in `longest_prefix` are easy to get wrong. The remainder is measured
from where the **walk** died, not from where `word` ended: with only
`["their", "and"]` stored, searching `"theyre"` walks as far as `"the"` and
leaves `"yre"`, with `word` still `None`. And `Some("")` — the trie stores the
empty string — is a different answer from `None`, so a
`if let Some(w) = … if !w.is_empty()` guard conflates two distinct results.

`longest_prefix` cuts both fields out of your own search string, so on a
case-sensitive trie the whole operation borrows and allocates nothing;
`longest_prefix_lengths` allocates nothing on any trie, and is the one to reach
for when you only need offsets.

## Freezing a finished index

An autocomplete index is built once and queried forever, which is exactly the
shape `Trie::freeze` is for. It compresses runs of single-child, non-word nodes
into one edge label and precomputes the enumeration-order key table:

```rust
use verbora_trie::Trie;

let mut t = Trie::new();
t.insert_all(["cat", "cats", "car", "care", "careful"]);

let frozen = t.freeze();
assert!(frozen.contains("cats"));
assert_eq!(frozen.keys_with_prefix("car"), t.keys_with_prefix("car"));

// Frozen keys are already contiguous, so a prefix query can hand back a slice.
assert_eq!(frozen.keys_slice("care"), ["care", "careful"]);
```

The compression is exact, not approximate: every position a query could land on —
a `contains` that returns `true`, a word `keys_with_prefix` emits, a prefix
argument ending exactly there — survives as a real node, and only the
unobservable pass-through nodes disappear. Ordering is unchanged.

`FrozenTrie` implements `contains` and the key enumerations, and nothing else.
`prefix_matches` and `longest_prefix` have no frozen counterpart — call them on
the original `Trie`, which you keep if you need them. Call `freeze` once after a
bulk load, never per query.

## Sizing

Two different counts, both O(1):

```rust
use verbora_trie::Trie;

let mut t = Trie::new();
t.insert("a👍");

assert_eq!(t.len(), 1);         // stored words
assert_eq!(t.node_count(), 3);  // root + 'a' + '👍'
```

`len()` is the number of distinct stored words — a maintained subtree count read
off the root. `node_count()` is the size of the arena, and **one Unicode scalar
is one node**, whatever plane it lives in: `'👍'` costs one level, not two.
Neither is a traversal, so both are safe to call in a metrics loop.

Inserting the empty string marks the root as a word: it changes `len()` and
leaves `node_count()` alone. A trie storing only the empty string is therefore
not `is_empty()`.

## What a trie cannot do

- **No removal.** There is no `remove`, `delete` or `clear`. Rebuild from
  `keys()` if you need to drop entries.
- **No fuzzy matching.** A trie answers prefix questions. For sound-alike or
  typo-tolerant lookup, see [Fuzzy name matching](fuzzy-matching.md).
- **No weights or scores.** It stores membership. Rank afterwards, as above.
- **No concurrent insertion.** `insert` takes `&mut self`.
- **No grapheme awareness.** The unit is the Unicode scalar, so `"e\u{301}"` and
  `"é"` are two different keys, exactly as they are for `str` equality. Normalise
  first if that is not what you want — see
  [Normalizers](../features/normalizers.md).

## Related

- [Trie](../features/trie.md) — the full API and the arena design.
- [Fuzzy name matching](fuzzy-matching.md)
- [Iterator vs reusable buffer](../performance/iterator-vs-into.md)
