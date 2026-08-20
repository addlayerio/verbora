# Choosing a distance API

`verbora-distance` exposes sixteen free functions across four metric families,
plus `PreparedPattern` — a build-once type for comparing a fixed pattern against
many candidates — and three cost types for the weighted forms. Behind the
optional `parallel` feature there are six more, one batch fan-out per metric.
This page is about picking one — by the problem you have, not by the name you
remember.

For what each function does in detail, see
[String distance and similarity](../features/distance.md). For how Verbora
shapes its APIs generally, see [API shapes](./api-shapes.md).

## Start from the problem

| Your problem | Metric | Why this one |
|---|---|---|
| Spelling correction, "did you mean…?", fuzzy command lookup | `osa` | typing errors are insert/delete/substitute **plus** adjacent swaps; OSA counts a swap as one mistake and, at unit cost, runs the same bit-parallel kernel family as plain `levenshtein` |
| The same, but the number must match a plain edit distance | `levenshtein` | the plain recurrence is what "edit distance" usually means, and it is the cheapest path here |
| Record linkage: people, companies, addresses | `jaro_winkler` | bounded `0..=1`, tolerant of the middle of a string, and boosted for a shared prefix — which is exactly how human names vary |
| Fixed-length codes, checksums, DNA, bit strings | `hamming` | `O(n)`, allocation-free on every input, and an `Option<usize>` — `None` when the operands do not hold the same number of characters |
| Long strings where shared content matters more than order | `dice_coefficient` | bigram *set* overlap; `O(n + m)` expected rather than `O(nm)`, and insensitive to reordering |
| "Where in this document does this phrase roughly appear?" | `levenshtein_search` / `damerau_levenshtein_search` / `osa_search` | returns the matched text borrowed from the target, its byte range, and its distance |
| Edit operations that have genuinely different prices | the matching `*_weighted` function | a validated cost set prices each operation, and the answer is an `f64` |

## Decision table

| What you are comparing | Call |
|---|---|
| Two strings holding the same number of characters | `hamming()` → `Option<usize>`, `None` when the counts differ |
| A query against candidate spellings, where a swap ("teh"/"the") costs one edit | `osa()` |
| The same, but a swap is honestly two edits | `levenshtein()` |
| Two names or records that may differ anywhere, with the usual prefix boost | `jaro_winkler()` |
| The same, but you want the raw score to boost yourself | `jaro()` |
| Two longer texts where word order is not meaningful | `dice_coefficient()` |
| A short needle inside a long haystack | `levenshtein_search()` / `damerau_levenshtein_search()` / `osa_search()` — check the memory cost first |
| Any of the above, with insertions, deletions or swaps priced differently | the matching `*_weighted` function, and a cost set built through its `new` |

## What one unit is

Every count this crate reports is a count of **Unicode scalar values** — what
`str::chars()` yields, and what `s.chars().count()` counts. `"a😀b"` is three
units, `"café"` in NFC is four, `"日本語"` is three. There is no length function
to learn, because `chars().count()` is that function.

Positions are the one quantity measured in something else, and that something is
**bytes**: `SearchResult::range()` is a byte range, so it slices the target
directly.

Two consequences worth knowing before you pick a metric.

**No metric rewrites its input.** Nothing is lower-cased, trimmed, collapsed or
normalised anywhere in the crate, so case and whitespace are significant in every
function. Caseless matching is a fold at the call site, done once at ingestion
rather than re-done per candidate:

```rust
use verbora_distance::dice_coefficient;

fn main() {
    assert_eq!(dice_coefficient("ABC", "abc"), 0.0);
    assert_eq!(dice_coefficient(&"ABC".to_lowercase(), "abc"), 1.0);
}
```

**A scalar is not a grapheme cluster, and not a normalisation form.** Deleting a
whole cluster costs as many edits as it has scalars, and `"café"` in NFC against
`"café"` in NFD is a distance of 2 rather than 0. If either matters, segment with
`unicode-segmentation` or normalise with
[`verbora-normalizers`](../features/normalizers.md) before you compare.

