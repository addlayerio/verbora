# `verbora-distance` — the Rust-native contract

**Status:** implemented and normative. Steps 1–6 of §5 have landed, and
Step 7's documentation half with them; what remains outstanding there is the
measurement campaign of §7, which is never launched on an implementer's own
initiative. This document defines the public behaviour of `verbora-distance`
after the Rust-native migration (`docs/design/rust-native-migration.md`,
per-crate item 1). The crate's rustdoc cites it as the source of that
behaviour, so it is no longer a plan the code will one day meet: **where this
document and the shipped code disagree, one of them is a defect and both are
edited in the same change.** §5 is kept as the implementation record.

Behaviour here is derived from published definitions — Levenshtein (1966),
Lowrance & Wagner (1975), Hamming (1950), Jaro (1989), Winkler (1990), Dice
(1945) — or, where a definition is silent (degenerate inputs, tie-breaking,
floating-point evaluation order), from an explicit choice stated and justified
below. No clause exists because another implementation behaves that way.

Incompatible changes are authorised: nothing depends on this crate yet. There
are no deprecation shims, no `#[deprecated]` aliases, and no transition
period. The goal is an API that is born correct.

---

## 1. The contract in brief

**The unit.** One Unicode scalar value (`char`) is one unit. Every count this
crate reports is a count of scalars. `"a😀b"` is three units; `"café"` in NFC
is four; `"日本語"` is three. The crate ships no length function, because
`s.chars().count()` is that function.

