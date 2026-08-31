# Trie

`verbora-trie` is an arena-backed prefix tree over `&str` keys. It answers four
questions about a set of strings: *is this exact string stored?*, *which stored
strings start with this prefix?*, *which stored strings are prefixes of this
string?*, and *where does the longest stored prefix of this string end?*

The crate is two types — `Trie`, the one you build with, and `FrozenTrie`, the
read-only representation [`Trie::freeze`](#freezing-for-query-heavy-workloads)
precomputes — plus the iterators they hand out.

Four lines fix the whole contract:

- **The unit is one Unicode scalar value.** A key is a `&str`, and the smallest
  piece of one that is itself text is a scalar, so that is what labels an edge.
  Every node is a position a caller can name, and every string the crate hands
  back is text the caller supplied.
- **Enumeration is ascending by scalar sequence**, which for well-formed Rust
  strings is exactly `<str as Ord>`.
- **Case handling is fixed at construction and applies to every argument of
  every method**, with no exceptions.
- **Nothing panics** for any input, short of capacity overflow on an arena
  larger than `u32::MAX` nodes.

<div class="callout callout-spec">
<strong>Specification status.</strong> The text unit, the enumeration order,
case handling, insertion and every query are documented and test-pinned,
interleaved mutation/query sequences included; the contract suite additionally
walks the whole shared 20,000-word benchmark list rather than a sample, checking
every entry for membership and every prefix of every entry against a sorted
reference. <code>cargo test -p verbora-trie</code> runs <strong>80</strong>
tests (67 unit, 13 contract) and <strong>18</strong> doctests.
</div>

## When to use it

- **Autocomplete and typeahead.** `iter_keys_with_prefix` streams completions in
  sorted order and stops when you stop, and *counting* the words under a prefix
  is O(prefix length) — no traversal at all.
- **Longest-match tokenization and dictionary segmentation.** `longest_prefix`
  and `longest_prefix_lengths` give the split point of the longest stored word
  that prefixes the input, in one linear walk.
- **Membership over a large, static string set** where the strings share
  prefixes. Node sharing means a dictionary of inflected forms costs far less
  than one entry per word, and [`freeze`](#freezing-for-query-heavy-workloads)
  turns a finished set into a structure whose enumerations are range copies.
- **Deterministic, reproducible enumeration order.** The order is a total order
  defined by the contract, not by whatever the structure happens to produce, so
  results are exactly reproducible across runs — which matters for golden-file
  tests and snapshot diffs.

## When not to use it

- **You only need set membership.** A `HashSet<String>` is simpler when you
  never query by prefix.
- **You need to remove entries.** There is **no `remove`, no `delete`, and no
  `clear`.** See [Removing words](#removing-words) for the rebuild pattern.
- **You need fuzzy matching.** A trie is exact-prefix only. For edit distance
  and phonetic similarity, see [Distance](./distance.md).
- **Your keys are not prefix-structured** (UUIDs, hashes, random identifiers).
  Every node then has one child and the trie degenerates into a linked list with
  worse locality than a hash table.
- **The set changes constantly and must shrink.** Rebuilding is the only way to
  drop a word, which is O(total input) each time.

## Quick example

```rust
use verbora_trie::Trie;

fn main() {
    let mut trie = Trie::new();
    trie.insert_all(["and", "their", "they", "them"]);

    assert!(trie.contains("they"));
    assert!(!trie.contains("the")); // a prefix is not a word

    // Enumeration is ascending scalar order, which is `<str as Ord>`.
    assert_eq!(trie.keys_with_prefix("the"), ["their", "them", "they"]);

    // The stored words that prefix a search string, shortest first…
    assert_eq!(trie.prefix_matches("theyre"), ["they"]);

    // …and the longest of them, as scalar counts, allocating nothing.
    let split = trie.longest_prefix_lengths("theyre");
    assert_eq!((split.word, split.rest), (Some(4), 2));
}
```

## Construction

| Constructor | Case handling |
|---|---|
| `Trie::new()` | `Sensitive` |
| `Trie::default()` | `Sensitive` |
| `Trie::with_case_handling(CaseHandling::Sensitive)` | `Sensitive` |
| `Trie::with_case_handling(CaseHandling::Folded)` | `Folded` |
| `Trie::case_insensitive()` | `Folded` |
| `["a", "ab"].into_iter().collect::<Trie>()` | `Sensitive` |

**The default is case-sensitive.** `case_handling()` reports which mode a trie
is in, and it is the whole story: `Sensitive` compares scalar for scalar and
never rewrites its input, `Folded` lowercases what it stores *and* every
argument it is given. `FromIterator` and `Extend` are implemented for any
`IntoIterator` whose items are `AsRef<str>`; `collect` builds a
**case-sensitive** trie, so for the folding variant construct with
`Trie::case_insensitive()` and use `insert_all`. `Extend` adds to whatever trie
you already have and keeps its case handling.

```rust
use verbora_trie::{CaseHandling, Trie};

fn main() {
    assert_eq!(Trie::new().case_handling(), CaseHandling::Sensitive);
    assert_eq!(Trie::case_insensitive().case_handling(), CaseHandling::Folded);

    let mut t = Trie::new();
    assert_eq!(t.node_count(), 1); // the root always exists
    assert_eq!(t.len(), 0);        // …but no word is stored in it

    t.insert("hi");
    assert_eq!((t.len(), t.node_count()), (1, 3));
}
```

`len()` counts **words**, `node_count()` counts **nodes**. Both are O(1): `len`
reads the root's maintained subtree word count, and the arena's length *is* the
node count.

### `reserve`

`reserve(additional)` reserves capacity for `additional` more **nodes**, not
words. A trie needs roughly one node per distinct prefix, counted in Unicode
scalars; the total scalar length of the input is a safe upper bound.

```rust
use verbora_trie::Trie;

fn bulk_load(words: &[String]) -> Trie {
    let mut trie = Trie::new();
    let upper_bound: usize = words.iter().map(|w| w.chars().count()).sum();
    trie.reserve(upper_bound + 1);
    trie.insert_all(words.iter().map(String::as_str));
    trie
}

fn main() {
    let words = vec![String::from("alpha"), String::from("beta")];
    assert_eq!(bulk_load(&words).keys_with_prefix(""), ["alpha", "beta"]);
}
```

Reserving up front removes the arena's growth reallocations from a bulk load,
and for a large load it is the only thing that does. `insert_all` reserves the
iterator's `size_hint().0` — one node per item, a lower bound — but **clamps it
at 4,096 nodes**, so it skips the first few doublings and no more. A `Vec` of a
million words reports a million and is still pre-sized for 4,096. The clamp is
deliberate: `size_hint` is a hint, not a bound, and an iterator that overstates
it must not be able to turn a bulk load into an unbounded allocation. Growth
past the ceiling is amortised, which is a constant factor; trusting the hint is
not bounded at all. When you know the real size, say so with `reserve` — as
above — rather than relying on the hint.

## Insertion

`insert` follows `HashSet::insert`'s convention: **`true` means this call added
the word.**

```rust
use verbora_trie::Trie;

fn main() {
    let mut trie = Trie::new();
    assert!(trie.insert("test"));  // added
    assert!(!trie.insert("test")); // already stored

    // The empty string is a word that creates no node.
    assert!(trie.insert(""));
    assert!(trie.contains(""));
    assert_eq!(trie.len(), 2);        // "test" and ""
    assert_eq!(trie.node_count(), 5); // root + t + e + s + t
}
```

Inserting the empty string marks the root as a word. It creates no node, so
`node_count()` does not change — but `len()` does, `contains("")` becomes
`true`, `""` becomes the first result of `keys_with_prefix("")` and of every
`prefix_matches`, and `longest_prefix` starts returning `Some("")` instead of
`None` for total misses.

`insert_all<I>(list)` takes any `IntoIterator` whose items are `AsRef<str>`. It
reserves `size_hint().0` nodes — capped at 4,096, see above — pre-slots the
membership table to match, and then calls `insert` per item, so the return
values are discarded. There is no batch or
parallel insertion API: `insert` needs `&mut self` and appends to one shared
arena, so building a trie is inherently single-threaded — see
[Sharing a trie across threads](#sharing-a-trie-across-threads) for what *can*
be parallelised.

Each insertion also maintains two query accelerators: the per-node subtree word
counts behind `len()` and the O(1) prefix count, and the hash membership set
behind `contains`. Both cost build time and are disclosed here rather than
hidden — they are why a prefix count needs no traversal and why `contains` is
one hash rather than one dependent cache miss per scalar.

## Choosing the right API

### Comparison table

"Allocations" assumes a case-sensitive trie, or a folding trie whose argument
is already lowercase — the folding step is covered
[below](#allocation-behaviour).

| API | Answers | Lazy | Output | Allocations |
|---|---|:--:|---|---|
| `contains(s)` | is `s` a stored word? | n/a | `bool` | none |
| `len()` | how many words? | n/a | `usize` | none — O(1) |
| `node_count()` | how many nodes? | n/a | `usize` | none — O(1) |
| `keys_with_prefix(p)` | all words under `p` | ❌ | `Vec<String>` | one exactly-sized `Vec` + one `String` per word |
| `iter_keys_with_prefix(p)` | all words under `p` | ✅ | `KeysWithPrefix` → `String` | one path buffer + one stack; one `String` per word yielded |
| `for_each_key_with_prefix(p, f)` | all words under `p` | n/a | `&str` per call | one path buffer, nothing per word |
| `keys()` | all words | ✅ | `KeysWithPrefix` → `String` | as `iter_keys_with_prefix` |
| `prefix_matches(s)` | stored words that prefix `s` | ❌ | `Vec<Cow<'a, str>>` | one `Vec`; items borrow `s` |
| `iter_prefix_matches(s)` | stored words that prefix `s` | ✅ | `PrefixMatches` → `Cow<'a, str>` | none |
| `longest_prefix(s)` | longest stored prefix + remainder | n/a | `PrefixSplit<'a>` | none |
| `longest_prefix_lengths(s)` | the same split, in scalars | n/a | `PrefixSplitLengths` | none, ever |

Two rows deserve a second look:

- **Counting is not enumerating.** `iter_keys_with_prefix(p).count()` reads a
  maintained subtree word count after descending `p`, so it is O(len(p)) with no
  traversal and no allocation. `keys_with_prefix(p).len()` builds the whole
  subtree to answer the same question.
- **`longest_prefix_lengths` never allocates**, not even on a folding trie given
  upper-case input, because it returns counts rather than slices.

### Which one

| Your question | Use |
|---|---|
| Is this exact string stored? | `contains()` |
| How many words are stored? | `len()` — O(1) |
| How big is the structure? | `node_count()` — nodes, not words, O(1) |
| Which stored words start with mine, and I need to keep them all? | `keys_with_prefix()` → `Vec<String>` |
| …only the first N, or I stop on a condition? | `iter_keys_with_prefix().take(N)` |
| …how many, without the words themselves? | `iter_keys_with_prefix().count()` — O(prefix) |
| …only "does anything start with this?" | `iter_keys_with_prefix().next().is_some()` |
| …and each word is consumed then dropped? | `for_each_key_with_prefix()` — no `String` per word |
| Every word in the trie | `keys()` — lazy, same as `iter_keys_with_prefix("")` |
| Which stored words are prefixes of mine, all of them, shortest first? | `prefix_matches()` → `Vec<Cow<str>>` |
| …only the shortest, or the first few? | `iter_prefix_matches().next()` / `.take(n)` |
| …only the longest? | `longest_prefix().word` — one walk, no iterator |
| Where does the longest stored prefix end, as text? | `longest_prefix()` → `PrefixSplit` |
| …as offsets, exactly, with no allocation? | `longest_prefix_lengths()` → `PrefixSplitLengths` |
| The same trie, enumerated over and over? | [`freeze()`](#freezing-for-query-heavy-workloads), then `keys_slice()` |

### `keys_with_prefix` <a class="badge badge-owned" href="../performance/allocation">OWNED</a>

Eager: descend the prefix, walk the subtree, hand back a `Vec<String>`. It is
not the lazy iterator collected — the straight-line walk keeps its cursor in
registers instead of suspending and resuming per word, and the subtree word
count sizes the vector exactly, so the only allocations left are the one
`String` per returned word that an owned result requires. Reach for it when the
result is small and you want to hold on to it.

### `iter_keys_with_prefix` and `keys` <a class="badge badge-lazy" href="../performance/iterator-vs-into">LAZY</a>

Lazy and depth-first, one word per `next()`. The working set is one reusable
path `String` and one frame `Vec`, both O(depth), plus one `String` per word
actually yielded. `keys()` is exactly `iter_keys_with_prefix("")`, and `&trie`
implements `IntoIterator` with the same behaviour, so `for word in &trie` works.
Both are `FusedIterator`.

`size_hint` and `count` are **exact**, not estimates: the trie is borrowed for
the iterator's whole lifetime, so the subtree cannot change underneath it and
the maintained word count is right at every point. That is what makes
`collect::<Vec<_>>()` allocate its result exactly once and `count()` free.

### `for_each_key_with_prefix` <a class="badge badge-reuse" href="../performance/allocation">NO PER-WORD ALLOCATION</a>

Same words, same order, no `String` per word — one reused path buffer for the
whole traversal and nothing else. A stored word exists nowhere contiguously in
the arena — it is spelled out one scalar per node — so there is no `&str` inside
the trie to borrow, and an iterator of borrows is impossible without
materialising something first. Lending the shared path buffer to a callback is
the one shape that avoids a per-word allocation entirely: the argument is valid
for the duration of the call, which is exactly how long the buffer holds that
word.

```rust
use verbora_trie::Trie;

fn suggest(trie: &Trie, prefix: &str, limit: usize) -> Vec<String> {
    trie.iter_keys_with_prefix(prefix).take(limit).collect()
}

fn main() {
    let mut trie = Trie::new();
    trie.insert_all((0..5_000).map(|i| format!("search{i:04}")));

    // Materialising: walks all 5,000 words and allocates one String each.
    assert_eq!(trie.keys_with_prefix("search").len(), 5_000);

    // Streaming: stops after 10 words.
    let page = suggest(&trie, "search", 10);
    assert_eq!((page.len(), page[0].as_str()), (10, "search0000"));

    // Counting: a descent and one field read, no traversal.
    assert_eq!(trie.iter_keys_with_prefix("search1").count(), 1_000);

    // Consuming without keeping: no String per word at all.
    let mut bytes = 0;
    trie.for_each_key_with_prefix("search1", |word| bytes += word.len());
    assert_eq!(bytes, 1_000 * "search1000".len());

    // "Is there anything under this prefix?" needs exactly one word.
    assert!(trie.iter_keys_with_prefix("search1").next().is_some());
    assert!(trie.iter_keys_with_prefix("zzz").next().is_none());
}
```

### `prefix_matches` and `iter_prefix_matches`

`prefix_matches` is eager — one linear walk of the search string, one `Vec`.
`iter_prefix_matches` advances that same walk one scalar per `next()` and
allocates nothing on a case-sensitive trie. Results are **cut from the search
string** (after folding), not rebuilt from the stored keys, which is why they
can borrow. The number of matches is bounded by the length of the search string,
so the eager `Vec` is small by construction — the lazy variant matters less here
than for `keys_with_prefix`.

```rust
use std::borrow::Cow;
use verbora_trie::Trie;

fn main() {
    let mut trie = Trie::new();
    trie.insert_all(["a", "ab", "bc", "cd", "abc"]);

    // All of them, shortest first.
    let all: Vec<Cow<'_, str>> = trie.prefix_matches("abcd");
    assert_eq!(all, ["a", "ab", "abc"]);

    // Shortest only: one step of the walk.
    assert_eq!(trie.iter_prefix_matches("abcd").next().as_deref(), Some("a"));

    // Longest only: longest_prefix answers it without an iterator at all.
    let split = trie.longest_prefix("abcd");
    assert_eq!((split.word.as_deref(), split.rest.as_ref()), (Some("abc"), "d"));
}
```

<div class="callout callout-note">
<strong>Note.</strong> Do not reach for <code>.last()</code> on
<code>iter_prefix_matches</code> to get the longest match. It works, but it
walks the whole string and yields every shorter match on the way.
<code>longest_prefix(s).word</code> is the same answer from the same single
walk, and <code>longest_prefix_lengths(s).word</code> is that answer without any
allocation.
</div>

Like `KeysWithPrefix`, `PrefixMatches` is a `FusedIterator`.

### `longest_prefix` <a class="badge badge-cow" href="../performance/zero-copy">COW</a>

Returns a `PrefixSplit`: the longest stored prefix in `word`, if any, paired
with the unconsumed remainder of the search string in `rest`. One linear walk;
both fields are cut from the search string, so on a case-sensitive trie they
borrow it and nothing is allocated.

```rust
use std::borrow::Cow;
use verbora_trie::Trie;

fn main() {
    let mut trie = Trie::new();
    trie.insert_all(["their", "and", "they"]);

    let split = trie.longest_prefix("theyre");
    assert_eq!((split.word.as_deref(), split.rest.as_ref()), (Some("they"), "re"));
    // Borrowed on a case-sensitive trie: no allocation.
    assert!(matches!(split.word, Some(Cow::Borrowed(_))));
    assert!(matches!(split.rest, Cow::Borrowed(_)));

    // The remainder is where the WALK died, not where the word ended.
    let mut partial = Trie::new();
    partial.insert_all(["their", "and"]);
    let split = partial.longest_prefix("theyre");
    assert_eq!((split.word, split.rest.as_ref()), (None, "yre")); // the walk got as far as "the"
}
```

Two details are easy to get wrong:

1. **The remainder is what was left when the walk died**, not what was left
   after the last word ended. The two coincide only when the walk stops exactly
   at the end of a stored word.
2. **`Some("")` and `None` are different answers.** A trie containing the empty
   string returns `word: Some("")` for a total miss, so an
   `if let Some(w) = … if !w.is_empty()` guard silently treats a real match as a
   miss.

### `longest_prefix_lengths` <a class="badge badge-alloc" href="../performance/allocation">ALLOCATION-FREE</a>

The same single walk with the string-building removed, returning a
`PrefixSplitLengths` of `Option<usize>` and `usize`. **Prefer it whenever you do
not need the two halves as strings** — it is the one query that allocates
nothing even on a folding trie handed upper-case input, because counts have no
folded copy to hold.

```rust
use verbora_trie::Trie;

fn main() {
    let mut trie = Trie::new();
    trie.insert_all(["their", "and", "they"]);

    let lengths = trie.longest_prefix_lengths("theyre");
    assert_eq!((lengths.word, lengths.rest), (Some(4), 2));
}
```

<div class="callout callout-warn">
<strong>Careful.</strong> The lengths are <strong>Unicode scalars</strong>, not
bytes. They index a Rust <code>&amp;str</code> only after you convert — for
example with <code>char_indices</code>. For pure ASCII the two coincide, which is
exactly what makes this easy to get wrong later.
</div>

## Freezing for query-heavy workloads

`Trie::freeze()` pays one linear pass to build a `FrozenTrie`: a read-only,
path-compressed tree with a precomputed key table and membership set. Freeze
when the same set will be queried many times and never changed again; do not
freeze for a handful of lookups, because the freeze itself costs more than they
do.

Compression keeps a node when it is the root, is itself a stored word, or has
zero or more than one child. Every other node has exactly one child and marks no
word, so no query can stop there, and it is folded into the edge label leading
to the next kept node. That is exact rather than approximate: every position a
caller could land on survives as a real node, and only the unobservable
pass-through nodes disappear. Nothing is reordered, so the frozen tree
enumerates in the same ascending scalar order.

```rust
use verbora_trie::Trie;

fn main() {
    let mut trie = Trie::new();
    trie.insert_all(["cat", "cats", "car", "care", "careful"]);
    let frozen = trie.freeze();

    assert!(frozen.contains("cats"));
    assert!(!frozen.contains("ca")); // still a prefix, still not a word
    assert_eq!(frozen.keys_with_prefix("car"), trie.keys_with_prefix("car"));

    // Borrowed straight out of the precomputed key table: no per-word copy.
    let ca: &[String] = frozen.keys_slice("ca");
    assert_eq!(ca, ["car", "care", "careful", "cat", "cats"]);
    assert!(frozen.keys_slice("dog").is_empty());

    // Compression removes only the unobservable pass-through nodes.
    assert_eq!(trie.node_count(), 10);
    assert_eq!(frozen.node_count(), 7);
}
```

| Operation | `Trie` | `FrozenTrie` |
|---|---|---|
| Insert | `insert`, `insert_all` | — build a `Trie` and freeze it again |
| `contains` | hash membership set | hash membership set |
| `keys_with_prefix` | descend + subtree walk | descend + range copy of the key table |
| `keys_slice` | — a word exists nowhere contiguously | `&[String]`, no allocation at all |
| `iter_keys_with_prefix` | depth-first traversal | cursor over a slice; `count`, `nth`, `size_hint` all O(1) |
| `prefix_matches`, `longest_prefix` | ✅ | — call them on the original `Trie` |
| `node_count` | one node per scalar | one node per kept position |

`FrozenTrie::node_count` is deliberately not the same number as
`Trie::node_count`: a long branch-free chain is many original nodes and exactly
one frozen node beyond the root. `len()` — the number of stored words — is the
same on both.

`prefix_matches` and `longest_prefix` have no frozen counterpart. Freezing
covers membership and enumeration; those two are top-down single-path walks
whose compressed form would need byte-offset tracking across a multi-scalar
edge label, and the scope stops short of that deliberately. Call them on the
original `Trie`.

## Advanced usage

### Sharing a trie across threads

A `Trie` is a plain owned value, and every query method takes `&self`. Build
once, wrap in an `Arc`, then fan out. `FrozenTrie` shares the same way, and is
the better thing to share when the queries are enumerations.

```rust
use std::sync::Arc;
use verbora_trie::{FrozenTrie, Trie};

fn main() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Trie>();
    assert_send_sync::<FrozenTrie>();

    let mut trie = Trie::new();
    trie.insert_all(["alpha", "beta", "gamma"]);
    let trie = Arc::new(trie);

    let handles: Vec<_> = ["alpha", "beta", "gamma"]
        .into_iter()
        .map(|word| {
            let trie = Arc::clone(&trie);
            std::thread::spawn(move || trie.contains(word))
        })
        .collect();

    for h in handles {
        assert!(h.join().unwrap());
    }
}
```

If the trie outlives the threads, `std::thread::scope` avoids the `Arc`
entirely.

<div class="callout callout-note">
<strong>Note.</strong> <code>verbora-trie</code> ships <strong>no
<code>par_*</code> API</strong> and has no <code>parallel</code> Cargo feature.
Construction cannot be parallelised at all — <code>insert</code> takes
<code>&amp;mut self</code> and appends to one shared arena — and a single query
is far too small a unit of work to hand to a thread pool. Parallelising
<em>queries</em> yourself, as above, is the supported route. See
<a href="../performance/parallelism">Parallelism</a>.
</div>

### Removing words

There is no `remove`, no `delete` and no `clear`. The pattern is to rebuild from
`keys()`, which is lazy, so the old trie is streamed rather than materialised.

```rust
use verbora_trie::Trie;

fn rebuild_without(trie: &Trie, drop: &str) -> Trie {
    let mut rebuilt = Trie::with_case_handling(trie.case_handling());
    // The rebuilt trie can never need more nodes than the original had.
    rebuilt.reserve(trie.node_count());
    rebuilt.insert_all(trie.keys().filter(|w| w != drop));
    rebuilt
}

fn main() {
    let mut trie = Trie::new();
    trie.insert_all(["alpha", "beta", "gamma"]);

    let smaller = rebuild_without(&trie, "beta");
    assert_eq!(smaller.keys_with_prefix(""), ["alpha", "gamma"]);
    assert_eq!(smaller.len(), 2);
    assert!(smaller.node_count() < trie.node_count());
}
```

This is O(total stored text) and allocates a `String` per surviving word, so it
is a maintenance operation, not something to do per request. If your workload
needs frequent deletion, keep an auxiliary `HashSet` of tombstones and filter
results, or rebuild on a schedule.

## Three behaviours worth knowing

### One scalar, one node

The label on an edge is exactly one Unicode scalar value. Keying nodes by
anything smaller — UTF-8 bytes, UTF-16 code units — would put positions in the
tree that no `&str` can name and let a walk stop somewhere that is not a
character boundary. With the scalar unit, `'😀'` is one node rather than two,
and every remainder `longest_prefix` returns is a suffix of the caller's own
text: no `U+FFFD`, no invented scalar, ever.

```rust
use verbora_trie::Trie;

fn main() {
    let mut trie = Trie::new();
    trie.insert("a👍");
    assert_eq!(trie.node_count(), 3); // root + 'a' + '👍'

    let mut bmp = Trie::new();
    bmp.insert("日本語");
    assert_eq!(bmp.node_count(), 4); // one node per scalar, whatever the plane

    // A search that shares 'a' and then diverges keeps its own text intact.
    let split = trie.longest_prefix("a👌");
    assert_eq!((split.word, split.rest.as_ref()), (None, "👌"));

    let lengths = trie.longest_prefix_lengths("a👌");
    assert_eq!((lengths.word, lengths.rest), (None, 1));
}
```

The unit is *not* the grapheme cluster. `"e\u{301}"` and `"é"` are two different
keys here, as they are for `str` equality itself; normalising them together is
your explicit choice — see [Normalizers](./normalizers.md).

### Enumeration order

Every enumeration yields words in **ascending order of their scalar sequence**.
For well-formed Rust strings that is byte-wise UTF-8 order, so it is exactly
`<str as Ord>` — the order `sort_unstable` on a `Vec<&str>` produces.

```rust
use verbora_trie::Trie;

fn main() {
    let mut trie = Trie::new();
    trie.insert_all(["b1", "a1", "9x", "1x", "0x", "zz"]);
    assert_eq!(trie.keys_with_prefix(""), ["0x", "1x", "9x", "a1", "b1", "zz"]);

    let mut mixed = Trie::new();
    mixed.insert_all(["cat", "0x", "car", "Ångström", "日本", "😀"]);

    let mut expected = ["cat", "0x", "car", "Ångström", "日本", "😀"];
    expected.sort_unstable();
    assert_eq!(mixed.keys_with_prefix(""), expected);
}
```

Insertion order never reaches the result: each node's child list is kept sorted
*on insertion*, so iteration is a straight scan with no sorting at read time. A
node's own word is emitted before its subtree, which is not an extra rule but a
consequence of the one above — a node's word is a proper prefix of every word
beneath it, and a proper prefix sorts first, which is why `"a"` precedes `"ab"`.

### Case handling has no exceptions

On a folding trie, **every** argument of **every** method is lowercased: what is
inserted, what `contains` is asked, and the prefix or search string handed to
any query. A method that folded some of its arguments and not others would make
results depend on which entry point a caller happened to use.

```rust
use verbora_trie::Trie;

fn main() {
    let mut trie = Trie::case_insensitive();
    trie.insert_all(["thEIr", "And", "theY"]);

    assert!(trie.contains("THEIR"));
    assert_eq!(trie.keys_with_prefix("TH"), ["their", "they"]);
    assert_eq!(trie.keys_with_prefix("th"), ["their", "they"]);
    assert_eq!(trie.prefix_matches("THEYRE"), ["they"]);
    assert_eq!(trie.longest_prefix("ThEyRe").word.as_deref(), Some("they"));

    // Stored words come back in their folded spelling, because that is what
    // was stored.
    assert_eq!(trie.keys().collect::<Vec<_>>(), ["and", "their", "they"]);
}
```

Folding is `str::to_lowercase` — the full, locale-independent Unicode
`Lowercase_Mapping`, `SpecialCasing.txt` included — with a byte-wise fast path
for ASCII that reaches the same answer. It is a *declared* transformation:
chosen at construction, reported by `case_handling()`, and applied to everything
alike.

## Performance characteristics

All nodes live in one flat `Vec<Node>` arena addressed by `u32`, so the whole
tree is one allocation rather than one per node. What follows from that, and
from the two accelerators each insertion maintains:

| Property | What follows |
|---|---|
| Flat arena, `u32` indices | No per-node allocation during a bulk load, and a descent touches consecutive cache lines instead of chasing pointers — see [Cache locality](../performance/cache-locality.md) |
| `len()` and `node_count()` are field reads | O(1) instead of a tree walk, so both are safe to call in a loop |
| Subtree word counts | A prefix *count* is a descent plus one field read, so `iter_keys_with_prefix(p).count()` never traverses, and `keys_with_prefix` sizes its `Vec` exactly |
| Hash membership set | `contains` is one hash of the folded bytes plus a short probe, rather than one dependent cache miss per scalar. A 256-bit first-byte gate sits in front of it, so a query whose first byte begins no stored word is rejected before anything is hashed |
| `SmallVec<[Child; 2]>` inline children | Nodes with one or two children — the overwhelming majority in a natural-language trie — keep their edges inside the node. `Node` is 32 bytes, exactly what a plain `Vec<Child>` would cost, so the inline capacity is free and the subtree count sits in bytes that were already padding |
| Sorted child lists | Lookup is a linear scan below eight children and a binary search above it. Both are correct at any size; the threshold is a speed knob, never a semantic one |

The two accelerators are a real trade, not free: they add work to every
insertion in exchange for the query behaviour above. The direction of the trade
is stated here rather than hidden, and its size on your corpus is a measurement.

### Complexity

With `m` = length of the argument in Unicode scalars and `k` = the number of
children of a node:

| Operation | Complexity |
|---|---|
| `insert` | O(m · log k) — a binary-searched child lookup per scalar, plus an ordered insert for new edges |
| `contains` | O(m) — one hash of the folded bytes plus a probe |
| `len`, `node_count` | O(1) |
| `longest_prefix`, `longest_prefix_lengths` | O(m · log k) |
| `prefix_matches` | O(m · log k); at most one result per scalar consumed, plus `""` if it was stored |
| `keys_with_prefix(p)` | O(len(p) · log k + size of the subtree + total length of the results) |
| `iter_keys_with_prefix(p).take(n)` | O(len(p) · log k + the part of the subtree needed for `n` words) |
| `iter_keys_with_prefix(p).count()` | O(len(p)) — no traversal |
| `freeze()` | O(nodes), once |
| `FrozenTrie::keys_slice(p)` | O(len(p)) — a descent and a range |

`k` is one or two for the overwhelming majority of nodes in natural-language
text, so the child-lookup factor behaves as a small constant.

### Measured

**Timings are unmeasured.** No benchmark has been run against the current
implementation of this crate, and no figure is estimated in place of one. The
Criterion suite in `crates/verbora-trie/benches/trie.rs` still compares the
arena against the closest faithful one-hash-map-per-node analogue — in both
`std`'s SipHash and `rustc-hash`'s FxHash — over the shared 20,000-word list,
and adds groups for `freeze`, the enumeration APIs, the frozen counterparts, and
the folding paths. The structural properties above are properties of the
implementation and are stated as such; no timing claim is made, and none should
be inferred. See [Benchmarks](../benchmarks/index.md).

## Allocation behaviour

**The trie itself.** One `Vec<Node>` arena, 32 bytes per node, grown by doubling
unless you `reserve`, plus the membership set's own buffers, plus one heap
allocation per node that acquires a third child. Node count equals the number of
distinct prefixes across all stored words, measured in Unicode scalars, plus one
for the root. The membership set defers every buffer to its first insertion, so
constructing a `Trie` that never stores anything costs nothing beyond the arena
holding the root.

**Queries** — assuming a case-sensitive trie, or a folding one whose argument is
already lowercase:

| Call | Allocates |
|---|---|
| `contains`, `len`, `node_count`, `longest_prefix_lengths`, `iter_prefix_matches` | nothing |
| `longest_prefix(s)` | nothing — both halves are slices of `s` |
| `prefix_matches(s)` | one `Vec`; the items borrow `s` |
| `for_each_key_with_prefix(p, f)` | one path `String`, reused for the whole traversal |
| `iter_keys_with_prefix(p)` | one path `String` and one frame `Vec` (both O(depth)), plus one `String` per word yielded |
| `keys_with_prefix(p)` | the above, plus one exactly-sized `Vec` |
| `FrozenTrie::keys_slice(p)` | nothing |
| `FrozenTrie::keys_with_prefix(p)` | one `Vec` and one `String` per word — the owned return shape itself |

**When folding does change the argument** — a case-insensitive trie given
upper-case input — one `String` copy is made up front and every `Cow` result
derived from it becomes owned. So `prefix_matches("THEYRE")` on a folding trie
allocates the folded copy plus one `String` per match, where the same call on a
case-sensitive trie allocates only the `Vec`. Fold your inputs once at your own
boundary if this is hot, or use `longest_prefix_lengths`, which never allocates
either way.

There is **no `_into` variant and no caller-supplied output buffer** anywhere in
this crate. The two ways to enumerate without a `String` per word are
`for_each_key_with_prefix`, which lends its internal path buffer rather than
filling one of yours, and `FrozenTrie::keys_slice`, which hands back a borrow of
a precomputed table and allocates nothing at all. See
[Allocation](../performance/allocation.md) and
[Iterator vs. `_into`](../performance/iterator-vs-into.md).

## Unicode and language notes

- **Keys are Unicode scalars.** See [One scalar, one node](#one-scalar-one-node).
  Every scalar is one node and one position in every length this crate reports,
  whatever plane it lives in.
- **Nothing invents a scalar.** Every string handed back is either text the
  caller supplied (`prefix_matches`, `longest_prefix`) or a word that was
  inserted (`keys`, `keys_with_prefix`). `U+FFFD` never appears in a result
  unless you put it there.
- **Folding is `str::to_lowercase`.** It handles every Unicode scalar —
  including multi-character expansions such as `'İ'` → `"i̇"` and the
  context-sensitive Greek final sigma — but applies neither Turkish nor
  Lithuanian locale rules. Folding can *lengthen* a word: `'İ'` becomes two
  scalars, so it occupies two nodes.
- **Folding is not normalization and not case-*folding* in the Unicode sense.**
  `'ß'` has no single-character uppercase, so `"straße"` and `"strasse"` remain
  different words on a folding trie. Decomposed and precomposed forms of the same
  grapheme are different words too — normalize before inserting if that matters.
- **Nothing is trimmed or tokenized.** Whitespace and punctuation are ordinary
  scalars; `"  double  "` is a word with its spaces. Split text with
  [Tokenizers](./tokenizers.md) first.

## Common mistakes

**Expecting `contains` to match prefixes.** `contains` is exact-word. With only
`"tested"` stored, `contains("test")` is `false`; the prefix question is
`iter_keys_with_prefix("test").next().is_some()`.

**Confusing `len` with `node_count`.** `len()` counts stored words;
`node_count()` counts nodes, root included — for `["a", "ab", "abc"]` that is 3
words and 4 nodes.

**Treating `Some("")` as "no match" in `longest_prefix`.** If the empty string
was inserted, the root is a word and every total miss returns `word: Some("")`,
not `None`.

**Sorting the output of `keys_with_prefix`.** It is already in `<str as Ord>`
order. Sorting it again is work with no effect.

**Building the whole result to count it.** `keys_with_prefix(p).len()` walks the
entire subtree; `iter_keys_with_prefix(p).count()` reads one maintained field.

**Building the whole result to check emptiness.**
`keys_with_prefix(p).is_empty()` has the same problem;
`iter_keys_with_prefix(p).next().is_none()` does not.

**Calling `longest_prefix` when you only need offsets.**
`longest_prefix_lengths` is the same walk without the string building, and it
allocates nothing even when folding rewrites the input.

**Freezing for a handful of lookups.** `freeze()` is a linear pass over every
node plus a full key table; it pays back over many queries, not a few.

**Looking for `remove`.** There is none. See [Removing words](#removing-words).

## Related

- [Choosing an API](../choosing/index.md) — the cross-crate version of the
  decision table above.
- [Iterator vs. `_into`](../performance/iterator-vs-into.md) — why the lazy
  variants exist and when they pay.
- [Allocation](../performance/allocation.md) — what "allocation-free" means
  across Verbora.
- [Cache locality](../performance/cache-locality.md) — the arena's other
  advantage.
- [Parallelism](../performance/parallelism.md) — what you can and cannot
  parallelise.
- [Performance overview](../performance/index.md) ·
  [Benchmarks](../benchmarks/index.md)
- [Tokenizers](./tokenizers.md) — produce the strings you insert.
- [Normalizers](./normalizers.md) — fold decomposed and precomposed forms
  together before inserting.
- [Distance](./distance.md) — for fuzzy matching, which a trie cannot do.
- [Core traits](./core.md) — the shared vocabulary the rest of the workspace
  uses.
- [Recipes](../recipes/index.md) — end-to-end pipelines.

## API reference

```rust ignore
// verbora_trie
pub struct Trie { /* private */ }
pub struct FrozenTrie { /* private */ }
pub struct KeysWithPrefix<'t> { /* private */ }
pub struct PrefixMatches<'t, 'a> { /* private */ }
pub struct FrozenKeysWithPrefix<'t> { /* private */ }

pub enum CaseHandling { Sensitive, Folded }

pub struct PrefixSplit<'a> { pub word: Option<Cow<'a, str>>, pub rest: Cow<'a, str> }
pub struct PrefixSplitLengths { pub word: Option<usize>, pub rest: usize }

impl Trie {
    // Construction
    pub fn new() -> Self;                                   // case-sensitive
    pub fn case_insensitive() -> Self;
    pub fn with_case_handling(handling: CaseHandling) -> Self;
    pub fn case_handling(&self) -> CaseHandling;
    pub fn reserve(&mut self, additional: usize);           // nodes, not words

    // Mutation
    pub fn insert(&mut self, string: &str) -> bool;         // true = ADDED by this call
    pub fn insert_all<I>(&mut self, list: I)
    where I: IntoIterator, I::Item: AsRef<str>;

    // Query
    pub fn contains(&self, string: &str) -> bool;
    pub fn len(&self) -> usize;                             // words, O(1)
    pub fn is_empty(&self) -> bool;
    pub fn node_count(&self) -> usize;                      // nodes, O(1)

    pub fn keys_with_prefix(&self, prefix: &str) -> Vec<String>;
    pub fn iter_keys_with_prefix(&self, prefix: &str) -> KeysWithPrefix<'_>;
    pub fn for_each_key_with_prefix<F: FnMut(&str)>(&self, prefix: &str, f: F);
    pub fn keys(&self) -> KeysWithPrefix<'_>;

    pub fn prefix_matches<'a>(&self, search: &'a str) -> Vec<Cow<'a, str>>;
    pub fn iter_prefix_matches<'a>(&self, search: &'a str) -> PrefixMatches<'_, 'a>;

    pub fn longest_prefix<'a>(&self, search: &'a str) -> PrefixSplit<'a>;
    pub fn longest_prefix_lengths(&self, search: &str) -> PrefixSplitLengths;

    // Build -> Freeze -> Query
    pub fn freeze(&self) -> FrozenTrie;
}

impl FrozenTrie {
    pub fn case_handling(&self) -> CaseHandling;
    pub fn contains(&self, string: &str) -> bool;
    pub fn len(&self) -> usize;                             // words
    pub fn is_empty(&self) -> bool;
    pub fn node_count(&self) -> usize;                      // kept nodes, not scalars

    pub fn keys_with_prefix(&self, prefix: &str) -> Vec<String>;
    pub fn keys_slice(&self, prefix: &str) -> &[String];    // borrowed, no allocation
    pub fn iter_keys_with_prefix(&self, prefix: &str) -> FrozenKeysWithPrefix<'_>;
    pub fn keys(&self) -> FrozenKeysWithPrefix<'_>;
}

// Trait implementations
impl Default for Trie;                       // = Trie::new()
impl Clone for Trie;
impl Debug for Trie;
impl PartialEq for Trie;                     // nodes and case handling
impl Eq for Trie;
impl<S: AsRef<str>> Extend<S> for Trie;
impl<S: AsRef<str>> FromIterator<S> for Trie;    // case-sensitive
impl<'a> IntoIterator for &'a Trie;              // Item = String, IntoIter = KeysWithPrefix<'a>

impl Clone for FrozenTrie;
impl Debug for FrozenTrie;
impl PartialEq for FrozenTrie;
impl Eq for FrozenTrie;
impl<'a> IntoIterator for &'a FrozenTrie;        // Item = String, IntoIter = FrozenKeysWithPrefix<'a>

impl Iterator for KeysWithPrefix<'_>;            // Item = String; exact count and size_hint
impl FusedIterator for KeysWithPrefix<'_>;
impl Debug for KeysWithPrefix<'_>;

impl<'a> Iterator for PrefixMatches<'_, 'a>;     // Item = Cow<'a, str>
impl FusedIterator for PrefixMatches<'_, '_>;
impl Debug for PrefixMatches<'_, '_>;

impl Iterator for FrozenKeysWithPrefix<'_>;      // O(1) count, nth and size_hint
impl FusedIterator for FrozenKeysWithPrefix<'_>;
impl Debug for FrozenKeysWithPrefix<'_>;
```

No `remove`, no `clear`, no batch API, no parallel API, and no `unsafe`
anywhere. `Trie` and `FrozenTrie` are both `Send + Sync`.
