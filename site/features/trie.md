# Trie

`verbora-trie` is a prefix tree keyed by UTF-16 code units, tested against
The reference's the reference trie. It answers four questions about a set of
strings: *is this exact string stored?*, *which stored strings start with this
prefix?*, *which stored strings are prefixes of this string?*, and *where does
the longest stored prefix of this string end?* The whole crate is one type,
`Trie`, plus the two iterators it hands out.

The port keeps every observable behaviour of the reference — including one of its
bugs — while replacing the object-per-node storage with a single flat arena. That
storage decision is the most consequential performance choice in Verbora, and it
is described in detail under [Performance characteristics](#performance-characteristics).

<div class="callout callout-spec">
<strong>Specification status.</strong> <code>add_string</code>,
<code>add_strings</code>, <code>size</code>, <code>contains</code>,
<code>keys_with_prefix</code>, <code>find_matches_on_path</code> and
<code>find_prefix</code> are documented and test-pinned, interleaved
mutation/query sequences included.
<code>cargo test -p verbora-trie</code> runs <strong>33</strong> unit tests and
<strong>9</strong> doctests.
</div>

## When to use it

- **Autocomplete and typeahead.** `iter_keys_with_prefix` streams completions in
  the reference's order and stops when you stop.
- **Longest-match tokenization and dictionary segmentation.** `find_prefix` and
  `find_prefix_lengths` give you the split point of the longest stored word that
  prefixes the input, in one linear walk.
- **Membership over a large, static string set** where the strings share
  prefixes. Node sharing means a dictionary of inflected forms costs far less
  than one entry per word.
- **Porting the reference that used the reference's `Trie`.** The results — including
  enumeration order — are byte-identical to the reference.

## When not to use it

- **You only need set membership.** A `HashSet<String>` is simpler and has a
  better constant factor when you never query by prefix. A trie earns its keep
  through prefix queries and prefix sharing, not through `contains` alone.
- **You need to remove entries.** There is **no `remove`, no `delete`, and no
  `clear`** — the reference has none, so the port has none. See
  [Removing words](#removing-words) for the rebuild pattern.
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
    trie.add_strings(["and", "their", "they", "them"]);

    assert!(trie.contains("they"));
    assert!(!trie.contains("the")); // a prefix is not a word

    // Children are visited in insertion order, not sorted order.
    assert_eq!(trie.keys_with_prefix("the"), ["their", "they", "them"]);
    assert_eq!(trie.find_matches_on_path("theyre"), ["they"]);
    assert_eq!(trie.find_prefix_lengths("theyre"), (Some(4), 2));
}
```

That `["their", "they", "them"]` is not a typo and not a sort. Enumeration order
is the reference's, and it is explained under
[reference `for…in` child ordering](#reference-for-in-child-ordering).

## Construction

```rust
use verbora_trie::Trie;

fn main() {
    assert!(Trie::new().is_case_sensitive());
    assert!(Trie::default().is_case_sensitive());
    assert!(Trie::with_case_sensitivity(true).is_case_sensitive());
    assert!(!Trie::case_insensitive().is_case_sensitive());
    assert!(!Trie::with_case_sensitivity(false).is_case_sensitive());

    let mut t = Trie::new();
    assert_eq!(t.get_size(), 1); // the root always exists
    t.add_string("hi");
    assert_eq!(t.get_size(), 3);
}
```

| Constructor | Equivalent the reference | Folds case? |
|---|---|:--:|
| `Trie::new()` | `new Trie()` | ❌ |
| `Trie::default()` | `new Trie()` | ❌ |
| `Trie::with_case_sensitivity(true)` | `new Trie(true)` | ❌ |
| `Trie::with_case_sensitivity(false)` | `new Trie(false)` | ✅ |
| `Trie::case_insensitive()` | `new Trie(false)` | ✅ |
| `["a", "ab"].into_iter().collect::<Trie>()` | — | ❌ |

**The default is case-sensitive.** `is_case_sensitive()` reports which mode a
trie is in.

<div class="callout callout-note">
<strong>Note.</strong> The reference defaults its flag when the constructor
argument is <code>undefined</code> but tests it with a strict <code>=== false</code>
everywhere else, so <code>new Trie(null)</code> and <code>new Trie(0)</code> are
both case-<em>sensitive</em>. Only a literal <code>false</code> enables folding —
which is exactly what a Rust <code>bool</code> expresses, so
<code>with_case_sensitivity</code> needs no three-state dance.
</div>

`FromIterator` and `Extend` are implemented for any `IntoIterator` whose items
are `AsRef<str>`. Both build (or extend) a **case-sensitive** trie; for the
folding variant, construct with `Trie::case_insensitive()` and use `add_strings`.

### `reserve`

`reserve(additional)` reserves capacity for `additional` more **nodes**, not
words. A trie needs roughly one node per distinct prefix, counted in UTF-16 code
units; the total UTF-16 length of the input is a safe upper bound.

```rust
use verbora_trie::Trie;

fn bulk_load(words: &[String]) -> Trie {
    let mut trie = Trie::new();
    // One node per distinct prefix; total UTF-16 length is a safe upper bound.
    let upper_bound: usize = words.iter().map(|w| w.encode_utf16().count()).sum();
    trie.reserve(upper_bound + 1);
    trie.add_strings(words.iter().map(String::as_str));
    trie
}

fn main() {
    let words = vec![String::from("alpha"), String::from("beta")];
    let trie = bulk_load(&words);
    assert_eq!(trie.keys_with_prefix(""), ["alpha", "beta"]);
}
```

Reserving up front removes the arena's growth reallocations from a bulk load.
`add_strings` already reserves the iterator's `size_hint().0` — one node per
item, a lower bound — which skips the first few doublings but not the rest; call
`reserve` yourself when you know the real size.

## Insertion

### `add_string`

```rust
use verbora_trie::Trie;

fn main() {
    let mut trie = Trie::new();
    assert!(!trie.add_string("test")); // false: it was NOT already there
    assert!(trie.add_string("test")); // true: it WAS already a word

    // The empty string is a word that creates no node.
    assert!(!trie.add_string(""));
    assert!(trie.contains(""));
    assert_eq!(trie.get_size(), 5); // root + t + e + s + t
}
```

<div class="callout callout-warn">
<strong>Careful.</strong> <code>add_string</code> returns <strong>true when the
string was already present</strong>. That is the reference's convention and the
opposite of <code>HashSet::insert</code>, which returns <code>true</code> when
the value is <em>new</em>. If you want "did I insert something?", negate it:
<code>let inserted = !trie.add_string(w);</code>
</div>

Adding the empty string marks the root as a word. It creates no node, so
`get_size()` does not change — but `contains("")` becomes `true`, `""` becomes
the first result of `keys_with_prefix("")` and of every `find_matches_on_path`,
and `find_prefix` starts returning `Some("")` instead of `None` for total misses.

### `add_strings`

`add_strings<I>(list)` takes any `IntoIterator` whose items are `AsRef<str>` —
arrays, `Vec<String>`, `&Vec<String>`, iterator adapters. It reserves
`size_hint().0` nodes and then calls `add_string` per item, so the return values
are discarded.

<div class="callout callout-note">
<strong>Note.</strong> The reference's <code>addStrings</code> iterates with
<code>for…in</code>, which skips array holes and, if handed a <em>string</em>,
walks its characters. A Rust iterator has neither behaviour, so
<code>add_strings</code> is the faithful port of what that loop actually visits:
pass the sequence you want inserted.
</div>

There is no batch or parallel insertion API. `add_string` needs `&mut self` and
mutates shared arena state, so building a trie is inherently single-threaded; see
[Sharing a trie across threads](#sharing-a-trie-across-threads) for what *can* be
parallelised.

## Choosing the right API

The query surface has three lazy/materialised pairs, plus two more decisions that
are easy to get wrong. This section is the map.

### Comparison table

"Folds" means *on a case-insensitive trie* — a case-sensitive trie never folds
anything. "Allocations" assumes the folding step had nothing to do.

| API | Answers | Lazy | Output | Folds | Allocations |
|---|---|:--:|---|:--:|---|
| `contains(s)` | is `s` a stored word? | n/a | `bool` | ✅ | none, unless folding rewrites `s` |
| `get_size()` | how many nodes? | n/a | `usize` | n/a | none — O(1) |
| `keys_with_prefix(p)` | all words under `p` | ❌ | `Vec<String>` | ❌ **never** | one `Vec` + one `String` per key |
| `iter_keys_with_prefix(p)` | all words under `p` | ✅ | `KeysWithPrefix` → `String` | ❌ **never** | one path buffer + one stack; one `String` per key yielded |
| `keys()` | all words | ✅ | `KeysWithPrefix` → `String` | n/a | as above |
| `find_matches_on_path(s)` | stored words that prefix `s` | ❌ | `Vec<Cow<'a, str>>` | ✅ | one `Vec`; items borrow `s` |
| `iter_matches_on_path(s)` | stored words that prefix `s` | ✅ | `MatchesOnPath` → `Cow<'a, str>` | ✅ | none on a case-sensitive trie |
| `find_prefix(s)` | longest stored prefix + remainder | n/a | `(Option<Cow>, Cow)` | ✅ | none in the common case; see below |
| `find_prefix_lengths(s)` | the same split, in code units | n/a | `(Option<usize>, usize)` | ✅ | none |

Two columns deserve a second look:

- **"Folds the argument" is `❌ never` for the `keys_with_prefix` family even on
  a case-insensitive trie.** That is a reproduced the reference bug, described in
  [the preserved bug below](#keyswithprefix-never-folds-case).
- **`find_prefix` allocates in two situations**: when folding actually rewrites
  the search string, and when the walk stops between the halves of a surrogate
  pair. `find_prefix_lengths` allocates in the first situation only, and is exact
  in the second.

### Decision tree

```text
I have a Trie and a string
│
├── "Is this exact string stored?"
│      └── contains()
│
├── "How big is the structure?"
│      └── get_size()          (nodes, not words — O(1))
│
├── "Which stored words START WITH my string?"
│   │
│   ├── I need all of them, and I need to keep/index them
│   │      └── keys_with_prefix()        → Vec<String>
│   │
│   ├── I need the first N, or I stop on a condition
│   │      └── iter_keys_with_prefix().take(N)
│   │
│   ├── I only need "does anything start with this?"
│   │      └── iter_keys_with_prefix().next().is_some()
│   │
│   └── I want every word in the trie
│          └── keys()          (lazy; same as iter_keys_with_prefix(""))
│
├── "Which stored words ARE PREFIXES OF my string?"
│   │
│   ├── All of them, shortest first
│   │      └── find_matches_on_path()    → Vec<Cow<str>>
│   │
│   ├── Only the shortest / only the first few
│   │      └── iter_matches_on_path().next() / .take(n)
│   │
│   └── Only the LONGEST
│          └── find_prefix().0           (one walk, no iterator)
│
└── "Where does the longest stored prefix end?"
    │
    ├── I need the text of the two halves
    │      └── find_prefix()             → (Option<Cow>, Cow)
    │
    └── I need offsets, exactness, or zero allocation
           └── find_prefix_lengths()     → (Option<usize>, usize)
```

### `keys_with_prefix` <a class="badge badge-owned" href="../performance/allocation">OWNED</a>

<div class="perf">
<div class="perf-row"><span class="perf-k">Execution</span><span class="perf-v">Eager — walks the entire subtree</span></div>
<div class="perf-row"><span class="perf-k">Output</span><span class="perf-v">Owned <code>Vec&lt;String&gt;</code></span></div>
<div class="perf-row"><span class="perf-k">Allocations</span><span class="perf-v">One <code>Vec</code> (grown by doubling), one <code>String</code> per key, plus the iterator's path buffer and frame stack</span></div>
<div class="perf-row"><span class="perf-k">Buffer reuse</span><span class="perf-v">Internal only; no caller-supplied buffer</span></div>
<div class="perf-row"><span class="perf-k">Batch</span><span class="perf-v">No</span></div>
<div class="perf-row"><span class="perf-k">Parallel</span><span class="perf-v">No</span></div>
<div class="perf-row"><span class="perf-k">Best for</span><span class="perf-v">Small result sets you need to keep, index, or return</span></div>
</div>

Literally `self.iter_keys_with_prefix(prefix).collect()`. Reach for it when the
result is small and you want to hold on to it; reach for the iterator when it
might not be.

### `iter_keys_with_prefix` and `keys` <a class="badge badge-lazy" href="../performance/iterator-vs-into">LAZY</a>

<div class="perf">
<div class="perf-row"><span class="perf-k">Execution</span><span class="perf-v">Lazy — depth-first, one key per <code>next()</code></span></div>
<div class="perf-row"><span class="perf-k">Output</span><span class="perf-v"><code>KeysWithPrefix&lt;'_&gt;</code>, yielding owned <code>String</code></span></div>
<div class="perf-row"><span class="perf-k">Allocations</span><span class="perf-v">One reusable path <code>String</code> and one frame <code>Vec</code>, both O(depth); one <code>String</code> per key actually yielded</span></div>
<div class="perf-row"><span class="perf-k">Buffer reuse</span><span class="perf-v">The path buffer is pushed and truncated for the whole traversal</span></div>
<div class="perf-row"><span class="perf-k">Batch</span><span class="perf-v">No</span></div>
<div class="perf-row"><span class="perf-k">Parallel</span><span class="perf-v">No</span></div>
<div class="perf-row"><span class="perf-k">Best for</span><span class="perf-v">Autocomplete, existence checks, any early <code>take</code>/<code>find</code></span></div>
</div>

`keys()` is exactly `iter_keys_with_prefix("")`, and `&trie` implements
`IntoIterator` with the same behaviour, so `for word in &trie` works.

Both are `FusedIterator`: once they return `None` they keep returning `None`.

Each item is an owned `String` because a stored word exists nowhere
contiguously — it is spelled out one code unit per node — so it has to be
materialised. That cost is visible in the item type rather than hidden. What the
iterator *does* avoid is the result `Vec` and every key you never asked for.

#### Worked example: early termination

An autocomplete box shows ten suggestions. The user types `search`, which in
this trie has 5,000 completions.

```rust
use verbora_trie::Trie;

fn suggest(trie: &Trie, prefix: &str, limit: usize) -> Vec<String> {
    trie.iter_keys_with_prefix(prefix).take(limit).collect()
}

fn main() {
    let mut trie = Trie::new();
    trie.add_strings((0..5_000).map(|i| format!("search{i:04}")));

    // Materialising: walks all 5,000 keys and allocates 5,001 Strings.
    let all = trie.keys_with_prefix("search");
    assert_eq!(all.len(), 5_000);

    // Streaming: stops after 10 keys, allocates 10 Strings plus the path buffer.
    let page = suggest(&trie, "search", 10);
    assert_eq!(page.len(), 10);
    assert_eq!(page[0], "search0000");

    // "Is there anything under this prefix?" needs exactly one key.
    assert!(trie.iter_keys_with_prefix("search1").next().is_some());
    assert!(trie.iter_keys_with_prefix("zzz").next().is_none());
}
```

The two paths do the same amount of work per key. The difference is how many keys
get visited: `keys_with_prefix` walks the whole subtree and allocates 5,000
`String`s plus one growing `Vec`; `.take(10)` stops the depth-first walk after
ten keys and allocates ten. On a trie built from a real vocabulary, a one-letter
prefix routinely has tens of thousands of completions and a UI wants ten of them.

The existence check is the sharper version of the same point. Calling
`keys_with_prefix(p)` and testing `is_empty()` builds the whole subtree only to
throw it away; `iter_keys_with_prefix(p).next().is_some()` descends the prefix
and takes at most one more step.

### `find_matches_on_path` <a class="badge badge-owned" href="../performance/allocation">OWNED</a>

<div class="perf">
<div class="perf-row"><span class="perf-k">Execution</span><span class="perf-v">Eager — one linear walk of the search string</span></div>
<div class="perf-row"><span class="perf-k">Output</span><span class="perf-v"><code>Vec&lt;Cow&lt;'a, str&gt;&gt;</code>, items cut from the search string</span></div>
<div class="perf-row"><span class="perf-k">Allocations</span><span class="perf-v">One <code>Vec</code>; no per-item allocation unless folding rewrote the search, in which case one <code>String</code> per match plus the folded copy</span></div>
<div class="perf-row"><span class="perf-k">Buffer reuse</span><span class="perf-v">N/A</span></div>
<div class="perf-row"><span class="perf-k">Batch</span><span class="perf-v">No</span></div>
<div class="perf-row"><span class="perf-k">Parallel</span><span class="perf-v">No</span></div>
<div class="perf-row"><span class="perf-k">Best for</span><span class="perf-v">Every dictionary word that prefixes a token, shortest first</span></div>
</div>

Results are **cut from the search string** (after folding), not rebuilt from the
stored keys, which is why they can borrow. The number of matches is bounded by
the length of the search string, so this `Vec` is small by construction — the
lazy variant matters less here than for `keys_with_prefix`.

### `iter_matches_on_path` <a class="badge badge-lazy" href="../performance/iterator-vs-into">LAZY</a>

<div class="perf">
<div class="perf-row"><span class="perf-k">Execution</span><span class="perf-v">Lazy — advances the walk one character per <code>next()</code></span></div>
<div class="perf-row"><span class="perf-k">Output</span><span class="perf-v"><code>MatchesOnPath&lt;'_, 'a&gt;</code>, yielding <code>Cow&lt;'a, str&gt;</code> borrowed from the search string</span></div>
<div class="perf-row"><span class="perf-k">Allocations</span><span class="perf-v">None on a case-sensitive trie; one <code>String</code> for the folded search when folding changes the input</span></div>
<div class="perf-row"><span class="perf-k">Buffer reuse</span><span class="perf-v">N/A</span></div>
<div class="perf-row"><span class="perf-k">Batch</span><span class="perf-v">No</span></div>
<div class="perf-row"><span class="perf-k">Parallel</span><span class="perf-v">No</span></div>
<div class="perf-row"><span class="perf-k">Best for</span><span class="perf-v">The shortest match, or a bounded number of them</span></div>
</div>

```rust
use verbora_trie::Trie;
use std::borrow::Cow;

fn main() {
    let mut trie = Trie::new();
    trie.add_strings(["a", "ab", "bc", "cd", "abc"]);

    // All of them.
    let all: Vec<Cow<'_, str>> = trie.find_matches_on_path("abcd");
    assert_eq!(all, ["a", "ab", "abc"]);

    // Shortest only: one step of the walk.
    let shortest = trie.iter_matches_on_path("abcd").next();
    assert_eq!(shortest.as_deref(), Some("a"));

    // Longest only: find_prefix answers it without an iterator at all.
    let (longest, rest) = trie.find_prefix("abcd");
    assert_eq!(longest.as_deref(), Some("abc"));
    assert_eq!(rest, "d");
}
```

<div class="callout callout-note">
<strong>Note.</strong> Do not reach for <code>.last()</code> on
<code>iter_matches_on_path</code> to get the longest match. It works, but it walks
the whole string and yields every shorter match on the way.
<code>find_prefix(s).0</code> is the same answer from the same single walk, and
<code>find_prefix_lengths(s).0</code> is that answer without any allocation.
</div>

Like `KeysWithPrefix`, `MatchesOnPath` is a `FusedIterator`.

### `find_prefix` <span class="badge badge-utf16">UTF-16</span>

<div class="perf">
<div class="perf-row"><span class="perf-k">Execution</span><span class="perf-v">Eager — one linear walk</span></div>
<div class="perf-row"><span class="perf-k">Output</span><span class="perf-v"><code>(Option&lt;Cow&lt;'a, str&gt;&gt;, Cow&lt;'a, str&gt;)</code> — longest stored prefix, and the unconsumed remainder</span></div>
<div class="perf-row"><span class="perf-k">Allocations</span><span class="perf-v">None when nothing folds and the walk stops on a character boundary; otherwise the folded copy, and one <code>String</code> for the remainder when the walk splits a surrogate pair</span></div>
<div class="perf-row"><span class="perf-k">Buffer reuse</span><span class="perf-v">N/A</span></div>
<div class="perf-row"><span class="perf-k">Batch</span><span class="perf-v">No</span></div>
<div class="perf-row"><span class="perf-k">Parallel</span><span class="perf-v">No</span></div>
<div class="perf-row"><span class="perf-k">Best for</span><span class="perf-v">Longest-match segmentation where you need the text of both halves</span></div>
</div>

Mirrors the reference's two-element `[lastWord, remainder]` array.

```rust
use verbora_trie::Trie;
use std::borrow::Cow;

fn main() {
    let mut trie = Trie::new();
    trie.add_strings(["their", "and", "they"]);

    let (word, rest) = trie.find_prefix("theyre");
    assert_eq!(word.as_deref(), Some("they"));
    assert_eq!(rest, "re");
    // Borrowed on a case-sensitive trie: no allocation.
    assert!(matches!(word, Some(Cow::Borrowed(_))));
    assert!(matches!(rest, Cow::Borrowed(_)));

    // The remainder is where the WALK died, not where the word ended.
    let mut partial = Trie::new();
    partial.add_strings(["their", "and"]);
    let (word, rest) = partial.find_prefix("theyre");
    assert_eq!(word, None);
    assert_eq!(rest, "yre"); // the walk got as far as "the"
}
```

Two details are easy to get wrong, and both are the reference's semantics:

1. **The remainder is what was left when the walk died**, not what was left after
   the last word ended. The two coincide only when the walk stops exactly at the
   end of a stored word.
2. **`Some("")` and `None` are different answers.** A trie containing the empty
   string returns `(Some(""), "zzz")` for `find_prefix("zzz")`, so a
   `if let Some(w) = … if !w.is_empty()` guard diverges from the reference.

### `find_prefix_lengths` <a class="badge badge-alloc" href="../performance/allocation">ALLOCATION-FREE</a> <span class="badge badge-utf16">UTF-16</span>

<div class="perf">
<div class="perf-row"><span class="perf-k">Execution</span><span class="perf-v">Eager — the same single walk as <code>find_prefix</code></span></div>
<div class="perf-row"><span class="perf-k">Output</span><span class="perf-v"><code>(Option&lt;usize&gt;, usize)</code> — lengths in UTF-16 code units</span></div>
<div class="perf-row"><span class="perf-k">Allocations</span><span class="perf-v">None; on a case-insensitive trie, one <code>String</code> only if folding actually rewrites the input</span></div>
<div class="perf-row"><span class="perf-k">Buffer reuse</span><span class="perf-v">N/A</span></div>
<div class="perf-row"><span class="perf-k">Batch</span><span class="perf-v">No</span></div>
<div class="perf-row"><span class="perf-k">Parallel</span><span class="perf-v">No</span></div>
<div class="perf-row"><span class="perf-k">Best for</span><span class="perf-v">Hot segmentation loops, and any case where the split must be exact</span></div>
</div>

```rust
use verbora_trie::Trie;

fn main() {
    let mut trie = Trie::new();
    trie.add_strings(["their", "and", "they"]);
    assert_eq!(trie.find_prefix_lengths("theyre"), (Some(4), 2));
}
```

Prefer this whenever you do not need the two halves as strings. It is the same
walk with the string-building removed, and it is the only one of the two that
stays exact when the walk stops inside a surrogate pair — see
[the divergence below](#the-find-prefix-surrogate-divergence).

<div class="callout callout-warn">
<strong>Careful.</strong> The lengths are <strong>UTF-16 code units</strong>, not
bytes and not <code>char</code>s. They are directly comparable to the reference's
<code>String#length</code>, and they index a Rust <code>&amp;str</code> only after
you convert. For pure ASCII all three coincide, which is exactly what makes this
easy to get wrong later.
</div>

## Advanced usage

### Sharing a trie across threads

A `Trie` is a plain owned value: a `Vec<Node>` and a `bool`, where a `Node` holds
a `SmallVec<[Child; 2]>` of `Copy` fields. It is therefore `Send + Sync` (this is
asserted below, and compiles), and every query method takes `&self`.

```rust
use verbora_trie::Trie;
use std::sync::Arc;

fn main() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Trie>();

    let mut trie = Trie::new();
    trie.add_strings(["alpha", "beta", "gamma"]);
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

If the trie outlives the threads, scoped threads avoid the `Arc` entirely:

```rust
use verbora_trie::Trie;

fn main() {
    let mut trie = Trie::new();
    trie.add_strings(["alpha", "beta"]);

    let found = std::thread::scope(|s| {
        let a = s.spawn(|| trie.contains("alpha"));
        let b = s.spawn(|| trie.contains("beta"));
        a.join().unwrap() && b.join().unwrap()
    });
    assert!(found);
}
```

<div class="callout callout-note">
<strong>Note.</strong> <code>verbora-trie</code> ships <strong>no <code>par_*</code>
API</strong> — the Fase 2 performance audit evaluated one and rejected it: query
cost measured at ~67 ns, at or below typical <code>rayon</code> dispatch
overhead, so a naive <code>par_iter</code> over queries would likely lose to its
own scheduling cost. <strong>Construction cannot be parallelised at all</strong>:
<code>add_string</code> takes <code>&amp;mut self</code> and appends to one
shared arena. What the type system still gives you is the freedom to
parallelise <em>queries</em> yourself: share one <code>Arc&lt;Trie&gt;</code> and
drive <code>contains</code>, <code>find_prefix_lengths</code> or
<code>iter_matches_on_path</code> from a <code>rayon</code> parallel iterator over
your inputs — build once, wrap in an <code>Arc</code>, then fan out. See
<a href="../performance/parallelism">Parallelism</a> for the full table of the
thirteen crates that do ship one.
</div>

### Removing words

There is no `remove`, no `delete` and no `clear` — the reference has none, so the
port has none, and this is the first thing users go looking for. The pattern is
to rebuild from `keys()`, which is lazy, so the old trie is streamed rather than
materialised:

```rust
use verbora_trie::Trie;

fn rebuild_without(trie: &Trie, drop: &str) -> Trie {
    let mut rebuilt = if trie.is_case_sensitive() {
        Trie::new()
    } else {
        Trie::case_insensitive()
    };
    rebuilt.reserve(trie.get_size());
    rebuilt.add_strings(trie.keys().filter(|w| w != drop));
    rebuilt
}

fn main() {
    let mut trie = Trie::new();
    trie.add_strings(["alpha", "beta", "gamma"]);

    let smaller = rebuild_without(&trie, "beta");
    assert_eq!(smaller.keys_with_prefix(""), ["alpha", "gamma"]);
    assert!(!smaller.contains("beta"));
    // Nodes that only "beta" needed are gone, so the arena shrank.
    assert!(smaller.get_size() < trie.get_size());
}
```

This is O(total stored text) and allocates a `String` per surviving word, so it
is a maintenance operation, not something to do per request. If your workload
needs frequent deletion, a trie with these constraints is the wrong structure —
keep an auxiliary `HashSet` of tombstones and filter results, or rebuild on a
schedule.

`reserve(trie.get_size())` is exactly right here: the rebuilt trie can never need
more nodes than the original had.

## the reference properties reproduced exactly

Three properties of the reference are observable through its public API and are
reproduced exactly, even though none of them is what a fresh design would choose.
All three are verified by the test suite.

### UTF-16 code-unit keying

<span class="badge badge-utf16">UTF-16</span>

The reference indexes each node's child map by the result of the reference string
indexing, which yields **UTF-16 code units**, not Unicode scalar values. A
non-BMP character such as `'😀'` (U+1F600) is a surrogate pair, so it occupies
**two levels** of the tree and `get_size` counts it twice.

```rust
use verbora_trie::Trie;

fn main() {
    let mut trie = Trie::new();
    trie.add_string("a👍");
    // root + 'a' + high surrogate + low surrogate
    assert_eq!(trie.get_size(), 4);

    let mut bmp = Trie::new();
    bmp.add_string("日本語");
    assert_eq!(bmp.get_size(), 4); // one node per BMP character
}
```

A `char`-keyed port would report `3` for the first case and would split
`find_prefix` at different points. Everything user-visible stays correct — words
round-trip, `contains` works, iteration reassembles surrogate pairs into proper
`char`s — but *node counts* and *walk failure points* follow UTF-16. The one place
this leaks into results is [the `find_prefix` surrogate
divergence](#the-find-prefix-surrogate-divergence).

### Reference `for…in` child ordering

The reference enumerates a node's children with `for…in` over a plain object,
whose order is specified: **integer-index-like keys first, in ascending numeric
order, then every other key in insertion order.** Trie keys are single code
units, so the only keys that qualify as array indices are the ASCII digits
`'0'`–`'9'` — `'10'` is two code units and never appears as a key, and non-ASCII
digits such as `'٣'` are not canonical decimal spellings.

```rust
use verbora_trie::Trie;

fn main() {
    let mut trie = Trie::new();
    trie.add_strings(["b1", "a1", "9x", "1x", "0x", "zz"]);
    assert_eq!(trie.keys_with_prefix(""), ["0x", "1x", "9x", "b1", "a1", "zz"]);
}
```

Read that result carefully: `0x`, `1x`, `9x` come first *sorted*, then `b1`,
`a1`, `zz` in the order they were inserted. Neither a `HashMap` nor a `BTreeMap`
nor a plain insertion-ordered list reproduces it. The port keeps each node's
child list in this order *on insertion*, so iteration is a straight scan with no
sorting at read time.

Traversal is otherwise pre-order depth-first: a node's own word is emitted
**before** its children are visited, which is why `"a"` precedes `"ab"`.

### `keysWithPrefix` never folds case

<div class="callout callout-warn">
<strong>Careful.</strong> On a case-insensitive trie, every method folds its
argument <strong>except</strong> <code>keys_with_prefix</code>,
<code>iter_keys_with_prefix</code> and <code>keys</code>. An upper-case prefix
matches nothing, because every stored word was folded on the way in. This is a
The reference bug, reproduced deliberately.
</div>

The reference guards its lowercasing with `if (this.caseSensitive === false)`,
but its constructor stores the flag as `this.cs`. `this.caseSensitive` is
therefore always `undefined`, the guard never fires, and the prefix is never
folded — not even on a trie that folded everything it stored.

```rust
use verbora_trie::Trie;

fn main() {
    let mut trie = Trie::case_insensitive();
    trie.add_strings(["thEIr", "And", "theY"]);

    // Every other method folds.
    assert!(trie.contains("THEIR"));
    assert_eq!(trie.find_matches_on_path("THEYRE"), ["they"]);
    assert_eq!(trie.find_prefix("ThEyRe").0.as_deref(), Some("they"));

    // keys_with_prefix does not.
    assert_eq!(trie.keys_with_prefix("th"), ["their", "they"]);
    assert!(trie.keys_with_prefix("TH").is_empty()); // not a typo
}
```

Correcting this would silently change results for every caller who depends on the
recorded behaviour, so it is preserved and documented rather than fixed. **If you
want the intended semantics, fold the prefix yourself:**

```rust
use verbora_trie::Trie;
use std::borrow::Cow;

fn keys_with_prefix_folded<'a>(trie: &Trie, prefix: &'a str) -> Vec<String> {
    let folded: Cow<'a, str> = if trie.is_case_sensitive() {
        Cow::Borrowed(prefix)
    } else {
        Cow::Owned(prefix.to_lowercase())
    };
    trie.keys_with_prefix(&folded)
}

fn main() {
    let mut trie = Trie::case_insensitive();
    trie.add_strings(["thEIr", "And", "theY"]);
    assert_eq!(keys_with_prefix_folded(&trie, "TH"), ["their", "they"]);
}
```

`str::to_lowercase` and the reference's `toLowerCase` agree on every Unicode scalar
value, so folding the prefix yourself matches what the reference *meant* to do.

## The `find_prefix` surrogate divergence

This is the one place where the Rust output cannot be byte-identical to
the reference's, and it is a limitation of `String`, not of the walk.

Because the walk advances one code unit at a time, it can stop **between** the
halves of a surrogate pair. The reference happily returns a remainder that begins
with an unpaired low surrogate; a Rust `String` cannot hold one. That single
position is therefore rendered as `U+FFFD` (`�`). Everything after it is intact,
and the split point itself is exact.

```rust
use verbora_trie::Trie;

fn main() {
    let mut trie = Trie::new();
    trie.add_string("a👍"); // U+1F44D = D83D DC4D

    // U+1F44C = D83D DC4C shares the high surrogate but not the low one, so the
    // walk consumes three of the four code units.
    let (word, rest) = trie.find_prefix("a👌");
    assert_eq!(word, None);
    assert_eq!(rest, "\u{FFFD}"); // rendered lossily
    assert_eq!(trie.find_prefix_lengths("a👌"), (None, 1)); // exact

    // A character that differs in its FIRST half dies on a clean boundary, and
    // the remainder is ordinary text.
    let (_, rest) = trie.find_prefix("a𝕳x");
    assert_eq!(rest, "𝕳x");
    assert_eq!(trie.find_prefix_lengths("a𝕳x"), (None, 3));
}
```

**Use `find_prefix_lengths` if this matters to you.** It reports the split in
code units with no loss at all, which is why the test fixtures pin both. The
divergence can only occur when two stored/searched astral characters share a high
surrogate and differ in the low one — that is, within the same 1,024-code-point
block. It cannot occur for BMP text of any script.

## Performance characteristics

### The structural decision: one flat arena, not one object per node

This is the crate's defining choice, and it is the clearest architectural
performance decision in Verbora.

The reference allocates **one the reference object per node**, each holding its own
hash map, and recurses once per code unit for every operation. A 20,000-word
dictionary is therefore tens of thousands of separately allocated objects
scattered across the heap, chained by pointer.

This port stores **all nodes in one flat `Vec<Node>` arena addressed by `u32`**.
The crate's own summary is that "a trie is two allocations rather than one per
node": the arena is a single contiguous block, and the only other heap traffic is
a spill vector for a node that acquires a third child. Five consequences follow,
each of which a user can observe:

| Decision | Why it matters | What you observe |
|---|---|---|
| Flat `Vec<Node>` arena, `u32` indices | No per-node allocation during a bulk load; nodes are contiguous, so a descent touches consecutive cache lines instead of chasing pointers across the heap | Build time dominated by memcpy-like growth rather than allocator calls; see [Cache locality](../performance/cache-locality.md) |
| `get_size()` is `Vec::len` | The arena's length *is* the node count; nothing to traverse | O(1) instead of a full tree walk. The reference's documentation warns against calling `getSize` frequently; that warning does not apply here |
| `SmallVec<[Child; 2]>` inline children | Nodes with one or two children — the overwhelming majority in a natural-language trie — keep their edges *inside* the node, so they never touch the heap | Lower memory, fewer allocations, and a child lookup that is a linear scan of two contiguous 8-byte entries rather than a hash plus a pointer chase |
| Case folded **once**, at the entry point | The reference re-lowercases the remaining suffix at every recursion level, which is quadratic in word length. Folding is idempotent and the only context-sensitive rule (Greek final sigma) cannot fire on already-folded text, so one pass is observably identical | Case-insensitive operations stay linear in the length of the input. Long words on a folding trie do not degrade |
| Every operation is iterative | The reference recurses once per code unit | A 200,000-code-unit input is a loop, not 200,000 stack frames. `crates/verbora-trie/src/trie.rs` pins this with a test that inserts and queries a 200,000-code-unit word |

The inline child capacity of 2 is not arbitrary. With `SmallVec`'s union
representation, `max(size_of::<[T; N]>(), 16)` bytes are reserved regardless, so
for an 8-byte `Child`, `N = 2` is the largest inline capacity that is **free** —
a `Node` is 32 bytes, exactly what it would cost with a plain `Vec<Child>`.
Raising it to 4 would grow every node in the arena by 16 bytes. The crate asserts
both facts in a unit test.

### Measured: arena vs. one hash map per node

`crates/verbora-trie/benches/trie.rs` builds the closest faithful Rust analogue
of the reference's shape — `HashMap<u16, Box<Node>>` per node — in two flavours,
`std`'s SipHash and `rustc-hash`'s FxHash, so the arena is not flattered by a
slow hasher. Inputs come from `benches/data/words.json` (20,000 words; 32,000 for
`prefix_heavy`, which appends eight inflectional suffixes to 4,000 stems to
produce the shared-stem shape a real vocabulary has).

The numbers recorded in that benchmark's module documentation:

| Benchmark | arena | hashmap (Fx) | hashmap (Sip) |
|---|--:|--:|--:|
| `build/random` | **1.48 ms** | 25.1 ms | 41.9 ms |
| `build/prefix_heavy` | **2.18 ms** | 11.5 ms | 15.2 ms |
| `contains_hit` | **1.13 ms** | 1.20 ms | 3.15 ms |
| `contains_miss` | **1.26 ms** | 1.94 ms | 3.51 ms |
| `get_size` | **0.23 ns** | 1.87 ms | — |

The margin says *why* the layout was chosen. Build is roughly 17× faster than the
fastest hash baseline, because the arena makes no per-node allocation.
`contains_hit` is a near tie with FxHash: at one or two children per node,
scanning an inline array costs about what hashing a `u16` does. **The arena is
chosen for its build cost and its memory behaviour, not for a lookup advantage
that does not exist.** `get_size` is the outlier only because the arena's length
*is* the answer.

Against the reference itself, the benchmark's header records the same inputs running
5.5–15× faster, with `get_size` moving from a full traversal to a field read. The
reference side was recorded at version 8.1.1, and both sides read
byte-identical input data.

Reproduce with:

```text
cargo bench -p verbora-trie
cargo bench -p verbora-trie
```

<div class="callout callout-note">
<strong>Note.</strong> These are one machine's recordings, kept in the benchmark
source rather than as a headline claim. Treat the <em>ratios and their
explanation</em> as the durable part; re-run the commands above for numbers that
mean anything on your hardware. See <a href="../benchmarks/">Benchmarks</a>.
</div>

### Complexity

With `m` = length of the argument in UTF-16 code units and `k` = the number of
children of a node:

| Operation | Complexity |
|---|---|
| `add_string` | O(m · k) — one linear child scan per code unit, plus an ordered insert for new edges |
| `contains` | O(m · k) |
| `get_size` | O(1) |
| `find_prefix`, `find_prefix_lengths` | O(m · k) |
| `find_matches_on_path` | O(m · k); at most one result per character consumed, plus `""` if it was stored |
| `keys_with_prefix(p)` | O(len(p) · k + size of the subtree + total length of the results) |
| `iter_keys_with_prefix(p).take(n)` | O(len(p) · k + the part of the subtree needed for `n` keys) |

`k` is one or two for the overwhelming majority of nodes in natural-language
text, so the `· k` factor behaves as a small constant everywhere below the first
level or two.

## Allocation behaviour

**The trie itself.** One `Vec<Node>` arena, 32 bytes per node, grown by doubling
unless you `reserve`. One additional heap allocation per node that acquires a
third child. Node count equals the number of distinct prefixes across all stored
words, measured in UTF-16 code units, plus one for the root.

**Queries** — assuming a case-sensitive trie, or a case-insensitive one whose
argument is already folded (in which case folding borrows rather than copies):

| Call | Allocates |
|---|---|
| `contains(s)` | nothing |
| `get_size()` | nothing |
| `find_prefix_lengths(s)` | nothing |
| `find_prefix(s)` | nothing, unless the walk splits a surrogate pair (one `String` for the remainder) |
| `iter_matches_on_path(s)` | nothing |
| `find_matches_on_path(s)` | one `Vec`; the items borrow `s` |
| `iter_keys_with_prefix(p)` | one path `String` and one frame `Vec` (both O(depth)), plus one `String` per key yielded |
| `keys_with_prefix(p)` | the above, plus one `Vec` grown by doubling |

**When folding does change the argument** — a case-insensitive trie given
upper-case input — one `String` copy of the argument is made up front, and every
`Cow` result derived from it becomes owned. So on a folding trie,
`find_matches_on_path("THEYRE")` allocates the folded copy plus one `String` per
match, where the same call on a case-sensitive trie allocates only the `Vec`.
Fold your inputs once at your own boundary if this is hot.

There is **no `_into` variant and no caller-supplied output buffer** anywhere in
this crate; the only buffer reuse is internal to `KeysWithPrefix`, which pushes
and truncates one path `String` for the whole traversal instead of building a
fresh string per edge the way the reference does. See
[Allocation](../performance/allocation.md) and
[Iterator vs. `_into`](../performance/iterator-vs-into.md).

## Unicode and language notes

- **Keys are UTF-16 code units.** See
  [UTF-16 code-unit keying](#utf-16-code-unit-keying). BMP characters — all of Latin, Greek,
  Cyrillic, Hebrew, Arabic, and the common CJK blocks — are one code unit and so
  one node. Emoji, historic scripts, and mathematical alphanumerics are two.
- **Iteration reassembles surrogate pairs**, so `keys()` yields well-formed
  `String`s even though the tree stores halves. The only place a half escapes is
  the [`find_prefix` remainder](#the-find-prefix-surrogate-divergence).
- **Folding is `str::to_lowercase`** (with a byte-wise fast path for ASCII input,
  which reaches the same answer). It agrees with the reference's `toLowerCase` on
  every Unicode scalar value — including multi-character expansions such as
  `'İ'` → `"i̇"` and the context-sensitive Greek final sigma. Neither applies
  locale-specific Turkish or Lithuanian rules. Note that folding can *lengthen* a
  word: `'İ'` becomes two code points, so it occupies two nodes.
- **Folding is not normalization and not case-*folding* in the Unicode sense.**
  `'ß'` has no single-character uppercase, so `"straße"` and `"strasse"` remain
  different words on a case-insensitive trie. Decomposed and precomposed forms of
  the same grapheme are different words too — normalize before inserting if that
  matters.
- **Nothing is trimmed or tokenized.** Whitespace and punctuation are ordinary
  code units; `"  double  "` is a word with its spaces. Split text with
  [Tokenizers](./tokenizers.md) first.
- **No prototype hazards.** The reference uses `Object.create(null)` for its node
  maps; `"__proto__"`, `"constructor"` and `"toString"` are ordinary words in
  both implementations.

## Common mistakes

**Reading `add_string`'s `bool` backwards.** It returns `true` when the word was
**already** stored. `HashSet::insert` returns `true` when the value is new. If
you want the `HashSet` sense, negate it.

```rust
use verbora_trie::Trie;
fn main() {
    let mut trie = Trie::new();
    let inserted = !trie.add_string("word"); // note the !
    assert!(inserted);
}
```

**Expecting `contains` to match prefixes.** `contains` is exact-word. With only
`"tested"` stored, `contains("test")` is `false`; the prefix question is
`iter_keys_with_prefix("test").next().is_some()`.

```rust
use verbora_trie::Trie;
fn main() {
    let mut trie = Trie::new();
    trie.add_string("tested");
    assert!(!trie.contains("test"));
    assert!(trie.iter_keys_with_prefix("test").next().is_some());
}
```

**Assuming `get_size` counts words.** It counts **nodes**, root included.

```rust
use verbora_trie::Trie;
fn main() {
    let mut trie = Trie::new();
    trie.add_strings(["a", "ab", "abc"]);
    assert_eq!(trie.get_size(), 4); // 4 nodes
    assert_eq!(trie.keys().count(), 3); // 3 words
}
```

**Passing an upper-case prefix to `keys_with_prefix` on a case-insensitive
trie.** It silently returns nothing. See
[the preserved bug](#keyswithprefix-never-folds-case).

**Treating `Some("")` as "no match" in `find_prefix`.** If the empty string was
added, the root is a word and every total miss returns `Some("")`, not `None`.

**Sorting the output of `keys_with_prefix`.** If you sort it you have thrown away
the established order. Sort only when you *want* sorted output and do not
care about matching the reference.

**Building the whole result to check emptiness.** `keys_with_prefix(p).is_empty()`
walks the entire subtree; `iter_keys_with_prefix(p).next().is_none()` does not.

**Calling `find_prefix` when you only need offsets.** `find_prefix_lengths` is
the same walk without the string building, and it is exact across surrogate
pairs.

**Looking for `remove`.** There is none. See
[Removing words](#removing-words).

## Related

- [Choosing an API](../choosing/index.md) — the cross-crate version of the
  decision tree above.
- [Iterator vs. `_into`](../performance/iterator-vs-into.md) — why the lazy
  variants exist and when they pay.
- [Allocation](../performance/allocation.md) — what "allocation-free" means
  across Verbora.
- [Cache locality](../performance/cache-locality.md) — the arena's other
  advantage.
- [Parallelism](../performance/parallelism.md) — what you can and cannot
  parallelise, and why `verbora-trie` is one of the crates that stayed
  sequential-only.
- [Performance overview](../performance/index.md) and
  [Benchmarks](../benchmarks/index.md).
- [Tokenizers](./tokenizers.md) — produce the strings you insert.
- [Distance](./distance.md) — for fuzzy matching, which a trie cannot do.
- [Core traits](./core.md) — the shared vocabulary the rest of the workspace uses.
- [Recipes](../recipes/index.md) — end-to-end pipelines.

## API reference

Everything the crate exports:

```rust  ignore
// verbora_trie
pub struct Trie { /* private */ }
pub struct KeysWithPrefix<'t> { /* private */ }
pub struct MatchesOnPath<'t, 'a> { /* private */ }

impl Trie {
    // Construction
    pub fn new() -> Self;                                   // case-sensitive
    pub fn case_insensitive() -> Self;
    pub fn with_case_sensitivity(case_sensitive: bool) -> Self;
    pub fn is_case_sensitive(&self) -> bool;
    pub fn reserve(&mut self, additional: usize);           // nodes, not words

    // Mutation
    pub fn add_string(&mut self, string: &str) -> bool;     // true = ALREADY present
    pub fn add_strings<I>(&mut self, list: I)
    where I: IntoIterator, I::Item: AsRef<str>;

    // Query
    pub fn contains(&self, string: &str) -> bool;
    pub fn get_size(&self) -> usize;                        // nodes, O(1)

    pub fn keys_with_prefix(&self, prefix: &str) -> Vec<String>;
    pub fn iter_keys_with_prefix(&self, prefix: &str) -> KeysWithPrefix<'_>;
    pub fn keys(&self) -> KeysWithPrefix<'_>;

    pub fn find_matches_on_path<'a>(&self, search: &'a str) -> Vec<Cow<'a, str>>;
    pub fn iter_matches_on_path<'a>(&self, search: &'a str) -> MatchesOnPath<'_, 'a>;

    pub fn find_prefix<'a>(&self, search: &'a str)
        -> (Option<Cow<'a, str>>, Cow<'a, str>);
    pub fn find_prefix_lengths(&self, search: &str) -> (Option<usize>, usize);
}

// Trait implementations
impl Default for Trie;                       // = Trie::new()
impl Clone for Trie;
impl Debug for Trie;
impl PartialEq for Trie;
impl Eq for Trie;
impl<S: AsRef<str>> Extend<S> for Trie;
impl<S: AsRef<str>> FromIterator<S> for Trie;    // case-sensitive
impl<'a> IntoIterator for &'a Trie;              // Item = String, IntoIter = KeysWithPrefix<'a>

impl Iterator for KeysWithPrefix<'_>;            // Item = String
impl FusedIterator for KeysWithPrefix<'_>;
impl Debug for KeysWithPrefix<'_>;

impl<'a> Iterator for MatchesOnPath<'_, 'a>;     // Item = Cow<'a, str>
impl FusedIterator for MatchesOnPath<'_, '_>;
impl Debug for MatchesOnPath<'_, '_>;
```

No `remove`, no `clear`, no batch API, no parallel API. `Trie` is `Send + Sync`.
