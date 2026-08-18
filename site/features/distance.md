# String distance and similarity

`verbora-distance` answers one question in seven ways: *how far apart are these
two strings?* It implements seven distance and similarity metrics —
Levenshtein and Damerau–Levenshtein (each in a scalar and a substring-search
flavour), Jaro, Jaro–Winkler, the Sørensen–Dice coefficient, and Hamming —
with exact, fully specified results, down to the edge cases most
implementations leave undefined (see
[Three specified surprises](#three-specified-surprises)).

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
- **Approximate substring location.** Finding where in a longer document a
  short phrase most nearly occurs (`levenshtein_search`).
- **Fixed-width code comparison.** Counting differing positions in equal-length
  identifiers, checksums or binary strings (`hamming`).
- **Deterministic, fully specified results.** Every metric's behavior —
  including the edge cases most implementations leave undefined — is documented
  and test-pinned, so a change of implementation cannot silently change what
  your application ranks first.

## When not to use it

- **Semantic similarity.** These are all *surface* metrics. "car" and
  "automobile" are maximally distant under every one of them.
- **Whole-document comparison at scale.** Levenshtein is `O(nm)` in time;
  search mode is also `O(nm)` in *memory*. A 10-unit needle against a
  100,000-unit haystack allocates a matrix of `11 × 100,001` cells at 16 bytes
  each — about 17 MB — for a single call.
- **Indexed search over a large corpus.** Verbora ships no index and no
  BK-tree. The `parallel`-gated batch fan-out scores many pairs you already
  know about, but every pair still costs a full comparison, so an unindexed
  nearest-neighbour search is still `O(corpus size)` per query. See
  [Choosing a distance API](../choosing/distance.md#repeated-and-bulk-comparison).
- **Sound-alike matching.** For "Smith" ≈ "Smyth" as *pronunciation* rather
  than spelling, reach for [phonetics](./phonetics.md) first and use a distance
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

This is the first thing to internalise, and the most common source of a
silently inverted ranking. Some metrics are distances (lower is closer), some
are similarities (higher is closer), and mixing them in one ranking inverts it.

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
silently. The <a href="#the-stringmetric-trait"><code>StringMetric</code></a>
trait exists to make the direction explicit in generic code.
</div>

## Choosing the right API

| API | Answers | Returns | Cost (time) | Working set | Allocation-free |
|---|---|---|---|---|:--:|
| `levenshtein` | how many single-character edits? | `f64` | `O(nm)` | bit-vector (unit costs) / 1 row | ❌ |
| `damerau_levenshtein` (`restricted: true`) | …counting an adjacent swap as one edit, with nothing edited between two swaps | `f64` | `O(nm)` | bit-vector (unit costs) / 3 rows | ❌ |
| `damerau_levenshtein` (`restricted: false`) | …counting a swap as one edit however far apart the pair ends up | `f64` | `O(nm)` | 2 rows + per-symbol snapshots | ❌ |
| `levenshtein_search` | *where* in the target does the source best occur? | `SearchResult` | `O(nm)` | full matrix | ❌ |
| `damerau_levenshtein_search` | same, tolerating transpositions | `SearchResult` | `O(nm)` | full matrix | ❌ |
| `jaro` | how many characters match within a sliding window? | `f64` | `O(nm)` | bit-parallel match masks | ✅ (≤ 128 units, ASCII) |
| `jaro_winkler` | …with a bonus for a shared prefix | `f64` | `O(nm)` | bit-parallel match masks | ✅ (≤ 128 units, ASCII, no `ignore_case`) |
| `dice_coefficient` | how much do the bigram *sets* overlap? | `f64` | `O(n + m)` expected | 2 hash sets | ❌ |
| `hamming` | how many positions differ? | `i64` | `O(n)` | none | ✅ (ASCII, no `ignore_case`) |
| `hamming_checked` | same, as an `Option` | `Option<u64>` | `O(n)` | none | ✅ (ASCII, no `ignore_case`) |

`n` and `m` are lengths **in UTF-16 code units**, not bytes or `char`s. See
[Unicode and language notes](#unicode-and-language-notes).

### Which one

| Your situation | Use |
|---|---|
| Equal-length codes, hashes, fixed fields; `-1` means "incomparable" | `hamming` |
| Same, but you want Rust's vocabulary for "no answer" | `hamming_checked` → `Option<u64>` |
| Typos, and a swap is honestly two edits | `levenshtein` |
| Typos, adjacent swaps cost 1, swapped characters never edited again | `damerau_levenshtein` with `restricted: true` (OSA) |
| Typos, swaps may be arbitrarily far apart | `damerau_levenshtein` with `restricted: false` |
| The *position* of the best approximate occurrence in a longer string | `levenshtein_search` / `damerau_levenshtein_search` |
| Names or short records, shared prefix is meaningful | `jaro_winkler` (or `jaro` for the unboosted score) |
| Shared content, order and position irrelevant | `dice_coefficient` |

### `levenshtein`

Eager, returns `f64`, no scratch or `_into` API. With the default unit costs it
takes a Myers/Hyyrö bit-vector fast path at every length — one `u64` word for
operands up to 64 units, a block extension beyond — answering in `O(nm/64)`
bitwise operations rather than `O(nm)` scalar cell updates. Weighted costs fall
back to a rolling row plus two scalar temporaries, never a full matrix. That
single choice is what makes long operands cheap; see
[Measured performance](#measured-performance).

```rust
use verbora_distance::levenshtein::{Options, levenshtein};

fn main() {
    assert_eq!(levenshtein("kitten", "sitting", &Options::default()), 3.0);
    assert_eq!(levenshtein("", "abc", &Options::default()), 3.0);
    assert_eq!(levenshtein("same", "same", &Options::default()), 0.0);
}
```

<div class="callout callout-note">
<strong>Note.</strong> <code>levenshtein</code> reads only three of the five
<code>Options</code> fields. <code>transposition_cost</code> and
<code>restricted</code> are inspected by the Damerau functions and ignored here
— setting them on a call to <code>levenshtein</code> does nothing at all.
</div>

### `damerau_levenshtein`

Same shape as `levenshtein`, but an adjacent swap costs one edit rather than
two. `restricted` picks between two genuinely different algorithms:

| `restricted` | Rule | Working set | Classic name |
|---|---|---|---|
| `false` (default) | a transposition may reach back to *any* earlier row; the substring between two swapped characters may itself be edited | 2 rows + per-symbol snapshots | unrestricted Damerau–Levenshtein |
| `true` | a transposition may only reach row − 2, so no substring is edited between two transpositions | bit-vector (unit costs) / 3 rows | optimal string alignment (OSA) |

```rust
use verbora_distance::levenshtein::{Options, damerau_levenshtein, levenshtein};

fn main() {
    // A swap is two edits under Levenshtein, one under Damerau.
    assert_eq!(levenshtein("ab", "ba", &Options::default()), 2.0);
    assert_eq!(damerau_levenshtein("ab", "ba", &Options::default()), 1.0);

    // The two algorithms disagree on real input.
    let restricted = Options { restricted: true, ..Options::default() };
    assert_eq!(damerau_levenshtein("ca", "abc", &Options::default()), 2.0);
    assert_eq!(damerau_levenshtein("ca", "abc", &restricted), 3.0);
}
```

**Prefer `restricted: true` unless you need the true metric.** At the default
unit costs it runs on a bit-parallel kernel (179.4 ns against 7.75 µs at 64
characters in the measured suite), at weighted costs it is still the cheaper
structure, and it is what most "Damerau–Levenshtein" implementations actually
compute.

### `levenshtein_search` and `damerau_levenshtein_search`

Search mode makes every prefix of the target a free starting point (row 0 costs
nothing), takes the minimum over the last row, then walks the parent
back-pointers to recover where the match began. That backtrace is why the
parent array — and therefore the whole matrix — has to exist, which makes this
the one expensive mode: one `Vec<f64>` plus one `Vec<(u32, u32)>` of
`(n+1)(m+1)` cells at 16 bytes each, plus the result `String`.

```rust
use verbora_distance::levenshtein::{Options, levenshtein_search};

fn main() {
    let r = levenshtein_search("ca", "abc", &Options::default());
    assert_eq!((r.substring.as_str(), r.distance, r.offset), ("a", 1.0, 0));

    // The cheapest substring need not be a word.
    let r = levenshtein_search("quikc", "the quick brown fox", &Options::default());
    assert_eq!((r.substring.as_str(), r.distance, r.offset), ("quic", 1.0, 4));
}
```

<div class="callout callout-warn">
<strong>Careful.</strong> Search returns the cheapest <em>substring</em>, which
need not be a word or anything a human would have picked — above, deleting the
stray <code>k</code> is cheaper than the two substitutions that would reach
<code>"quick"</code>. Tie-breaking is deterministic: the cheapest predecessor
wins on a strict <code>&lt;</code>, so the first candidate in the fixed order
insert, delete, substitute, unrestricted-transpose, restricted-transpose wins a
tie. Totals never depend on that order, but the recorded parent does — and the
parent chain is what produces <code>offset</code> and <code>substring</code>.
</div>

### `jaro` and `jaro_winkler`

`jaro` counts characters that match within a window of
`floor(max(n, m) / 2) - 1` positions, then penalises the ones that matched out
of order. `jaro_winkler` adds `0.1 × l × (1 − jaro)`, where `l` is the shared
prefix length capped at 4. Both allocate nothing for ASCII operands of ≤ 128
units with no case folding.

```rust
use verbora_distance::jaro_winkler;

fn main() {
    let opts = Default::default();
    assert!((jaro_winkler("MARTHA", "MARHTA", &opts) - 0.9611111111111111).abs() < 1e-12);
    assert!((jaro_winkler("DWAYNE", "DUANE", &opts) - 0.84).abs() < 1e-12);
}
```

Two behaviours worth knowing before you rely on the score:

- The match window is **signed**. For two one-character strings it is
  `floor(1/2) - 1 == -1`, so nothing can match and Jaro is `0.0` —
  `jaro("a", "b")` and `jaro("a", "a")` are both `0.0` through that path.
- `jaro_winkler` short-circuits to `1.0` when the two strings are equal
  **before** any case folding. That guard normally hides the previous point;
  when `ignore_case` makes the strings equal only *after* folding, it does not
  fire. See [Three specified surprises](#three-specified-surprises).

### `dice_coefficient`

Dice compares the *sets* of adjacent code-unit pairs. Before that it
lower-cases each input, collapses runs of whitespace to one space using this
crate's own `is_whitespace` predicate (not Rust's `char::is_whitespace` — see
[Unicode and language notes](#unicode-and-language-notes)), and trims. Because
the bigrams form a set, repeats collapse.

```rust
use verbora_distance::dice_coefficient;

fn main() {
    assert_eq!(dice_coefficient("Hello  World", "hello world"), 1.0);
    assert_eq!(dice_coefficient("aaaa", "aa"), 1.0); // both reduce to {"aa"}
    assert_eq!(dice_coefficient("", "abc"), 0.0);
    assert!(dice_coefficient("", "").is_nan());
}
```

Dice is the only metric here with no ASCII fast path: its bigram keys are
code-unit pairs, so it always builds `Vec<u16>` operands. Its cost is dominated
by hashing rather than by a quadratic sweep — at 1024 characters it costs about
the same as Jaro–Winkler (10.61 µs against 10.34 µs in the measured suite).

### `hamming` and `hamming_checked`

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

`hamming_checked` is the same computation with `INCOMPARABLE` mapped to `None`,
so `?`, `filter_map`, `unwrap_or` and the rest of the `Option` vocabulary
apply. **Use it by default**; reach for `hamming` when your own code expects
the `-1` sentinel, or when feeding a system that does.

The length check runs on the *original* strings, before any case folding —
which matters because Unicode case mapping can change length (`İ` lowercases to
two code units; `ß` uppercases to `SS`). After folding, the comparison loop is
bounded by the first string and treats a read past the end of the second as "no
character", which never equals anything.

### Parallel batch (`par_*_batch`, feature `parallel`)

<a class="badge badge-batch" href="../performance/batch-vs-streaming">BATCH</a>

Behind the `parallel` Cargo feature (never on by default), five functions fan a
slice of pairs out across a `rayon` thread pool at **per-pair** granularity.
Each is exactly `pairs.par_iter().map(<the sequential function>).collect()` —
never a second implementation — and each preserves input order, with its own
sequential-vs-parallel equivalence test.

| Function | Wraps | Missing sibling |
|---|---|---|
| `par_levenshtein_batch` | `levenshtein` | — |
| `par_damerau_levenshtein_batch` | `damerau_levenshtein` | — |
| `par_jaro_winkler_batch` | `jaro_winkler` | no `par_jaro_batch` for the unboosted score |
| `par_dice_coefficient_batch` | `dice_coefficient` | — |
| `par_hamming_batch` | `hamming` | no batch form of `hamming_checked` |

There is no wrapper for `levenshtein_search` or `damerau_levenshtein_search`;
apply the same `par_iter().map(...)` pattern yourself if you need them in
parallel (see [Parallelism](../performance/parallelism.md)).

```rust  ignore
use verbora_distance::levenshtein::Options;
use verbora_distance::par_levenshtein_batch;

let pairs = [("kitten", "sitting"), ("", "abc")];
let distances = par_levenshtein_batch(&pairs, &Options::default());
assert_eq!(distances, [3.0, 3.0]);
```

<div class="callout callout-note">
<strong>Note.</strong> This block needs the <code>parallel</code> feature, which
this site's own snippet checker builds without, so it is marked
<code>ignore</code> rather than compiled — every other block on this page
compiles and runs in CI.
</div>

**The crossover differs sharply per metric**, because each metric's per-pair
cost has to clear `rayon`'s roughly-one-microsecond scheduling cost before a
batch call is worth it. One machine's numbers (32 threads, default global
`rayon` pool), as a speed-up against the sequential loop:

| Batch function | 4 chars | 64 chars | 1024 chars |
|---|--:|--:|--:|
| `par_levenshtein_batch` / `par_damerau_levenshtein_batch` (300 pairs) | 0.6× | 15.6× | 22.0× |
| `par_jaro_winkler_batch` (1000 pairs) | 0.16× | 7.9× | 15.6× |
| `par_dice_coefficient_batch` (1000 pairs) | 2.0× | 10.3× | — |
| `par_hamming_batch` (1000 pairs) | 0.12× | 0.12×–1× | 1.7× from 256 chars |

`hamming` is nearly free per short pair, so it loses until pairs are long.
`dice_coefficient` hashes two bigram sets per call, already heavier than
scheduling at every length tested, so it wins even at four characters. Reproduce
with `cargo bench -p verbora-distance --features parallel -- par_<name>` before
relying on these for capacity planning.

## `levenshtein::Options`

| Field | Type | Default | Read by | Effect |
|---|---|--:|---|---|
| `insertion_cost` | `f64` | `1.0` | all four functions | cost of inserting one symbol |
| `deletion_cost` | `f64` | `1.0` | all four functions | cost of deleting one symbol |
| `substitution_cost` | `f64` | `1.0` | all four functions | cost of replacing one symbol with another |
| `transposition_cost` | `f64` | `1.0` | Damerau variants only | cost of swapping two adjacent symbols |
| `restricted` | `bool` | `false` | Damerau variants only | `true` selects optimal string alignment (3 rows); `false` selects unrestricted Damerau |

The costs are `f64` rather than `usize` because arbitrary numbers are accepted
here — fractions and zero included. Zero costs are legal and produce zero
distances. `Options` is `Copy + Clone + Debug + PartialEq` (not `Eq` — it holds
`f64`).

```rust
use verbora_distance::levenshtein::{Options, levenshtein};

fn main() {
    let asymmetric = Options { deletion_cost: 3.0, ..Options::default() };
    // Deleting `c` costs 3; inserting it still costs 1.
    assert_eq!(levenshtein("abc", "ab", &asymmetric), 3.0);
    assert_eq!(levenshtein("ab", "abc", &asymmetric), 1.0);

    // Fractional costs work as expected.
    let weighted = Options {
        insertion_cost: 0.5,
        deletion_cost: 1.5,
        substitution_cost: 0.75,
        ..Options::default()
    };
    assert_eq!(levenshtein("ab", "abc", &weighted), 0.5);
}
```

<div class="callout callout-note">
<strong>Note.</strong> With the default symmetric costs,
<code>levenshtein(a, b)</code> equals <code>levenshtein(b, a)</code>. As soon as
<code>insertion_cost != deletion_cost</code> that stops being true: the
directional reading is "the cost of turning <code>source</code> into
<code>target</code>". Argument order also sizes the row buffers — they are
<code>len(target) + 1</code> long — so with symmetric costs, passing the shorter
string as <code>target</code> allocates less.
</div>

## `jaro_winkler::Options`

| Field | Type | Default | Effect |
|---|---|--:|---|
| `ignore_case` | `bool` | `false` | lower-cases both inputs before comparing (after the identity short-circuit, which sees the originals) |
| `dj` | `Option<f64>` | `None` | a precomputed Jaro similarity; supplying it skips the `O(nm)` matching pass entirely |

`dj` is a genuine optimisation when you already hold the Jaro score — re-scoring
the same pair under several prefix policies, say — and a footgun otherwise,
because nothing checks that the number you supply is the Jaro similarity of
these two strings. `jaro` itself takes no options: it is `jaro(s1, s2) -> f64`.

```rust
use verbora_distance::jaro_winkler::{Options, jaro, jaro_winkler};

fn main() {
    let folding = Options { ignore_case: true, dj: None };
    assert_eq!(jaro_winkler("MARTHA", "martha", &folding), 1.0);

    let precomputed = Options { ignore_case: false, dj: Some(jaro("MARTHA", "MARHTA")) };
    assert_eq!(
        jaro_winkler("MARTHA", "MARHTA", &precomputed),
        jaro_winkler("MARTHA", "MARHTA", &Options::default())
    );
}
```

## `SearchResult`, and the offset that really is negative

| Field | Type | Meaning |
|---|---|---|
| `substring` | `String` | the best-matching substring of the **target**, owned |
| `distance` | `f64` | the edit distance from `source` to that substring |
| `offset` | `isize` | the substring's start in the target, **in UTF-16 code units**, signed |

`offset` is `isize` and that is not defensive typing. When the parent backtrace
exits through column 0, `col - 1 == -1` is a real, reachable value, and Verbora
reports it as-is. The matching `substring` is then sliced the way a negative
index counts from the *end* of the target rather than being clamped.

```rust
use verbora_distance::levenshtein::{Options, damerau_levenshtein_search};

fn main() {
    let opts = Options { deletion_cost: 2.0, ..Options::default() };
    let r = damerau_levenshtein_search("ab", "ba", &opts);

    assert_eq!(r.offset, -1);       // genuinely negative, not clamped
    assert_eq!(r.substring, "a");   // "ba" sliced from index -1 (the last character)
    assert_eq!(r.distance, 1.0);
}
```

**`levenshtein_search` cannot return a negative offset.** Its only parents are
insert `(r, c-1)`, delete `(r-1, c)` and substitute `(r-1, c-1)`, none of which
can land in column 0. The Damerau variants can: a restricted transposition
parents to `(r-2, c-2)` and an unrestricted one to `(lrm-1, lcm-1)`, either of
which reaches column 0 from column 2.

<div class="callout callout-warn">
<strong>Prefer <code>substring</code> to re-slicing the target.</strong> It is
already the matched text, cut with the negative-index slice semantics above,
which ordinary Rust indexing will not reproduce. If you must use
<code>offset</code> as an index, remember it is a UTF-16 index — a Rust byte
index only when the target is ASCII — and handle the negative case explicitly
with <code>usize::try_from</code>.
</div>

## The `StringMetric` trait

`verbora_core::StringMetric` is the generic seam:

```rust  ignore
pub trait StringMetric {
    const IS_SIMILARITY: bool;
    fn measure(&self, a: &str, b: &str) -> f64;
}
```

It deliberately does **not** normalise direction. `IS_SIMILARITY` records which
convention a metric uses so generic code can adapt, while each metric keeps its
own documented output — see
[Direction and range differ per metric](#direction-and-range-differ-per-metric).
`verbora-distance` provides five marker types implementing it, all
`Debug + Clone + Copy + Default`:

| Type | `IS_SIMILARITY` | Shape | Wraps |
|---|:--:|---|---|
| `Levenshtein(pub levenshtein::Options)` | `false` | tuple struct | `levenshtein` |
| `DamerauLevenshtein(pub levenshtein::Options)` | `false` | tuple struct | `damerau_levenshtein` |
| `JaroWinkler(pub jaro_winkler::Options)` | `true` | tuple struct | `jaro_winkler` |
| `Dice` | `true` | unit struct | `dice_coefficient` |
| `Hamming { pub ignore_case: bool }` | `false` | named field | `hamming`, cast to `f64` |

There is no marker for `jaro` or for the two search functions: call `jaro`
directly when you want the unboosted score, and search returns a `SearchResult`
rather than an `f64`, which does not fit `measure`'s signature.

**Use the marker types when the metric is a parameter**; use the free functions
when it is not. A call site that always computes Levenshtein gains nothing from
`Levenshtein::default().measure(a, b)` over `levenshtein(a, b, &opts)`.

```rust
use verbora_core::StringMetric;
use verbora_distance::{Hamming, JaroWinkler, Levenshtein};

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

    // Options travel inside the marker; Hamming's carries a flag instead.
    let folding = Hamming { ignore_case: true };
    assert_eq!(folding.measure("ABC", "abc"), 0.0);
    assert_eq!(folding.measure("abc", "ab"), -1.0); // the sentinel survives the cast
}
```

<div class="callout callout-warn">
<strong>Careful.</strong> <code>Hamming</code>'s <code>measure</code> returns
<code>-1.0</code> for incomparable input, and <code>Dice</code>'s returns
<code>NaN</code> for two empty strings. Generic code over
<code>StringMetric</code> must handle both, because the trait has no "no answer"
representation — which is why the <code>best()</code> example filters
<code>NaN</code>.
</div>

## The `units` module

Every metric here indexes text by UTF-16 code unit, because that choice is
*observable in the results*: any character outside the Basic Multilingual Plane
counts as **two**, so a char-based implementation disagrees on astral input.

```text
levenshtein("a😀b", "ab")
  UTF-16 code units : 2   (lengths 4 and 2; two surrogate halves deleted)
  Rust chars        : 1   (lengths 3 and 2; one character deleted)
```

`verbora_distance::units` is the mechanism, and it is public so you can use the
same fast path in your own code. Each algorithm is written once, generically
over `&[T]`, and `dispatch` picks the narrowest representation *provably
identical* to UTF-16 for the given input: `&[u8]` when both operands are ASCII
(no allocation), one `Vec<u16>` per operand otherwise. Both operands are
promoted together, since a `&[u8]` view cannot be compared against a `&[u16]`
one.

```rust
use verbora_distance::levenshtein;
use verbora_distance::units::{Operands, dispatch, utf16_len};

fn main() {
    assert_eq!(utf16_len("a😀b"), 4);        // UTF-16 code-unit length
    assert_eq!("a😀b".chars().count(), 3);   // Rust's idea of length
    assert_eq!(levenshtein("a😀b", "ab", &Default::default()), 2.0);

    assert!(dispatch("kitten", "sitting", |ops| matches!(ops, Operands::Bytes(..))));
    // One non-ASCII operand promotes the pair to UTF-16.
    assert!(dispatch("kitten", "café", |ops| matches!(ops, Operands::Units(..))));
}
```

The byte path is narrower in a second way. Unrestricted Damerau needs a map
from symbol to "last row this symbol appeared in", and the `Unit` trait picks
the representation: `u8` gets `ByteMap`, a flat `[usize; 256]` where a lookup is
one indexed load and nothing is hashed; `u16` falls back to an
`FxHashMap<u16, usize>`. `UnitMap<T>` (`get`, `set`, `clear`) is the shared
interface. `utf16_len` itself never allocates — it returns `s.len()` for ASCII
and otherwise sums `char::len_utf16`, so it is the cheap pre-filter before an
expensive comparison.

## Performance characteristics

Verbora picks the cheapest structure — and, wherever a faster *algorithm*
exists, the fastest algorithm — that can answer the question actually asked. A
full `(n+1) × (m+1)` matrix survives in one place only: search mode, where the
backtrace requires it.

| Mode | Working set | Why |
|---|---|---|
| distance, no Damerau, unit cost | **bit-vector** (one `u64` word per 64 units of the shorter operand) | Myers'/Hyyrö's bit-parallel algorithm answers in `O(nm/64)` bitwise operations; single-word path for 1–64 units, blocks beyond |
| distance, no Damerau, weighted costs | **1 row** | each cell needs only `up`, `left`, `diag` |
| distance, restricted Damerau, unit cost | **bit-vector** (word + block) | Hyyrö's 2003 transposition extension of Myers computes OSA in the same style |
| distance, restricted Damerau, weighted costs | **3 rows** | a transposition reaches back to row − 2 |
| distance, unrestricted Damerau | **2 rows + per-symbol row snapshots** | a transposition reaches an arbitrary earlier row, so each symbol's last matching row is snapshotted into an arena (`u16` cells while the combined length fits, `u32` beyond) |
| search, no Damerau, unit cost | per-column bit-vector deltas | Myers' state recovers the required cell costs |
| search, otherwise | full matrix | weighted and Damerau search retain matrix state for backtracking |

Where the full matrix *is* required it is stored struct-of-arrays — costs in one
flat `Vec<f64>`, parents in another `Vec<(u32, u32)>` — so the hot cost sweep is
contiguous and the parents, touched only during backtracking, never pollute the
cache line during the inner loop.

### Measured performance

Criterion timings on one development machine (Intel i9-14900KF, rustc 1.97.1,
`--release`). Treat the exact figures as machine-dependent and the shape of the
table as the reproducible part.

| Workload | Time |
|---|--:|
| `levenshtein`, 4-character ASCII pair | 14.7 ns |
| `levenshtein`, 1024-character ASCII pair | 29.08 µs |
| `levenshtein`, 256-character Cyrillic pair | 3.97 µs |
| `levenshtein`, 64-character pair, unit costs | 166.1 ns |
| `damerau_levenshtein` (`restricted: true`), 64 characters | 179.4 ns |
| `damerau_levenshtein` (`restricted: false`), 64 characters | 7.75 µs |
| `levenshtein_search`, 64 characters | 12.79 µs |
| `jaro_winkler`, 4 characters | 15.3 ns |
| `jaro_winkler`, 1024 characters | 10.34 µs |
| `dice_coefficient`, 1024 characters | 10.61 µs |
| `hamming`, 4 characters | 6.6 ns |
| `hamming`, 1024 characters | 275.3 ns |

Three things that table is worth reading for:

- **Length costs far less than the `O(nm)` label suggests, at unit costs.** A
  1024-character Levenshtein pair costs 29.08 µs because the bit-parallel kernel
  does `O(nm/64)` bitwise operations instead of `O(nm)` cell updates. Weighted
  costs fall back to the scalar DP, which does scale as the label says.
- **`restricted: true` is the cheap Damerau.** At 64 characters, 179.4 ns
  against 7.75 µs — a 43× difference on the same input, for a metric most
  callers do not need.
- **Search is the expensive mode**, and the only one that builds a matrix: 12.79
  µs at 64 characters against 166.1 ns for the same-size distance-only call. Ask
  for a position only when you need one.

`jaro`/`jaro_winkler` keep their two match-flag arrays on the stack for operands
up to 128 code units and only spill to `Vec<bool>` above that; the threshold is
measured, and words are short by nature, so the stack path is the common case.

Reproduce with:

```text
python3 tools/bench-data/generate.py       # shared inputs (run once)
cargo bench -p verbora-distance
```

The full 26-benchmark table is in
[Benchmarks: distance](../benchmarks/distance.md).

## Allocation behaviour

No function in this crate exposes a scratch buffer, an `_into` variant, or any
form of reuse. Each call allocates what it needs and drops it.

| Call | Allocations |
|---|---|
| `levenshtein` (unit-cost, ASCII) | bit-vector state and Peq table; no DP row allocation |
| `levenshtein` (weighted, ASCII) | one `Vec<f64>` of `len(target) + 1` |
| `levenshtein` with long non-ASCII operands | may add two `Vec<u16>`, one per operand; short operands use stack buffers |
| `damerau_levenshtein`, `restricted: true` (ASCII) | three `Vec<f64>` of `len(target) + 1` |
| `damerau_levenshtein`, `restricted: false` (ASCII) | two integer rows plus a per-symbol snapshot arena — no matrix |
| `levenshtein_search` / `damerau_levenshtein_search` (ASCII) | the full matrix, plus one `String` for `substring` |
| `jaro` / `jaro_winkler` (ASCII, ≤ 128 units, no folding) | **none** |
| `jaro` / `jaro_winkler` (either operand > 128 units) | one `Vec<bool>` per over-long operand |
| `jaro_winkler` with `ignore_case: true` | two `String`s from `to_lowercase`, plus the above |
| `dice_coefficient` (any input) | two `String`s, two `Vec<u16>`, two `FxHashSet<(u16, u16)>` |
| `hamming` / `hamming_checked` (ASCII, no folding) | **none** |
| `hamming` / `hamming_checked` with `ignore_case: true` | two `String`s from `to_lowercase` |

Three details worth knowing: **unrestricted Damerau in distance mode does not
allocate the matrix** (only the search functions build it); **`jaro_winkler` on
non-ASCII input converts to UTF-16 twice**, once inside `jaro` and once inside
the shared-prefix scan, because each calls `dispatch` independently; and
**`dice_coefficient` never takes the ASCII path**, since its bigram keys are
`(u16, u16)` pairs.

For the general treatment see [Allocation](../performance/allocation.md).

## Unicode and language notes

- **Lengths and indices are UTF-16 code units** everywhere in this crate: the
  distance between `"a😀b"` and `"ab"` is 2, `hamming` compares
  `utf16_len`-equal strings, `SearchResult::offset` is a UTF-16 index, and
  Dice's bigrams are code-unit pairs.
- **`substring` may contain U+FFFD.** A search slice can split a surrogate pair;
  lossy decoding maps the lone surrogate to the replacement character.
- **Case folding is `str::to_lowercase`**, full Unicode lowercasing, which can
  change length. `hamming` compares lengths *before* folding and `jaro_winkler`
  short-circuits on equality *before* folding, so both orderings are observable.
- **Dice's whitespace rule is a specific `\s`-equivalent class**, not Rust's
  `char::is_whitespace`. `verbora_core::is_whitespace` is the shared predicate;
  the two sets are not interchangeable.
- **No normalisation is applied.** `"café"` composed (`e` + U+0301) and
  precomposed (U+00E9) are different strings with a non-zero distance. Normalise
  upstream if that matters.

## Three specified surprises

Three results look like bugs. All three are specified, pinned by the test
suite, and safe to build on — but you have to guard for them at the call site.

| Result | Why | What to do |
|---|---|---|
| `dice_coefficient("", "")` is `NaN` | two empty strings produce no bigrams, so the computation is `0 / 0` | check `is_nan()` before ranking |
| `hamming` returns `-1` on a length mismatch | the sentinel is part of the contract, and it sorts *below* every real distance | use `hamming_checked`, which maps it to `None` |
| `jaro_winkler("A", "a", ignore_case)` is `0.4`, not `1.0` | the prefix loop treats a read past the end of either string as "no character", and two of those compare equal, so the counter saturates at 4; for one-character inputs the match window is `−1`, Jaro is 0, and the boost is fully exposed: `0 + 4 × 0.1 × (1 − 0)` | expect it only at length 1 — `jaro_winkler("AB", "ab")` is `1.0` |

```rust
use verbora_distance::hamming_checked;
use verbora_distance::jaro_winkler::{Options, jaro_winkler};
use verbora_distance::dice_coefficient;

fn main() {
    let score = dice_coefficient("", "");
    assert_eq!(if score.is_nan() { 0.0 } else { score }, 0.0);

    // filter_map drops the incomparable pair instead of sorting it to the front.
    let mut scored: Vec<(&str, u64)> = ["kathrin", "kadolin", "short"]
        .iter()
        .filter_map(|c| hamming_checked("karolin", c, false).map(|d| (*c, d)))
        .collect();
    scored.sort_by_key(|(_, d)| *d);
    assert_eq!(scored[0].0, "kadolin");
    assert_eq!(scored.len(), 2);

    let folding = Options { ignore_case: true, dj: None };
    assert_eq!(jaro_winkler("A", "a", &folding), 0.4); // not 1.0
    assert_eq!(jaro_winkler("AB", "ab", &folding), 1.0);
}
```

## Common mistakes

1. **Mixing directions in one ranking.** Sorting ascending is right for
   Levenshtein and Hamming and exactly backwards for Jaro–Winkler and Dice. Use
   `StringMetric::IS_SIMILARITY` when the metric is a parameter.
2. **Sorting on `hamming` without filtering `-1`.** Incomparable pairs land at
   the top. Use `hamming_checked`.
3. **Reducing over Dice scores without a `NaN` guard.** `min_by`/`max_by` with a
   partial comparison silently mis-order.
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
   (`_into`) API, but the `parallel` feature adds a per-pair batch fan-out with
   no sibling for `levenshtein_search`, `damerau_levenshtein_search`, plain
   `jaro` or `hamming_checked`.

## Related

- [Choosing a distance API](../choosing/distance.md) — which metric for which
  problem, and how to run millions of comparisons without an index.
- [Core traits](./core.md) — `StringMetric` and the rest of the shared
  vocabulary.
- [Phonetics](./phonetics.md) — sound-alike keys, the usual partner for a
  distance metric in name matching.
- [Performance](../performance/index.md) ·
  [Allocation](../performance/allocation.md) ·
  [Parallelism](../performance/parallelism.md)
- [Benchmarks: distance](../benchmarks/distance.md) — the full measured results.
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