## Typo correction: Levenshtein or Damerau?

Both are `O(nm)` in time, and at unit cost both `levenshtein` and `osa` run
bit-parallel kernels that handle 64 cells per word. The choice is about what a
transposition costs — and it is made by calling a different function, never by
setting a flag.

| | `levenshtein` | `osa` | `damerau_levenshtein` |
|---|:--:|:--:|:--:|
| "ab" → "ba" | 2 | 1 | 1 |
| "ca" → "abc" | 3 | 3 | 2 |
| Working set | bit-vector (unit cost) or 1 row (weighted) | bit-vector (unit cost) or 3 rows (weighted) | Zhao–Sahni's linear-space rows (unit cost) or the full matrix (weighted) |
| Is a true metric | yes | no (triangle inequality can fail) | yes (symmetric, and the triangle inequality holds) |
| Which cost sets the weighted form takes | every admissible one — `LevenshteinCosts` has no transposition price to constrain | every admissible one | only a `DamerauCosts`, which requires `2 × transposition >= insertion + deletion` |
| Measured, 64 units † | 166.1 ns | 179.4 ns | 7.75 µs |

`levenshtein` and `osa` are within a few percent of each other. The unrestricted
variant is on a different curve — its transposition can reach an arbitrary
earlier row, so it sweeps cell by cell rather than word by word, and no
bit-vector formulation applies. The `levenshtein` and `osa` numbers come from the
[`levenshtein_variants`](../benchmarks/distance.md) group on a shared
64-character pair.

† Pending re-measurement, and left as recorded rather than replaced with a
guess; the ratios between the columns are not current either.

**Default to `osa`** for user-facing typo tolerance. Take
`damerau_levenshtein` when a transposition should cost one operation even with
edits in between, or when you need the true metric.

<div class="callout callout-warn">
<strong>Do not build a metric-space index on <code>osa</code>.</strong>
Optimal string alignment can violate the triangle inequality, because nothing
may be edited between a swapped pair. If you are feeding distances into a
BK-tree or anything else that assumes metric axioms, use
<code>levenshtein</code> or <code>damerau_levenshtein</code> — both are true
metrics.
</div>

## Unit cost or weighted?

Each of the three edit distances is published twice: a **unit-cost** function
that takes no cost argument, and a **weighted** function that takes a validated
cost set.

| | `levenshtein`, `damerau_levenshtein`, `osa` | `levenshtein_weighted`, `damerau_levenshtein_weighted`, `osa_weighted` |
|---|---|---|
| Question answered | how many edits separate the two strings | the minimum total cost of an edit script under prices you assign |
| Cost argument | none | `LevenshteinCosts` / `DamerauCosts` / `OsaCosts`, each built through a `new` returning `Result` |
| Returns | `usize` — exact, `Ord`, `Hash`, usable as a map key | `f64` |
| Kernel | bit-parallel: Myers, Hyyrö, Zhao–Sahni | scalar dynamic program |
| Common-affix trimming | yes — a pair differing in one interior position collapses to `O(1)` kernel work | no |
| `PreparedPattern` | yes | no |
| Can the call get it wrong | there is no argument to get wrong | an inadmissible cost set is rejected by the constructor, before any metric sees it |

**Unit cost is the absence of an argument, not a value.** There is no way to
spell "unit costs" as something you pass in, because the unit metric is a
different function: a different kernel, a different complexity class and a
different result type. Reach for it unless you can name the prices — it is
faster, it cannot fail to construct, and its `usize` result keys a `HashMap` or
seeds a BK-tree edge without raising any of the questions a floating-point
distance does. Where the two tiers overlap they agree exactly:

