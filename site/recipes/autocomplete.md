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
    trie.add_strings(words.iter().copied());
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
index.add_strings(["rust", "rustic", "rusty", "rustle", "ruse", "rush"]);

// Materialises every match, then you throw most away.
let all: Vec<String> = index.keys_with_prefix("rust");
assert_eq!(all, ["rust", "rustic", "rusty", "rustle"]);

// Walks only as far as it needs to.
let top: Vec<String> = index.iter_keys_with_prefix("rust").take(3).collect();
assert_eq!(top, ["rust", "rustic", "rusty"]);
```

With four matches the difference is nothing. With a prefix like `"a"` over a
300,000-word dictionary, `keys_with_prefix` reconstructs tens of thousands of
`String`s to show you ten.

<a class="badge badge-lazy" href="../performance/iterator-vs-into">LAZY</a>
<a class="badge badge-owned" href="../performance/allocation">OWNED</a>

Either way the output is a `String` per key — trie keys are reconstructed by
walking, so there is nothing to borrow. The lazy form allocates one only for
each key you actually take, which is what makes it right for a top-*k*
suggestion list.

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
index.add_strings(["rust", "rustic", "rusty", "rustle", "ruse", "rush"]);
assert_eq!(suggest(&index, "rus", 2), ["rust", "rustic"]);
assert!(suggest(&index, "", 10).is_empty());
```

## The case-sensitivity trap

<div class="callout callout-warn">
<strong><code>keys_with_prefix</code> never folds case — even on a
case-insensitive trie.</strong> Every <em>other</em> method does. This is
specified behaviour, not an oversight, and it is pinned by the test suite: fold
the prefix yourself before you pass it in.
</div>

```rust
use verbora_trie::Trie;

let mut index = Trie::case_insensitive();
index.add_strings(["Rust", "Rusty"]);

// contains() folds, as you would expect.
assert!(index.contains("RUST"));

// keys_with_prefix() does not. Fold the prefix yourself.
assert!(index.keys_with_prefix("RUST").is_empty());
assert_eq!(index.keys_with_prefix("rust"), ["rust", "rusty"]);
```

Note that a case-insensitive trie stores the *folded* keys, so what comes back is
lowercase. If you need the original casing, keep a side map from folded key to
display form.

## Result ordering

Keys come back in `Trie`'s own child order, which is **not** insertion
order: keys that look like array indices come first in ascending numeric order,
then everything else in insertion order.

```rust
use verbora_trie::Trie;

let mut t = Trie::new();
t.add_strings(["b1", "a1", "9x", "1x", "0x", "zz"]);

// `keys()` is itself lazy — collect to inspect the order.
assert_eq!(
    t.keys().collect::<Vec<_>>(),
    ["0x", "1x", "9x", "b1", "a1", "zz"],
);
```

For single code units the "array-index-like" keys are exactly `'0'`–`'9'`. Treat
the order as an implementation detail: if you need a specific one, sort after
collecting.

**If you want relevance ordering, sort after taking.** Take more than you need,
score them, then truncate:

```rust
use verbora_distance::{jaro_winkler, jaro_winkler::Options};
use verbora_trie::Trie;

fn ranked(index: &Trie, prefix: &str, limit: usize) -> Vec<String> {
    let opts = Options::default();

    let mut candidates: Vec<String> = index
        .iter_keys_with_prefix(prefix)
        .take(limit * 10)                       // a bounded superset
        .collect();

    candidates.sort_by(|a, b| {
        jaro_winkler(prefix, b, &opts).total_cmp(&jaro_winkler(prefix, a, &opts))
    });
    candidates.truncate(limit);
    candidates
}

let mut index = Trie::new();
index.add_strings(["rust", "rustic", "rusty", "rustle", "ruse", "rush"]);
assert_eq!(ranked(&index, "rust", 2), ["rust", "rusty"]);
```

`take(limit * 10)` keeps the cost bounded: you never reconstruct the whole
subtree, and you still have enough candidates for the ranking to mean something.

## Related trie queries

```rust
use verbora_trie::Trie;

let mut t = Trie::new();
t.add_strings(["rust", "rusty"]);

// Every stored key that is a prefix of the search string — dictionary matching.
assert_eq!(t.find_matches_on_path("rusty"), ["rust", "rusty"]);

// Split a string into (longest stored prefix, remainder).
assert_eq!(t.find_prefix("rustyx"), (Some("rusty".into()), "x".into()));

// The same split, in UTF-16 code units, with no allocation and no
// surrogate-splitting loss.
assert_eq!(t.find_prefix_lengths("rustyx"), (Some(5), 1));
```

`find_prefix_lengths` is the one to use when precision matters: `find_prefix`'s
remainder can begin inside a surrogate pair, which a Rust `String` cannot
represent, so that one position renders as `U+FFFD`.

## Sizing

`get_size()` counts **nodes**, not words, and it counts by UTF-16 code unit — so
an astral character occupies two levels:

```rust
use verbora_trie::Trie;

let mut t = Trie::new();
t.add_string("a👍");

assert_eq!(t.get_size(), 4);   // root + 'a' + two surrogate halves
```

It is O(1) — a maintained counter, not a traversal — so it is safe to call in a
metrics loop.

## What a trie cannot do

- **No removal.** There is no `remove`, `delete` or `clear`. Rebuild from
  `keys()` if you need to drop entries.
- **No fuzzy matching.** A trie answers prefix questions. For sound-alike or
  typo-tolerant lookup, see [Fuzzy name matching](fuzzy-matching.md).
- **No weights or scores.** It stores membership. Rank afterwards, as above.
- **No concurrent insertion.** `add_string` takes `&mut self`.

## Related

- [Trie](../features/trie.md) — the full API and the arena design.
- [Fuzzy name matching](fuzzy-matching.md)
- [Iterator vs reusable buffer](../performance/iterator-vs-into.md)