**A distance** is a count of edit operations, or — when the caller assigns
weights — the minimum total cost of an edit script. Unit-cost distances return
`usize`: exact, `Ord`, `Hash`, no rounding, and incapable of overflowing.
Weighted distances return `f64`, which is finite for every cost set whose
accumulation fits in an `f64` and `+∞` when it does not (§3.1, "Numeric
limits"). It is never `NaN`.

**A similarity** is a finite `f64` in `0.0..=1.0`. Every similarity in this
crate satisfies four properties for every input, including empty and
single-unit ones:

1. **Total** — the return is a finite `f64`. Never `NaN`, never infinite.
2. **Range** — the return is in `0.0..=1.0` inclusive.
3. **Identity** — `f(x, x) == 1.0` bit-exactly, for every `x`, so
   `score == 1.0` is a sound test to write.
4. **Symmetry** — `f(a, b)` and `f(b, a)` are bit-identical.

**Never happens.**

- **No sentinels.** No magic value is carved out of a numeric range to mean
  "no answer". Absence is `Option::None`, and only Hamming has it.
- **No `NaN`.** No function in this crate returns `NaN`, for any input, under
  any cost set. That is what makes `total_cmp`, `max_by`, `min_by` and
  `sort_by` over results well defined and order-independent.

  The guarantee is *not* finiteness, and saying so would be an overreach:
  every **similarity** is finite and in `0.0..=1.0` (see above), and every
  unit-cost distance is a `usize`, but a **weighted distance** saturates to
  `+∞` when the caller's own costs overflow `f64` accumulation —
  `levenshtein_weighted(&"a".repeat(64), "", &LevenshteinCosts::new(f64::MAX,
  f64::MAX, f64::MAX)?)` is `+∞`, and so is the weighted search's distance.
  Whether the sum overflows depends on the operand length as well as the
  costs, so no constructor can reject it in advance and no metric may fail
  (§3.1, "Numeric limits"). `+∞` orders perfectly well and compares greater
  than every finite distance, which is what a saturated cost means; `NaN`
  would not, which is why it is the value the crate excludes.
- **No panics.** No function in this crate panics, on any input, under any
  cost set, under any feature combination. The one precondition the crate
  enforces — Lowrance & Wagner's transposition threshold — is checked by a
  constructor that returns `Result`, so an inadmissible cost set cannot reach
  a metric.
- **No input rewriting.** No metric folds case, trims, collapses whitespace,
  or normalises. A metric that silently rewrote its inputs could not be
  composed with one that didn't.
- **No Unicode character database.** As a direct consequence of the previous
  clause, no function in this crate consults a UCD table. Results are frozen
  across Unicode versions, for all time. This matters for any structure that
  persists distances or distance-derived keys.

**Determinism.** The same inputs produce bit-identical output on every
platform and every release. Floating-point evaluation order is specified per
formula and is part of the contract, not an implementation detail. The
internal hash maps use `rustc-hash`'s unseeded hasher, so nothing varies run
to run.

**Positions are bytes.** A distance is a count and counts are in scalars. A
*position* has exactly one use — indexing the target — and `&str` accepts
exactly one index type. Search therefore reports a byte `Range<usize>`
alongside the borrowed matched text, and the two cannot disagree because the
range is derived from the text. A scalar boundary *is* a byte boundary, so
the range is always sliceable. See §2.3 and §3.2.

---

## 2. The text unit

### 2.1 The decision

**One `char` is one unit.** Every metric counts Unicode scalar values.

This replaces the UTF-16 code unit. The old unit was justified in the crate's
own module documentation by another runtime's string representation — the one
basis `docs/design/rust-native-migration.md` § "The rule" forbids. The status
quo is not a suboptimal choice that happens to need revisiting; it is the
specific artifact this migration exists to remove.

### 2.2 Why scalars

**It is the unit the caller already has.** `str::chars()` yields it, and
`s.chars().count()` is the length every metric counts in. That is why this
crate ships no length function: a unit that needs a bespoke length helper is
a unit the caller did not already have, and the helper is where the
divergence hides. `utf16_len` is deleted rather than renamed, so every caller
that depended on it fails to compile instead of silently computing the wrong
gate (§5, Step 3).

**It is the only unit under which the search API can be total.** A search
reports where the match sits. Under UTF-16 a match boundary can genuinely
fall between the halves of a surrogate pair, so no byte range exists for it —
which is exactly why the current `SearchResult::substring` is an owned
`String` produced by `String::from_utf16_lossy`, and why it can return text
that does not occur in the target at all. Under scalars every unit boundary
is a byte boundary, so a byte range always exists. The borrowed,
non-fabricating search result of §3.2 is not an optimisation enabled by this
choice; it is only *expressible* because of it.

**It is frozen forever.** Decoding UTF-8 to scalars consults no Unicode
table. A grapheme-cluster unit would consult UAX #29 and would therefore
change with the Unicode version, silently altering the recall of any
persisted index keyed on distance. Combined with the removal of all case
folding (§3.3, §3.4, §3.5), this crate has no UCD dependency at all.

**It costs the common case nothing.** Every fast path in the crate is gated
on `str::is_ascii`, and on ASCII input one byte is one UTF-16 code unit is
one Unicode scalar, exactly and by definition. The ASCII branch computes an
identical answer under all three unit choices and requires no change. All
behaviour and cost changes land exclusively in the non-ASCII branch.

**It is not a novel semantic.** For every Basic Multilingual Plane script the
crate serves — Latin-1, Greek, Cyrillic, Arabic, Hebrew, Devanagari, Thai,
Hangul, CJK — one UTF-16 code unit already *is* one scalar, so distance
values are unchanged. Only astral-plane input (emoji, mathematical
alphanumerics, historic scripts, many CJK extensions) moves.

### 2.3 Counts are in scalars; positions are in bytes

These are different quantities with different uses, and conflating them is
what produced the defects in §2.5. The rule:

| Quantity | Unit | Where it appears |
|---|---|---|
| distance, length, match window, prefix length | scalars | every metric's return; Jaro's window and denominators |
| position, range | bytes | `SearchResult::range` only |

`SearchResult::range` is always a valid `&str` boundary. A caller who wants a
scalar offset writes `target[..r.range().start].chars().count()`.

### 2.4 What the unit is per function

| Function | One unit is | Reported |
|---|---|---|
| `levenshtein`, `damerau_levenshtein`, `osa` | one inserted, deleted or substituted scalar; a transposition swaps two adjacent scalars | `usize` |
| `*_weighted` | as above, priced by the cost set | `f64` |
| `*_search` | window length in scalars | matched text (borrowed) + byte range + distance |
| `hamming` | comparable **iff** equal scalar count; one differing aligned scalar | `Option<usize>` |
| `jaro`, `jaro_winkler` | match window `⌊max/2⌋−1` and both denominators in scalars; Winkler's common prefix in scalars | `f64` in `0..=1` |
| `dice_coefficient` | a bigram is an ordered pair of adjacent scalars | `f64` in `0..=1` |
| `PreparedPattern` | the pattern's scalars | `usize` |

### 2.5 Where the choice is observable

Everything below is a change in behaviour, measured against the current
implementation. ASCII results are bit-identical; BMP-only non-ASCII results
are bit-identical; only the listed cases move.

**Astral values.**

| Call | Now | Under this contract |
|---|---|---|
| `levenshtein("a😀b", "ab")` | `2.0` | `1` |
| `osa("😀😁", "😁😀")` | `2.0` | `1` |
| `jaro("😀", "😁")` | `0.6666…` | `0.0` |
| `jaro("北京", "南京")` | `0.6666…` | `0.6666…` (unchanged) |
| `hamming("😀", "ab")` | `2` | `None` |
| `hamming("😀", "𝕳")` | `2` | `Some(1)` |
| `dice_coefficient("😀😁", "😀")` | `0.5` | `0.0` |

The first row of that table and the third are the point. `jaro("😀","😁")` and
`jaro("北京","南京")` are currently *the same number*: two emoji that share a
high surrogate are indistinguishable from two CJK words that share a real
character. `hamming("😀","𝕳")` currently reports a distance of 2 for two
one-character strings — a value that exceeds the operand length, which the
definition forbids. Neither is a rounding difference.

**Search positions — the widest defect, and it is not astral.** Search
currently reports a UTF-16 offset that the rustdoc documents as a `&str`
index. Three tiers:

1. *Silently wrong, ordinary European text.*
   `levenshtein_search("Berlin", "Zürich, Berlin, Wien")` returns offset 8;
   the byte offset is 9. Byte 8 *is* a char boundary, so `&target[8..]` does
   not panic — it yields `" Berlin, Wien"`. One umlaut is enough. Search UIs
   consume this value as a highlight range. Cyrillic is off by 6, Thai by 12.
2. *Loudly wrong, all non-ASCII.* `"caféx"`, `"北京b"`, `"한국z"`, `"مرحباw"`
   place the offset mid-character, so the documented `&target[offset..]`
   panics.
3. *Silently wrong, astral.* `levenshtein_search("😀", "😁")` returns
   substring `"\u{FFFD}"` with distance 1. That string does not occur in the
   target, and its actual distance from the source is 2 — so the module's own
   promised invariant `distance(source, result.substring) == result.distance`
   is false.

Over 600,000 randomised astral searches, 17.1% of returned offsets are not
valid UTF-8 boundaries and 12.4% of returned substrings contain U+FFFD.

The reason tier 1 survived is a test gap, not an oversight in review: the only
test of the external search contract uses an ASCII corpus, and the astral
search test compares against an internal oracle that performs the same UTF-16
slicing. §6.2 closes both.

### 2.6 What the unit is *not*

**It is not normalisation.** `levenshtein("café", "café") == 2` when one
operand is NFC and the other NFD: different scalar sequences that render
identically. No unit choice fixes this — a grapheme unit gives 1, still not 0.
Normalise first (`verbora-normalizers`) if it matters.

**It is not a grapheme cluster.** `"क्षि"` is four scalars, `"👨‍👩‍👧‍👦"` is
seven, `"👋"` and `"👋🏽"` are one and two. Editing *within* a cluster behaves
sensibly (`levenshtein("क्षि", "क्ष") == 1`); deleting a whole cluster costs
as many edits as it has scalars. Callers for whom that is wrong should
segment first with `unicode-segmentation` and compare the segments themselves.

Graphemes were considered as the default unit and rejected; see §4.2.

**It is not case-insensitive.** See §3.3.

---

## 3. Per-surface specification

### 3.1 Edit distances

#### Signatures

```rust
// module `verbora_distance::levenshtein`

// Unit costs. No cost argument exists.
pub fn levenshtein(source: &str, target: &str) -> usize;
pub fn damerau_levenshtein(source: &str, target: &str) -> usize;
pub fn osa(source: &str, target: &str) -> usize;

// Weighted.
pub fn levenshtein_weighted(source: &str, target: &str, costs: &LevenshteinCosts) -> f64;
pub fn damerau_levenshtein_weighted(source: &str, target: &str, costs: &DamerauCosts) -> f64;
pub fn osa_weighted(source: &str, target: &str, costs: &OsaCosts) -> f64;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LevenshteinCosts { /* private: insertion, deletion, substitution */ }

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OsaCosts { /* private: + transposition */ }

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DamerauCosts { /* private: the same four, plus a discharged precondition */ }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operation { Insertion, Deletion, Substitution, Transposition }

#[derive(Debug, Clone, Copy)]
pub enum CostError {
    NotFinite { operation: Operation, value: f64 },
    Negative  { operation: Operation, value: f64 },
    TranspositionBelowThreshold { transposition: f64, minimum: f64 },
}
// CostError: Display + std::error::Error + PartialEq + Eq.
// `PartialEq` is hand-written and compares the f64 payloads bitwise; see
// "Error equality is reflexive" below.

impl LevenshteinCosts {
    pub const fn new(insertion: f64, deletion: f64, substitution: f64) -> Result<Self, CostError>;
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
    /// Additionally requires `2 * transposition >= insertion + deletion`.
    pub const fn new(insertion: f64, deletion: f64, substitution: f64, transposition: f64)
        -> Result<Self, CostError>;
    // four const accessors
}
```

No `Default` on any cost type. No conversions between them.

#### The contract

**Costs.** A cost is the price of one edit operation on one unit.
`insertion` is charged per unit of `target` not matched from `source`;
`deletion` per unit of `source` not matched into `target`; `substitution` per
aligned pair of differing units; `transposition` per swap of two adjacent
units. Matching a unit against itself is always free and is never a
substitution.

**Admissibility.** A cost is admissible when it is **finite and
non-negative**. Zero is admissible: `LevenshteinCosts::new(0.0, 0.0, 0.0)`
returns `0.0` for every pair, by construction and by design, and makes the
result a pseudometric rather than a metric. Negative and non-finite costs are
rejected, because Verbora does not ship a value it cannot defend: a
"distance" of `-4.0` between a string and itself is not a distance.

**Unit costs are the absence of an argument, not a value.** `levenshtein`,
`damerau_levenshtein` and `osa` evaluate the cost set `(1, 1, 1, 1)` and
return `usize`. There is no way to spell "unit costs" as a value; the unit
metric is the function without a cost argument. This is the crate's existing
lesson applied again — `Options.restricted` became `osa` because the
algorithm lives in the function name, and unit-cost Levenshtein is likewise a
different function with a different kernel, a different complexity class and
a different result type.

**Which cost type binds which algorithm.** `LevenshteinCosts` has three
fields, so a transposition cost cannot be handed to a function that discards
it — today that is a silently-ignored field documented only in prose.
`DamerauCosts` is the only type `damerau_levenshtein_weighted` accepts.

**The transposition precondition, discharged at construction.**
`DamerauCosts::new` returns `Err(CostError::TranspositionBelowThreshold)`
unless

```text
2 * transposition >= insertion + deletion
```

Lowrance & Wagner (1975): below that threshold a chain of adjacent swaps is a
cheaper way to move a unit than delete-and-reinsert, and their recurrence —
which credits at most one transposition per matching row/column pair — stops
ranging over every edit script, so what it returns is *a* script's cost rather
than the minimum. Measured with a Dijkstra search over edit scripts as the
reference: at `i = 1, d = 1, s = 5, t = 0.999` the recurrence reports
`d("aab","baa") = 2` where two transpositions achieve `1.998`.

**The predicate is evaluated as written.** The rejection test is
`2.0 * transposition < insertion + deletion` in `f64`, not the rearranged
`transposition < (insertion + deletion) / 2.0`. The two agree wherever
`insertion + deletion` is a normal `f64` — doubling and halving are both
exact there, and IEEE-754 rounding commutes with scaling by a power of two —
and they disagree at both ends of the range, where the rearranged form is the
wrong one:

- **The sum overflows.** With all four costs at `f64::MAX`, `2 * transposition`
  and `insertion + deletion` are both `+∞`, so the predicate holds — as it
  must, since it holds in the real numbers. The rearranged form computes a
  threshold of `∞ / 2 == ∞` and rejects a cost set Lowrance & Wagner admit.
- **The sum is subnormal.** Halving a subnormal loses its low bit, so the
  rearranged threshold can round *below* the true mean and admit a
  transposition the predicate excludes: with `insertion` the smallest
  positive `f64` `u` and `deletion = 4u`, the threshold is `2.5u` and a
  transposition of `2u` must be rejected, but `fl(5u / 2) == 2u` accepts it.

Against the real-number predicate the `f64` form never over-rejects, and it
over-accepts in exactly one regime: `transposition` above `f64::MAX / 2`,
where the doubling saturates to `+∞` and the test admits unconditionally.
That regime is not observable, because every edit script whose cost would
distinguish a chain of such transpositions from a delete-and-reinsert exceeds
`f64::MAX` and saturates to `+∞` as well, so the recurrence still returns the
minimum over the values `f64` can hold ("Numeric limits", below).

**`CostError::TranspositionBelowThreshold::minimum` is a diagnostic, and it
is always finite.** It reports Lowrance & Wagner's threshold — the real number
`(insertion + deletion) / 2` rounded to the nearest `f64` — which exists as a
finite value for every admissible pair, since two finite costs have a mean of
at most `f64::MAX` even when their sum is not representable. It is computed as
`sum / 2` where the sum is finite and as `insertion / 2 + deletion / 2` where
it is not, so it is never `+∞`: reporting `∞` would name a threshold no cost
could ever meet. It is not the comparison performed, and where the threshold
has no exact `f64` the rounding can land on the rejected `transposition`
itself, so the field is not to be read as "any value at least this large is
accepted". The predicate above is the authority.

**Error equality is reflexive.** `CostError`'s `PartialEq` is written by hand
and compares its `f64` payloads with `to_bits()`. A derived impl compares them
with `==`, under which `NaN != NaN` — and `NaN` is the canonical way to reach
`NotFinite`, so a derived impl leaves the error unequal to *itself* in its
most common case, which breaks `assert_eq!` on errors and every `Result`
comparison a test wants to write. Bit equality is a genuine equivalence
relation over `f64`, so `Eq` is implemented too. It is finer than `==` at one
point only, `-0.0` versus `0.0`, which is reachable in exactly one place: a
`transposition` supplied as `-0.0` and rejected by the threshold, where
distinguishing it is right, because the variant exists to report the value as
supplied. (`NotFinite` carries only non-finite values; `Negative` only values
satisfying `value < 0.0`, which `-0.0` does not; and a rejection's `minimum`
is never negative. It can be `+0.0`: a rejection requires `insertion +
deletion` to be strictly positive, but at the subnormal end the correctly
rounded half of a strictly positive sum is `+0.0`, so
`DamerauCosts::new(f64::from_bits(1), 0.0, 1.0, 0.0)` is rejected with
`minimum: +0.0`. Since `-0.0` is unreachable there, bit equality and `==`
still agree on this field.) The three cost *types* keep their derived
`PartialEq`: they can hold only finite values, so a derived comparison is
already reflexive there.

**Unrestricted Damerau–Levenshtein cannot be called with an inadmissible cost
set. There is no runtime check and no panic.** `osa_weighted` imposes no such
condition — optimal string alignment's recurrence *defines* its answer as the
minimum over alignments editing no position twice, so every admissible cost
set is sound there. `levenshtein_weighted` has no transposition cost to
constrain. `damerau_levenshtein` (unit) evaluates `(1,1,1,1)`, which satisfies
`2 ≥ 2`.

**Empty operands.**

| Call | Result |
|---|---|
| `levenshtein("", t)` | `t.chars().count()` |
| `levenshtein(s, "")` | `s.chars().count()` |
| `levenshtein("", "")` | `0` |
| `levenshtein_weighted("", t, c)` | `c.insertion()` added to itself `t.chars().count()` times |
| `levenshtein_weighted(s, "", c)` | `c.deletion()` added to itself `s.chars().count()` times |

The weighted empty-operand cost is a **fold of repeated additions, not a
multiplication.** Under IEEE-754 these differ, and the crate specifies the
fold so the answer matches the general recurrence's own accumulation. This is
contract, not incidental.

**Numeric limits.** A weighted result is a sum of at most
`source_units + target_units` costs; a cost near `f64::MAX` over a long
operand saturates to `+∞` rather than erroring. Rejecting it would require a
length-dependent bound the constructor cannot know — the same costs overflow
on one operand pair and not on another — so the alternative is not a stricter
constructor but a fallible metric, and §1's "no sentinels, no panics" rules
that out. A unit result is bounded by `max(source_units, target_units)` and
cannot overflow `usize`.

`+∞` is the only non-finite value any function here returns, and it appears
only on this weighted path. **No return is ever `NaN`** (§1): every cell of
every recurrence is a minimum over sums of non-negative costs, and neither
addition nor `min` can manufacture a `NaN` from finite non-negative inputs or
from `+∞`. `+∞` is ordered, so a saturated distance still sorts, thresholds
and compares greater than every finite one — which is what a saturated cost
means. Pinned by §6.1's saturation clause and by the no-`NaN` assertion
inside its no-panic sweep.

**The length lemma, published as contract.** For unit costs,

```text
|a.chars().count() - b.chars().count()| <= levenshtein(a, b)
```

because each insertion or deletion changes the scalar count by exactly one and
each substitution by zero. This is exact and holds bit-for-bit: `levenshtein`
returns `usize`, so there is no rounding to erode it. It is stated as contract
because callers build screening gates on it, and a gate built on the wrong
length function silently discards true matches — which is exactly what the two
currently published gates do (§5, Step 3).

**The weighted analogue is not a bit-level guarantee.** In exact arithmetic the
bound is `min(insertion, deletion) * |Δ|`, by the same argument. The returned
`f64` may sit a few ulps below it, because the recurrence *accumulates* costs by
repeated addition while the bound is written as a single multiplication, and the
two round differently. Measured: `levenshtein_weighted("", "abcdefghij",
&LevenshteinCosts::new(0.1, 0.1, 1.0)?)` returns `0.99999999999999989`, one ulp
under `0.1f64.min(0.1) * 10.0`.

A weighted screening gate must therefore carry a tolerance. `distance >= bound`
is not a sound assertion; `distance >= bound - ε` for an ε of a few ulps is.
Rewriting the recurrence to multiply would not fix this — the accumulation order
is what the algorithm *is*, and forcing agreement with the closed form would
change the distances themselves. Pinned by §6.1.

**Relation to Hamming.** `hamming(a, b) == Some(d)` implies
`levenshtein(a, b) <= d`: substitutions alone realise the Hamming distance,
and Levenshtein is a minimum over all edit scripts.

**Symmetry.** `levenshtein`, `damerau_levenshtein` and `osa` are symmetric
under unit costs. Under weighted costs they are symmetric iff
`insertion == deletion`; this is a property of the cost set, not a defect.

#### Choosing between them

| | Use when | Returns | Kernel | Affix trimming | `PreparedPattern` |
|---|---|---|---|---|---|
| `levenshtein` &c. | you want the number of edits | `usize` | bit-parallel (Myers / Zhao–Sahni) | yes | yes |
| `*_weighted` | operations have genuinely different prices | `f64` | scalar dynamic program | no | no |

The unit form is the right choice for the large majority of programs. The
weighted form exists because it answers a question the unit form cannot, not
because it is a more general spelling of the same thing: the bit-parallel
kernels have no notion of a weighted operation at all, so `substitution: 2.0`
is exactly as slow as `substitution: 0.5`. The boundary is **unit versus
weighted**, never integer versus float.

Moving from the unit form to the weighted form changes the return type, which
is a mechanical but pervasive edit for a caller who keyed structures on
`usize`. That cost is accepted deliberately: it buys an exact, `Ord`, `Hash`
distance for the case every in-tree consumer actually uses, and it is pinned
that the two agree —
`levenshtein(a, b) as f64 == levenshtein_weighted(a, b, &LevenshteinCosts::new(1.0,1.0,1.0)?)`
— so the transition is provably answer-preserving.

### 3.2 Search

#### Signatures

```rust
// module `verbora_distance::levenshtein`

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SearchResult<'t, D> {
    substring: &'t str,
    start: usize,   // byte offset into `target`
    distance: D,
}

impl<'t, D: Copy> SearchResult<'t, D> {
    /// The matched text, borrowed from `target`.
    pub fn substring(&self) -> &'t str;

    /// The match's **byte** range in `target`: `&target[r.range()] == r.substring()`.
    /// Derived from `substring`, never stored, so it cannot disagree with it.
    pub fn range(&self) -> core::ops::Range<usize>;

    /// The distance from `source` to `substring()`, under the metric that
    /// produced this result.
    pub fn distance(&self) -> D;
}

pub fn levenshtein_search<'t>(source: &str, target: &'t str) -> SearchResult<'t, usize>;
pub fn damerau_levenshtein_search<'t>(source: &str, target: &'t str) -> SearchResult<'t, usize>;
pub fn osa_search<'t>(source: &str, target: &'t str) -> SearchResult<'t, usize>;

pub fn levenshtein_search_weighted<'t>(source: &str, target: &'t str,
    costs: &LevenshteinCosts) -> SearchResult<'t, f64>;
pub fn damerau_levenshtein_search_weighted<'t>(source: &str, target: &'t str,
    costs: &DamerauCosts) -> SearchResult<'t, f64>;
pub fn osa_search_weighted<'t>(source: &str, target: &'t str,
    costs: &OsaCosts) -> SearchResult<'t, f64>;
```

The lifetime is named rather than elided — elision cannot choose between two
input lifetimes — which usefully documents that the result borrows the
**target**, never the source.

#### The three guarantees

For every cost set, every variant and every input:

1. `&target[r.range()] == r.substring()`. The matched text genuinely occurs in
   the target, at the reported position.
2. `metric(source, r.substring()) == r.distance()`, where `metric` is the
   distance function matching the search function called. The reported
   distance is realised by the reported text alone. Exact equality, including
   under weighted costs — both paths accumulate in the specified order, and a
   divergence is a bug to fix rather than a caveat to add.
3. Ties are resolved to the first candidate in insert → delete → substitute →
   transpose order, at the earliest end column, with the empty substring ahead
   of all of them.

(1) holds by construction: the substring is produced by slicing the target at
the reported range, so no debug assertion is needed and none is added. (2) is
pinned by property test against a brute force over every substring of the
target that shares no code with the search routines. (3) is observable —
tie-breaking does not change the cost total but does change the recorded
parent, and the parent chain determines the range — so it is specified rather
than left incidental.

#### Storage and derivation

`substring` and `start` are stored; `range().end` is computed as
`start + substring.len()`. The apparent redundancy between "the text" and
"where the text is" is therefore not redundant *storage*: there is exactly one
end, and it cannot drift.

#### Totality — no `Option`

The search is total: column 0 of the last row is always a candidate — the
empty substring, whose cost is the real cost of deleting `source` — so a best
match always exists, including against an empty target, whose only substring
is `""`. An `Option` that is always `Some` is the same defect as an `isize`
that is never negative: a type wider than its contract. "Good enough" is a
threshold the caller owns: `if r.distance() <= 2 { … }`.

| Call (unit costs) | `substring()` | `range()` | `distance()` |
|---|---|---|---|
| `levenshtein_search("abc", "")` | `""` | `0..0` | `3` |
| `levenshtein_search("", "abc")` | `""` | `0..0` | `0` |
| `levenshtein_search("", "")` | `""` | `0..0` | `0` |

#### Index derivation

The search kernels return `(start, end, distance)` in **operand units**, which
under this contract are scalars. Byte positions are derived:

- **ASCII operands** — byte index equals scalar index; the conversion is the
  identity and there is no walk. `String::from_utf8_lossy(..).into_owned()`
  is replaced by `&target[start..end]`, removing one allocation.
- **Non-ASCII operands** — one `char_indices()` walk recovers both byte ends.
  Allocation-free, `O(m)` alongside `Θ(n·m)` matrix work.
  `String::from_utf16_lossy` — the fabrication site — is deleted outright.

A `debug_assert!` that the derived byte slice has exactly `end - start`
scalars guards the new index arithmetic at debug-only cost. Guarantee (2) is
deliberately *not* a debug assertion: it would cost a second `Θ(n·m)`
evaluation on every call.

#### The cost of borrowing

Borrowing removes one allocation per call for the *search → read → discard*
shape and makes the allocation opt-in (`r.substring().to_owned()`) rather than
mandatory. For the *search a corpus → keep the good hits* shape it is a
memory regression: `Vec<SearchResult<'t, _>>` pins every target alive. A
caller who filters and keeps should copy out `(range, distance)` or own the
substring at the filter point, and the release note must say "the allocation
becomes opt-in", never "the allocation is eliminated".

### 3.3 Hamming

#### Signatures

```rust
// module `verbora_distance::hamming`
#[must_use]
pub fn hamming(a: &str, b: &str) -> Option<usize>;

#[cfg(feature = "parallel")]
pub fn par_hamming_batch(pairs: &[(&str, &str)]) -> Vec<Option<usize>>;
```

`INCOMPARABLE` and `hamming_checked` are deleted.

#### The contract

Hamming distance is the number of positions at which two **equal-length**
sequences differ (Hamming 1950, §2). It is not defined for sequences of
unequal length. `hamming` returns that number, or `None` when the operands
have different scalar counts.

```rust
assert_eq!(hamming("karolin", "kathrin"), Some(3));
assert_eq!(hamming("😀", "𝕳"),           Some(1));
assert_eq!(hamming("a😀b", "abc"),       Some(2));   // 3 scalars each
assert_eq!(hamming("a😀b", "abcd"),      None);      // 3 scalars vs 4
assert_eq!(hamming("", ""),              Some(0));
assert_eq!(hamming("ABC", "abc"),        Some(3));   // no folding
```

`hamming` never allocates, for any input.

**Guarantees.** Identity `hamming(x, x) == Some(0)`; discernibility
`hamming(a,b) == Some(0)` iff `a == b`; symmetry; the bound
`Some(d) ⟹ d <= a.chars().count()`; comparability iff equal scalar count; the
triangle inequality over comparable triples; and
`Some(d) ⟹ levenshtein(a,b) <= d`.

**Why `Option`, not a sentinel.** A caller does exactly three things with a
Hamming distance: threshold it, rank on it, or normalise it. The `-1` sentinel
corrupts all three. It is worst under ranking: `Hamming::measure` widens `-1`
to `-1.0` with `IS_SIMILARITY = false`, so an incomparable pair currently
scores *strictly better than a perfect match* and a generic "pick the closest"
loop silently prefers length-mismatched candidates. `Option<usize>` removes
the value from the type rather than documenting the hazard.

**Why `Option`, not `Result`.** A length mismatch is not a failure; it is the
ordinary answer when screening a candidate list. `Result` connotes fault,
forces a public error type, and turns the batch return into
`Vec<Result<_, _>>`. The published ranking recipes already have `Option`'s
shape (`filter_map`).

**Why one function.** `hamming_checked` is currently
`match hamming(..) { INCOMPARABLE => None, d => Some(d as u64) }` — the crate
computes a sentinel and launders it. Once `hamming` is honest, `checked` is
the identity under a second name.

**Why `usize`.** The distance is a count of positions bounded above by the
operand's scalar count. The kernels keep their `u64` accumulators and cast
once, outside every loop.

#### Case is the caller's

`hamming` performs no case mapping. `ignore_case` is deleted. It was an input
transformation wearing a parameter's clothes — `hamming(fold(a), fold(b))` is
its entire meaning — and hiding it caused three observable problems:

- It is **context-sensitive**, inside a metric defined position-wise:
  `hamming("Σ","ς",true) == 1` but `hamming("ΑΣ","Ας",true) == 0`, because
  Final_Sigma makes position *i*'s verdict depend on position *i−1*.
- It is **lowercasing, not case folding**: `hamming("ß","SS",true)` is
  incomparable, though UAX #21 Default Caseless Matching equates them.
- It **changes the function's domain in both directions**: `("İ","i̇")` goes
  `None → Some(0)` and `("İ","x")` goes `Some(1) → None`. U+0130 is the only
  code point in Unicode whose `to_lowercase` changes length, and the crate
  currently carries three paragraphs of rustdoc for it alone.

A parameter that changes a function's domain is a second function. Callers
fold once at ingestion:

```rust
assert_eq!(hamming(&a.to_lowercase(), &b.to_lowercase()), Some(0));
```

which is also strictly cheaper in a screening loop, where the query is
currently re-folded against every candidate.

Removing the flag also makes `hamming` allocation-free on *every* input: the
fall-through path becomes one fused `chars()` pass that counts differences
and decides comparability together, replacing five passes and two allocations
(seven and four when folding).

#### Performance note

The ASCII tiered kernel (scalar zip below 8 bytes, SWAR word kernel to 15,
fused 16-lane difference-count/ASCII-check above) is untouched: the return
type is a wrapper at the boundary and never enters a loop. The **wrapper** is
not free — `Option<usize>` returns in two registers rather than one, which
costs the SWAR tier the tail call the compiler currently emits. Marked
`UNMEASURED`; see §7.

### 3.4 Jaro and Jaro–Winkler

#### Signatures

```rust
// module `verbora_distance::jaro_winkler`
pub fn jaro(a: &str, b: &str) -> f64;
pub fn jaro_winkler(a: &str, b: &str) -> f64;

#[cfg(feature = "parallel")]
pub fn par_jaro_winkler_batch(pairs: &[(&str, &str)]) -> Vec<f64>;
```

`jaro_winkler::Options` is deleted in full — both `ignore_case` and `dj`.

#### `jaro`

Jaro, M. A. (1989), *Advances in record linkage methodology as applied to
matching the 1985 census of Tampa, Florida*, JASA 84(406), 414–420.

Let `n1` and `n2` be the operands' scalar counts. The **match window** is

```text
w = max(0, floor(max(n1, n2) / 2) - 1)      scalars
```

Unit `s1[i]` **matches** `s2[j]` when `s1[i] == s2[j]` and `|i - j| <= w`.
Matching is a one-to-one assignment computed by a single left-to-right greedy
pass over `s1` that, for each `i`, claims the lowest unclaimed `j` in the
window. `m` is the number of matched pairs. `t` is half the number of matched
pairs appearing in a different relative order in the two strings, formed as
`raw_transpositions as f64 / 2.0` — exact, so an odd raw count contributes
`x.5`.

With `m > 0`:

```text
((m / n1) + (m / n2) + ((m - t) / m)) / 3
```

evaluated left to right in exactly that grouping.

**Degenerate cases — complete.**

| Condition | Result | Source |
|---|---|---|
| `s1 == s2`, including `("", "")` | `1.0` | identity axiom |
| exactly one operand empty | `0.0` | nothing to match |
| `m == 0` | `0.0` | the standard's `m = 0` clause |

**The window clamp at 0.** `floor(max/2) - 1` is negative only when
`max(n1,n2) <= 1`. The window is a *pruning* device — Jaro introduces
`|i − j| <= w` so that units too far apart are not treated as matches — not a
definition of matching, and evaluating a displacement bound outside its
intended domain to `−1` prunes the one candidate pair at displacement 0.
Clamping at `0` therefore changes exactly one input class, both operands one
unit long, and there it is the difference between the identity axiom holding
and failing: `jaro("a","a")` becomes `1.0` and `jaro("a","b")` stays `0.0`.
Every input with `max(n1,n2) >= 2` is untouched, since `floor(max/2) - 1 >= 0`
there.

Without the clamp the scalar unit would introduce a **regression**:
`jaro("😀","😀")` is `1.0` today only because the emoji is two UTF-16 units;
as one scalar it would fall into the `max_len < 2` branch and silently become
`0.0`. The clamp also repairs a pre-existing disagreement — `jaro("a","a")` is
`0.0` today while `jaro_winkler("a","a")` is `1.0`, so the two functions
currently contradict each other about their own identity element.

**Exactness.** For `s1 == s2` non-empty, `m == n1 == n2` and `t == 0`, so the
formula is `(1.0 + 1.0 + 1.0) / 3.0`; IEEE-754 gives `x/x == 1.0` exactly for
finite non-zero `x` and `3.0/3.0 == 1.0`. `jaro(x, x) == 1.0` is a bitwise
guarantee, not a near-miss. Conversely `jaro(a,b) == 1.0` implies `a == b` for
any operand shorter than 2^52 units: a single mismatch drops one term to at
most `1 - 1/max(n1,n2)`, and the resulting sum `<= 3 - 2^-52` is more than
half an ulp below `3.0`.

**Range.** `m <= min(n1,n2)` bounds the first two terms by 1; the lockstep
transposition walk visits `m` pairs so `t <= m/2`, giving `(m-t)/m` in
`[0.5, 1]`. No rounding of three values each `<= 1` can carry the quotient
above `1.0`.

#### `jaro_winkler`

Winkler, W. E. (1990), *String comparator metrics and enhanced decision rules
in the Fellegi–Sunter model of record linkage*, ASA Proceedings of the Section
on Survey Research Methods, 354–359.

```text
sim_w = sim_j + l * p * (1 - sim_j)
```

where `sim_j` is `jaro`, `l = min(4, common_prefix_len(s1, s2))` in scalars,
and `p = 0.1` (Winkler's value). Evaluated left to right in that grouping.

The boost is applied unconditionally. Winkler's later "apply only when
`sim_j > 0.7`" variant introduces a discontinuity and is **not** implemented;
if it is ever added it arrives as a separately named function, never as a
change to this one.

**`l` is a length, capped.** It is bounded by both operands: for
`("ab","ab")`, `l == 2`, not `4`. The current implementation returns 4 there,
because its scan compares `Option`s and `None == None` continues the loop past
both operands' ends. That is a defect, and it is why
`jaro_winkler("A","a", ignore_case)` is `0.4` today — the folded pair misses
the equality short-circuit, `jaro` returns `0.0` for one-unit operands, and a
saturated prefix leaves the boost fully exposed.

**Range.** Rearranged, `sim_w = (1 - l*p) * sim_j + l*p`: an affine
interpolation between `sim_j` and `1.0` with weight `q = l*p`. With `l <= 4`
and `p = 0.1`, `q <= 0.4`, so the result lies strictly inside the convex hull
of `{sim_j, 1}` and no clamp is applied or needed.

**Identity.** `jaro_winkler(x, x) == 1.0` exactly for every `x`, and only for
`x == y`: `jaro` gives exactly `1.0`, so the boost term is `l * p * 0.0 == 0.0`
and the sum is `1.0 + 0.0`. This holds for `""`, for single-unit operands, and
for astral operands, **with no equality short-circuit** — the current
`if s1 == s2 { return 1.0 }` at the top of the function is deleted. It exists
only to mask `jaro`'s degenerate-window defect, and it is the mechanism that
split folded from unfolded identity. The fast exit it provided already exists
structurally: the common-prefix scan consumes identical operands entirely, so
the kernels never run.

**Why `dj` is deleted.** `Options::dj` is a caller-supplied precomputed Jaro
score, and it is the only way any caller can make this crate return a value
outside `[0,1]` or `NaN`: `dj = Some(100.0)` returns `100.0`, `dj = Some(-5.0)`
returns `-3.8`, `dj = Some(NaN)` returns `NaN`. It is not part of Winkler's
definition, it is silently ignored on the identity branch and honoured on the
other, and in a batch a single `dj` is applied to every pair — meaningless for
a per-pair quantity. A caller who already holds a Jaro score computes the
boost directly; it is one multiply-add and the formula is published above.

#### Degenerate table, shared with Dice

| Input | `jaro` | `jaro_winkler` | `dice_coefficient` |
|---|---|---|---|
| `("", "")` | `1.0` | `1.0` | `1.0` |
| `("", "a")` / `("a", "")` | `0.0` | `0.0` | `0.0` |
| `("a", "a")` | `1.0` | `1.0` | `1.0` |
| `("a", "b")` | `0.0` | `0.0` | `0.0` |

One sentence governs all three: **identical inputs score 1.0; inputs sharing
no feature score 0.0.** The `0/0` cases are removable singularities, and which
value to insert is a design question whose answer is forced by consistency —
two empty strings are *identical*, not *disjoint*.

### 3.5 Dice

#### Signature

```rust
// module `verbora_distance::dice`
pub fn dice_coefficient(a: &str, b: &str) -> f64;

#[cfg(feature = "parallel")]
pub fn par_dice_coefficient_batch(pairs: &[(&str, &str)]) -> Vec<f64>;
```

#### The contract

Dice, L. R. (1945), *Measures of the amount of ecologic association between
species*, Ecology 26(3), 297–302. Dice's coincidence index for two samples is
`2C / (A + B)`. The string application takes the "species" to be the distinct
**bigrams**: ordered pairs of adjacent scalars.

For a string of `n` scalars, `bigrams(s)` is the **set**
`{ (s[i], s[i+1]) : 0 <= i < n - 1 }`; it has at most `n - 1` elements and is
empty when `n < 2`. With `A = bigrams(s1)`, `B = bigrams(s2)`:

```text
dice = 2 * |A ∩ B| / (|A| + |B|)     when |A| + |B| > 0
dice = 1.0                           when |A| + |B| == 0 and s1 == s2
dice = 0.0                           when |A| + |B| == 0 and s1 != s2
```

**No preprocessing.** The operands are used as given. Case is significant,
whitespace is significant, nothing is trimmed or collapsed, and `' '` is an
ordinary unit forming ordinary bigrams. The current `sanitize` — unconditional
`to_lowercase`, whitespace-run collapse, trim, over a whitespace set that is
another language's regex `\s` class and differs from Unicode `White_Space` at
U+0085 and U+FEFF — is deleted. It is a policy no caller can turn off, it is
not in Dice (1945) nor in the bigram application, and it is the last UCD
dependency in the crate.

**No padding.** A one-scalar operand yields no bigram. The current
space-padding fabricates a bigram containing a unit absent from the input and
manufactures similarity: `dice("a", "a b")` scores `0.667` today where the
definition gives `0.0`.

**Totality.** The `|A| + |B| == 0` branch is taken before the division, so no
input divides by zero. The current implementation returns `NaN` there, and the
defect is wider than its own rustdoc admits — `dice(" ", "\t")`,
`dice("\u{FEFF}", " ")` and `dice("  ", "\n\n")` are all `NaN` too, because
`sanitize` strips them to empty first. `NaN` is the worst possible sentinel: it
is not orderable, so `max_by` over a candidate list gives order-dependent
results, and a single `NaN` poisons a ranking with no visible failure.

**Identity is exact but not injective.** `dice(x, x) == 1.0` exactly
(`2k / 2k` with `k = |A|` an exactly representable integer). The converse does
**not** hold: bigrams form a set, so `dice("aaaa", "aa") == 1.0`. This is
inherent to the set-based definition and is documented rather than patched; a
multiset variant would not fix it either (`"aabab"` and `"abaab"` have
identical bigram multisets). Callers needing `score == 1.0` to imply equality
compare the strings.

**Range.** `|A ∩ B| <= min(|A|, |B|)`, so the numerator never exceeds the
denominator and the quotient of two exactly representable non-negative
integers is in `[0, 1]`.

#### Observable changes

Dice moves the most of any function here. Every input containing an
upper-case letter or any whitespace changes, plus the whole one-scalar and
empty class.

| Input | Now | Contract | Arithmetic |
|---|---|---|---|
| `("", "")`, `(" ", "\t")`, `("\u{FEFF}", " ")` | `NaN` | `1.0`, `0.0`, `0.0` | degenerate rule |
| `("a", "a b")` | `0.6667` | `0.0` | `A = {}`, `B = {(a,␠),(␠,b)}` → `2·0/(0+2)` |
| `("ABC", "abc")` | `1.0` | `0.0` | `{AB,BC} ∩ {ab,bc} = ∅` |
| `("Hello  World", "hello world")` | `1.0` | `14/21` | 11 and 10 bigrams, 7 shared |
| `("  padded  ", "padded")` | `1.0` | `10/13` | 8 and 5 bigrams, 5 shared |
| `("night", "nacht")` | `0.25` | `0.25` | `{ht}` shared of 4+4 |
| `("aaaa", "aa")` | `1.0` | `1.0` | unchanged |

This is the widest **silent** change in the contract: nothing fails to
compile, and the crate is overwhelmingly used for fuzzy name and title
matching where `"Hello  World"` and `"hello world"` scoring `1.0` is what a
caller wants. The mitigation is documentation, not code: the site page must
show `dice_coefficient(&a.to_lowercase(), &b.to_lowercase())` at the top of
the Dice section, not in a footnote. See §4.6 for why the folding is not kept
as a defaulted option.

### 3.6 `PreparedPattern`

```rust
impl PreparedPattern {
    pub fn new(pattern: &str) -> Self;
    pub fn pattern(&self) -> &str;
    pub fn levenshtein(&self, target: &str) -> usize;
    pub fn osa(&self, target: &str) -> usize;
}
```

Unit costs only. The precomputed pattern-match table exists for no other case:
the weighted path does not use it, so the weighted fallback branch is deleted
outright, along with the argument-order caveat that only mattered when
`insertion != deletion`.

`unit_len` is deleted. Its only purpose was to surface a length the caller
could not otherwise compute; under this contract that length is
`p.pattern().chars().count()`, and the crate ships exactly one way to spell
it.

The pattern stays owned (no `PreparedPattern<'a>`). This is a build-once,
query-many type that callers store in structs; a lifetime parameter would
propagate into every such struct to save one `String` per pattern, which is
the wrong trade for this shape.

There is deliberately no prepared unrestricted Damerau–Levenshtein: its
recurrence needs per-cell state the pattern table cannot carry.

### 3.7 Parallel batch

```rust
#[cfg(feature = "parallel")]
pub fn par_levenshtein_batch(pairs: &[(&str, &str)]) -> Vec<usize>;
pub fn par_damerau_levenshtein_batch(pairs: &[(&str, &str)]) -> Vec<usize>;
pub fn par_osa_batch(pairs: &[(&str, &str)]) -> Vec<usize>;
pub fn par_hamming_batch(pairs: &[(&str, &str)]) -> Vec<Option<usize>>;
pub fn par_jaro_winkler_batch(pairs: &[(&str, &str)]) -> Vec<f64>;
pub fn par_dice_coefficient_batch(pairs: &[(&str, &str)]) -> Vec<f64>;
```

Six functions, one per metric, each `pairs.par_iter().map(f).collect()` — the
exact shape `AGENTS.md` § Rayon Policy fixes permanently. Output order matches
input order. `par_damerau_levenshtein_batch` no longer panics for any input,
including an empty `pairs`, because there is no cost set to reject.

**No weighted batch variants.** The weighted path is strictly heavier per
pair, so the crossover at which parallel wins is strictly *earlier* than the
unit form's — the published guidance is conservative for it — and six more
wrappers saying so would be surface without content. A caller with weighted
costs writes the one-line `par_iter` themselves.

Every benchmark table currently embedded in these functions' rustdoc is stale
the moment this contract lands: Hamming's signature changes, Dice's algorithm
changes, and the unit-cost dispatch loses its cost comparisons. They are
removed rather than adjusted, and restored only from a fresh full-precision
run (§7).

### 3.8 Removed from the public surface

Deleted outright, with the reason in §4 where it needs one:

| Removed | Replacement |
|---|---|
| `levenshtein::Options` | `LevenshteinCosts` / `OsaCosts` / `DamerauCosts`, or no argument at all |
| `jaro_winkler::Options` (`ignore_case`, `dj`) | nothing — `jaro_winkler(a, b)` |
| `hamming`'s `ignore_case` parameter | `a.to_lowercase()` at the call site |
| `hamming::INCOMPARABLE` | `Option::None` |
| `hamming_checked` | `hamming` |
| `units::utf16_len` (both copies) | `s.chars().count()` |
| `pub mod units` — `Unit`, `UnitMap`, `ByteMap`, `Operands`, `dispatch` | private; never intentional API |
| `PreparedPattern::unit_len` | `p.pattern().chars().count()` |
| `verbora_core::StringMetric` and `Levenshtein`, `DamerauLevenshtein`, `Osa`, `JaroWinkler`, `Dice`, `Hamming` | closures at the call site |
| `dice::sanitize` and its one-scalar padding | caller-side normalisation |
| `tests/zz_audit_tmp.rs` | — (its own header says it should already be gone) |

`verbora-distance` used `verbora-core` in exactly two places: `StringMetric`
and `whitespace::is_whitespace`. Both are gone, so the crate's dependency list
becomes `rustc-hash` plus optional `rayon`. `is_whitespace` has many other
consumers in the workspace and stays where it is.

Considered and **not added**:

| Not added | Why |
|---|---|
| `*_seq` / `*_slice` over a `Symbol` trait | §4.3 |
| `best_by_distance` / `best_by_similarity` | §4.4 |
| `PrefixScale` (configurable Winkler `p`) | §4.5 |
| `fold_case` at the crate root | §4.7 |
| `LevenshteinCosts::from_integers` | §4.8 |
| cost-type conversion lattice (`From` / `TryFrom`) | §4.8 |
| `Default` on any cost type | §4.8 |

---

## 4. Rejected alternatives

### 4.1 Keeping the UTF-16 code unit

Rejected because its only justification is another runtime's string
representation, which `docs/design/rust-native-migration.md` § "The rule"
forbids as a basis for behaviour, and because it makes a total search API
impossible: a match boundary can fall between surrogate halves, so no byte
range exists, so the result must be an owned lossy `String` that may not occur
in the target. It also produces values the definitions forbid —
`hamming("😀","𝕳") == 2` for two one-character strings — and collapses
distinctions that matter, scoring `jaro("😀","😁")` identically to
`jaro("北京","南京")`.

The one real argument for it — JS interop parity — is not an argument this
crate accepts, and a caller who genuinely needs it can encode to UTF-16 and
compare the sequences themselves once the sequence layer of §4.3 exists.

### 4.2 Grapheme clusters as the default unit

Rejected on three grounds, in order of weight.

**It forfeits the byte-equals-unit theorem the whole crate is built on.**
`"a\r\nb"` is `is_ascii() == true`, four bytes, four scalars, and **three**
clusters under UAX #29 GB3. So the ASCII fast path — every `is_ascii()` gate
in the crate — would no longer be exact, and the crate's cheapest and most
common path would become an approximation.

**It is not frozen.** UAX #29 changes between Unicode versions, so a persisted
index keyed on distance would silently change its recall on a dependency
bump.

**It does not fix the case it is wanted for.** NFC/NFD `"café"` is 1 under
graphemes, not 0 — normalisation is still the caller's job either way.

Graphemes remain the right unit for some callers, which is what §4.3's
deferred sequence layer is for.

### 4.3 A public sequence API (`levenshtein_seq` / `_slice` over a `Symbol` trait)

**Cut, deferred, not rejected on principle.** The kernels are already generic
(`plain_levenshtein<T: BitPeq>`, `full_matrix<T: Unit>`, `search_generic`), so
exposing them is cheap in code, and it is the honest escape hatch for
graphemes, words, opcodes and UTF-16 parity. It is cut from *this* contract
for three reasons:

1. **Zero consumers.** Nothing in the workspace would call it, and the same
   test that condemns the metric trait (§4.4) applies here.
2. **It doubles the surface it touches.** Eight functions plus a trait plus a
   second search-result type (`&[S]` cannot borrow into a `&str`), against a
   contract whose value is that it is small and fully pinned.
3. **The trait's shape is not yet settled, and getting it wrong is
   expensive.** A blanket `impl<S: Copy + Eq + Hash>` cannot coexist with the
   `u8` specialisation (`ByteMap = [usize; 256]`, `BitPeq::Table1 = [u64; 256]`)
   that makes the ASCII path fast, and Rust has no specialisation. A *sealed*
   trait over `{u8, u16, u32, u64, char}` does work today and does reach the
   flat tables — but unsealing it later requires re-parameterising ~15 hot
   generic kernels onto a separate `PeqTable<S>` type, i.e. a refactor of the
   most-tested code in the crate, measured. Shipping the sealed version now
   therefore de-risks nothing.

Adding functions later is not a breaking change. The designated shape, so it
is not re-derived: sealed `Symbol` over the five primitives, `*_seq` naming
(a different function on a different type, so the type states what is
measured, rather than a mode flag that leaves `levenshtein(&str, &str)`
ambiguous at the call site), and a separate `SeqSearchResult { range, distance }`.

### 4.4 Keeping `StringMetric` (or replacing it with a `Distance`/`Similarity` pair)

Rejected. Three independent facts:

**It has no consumer.** `StringMetric` is named only in its own `verbora-core`
definition, its six `verbora-distance` impls, and site prose. Zero generic
functions, zero stored metrics, zero `dyn`. The two structures that look like
candidates refuse genericity *in writing*: `FuzzyIndex`'s module doc argues
that BK-tree pruning is correct only under a triangle-inequality metric and
therefore fixes the metric rather than exposing it, and `DeletionIndex` copies
the reasoning. Trait seams in this workspace exist where a long-lived
structure stores a strategy (`SentimentAnalyzer<S: Stemmer>`,
`Arc<dyn Stemmer + Send + Sync>`); nothing stores a metric, because a metric
is a pure function applied at a call site.

**It manufactures wrong answers.** The site's own published, CI-executed
`best()` example returns `"sitting"` for
`best(Hamming, "kitten", ["kitten","mitten","sitting"])` — scores
`[0.0, 1.0, -1.0]`, and the incomparable candidate wins — and returns `"x"`
for `best(Dice, "", ["", "x"])`, because `dice("","")` is `NaN` and the
example's own NaN filter deletes the only exact match. The free functions have
neither defect. The defects are created by `measure(..) -> f64`, a
lowest-common-denominator return with no representation for "no answer". An
abstraction that takes correct primitives and returns wrong answers is not
paying rent.

**It cannot do the job a trait is for.** `Box<dyn StringMetric>` is `E0038` —
the associated const makes it dyn-incompatible — so it cannot play the
`Arc<dyn Stemmer>` role that justifies `verbora-core`'s other traits. It is a
static-dispatch-only trait with zero static-dispatch consumers.

A two-trait split fixes direction but not partiality: Hamming still has no
total `f64` answer. And the cheapest honest encoding of direction is not a
trait at all — it is the name of the function the caller calls.

The strongest counter-argument, recorded so it is not lost: `FuzzyIndex`'s
refusal is arguably a *typing* failure, and a `Metric: Distance` supertrait
guaranteeing the triangle inequality would let `FuzzyIndex<M: Metric>` be
generic *and* statically exclude `Osa`, which is not a metric. That is a
better design than a hardcoded default plus a paragraph of prose — if weighted
or pluggable fuzzy indexing is ever wanted. It is not wanted now, adding it
later is an authorised breaking change, and building it now would monomorphise
an index for speculative demand.

### 4.5 A configurable Winkler prefix scale `p`

Cut. `p` is genuinely Winkler's parameter and callers do tune it in the record
linkage literature, so this is the least comfortable cut in the document. It
goes because it has no consumer, because exposing it as a bare `pub f64`
would break the range guarantee (`p > 0.25` makes `l*p > 1` and lets the boost
push the score above `1.0`), and because exposing it *safely* means shipping a
validated `PrefixScale` newtype — a whole type whose only job is to exist so
that a field cannot be set wrongly. Hardcoding `0.1` keeps the range guarantee
for free and deletes an entire `Options` struct, including its name collision
with `levenshtein::Options`. If `p` returns, it returns as
`jaro_winkler_scaled(a, b, PrefixScale)`, which is an addition, not a break.

### 4.6 Keeping Dice's case folding as a defaulted-on option

Rejected, with the honest cost stated: this is the change most likely to be
experienced as a regression, because Dice-on-bigrams is overwhelmingly used
for fuzzy name and title matching where case- and whitespace-insensitivity is
the point.

It goes because a defaulted-on preprocessing step is still silent mangling for
anyone who does not read the docs; because it is the only remaining thing that
would make one metric in this crate rewrite its inputs while the others do
not; because it is the crate's last UCD dependency, which the "frozen for all
time" clause in §1 depends on removing; and because the specific whitespace
set it applies is another language's regex `\s` class, which is the
inheritance this migration exists to delete. Two metrics that disagree about
whether they normalise cannot be composed, and after this change the rule is
one sentence with no exceptions.

Boundary-marker padding (`^a$`) was also considered as a way to keep
short-string discrimination — it would give identity, disjointness *and*
gradation for one-scalar operands, which the no-padding rule sacrifices
(`dice("a","ab") == dice("a","zzz") == 0.0`). Rejected because it requires an
out-of-alphabet sentinel scalar and changes *every* score, not just the
degenerate ones, taking the function one convention further from Dice (1945)
rather than closer.

### 4.7 `fold_case` at the crate root

Cut. It is either `str::to_lowercase` under a new name, which adds nothing, or
real Unicode case folding (UAX #21 `toCasefold`), which needs CaseFolding data
and would reintroduce the UCD dependency §1 deliberately eliminates. Case
mapping is a normalisation concern and belongs in `verbora-normalizers`.
Callers get `str::to_lowercase` and a pointer.

### 4.8 A single validated `Costs` type, `Default`, `from_integers`, conversions

**One validated type: rejected.** The Lowrance–Wagner precondition is not a
property of a cost set; it is a property of a (cost set, algorithm) pair.
`osa` is defined for *every* cost set — its recurrence defines its own answer,
and `transposition: 0.25` with `osa` is deliberately published today —
while `levenshtein` never reads a transposition cost at all. One constructor
therefore either over-rejects (breaking `osa`) or under-rejects (leaving a
runtime panic). Three types is the only shape in which the check binds exactly
the function that needs it, and it is also what deletes the silently-ignored
field.

**`Default`: rejected.** The one value everybody wants is now reachable
*without* a cost type. A `Default` impl would exist solely to enable
`levenshtein_weighted(a, b, &Default::default())` — the slow path for the fast
case, an attractive nuisance whose only effect is to make callers slower.

**`from_integers`: cut.** `new` is a `const fn` and covers every case,
including `const` positions. A second constructor whose only claimed advantage
is infallibility does not even hold that advantage uniformly —
`DamerauCosts::from_integers` is still fallible, because integrality does not
imply the precondition. `1.0` is as clear as `1`.

**Conversions: cut.** No consumer, and a `From<OsaCosts> for LevenshteinCosts`
that silently drops a field is exactly the kind of quiet discard this design
removed from `Options`.

**An integer cost type: rejected.** It buys no speed. Every fast path in the
crate gates on *all costs exactly 1.0*, never on integrality — Myers'
algorithm has no notion of a weighted operation, so `substitution: 2` is
exactly as slow as `0.5`. The genuine integer wins are exactness and
infallibility, and both are captured by the unit tier returning `usize`. The
remaining integer use case, `ins = del = 1, sub = 2`, is the *indel/LCS
distance* — a named metric with its own published bit-parallel algorithm — and
if it is ever wanted it arrives as `fn indel(a, b) -> usize`, not as a cost
configuration.

**A builder or setters: rejected.** Validation is a whole-tuple predicate
binding three fields, so a builder can only check at `.build()` — a `Result`
constructor with an extra type and no `const`. Setters are worse:
`set_insertion` can invalidate an already-valid `DamerauCosts`, so every
setter returns `Result` and the type acquires a partially-valid state.

**Rejecting only `NaN`, keeping negative costs: rejected, and this one costs
something real.** Roughly fifteen in-crate tests deliberately construct
negative, zero, infinite and `NaN` costs and assert exact results — including
the executable witnesses that the affix-trimming gate is load-bearing
(`levenshtein("abcdefg","abcdefg")` with `substitution: -1.0` is `-4.0`;
trimming first would have answered `0.0`). Those witnesses cannot be rewritten
under a type that rejects negatives; they are deleted. That is acceptable
*only* because the structural split makes what they proved unnecessary: there
is no longer a runtime gate to take correctly, since `levenshtein` trims and
`levenshtein_weighted` does not, and no cost value can move a call between
them. The proof is relocated into the type system rather than lost. §6.1 names
the replacements.

The weaker half of this is honest: the IEEE evaluation-order tests lose their
`INFINITY` and `NaN` entries, which are precisely the inputs where
association order is most visible. The property they pin survives with a
smaller domain (`(0.5, 1.5, 0.75)`, all-zero, unit), and the empty-operand
fold-of-additions clause in §3.1 is pinned separately.

### 4.9 `Result` instead of `Option` for Hamming

Rejected, but the counter-case is real and worth recording. The most common
way a Hamming call fails in production is a fixed-width field that quietly was
not fixed-width; `None` tells that caller nothing, while
`Err(LengthMismatch { a: 16, b: 17 })` tells them what to log, and "the caller
can compute the lengths" is weak, because the caller who is surprised is by
definition the one who did not think to. `Option` wins on composability
(`filter_map`, `Vec<Option<_>>`, and "not an error" is the truth for candidate
screening), which is the dominant shape here. A crate whose primary audience
validated fixed-width records rather than screened candidates should choose
`Result`.

### 4.10 An `Option` return for search

Rejected. The search is total — the empty substring is always a candidate — so
`None` would never be produced by the definition and could only be produced by
a threshold, which belongs to the caller. An `Option` that is always `Some` is
the same defect as the `isize` offset that is never negative.

### 4.11 `SearchResult { range, distance }` with no lifetime

The genuine runner-up, and it is simpler: no lifetime, no lifetime infection
into caller types, no pinned targets, and it fixes every measured defect in
§2.5 — because it is the *byte range* that makes the offset correct, not the
borrow.

Rejected on two grounds. First, it loses the compiler-checked tie between the
indices and the string they index: `let r = search(n, &build());` compiles
fine and leaves the caller holding plausible-looking `usize`s into a dropped
temporary. Second, every consumer that wants the matched text must also be
handed the target, so consumer signatures grow a parameter they do not
conceptually need.

The cost of the choice is stated in §3.2 and must not be understated in the
release note.

### 4.12 Cutting weighted search

Considered, because it would make `SearchResult` non-generic and drop the
public surface from twelve levenshtein-family functions to nine. Rejected
because, unlike everything else cut in this document, weighted search is
existing, test-pinned behaviour that answers a question the unit form cannot,
and `AGENTS.md` § Choosing the Right API's test is met: the difference —
"exact integer count at unit cost on the bit-parallel kernel" versus "minimum
script cost under caller-assigned weights on the scalar DP" — is explainable
in one sentence. Cutting an abstraction with no consumer is discipline;
cutting working behaviour for surface economy is not.

### 4.13 Cutting the `par_*_batch` family

Considered under `AGENTS.md` § Batch APIs ("do not add a batch API that
provides no meaningful advantage over `items.iter().map(...).collect()`") and
rejected, because § Rayon Policy is the more specific authority and *fixes*
exactly this shape: "A `par_*_batch` function's body should be, in essence,
`items.par_iter().map(<the existing sequential function>).collect()`", and
"prefer explicit parallel batch APIs over silently parallelizing an existing
API when a feature is enabled." The family stays at six — one per metric,
matching each metric's default form — and does not grow weighted variants
(§3.7).

---

## 5. Implementation plan

Seven steps. Each compiles, passes `cargo test --workspace --all-features` in
debug and release, and passes `cargo clippy --workspace --all-targets -- -D
warnings` and `cargo fmt --check` on its own. **No benchmarks are run at any
point during implementation** — see §7.

Site snippets under `site/**/*.md` are compiled and executed by
`site/check-snippets.py` through `crates/verbora-examples`, so site breakage is
CI breakage. The site half of each step lands in the same commit as its code;
per `CLAUDE.md` the writing is delegated to `doc-sync` with a self-contained
brief, but it is not deferred to a later commit. `docs/**` is prose only and
may follow.

### Step 1 — Delete `StringMetric` and the six marker types

Pure removal; unblocks every later step by removing six `measure -> f64`
wrappers that would otherwise be rewritten five more times.

- `crates/verbora-core/src/lib.rs:195` — delete the trait (~15 lines).
- `crates/verbora-distance/src/lib.rs:87-177` — delete `use
  verbora_core::StringMetric`, six structs, six impls (~90 lines).
- `crates/verbora-distance/tests/zz_audit_tmp.rs` — delete the file.
- Site: `site/features/distance.md:635-708` (the whole `StringMetric` section,
  including the live snippet asserting `folding.measure("abc","ab") == -1.0`
  and the callout explaining the `-1.0`/`NaN` pair);
  `site/choosing/distance.md:196-230` and the row at `:23`;
  `site/features/core.md:174-195`, `:78`, `:107`, `:116`, `:226` (already
  stale: says 5 impls, there are 6, and omits `Osa`), `:251`, `:566`;
  prose at `site/recipes/fuzzy-matching.md:129`,
  `site/choosing/index.md:84`, `site/features/index.md:42`,
  `site/getting-started/workspace.md:12`.

**Blast radius outside these files: none.** No other crate, test or benchmark
in the workspace references `StringMetric`.

### Step 2 — Cost types and the unit/weighted split

No unit change. Mechanical but wide.

- `levenshtein.rs` — replace `Options` with the three cost types and
  `CostError`/`Operation`; split six functions into twelve; delete
  `is_unit_cost` (`:643`) and its six gates (`:399`, `:418`, `:603`, `:741`,
  `:1955`, `prepared.rs:309`), each becoming a *structural* property of which
  function was called; merge `assert_lowrance_wagner_costs` (`:694`) and
  `lowrance_wagner_costs_are_admissible` (`:653`) into `DamerauCosts::new`;
  keep `repeated_cost` (`:479`) on the weighted path only, the unit path
  multiplying integers. ~98 struct-literal sites in this file.
- `prepared.rs` — delete the weighted fallback branch (`:305-313`) and the
  argument-order doctest (`:265-275`).
- `crates/verbora-spellcheck` — `deletion_index.rs:113` and
  `fuzzy_index.rs:117`: both `fn edit_distance(a,b) -> u32 { levenshtein(a,b,
  &Default::default()).round() as u32 }` are **deleted**; call
  `levenshtein(a, b)` and compare `usize` directly. `edits.rs:411` (test),
  `tests/deletion_index.rs:53`, `tests/fuzzy_index.rs:53`,
  `benches/{fuzzy_index,deletion_index}.rs` lose the same `.round() as u32`.
  `fuzzy_index.rs`'s module doc (`:81-95`), which argues at length that
  arbitrary caller costs are not guaranteed to be metrics, shrinks to a
  sentence: there is no cost parameter.
- Tests that change shape rather than value: four `#[should_panic]` tests
  (`levenshtein.rs:5093`, `:5098`, `:5110`, `tests/parallel.rs:205`) become
  `assert!(DamerauCosts::new(..).is_err())`. The invalid-cost battery listed
  in §4.8 is deleted and replaced per §6.1.
- Benchmarks: `crates/verbora-distance/benches/distance.rs:151`, `:191`;
  `benchmarks/competitive/rust-competitors/{benches/distance.rs,
  examples/distance_memory.rs, tests/distance_correctness.rs}` drop
  `&LevOptions::default()`. **Compile-only — `cargo check --benches`.**
- Site: `site/features/distance.md:230-243`, `:610-615`, table rows `:205`,
  `:518`, `:940`, `:1007-1008`, the `ignore`d API digest at `:975-1020`;
  `site/choosing/distance.md:313`, `:341-357`, `:370-390`, and the
  Lowrance–Wagner rows at `:50`/`:71` (from "panics" to "`DamerauCosts::new`
  returns `Err`"); `site/recipes/interactive.md:70`. A `Choosing the Right
  API` block for the `levenshtein` / `levenshtein_weighted` pair is now
  **required** by `AGENTS.md` and must be written, not deferred.

**Silent breakage in this step: none.** Every consumer either fails to compile
or is semantically unaffected (the metric's *values* do not change).

### Step 3 — The scalar unit, the search redesign, and `verbora-spellcheck`

**The dangerous step.** The unit change and the search redesign land together
because they are genuinely coupled: the byte range is only expressible under
scalars, and `from_utf16_lossy` disappears with the owned `String`.

*Type-level work, mechanical.* Add `impl Unit for char`, `impl BitPeq for char`
and — the one easy thing to miss — `impl DamerauUnit for char`
(`levenshtein.rs:1351`), a third unit-specialised trait alongside the other
two. All three are copies of the existing `u16` impls, which are `FxHashMap`
-based and assume nothing about width; the `u8` impls, whose `[usize; 256]`
and `[u64; 256]` tables *are* width-dependent, serve the ASCII path and stay
verbatim. Then: `Operands::Units(&[char], &[char])`; `dispatch`
(`units.rs:132`) collects `chars()` instead of `encode_utf16()`; both
`with_utf16_units` copies (`levenshtein.rs:464`, `prepared.rs:500`) become
`[char; 64]` stack buffers — the soundness argument survives verbatim, since a
UTF-8 string of *n* bytes has at most *n* scalars just as it has at most *n*
UTF-16 units; `encode_utf16_into` (`levenshtein.rs:527`) becomes a `chars()`
fill; `dice::sanitize`'s re-encoding step disappears (it already iterates
`chars()`); the four hand-rolled `is_ascii` branches that bypass `dispatch`
(`levenshtein.rs:392`, `:410`, `:419`, `:440`) are updated alongside the six
`dispatch` call sites (`hamming.rs:249`, `jaro_winkler.rs:24` and `:439`,
`levenshtein.rs:446`, `:453`, `:1913`).

*Deletions.* Both `utf16_len` implementations (`units.rs:156` and the private
duplicate at `levenshtein.rs:485`); `pub mod units` becomes `mod units`;
`PreparedPattern::unit_len`. `AFFIX_CHUNK`'s doc comment ("16 bytes for `u8`,
32 for `u16`") is restated for 64 bytes at `char`.

*Search.* `SearchResult<'t, D>` per §3.2; `search_impl` (`levenshtein.rs:1913`)
returns borrowed slices with derived byte ranges;
`String::from_utf8_lossy(..).into_owned()` (`:1918`) and
`String::from_utf16_lossy` (`:1930`) both deleted.

*Consequences that need no work.* Affix trimming (`units.rs:184`, `:202`) can
no longer stop mid-character, which was legal over `u16` sequences but is now
structurally impossible. `trim_common_utf8_affixes` (`levenshtein.rs:493`),
which already backs off to char boundaries, becomes exactly aligned with the
inner trim.

**Silent breakages — the whole reason this step needs specific attention:**

1. **`verbora-spellcheck::DeletionIndex` — must move in this commit.** It
   generates deletion sets over `Vec<u16>` (`deletion_index.rs:106`, `:123`,
   `:189`, `:285`, via `crate::units::to_utf16`) *specifically* because
   `levenshtein` counted UTF-16 units. The SymSpell completeness argument —
   deleting up to *n* of the same atomic unit from each side is guaranteed to
   produce a common string — holds **only** while generation and verification
   agree on the unit. Under a scalar metric the u16 sets are too fine-grained
   in the wrong direction: an astral character differing by one scalar costs
   two u16 deletions, so a query and a dictionary word can sit at true
   distance *n* with no u16 deletion of depth ≤ *n* connecting them.
   `neighbors()` then silently returns fewer matches and nothing fails to
   compile. Move generation to `Vec<char>`. The crate's own performance record
   documents this exact bug class as found and fixed during implementation;
   leaving the unit split re-introduces it. Note that `deletion_index` uses
   `to_utf16` from a shared module — switching it to a local `Vec<char>` does
   *not* require touching spellcheck's public `edits_utf16` surface, which is
   that crate's own migration item.
2. **`max_distance` is baked in at build time.** `DeletionIndexBuilder::new`
   fixes the threshold; an index built before this step answers a different
   question after it. Same for `FuzzyIndex`, whose BK-tree edges are keyed by
   distance at insert time — build and query use the same function, so pruning
   stays correct *within* one build, but a tree built pre-change and queried
   post-change has edges in one unit and queries in another, which is unsound.
   Nothing in-tree persists either structure; this is latent, and must be
   stated in the release note.
3. **Two published length gates become unsound and stay green.**
   `site/recipes/fuzzy-matching.md:170` and `site/choosing/distance.md:341`
   filter with `utf16_len(a).abs_diff(utf16_len(b)) <= max_edits` and claim it
   "cannot discard a real match" — true only under UTF-16. Under scalars,
   `"ab"` versus `"ab😀"` has a UTF-16 difference of 2 and a scalar distance of
   1, so a `k = 1` gate discards a true match. Their own assertions are ASCII
   and keep passing. **The mitigation is that `utf16_len` is deleted rather
   than renamed, so both snippets fail to compile.** They become
   `chars().count()` and the lemma in §3.1 is what makes the claim true again.
   `site/recipes/fuzzy-matching.md:184-185`'s instruction to "use `utf16_len`,
   not `str::len` or `chars().count()`" inverts completely.
4. **The competitive suite cannot detect this step.**
   `benchmarks/competitive/rust-competitors/tests/distance_correctness.rs` is
   ASCII-only by design (`load_ascii_pairs()` plus `b'a' + n%26` generators),
   because on ASCII byte = char = UTF-16 unit. It will stay green under any
   unit. A passing competitive run is **not** validation of this step. The same
   holds for `tests/fuzzy_index.rs` (lowercase-ASCII corpus) and
   `edits.rs`'s OSA cross-check (corpus `["", "a", "ab", "cat", "something",
   "café"]`, all BMP) — the latter is the only place spellcheck's own u16 edit
   generator is checked against this crate's metric, and it cannot see the
   drift.
5. **One latent in-tree bug is fixed by construction.**
   `distance_correctness.rs:838` slices `&haystack[off..off + a.len()]`, using
   a UTF-16 offset directly as a byte index — correct today only because the
   corpus is ASCII, and precisely the mistake `site/features/distance.md:936`
   warns readers against. Under `Range<usize>` it is correct by construction.
6. **`fst` classification prose must be re-derived, not widened.**
   `benchmarks/competitive/rust-competitors/benches/fst_fuzzy.rs:7-50` and
   `tests/fst_fuzzy_correctness.rs:10-14` build their `NARROWED_EXACT`
   classification and their ASCII-only restriction on "UTF-16 code units
   versus `fst`'s Unicode scalar values". It is tempting to conclude the two
   metrics now agree everywhere and widen the classification to `EXACT`. **That
   is false.** The narrowing is driven by `fst` 0.4.7's own automaton defect —
   it silently returns incomplete results for same-byte-length multi-byte
   UTF-8 substitutions, e.g. Cyrillic `"аб"`/`"ав"` at any max_distance
   (upstream issue #38) — which is a BMP defect and survives the unit change
   untouched. The prose must be re-derived from that defect, not from the unit.
7. **`verbora-tagger/src/utf16.rs:13`'s cross-reference goes stale.** It says
   "the same dispatch `verbora_distance::units` uses"; the tagger maintains its
   own UTF-16 helpers for Brill-predicate reasons of its own and does not
   depend on this crate. Prose fix only, but the two crates now disagree about
   what "the same dispatch" means.

*Loud tripwire to preserve and run:*
`crates/verbora-spellcheck/tests/deletion_index.rs:109`
(`matches_brute_force_on_astral_heavy_input`) is the **only** test in the
workspace that fails loudly if item 1 is missed. It must not be weakened.

*Test work in this step:* items in §6.2, especially the BMP-only search
battery — the reason the tier-1 defect in §2.5 survived is that no test
covered Latin-1 through CJK search offsets.

*Site:* `site/features/distance.md:53-70`, `:270`, `:324-334`, `:589-601`
(the field table), `:605-624`, `:626-635` (the `encode_utf16()` slicing callout
— **deleted**, its advice becomes wrong), `:721-742`, `:877`, `:936` (pitfall
#4 — **deleted**, the trap no longer exists); `site/choosing/distance.md:155-162`,
`:170`, `:204`, `:341-357`; `site/choosing/decision-trees.md:50-55`;
`site/performance/allocation.md:47-51` (drop "plus a `String` for the result
substring"); `site/performance/zero-copy.md:132`.

### Step 4 — Hamming

- `hamming.rs` — delete `INCOMPARABLE` (`:58`), `hamming_checked` (`:235`) and
  `count_diffs` (`:248`); signature to `Option<usize>` (`:60-118`) with the
  `ignore_case` lane (`:99-117`) deleted; `hamming_slow` (`:134-148`) becomes
  one fused `chars()` pass; kernel return types to `usize` at the boundary
  (accumulators stay `u64`; `fused_ascii_diffs`'s `Option` means "retry" and
  should be renamed so it is not confused with the public one); `:265`'s
  `as i64` becomes `as usize`; `par_hamming_batch` to `Vec<Option<usize>>`.
  Of 13 tests here, four folding tests are deleted, two are retargeted to
  scalars, one is renamed.
- `tests/parallel.rs` — `assert_i64_parity` (`:111-122`) becomes
  `assert_option_parity`; four call sites; `hamming_batch_respects_ignore_case`
  (`:385-389`) deleted.
- `benchmarks/.../tests/distance_correctness.rs:178-183` and `:543-548` —
  `let want = hamming(..); assert!(want >= 0, ...); want as u32` collapses to
  one `if let Some(want)`. **Note `:542`'s `if a.len() == b.len()` byte-length
  gate is a latent non-ASCII bug and is removed by that collapse.**
- Site: `site/features/distance.md:58-68`, `:400-446` (the whole `INCOMPARABLE`
  and İ-folding section), `:903-920`, plus prose rows;
  `site/choosing/distance.md:124-144` — the "`hamming` or `hamming_checked`?"
  section is **deleted, not reworded**: the decision it documents ceases to
  exist. `site/features/core.md:163-171`'s callout loses its Hamming half.
  `docs/COMPETITIVE_BENCHMARKS.md:315`'s recorded divergence
  ("`Result` instead of Verbora's `-1` sentinel") disappears.

**Silent breakage:** callers who passed `ignore_case: true` now compare
case-sensitively — but the parameter's removal is a compile error, so this is
loud. The only silent part is the unit change to the *comparability relation*,
already landed in Step 3.

### Step 5 — Jaro and Jaro–Winkler

- `jaro_winkler.rs:80-81` — delete the `max_len < 2 => (0,0)` branch; compute
  `w = (max_len / 2).saturating_sub(1)` once. `jaro_scalar`'s own window
  (`:133`) changes identically, which also lets `:133`, `:162`, `:163` drop
  their `isize` casts.
- `:70-72` — split the empty guard: both-empty returns `1.0` before
  either-empty returns `0.0`.
- **Test helpers must move in lockstep.** `jaro_forced` (`:637-640`) and
  `jaro_prefix_reduced` (`:1110-1112`) each carry their own
  `if max_len < 2 { return 0.0 }`, and
  `jaro_large_randomized_differential_splitmix` (`:1057`) can draw
  `len1 = len2 = 1`. Not updating them produces a spurious differential
  failure.
- `:392` — delete the identity short-circuit.
- `:445-451` — `l = min(4, common_prefix_len(s1, s2))`, deleting the
  `None == None` saturation and the 19-line rustdoc block (`:419-437`) written
  to explain another runtime's `undefined === undefined`.
- `:12-20` — delete `Options` entirely; `jaro_winkler(a, b)` and
  `par_jaro_winkler_batch(pairs)`.
- Tests deleted (they pin behaviour this contract rejects):
  `single_char_window_is_negative_and_yields_zero` (`:546-551`, rewritten —
  `jaro("a","b") == 0.0` survives, the title's premise does not),
  `single_char_ignore_case_exposes_the_prefix_quirk` (`:581-594`),
  `prefix_counter_saturates_at_four` (`:596-602`, rewritten per §6.4),
  `supplied_dj_short_circuits` (`:567-579`).
- Competitive suite: `strsim` and `rapidfuzz` do not clamp the window, so they
  return `0.0` for `jaro("a","a")`. If `load_ascii_pairs()` or the randomized
  sweeps can emit a length-1 pair, that agreement assertion now fails
  **correctly** and must be narrowed with a note stating Verbora's
  degenerate-window definition and why.
- Site: `site/recipes/fuzzy-matching.md:80`, `site/recipes/autocomplete.md:130`,
  `site/features/phonetic-index.md:205`.

### Step 6 — Dice

- `dice.rs:42-60` — delete `sanitize` (this is the crate's last
  `verbora_core` use); `:72-75` — delete the padding branch; `:19-35` — add the
  `|A| + |B| == 0` branch before the division. Bigram keys become `(char, char)`
  (8 bytes, from 4).
- Delete the `verbora-core` dependency from `Cargo.toml`.
- Tests rewritten: `empty_pair_is_nan` (`:163-165`),
  `sanitize_folds_case_and_collapses_space` (`:180-183`),
  `single_char_is_padded_not_dropped` (`:192-196`),
  `astral_characters_use_code_unit_pairs` (`:199-205`).
- Site: the Dice section of `site/features/distance.md` and
  `site/choosing/distance.md`'s Dice row must lead with the caller-side
  lowercasing recipe, per §3.5.

**Silent breakage:** the widest in the contract for *callers* — every input
with an uppercase letter or any whitespace changes score, and nothing fails to
compile. The release note leads with this, not with the astral cases.

### Step 7 — Documentation, benchmark prose, and measurement

- Remove the stale benchmark tables from all six `par_*_batch` rustdocs and
  from `site/`'s distance performance rows; mark them unmeasured.
- `docs/PERFORMANCE_GAPS.md` entry 29 (the 283× fuzzy-substring-search memory
  gap versus `triple_accel`) and
  `benchmarks/competitive/results/distance-memory.json` are stale on landing,
  because search no longer allocates a result `String`. State them stale; do
  not re-derive.
- `docs/design/rust-native-migration.md` — record the standing findings in §7
  and mark per-crate item 1 complete.
- Correct the counts already known stale: `site/features/distance.md:18`
  ("150 unit tests and 9 doctests"), `site/features/core.md:226` and
  `site/getting-started/workspace.md:12` ("5 `StringMetric` impls").
- **Then** ask whether to run a measurement campaign, naming the targets and
  the expected duration. Benchmarks are never launched on the implementer's own
  initiative.

---

## 6. Test obligations

Every expected value below comes from a published definition or from
arithmetic shown inline. **No test may assert "matches current behaviour",**
and no expected value may be produced by running the new code. Seven existing
tests currently derive their expectations from UTF-16-denominated from-scratch
oracles (`damerau_levenshtein_agrees_with_the_from_scratch_reference_on_random_corpora`,
`osa_agrees_...`, `damerau_unit_many_distinct_symbols`,
`damerau_unit_fast_path_agrees_on_utf16_input`,
`damerau_affix_trim_agrees_on_near_identical_corpora_at_scale`,
`jaro_bit_kernels_agree_on_utf16_input`,
`jaro_prefix_reduction_agrees_on_near_identical_corpora_at_scale`); their
oracles must be **re-derived from the recurrence over scalars**, never
re-recorded.

### 6.1 Edit distances and costs

**Definitional fixtures.** `levenshtein("kitten","sitting") == 3`;
`levenshtein("ab","ba") == 2` and `osa("ab","ba") == 1` and
`damerau_levenshtein("ab","ba") == 1`; the discriminator
`damerau_levenshtein("CA","ABC") == 2` versus `osa("CA","ABC") == 3`; the
symmetry pair `d("bb","abbb") == d("abbb","bb") == 2` for unrestricted
Damerau, which the settled Step 0 work established. *(Corrected: earlier
revisions of this clause wrote `== 1`, which no unit-cost edit distance can
return for that pair. The operands differ by two scalars, and the length
lemma below — each insertion or deletion moves the count by one, each
substitution by none — puts every distance between them at `>= 2`; two
insertions achieve it, so the value is exactly `2`, in both argument orders.
The fixture exists for the **symmetry**, which was the original defect, not
for the magnitude, and the symmetry is unaffected. The code asserted `2`
throughout.)*

**Empty operands.** `levenshtein("", t) == t.chars().count()` over a corpus
spanning ASCII, BMP and astral; `levenshtein("","") == 0`. Weighted:
`levenshtein_weighted("", t, c)` equals the left-to-right fold of
`c.insertion()` added *t.chars().count()* times, asserted bitwise against an
independently written fold — not against a multiplication.

**Equivalence of the two tiers.**
`levenshtein(a,b) as f64 == levenshtein_weighted(a,b, &LevenshteinCosts::new(1.0,1.0,1.0)?)`
over the existing randomized corpus, and likewise for the OSA and Damerau
pairs. **This replaces `affix_trim_never_runs_for_weighted_costs`**, whose
negative-cost witnesses become unconstructable (§4.8): the property it proved
— that trimming is applied exactly where it is sound — is now structural, and
what remains to pin is that the two tiers agree.

**Weighted correctness.** The weighted DP against a from-scratch dynamic
program over admissible cost sets — `(0.5, 1.5, 0.75)`, all-zero, unit, and
`(0.1, 0.2, 0.15)` — written independently of the implementation.
`one_row_weighted_matches_two_row_oracle_bit_for_bit` survives with its
`INFINITY`, `NaN` and negative entries removed.

**Cost validation.** `LevenshteinCosts::new` rejects `NaN`, `±INFINITY` and
any negative; accepts zero. `DamerauCosts::new` additionally rejects
`2t < i + d`, tested at the inclusive boundary in the constructor's own f64
arithmetic: `(1.0, 1.0, _, 0.999)` → `1.998 < 2.0` rejected;
`(2.0, 2.0, _, 1.5)` → `3.0 < 4.0` rejected; `(0.5, 0.5, _, 0.5)` →
`1.0 >= 1.0` accepted; and `(0.1, 0.2, _, (0.1 + 0.2) / 2.0)` accepted while
`(0.1, 0.2, _, 0.15)` is **rejected**. *(Corrected during Step 2: an earlier
revision of this clause claimed `(0.1, 0.2, _, 0.15)` was accepted "→
`0.3 >= 0.3`". It is not, and the arithmetic it showed is not IEEE-754's:
`0.1 + 0.2` rounds to `0.30000000000000004`, so the minimum admissible
transposition is `0.15000000000000002` and `0.15` is below it. The normative
predicate in §3.1 is unchanged; only this fixture's expectation was wrong.
Both halves are now pinned as tests, because the near-miss is precisely the
case an epsilon tolerance would silently paper over.)* `OsaCosts::new`
accepts every finite non-negative set, including `transposition: 0.25`.

**The predicate's spelling**, pinned separately from its boundary, because
the two forms in §3.1 diverge only where `insertion + deletion` leaves the
normal range and nothing else in this section reaches there:
`(f64::MAX, f64::MAX, _, f64::MAX)` **accepted** — both sides of the
predicate are `+∞`; `(f64::MAX, f64::MAX, _, f64::MAX / 2.0)` **rejected**,
since `2 * (f64::MAX / 2)` is `f64::MAX` exactly and `f64::MAX < ∞`; and, at
the other end, `(u, 4u, _, 2u)` **rejected** while `(u, 4u, _, 3u)` is
accepted, where `u` is the smallest positive `f64` — `4u < 5u` against
`6u >= 5u`. Each of the three rejected-or-accepted verdicts is the opposite
of what the rearranged form `t < (i + d) / 2.0` produces. Plus: every
rejection reports a **finite** `minimum`, swept over the corners of the range
(`0`, `u`, `f64::MIN_POSITIVE`, `0.5`, `1.0`, `f64::MAX / 4`, `f64::MAX / 2`,
`f64::MAX`) in all three positions.

**Error equality.** `CostError` is reflexive under `PartialEq` including in
the `NaN` case: `LevenshteinCosts::new(f64::NAN, 1.0, 1.0)` equals itself and
equals a written-out `Err(CostError::NotFinite { .. })` with a `NaN` payload.
Nine distinct rejections, compared pairwise, are equal exactly on the
diagonal; and the same rejection reached through two different constructors
compares equal, since the error is about the cost and not about which type
refused it.

**Saturation.** `levenshtein_weighted(&"a".repeat(64), "", c)` with `c` all
`f64::MAX` is `+∞` and not `NaN`, for all three variants and for the weighted
searches; `total_cmp` against a finite distance orders it `Greater`. A cost
set that saturates in one direction only still returns the cheap script:
`(f64::MAX, 1.0, 1.0)` on the same pair is `64.0`. The unit tier cannot
saturate at all.

**Algebraic properties**, as property tests over a corpus mixing ASCII,
Latin-1, Greek, Cyrillic, CJK and astral: identity, discernibility, symmetry
(unit costs and weighted with `insertion == deletion`), the triangle
inequality for `levenshtein` and `damerau_levenshtein`, and the length lemma
`|a.chars().count() - b.chars().count()| <= levenshtein(a,b)`.

**No panics, and no `NaN`.** A sweep asserting that no entry point panics for
any operand pair drawn from the corpus, under every cost set in the grid, and
that every weighted return is a number (`!is_nan()`, and `>= 0.0`, which `+∞`
satisfies and `NaN` does not).

The grid **declares** per entry whether `DamerauCosts::new` must accept it,
worked out by hand from `2 * transposition >= insertion + deletion`, and the
sweep asserts that verdict in both directions. It must not write `if let
Ok(costs) = DamerauCosts::new(..)` and skip the rest: that turns a rejection
into a silent `continue`, so a cost set the constructor rejects *wrongly*
disappears from a sweep whose entire claim is that it covers everything the
type system admits. This is not hypothetical. The all-`f64::MAX` row was
skipped exactly that way for as long as the constructor evaluated the
rearranged predicate, which is why the over-rejection above survived; and the
grid carries a genuinely sub-threshold entry so the rejecting branch is
exercised rather than merely permitted.

### 6.2 Search

**The invariants**, over a brute force across every substring of the target
that shares no code with the search routines: `&target[r.range()] ==
r.substring()`; `metric(source, r.substring()) == r.distance()` exactly; and
the tie-break rule of §3.2.

**The BMP battery — the gap that let the tier-1 defect survive.** The current
external-contract test uses `[b"ab", b"abc"]`, and the astral test compares
against an oracle sharing the same slicing. Add a corpus spanning Latin-1
(`"Zürich, Berlin, Wien"`), Greek, Cyrillic, Thai, Hangul, Arabic and CJK,
*plus* astral, and assert both invariants above for every variant and every
cost set in the grid. Specifically pin
`levenshtein_search("Berlin", "Zürich, Berlin, Wien").range() == 9..15` — the
byte offset, derived by counting UTF-8 bytes, not by running the code.

**Degenerate table** of §3.2, asserted directly, for all three variants.

**No fabrication.** For every result, `target.contains(r.substring())`. This
is implied by invariant (1) and asserted separately because it is the property
whose violation was invisible for so long.

**Doctests.** None of the three `*_search` functions has a doc example today.
Each gains one, including the borrowed-slice and highlight-range shapes.

### 6.3 Hamming

**Definitional fixtures**, counted from `d = |{i : x_i ≠ y_i}|`:
`karolin`/`kathrin` → `Some(3)` (positions 2, 3, 4); `karolin`/`kerstin` →
`Some(3)` (1, 3, 4); `1011101`/`1001001` → `Some(2)` (2, 4);
`2173896`/`2233796` → `Some(3)` (1, 2, 4); `("","")` → `Some(0)`;
`aaaa`/`bbbb` → `Some(4)` (the bound *n*); `abc`/`ab` and `ab`/`abc` → `None`.

**Unit clauses**, each asserted directly: the two rows of §2.5's astral table
that involve Hamming — `hamming("😀","ab") == None` and
`hamming("😀","𝕳") == Some(1)` — plus `hamming("é","a") == Some(1)`, two
bytes against one, one scalar each. That last case closes a real gap: the
existing test covers only the converse (equal bytes, unequal units), so
nothing pins that the ASCII fast lane's byte-length gate must **fall through**
rather than reject. *(Corrected: earlier revisions said "the six rows of
§2.5's astral table that involve Hamming". That table has seven rows in
total, of which two name `hamming`; the count was wrong twice over.)*

**Case clauses.** `hamming("ABC","abc") == Some(3)`; the caller-side
equivalence `hamming(&a.to_lowercase(), &b.to_lowercase())`; and
`hamming(&"ß".to_lowercase(), &"SS".to_lowercase()) == None`, pinning that
lowercasing is not case folding *where the caller can see it*.

**Algebraic properties**, as property tests over the mixed-script corpus:
identity, discernibility, symmetry, `d <= a.chars().count()`, comparability
iff equal scalar count, the triangle inequality over comparable triples, and
`hamming(a,b) == Some(d) ⟹ levenshtein(a,b) <= d`.

**Kernel differential**, retained and retargeted: the tiered ASCII lane
against the fused `chars()` oracle over a corpus straddling lengths
0/1/7/8/9/15/16/17 and block boundaries 2040/4080/8160, with both operands
non-ASCII in half the draws.

### 6.4 Jaro and Jaro–Winkler

**Published worked examples**, with the arithmetic shown:

- `jaro("MARTHA","MARHTA")` — `n = 6`, `w = 2`; M, A, R match in place, T↔H
  cross, A matches; `m = 6`, raw transpositions 2 so `t = 1`;
  `(6/6 + 6/6 + 5/6)/3 = 17/18`. `jaro_winkler` = `17/18 + 3·0.1·(1/18) =
  17.3/18 ≈ 0.9611`.
- `jaro("DIXON","DICKSONX")` — `n1 = 5`, `n2 = 8`, `w = 3`; X is unreachable
  (`|2−7| = 5 > 3`); `m = 4`, `t = 0`; `(4/5 + 4/8 + 4/4)/3 = 2.3/3`.
  `jaro_winkler` = `2.3/3 + 2·0.1·(1 − 2.3/3) ≈ 0.8133`.
- `jaro("DWAYNE","DUANE")` — `m = 4`, `t = 0`;
  `(4/6 + 4/5 + 4/4)/3 = 2.4666…/3`. `jaro_winkler` = `+ 1·0.1·(1 − that)
  = 0.84` exactly.
- `jaro("aaaa","aa")` — `w = 1`; greedy claims `j = 0` then `j = 1`, and the
  remaining `i` have no free `j` in window; `m = 2`, `t = 0`;
  `(0.5 + 1 + 1)/3`.

**Identity**, bitwise, for `x` in `{"", "a", "ab", "abcd", "abcdefgh",
"aaaa", "😀", "क्षि"}` and for a 20-character operand past the scalar-kernel
threshold: `jaro(x,x).to_bits() == 1.0f64.to_bits()` and the same for
`jaro_winkler`. `jaro_winkler` must reach `1.0` through the formula with the
short-circuit removed.

**The window clamp**, the one class that changes: `jaro("a","b") == 0.0`,
`jaro("x","x") == 1.0`, and `jaro("aaaa","aa")` unchanged, demonstrating the
clamp is invisible for `max >= 2`.

**Empty operands**: `jaro("","") == 1.0`; `jaro("","a") == jaro("a","") ==
0.0`; `jaro("","abcdef") == 0.0`.

**Prefix length**, replacing the test that currently asserts `4` for
`("ab","ab")`: `prefix_len("ab","ab") == 2`; `("","") → 0`; `("a","a") → 1`;
`("abc","abc") → 3`; `("abcd","abcd") → 4`; `("abcdefg","abcdefg") → 4` (the
cap); `("abz","abx") → 2`; `("a","ab") → 1`; `("","abc") → 0`.

**Boost is zero when prefixes differ**: `jaro_winkler("abcd","zbcd").to_bits()
== jaro("abcd","zbcd").to_bits()`.

**Evaluation order**, pinning the fractional halving and the three-division
grouping: `jaro("abc","bcaaaa")` — `n1 = 3`, `n2 = 6`, `w = 2`, `m = 3`, raw
transpositions 3 so `t = 1.5` — must equal
`(((3.0/3.0) + (3.0/6.0) + ((3.0 - 1.5)/3.0))/3.0)` bitwise.

**Range, totality, symmetry and strict identity** as a property test over at
least 50,000 randomized pairs, lengths 0..=300, alphabets `{a}`, `ab`, `abc`,
the Latin alphabet, a BMP alphabet and an astral alphabet: finite, in
`[0,1]`, bit-identical under argument swap, and `v == 1.0` iff `a == b`. Plus
an exhaustive sweep over alphabet `ab` up to length 8 for the identity-only-at-
equality clause.

### 6.5 Dice

**Arithmetic from the definition**: `("night","nacht") → 2·1/(4+4) = 0.25`;
`("abc","abd") → 2·1/(2+2) = 0.5`; `("ab","abc") → 2·1/(1+2) = 2/3`;
`("abc","xyz") → 0.0`; `("Hello  World","hello world") → 14/21`;
`("  padded  ","padded") → 10/13`.

**Degenerate rule**, replacing `empty_pair_is_nan`: `("","") → 1.0`;
`("a","a") → 1.0`; `("a","b") → 0.0`; `("","a") → 0.0`; `("a","ab") → 0.0`
(one set empty: `2·0/(0+1)`); `(" "," ") → 1.0`; `(" ","\t") → 0.0`;
`("\u{FEFF}"," ") → 0.0`.

**Totality**: over the cross product of
`["", " ", "\t", "\n", "\u{FEFF}", "\u{0085}", "a", "😀", "  "]` with itself,
every result is finite, in `[0,1]`, and bit-identical under argument swap.

**No padding**: `dice("a","a b") == 0.0`, `dice("a","a a") == 0.0`, and the
internal `bigrams` of a one-scalar operand is empty.

**No preprocessing**: `dice("ABC","abc") == 0.0`; `dice("a\u{FEFF}b",
"a\u{FEFF}b") == 1.0` and `dice("a\u{FEFF}b","ab") < 1.0`, pinning that the
foreign whitespace class is gone.

**Identity is not injective**: `dice("aaaa","aa") == 1.0`, kept and
documented.

**Range and symmetry** as a property test over at least 50,000 randomized
pairs including whitespace-heavy, all-identical-character, BMP and astral
operands.

### 6.6 Cross-function

**The degenerate table of §3.4** asserted across all three similarities in one
test:

```text
("", "")  -> 1.0     ("", "a") -> 0.0     ("a", "") -> 0.0
("a","a") -> 1.0     ("a","b") -> 0.0
```

**No similarity ever returns a non-finite value**, over the union of every
degenerate corpus above.

**`PreparedPattern` parity**: `p.levenshtein(t) == levenshtein(p.pattern(), t)`
and `p.osa(t) == osa(p.pattern(), t)` over the randomized corpus, covering the
empty pattern, ASCII pattern with non-ASCII target, and the affix-trim
crossover.

---

## 7. `UNMEASURED` — what the next benchmark campaign must answer

No benchmark was run in producing this contract, and none is run during
implementation (`CLAUDE.md`; `docs/design/rust-native-migration.md` §
"Performance baseline"). Every item below is a structural argument, not a
measurement, and must not be published as a number until a full-precision run
exists.

1. **`UNMEASURED` — BMP non-ASCII operand width.** For all real non-ASCII
   text (Cyrillic, Greek, Hebrew, Arabic, Indic, BMP CJK, accented Latin) the
   scalar unit has *identical* element counts and identical kernel iterations
   to UTF-16, and pays 2× scratch bytes per element on one linear pass
   alongside a quadratic kernel — plus 2× on downstream `Vec` footprints.
   (`DeletionIndex`'s persisted key was one such footprint when this was
   written; it no longer is. The index now stores a 64-bit hash per deletion
   sequence rather than the sequence, so its retained size is independent of
   the unit's width — see `docs/PERFORMANCE_MATRIX.md`'s own entry.) Whether that is
   observable in the ~16–40 ns short-operand regime, and specifically in the
   single-word Myers kernel where there is one word of state per element and
   the width cost is not amortised against `ceil(m/64)` words of work, is the
   single most important open question. **The designated fallback, recorded so
   it is not rediscovered: a three-arm `Bytes` / BMP-`u16` / `char` dispatch.**
   It is not pre-built, because the I-cache cost of a third arm is itself
   unmeasured.
2. **`UNMEASURED` — astral operand reduction.** Astral text gets *fewer*
   elements: a 4× smaller DP matrix and half the bit-parallel blocks, with the
   per-block scan also halving. Direction is certain; magnitude is not.
3. **`UNMEASURED` — `Option<usize>` return ABI on Hamming.** The two-register
   return costs the SWAR tier the tail call the compiler currently emits and
   adds a push/pop pair plus a move per early exit. On the crate's cheapest
   metric — already the widest competitive margin against `triple_accel` — that
   is a nonzero fraction of a ~6.6 ns call. The designated mitigation is
   `#[inline]` on `hamming` (it is not inlined today), which should fold the
   discriminant into the caller's `match` under the workspace's thin LTO. **Do
   not add it on intuition;** batch it into the measured run.
4. **`UNMEASURED` — search allocation.** Returning a borrowed `&str` removes
   one allocation per call on both the ASCII and non-ASCII arms, and adds one
   `O(m)` `char_indices` walk on the non-ASCII arm only. Every published
   fuzzy-substring-search **memory** figure is stale on landing —
   `docs/PERFORMANCE_GAPS.md` entry 29 (the 283× gap versus `triple_accel`)
   and `benchmarks/competitive/results/distance-memory.json`.
5. **`UNMEASURED` — Dice.** Deleting `sanitize` removes two `String`
   allocations and two `Vec` allocations per call, and opens the door to an
   ASCII fast path that Dice uniquely lacks (it never calls `dispatch` at
   all). Bigram keys grow from 4 to 8 bytes. Net direction unknown.
6. **`UNMEASURED` — unit-cost dispatch.** Removing `is_unit_cost` deletes
   three `f64` comparisons per call from the hottest path; the weighted path
   loses nothing.
7. **`UNMEASURED` — `[char]` slice equality.** `AFFIX_CHUNK = 16` becomes 64
   bytes at `char`. Whether the chunked compare still lowers to a single
   `memcmp` must be confirmed from the emitted code before the doc comment's
   "at least one SIMD register" rationale is restated as fact.
8. **`UNMEASURED` — every `par_*_batch` crossover table.** All six embedded
   tables are invalidated by the signature and algorithm changes above.

Two things that are **not** performance questions and must not be deferred to
a benchmark: the correctness of the search invariants (§6.2) and the
completeness of `DeletionIndex` (§5, Step 3, item 1). Both are settled by
tests, and both are what a green competitive run will fail to tell you.
