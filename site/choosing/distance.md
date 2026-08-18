# Choosing a distance API

`verbora-distance` exposes eleven public functions: two utilities (`dispatch`,
`utf16_len`) and four algorithms in two or three shapes each. This page is about
picking one — by the problem you have, not by the name you remember.

For what each function does in detail, see
[String distance and similarity](../features/distance.md). For how Verbora
shapes its APIs generally, see [API shapes](./api-shapes.md).

## Start from the problem

| Your problem | Metric | Why this one |
|---|---|---|
| Spelling correction, "did you mean…?", fuzzy command lookup | `damerau_levenshtein` with `restricted: true` | typing errors are insert/delete/substitute **plus** adjacent swaps; OSA counts a swap as one mistake and, at unit costs, runs the same bit-parallel kernel family as plain `levenshtein` |
| The same, but the number must match a plain edit distance | `levenshtein` | the plain recurrence is what "edit distance" usually means, and it is the cheapest path here |
| Record linkage: people, companies, addresses | `jaro_winkler` | bounded `0..=1`, tolerant of the middle of a string, and boosted for a shared prefix — which is exactly how human names vary |
| Fixed-length codes, checksums, DNA, bit strings | `hamming_checked` | `O(n)`, no allocation on ASCII, and an `Option` instead of a magic number |
| Long strings where shared content matters more than order | `dice_coefficient` | bigram *set* overlap; `O(n + m)` expected rather than `O(nm)`, and insensitive to reordering |
| "Where in this document does this phrase roughly appear?" | `levenshtein_search` / `damerau_levenshtein_search` | returns the matched substring, its distance and its start offset |
| Generic code that takes the metric as a parameter | the `StringMetric` markers | `IS_SIMILARITY` tells the caller which direction "better" is |

## Decision table

| What you are comparing | Call |
|---|---|
| Two strings guaranteed to be the same length | `hamming_checked()` → `Option<u64>`, `None` on mismatch. (`hamming()` is the same computation, returning `-1`.) |
| A query against candidate spellings, where a swap ("teh"/"the") costs one edit | `damerau_levenshtein(.., Options { restricted: true, .. })` |
| The same, but a swap is honestly two edits | `levenshtein()` |
| Two names or records that may differ anywhere, with the usual prefix boost | `jaro_winkler()` |
| The same, but you want the raw score to boost yourself | `jaro()` |
| Two longer texts where word order is not meaningful | `dice_coefficient()` — guard `NaN` on two empty inputs |
| A short needle inside a long haystack | `levenshtein_search()` / `damerau_levenshtein_search()` — check the memory cost first: the full matrix is `(n+1)(m+1)` cells at 16 bytes each |

## Typo correction: Levenshtein or Damerau?

Both are `O(nm)` in time, and at unit costs both plain and restricted run
bit-parallel kernels that handle 64 cells per word. The choice is about what a
transposition costs.

| | `levenshtein` | `damerau_levenshtein`<br>`restricted: true` | `damerau_levenshtein`<br>`restricted: false` |
|---|:--:|:--:|:--:|
| "ab" → "ba" | 2.0 | 1.0 | 1.0 |
| "ca" → "abc" | 3.0 | 3.0 | 2.0 |
| Working set | bit-vector (unit cost) or 1 row (weighted) | bit-vector (unit cost) or 3 rows (weighted) | 2 rows + per-symbol row snapshots |
| Is a true metric | yes | no (triangle inequality can fail) | no (its recurrence is not symmetric) |
| Measured, 64 units | 166.1 ns | 179.4 ns | 7.75 µs |

Plain and restricted are within a few percent of each other. The unrestricted
variant is on a different curve — its transposition can reach an arbitrary
earlier row, so it sweeps cell by cell rather than word by word. That is why it
costs roughly 40× the other two at 64 units, and the gap widens with input
length. The numbers come from the
[`levenshtein_variants`](../benchmarks/distance.md) group on a shared
64-character pair.

**Default to `restricted: true`** for user-facing typo tolerance. Take
`restricted: false` when a transposition should cost one operation even with
edits in between.

<div class="callout callout-warn">
<strong>Do not build a metric-space index on either Damerau variant.</strong>
Restricted OSA can violate the triangle inequality, and the unrestricted variant
follows a recurrence that is not symmetric (<code>"bb"→"abbb"</code> is 1, where
textbook Damerau–Levenshtein gives 2). If you are feeding distances into a
BK-tree or anything else that assumes metric axioms, use plain
<code>levenshtein</code>.
</div>

## Record linkage: Jaro–Winkler or Dice?

Both return `0.0..=1.0` with higher meaning closer, so they are interchangeable
in a ranking function's *shape*. They are not interchangeable in behaviour.

