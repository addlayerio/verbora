# String distance and similarity

`verbora-distance` answers one question in seven ways: *how far apart are these
two strings?* It is tested against the reference distance metrics — Levenshtein and
Damerau–Levenshtein (each in a scalar and a substring-search flavour), Jaro,
Jaro–Winkler, the Sørensen–Dice coefficient, and Hamming — with the reference
implementation's exact results, including the ones that are arguably wrong.
It is the only Verbora subsystem with published, paired benchmark numbers
against the reference.

<div class="callout callout-spec">
<strong>Specification status.</strong> All <strong>7</strong> distance APIs are
documented and test-pinned across 53 option matrices, including the deliberate
edge cases this page describes: <code>NaN</code>, the <code>-1</code> sentinel
and negative search offsets. <code>cargo test -p verbora-distance</code> runs
<strong>35</strong> unit tests and <strong>1</strong> doctest.
</div>

## When to use it

- **Fuzzy matching and typo correction.** Ranking candidate spellings, command
  names, or dictionary entries against a possibly-misspelled query.
- **Record linkage and deduplication.** Deciding whether "Jonathon Smith" and
  "Jonathan Smyth" are the same person.
- **Approximate substring location.** Finding where in a longer document a short
  phrase most nearly occurs (`levenshtein_search`).
- **Fixed-width code comparison.** Counting differing positions in equal-length
  identifiers, checksums or binary strings (`hamming`).
- **Porting a reference codebase.** Every result matches, so a migration cannot
  silently change what your application ranks first.

## When not to use it

- **Semantic similarity.** These are all *surface* metrics. "car" and
  "automobile" are maximally distant under every one of them.
- **Whole-document comparison at scale.** Levenshtein is `O(nm)` in time; search
  mode is also `O(nm)` in *memory*. A 10-unit needle against a 100,000-unit
  haystack allocates a matrix of `11 × 100,001` cells at 16 bytes each — about
  17 MB — for a single call.