```rust
use verbora_distance::{LevenshteinCosts, levenshtein, levenshtein_weighted};

fn main() {
    assert_eq!(levenshtein("kitten", "sitting"), 3);

    let unit = LevenshteinCosts::new(1.0, 1.0, 1.0).expect("1.0 is admissible");
    assert_eq!(
        levenshtein("kitten", "sitting") as f64,
        levenshtein_weighted("kitten", "sitting", &unit)
    );

    // Priced: a deletion costs three times what an insertion does, so
    // shortening "abc" to "ab" costs 3.0 rather than 1.0.
    let costs = LevenshteinCosts::new(1.0, 3.0, 1.0).expect("finite and non-negative");
    assert_eq!(levenshtein_weighted("abc", "ab", &costs), 3.0);
}
```

A cost is admissible when it is finite and non-negative; anything else comes back
as a `CostError` naming the operation and the offending value. Zero is
admissible, and makes the result a pseudometric rather than a metric.

<div class="callout callout-warn">
<strong><code>DamerauCosts::new</code> rejects a transposition cheaper than the
pair of edits it replaces.</strong> It returns
<code>Err(CostError::TranspositionBelowThreshold)</code> unless
<code>2 × transposition &gt;= insertion + deletion</code>. Below that threshold a
chain of adjacent swaps is a cheaper way to move a character than
delete-and-reinsert, and the Lowrance–Wagner recurrence (Lowrance &amp; Wagner
1975, JACM 22(2)) — which credits at most one transposition per matching
row/column pair — stops ranging over every edit script, so what it returns is
<em>a</em> script's cost rather than the minimum. If your weights put a swap
below the threshold, use <code>osa_weighted</code>: optimal string alignment's
recurrence <em>defines</em> its answer as the minimum over alignments that edit
no position twice, so every admissible cost set is sound there. The precondition
is discharged once, at construction, so
<code>damerau_levenshtein_weighted</code> and
<code>damerau_levenshtein_search_weighted</code> cannot be reached with a cost
set that violates it — and no function in this crate panics on any input. Full
reasoning on <a href="../features/distance">the feature page</a>.
</div>

## Record linkage: Jaro–Winkler or Dice?

Both return `0.0..=1.0` with higher meaning closer, so they are interchangeable
in a ranking function's *shape*. They are not interchangeable in behaviour.

| | `jaro_winkler` | `dice_coefficient` |
|---|---|---|
| Compares | positions, within a sliding window | the set of adjacent bigrams |
| Sensitive to word order | yes | no |
| Rewards a shared prefix | yes, up to `+0.4` | no |
| Case and whitespace | significant | significant |
| Repeated content | counted | collapses (`"aaaa"` ≡ `"aa"`) |
| Complexity | `O(nm)` | `O(n + m)` expected |
| Identical inputs | `1.0`, bit-exactly | `1.0`, bit-exactly |
| One side empty, or nothing in common | `0.0` | `0.0` |
| Measured, 1024 units † | 10.34 µs | 10.61 µs |

† Pending re-measurement, and left as recorded rather than replaced with a guess.

Use `jaro_winkler` for short, ordered records — personal names above all. Use
`dice_coefficient` for longer strings, and for titles and descriptions where
word order varies. **Choose on behaviour, not speed.**

<div class="callout callout-good">
<strong>Both are safe to rank on directly.</strong> Every similarity in this
crate is a finite <code>f64</code> in <code>0.0..=1.0</code>; identical inputs
score <code>1.0</code> bit-exactly, so <code>score == 1.0</code> is a sound test
to write; and swapping the arguments returns the identical bits. There is no
<code>NaN</code> to filter out and no sentinel to guard, so
<code>total_cmp</code>, <code>max_by</code> and <code>sort_by</code> over a
candidate list are well defined and order-independent.
</div>

## Scalar or search?