| | `jaro_winkler` | `dice_coefficient` |
|---|---|---|
| Compares | positions, within a sliding window | the set of adjacent bigrams |
| Sensitive to word order | yes | no |
| Rewards a shared prefix | yes, up to `+0.4` | no |
| Case sensitivity | opt-in via `ignore_case` | always folded |
| Whitespace | significant | runs collapsed, ends trimmed |
| Repeated content | counted | collapses (`"aaaa"` ≡ `"aa"`) |
| Complexity | `O(nm)` | `O(n + m)` expected |
| Degenerate input | `0.0` when either side is empty | `NaN` when **both** are empty |
| Measured, 1024 units | 10.34 µs | 10.61 µs |

Use `jaro_winkler` for short, ordered records — personal names above all. Use
`dice_coefficient` for longer strings, and for titles and descriptions where
word order varies. **Choose on behaviour, not speed:** the two are effectively
tied at 1024 units.

<div class="callout callout-warn">
<strong>Careful.</strong> <code>dice_coefficient("", "")</code> is
<code>NaN</code> and <code>hamming</code> returns <code>-1</code>. Both survive
into a ranking and both sort wrongly. Filter before you sort — see
<a href="../features/distance">String distance and similarity</a>.
</div>

## Fixed-length codes: `hamming` or `hamming_checked`?

Identical computation; only the failure shape differs.

| | `hamming` | `hamming_checked` |
|---|---|---|
| Returns | `i64` | `Option<u64>` |
| Length mismatch | `INCOMPARABLE` (`-1`) | `None` |
| Sorts correctly out of the box | ❌ — `-1` is below every real distance | ✅ — `None` filters out |

**Use `hamming_checked` unless you are handing the number to something that
expects the `-1` sentinel.**

```rust
use verbora_distance::hamming_checked;

fn main() {
    let candidates = ["kathrin", "kadolin", "short"];
    let mut scored: Vec<(&str, u64)> = candidates
        .iter()
        .filter_map(|c| hamming_checked("karolin", c, false).map(|d| (*c, d)))
        .collect();
    scored.sort_by_key(|(_, d)| *d);

    // "short" was dropped rather than sorted to the front with -1.
    assert_eq!(scored[0].0, "kadolin");
    assert_eq!(scored.len(), 2);
}
```

"Equal length" means equal **UTF-16 code-unit** length:
`hamming("a😀b", "abcd", false)` is comparable (both are 4 units), while
`hamming("a😀b", "ab", false)` is not.

## Scalar or search?

| | `levenshtein` / `damerau_levenshtein` | `levenshtein_search` / `damerau_levenshtein_search` |
|---|---|---|
| Question | how far apart are these two strings? | where in the target does the source best occur? |
| Returns | `f64` | `SearchResult { substring, distance, offset }` |
| Row 0 of the matrix | costs accumulate | free — every prefix is a valid start |
| Working set | bit-vector, 2–3 rows, or rows + per-symbol snapshots — never the full matrix | always the full matrix |
| Extra allocation | none beyond the working set above | one `String` for the matched text |
| Measured, 64 units | 166.1 ns (plain) | 12.79 µs |

Search is not "distance plus a bonus" — it answers a different question, and its
answer to *"how far apart are these?"* is not the same number.
`levenshtein("ca", "abc")` is 3.0; `levenshtein_search("ca", "abc").distance` is
1.0, because the search is free to ignore the unmatched prefix of the target.

Two constraints before reaching for it:

1. **Memory is `O(nm)`, not `O(min(n, m))`.** A 10-unit needle in a
   100,000-unit haystack costs `11 × 100,001` cells at 16 bytes — about 17 MB per
   call. Chunk long targets yourself, with an overlap of at least the needle
   length, if you need to search a large document.
2. **The result is a substring, not a token.** It is whatever range of the target
   is cheapest, which is frequently a fragment of a word. If you need word
   boundaries, tokenize first and score the tokens.

## Free function or `StringMetric`?

| | Free function | `StringMetric` marker |
|---|---|---|
| Call | `levenshtein(a, b, &opts)` | `Levenshtein(opts).measure(a, b)` |
| Metric known at the call site | ✅ prefer this | overkill |
| Metric is a generic parameter or a config value | awkward | ✅ prefer this |
| Direction is discoverable | you must know it | `M::IS_SIMILARITY` |
| Return type | metric-specific (`f64`, `i64`, `SearchResult`) | always `f64` |
| Covers | all nine metrics | five: `Levenshtein`, `DamerauLevenshtein`, `JaroWinkler`, `Dice`, `Hamming` |

The markers are zero-cost wrappers: `measure` calls straight through to the free
function. Use them when the *choice of metric* is data — a CLI flag, a config
field, a type parameter — and the free functions everywhere else.