- **Indexed search over a large corpus.** Verbora ships no index and no
  BK-tree. There is a `parallel`-gated batch fan-out — `par_levenshtein_batch`
  and four siblings, one per metric, see "Parallel batch" below — for scoring
  many pairs you already know about, but it is not an index: every pair still
  costs a full comparison, so an unindexed nearest-neighbour search over a
  large corpus is still `O(corpus size)` per query. See
  [Choosing a distance API](../choosing/distance.md#repeated-and-bulk-comparison)
  for what to do about that.
- **Sound-alike matching.** For "Smith" ≈ "Smyth" as *pronunciation* rather than
  as spelling, reach for [phonetics](./phonetics.md) first and use a distance
  metric to break ties.

## Quick example

```rust
use verbora_distance::{dice_coefficient, hamming, jaro_winkler, levenshtein};

fn main() {
    // Distances: lower is closer.
    assert_eq!(levenshtein("kitten", "sitting", &Default::default()), 3.0);
    assert_eq!(hamming("karolin", "kathrin", false), 3);

    // Similarities: higher is closer.
    assert_eq!(dice_coefficient("abc", "abc"), 1.0);
    assert_eq!(jaro_winkler("abc", "abc", &Default::default()), 1.0);
}
```

## Direction and range differ per metric

This is the first thing to internalise, and the most common source of a silently
inverted ranking. The reference is not internally consistent about direction, and
this port does not "fix" it — doing so would change every existing caller's
results.

| Metric | Return type | Range | Direction | Identical inputs |
|---|---|---|---|---|
| `levenshtein` | `f64` | `0..` (unbounded) | **distance** — lower is closer | `0.0` |
| `damerau_levenshtein` | `f64` | `0..` (unbounded) | **distance** — lower is closer | `0.0` |
| `hamming` | `i64` | `-1`, then `0..` | **distance** — lower is closer | `0` |
| `jaro` | `f64` | `0.0..=1.0` | **similarity** — higher is closer | `1.0` |
| `jaro_winkler` | `f64` | `0.0..=1.0` | **similarity** — higher is closer | `1.0` |
| `dice_coefficient` | `f64` | `0.0..=1.0`, or `NaN` | **similarity** — higher is closer | `1.0` |

<div class="callout callout-warn">
<strong>Careful.</strong> Three traps hide in that table.
<code>hamming</code> returns <code>-1</code> — <em>lower than any real
distance</em> — for length-mismatched input, so a naive
<code>sort_by_key</code> puts the incomparable pairs first.
<code>dice_coefficient("", "")</code> is <code>NaN</code>, which loses every
comparison and can corrupt a <code>min_by</code>/<code>max_by</code> reduction.
And mixing a distance with a similarity in one ranking function inverts it
silently. The <a href="#the-stringmetric-trait">
<code>StringMetric</code></a> trait exists to make the direction explicit in
generic code.
</div>

## Choosing the right API

### Comparison table

| API | Answers | Returns | Cost (time) | Working set | Allocation-free |
|---|---|---|---|---|:--:|
| `levenshtein` | how many single-character edits? | `f64` | `O(nm)` | bit-vector (unit costs) / 2 rows | ❌ |
| `damerau_levenshtein` (`restricted: true`) | …counting an adjacent swap as one edit, with nothing edited between two swaps | `f64` | `O(nm)` | bit-vector (unit costs) / 3 rows | ❌ |
| `damerau_levenshtein` (`restricted: false`) | …counting a swap as one edit however far apart the pair ends up | `f64` | `O(nm)` | 2 rows + per-symbol snapshots | ❌ |
| `levenshtein_search` | *where* in the target does the source best occur? | `SearchResult` | `O(nm)` | full matrix | ❌ |
| `damerau_levenshtein_search` | same, tolerating transpositions | `SearchResult` | `O(nm)` | full matrix | ❌ |
| `jaro` | how many characters match within a sliding window? | `f64` | `O(nm)` | bit-parallel match masks (scalar flag arrays ≤ 16 units) | ✅ (≤ 128 units, ASCII) |
| `jaro_winkler` | …with a bonus for a shared prefix | `f64` | `O(nm)` | bit-parallel match masks (scalar flag arrays ≤ 16 units) | ✅ (≤ 128 units, ASCII, no `ignore_case`) |
| `dice_coefficient` | how much do the bigram *sets* overlap? | `f64` | `O(n + m)` expected | 2 hash sets | ❌ |
| `hamming` | how many positions differ? | `i64` | `O(n)` | none | ✅ (ASCII, no `ignore_case`) |
| `hamming_checked` | same, as an `Option` | `Option<u64>` | `O(n)` | none | ✅ (ASCII, no `ignore_case`) |

`n` and `m` are lengths **in UTF-16 code units**, not bytes or `char`s. See
[Unicode and language notes](#unicode-and-language-notes).

### Decision tree

```text
I need to compare two strings
│
├── They are the same length by construction (codes, hashes, fixed fields)
│      ├── I want a plain number, -1 meaning "incomparable"
│      │      └── hamming()
│      └── I want Rust's own vocabulary for "no answer"
│             └── hamming_checked()  ->  Option<u64>
│
├── I care about typos: insert / delete / substitute
│      ├── ...and adjacent swaps ("teh" -> "the") should cost 1, not 2
│      │      ├── swapped characters are never edited again
│      │      │      └── damerau_levenshtein(.., restricted: true)   [bit-vector / 3 rows]
│      │      └── swaps may be arbitrarily far apart
│      │             └── damerau_levenshtein(.., restricted: false)  [2 rows + snapshots]
│      └── ...and a swap is honestly two edits
│             └── levenshtein()                                      [bit-vector / 2 rows]
│
├── I need the position of the best approximate occurrence in a longer string
│      └── levenshtein_search() / damerau_levenshtein_search()       [matrix]
│
├── I am matching names or short records, and a shared prefix is meaningful
│      ├── I want the raw Jaro score
│      │      └── jaro()
│      └── I want the prefix-boosted score (the usual choice)
│             └── jaro_winkler()
│
└── I care about shared content, not order or position
       └── dice_coefficient()      (bigram set overlap; NaN on two empties)
```

### `levenshtein`

<div class="perf">
<div class="perf-row"><span class="perf-k">Execution</span><span class="perf-v">Eager</span></div>
<div class="perf-row"><span class="perf-k">Output</span><span class="perf-v">Owned <code>f64</code></span></div>
<div class="perf-row"><span class="perf-k">Allocations</span><span class="perf-v">Two <code>Vec&lt;f64&gt;</code> of <code>len(target) + 1</code>; two <code>Vec&lt;u16&gt;</code> more if either operand is non-ASCII</span></div>
<div class="perf-row"><span class="perf-k">Buffer reuse</span><span class="perf-v">None — no scratch API</span></div>
<div class="perf-row"><span class="perf-k">Batch</span><span class="perf-v">Only via <code>par_levenshtein_batch</code> (feature <code>parallel</code>)</span></div>
<div class="perf-row"><span class="perf-k">Parallel</span><span class="perf-v">Yes — <code>par_levenshtein_batch</code>, feature <code>parallel</code>, per pair; see below</span></div>
<div class="perf-row"><span class="perf-k">Best for</span><span class="perf-v">Typo distance between one pair of words or short strings</span></div>
</div>

```rust
use verbora_distance::levenshtein::{Options, levenshtein};

fn main() {
    assert_eq!(levenshtein("kitten", "sitting", &Options::default()), 3.0);
    assert_eq!(levenshtein("", "abc", &Options::default()), 3.0);
    assert_eq!(levenshtein("same", "same", &Options::default()), 0.0);
}
```

Because a cell in the plain recurrence needs only the values above, left and
diagonally up-left, this path never materialises the matrix: it keeps two
rows and swaps them. That is still the working set for weighted
(non-default) costs. With the default unit costs, `levenshtein` instead
takes a Myers/Hyyrö bit-vector fast path at every length — one `u64` word
for operands up to 64 units, a block extension beyond that — which is now
the single largest source of the
measured speedup over the reference. Either way the reference allocates a
cell object per matrix position regardless of what the caller asked for. See
[Measured against the reference](#measured-against-the-reference) below.

<div class="callout callout-note">
<strong>Note.</strong> <code>levenshtein</code> reads only three of the five
<code>Options</code> fields. <code>transposition_cost</code> and
<code>restricted</code> are inspected by the Damerau functions and ignored here —
setting them on a call to <code>levenshtein</code> does nothing at all.
</div>

### `damerau_levenshtein`

<div class="perf">
<div class="perf-row"><span class="perf-k">Execution</span><span class="perf-v">Eager</span></div>
<div class="perf-row"><span class="perf-k">Output</span><span class="perf-v">Owned <code>f64</code></span></div>
<div class="perf-row"><span class="perf-k">Allocations</span><span class="perf-v"><code>restricted: true</code> — bit-parallel kernel scratch at unit costs; three <code>Vec&lt;f64&gt;</code> of <code>len(target) + 1</code> at weighted costs. <code>restricted: false</code> — two integer rows plus a per-symbol snapshot arena (<code>u16</code> cells while the combined length fits, <code>u32</code> beyond); no full matrix in distance mode</span></div>
<div class="perf-row"><span class="perf-k">Buffer reuse</span><span class="perf-v">None — no scratch API</span></div>
<div class="perf-row"><span class="perf-k">Batch</span><span class="perf-v">Only via <code>par_damerau_levenshtein_batch</code> (feature <code>parallel</code>)</span></div>
<div class="perf-row"><span class="perf-k">Parallel</span><span class="perf-v">Yes — <code>par_damerau_levenshtein_batch</code>, feature <code>parallel</code>, per pair; see below</span></div>
<div class="perf-row"><span class="perf-k">Best for</span><span class="perf-v">Keyboard typos, where transposed letters are one mistake</span></div>
</div>

```rust
use verbora_distance::levenshtein::{Options, damerau_levenshtein, levenshtein};

fn main() {
    // A swap is two edits under Levenshtein, one under Damerau.
    assert_eq!(levenshtein("ab", "ba", &Options::default()), 2.0);
    assert_eq!(damerau_levenshtein("ab", "ba", &Options::default()), 1.0);
}
```

`restricted` picks between two genuinely different algorithms:

| `restricted` | Rule | Working set | Classic name |
|---|---|---|---|
| `false` (default) | a transposition may reach back to *any* earlier row; the substring between two swapped characters may itself be edited | full matrix | unrestricted Damerau–Levenshtein |
| `true` | a transposition may only reach row − 2, so no substring is edited between two transpositions | 3 rows | optimal string alignment (OSA) |

They disagree on real input:

```rust
use verbora_distance::levenshtein::{Options, damerau_levenshtein};

fn main() {
    let unrestricted = Options { restricted: false, ..Options::default() };
    let restricted = Options { restricted: true, ..Options::default() };

    assert_eq!(damerau_levenshtein("ca", "abc", &unrestricted), 2.0);
    assert_eq!(damerau_levenshtein("ca", "abc", &restricted), 3.0);
}
```

Prefer `restricted: true` unless you need the true metric: at the default
unit costs it runs on a bit-parallel kernel (179.4 ns against 7.75 µs at
64 characters in the measured suite), at weighted costs it is still the
cheaper structure, and it is what most "Damerau–Levenshtein"
implementations elsewhere actually compute.

### `levenshtein_search` and `damerau_levenshtein_search`

<div class="perf">
<div class="perf-row"><span class="perf-k">Execution</span><span class="perf-v">Eager</span></div>
<div class="perf-row"><span class="perf-k">Output</span><span class="perf-v">Owned <code>SearchResult</code>, carrying a <code>String</code></span></div>
<div class="perf-row"><span class="perf-k">Allocations</span><span class="perf-v">Full matrix: one <code>Vec&lt;f64&gt;</code> + one <code>Vec&lt;(u32, u32)&gt;</code> of <code>(n+1)(m+1)</code> cells (16 bytes/cell), plus the result <code>String</code></span></div>
<div class="perf-row"><span class="perf-k">Buffer reuse</span><span class="perf-v">None — no scratch API</span></div>
<div class="perf-row"><span class="perf-k">Batch</span><span class="perf-v">No</span></div>
<div class="perf-row"><span class="perf-k">Parallel</span><span class="perf-v">No built-in — the <code>parallel</code> feature wraps only <code>levenshtein</code>/<code>damerau_levenshtein</code>, not the search variants; fan out yourself, see below</span></div>
<div class="perf-row"><span class="perf-k">Best for</span><span class="perf-v">Locating a short phrase inside a bounded-size target</span></div>
</div>

Search mode makes every prefix of the target a free starting point (row 0 costs
nothing), takes the minimum over the last row, and then walks the parent
back-pointers to recover where the match began. That backtrace is why the parent
array — and therefore the whole matrix — has to exist.

```rust
use verbora_distance::levenshtein::{Options, levenshtein_search};

fn main() {
    let r = levenshtein_search("ca", "abc", &Options::default());
    assert_eq!(r.substring, "a");
    assert_eq!(r.distance, 1.0);
    assert_eq!(r.offset, 0);
}
```

<div class="callout callout-warn">
<strong>Careful.</strong> Search returns the cheapest <em>substring</em>, which
need not be a word or anything a human would have picked. Searching for
<code>"quikc"</code> in <code>"the quick brown fox"</code> returns
<code>"quic"</code> at distance 1 — deleting the stray <code>k</code> is cheaper
than the two substitutions that would reach <code>"quick"</code>, and ties are
broken in favour of the leftmost end column.
</div>

```rust
use verbora_distance::levenshtein::{Options, levenshtein_search};

fn main() {
    let r = levenshtein_search("quikc", "the quick brown fox", &Options::default());
    assert_eq!(r.substring, "quic");
    assert_eq!(r.distance, 1.0);
    assert_eq!(r.offset, 4);
}
```

Tie-breaking is reproduced deliberately. The reference picks the cheapest
predecessor with underscore's `_.min`, whose comparison is a strict `<`, so the
**first** candidate wins a tie; the candidate order is insert, delete,
substitute, unrestricted-transpose, restricted-transpose. Totals do not depend on
that order, but the recorded parent does — and the parent chain is exactly what
produces `offset` and `substring`.

### `jaro` and `jaro_winkler`

<div class="perf">
<div class="perf-row"><span class="perf-k">Execution</span><span class="perf-v">Eager</span></div>
<div class="perf-row"><span class="perf-k">Output</span><span class="perf-v">Owned <code>f64</code></span></div>
<div class="perf-row"><span class="perf-k">Allocations</span><span class="perf-v">None for ASCII operands of ≤ 128 units; otherwise one <code>Vec&lt;bool&gt;</code> per over-long operand, two <code>Vec&lt;u16&gt;</code> if non-ASCII, and two <code>String</code>s if <code>ignore_case</code></span></div>
<div class="perf-row"><span class="perf-k">Buffer reuse</span><span class="perf-v">None — no scratch API</span></div>
<div class="perf-row"><span class="perf-k">Batch</span><span class="perf-v">Only via <code>par_jaro_winkler_batch</code> (feature <code>parallel</code>) — wraps <code>jaro_winkler</code> only, not the unboosted <code>jaro</code></span></div>
<div class="perf-row"><span class="perf-k">Parallel</span><span class="perf-v">Yes for <code>jaro_winkler</code> — <code>par_jaro_winkler_batch</code>, feature <code>parallel</code>, per pair; no built-in for plain <code>jaro</code>; see below</span></div>
<div class="perf-row"><span class="perf-k">Best for</span><span class="perf-v">Personal names and short records, where a shared prefix matters</span></div>
</div>

`jaro` counts characters that match within a window of `floor(max(n, m) / 2) - 1`
positions, then penalises the ones that matched out of order. `jaro_winkler`
adds `0.1 × l × (1 − jaro)`, where `l` is the shared prefix length capped at 4.

```rust
use verbora_distance::jaro_winkler;

fn main() {
    let opts = Default::default();
    assert!((jaro_winkler("MARTHA", "MARHTA", &opts) - 0.9611111111111111).abs() < 1e-12);
    assert!((jaro_winkler("DWAYNE", "DUANE", &opts) - 0.84).abs() < 1e-12);
}
```

Two behaviours are worth knowing before you rely on the score:

- The match window is **signed**. For two one-character strings it is
  `floor(1/2) - 1 == -1`, so nothing can match and Jaro is `0.0` —
  `jaro("a", "b")` and `jaro("a", "a")` are both `0.0` through that path.
- `jaro_winkler` short-circuits to `1.0` when the two strings are equal
  **before** any case folding. That guard is what normally hides the previous
  point; when `ignore_case` makes the strings equal only *after* folding, it does
  not fire. See [Faithful, not flattering](#faithful-not-flattering).

### `dice_coefficient`

<div class="perf">
<div class="perf-row"><span class="perf-k">Execution</span><span class="perf-v">Eager</span></div>
<div class="perf-row"><span class="perf-k">Output</span><span class="perf-v">Owned <code>f64</code> — may be <code>NaN</code></span></div>
<div class="perf-row"><span class="perf-k">Allocations</span><span class="perf-v">Per call: two <code>String</code>s (lower-casing), two <code>Vec&lt;u16&gt;</code>, two <code>FxHashSet&lt;(u16, u16)&gt;</code></span></div>
<div class="perf-row"><span class="perf-k">Buffer reuse</span><span class="perf-v">None — no scratch API</span></div>
<div class="perf-row"><span class="perf-k">Batch</span><span class="perf-v">Only via <code>par_dice_coefficient_batch</code> (feature <code>parallel</code>)</span></div>
<div class="perf-row"><span class="perf-k">Parallel</span><span class="perf-v">Yes — <code>par_dice_coefficient_batch</code>, feature <code>parallel</code>, per pair; see below</span></div>
<div class="perf-row"><span class="perf-k">Best for</span><span class="perf-v">Longer strings where shared content matters more than order</span></div>
</div>

Dice compares the *sets* of adjacent code-unit pairs. Before that it lower-cases
each input, collapses runs of the reference `\s` whitespace to one space, and trims
— so `"Hello  World"` and `"hello world"` score 1.0. Because the bigrams form a
set, repeats collapse: `"aaaa"` and `"aa"` both reduce to `{"aa"}`.

```rust
use verbora_distance::dice_coefficient;

fn main() {
    assert_eq!(dice_coefficient("Hello  World", "hello world"), 1.0);
    assert_eq!(dice_coefficient("aaaa", "aa"), 1.0);
    assert_eq!(dice_coefficient("", "abc"), 0.0);
    assert!(dice_coefficient("", "").is_nan());
}
```

Dice is the only metric here with no ASCII fast path: it always builds
`Vec<u16>` operands, because the bigram keys are code-unit pairs. Its cost is
dominated by hashing rather than by a quadratic sweep, which used to make it
more than an order of magnitude cheaper than Jaro–Winkler at 1024 characters;
the bit-parallel Jaro kernels have since closed that gap almost exactly —
10.61 µs against 10.34 µs in the measured suite.

### `hamming` and `hamming_checked`

<div class="perf">
<div class="perf-row"><span class="perf-k">Execution</span><span class="perf-v">Eager</span></div>
<div class="perf-row"><span class="perf-k">Output</span><span class="perf-v"><code>i64</code> (<code>hamming</code>) / <code>Option&lt;u64&gt;</code> (<code>hamming_checked</code>)</span></div>
<div class="perf-row"><span class="perf-k">Allocations</span><span class="perf-v">None for ASCII operands with <code>ignore_case: false</code>; two <code>String</code>s when folding, two <code>Vec&lt;u16&gt;</code> when non-ASCII</span></div>
<div class="perf-row"><span class="perf-k">Buffer reuse</span><span class="perf-v">N/A</span></div>
<div class="perf-row"><span class="perf-k">Batch</span><span class="perf-v">Only via <code>par_hamming_batch</code> (feature <code>parallel</code>) — wraps <code>hamming</code> only, not <code>hamming_checked</code></span></div>
<div class="perf-row"><span class="perf-k">Parallel</span><span class="perf-v">Yes for <code>hamming</code> — <code>par_hamming_batch</code>, feature <code>parallel</code>, per pair; no built-in for <code>hamming_checked</code>; see below</span></div>
<div class="perf-row"><span class="perf-k">Best for</span><span class="perf-v">Equal-length codes, identifiers and bit strings</span></div>
</div>

```rust
use verbora_distance::{INCOMPARABLE, hamming, hamming_checked};

fn main() {
    assert_eq!(hamming("karolin", "kathrin", false), 3);
    assert_eq!(hamming("ABC", "abc", true), 0);

    // A length mismatch is a sentinel, not an error.
    assert_eq!(hamming("abc", "ab", false), INCOMPARABLE);
    assert_eq!(INCOMPARABLE, -1);

    // The Rust-shaped alternative.
    assert_eq!(hamming_checked("karolin", "kathrin", false), Some(3));
    assert_eq!(hamming_checked("abc", "ab", false), None);
}
```

`hamming_checked` is a Verbora addition, not a reference API. It is the same
computation with `INCOMPARABLE` mapped to `None`, so `?`, `filter_map`,
`unwrap_or` and the rest of the `Option` vocabulary apply. **Use it by default**;
reach for `hamming` when you are comparing output against the reference
reference or feeding a system that expects the `-1`.

The length check runs on the *original* strings, before any case folding — which
matters because Unicode case mapping can change length (`İ` lowercases to two
code units; `ß` uppercases to `SS`). After folding, the comparison loop is bounded
by the first string and reads past the end of the second as "no character", which
never equals anything — reproducing what the reference's `undefined` does.

### Parallel batch (`par_*_batch`, feature `parallel`)

<a class="badge badge-batch" href="../performance/batch-vs-streaming">BATCH</a>

Behind this crate's `parallel` Cargo feature (`parallel = ["dep:rayon"]`,
never on by default), five functions fan a slice of pairs out across a
`rayon` thread pool — one per metric, at **per-pair** granularity:

| Function | Wraps | Missing sibling |
|---|---|---|
| `par_levenshtein_batch` | `levenshtein` | — |
| `par_damerau_levenshtein_batch` | `damerau_levenshtein` | — |
| `par_jaro_winkler_batch` | `jaro_winkler` | no `par_jaro_batch` for the unboosted score |
| `par_dice_coefficient_batch` | `dice_coefficient` | — |
| `par_hamming_batch` | `hamming` | no batch form of `hamming_checked` |

Each is exactly `pairs.par_iter().map(<the sequential function>).collect()` —
a thin fan-out over the metric directly above it in this page, never a second
implementation (see `AGENTS.md`'s Rayon Policy). None of these wrap
`levenshtein_search` or `damerau_levenshtein_search`: apply the same
`par_iter().map(...)` pattern yourself if you need the search variants in
parallel (see [Parallelism](../performance/parallelism.md)).

```rust  ignore
use verbora_distance::levenshtein::Options;
use verbora_distance::par_levenshtein_batch;

let pairs = [("kitten", "sitting"), ("", "abc")];
let distances = par_levenshtein_batch(&pairs, &Options::default());
assert_eq!(distances, [3.0, 3.0]);
```

<div class="callout callout-note">
<strong>Note.</strong> This block needs the <code>parallel</code> feature
enabled on <code>verbora-distance</code>, which this site's own snippet
checker builds without, so it is marked <code>ignore</code> rather than
compiled — every other block on this page compiles and runs in CI.
</div>

**The crossover differs sharply per metric**, because each metric's per-pair
cost has to clear `rayon`'s roughly-one-microsecond scheduling cost before a
batch call is worth it. These are one machine's numbers, from each function's
own doc comment (32-thread machine, default global `rayon` pool) — reproduce
with `cargo bench -p verbora-distance --features parallel -- par_<name>`
before relying on them for capacity planning:

- `par_levenshtein_batch` / `par_damerau_levenshtein_batch` (300 pairs/batch):
  4-character pairs *lose* (0.6×); 64-character pairs win 15.6×; 1024-character
  pairs win 22.0×.
- `par_jaro_winkler_batch` (1000 pairs/batch): 4-character pairs lose badly
  (0.16×); 64-character pairs win 7.9×; 1024-character pairs win 15.6×.
- `par_dice_coefficient_batch` (1000 pairs/batch): wins at every length
  tested, from 2.0× at 4 characters to 10.3× at 64.
- `par_hamming_batch` (1000 pairs/batch): loses at 4–64 characters (as low as
  0.12×); only starts winning from 256 characters (1.7×) up.

`hamming` is nearly free per short pair — a handful of comparisons — so it
loses until pairs are long. `dice_coefficient` hashes two bigram sets per
call, already heavier than scheduling at every length tested, so it wins even
at four characters. `levenshtein`, `damerau_levenshtein` and `jaro_winkler`
sit in between: cheap at four characters, expensive enough by 64 to clear the
scheduling cost.

Every `par_*_batch` function preserves input order and has its own
sequential-vs-parallel equivalence test (`crates/verbora-distance/tests/parallel.rs`),
asserting identical output over the same inputs — empty, one pair, many pairs,
Unicode included.

## `levenshtein::Options`

All five fields, with the exact defaults from `Default::default()`:

| Field | Type | Default | Read by | Effect |
|---|---|--:|---|---|
| `insertion_cost` | `f64` | `1.0` | all four functions | cost of inserting one symbol |
| `deletion_cost` | `f64` | `1.0` | all four functions | cost of deleting one symbol |
| `substitution_cost` | `f64` | `1.0` | all four functions | cost of replacing one symbol with another |
| `transposition_cost` | `f64` | `1.0` | Damerau variants only | cost of swapping two adjacent symbols |
| `restricted` | `bool` | `false` | Damerau variants only | `true` selects optimal string alignment (3 rows); `false` selects unrestricted Damerau (full matrix) |

The costs are `f64` rather than `usize` because the reference accepts arbitrary
the reference numbers here — fractions and zero included — and callers do pass them.
Zero costs are legal and produce zero distances.

```rust
use verbora_distance::levenshtein::{Options, damerau_levenshtein, levenshtein};

fn main() {
    let asymmetric = Options {
        insertion_cost: 1.0,
        deletion_cost: 3.0,
        substitution_cost: 1.0,
        transposition_cost: 1.0,
        restricted: false,
    };
    // Deleting `c` costs 3; inserting it still costs 1.
    assert_eq!(levenshtein("abc", "ab", &asymmetric), 3.0);
    assert_eq!(levenshtein("ab", "abc", &asymmetric), 1.0);

    // `restricted` and `transposition_cost` are read only by the Damerau
    // functions. `levenshtein` ignores both.
    let restricted = Options { restricted: true, ..Options::default() };
    assert_eq!(
        levenshtein("ca", "abc", &restricted),
        levenshtein("ca", "abc", &Options::default())
    );
    assert_eq!(damerau_levenshtein("ca", "abc", &restricted), 3.0);
    assert_eq!(damerau_levenshtein("ca", "abc", &Options::default()), 2.0);
}
```

Fractional costs work exactly as you would expect:

```rust
use verbora_distance::levenshtein::{Options, levenshtein};

fn main() {
    let weighted = Options {
        insertion_cost: 0.5,
        deletion_cost: 1.5,
        substitution_cost: 0.75,
        ..Options::default()
    };
    assert_eq!(levenshtein("ab", "abc", &weighted), 0.5);
}
```

`Options` is `Copy`, `Clone`, `Debug` and `PartialEq` (not `Eq` — it holds
`f64`), so building one per call costs nothing but is still worth hoisting out of
a loop for readability.

<div class="callout callout-note">
<strong>Note.</strong> With the default symmetric costs,
<code>levenshtein(a, b)</code> equals <code>levenshtein(b, a)</code>. As soon as
<code>insertion_cost != deletion_cost</code> that stops being true: the
directional reading is "the cost of turning <code>source</code> into
<code>target</code>". Argument order is also what sizes the row buffers — they
are <code>len(target) + 1</code> long — so with symmetric costs, passing the
shorter string as <code>target</code> allocates less.
</div>

## `jaro_winkler::Options`

| Field | Type | Default | Effect |
|---|---|--:|---|
| `ignore_case` | `bool` | `false` | lower-cases both inputs before comparing (after the identity short-circuit, which sees the originals) |
| `dj` | `Option<f64>` | `None` | a precomputed Jaro similarity; supplying it skips the `O(nm)` matching pass entirely |

`dj` mirrors the reference `options.dj`. It is a genuine optimisation when you
already hold the Jaro score — for example when re-scoring the same pair under
several prefix policies — and a footgun otherwise, because nothing checks that
the number you supply is the Jaro similarity of these two strings.

```rust
use verbora_distance::jaro_winkler::{Options, jaro, jaro_winkler};

fn main() {
    let folding = Options { ignore_case: true, dj: None };
    assert_eq!(jaro_winkler("MARTHA", "martha", &folding), 1.0);

    // `dj` supplies the Jaro score, skipping the O(nm) matching pass.
    let precomputed = Options { ignore_case: false, dj: Some(jaro("MARTHA", "MARHTA")) };
    assert_eq!(
        jaro_winkler("MARTHA", "MARHTA", &precomputed),
        jaro_winkler("MARTHA", "MARHTA", &Options::default())
    );
}
```

`jaro` itself takes no options: it is `jaro(s1, s2) -> f64`.

## `SearchResult`, and the offset that really is negative

```rust  ignore
pub struct SearchResult {
    pub substring: String,
    pub distance: f64,
    pub offset: isize,
}
```

| Field | Meaning |
|---|---|
| `substring` | the best-matching substring of the **target**, as an owned `String` |
| `distance` | the edit distance from `source` to that substring |
| `offset` | the substring's start in the target, **in UTF-16 code units**, signed |

`offset` is `isize` and is not defensive typing. When the parent backtrace exits
through column 0, the reference implementation computes `col - 1 == -1` and
reports it. The reference then calls `target.slice(-1, end)`, which counts from the
*end* of the string rather than clamping, and the returned `substring` reflects
that. Verbora reproduces both halves verbatim:

```rust
use verbora_distance::levenshtein::{Options, damerau_levenshtein_search};

fn main() {
    let opts = Options { deletion_cost: 2.0, ..Options::default() };
    let r = damerau_levenshtein_search("ab", "ba", &opts);

    assert_eq!(r.offset, -1);       // genuinely negative, not clamped
    assert_eq!(r.substring, "a");   // the reference's "ba".slice(-1, 2)
    assert_eq!(r.distance, 1.0);
}
```

Which variants can produce it follows from the parent candidates. The backtrace
only ever steps out of a cell at column 2 or beyond, and in plain Levenshtein the
only parents are insert `(r, c-1)`, delete `(r-1, c)` and substitute
`(r-1, c-1)` — none of which can land in column 0. **`levenshtein_search` cannot
return a negative offset.** The Damerau variants can: a restricted transposition
parents to `(r-2, c-2)`, and an unrestricted one to `(lrm-1, lcm-1)`, either of
which reaches column 0 from column 2.

Two rules follow. **Prefer `substring` to re-slicing the target**: it is already
the matched text, and it was cut with the reference's slice semantics, which your
own indexing will not reproduce. If you must use `offset` as an index, remember
it is a UTF-16 index — a Rust byte index only when the target is ASCII — and
handle the negative case explicitly:

```rust
use verbora_distance::levenshtein::{Options, levenshtein_search};

fn main() {
    let target = "the quick brown fox";
    let r = levenshtein_search("quikc", target, &Options::default());

    if let Ok(units) = usize::try_from(r.offset) {
        if target.is_ascii() {
            assert_eq!(&target[units..units + r.substring.len()], "quic");
        }
    }
}
```

## The `StringMetric` trait

`verbora_core::StringMetric` is the generic seam:

```rust  ignore
pub trait StringMetric {
    const IS_SIMILARITY: bool;
    fn measure(&self, a: &str, b: &str) -> f64;
}
```

It deliberately does **not** normalise direction. `IS_SIMILARITY` records which
convention a metric uses so generic code can adapt, while every metric keeps the
output the reference produces.

`verbora-distance` provides five marker types implementing it:

| Type | `IS_SIMILARITY` | Shape | Wraps |
|---|:--:|---|---|
| `Levenshtein(pub levenshtein::Options)` | `false` | tuple struct | `levenshtein` |
| `DamerauLevenshtein(pub levenshtein::Options)` | `false` | tuple struct | `damerau_levenshtein` |
| `JaroWinkler(pub jaro_winkler::Options)` | `true` | tuple struct | `jaro_winkler` |
| `Dice` | `true` | unit struct | `dice_coefficient` |
| `Hamming { pub ignore_case: bool }` | `false` | named field | `hamming`, cast to `f64` |

All five are `Debug + Clone + Copy + Default`. There is no marker for `jaro` or
for the two search functions: call `jaro` directly when you want the unboosted
score, and search returns a `SearchResult` rather than an `f64`, which does not
fit `measure`'s signature.

**Use the marker types when the metric is a parameter**; use the free functions
when it is not. A single call site that always computes Levenshtein gains nothing
from `Levenshtein::default().measure(a, b)` over `levenshtein(a, b, &opts)` — it
is the same work behind one more layer.

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

Options travel inside the marker:

```rust
use verbora_core::StringMetric;
use verbora_distance::{DamerauLevenshtein, Hamming, levenshtein};

fn main() {
    let osa = DamerauLevenshtein(levenshtein::Options {
        restricted: true,
        ..Default::default()
    });
    assert_eq!(osa.measure("ca", "abc"), 3.0);

    // Hamming's marker carries a flag rather than an Options struct.
    let folding = Hamming { ignore_case: true };
    assert_eq!(folding.measure("ABC", "abc"), 0.0);
    assert_eq!(folding.measure("abc", "ab"), -1.0); // the sentinel survives the cast
}
```

<div class="callout callout-warn">
<strong>Careful.</strong> <code>Hamming</code>'s
<code>measure</code> returns <code>-1.0</code> for incomparable input, and
<code>Dice</code>'s returns <code>NaN</code> for two empty strings. Generic code
over <code>StringMetric</code> must handle both, because the trait has no
"no answer" representation. The <code>best()</code> example above filters
<code>NaN</code> for exactly this reason.
</div>

## Advanced usage: the `units` module

Every metric here indexes text the way the reference does, by UTF-16 code unit,
because that choice is *observable in the results*. `verbora_distance::units` is
the mechanism, and it is public so you can use the same fast path in your own
code.

### Why UTF-16 indexing is observable

The reference strings are sequences of UTF-16 code units. `s.length`, `s[i]` and
`s.slice(i, j)` all count code units, and any character outside the Basic
Multilingual Plane counts as **two**. Rust's `char` is a Unicode scalar value, so
a straightforward port disagrees with the reference on astral-plane input:

```text
LevenshteinDistance("a😀b", "ab")
  the reference : 2   (lengths 4 and 2; two surrogate halves deleted)
  Rust chars : 1   (lengths 3 and 2; one character deleted)
```

Verbora returns 2, and there are recorded fixtures asserting it:

```rust
use verbora_distance::levenshtein;
use verbora_distance::units::utf16_len;

fn main() {
    assert_eq!(utf16_len("a😀b"), 4);        // the reference's String#length
    assert_eq!("a😀b".chars().count(), 3);   // Rust's idea of length

    // The metric follows the reference: two surrogate halves are deleted.
    assert_eq!(levenshtein("a😀b", "ab", &Default::default()), 2.0);
    // BMP characters are one unit each, so this is the intuitive answer.
    assert_eq!(levenshtein("café", "cafe", &Default::default()), 1.0);
}
```

### `dispatch`: the ASCII fast path

Forcing every algorithm onto `Vec<u16>` would allocate on every call and slow
down the overwhelmingly common case. Instead each algorithm is written once,
generically over `&[T]`, and `dispatch` picks the narrowest representation that
is *provably identical* to UTF-16 for the given input:

| Input | Representation | Allocates |
|---|---|:--:|
| both operands ASCII | `&[u8]` | ❌ |
| otherwise | `Vec<u16>` (one per operand) | ✅ |

For ASCII input one byte *is* one code unit, so the fast path is not an
approximation — it is the same computation on a narrower type. The check is
`str::is_ascii`, which is vectorised. Both operands are promoted together: a
`&[u8]` view cannot be compared against a `&[u16]` view, so one non-ASCII operand
promotes the pair.

```rust
use verbora_distance::units::{Operands, dispatch};

fn main() {
    let ascii = dispatch("kitten", "sitting", |ops| matches!(ops, Operands::Bytes(..)));
    assert!(ascii);

    // One non-ASCII operand promotes the pair to UTF-16.
    let promoted = dispatch("kitten", "café", |ops| matches!(ops, Operands::Units(..)));
    assert!(promoted);
}
```

`Operands<'a>` is the two-variant enum you match on: `Bytes(&[u8], &[u8])` and
`Units(&[u16], &[u16])`.

The byte path is narrower in a second way. Unrestricted Damerau needs a map from
symbol to "last row this symbol appeared in"; the `Unit` trait picks the
representation. `u8` gets `ByteMap`, a flat `[usize; 256]` where a lookup is one
indexed load and nothing is hashed; `u16` falls back to an `FxHashMap<u16,
usize>`. `UnitMap<T>` (`get`, `set`, `clear`) is the shared interface.

### `utf16_len`

`utf16_len(s)` is the reference's `String#length` for a Rust `&str`. It returns
`s.len()` unchanged for ASCII, and otherwise counts `char::len_utf16` **without
allocating** — so it is the cheap way to ask "how long would the reference think
this is?", for example as a pre-filter before an expensive comparison.

## Performance characteristics

### Working set per mode

The reference always materialises a full `(n+1) × (m+1)` matrix of heap-allocated
cell objects, each holding a cost and a parent coordinate, even when only the
final scalar is wanted. That is `O(nm)` allocations of `O(nm)` pointer-chased
objects. This port picks the cheapest structure — and, wherever a faster
*algorithm* exists, the fastest algorithm — that can answer the
question asked:

| Mode | Working set | Why |
|---|---|---|
| distance, no Damerau, unit cost | **bit-vector** (one `u64` word per 64 units of the shorter operand) | Myers'/Hyyrö's bit-parallel algorithm answers in `O(nm/64)` bitwise operations rather than `O(nm)` scalar cell updates; the single-word path covers operands of 1–64 units, blocks beyond |
| distance, no Damerau (fallback, weighted costs) | **2 rows** | each cell needs only `up`, `left`, `diag` |
| distance, restricted Damerau, unit cost | **bit-vector** (word + block) | Hyyrö's 2003 transposition extension of Myers computes OSA in the same bit-parallel style |
| distance, restricted Damerau (fallback, weighted costs) | **3 rows** | a transposition reaches back to row − 2 |
| distance, unrestricted Damerau | **2 rows + per-symbol row snapshots** | a transposition reaches an arbitrary earlier row, so each symbol's last matching row is snapshotted into an arena (`u16` cells while the combined length fits, `u32` beyond) — the cost + parent matrices are no longer built |
| search (any variant) | full matrix | the match start is recovered by backtracking parents |

Where the full matrix *is* required — now only in search mode — it is stored
struct-of-arrays — costs in one
flat `Vec<f64>`, parents in another `Vec<(u32, u32)>` — so the hot cost sweep is
contiguous and the parents, touched only during backtracking, never pollute the
cache line during the inner loop.

### Measured against the reference

26 benchmarks, **median speedup 23.4×**, range **1.4×–3307.4×**, measured on shared
input files that both implementations read (Intel i9-14900KF, rustc 1.97.1
`--release`, Node v25.9.0 with the JIT warmed, the reference). Full
methodology in [Performance](../performance/index.md).

| Benchmark | Reference | Verbora | Speedup |
|---|--:|--:|--:|
| `levenshtein/ascii/4` | 791.0 ns | 14.7 ns | **53.8×** |
| `levenshtein/ascii/1024` | 96.18 ms | 29.08 µs | **3307.4×** |
| `levenshtein/cyrillic/256` | 3.69 ms | 3.97 µs | **929.5×** |
| `levenshtein_variants/plain_2row` | 177.17 µs | 166.1 ns | **1066.6×** |
| `levenshtein_variants/damerau_restricted_3row` | 190.07 µs | 179.4 ns | **1059.5×** |
| `levenshtein_variants/damerau_unrestricted_matrix` | 304.11 µs | 7.75 µs | **39.2×** |
| `levenshtein_variants/search_matrix` | 176.86 µs | 12.79 µs | **13.8×** |
| `jaro_winkler/4` | 27.2 ns | 15.3 ns | **1.8×** |
| `jaro_winkler/1024` | 594.77 µs | 10.34 µs | **57.5×** |
| `dice/1024` | 80.10 µs | 10.61 µs | **7.5×** |
| `hamming/4` | 9.0 ns | 6.6 ns | **1.4×** |
| `hamming/1024` | 2.16 µs | 275.3 ns | **7.8×** |

The complete 26-row table is in [Benchmarks](../benchmarks/distance.md).

Three things in that table are worth reading carefully:

- **The biggest wins now come from bit-parallel kernels, not from
  row-count reduction.** At 1024 characters and the default unit costs,
  `levenshtein` takes the Myers/Hyyrö bit-vector fast path instead of the
  two-row scalar DP (see [`levenshtein`](#levenshtein) above), trading
  `O(nm)` cell updates for `O(nm/64)` bitwise ones. Hence **3307.4×**, the
  largest gap in the suite — well past the **29.6×** the two-row reduction
  alone produced at this size before the bit-vector path existed.
  Restricted Damerau now has a bit-parallel kernel of its own (Hyyrö's 2003
  transposition extension of Myers, unit costs only), and
  unrestricted-Damerau distance calls run on a two-row snapshot kernel
  instead of the full matrix. Row-count reduction is still the story for
  weighted (non-default) costs; the full matrix survives only in search
  mode.
- **The smallest wins are at four characters** (`hamming/4` 1.4×,
  `jaro_winkler/4` 1.8×). At that size the work is a handful of comparisons, both
  runtimes are dominated by call overhead, and the reference engine optimises the shape well. Small,
  genuine wins are the honest expectation; a large reported gap here would be a
  sign of a rigged benchmark.
- **The variant benchmark names are now legacy.** `plain_2row` and
  `damerau_restricted_3row` both run bit-parallel kernels at their fixed
  64-character input — **1066.6×** and **1059.5×**, sitting together rather
  than near the other two — and `damerau_unrestricted_matrix` no longer
  builds a matrix at all (**39.2×** on the snapshot kernel). Only
  `search_matrix` still is what its name says: the full cost + parent
  matrix the backtrace requires, at **13.8×**. Choosing `restricted: true`
  over the unrestricted metric is accordingly no longer a modest saving but
  a large one at unit costs — 179.4 ns against 7.75 µs on the same input.

Both sides read their inputs from the same files in `benches/data/`, generated
once by `tools/bench-data/generate.py`, so neither can be tuned to a friendlier
distribution. The Rust side is `crates/verbora-distance/benches/distance.rs`
(Criterion); the reference side was measured with its own harness, which warms the
JIT before measuring.

```text
python3 tools/bench-data/generate.py       # shared inputs (run once)
cargo bench -p verbora-distance         # Rust, via Criterion
```

### A measured regression, and its fix

The first recorded run had `jaro_winkler/4` at **0.6×** — Rust *slower* than
the reference. The cause was two `vec![false; len]` allocations per call: the reference engine's
`new Array(4)` is nearly free, `malloc` is not.

Moving the match flags to a stack buffer for inputs up to 128 units took the
benchmark from **48.6 ns → 16.4 ns** (0.6× → 1.7×), with the test suite re-run
and still green. Words are short by nature, so the stack path is the common case
rather than a micro-optimisation for a rare one. (Today's measurement sits at
15.3 ns — 1.8×.)

That constant is why the Jaro allocation rule reads "none for ASCII operands of
≤ 128 units": above the threshold the flags go back to the heap, one `Vec<bool>`
per over-long operand.

## Allocation behaviour

No function in this crate exposes a scratch buffer, an `_into` variant, or any
form of reuse. Each call allocates what it needs and drops it. What that means
per call:

| Call | Allocations |
|---|---|
| `levenshtein` (ASCII) | two `Vec<f64>` of `len(target) + 1` |
| `damerau_levenshtein`, `restricted: true` (ASCII) | three `Vec<f64>` of `len(target) + 1` |
| `damerau_levenshtein`, `restricted: false` (ASCII) | two integer rows plus a per-symbol snapshot arena (`u16` cells while the combined length fits, `u32` beyond) — no matrix |
| `levenshtein_search` / `damerau_levenshtein_search` (ASCII) | the same full matrix, plus one `String` for `substring` |
| `jaro` / `jaro_winkler` (ASCII, ≤ 128 units, no folding) | **none** |
| `jaro` / `jaro_winkler` (either operand > 128 units) | one `Vec<bool>` per over-long operand |
| `jaro_winkler` with `ignore_case: true` | two `String`s from `to_lowercase`, plus the above |
| `dice_coefficient` (any input) | two `String`s, two `Vec<u16>`, two `FxHashSet<(u16, u16)>` |
| `hamming` / `hamming_checked` (ASCII, no folding) | **none** |
| `hamming` / `hamming_checked` with `ignore_case: true` | two `String`s from `to_lowercase` |
| any of the above with a non-ASCII operand | plus two `Vec<u16>`, one per operand |

Three details you would only find by reading the source:

- **Unrestricted Damerau in distance mode no longer allocates the matrix.** It
  used to share `full_matrix` with search mode and drag the never-read parent
  array along; distance calls now run a two-row kernel with per-symbol row
  snapshots, and the full cost + parent matrix survives only in the search
  functions.
- **`jaro_winkler` on non-ASCII input converts to UTF-16 twice** — once inside
  `jaro`, once inside the shared-prefix scan — because each calls `dispatch`
  independently.
- **`dice_coefficient` never takes the ASCII path.** Its bigram keys are
  `(u16, u16)` pairs, so `sanitize` builds a `Vec<u16>` regardless of input. It
  still avoids the reference's `String`-per-bigram allocation, which is where its
  win comes from.

For the general treatment see [Allocation](../performance/allocation.md).

## Unicode and language notes

- **Lengths and indices are UTF-16 code units** everywhere in this crate: the
  distance between `"a😀b"` and `"ab"` is 2, `hamming` compares
  `utf16_len`-equal strings, `SearchResult::offset` is a UTF-16 index, and
  Dice's bigrams are code-unit pairs. - **`substring` may contain U+FFFD.** A search slice can split a surrogate pair
  exactly as the reference's `slice` does; lossy decoding maps the lone surrogate to
  the replacement character, the closest well-formed Rust representation.
- **Case folding is `str::to_lowercase`**, which is full Unicode lowercasing and
  can change length. `hamming` compares lengths *before* folding, and
  `jaro_winkler` short-circuits on equality *before* folding, so both orderings
  are observable.
- **Dice's whitespace rule is the reference's `\s`**, not Rust's
  `char::is_whitespace`. `verbora_core::is_whitespace` is the shared
  predicate; the two sets are not interchangeable.
- **No normalisation is applied.** `"café"` composed (`e` + U+0301) and
  precomposed (U+00E9) are different strings with a non-zero distance. Normalise
  upstream if that matters.

## Faithful, not flattering

Three results in this crate look like bugs. They are, in the reference — and
reproducing them is the point, because a port that quietly "fixed" them would
change what a migrated application ranks first.

**`dice_coefficient("", "")` is `NaN`.** Two empty strings produce no bigrams, so
the reference computes `0 / 0`. Smoothing that to `0.0` or `1.0` would be a
silent disagreement with the reference. Guard it at your call site:

```rust
use verbora_distance::dice_coefficient;

fn main() {
    let score = dice_coefficient("", "");
    let ranked = if score.is_nan() { 0.0 } else { score };
    assert_eq!(ranked, 0.0);
}
```

**`hamming` returns `-1` for length-mismatched input.** The reference returns `-1`
rather than raising, so the sentinel is part of the contract — and it is *lower*
than every real distance, so it sorts to the front of an ascending ranking.
`hamming_checked` is the Rust-idiomatic escape hatch:

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

**`jaro_winkler("A", "a", ignore_case)` is `0.4`, not `1.0`.** The reference's
prefix loop is `while (s1[l] === s2[l] && l < 4) l++`, which tests before it
bounds-checks: once both strings are exhausted, `s1[l]` and `s2[l]` are both
`undefined`, and `undefined === undefined` is `true`, so the counter climbs to 4
even for a two-character input. That is usually invisible, because equal strings
score Jaro 1.0 and the boost is multiplied by `1 − 1 == 0`. For single-character
inputs the match window is `floor(1/2) − 1 == −1`, nothing can match, Jaro is 0,
and the saturated boost is fully exposed: `0 + 4 × 0.1 × (1 − 0) = 0.4`.

```rust
use verbora_distance::jaro_winkler::{Options, jaro_winkler};

fn main() {
    let folding = Options { ignore_case: true, dj: None };
    assert_eq!(jaro_winkler("A", "a", &folding), 0.4); // not 1.0
    assert_eq!(jaro_winkler("AB", "ab", &folding), 1.0);
}
```

All three are recorded in the test fixtures and asserted on every test run.

## Common mistakes

1. **Mixing directions in one ranking.** Sorting ascending is right for
   Levenshtein and Hamming and exactly backwards for Jaro–Winkler and Dice. Use
   `StringMetric::IS_SIMILARITY` when the metric is a parameter.
2. **Sorting on `hamming` without filtering `-1`.** Incomparable pairs land at
   the top. Use `hamming_checked`.
3. **Reducing over Dice scores without a `NaN` guard.** `min_by`/`max_by` with a
   partial comparison silently mis-order; `f64::total_cmp` orders `NaN` last for
   positives, which may or may not be what you meant.
4. **Treating `SearchResult::offset` as a byte index.** It is a signed UTF-16
   index. Use `substring`.
5. **Expecting search to return a word.** It returns the cheapest substring,
   which is frequently a fragment.
6. **Setting `restricted` or `transposition_cost` on `levenshtein`.** Neither is
   read; you wanted `damerau_levenshtein`.
7. **Assuming `levenshtein(a, b) == levenshtein(b, a)`.** True only while
   `insertion_cost == deletion_cost`.
8. **Assuming `char`-based intuition.** `levenshtein("a😀b", "ab")` is 2.
9. **Assuming there is no batch API at all.** There is no scratch-buffer
   (`_into`) API, but the `parallel` feature does add a per-pair batch
   fan-out — `par_levenshtein_batch` and four siblings, one per metric (see
   "Parallel batch" above). It has no sibling for `levenshtein_search`,
   `damerau_levenshtein_search`, plain `jaro` or `hamming_checked` — see
   [Choosing a distance API](../choosing/distance.md#repeated-and-bulk-comparison)
   for what to do about those.

## Related

- [Choosing a distance API](../choosing/distance.md) — which metric for which
  problem, and how to run millions of comparisons without one.
- [Core traits](./core.md) — `StringMetric` and the rest of the shared
  vocabulary.
- [Phonetics](./phonetics.md) — sound-alike keys, the usual partner for a
  distance metric in name matching.
  count.
  in full.
- [Performance](../performance/index.md) · [Allocation](../performance/allocation.md) ·
  [Parallelism](../performance/parallelism.md)
- [Benchmarks: distance](../benchmarks/distance.md) — all 26 measurements.
- [Recipes](../recipes/index.md)

## API reference

```rust  ignore
// crate root re-exports
pub use dice::dice_coefficient;
pub use hamming::{INCOMPARABLE, hamming, hamming_checked};
pub use jaro_winkler::{jaro, jaro_winkler};
pub use levenshtein::{
    SearchResult, damerau_levenshtein, damerau_levenshtein_search, levenshtein, levenshtein_search,
};
// feature = "parallel"
pub use dice::par_dice_coefficient_batch;
pub use hamming::par_hamming_batch;
pub use jaro_winkler::par_jaro_winkler_batch;
pub use levenshtein::{par_damerau_levenshtein_batch, par_levenshtein_batch};

// verbora_distance::levenshtein
pub fn levenshtein(source: &str, target: &str, opts: &Options) -> f64;
pub fn damerau_levenshtein(source: &str, target: &str, opts: &Options) -> f64;
pub fn levenshtein_search(source: &str, target: &str, opts: &Options) -> SearchResult;
pub fn damerau_levenshtein_search(source: &str, target: &str, opts: &Options) -> SearchResult;
// feature = "parallel" — one rayon::par_iter().map(...).collect() fan-out each
pub fn par_levenshtein_batch(pairs: &[(&str, &str)], opts: &Options) -> Vec<f64>;
pub fn par_damerau_levenshtein_batch(pairs: &[(&str, &str)], opts: &Options) -> Vec<f64>;

pub struct Options {           // Debug + Clone + Copy + PartialEq + Default
    pub insertion_cost: f64,      // 1.0
    pub deletion_cost: f64,       // 1.0
    pub substitution_cost: f64,   // 1.0
    pub transposition_cost: f64,  // 1.0   (Damerau only)
    pub restricted: bool,         // false (Damerau only)
}

pub struct SearchResult {      // Debug + Clone + PartialEq
    pub substring: String,
    pub distance: f64,
    pub offset: isize,
}

// verbora_distance::jaro_winkler
pub fn jaro(s1: &str, s2: &str) -> f64;
pub fn jaro_winkler(s1: &str, s2: &str, opts: &Options) -> f64;
pub fn par_jaro_winkler_batch(pairs: &[(&str, &str)], opts: &Options) -> Vec<f64>; // feature = "parallel"

pub struct Options {           // Debug + Clone + Copy + Default + PartialEq
    pub ignore_case: bool,     // false
    pub dj: Option<f64>,       // None
}

// verbora_distance::dice
pub fn dice_coefficient(s1: &str, s2: &str) -> f64;
pub fn par_dice_coefficient_batch(pairs: &[(&str, &str)]) -> Vec<f64>; // feature = "parallel"

// verbora_distance::hamming
pub const INCOMPARABLE: i64 = -1;
pub fn hamming(s1: &str, s2: &str, ignore_case: bool) -> i64;
pub fn hamming_checked(s1: &str, s2: &str, ignore_case: bool) -> Option<u64>;
pub fn par_hamming_batch(pairs: &[(&str, &str)], ignore_case: bool) -> Vec<i64>; // feature = "parallel"

// verbora_distance::units
pub fn dispatch<R>(a: &str, b: &str, f: impl for<'x> FnOnce(Operands<'x>) -> R) -> R;
pub fn utf16_len(s: &str) -> usize;
pub enum Operands<'a> { Bytes(&'a [u8], &'a [u8]), Units(&'a [u16], &'a [u16]) }
pub trait Unit: Copy + PartialEq + Eq { type Map: UnitMap<Self>; fn new_map() -> Self::Map; }
pub trait UnitMap<T> {
    fn get(&self, key: T) -> Option<usize>;
    fn set(&mut self, key: T, row: usize);
    fn clear(&mut self);
}
pub struct ByteMap { /* private [usize; 256] */ }   // flat map for the byte path

// StringMetric markers (verbora_distance)
pub struct Levenshtein(pub levenshtein::Options);        // IS_SIMILARITY = false
pub struct DamerauLevenshtein(pub levenshtein::Options); // IS_SIMILARITY = false
pub struct JaroWinkler(pub jaro_winkler::Options);       // IS_SIMILARITY = true
pub struct Dice;                                         // IS_SIMILARITY = true
pub struct Hamming { pub ignore_case: bool }             // IS_SIMILARITY = false
```