| | `levenshtein` / `damerau_levenshtein` / `osa` | `levenshtein_search` / `damerau_levenshtein_search` / `osa_search` |
|---|---|---|
| Question | how far apart are these two strings? | where in the target does the source best occur? |
| Returns | `usize` — `f64` from the weighted forms | `SearchResult<'_, usize>` — `SearchResult<'_, f64>` from the weighted forms — carrying the matched text, its byte range and its distance |
| Row 0 of the matrix | costs accumulate | free — every prefix is a valid start |
| Working set | bit-vector, 1–3 rows, or Zhao–Sahni's rows | two 64-bit words per target column for plain Levenshtein at unit cost; the full matrix for every other combination |
| Extra allocation | none beyond the working set above | none — the result borrows the target |
| Measured, 64 units † | 166.1 ns (plain) | 12.79 µs |

† Pending re-measurement, and left as recorded rather than replaced with a guess.

Search is not "distance plus a bonus" — it answers a different question, and its
answer to *"how far apart are these?"* is not the same number.
`levenshtein("ca", "abc")` is `3`; `levenshtein_search("ca", "abc").distance()`
is `1`, because the search is free to ignore the unmatched part of the target.

```rust
use verbora_distance::levenshtein_search;

fn main() {
    let target = "Zürich, Berlin, Wien";
    let found = levenshtein_search("Berlin", target);

    assert_eq!(found.substring(), "Berlin");
    assert_eq!(found.distance(), 0);

    // A byte range: "Zürich, " is eight characters but nine bytes, because
    // "ü" takes two.
    assert_eq!(found.range(), 9..15);
    assert_eq!(&target[found.range()], found.substring());
}
```

Three things to know before reaching for it:

1. **Memory grows with the target.** At unit cost, `levenshtein_search` stores
   two 64-bit words per target column per 64 units of needle: a 10-unit needle
   in a 100,000-unit haystack is about 1.6 MB. Every other combination — both
   Damerau variants, and every weighted search — materialises the full
   `(n+1) × (m+1)` matrix at 16 bytes a cell, about 17 MB for that same pair.
   Chunk long targets yourself, with an overlap of at least the needle length.
2. **The result is a substring, not a token.** It is whatever range of the target
   is cheapest, which is frequently a fragment of a word. If you need word
   boundaries, tokenize first and score the tokens.
3. **The range is a byte range, and it is derived from the text.**
   `&target[r.range()] == r.substring()` for every input, so the range is a
   highlight span you can use directly; and `r.substring()` on its own is
   `r.distance()` away from the source, under weighted costs too. The search is
   total — the empty substring is always a candidate — so there is no `Option`
   to unwrap and "close enough" stays your threshold.

## Repeated and bulk comparison

<div class="callout callout-warn">
<strong>There is no scratch-buffer or sequential batch API in this crate.</strong>
No <code>levenshtein_with_scratch</code>, no <code>*_into</code> variant, no
plain <code>*_batch</code>. Nothing here takes mutable working memory you lend
it for the duration of a call: every scalar function builds its own
dynamic-programming working set and drops it. Behind the optional
<code>parallel</code> Cargo feature there is one <code>par_*_batch</code>
function per metric — <code>par_levenshtein_batch</code>,
<code>par_damerau_levenshtein_batch</code>, <code>par_osa_batch</code>,
<code>par_jaro_winkler_batch</code>, <code>par_hamming_batch</code> and
<code>par_dice_coefficient_batch</code> — each a thin
<code>pairs.par_iter().map(&lt;the sequential function&gt;).collect()</code>
fan-out over the unit-cost form. A weighted batch is the same one-liner at your
own call site.
</div>

<div class="callout callout-good">
<strong>What does exist is prepared pattern state.</strong> When one side of
the comparison is fixed — a query term against a candidate list —
<code>PreparedPattern::new(query)</code> builds the bit-parallel match table
that <code>levenshtein</code> and <code>osa</code> would otherwise rebuild on
every call, and answers every candidate from it. That is immutable state
derived from one operand, not a buffer you lend and the algorithm scribbles
in — which is why it needs no reset between queries and can be shared across
threads by reference.
</div>