```rust
use verbora_core::StringMetric;
use verbora_distance::{JaroWinkler, Levenshtein};

/// Picks the closest candidate, whichever convention `M` uses.
fn best<'a, M: StringMetric>(metric: &M, query: &str, candidates: &[&'a str]) -> Option<&'a str> {
    candidates
        .iter()
        .copied()
        .map(|c| (c, metric.measure(query, c)))
        .filter(|(_, score)| !score.is_nan())
        .reduce(|a, b| {
            let better = if M::IS_SIMILARITY { b.1 > a.1 } else { b.1 < a.1 };
            if better { b } else { a }
        })
        .map(|(c, _)| c)
}

fn main() {
    let candidates = ["kitten", "mitten", "sitting"];
    assert_eq!(best(&Levenshtein::default(), "sittin", &candidates), Some("sitting"));
    assert_eq!(best(&JaroWinkler::default(), "sittin", &candidates), Some("sitting"));
}
```

Note the two things this generic function has to do that a monomorphic one does
not: branch on `IS_SIMILARITY`, and filter `NaN`. The trait has no "no answer"
representation, so `Hamming`'s `-1.0` and `Dice`'s `NaN` come through as ordinary
`f64`s.

## Repeated and bulk comparison

<div class="callout callout-warn">
<strong>There is no scratch-buffer or sequential batch API in this crate.</strong>
No <code>levenshtein_with_scratch</code>, no <code>*_into</code> variant, no
plain <code>*_batch</code>. Behind the optional <code>parallel</code> Cargo
feature there is one <code>par_*_batch</code> function per metric —
<code>par_levenshtein_batch</code>, <code>par_damerau_levenshtein_batch</code>,
<code>par_jaro_winkler_batch</code>, <code>par_hamming_batch</code> and
<code>par_dice_coefficient_batch</code> — each a thin
<code>pairs.par_iter().map(&lt;the sequential function&gt;).collect()</code>
fan-out. Every call to a scalar function builds its own working state and drops
it, so scanning a 100,000-entry corpus repeats that setup 100,000 times.
</div>

Four things you can do at the call site.

### 1. Hoist `Options` out of the loop

`Options` is `Copy`, so this is about clarity more than cost, but it stops you
rebuilding a struct per candidate.

```rust
use verbora_distance::levenshtein::{Options, levenshtein};

fn main() {
    let opts = Options::default(); // built once, borrowed for every call
    let query = "sittin";
    let corpus = ["kitten", "mitten", "sitting", "bitten"];

    let best = corpus
        .iter()
        .map(|c| (*c, levenshtein(query, c, &opts)))
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(c, _)| c);

    assert_eq!(best, Some("sitting"));
}
```

`total_cmp` rather than `partial_cmp().unwrap()`: it is total, so it cannot
panic, and it gives `NaN` a defined position instead of a surprise.

### 2. Gate on length before paying for the matrix

With unit insertion and deletion costs, the edit distance is at least the
difference in length. `utf16_len` computes UTF-16 code-unit length without
allocating, so the gate is far cheaper than the comparison it skips — and it
cannot discard a real match.

```rust
use verbora_distance::levenshtein::{Options, levenshtein};
use verbora_distance::units::utf16_len;

fn main() {
    let opts = Options::default();
    let query = "sittin";
    let max_edits = 2.0;
    let n = utf16_len(query);

    let survivors: Vec<&str> = ["sitting", "a much longer phrase entirely", "mitten"]
        .into_iter()
        .filter(|c| utf16_len(c).abs_diff(n) as f64 <= max_edits)
        .filter(|c| levenshtein(query, c, &opts) <= max_edits)
        .collect();

    assert_eq!(survivors, ["sitting", "mitten"]);
}
```

With non-unit costs the bound becomes `min(insertion_cost, deletion_cost) ×
|n − m|`; adjust the gate accordingly or it will start discarding matches.

### 3. Pick the cheaper argument order

The row buffers are sized from the **target**, at `len(target) + 1` elements.
With the default symmetric costs the answer does not depend on argument order,
so passing the shorter string as `target` allocates less:

```rust
use verbora_distance::levenshtein::{Options, levenshtein};

fn main() {
    let opts = Options::default();
    assert_eq!(
        levenshtein("kitten", "sitting", &opts),
        levenshtein("sitting", "kitten", &opts)
    );

    // As soon as the costs are asymmetric, swapping asks a different question.
    let asymmetric = Options { deletion_cost: 3.0, ..Options::default() };
    assert_ne!(
        levenshtein("abc", "ab", &asymmetric),
        levenshtein("ab", "abc", &asymmetric)
    );
}
```

### 4. Parallelise — the crate's own batch function first

