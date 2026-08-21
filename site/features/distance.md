# String distance and similarity

`verbora-distance` answers one question in seven ways: *how far apart are these
two strings?* It implements seven distance and similarity metrics —
Levenshtein, unrestricted Damerau–Levenshtein and optimal string alignment
(each in a unit-cost and a weighted tier, and each of those in a scalar and a
substring-search flavour), Jaro, Jaro–Winkler, the Sørensen–Dice coefficient,
and Hamming — with exact, fully specified results, down to the edge cases most
implementations leave undefined (see [Specified edge cases](#specified-edge-cases)).

<div class="callout callout-spec">
<strong>Specification status.</strong> All <strong>7</strong> distance APIs are
documented and test-pinned, including every degenerate input this page
describes. Four properties hold for every similarity on every input: the result
is finite, it lies in <code>0.0..=1.0</code>, <code>f(x, x)</code> is exactly
<code>1.0</code>, and <code>f(a, b)</code> and <code>f(b, a)</code> are
bit-identical. No function in this crate panics, on any input.
<code>cargo test -p verbora-distance</code> runs
<strong>183</strong> unit tests and <strong>36</strong> doctests.
</div>

## When to use it

- **Fuzzy matching and typo correction.** Ranking candidate spellings, command
  names, or dictionary entries against a possibly-misspelled query.
- **Record linkage and deduplication.** Deciding whether "Jonathon Smith" and
  "Jonathan Smyth" are the same person.
- **Approximate substring location.** Finding where in a longer document a
  short phrase most nearly occurs (`levenshtein_search`), with a byte range you
  can slice or highlight directly.
- **Fixed-width code comparison.** Counting differing positions in equal-length
  identifiers, checksums or binary strings (`hamming`).
- **Deterministic, fully specified results.** Every metric's behavior —
  including the edge cases most implementations leave undefined — is documented
  and test-pinned, and no function consults a Unicode character database, so
  results are frozen across Unicode versions. That matters for anything that
  persists distances or distance-derived keys.

## When not to use it

- **Semantic similarity.** These are all *surface* metrics. "car" and
  "automobile" are maximally distant under every one of them.
- **Whole-document comparison at scale.** Levenshtein is `O(nm)` in time, and
  search mode is expensive in *memory*: unit-cost `levenshtein_search` keeps two
  64-bit words per column of the target for every 64 units of the source, and
  every other search variant keeps a full `(n+1) × (m+1)` matrix of cost and
  parent cells — a 10-unit needle against a 100,000-unit haystack is `11 ×
  100,001` cells at 16 bytes each, about 17 MB, for a single call.
- **Indexed search over a large corpus.** This crate ships no index. Its
  `parallel`-gated batch fan-out scores many pairs you already know about, and
  `PreparedPattern` makes each comparison against a fixed query cheaper, but
  every candidate still costs a comparison — a nearest-neighbour search over
  this crate alone is `O(corpus size)` per query. To rule candidates *out*
  instead, reach for `verbora-spellcheck`'s [`FuzzyIndex`](./spellcheck.md)
  (a BK-tree over edit distance) or a phonetic bucket. See
  [Choosing a distance API](../choosing/distance.md#repeated-and-bulk-comparison).
- **Sound-alike matching.** For "Smith" ≈ "Smyth" as *pronunciation* rather
  than spelling, reach for [phonetics](./phonetics.md) first and use a distance
  metric to break ties.
- **Case-insensitive or whitespace-insensitive matching, out of the box.** No
  metric here rewrites its inputs. Fold and normalise once at ingestion —
  `levenshtein(&a.to_lowercase(), &b.to_lowercase())` — rather than re-folding
  the query against every candidate.

## Quick example

```rust
use verbora_distance::{dice_coefficient, hamming, jaro_winkler, levenshtein};

fn main() {
    // Distances: lower is closer.
    assert_eq!(levenshtein("kitten", "sitting"), 3);
    assert_eq!(hamming("karolin", "kathrin"), Some(3));

    // Similarities: higher is closer.
    assert_eq!(dice_coefficient("abc", "abc"), 1.0);
    assert_eq!(jaro_winkler("abc", "abc"), 1.0);
}
```

## Direction and range differ per metric

This is the first thing to internalise, and the most common source of a
silently inverted ranking. Some metrics are distances (lower is closer), some
are similarities (higher is closer), and mixing them in one ranking inverts it.

| Metric | Return type | Range | Direction | Identical inputs |
|---|---|---|---|---|
| `levenshtein` | `usize` | `0..` (unbounded) | **distance** — lower is closer | `0` |
| `damerau_levenshtein` | `usize` | `0..` (unbounded) | **distance** — lower is closer | `0` |
| `osa` | `usize` | `0..` (unbounded) | **distance** — lower is closer | `0` |
| `levenshtein_weighted` and its two siblings | `f64` | `0.0..` (unbounded) | **distance** — lower is closer | `0.0` |
| `hamming` | `Option<usize>` | `Some(0..)`, or `None` | **distance** — lower is closer | `Some(0)` |
| `jaro` | `f64` | `0.0..=1.0` | **similarity** — higher is closer | `1.0` |
| `jaro_winkler` | `f64` | `0.0..=1.0` | **similarity** — higher is closer | `1.0` |
| `dice_coefficient` | `f64` | `0.0..=1.0` | **similarity** — higher is closer | `1.0` |

Direction is recorded by the name of the function you call. Code that ranks
candidates picks `min` for a distance and `max` for a similarity at the call
site, where the metric is already chosen.

<div class="callout callout-note">
<strong>Every similarity is total.</strong> <code>jaro</code>,
<code>jaro_winkler</code> and <code>dice_coefficient</code> return a finite
<code>f64</code> for every input, so <code>total_cmp</code>, <code>max_by</code>
and <code>sort_by</code> over their results are well defined and
order-independent. No metric here returns <code>NaN</code>, and none carves a
sentinel out of its numeric range: <code>hamming</code>'s one absent answer is
<code>None</code>.
</div>

## Choosing the right API

| API | Answers | Returns | Cost (time) | Working set | Allocation-free |
|---|---|---|---|---|:--:|
| `levenshtein` | how many single-character edits? | `usize` | `O(nm/64)` bitwise | bit-vector | ✅ (ASCII ≤ 64 units) |
| `osa` | …counting an adjacent swap as one edit, with nothing edited between two swaps | `usize` | `O(nm/64)` bitwise | bit-vector | ✅ (ASCII ≤ 64 units) |
| `damerau_levenshtein` | …counting a swap as one edit however far apart the pair ends up | `usize` | `O(nm)` | Zhao–Sahni rolling rows | ✅ (ASCII ≤ 8 units after the affix trim) |
| `*_weighted` | the same three questions, with operations priced individually | `f64` | `O(nm)` | 1 row / 3 rows / full matrix | ❌ |
| `levenshtein_search` | *where* in the target does the source best occur? | `SearchResult<'t, usize>` | `O(nm/64)` bitwise | per-column bit-vector deltas | ❌ |
| `damerau_levenshtein_search` / `osa_search` | same, tolerating transpositions | `SearchResult<'t, usize>` | `O(nm)` | full matrix | ❌ |
| `*_search_weighted` | same, with operations priced individually | `SearchResult<'t, f64>` | `O(nm)` | full matrix | ❌ |
| `jaro` | how many characters match within a sliding window? | `f64` | `O(nm)` | windowed greedy loop, or bit-parallel match masks | ✅ (ASCII, short operands) |
| `jaro_winkler` | …with a bonus for a shared prefix | `f64` | `O(nm)` | as `jaro`, plus a prefix scan bounded at 4 units | ✅ (ASCII, short operands) |
| `dice_coefficient` | how much do the bigram *sets* overlap? | `f64` | `O(n + m)` expected | 2 hash sets | ❌ |
| `hamming` | how many positions differ? | `Option<usize>` | `O(n)` | none | ✅ (always) |
| `PreparedPattern` | the same as `levenshtein` and `osa`, for one fixed pattern against many targets | `usize` per query | `O(nm/64)` bitwise | the bit-vector kernels' match table, built once at construction | ✅ (ASCII pattern ≤ 64 units vs. ASCII target) |

`n` and `m` are lengths **in Unicode scalar values**, not bytes. See
[Unicode and language notes](#unicode-and-language-notes).

### Which one

| Your situation | Use |
|---|---|
| Equal-length codes, hashes, fixed fields | `hamming` → `Option<usize>` |
| Typos, and a swap is honestly two edits | `levenshtein` |
| Typos, adjacent swaps cost 1, swapped characters never edited again | `osa` (optimal string alignment) |
| Typos, swaps may be arbitrarily far apart | `damerau_levenshtein` |
| Any of the three edit distances above, but insertions, deletions and swaps have genuinely different prices | the `_weighted` sibling of whichever you picked |
| The *position* of the best approximate occurrence in a longer string | `levenshtein_search` / `damerau_levenshtein_search` / `osa_search` |
| Names or short records, shared prefix is meaningful | `jaro_winkler` (or `jaro` for the unboosted score) |
| Shared content, order and position irrelevant | `dice_coefficient` |
| One fixed query compared against many candidates, under `levenshtein` or `osa` | `PreparedPattern` |

### Unit costs and weighted costs

Each of the three Levenshtein-family algorithms is published twice: a
**unit-cost** function that takes no cost argument and returns `usize`, and a
**weighted** function that takes a validated cost set and returns `f64`.

Unit cost is not a cost *value* here; it is the absence of an argument. There is
no way to spell "unit costs" as a value, because the unit metric is a different
function — a different kernel (bit-parallel rather than a scalar dynamic
program), a different complexity class, and a different result type.

| | `levenshtein`, `damerau_levenshtein`, `osa` | `levenshtein_weighted`, `damerau_levenshtein_weighted`, `osa_weighted` |
|---|---|---|
| Question answered | how many edits separate the two strings | the minimum total cost of an edit script under caller-assigned prices |
| Cost argument | none | `LevenshteinCosts` / `DamerauCosts` / `OsaCosts`, each built through a `new` returning `Result` |
| Returns | `usize` — exact, `Ord`, `Hash`, no rounding, usable as a map key | `f64` |
| Kernel | bit-parallel: Myers (1999), Hyyrö (2003), Zhao–Sahni (2020) | scalar dynamic program |
| Common-affix trimming | yes — a pair differing in one interior position collapses to `O(1)` kernel work | no; the reduction's proof assumes a unit-cost matrix |
| `PreparedPattern` | yes | no — the prepared table is a bit-parallel structure |
| Can the caller get it wrong? | no — there is no argument to get wrong | an inadmissible cost set is rejected by the constructor, before any metric sees it |

The boundary is **unit versus weighted**, never integer versus float:
`substitution: 2.0` is exactly as slow as `substitution: 0.5`, because the
bit-parallel kernels have no notion of a weighted operation at all. Reach for
the unit form unless you can name the prices — and the two agree where they
overlap, bit for bit.

### `levenshtein`

Eager, returns `usize`, no scratch or `_into` API — though a fixed pattern can
be prepared once and reused, see [`PreparedPattern`](#preparedpattern). It takes
a Myers/Hyyrö bit-vector fast path at every length — one `u64` word for operands
up to 64 units, a block extension beyond — answering in `O(nm/64)` bitwise
operations rather than `O(nm)` scalar cell updates. That single choice is what
makes long operands cheap; see [Measured performance](#measured-performance).

```rust
use verbora_distance::levenshtein;

fn main() {
    assert_eq!(levenshtein("kitten", "sitting"), 3);
    assert_eq!(levenshtein("", "abc"), 3);
    assert_eq!(levenshtein("same", "same"), 0);

    // One Unicode scalar is one unit, so deleting an emoji is one edit.
    assert_eq!(levenshtein("a😀b", "ab"), 1);
}
```

The **length lemma** is part of the contract, because callers build screening
gates on it: `|a.chars().count() - b.chars().count()| <= levenshtein(a, b)`,
since each insertion or deletion changes the scalar count by exactly one and
each substitution by zero. Under weighted costs the bound becomes
`min(insertion, deletion) * |Δ|`.

`levenshtein_weighted` prices each operation separately and falls back to a
rolling row plus two scalar temporaries — never a full matrix.

```rust
use verbora_distance::{LevenshteinCosts, levenshtein, levenshtein_weighted};

fn main() {
    let costs = LevenshteinCosts::new(1.0, 3.0, 1.0).unwrap();

    // Deleting the "c" costs 3; inserting it still costs 1.
    assert_eq!(levenshtein_weighted("abc", "ab", &costs), 3.0);
    assert_eq!(levenshtein_weighted("ab", "abc", &costs), 1.0);

    // The two tiers agree where they overlap.
    let unit = LevenshteinCosts::new(1.0, 1.0, 1.0).unwrap();
    assert_eq!(
        levenshtein("kitten", "sitting") as f64,
        levenshtein_weighted("kitten", "sitting", &unit)
    );
}
```

### `damerau_levenshtein` and `osa`

Same shape as `levenshtein`, but an adjacent swap costs one edit rather than
two. The algorithm is picked by the function you call, not by a flag — these
are two genuinely different algorithms:

| Function | Rule | Working set | Classic name |
|---|---|---|---|
| `damerau_levenshtein` | a transposition may reach back to *any* earlier row; the substring between two swapped characters may itself be edited | Zhao–Sahni's linear-space rows | unrestricted Damerau–Levenshtein (Lowrance–Wagner) |
| `osa` | a transposition may only reach row − 2, so no substring is edited between two transpositions | bit-vector | optimal string alignment (OSA) |

```rust
use verbora_distance::{damerau_levenshtein, levenshtein, osa};

fn main() {
    // A swap is two edits under Levenshtein, one under either Damerau rule.
    assert_eq!(levenshtein("ab", "ba"), 2);
    assert_eq!(damerau_levenshtein("ab", "ba"), 1);
    assert_eq!(osa("ab", "ba"), 1);

    // The two algorithms disagree on real input.
    assert_eq!(damerau_levenshtein("CA", "ABC"), 2);
    assert_eq!(osa("CA", "ABC"), 3);

    // That extra edit is where OSA's triangle inequality fails: 3 > 1 + 1.
    assert_eq!(osa("CA", "AC"), 1);
    assert_eq!(osa("AC", "ABC"), 1);

    // Unrestricted Damerau is symmetric and a true metric.
    assert_eq!(damerau_levenshtein("bb", "abbb"), 2);
    assert_eq!(damerau_levenshtein("abbb", "bb"), 2);
}
```

`damerau_levenshtein` computes the unrestricted Damerau–Levenshtein distance of
Lowrance & Wagner (JACM 22(2), 1975), and is a true metric: symmetric, and
satisfying the triangle inequality. `osa` computes optimal string alignment,
which is not a metric — forbidding an edit between two transpositions breaks the
triangle inequality, and `"CA"` / `"AC"` / `"ABC"` is the witness above:
`osa("CA", "ABC")` is 3, while the two-step route through `"AC"` costs 2.

**Prefer `osa` unless you need the true metric.** It runs on a bit-parallel
kernel, and it is the variant the name "Damerau–Levenshtein" commonly refers
to — so when a specification or a ticket asks for that, `osa` is usually the
function it means.

#### The cost precondition for unrestricted Damerau

`DamerauCosts::new` — the only cost type `damerau_levenshtein_weighted` and
`damerau_levenshtein_search_weighted` accept — returns
`Err(CostError::TranspositionBelowThreshold)` unless

```text
2 × transposition >= insertion + deletion
```

The precondition is therefore discharged at construction. **Nothing downstream
checks it, and no function panics**: an inadmissible cost set cannot reach a
metric, because there is no way to build one.

The threshold is Lowrance & Wagner's own (*An Extension of the String-to-String
Correction Problem*, JACM 22(2), 1975), not a Verbora restriction. Their
recurrence charges a transposition plus one deletion and one insertion for each
position of the span between the swapped pair, and credits at most one
transposition per matching row/column pair. That accounting is exact only while
a transposition costs at least as much as the delete/insert pair it stands in
for. Below the threshold, chains of adjacent swaps become the cheaper way to
move a character, the recurrence stops ranging over every edit script, and what
it returns is *a* script's cost rather than the minimum one. Checked against a
Dijkstra search over edit scripts: at `insertion = 1, deletion = 1,
substitution = 5, transposition = 0.999` the recurrence reports
`d("aab", "baa") = 2` where two transpositions achieve `1.998`.

**`OsaCosts` is the alternative below the threshold**, and it imposes no such
condition: optimal string alignment's recurrence *defines* its answer — the
minimum over alignments that edit no position twice — so every admissible cost
set is sound there. `LevenshteinCosts` has no transposition cost to constrain,
and unit-cost `damerau_levenshtein` evaluates `(1, 1, 1, 1)`, which satisfies
the threshold with equality (`2 ≥ 2`).

```rust
use verbora_distance::{
    CostError, DamerauCosts, OsaCosts, damerau_levenshtein_weighted, osa_weighted,
};

fn main() {
    // Admissible: 2 × 1.5 >= 1.0 + 1.0. A swap costs 1.5, two substitutions 2.0.
    let dear_swap = DamerauCosts::new(1.0, 1.0, 1.0, 1.5).unwrap();
    assert_eq!(damerau_levenshtein_weighted("ab", "ba", &dear_swap), 1.5);

    // Below the threshold: the constructor refuses, so no metric ever sees it.
    assert_eq!(
        DamerauCosts::new(1.0, 1.0, 1.0, 0.25),
        Err(CostError::TranspositionBelowThreshold { transposition: 0.25, minimum: 1.0 })
    );

    // OSA is defined for exactly that cost set.
    let cheap_swap = OsaCosts::new(1.0, 1.0, 1.0, 0.25).unwrap();
    assert_eq!(osa_weighted("ab", "ba", &cheap_swap), 0.25);
}
```

### `PreparedPattern`

Screening compares *one* string against thousands. The free functions are pure
functions of two strings, so each call rebuilds the bit-parallel kernels'
character-match table (`Peq`) from scratch — meaning a fixed query pays for its
own length once per candidate. `PreparedPattern` builds that table at
construction and answers `levenshtein` and `osa` from it.

```rust
use verbora_distance::PreparedPattern;
use verbora_distance::levenshtein;

fn main() {
    let query = PreparedPattern::new("Jonathan");

    assert_eq!(query.levenshtein("Jonathon"), 1);
    assert_eq!(query.osa("Jonahtan"), 1);

    // Identical to the free function, in that argument order.
    assert_eq!(query.levenshtein("Smith"), levenshtein("Jonathan", "Smith"));

    assert_eq!(query.pattern(), "Jonathan");
    assert_eq!(query.pattern().chars().count(), 8);
}
```

**Prepared state, not a scratch buffer.** The table is immutable memory derived
from one operand: never written during a query, so there is no reset step
between candidates and one instance serves any number of threads through a
shared `&`. That is a different thing from a scratch buffer — mutable working
memory a caller lends the algorithm for the duration of one call — which this
crate does not offer for any metric. The dynamic-programming working set is
still built per query.

**Unit costs only.** There is no weighted form here, and the absence is
structural: the prepared state *is* a bit-parallel pattern-match table, and the
bit-parallel kernels have no notion of a weighted operation. A caller with
weighted costs calls `levenshtein_weighted` or `osa_weighted` per pair; there is
nothing a prepared type could hoist for them.

**What is frozen at construction.** The free functions decide two things per
*pair* that a prepared pattern has to decide in advance, and each has a
fallback rather than a failure mode:

| Decision | Per call | Prepared | If the query does not fit |
|---|---|---|---|
| Element type | bytes if both operands are ASCII, else Unicode scalars | fixed by the pattern | a non-ASCII target against an ASCII pattern goes to the per-call function |
| Which operand is bit-packed | whichever is shorter | always the pattern | nothing — Myers' recurrence does not require the scanned operand to be the longer one, so a short target is served directly |
| Common-affix trimming | always, when it applies | not compatible with a table built over the whole pattern | a pair sharing enough of an affix goes to the per-call function, which trims |

Every fallback is literally the free function it replaces, so preparing a
pattern cannot change an answer and cannot lose to not preparing one. Which
path a query took is not observable except in timing.

**`damerau_levenshtein` has no prepared form**, and the absence is a
consequence of the algorithm rather than a gap: its unit-cost kernel is
Zhao–Sahni's linear-space recurrence, which uses no `Peq` at all. Its one table
is a last-occurrence map filled *during* the scan and keyed by the operand
being walked, so nothing in it is a function of the pattern alone. Call the
free function.

**Cost of holding one.** About 2 KB for an ASCII pattern of up to 64 units —
the flat `[u64; 256]` table stored inline rather than behind a pointer, so a
lookup is one indexed load. Patterns past 64 units and non-ASCII patterns
allocate. That is one value per fixed pattern, not one per comparison.

### `levenshtein_search`, `damerau_levenshtein_search` and `osa_search`

Search mode makes every prefix of the target a free starting point (row 0 costs
nothing), takes the minimum over the last row, then recovers where the winning
alignment began. The result **borrows the target** and reports a **byte** range
into it, so it can be used as a highlight span directly.

The search is total: the empty substring is always a candidate, so there is
always a best match and no `Option` to unwrap. "Close enough" is the caller's
threshold — `if r.distance() <= 2 { … }`.

```rust
use verbora_distance::levenshtein_search;

fn main() {
    let target = "the quick brown fox";
    let found = levenshtein_search("brwn", target);

    assert_eq!(found.substring(), "brown");
    assert_eq!(found.distance(), 1);
    assert_eq!(found.range(), 10..15);

    // The range slices the target, so highlighting is a split, not a search.
    let r = found.range();
    let (before, rest) = target.split_at(r.start);
    let (hit, after) = rest.split_at(r.len());
    assert_eq!((before, hit, after), ("the quick ", "brown", " fox"));

    // The cheapest substring need not be a word: nothing in "abc" is closer
    // to "ca" than the single "a".
    let fragment = levenshtein_search("ca", "abc");
    assert_eq!((fragment.substring(), fragment.range(), fragment.distance()), ("a", 0..1, 1));
}
```

<div class="callout callout-warn">
<strong>Careful.</strong> Search returns the cheapest <em>substring</em>, which
need not be a word or anything a human would have picked. Tie-breaking is
deterministic: the cheapest predecessor wins on a strict <code>&lt;</code>, so
the first candidate in the fixed order insert, delete, substitute, transpose
wins a tie, at the earliest end column, with the empty substring ahead of all of
them. Totals never depend on that order, but the recorded parent does — and the
parent chain is what produces the reported range and substring.
</div>

### `jaro` and `jaro_winkler`

`jaro` counts characters that match within a window of
`max(0, floor(max(n, m) / 2) - 1)` positions, then penalises the ones that
matched out of order. `jaro_winkler` adds `l × 0.1 × (1 − jaro)`, where `l` is
the shared prefix length capped at 4 and at both operand lengths. Both are
similarities: higher is closer.

```rust
use verbora_distance::{jaro, jaro_winkler};

fn main() {
    // Jaro's and Winkler's own worked examples.
    assert!((jaro("MARTHA", "MARHTA") - 17.0 / 18.0).abs() < 1e-12);
    assert!((jaro_winkler("MARTHA", "MARHTA") - 0.9611111111111111).abs() < 1e-12);
    assert!((jaro_winkler("DWAYNE", "DUANE") - 0.84).abs() < 1e-12);

    // The boost is what separates a shared prefix from a shared suffix.
    assert_eq!(jaro("abcde", "abcdz"), jaro("abcde", "zbcde"));
    assert!(jaro_winkler("abcde", "abcdz") > jaro_winkler("abcde", "zbcde"));

    // With no shared prefix the boost term is zero and the two agree exactly.
    assert_eq!(jaro_winkler("abcd", "zbcd"), jaro("abcd", "zbcd"));
}
```

Three behaviours worth knowing before you rely on the score:

- **Identical operands score exactly `1.0`, and only identical ones do.** That
  holds for `("", "")`, for single-unit operands and for astral ones, and it is
  reached through the formula rather than through an equality short-circuit —
  so `jaro` and `jaro_winkler` cannot disagree about their own identity element.
- **The match window is clamped at zero.** `floor(max/2) - 1` is negative only
  when both operands are at most one unit long, where it would prune the single
  candidate pair at displacement 0. The window is a pruning device, not a
  definition of matching, so it is clamped: `jaro("a", "a")` is `1.0` and
  `jaro("a", "b")` is `0.0`. Every input with `max(n, m) >= 2` is untouched.
- **Winkler's `p` is fixed at `0.1`**, his own value, and the boost is applied
  unconditionally. His later "only when `sim_j > 0.7`" variant introduces a
  discontinuity and is not implemented; if it ever arrives it arrives as a
  separately named function.

```rust
use verbora_distance::{jaro, jaro_winkler};

fn main() {
    assert_eq!(jaro("", ""), 1.0);
    assert_eq!(jaro("a", "a"), 1.0);
    assert_eq!(jaro("a", "b"), 0.0);
    assert_eq!(jaro("😀", "😀"), 1.0);
    assert_eq!(jaro_winkler("a", "a"), 1.0);

    // Case is significant, because no metric here rewrites its inputs.
    assert_eq!(jaro_winkler("A", "a"), 0.0);
    assert_eq!(jaro_winkler(&"A".to_lowercase(), "a"), 1.0);
}
```

Neither is a distance: the derived `1 − score` violates the triangle
inequality, so neither can seed a BK-tree. Use `levenshtein` or
`damerau_levenshtein` where a true metric is required.

### `dice_coefficient`

Dice, L. R. (1945). The coincidence index `2 |A ∩ B| / (|A| + |B|)` over the
**sets** of adjacent scalar pairs — bigrams — of the two operands. Because the
bigrams form a set, repeats collapse.

**The operands are used as given.** Case is significant, whitespace is
significant, nothing is trimmed, collapsed or padded, and `' '` is an ordinary
unit forming ordinary bigrams. Caseless matching is one call away, and it
belongs at the call site:

```rust
use verbora_distance::dice_coefficient;

fn main() {
    // Fold once, where you control the policy.
    assert_eq!(dice_coefficient(&"ABC".to_lowercase(), "abc"), 1.0);
    assert_eq!(dice_coefficient("ABC", "abc"), 0.0); // {AB,BC} ∩ {ab,bc} = ∅

    // {ni,ig,gh,ht} against {na,ac,ch,ht}: one shared bigram of 4 + 4.
    assert_eq!(dice_coefficient("night", "nacht"), 0.25);

    // Identical operands score 1.0 — two empty strings are identical, not
    // disjoint — and operands sharing no bigram score 0.0.
    assert_eq!(dice_coefficient("abc", "abc"), 1.0);
    assert_eq!(dice_coefficient("", ""), 1.0);
    assert_eq!(dice_coefficient("", "abc"), 0.0);

    // A set, so repeats collapse: both reduce to {(a, a)}.
    assert_eq!(dice_coefficient("aaaa", "aa"), 1.0);
}
```

<div class="callout callout-warn">
<strong>Dice discriminates nothing below two scalars.</strong> A one-scalar
operand yields no bigram, so it shares none:
<code>dice_coefficient("a", "ab")</code> and
<code>dice_coefficient("a", "zzz")</code> are both <code>0.0</code>. Padding a
short operand would fabricate a bigram containing a character that is not in the
input and manufacture similarity out of it, so no padding is applied. For very
short strings reach for <code>jaro_winkler</code> or <code>levenshtein</code>
instead.
</div>

Dice is the only metric here with no ASCII fast path: its bigram keys are
`(char, char)` pairs, so it never enters the crate's unit dispatch. Its cost is
dominated by hashing rather than by a quadratic sweep.

### `hamming`

Hamming distance is the number of positions at which two **equal-length**
sequences differ (Hamming, 1950, §2). It is not defined for sequences of
unequal length, so `hamming` returns `Option<usize>`: `Some(d)` when the two
operands have the same number of Unicode scalar values, `None` when they do not.

```rust
use verbora_distance::hamming;

fn main() {
    assert_eq!(hamming("karolin", "kathrin"), Some(3));
    assert_eq!(hamming("", ""), Some(0));

    // Comparability is decided by scalar count, never by byte length.
    assert_eq!(hamming("😀", "𝕳"), Some(1));   // one scalar each
    assert_eq!(hamming("a😀b", "abc"), Some(2)); // three scalars each
    assert_eq!(hamming("a😀b", "abcd"), None);   // three scalars vs. four

    // Case is significant; fold once, at the call site.
    assert_eq!(hamming("ABC", "abc"), Some(3));
    assert_eq!(hamming(&"ABC".to_lowercase(), "abc"), Some(0));
}
```

`None` is absence, not a value, so it cannot out-rank a real answer. Screening a
candidate list is the shape `Option` is for — an incomparable candidate drops
out instead of sorting to the front:

```rust
use verbora_distance::hamming;

fn main() {
    let mut scored: Vec<(&str, usize)> = ["kathrin", "kadolin", "short"]
        .into_iter()
        .filter_map(|c| hamming("karolin", c).map(|d| (c, d)))
        .collect();
    scored.sort_by_key(|&(_, d)| d);

    assert_eq!(scored[0].0, "kadolin");
    assert_eq!(scored.len(), 2); // "short" is incomparable and never ranked
}
```

`hamming` never allocates, on any input. It guarantees identity
(`hamming(x, x) == Some(0)`), discernibility (`Some(0)` iff the operands are
equal), symmetry, the bound `Some(d) ⟹ d <= a.chars().count()`, the triangle
inequality over comparable triples, and `Some(d) ⟹ levenshtein(a, b) <= d`.

### Parallel batch (`par_*_batch`, feature `parallel`)

<a class="badge badge-batch" href="../performance/batch-vs-streaming">BATCH</a>

Behind the `parallel` Cargo feature (never on by default), six functions fan a
slice of pairs out across a `rayon` thread pool at **per-pair** granularity.
Each is exactly `pairs.par_iter().map(<the sequential function>).collect()` —
never a second implementation — and each preserves input order, with its own
sequential-vs-parallel equivalence test.

| Function | Wraps | Returns |
|---|---|---|
| `par_levenshtein_batch` | `levenshtein` | `Vec<usize>` |
| `par_damerau_levenshtein_batch` | `damerau_levenshtein` | `Vec<usize>` |
| `par_osa_batch` | `osa` | `Vec<usize>` |
| `par_jaro_winkler_batch` | `jaro_winkler` | `Vec<f64>` |
| `par_dice_coefficient_batch` | `dice_coefficient` | `Vec<f64>` |
| `par_hamming_batch` | `hamming` | `Vec<Option<usize>>` |

There is no wrapper for plain `jaro`, for any weighted metric, or for any of the
six search functions; apply the same `par_iter().map(...)` pattern yourself if
you need them in parallel (see
[Parallelism](../performance/parallelism.md)). The weighted path is strictly
heavier per pair, so the crossover at which parallelism wins is *earlier* than
for the unit form — the guidance below is conservative for it.

No batch function panics, for any input, including an empty `pairs`: there is no
cost set to reject.

```rust  ignore
use verbora_distance::par_levenshtein_batch;

let pairs = [("kitten", "sitting"), ("", "abc")];
assert_eq!(par_levenshtein_batch(&pairs), [3, 3]);
```

<div class="callout callout-note">
<strong>Note.</strong> This block needs the <code>parallel</code> feature, which
this site's own snippet checker builds without, so it is marked
<code>ignore</code> rather than compiled — every other block on this page
compiles and runs in CI.
</div>

**The crossover differs sharply per metric**, because each metric's per-pair
cost has to clear `rayon`'s roughly-one-microsecond scheduling cost before a
batch call is worth it.

<div class="callout callout-warn">
<strong>Pending re-measurement.</strong> The speed-ups below are not current
figures. They are retained only until a fresh full-precision run replaces them;
reproduce with
<code>cargo bench -p verbora-distance --features parallel -- par_&lt;name&gt;</code>
before relying on them for capacity planning.
</div>

| Batch function | 4 chars | 64 chars | 1024 chars |
|---|--:|--:|--:|
| `par_levenshtein_batch` / `par_damerau_levenshtein_batch` (300 pairs) | 0.6× | 15.6× | 22.0× |
| `par_jaro_winkler_batch` (1000 pairs) | 0.16× | 7.9× | 15.6× |
| `par_dice_coefficient_batch` (1000 pairs) | 2.0× | 10.3× | — |
| `par_hamming_batch` (1000 pairs) | 0.12× | 0.12×–1× | 1.7× from 256 chars |

The shape is the durable part: `hamming` is nearly free per short pair, so it
loses until pairs are long, while `dice_coefficient` hashes two bigram sets per
call and is heavier than scheduling at every length tested.

## Cost sets

Three types, one per algorithm, so a transposition cost cannot be handed to a
function that would discard it. Each is built through a `new` that returns
`Result`, and each is `Debug + Clone + Copy + PartialEq` (not `Eq` — they hold
`f64`).

| Type | Fields | Accepted by | Extra condition |
|---|---|---|---|
| `LevenshteinCosts` | insertion, deletion, substitution | `levenshtein_weighted`, `levenshtein_search_weighted` | — |
| `OsaCosts` | insertion, deletion, substitution, transposition | `osa_weighted`, `osa_search_weighted` | — |
| `DamerauCosts` | insertion, deletion, substitution, transposition | `damerau_levenshtein_weighted`, `damerau_levenshtein_search_weighted` | `2 × transposition >= insertion + deletion` |

A cost is the price of one edit operation on one unit: `insertion` per unit of
`target` not matched from `source`, `deletion` per unit of `source` not matched
into `target`, `substitution` per aligned pair of differing units,
`transposition` per swap of two adjacent units. Matching a unit against itself
is always free and is never a substitution.

**A cost is admissible when it is finite and non-negative.** Zero is
admissible — `LevenshteinCosts::new(0.0, 0.0, 0.0)` prices every edit script at
`0.0` by construction, making the result a pseudometric rather than a metric.
`NaN`, infinities and negative values are rejected, because a "distance" of
`-4.0` between a string and itself is not a distance.

`CostError` is the rejection, and every variant carries the offending value:
`NotFinite { operation, value }`, `Negative { operation, value }`, and
`TranspositionBelowThreshold { transposition, minimum }`. It implements
`Display` and `std::error::Error`. `Operation` — `Insertion`, `Deletion`,
`Substitution`, `Transposition` — names which cost was at fault.

There is deliberately no `Default` on any cost type, and no conversion between
them: the one cost set everybody wants is reachable *without* a cost type at
all, by calling `levenshtein`, `damerau_levenshtein` or `osa`.

```rust
use verbora_distance::{
    CostError, LevenshteinCosts, Operation, levenshtein_weighted,
};

fn main() {
    let asymmetric = LevenshteinCosts::new(1.0, 3.0, 1.0).unwrap();
    // Deleting `c` costs 3; inserting it still costs 1.
    assert_eq!(levenshtein_weighted("abc", "ab", &asymmetric), 3.0);
    assert_eq!(levenshtein_weighted("ab", "abc", &asymmetric), 1.0);

    // Fractional costs work as expected, and zero is admissible.
    let fractional = LevenshteinCosts::new(0.5, 1.5, 0.75).unwrap();
    assert_eq!(levenshtein_weighted("ab", "abc", &fractional), 0.5);
    assert!(LevenshteinCosts::new(0.0, 0.0, 0.0).is_ok());

    // Negative and non-finite costs are refused, with the value that failed.
    assert_eq!(
        LevenshteinCosts::new(1.0, -1.0, 1.0),
        Err(CostError::Negative { operation: Operation::Deletion, value: -1.0 })
    );
    assert!(LevenshteinCosts::new(f64::INFINITY, 1.0, 1.0).is_err());

    // Accessors read back what was validated.
    assert_eq!(asymmetric.deletion(), 3.0);
}
```

<div class="callout callout-note">
<strong>Note.</strong> Under unit costs <code>levenshtein(a, b)</code> equals
<code>levenshtein(b, a)</code>. Under weighted costs that holds if and only if
<code>insertion == deletion</code>: the directional reading is "the cost of
turning <code>source</code> into <code>target</code>". Argument order also sizes
the row buffers — they are <code>len(target) + 1</code> long — so with symmetric
costs, passing the shorter string as <code>target</code> allocates less.
</div>

A weighted result is a sum of at most `source_units + target_units` costs, so a
cost near `f64::MAX` over a long operand saturates to `+∞` rather than erroring;
rejecting it would need a length-dependent bound the constructor cannot know. A
unit result is bounded by `max(source_units, target_units)` and cannot overflow
`usize`.

## `SearchResult`

All six search functions return `SearchResult<'t, D>`, borrowing the **target**
— never the source — with `D` the distance type of the tier: `usize` for the
unit-cost searches, `f64` for the weighted ones.

| Accessor | Type | Meaning |
|---|---|---|
| `substring()` | `&'t str` | the best-matching substring of the target, borrowed from it |
| `range()` | `Range<usize>` | that substring's **byte** range in the target |
| `distance()` | `D` | the edit distance from `source` to that substring |

Three guarantees hold for every cost set, every variant and every input:

1. `&target[r.range()] == r.substring()`. The matched text genuinely occurs in
   the target, at the reported position. This holds by construction — the
   substring *is* the slice at the reported range, and the range's end is
   derived from the substring rather than stored, so the two cannot drift.
2. `metric(source, r.substring()) == r.distance()`, where `metric` is the
   distance function matching the search function called. Exact equality,
   weighted costs included. Pinned by property test against a brute force over
   every substring of the target that shares no code with the search routines.
3. Ties resolve to the first candidate in insert → delete → substitute →
   transpose order, at the earliest end column, with the empty substring ahead
   of all of them.

A byte range is what indexes a `&str`, and every scalar boundary is a byte
boundary, so `range()` is always sliceable. A caller who wants a *scalar* offset
writes `target[..r.range().start].chars().count()`.

```rust
use verbora_distance::{LevenshteinCosts, levenshtein_search_weighted};

fn main() {
    // Every edit costs 5, so an exact occurrence inside the target still wins.
    let five = LevenshteinCosts::new(5.0, 5.0, 5.0).unwrap();
    let r = levenshtein_search_weighted("bcd", "axbcdxa", &five);
    assert_eq!((r.substring(), r.range(), r.distance()), ("bcd", 2..5, 0.0));

    // Nothing in "xyz" beats the empty substring: three deletions at 5 apiece,
    // and the empty match at byte 0 wins the tie.
    let r = levenshtein_search_weighted("abc", "xyz", &five);
    assert_eq!((r.substring(), r.range(), r.distance()), ("", 0..0, 15.0));
}
```

<div class="callout callout-note">
<strong>The allocation is opt-in.</strong> The result borrows the target, so the
<em>search → read → discard</em> shape allocates nothing and
<code>r.substring().to_owned()</code> is an explicit choice. For the <em>search a
corpus → keep the good hits</em> shape it is a memory cost instead: a
<code>Vec&lt;SearchResult&lt;'t, _&gt;&gt;</code> pins every target alive. Copy
out <code>(range, distance)</code>, or own the substring at the filter point.
</div>

## Performance characteristics

Verbora picks the cheapest structure — and, wherever a faster *algorithm*
exists, the fastest algorithm — that can answer the question actually asked. A
full `(n+1) × (m+1)` matrix survives in two places only: search mode outside
unit-cost plain Levenshtein, and unrestricted Damerau at weighted costs, where a
transposition can reach an arbitrary earlier row at an arbitrary price.

| Mode | Working set | Why |
|---|---|---|
| distance, Levenshtein, unit cost | **bit-vector** (one `u64` word per 64 units of the shorter operand) | Myers'/Hyyrö's bit-parallel algorithm answers in `O(nm/64)` bitwise operations; single-word path for 1–64 units, blocks beyond |
| distance, Levenshtein, weighted | **1 row** | each cell needs only `up`, `left`, `diag` |
| distance, OSA, unit cost | **bit-vector** (word + block) | Hyyrö's 2003 transposition extension of Myers computes OSA in the same style |
| distance, OSA, weighted | **3 rows** | a transposition reaches back to row − 2 |
| distance, unrestricted Damerau, unit cost | **3 rolling rows + one saved-cell row** | Zhao–Sahni's linear-space algorithm: of the arbitrarily-earlier-row transposition candidates, only the one with no column gap and the one with no row gap can win, and each needs a single remembered cell rather than a whole matrix |
| distance, unrestricted Damerau, weighted | **full matrix** | a weighted transposition reaches an arbitrary earlier row |
| search, Levenshtein, unit cost | **per-column bit-vector deltas** | every cell's parent is a pure function of its neighbours' costs, and unit-cost cell costs are recoverable from Myers' `Pv`/`Mv` words, so no parent matrix is needed |
| search, otherwise | **full matrix** | weighted costs have no bit-vector form, and transposition parents depend on last-row state that cell costs alone cannot recover |

All three unit-cost distance paths first strip the operands' common prefix and
suffix, the reduction that makes near-identical pairs almost free.

Where the full matrix *is* required it is stored struct-of-arrays — costs in one
flat `Vec<f64>`, parents in another `Vec<(u32, u32)>` — so the hot cost sweep is
contiguous and the parents, touched only during backtracking, never pollute the
cache line during the inner loop.

`jaro` and `jaro_winkler` run the classical windowed greedy loop for short
operands, where building a pattern-match table costs more than the handful of
window compares it would replace, and a bit-parallel matching pass above that —
after two reductions that never change the answer: positions of the longer
operand beyond the window's reach are dropped, and the common prefix is counted
rather than scanned.

### Measured performance

<div class="callout callout-warn">
<strong>Pending re-measurement.</strong> The figures in this section are not
current. They are retained only until a fresh full-precision run replaces them —
no number here should be quoted as the library's present performance, and the
shape of the table, not the values, is the part worth reading.
</div>

Criterion timings on one development machine (Intel i9-14900KF, rustc 1.97.1,
`--release`). Treat the exact figures as machine-dependent and the shape of the
table as the reproducible part.

| Workload | Time |
|---|--:|
| `levenshtein`, 4-character ASCII pair | 14.7 ns |
| `levenshtein`, 1024-character ASCII pair | 29.08 µs |
| `levenshtein`, 256-character Cyrillic pair | 3.97 µs |
| `levenshtein`, 64-character pair, unit costs | 166.1 ns |
| `osa`, 64 characters | 179.4 ns |
| `damerau_levenshtein`, 64 characters | 7.75 µs |
| `levenshtein_search`, 64 characters | 12.79 µs |
| `jaro_winkler`, 4 characters | 15.3 ns |
| `jaro_winkler`, 1024 characters | 10.34 µs |
| `dice_coefficient`, 1024 characters | 10.61 µs |
| `hamming`, 4 characters | 6.6 ns |
| `hamming`, 1024 characters | 275.3 ns |

Three things that table is worth reading for:

- **Length costs far less than the `O(nm)` label suggests, at unit costs**,
  because the bit-parallel kernel does `O(nm/64)` bitwise operations instead of
  `O(nm)` cell updates. The weighted tier falls back to the scalar dynamic
  program, which does scale as the label says.
- **`osa` is the cheap Damerau**, on the same input, for a metric most callers
  do not need.
- **Search is the expensive mode.** Ask for a position only when you need one.

Reproduce with:

```text
python3 tools/bench-data/generate.py       # shared inputs (run once)
cargo bench -p verbora-distance
```

The full benchmark table is in
[Benchmarks: distance](../benchmarks/distance.md).

## Allocation behaviour

No function in this crate exposes a scratch buffer or an `_into` variant:
nothing takes mutable working memory you lend it for the duration of a call, so
each call allocates its own dynamic-programming working set and drops it. The
one form of reuse is [`PreparedPattern`](#preparedpattern), which hoists a
pattern's immutable match table out of the loop — a build, not an allocation,
for short ASCII patterns.

| Call | Allocations |
|---|---|
| `levenshtein` / `osa` (ASCII, ≤ 64 units) | **none** — the Myers/Hyyrö state is registers and the byte match table is a stack array |
| `levenshtein` / `osa` (ASCII, longer) | packed `Peq` rows, one word per 64 units of the shorter operand |
| `levenshtein` / `osa` (non-ASCII) | one `Vec<char>` per operand past the stack threshold, plus a hashed match table |
| `damerau_levenshtein` (ASCII) | **none** for byte operands of at most 8 units after the affix trim (a fixed stack matrix); otherwise one contiguous three-row buffer plus a flat 256-entry last-occurrence table |
| `levenshtein_weighted` | one `Vec<f64>` of `len(target) + 1` |
| `osa_weighted` | three `Vec<f64>` of `len(target) + 1` |
| `damerau_levenshtein_weighted` | the full cost **and** parent matrices |
| `levenshtein_search` | two `Vec<u64>` of `len(target) × ⌈len(source) / 64⌉` — no parent matrix, and no `String`: the result borrows the target |
| `damerau_levenshtein_search` / `osa_search` / every `*_search_weighted` | the full cost **and** parent matrices |
| `jaro` / `jaro_winkler` (ASCII, short operands) | **none** — the match flags are stack arrays and the byte match table is a fixed 256-entry stack array |
| `jaro` / `jaro_winkler` (longer ASCII) | one `Vec<u64>` for the packed match table; the match-flag bitsets stay on the stack up to 1024 units per side |
| `jaro` / `jaro_winkler` (non-ASCII) | additionally one `Vec<char>` per operand, and a hashed match table rather than a flat one |
| `dice_coefficient` (any input) | two `FxHashSet<(char, char)>`, each reserved once to its operand's exact bigram bound so the table is never grown mid-fill |
| `hamming` (any input) | **none** |
| `PreparedPattern::new` (ASCII, ≤ 64 units) | one `String` for the pattern; the 2 KB match table is stored inline |
| `PreparedPattern::levenshtein` / `::osa` (ASCII pattern ≤ 64 units, ASCII target) | **none** — the table already exists and the bit-vector state is registers |
| `PreparedPattern::levenshtein` / `::osa` (a query the table cannot serve) | whatever the free function it falls through to allocates |

Three details worth knowing: **unit-cost unrestricted Damerau in distance mode
does not allocate a matrix** (only the weighted form and the matrix-building
searches build one); **search results borrow the target**, so the owned copy is
opt-in rather than mandatory; and **`dice_coefficient` never takes an ASCII
path**, since its bigram keys are `(char, char)` pairs and it never enters the
crate's unit dispatch.

For the general treatment see [Allocation](../performance/allocation.md).

## Unicode and language notes

- **Counts are in Unicode scalar values.** One `char` is one unit everywhere:
  `"a😀b"` is three units, `levenshtein("a😀b", "ab")` is `1`, `hamming`
  compares operands of equal `chars().count()`, Jaro's window and denominators
  are in scalars, and Dice's bigrams are pairs of adjacent scalars. The crate
  ships no length function, because `s.chars().count()` is that function.
- **Positions are in bytes.** `SearchResult::range()` is the one position this
  crate reports, and a byte range is what indexes a `&str`. Every scalar
  boundary is a byte boundary, so the range always slices the target.
- **No metric rewrites its input.** Nothing folds case, trims, collapses
  whitespace or normalises. As a direct consequence no function here consults a
  Unicode character database, so results are frozen across Unicode versions —
  which matters for any structure that persists distances or distance-derived
  keys. Caseless matching is the caller's, applied once at ingestion:
  `hamming(&a.to_lowercase(), &b.to_lowercase())`.
- **A unit is not a grapheme cluster.** `"क्षि"` is four units, `"👨‍👩‍👧‍👦"` is
  seven, and `"👋"` and `"👋🏽"` are one and two. Editing *within* a cluster
  behaves sensibly — `levenshtein("क्षि", "क्ष")` is `1` — but deleting a whole
  cluster costs as many edits as it has scalars. Segment with
  `unicode-segmentation` first if that is wrong for you.
- **No normalisation is applied.** `"café"` composed (`e` + U+0301) and
  precomposed (U+00E9) are different scalar sequences with a non-zero distance,
  and no unit choice fixes that. Normalise upstream if it matters.

## Specified edge cases

Several results look like bugs. All of them follow from the definitions, are
pinned by the test suite, and are safe to build on — but they are worth knowing
before you rely on a score.

| Result | Why | What to do |
|---|---|---|
| `dice_coefficient("aaaa", "aa")` is `1.0` | bigrams form a **set**, so both operands reduce to `{(a, a)}` — identity is exact but not injective | compare the strings when `1.0` must imply equality |
| `dice_coefficient("a", "ab")` and `dice_coefficient("a", "zzz")` are both `0.0` | an operand shorter than two scalars has no bigram to share, and none is fabricated by padding | use `jaro_winkler` or `levenshtein` for very short strings |
| `hamming("a😀b", "abcd")` is `None` | three scalars against four, and Hamming is defined only for equal-length sequences | `filter_map` the `None`s out before ranking |
| `jaro("😀", "😁")` is `0.0` | one scalar each, sharing nothing | — |
| `levenshtein("café", "café")` can be `2` | NFC and NFD are different scalar sequences that render identically | normalise upstream |
| `dice_coefficient("Hello  World", "hello world")` is not `1.0` | case and whitespace are significant, and `' '` forms ordinary bigrams | fold and collapse at the call site if you want them ignored |

```rust
use verbora_distance::{dice_coefficient, hamming, jaro, levenshtein};

fn main() {
    // A set of bigrams, so repeats collapse: 1.0 does not imply equality.
    assert_eq!(dice_coefficient("aaaa", "aa"), 1.0);

    // Below two scalars there is no bigram, so Dice discriminates nothing.
    assert_eq!(dice_coefficient("a", "ab"), 0.0);
    assert_eq!(dice_coefficient("a", "zzz"), 0.0);

    // Hamming is defined only for equal scalar counts.
    assert_eq!(hamming("a😀b", "abc"), Some(2));
    assert_eq!(hamming("a😀b", "abcd"), None);

    // One scalar is one unit, whatever plane it lives in.
    assert_eq!(jaro("😀", "😁"), 0.0);

    // Normalisation is not a unit choice: these render alike and differ.
    assert_eq!(levenshtein("cafe\u{301}", "caf\u{e9}"), 2);
}
```

## Common mistakes

1. **Mixing directions in one ranking.** Sorting ascending is right for
   Levenshtein and Hamming and exactly backwards for Jaro–Winkler and Dice.
2. **Treating `hamming`'s `None` as a distance.** It is absence, not a value —
   `filter_map` it out rather than mapping it to a number.
3. **Expecting a metric to fold case.** None of them does. Fold once at
   ingestion, which is also cheaper in a screening loop than re-folding the
   query against every candidate.
4. **Expecting Dice to ignore whitespace.** A space is an ordinary unit and
   forms ordinary bigrams; nothing is trimmed or collapsed.
5. **Reading `dice_coefficient(a, b) == 1.0` as `a == b`.** Bigrams are a set,
   so `"aaaa"` and `"aa"` score `1.0`. Compare the strings.
6. **Expecting search to return a word.** It returns the cheapest substring,
   which is frequently a fragment.
7. **Reaching for the weighted form when every operation costs the same.** The
   unit form is faster, exact, `Ord` and `Hash`, and cannot be constructed
   wrongly.
8. **Assuming `levenshtein_weighted(a, b, c) == levenshtein_weighted(b, a, c)`.**
   True only while `insertion == deletion`. The unit forms are always symmetric.
9. **Assuming an astral character costs two edits.** `levenshtein("a😀b", "ab")`
   is `1` and `hamming("😀", "𝕳")` is `Some(1)`: one Unicode scalar is one unit,
   in every plane.
10. **Assuming there is no way to amortise anything across calls.** There is no
    scratch-buffer (`_into`) API — but a fixed pattern can be prepared once with
    [`PreparedPattern`](#preparedpattern), and the `parallel` feature adds a
    per-pair batch fan-out.
11. **Reaching for `PreparedPattern` when the pattern changes every call.** It
    front-loads work that only pays back across a candidate set; for one or two
    comparisons the free functions are the cheaper call.

## Related

- [Choosing a distance API](../choosing/distance.md) — which metric for which
  problem, and how to run millions of comparisons without an index.
- [Core vocabulary](./core.md) — the shared traits and helpers the rest of the
  workspace is written against.
- [Phonetics](./phonetics.md) — sound-alike keys, the usual partner for a
  distance metric in name matching.
- [Performance](../performance/index.md) ·
  [Allocation](../performance/allocation.md) ·
  [Parallelism](../performance/parallelism.md)
- [Benchmarks: distance](../benchmarks/distance.md) — the full measured results.
- [Recipes](../recipes/index.md)

## API reference

```rust  ignore
// The crate root is the entire public surface: every name below is reached as
// `verbora_distance::<name>`, and the crate publishes no modules.

// Levenshtein family, unit costs: no cost argument, exact `usize`
pub fn levenshtein(source: &str, target: &str) -> usize;
pub fn damerau_levenshtein(source: &str, target: &str) -> usize;
pub fn osa(source: &str, target: &str) -> usize;

// …and weighted: a validated cost set, `f64`
pub fn levenshtein_weighted(source: &str, target: &str, costs: &LevenshteinCosts) -> f64;
pub fn damerau_levenshtein_weighted(source: &str, target: &str, costs: &DamerauCosts) -> f64;
pub fn osa_weighted(source: &str, target: &str, costs: &OsaCosts) -> f64;

// Search: borrows the target, reports a byte range
pub fn levenshtein_search<'t>(source: &str, target: &'t str) -> SearchResult<'t, usize>;
pub fn damerau_levenshtein_search<'t>(source: &str, target: &'t str) -> SearchResult<'t, usize>;
pub fn osa_search<'t>(source: &str, target: &'t str) -> SearchResult<'t, usize>;
pub fn levenshtein_search_weighted<'t>(source: &str, target: &'t str,
    costs: &LevenshteinCosts) -> SearchResult<'t, f64>;
pub fn damerau_levenshtein_search_weighted<'t>(source: &str, target: &'t str,
    costs: &DamerauCosts) -> SearchResult<'t, f64>;
pub fn osa_search_weighted<'t>(source: &str, target: &'t str,
    costs: &OsaCosts) -> SearchResult<'t, f64>;

// feature = "parallel" — one rayon::par_iter().map(...).collect() fan-out each
pub fn par_levenshtein_batch(pairs: &[(&str, &str)]) -> Vec<usize>;
pub fn par_damerau_levenshtein_batch(pairs: &[(&str, &str)]) -> Vec<usize>;
pub fn par_osa_batch(pairs: &[(&str, &str)]) -> Vec<usize>;

// Cost sets — no Default, no conversions between them
pub struct LevenshteinCosts;  // Debug + Clone + Copy + PartialEq
pub struct OsaCosts;          // …plus a transposition cost
pub struct DamerauCosts;      // …plus a discharged precondition

impl LevenshteinCosts {
    pub const fn new(insertion: f64, deletion: f64, substitution: f64)
        -> Result<Self, CostError>;
    pub const fn insertion(&self) -> f64;
    pub const fn deletion(&self) -> f64;
    pub const fn substitution(&self) -> f64;
}
impl OsaCosts {
    pub const fn new(insertion: f64, deletion: f64, substitution: f64, transposition: f64)
        -> Result<Self, CostError>;
    // four const accessors
}
impl DamerauCosts {
    // additionally requires 2 * transposition >= insertion + deletion
    pub const fn new(insertion: f64, deletion: f64, substitution: f64, transposition: f64)
        -> Result<Self, CostError>;
    // four const accessors
}

pub enum Operation { Insertion, Deletion, Substitution, Transposition }
pub enum CostError {          // Display + std::error::Error
    NotFinite { operation: Operation, value: f64 },
    Negative { operation: Operation, value: f64 },
    TranspositionBelowThreshold { transposition: f64, minimum: f64 },
    // #[non_exhaustive]
}

pub struct SearchResult<'t, D>;   // Debug + Clone + Copy + PartialEq
impl<'t, D: Copy> SearchResult<'t, D> {
    pub fn substring(&self) -> &'t str;              // borrowed from the target
    pub fn range(&self) -> core::ops::Range<usize>;  // byte range into the target
    pub fn distance(&self) -> D;
}

// One pattern against many candidates
pub struct PreparedPattern { /* pattern + its prepared match table */ }  // Debug + Clone
impl PreparedPattern {
    pub fn new(pattern: &str) -> Self;                 // builds the table
    pub fn pattern(&self) -> &str;
    pub fn levenshtein(&self, target: &str) -> usize;  // unit costs only
    pub fn osa(&self, target: &str) -> usize;
    // no damerau_levenshtein: its kernel has no per-pattern state to prepare
}

// Jaro and Jaro–Winkler
pub fn jaro(s1: &str, s2: &str) -> f64;
pub fn jaro_winkler(s1: &str, s2: &str) -> f64;
pub fn par_jaro_winkler_batch(pairs: &[(&str, &str)]) -> Vec<f64>; // feature = "parallel"

// Sørensen–Dice
pub fn dice_coefficient(s1: &str, s2: &str) -> f64;
pub fn par_dice_coefficient_batch(pairs: &[(&str, &str)]) -> Vec<f64>; // feature = "parallel"

// Hamming
pub fn hamming(s1: &str, s2: &str) -> Option<usize>;
pub fn par_hamming_batch(pairs: &[(&str, &str)]) -> Vec<Option<usize>>; // feature = "parallel"
```