Four things you can do at the call site.

### 1. Prepare the pattern when one side is fixed

`PreparedPattern` is built from the fixed operand and queried once per
candidate. It answers exactly what the free function of the same name answers
for `(pattern, candidate)` — where it cannot use its table, it calls that
function itself — so switching to it cannot change a ranking.

```rust
use verbora_distance::PreparedPattern;

fn main() {
    let query = PreparedPattern::new("sittin");   // table built once, here

    let best = ["kitten", "mitten", "sitting", "bitten"]
        .into_iter()
        .map(|c| (c, query.levenshtein(c)))       // and reused per candidate
        .min_by_key(|&(_, d)| d)
        .map(|(c, _)| c);

    assert_eq!(best, Some("sitting"));
}
```

Two cases fall back to the per-call function rather than misbehaving. The
element type is chosen from the pattern, so a non-ASCII candidate against an
ASCII pattern goes through it; and so does a pair sharing a long affix, because
trimming a shared prefix shifts every pattern position and a table built over
the whole pattern encodes the unshifted ones. Both paths compute the same
distance.

`levenshtein` and `osa` share the one table, and it is built for both at once,
so using both costs nothing extra. `damerau_levenshtein` has no prepared form:
its unit-cost kernel is Zhao–Sahni's linear-space recurrence, whose only table
is filled during the scan and depends on both operands, so there is nothing to
hoist. The weighted forms have none either — the prepared table is a
bit-parallel structure, and the bit-parallel kernels have no notion of a
weighted operation. Call those directly.

### 2. Gate on length before paying for the comparison

At unit cost, the edit distance is at least the difference in length:
`|a.chars().count() - b.chars().count()| <= levenshtein(a, b)`, because each
insertion or deletion changes the character count by exactly one and each
substitution by zero. Counting characters is a cheap linear walk next to the
comparison it skips, and the bound cannot discard a real match.

```rust
use verbora_distance::levenshtein;

fn main() {
    let query = "sittin";
    let max_edits = 2;
    let n = query.chars().count();

    let survivors: Vec<&str> = ["sitting", "a much longer phrase entirely", "mitten"]
        .into_iter()
        .filter(|c| c.chars().count().abs_diff(n) <= max_edits)
        .filter(|c| levenshtein(query, c) <= max_edits)
        .collect();

    assert_eq!(survivors, ["sitting", "mitten"]);
}
```

With a weighted cost set the bound becomes `min(insertion, deletion) × |n − m|`;
adjust the gate accordingly or it will start discarding matches.

### 3. Pick the cheaper argument order — for the weighted forms

The unit-cost kernels choose for you: they hand whichever operand is shorter to
the bit-parallel kernel as the pattern, which is sound because unit costs are
symmetric. `levenshtein_weighted` and `osa_weighted` size their row buffers from
the **target**, at `len(target) + 1` elements each, so passing the shorter string
as `target` allocates less. That is only safe while `insertion == deletion` —
with those apart, swapping asks a different question:

```rust
use verbora_distance::{LevenshteinCosts, levenshtein_weighted};

fn main() {
    let symmetric = LevenshteinCosts::new(1.0, 1.0, 1.0).expect("admissible");
    assert_eq!(
        levenshtein_weighted("kitten", "sitting", &symmetric),
        levenshtein_weighted("sitting", "kitten", &symmetric)
    );

    let asymmetric = LevenshteinCosts::new(1.0, 3.0, 1.0).expect("admissible");
    assert_ne!(
        levenshtein_weighted("abc", "ab", &asymmetric),
        levenshtein_weighted("ab", "abc", &asymmetric)
    );
}
```

### 4. Parallelise — the crate's own batch function first

Every function here is pure and stateless: `&str` arguments and, for the
weighted forms, a borrowed cost set. No interior mutability, no globals. The
cost types are `Copy + Send + Sync`, so one instance can be shared by reference
across threads. That is what the crate's `parallel` feature exploits, per
metric:

```toml
[dependencies]
verbora-distance = { version = "0.1", features = ["parallel"] }
```

```rust  ignore
// Requires verbora-distance's `parallel` feature.
use verbora_distance::par_levenshtein_batch;

fn main() {
    let pairs = [("kitten", "sitting"), ("mitten", "sitting")];
    let scores = par_levenshtein_batch(&pairs);
    assert_eq!(scores, vec![3, 3]);
}
```

That covers "distance for every pair in a batch". It does not cover a custom
reduction like "the single nearest candidate" — for that, parallelise at your own
call site. The block below is marked `ignore` because the documentation's example
crate does not depend on `rayon`; add `rayon = "1"` to your own `Cargo.toml` and
it compiles as written:

```rust  ignore
use rayon::prelude::*;
use verbora_distance::levenshtein;

fn nearest<'a>(query: &str, corpus: &[&'a str]) -> Option<&'a str> {
    corpus
        .par_iter()
        .map(|c| (*c, levenshtein(query, c)))
        .min_by_key(|&(_, d)| d)
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
a BK-tree over <code>levenshtein</code> or <code>damerau_levenshtein</code>
(both true metrics — not over <code>osa</code>).
</div>

[Parallelism](../performance/parallelism.md) covers the workspace-wide position:
outside the curated `par_*_batch` functions, Verbora ships no parallel entry
point, and expects `rayon` to live at the application boundary.

## Cost at a glance

Per single call. `n` and `m` are character counts; the measured column is a
Criterion median on a 64-unit ASCII pair, from
[Benchmarks: distance](../benchmarks/distance.md).

| API | Time | Allocations (ASCII) | Measured, 64 units † |
|---|---|---|--:|
| `hamming` | `O(n)` | none, on any input | 20.5 ns |
| `jaro`, `jaro_winkler` | `O(nm)`, bit-parallel above 16 units | none while both trimmed operands fit one 64-bit word; one `Vec<u64>` for the packed match table beyond that | 130.7 ns |
| `levenshtein` | `O(nm)`, bit-vector `O(nm/64)` at unit cost (the common case) | none beyond the kernel's character-mask tables; `levenshtein_weighted` adds 1 `Vec<f64>` row | 166.1 ns |
| `osa` | `O(nm)`, bit-vector `O(nm/64)` at unit cost | character-mask tables; `osa_weighted` adds 3 `Vec<f64>` rows | 179.4 ns |
| `dice_coefficient` | `O(n + m)` expected | 2 bigram hash sets | 1.00 µs |
| `damerau_levenshtein` | `O(nm)` | Zhao–Sahni's rows and a last-occurrence table; `damerau_levenshtein_weighted` uses the full matrix | 7.75 µs |
| `levenshtein_search`, `damerau_levenshtein_search`, `osa_search` | `O(nm)` | two `Vec<u64>` of `m × ⌈n/64⌉` words for plain Levenshtein at unit cost; the full `(n+1) × (m+1)` matrix at 16 bytes a cell otherwise, and for every weighted search | 12.79 µs |

`PreparedPattern::levenshtein` and `PreparedPattern::osa` have the same `O(nm)`
shape and the same `O(nm/64)` unit-cost kernels as the two rows they mirror,
minus the per-call construction of the character-mask table — which is the part
that grows with the *pattern's* length. Nothing above is measured with a
prepared pattern; the table is per single call.

Cost is strongly size- and script-dependent: `levenshtein` runs 14.7 ns on a
4-unit ASCII pair and 29.08 µs on a 1024-unit one †, and a non-ASCII pair pays
for a per-call scalar decode on top. The full result tables, the hardware and
the methodology are in [Benchmarks: distance](../benchmarks/distance.md) and
[Performance](../performance/index.md).

† Pending re-measurement, and left as recorded rather than replaced with a guess.

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