Every function here is pure and stateless: `&str` arguments and a `&Options`, no
interior mutability, no globals. `Options` is `Copy + Send + Sync`, so one
instance can be shared by reference across threads. That is what the crate's
`parallel` feature exploits, per metric:

```toml
[dependencies]
verbora-distance = { version = "0.1", features = ["parallel"] }
```

```rust  ignore
// Requires verbora-distance's `parallel` feature.
use verbora_distance::levenshtein::{Options, par_levenshtein_batch};

fn main() {
    let opts = Options::default();
    let pairs = [("kitten", "sitting"), ("mitten", "sitting")];
    let scores = par_levenshtein_batch(&pairs, &opts);
    assert_eq!(scores.len(), 2);
}
```

That covers "distance for every pair in a batch". It does not cover a custom
reduction like "the single nearest candidate" — for that, parallelise at your own
call site. The block below is marked `ignore` because the documentation's example
crate does not depend on `rayon`; add `rayon = "1"` to your own `Cargo.toml` and
it compiles as written:

```rust  ignore
use verbora_distance::levenshtein::{Options, levenshtein};
use rayon::prelude::*;

fn nearest<'a>(query: &str, corpus: &[&'a str]) -> Option<&'a str> {
    let opts = Options::default();   // shared by reference across threads

    corpus
        .par_iter()
        .map(|c| (*c, levenshtein(query, c, &opts)))
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(c, _)| c)
}
```

<div class="callout callout-warn">
<strong>Three things none of this fixes.</strong> Parallelism does not remove
the per-call setup, only overlaps it, and allocator contention is the usual
reason a naive <code>par_iter</code> over a cheap kernel scales poorly. If
several candidates tie for best, a parallel reduction need not pick the same one
a sequential scan would — break ties explicitly when that matters. And nothing
here turns an <code>O(nm)</code> per-pair metric into an index: at corpus scale
the fix is a length- or prefix-bucketed candidate set, a phonetic key from
<a href="../features/phonetics">verbora-phonetics</a> as a blocking function, or
a BK-tree over <code>levenshtein</code> (a true metric — not over either
<code>damerau_levenshtein</code> variant).
</div>

[Parallelism](../performance/parallelism.md) covers the workspace-wide position:
outside the curated `par_*_batch` functions, Verbora ships no parallel entry
point, and expects `rayon` to live at the application boundary.

## Cost at a glance

Per single call. `n` and `m` are UTF-16 code-unit lengths; the measured column is
a Criterion median on a 64-unit ASCII pair, from
[Benchmarks: distance](../benchmarks/distance.md).

| API | Time | Allocations (ASCII) | Measured, 64 units |
|---|---|---|--:|
| `hamming`, `hamming_checked` | `O(n)` | none | 20.5 ns |
| `jaro`, `jaro_winkler` | `O(nm)`, bit-parallel above 16 units | none at ≤ 128 units | 130.7 ns |
| `levenshtein` | `O(nm)`, bit-vector `O(nm/64)` for unit cost (the common case) | none beyond the kernel's character-mask tables (bit-vector) or 2 `Vec<f64>` (weighted fallback) | 166.1 ns |
| `damerau_levenshtein`, `restricted: true` | `O(nm)`, bit-vector `O(nm/64)` for unit cost | character-mask tables (bit-vector) or 3 `Vec<f64>` (weighted fallback) | 179.4 ns |
| `dice_coefficient` | `O(n + m)` expected | 2 `String`, 2 `Vec<u16>`, 2 hash sets | 1.00 µs |
| `damerau_levenshtein`, `restricted: false` | `O(nm)` | 2 rows + per-symbol row snapshots | 7.75 µs |
| `levenshtein_search`, `damerau_levenshtein_search` | `O(nm)` | full matrix + one `String` | 12.79 µs |

Cost is strongly size- and script-dependent: `levenshtein` runs 14.7 ns on a
4-unit ASCII pair and 29.08 µs on a 1024-unit one, and a Cyrillic pair pays for
the per-call promotion to UTF-16 on top. The full result tables, the hardware and
the methodology are in [Benchmarks: distance](../benchmarks/distance.md) and
[Performance](../performance/index.md).

## Related

- [Choosing an API](./index.md) — the same question for the other subsystems.
- [API shapes](./api-shapes.md) — the naming and shape conventions these
  functions follow (and where distance departs from them).
- [String distance and similarity](../features/distance.md) — the full feature
  page.
- [Allocation](../performance/allocation.md) ·
  [Parallelism](../performance/parallelism.md) ·
  [Zero-copy](../performance/zero-copy.md)
- [Benchmarks](../benchmarks/index.md) ·
  [Benchmarks: distance](../benchmarks/distance.md)
- [Recipes](../recipes/index.md)
