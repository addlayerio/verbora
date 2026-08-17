# String distance results

26 benchmarks, `verbora-distance` against the reference v8.1.1 on identical inputs.
**Median speedup 23.4×**, range **1.4×–3307.4×**.

<div class="callout callout-good">
<strong>The Levenshtein-family and Jaro–Winkler rows below run on
bit-parallel kernels.</strong> Plain <code>levenshtein</code> uses
Myers'/Hyyrö's bit-vector algorithm, with flat pattern-preprocessing tables
(no hash map) and a single-word fast path covering every operand from 1 to
64 units. Restricted Damerau (OSA) has its own bit-parallel kernels
(Hyyrö's 2003 transposition extension of Myers, unit costs only).
Unrestricted-Damerau distance calls use a two-row snapshot kernel instead of
the full cost + parent matrices. Jaro and Jaro–Winkler use bit-parallel
match-flagging kernels. See
<a href="competitive#levenshtein">the competitive benchmarks page</a> for
the mechanisms and how parity against the scalar implementations was
verified. The reference is measured as shipped, unmodified; every number
below comes from the Rust side.
</div>

The [method](index.md) matters more than the numbers: both sides read the same
files, the reference is measured warm, and the test suite proves both compute the
same values.

## All 26

| Benchmark | the reference | Verbora | Speedup |
|---|--:|--:|--:|
| `levenshtein/ascii/4` | 791.0 ns | 14.7 ns | **53.8×** |
| `levenshtein/ascii/16` | 11.07 µs | 41.8 ns | **264.8×** |
| `levenshtein/ascii/64` | 173.85 µs | 165.1 ns | **1053.0×** |
| `levenshtein/ascii/256` | 3.08 ms | 2.13 µs | **1446.0×** |
| `levenshtein/ascii/1024` | 96.18 ms | 29.08 µs | **3307.4×** |
| `levenshtein/cyrillic/16` | 12.23 µs | 266.4 ns | **45.9×** |
| `levenshtein/cyrillic/256` | 3.69 ms | 3.97 µs | **929.5×** |
| `levenshtein_variants/plain_2row` | 177.17 µs | 166.1 ns | **1066.6×** |
| `levenshtein_variants/damerau_restricted_3row` | 190.07 µs | 179.4 ns | **1059.5×** |
| `levenshtein_variants/damerau_unrestricted_matrix` | 304.11 µs | 7.75 µs | **39.2×** |
| `levenshtein_variants/search_matrix` | 176.86 µs | 12.79 µs | **13.8×** |
| `jaro_winkler/4` | 27.2 ns | 15.3 ns | **1.8×** |
| `jaro_winkler/16` | 532.3 ns | 79.4 ns | **6.7×** |
| `jaro_winkler/64` | 4.30 µs | 130.7 ns | **32.9×** |
| `jaro_winkler/256` | 63.76 µs | 1.77 µs | **36.0×** |
| `jaro_winkler/1024` | 594.77 µs | 10.34 µs | **57.5×** |
| `dice/4` | 346.4 ns | 106.9 ns | **3.2×** |
| `dice/16` | 1.21 µs | 308.1 ns | **3.9×** |
| `dice/64` | 4.58 µs | 1.00 µs | **4.6×** |
| `dice/256` | 18.48 µs | 3.17 µs | **5.8×** |
| `dice/1024` | 80.10 µs | 10.61 µs | **7.5×** |
| `hamming/4` | 9.0 ns | 6.6 ns | **1.4×** |
| `hamming/16` | 43.2 ns | 9.7 ns | **4.5×** |
| `hamming/64` | 133.5 ns | 20.5 ns | **6.5×** |
| `hamming/256` | 599.7 ns | 72.7 ns | **8.2×** |
| `hamming/1024` | 2.16 µs | 275.3 ns | **7.8×** |

## Where the Levenshtein win comes from

The reference always materialises a full `(n+1)×(m+1)` matrix of heap-allocated cell
objects — each holding a cost and a parent coordinate — even when the caller
wants only the final scalar. That is `O(nm)` allocations of pointer-chased
objects.

Verbora picks the smallest structure that can answer the question asked, and —
wherever a faster *algorithm* exists — the fastest algorithm, not just the
fastest data structure:

| Mode | Working set | Why |
|---|---|---|
| distance, no Damerau, unit cost | **bit-vector** (one `u64` word per 64 units of the shorter operand) | Myers'/Hyyrö's bit-parallel algorithm computes the same answer in `O(nm/64)` bitwise operations rather than `O(nm)` scalar cell updates; the pattern-preprocessing table is a flat array on the byte path (no hashing), and the single-word path covers operands of 1–64 units — see [the competitive benchmarks page](competitive.md#levenshtein) for the full story |
| distance, no Damerau (fallback, weighted costs) | **2 rows** | a cell needs only `up`, `left`, `diag` |
| distance, restricted Damerau, unit cost | **bit-vector** (word + block) | Hyyrö's 2003 transposition extension of Myers computes OSA in the same `O(nm/64)` bitwise style |
| distance, restricted Damerau (fallback, weighted costs) | **3 rows** | transposition reaches row − 2 |
| distance, unrestricted Damerau | **2 rows + per-symbol row snapshots** | transposition reaches an arbitrary earlier row, so the kernel snapshots each symbol's last matching row into an arena (integer cells: `u16` while the combined length fits, `u32` beyond) instead of materialising the cost + parent matrices |
| search, any variant | full matrix | the match start is recovered by walking parents |

The bit-parallel kernels are why `levenshtein/ascii/1024` posts **3307.4×** —
the largest gap on this page by a wide margin. Two of the variants rows carry
legacy names that no longer describe the code path they exercise:
`levenshtein_variants/plain_2row` and `damerau_restricted_3row` are named for
the row-based scalar DP they originally targeted, but at 64 characters (their
fixed input size) both now run bit-parallel kernels instead — hence
**1066.6×** and **1059.5×**, not what a literal two-row or three-row scalar
sweep would produce.
`damerau_unrestricted_matrix` is similarly legacy-named: distance mode never
builds a matrix at all here — its **39.2×** comes from the two-row snapshot
kernel described above. Only `search_matrix` is still what its name says —
the full cost + parent matrix, required for the backtrace — and its
**13.8×** is the structural-savings story below.

Where the full matrix *is* required — now only in search mode — it is stored
struct-of-arrays: costs in one
flat `Vec<f64>`, parents in another. The hot cost sweep stays contiguous, and the
parents — touched only during backtracking — never pollute a cache line during
it.

## Where the wins are smaller, and why

**`hamming/4` (1.4×) and `jaro_winkler/4` (1.8×).** At four characters the work
is a handful of comparisons; both runtimes are dominated by call overhead, and the reference engine
optimises this shape very well. Small, genuine wins are the honest expectation
here.

**Jaro–Winkler beyond four characters (6.7×–57.5×).** Jaro and Jaro–Winkler
run on bit-parallel match-flagging kernels (a single-word path, then a block
extension, with a scalar loop kept for inputs of 16 units or fewer), which
preserve the fractional transposition semantics exactly. The ratio *rises*
with input size instead of falling: a scalar implementation would do the
same quadratic work as the reference at 1024 units, while the bit-parallel
kernels do not.

**Dice (3.2×–7.5×).** Dominated by hashing. Verbora hashes `(u16, u16)` tuples
with `FxHashMap` instead of allocating a `String` per bigram as the reference
does; the win grows with input size as that allocation pressure compounds.

**Cyrillic vs ASCII.** `levenshtein/cyrillic/256` at 3.97 µs against
`levenshtein/ascii/256` at 2.13 µs — about 86% slower. Promoting non-ASCII
operands to `Vec<u16>` for exact UTF-16 semantics is a fixed cost, and the
`u16` bit-vector kernel builds its pattern-preprocessing table in a hash
map where the byte path uses a flat 256-entry array. The absolute
difference is still under two microseconds — and the Cyrillic row still
beats the reference by **929.5×**.

## A measured regression, and its fix

The first run recorded `jaro_winkler/4` at **0.6×** — Verbora *slower* than
the reference.

The cause was two `vec![false; len]` allocations per call, for the match flags.
The reference engine's `new Array(4)` is nearly free; `malloc` is not.

Moving the match flags to a stack buffer for inputs up to 128 code units took
the benchmark from **48.6 ns to 15.3 ns** — 0.6× to 1.8× — with the test
suite re-run and still green. Words are short by nature, so the stack path
is the common path rather than a micro-optimisation for a rare case.

<div class="callout callout-good">
<strong>Why this is on the site rather than in a commit message.</strong> Without
measuring, "it's Rust, so it's fast" would have shipped a regression. The whole
argument for the project's benchmark discipline rests on cases like this one, so
it is published rather than quietly fixed.
</div>

## What these numbers do *not* tell you

**They are one machine.** An i9-14900KF with 125 GiB of RAM. Ratios on your
hardware will differ.

**They are per-call microbenchmarks.** In a real workload the interesting
question is usually *how many calls you make*, not how fast each one is. A trie
or phonetic prefilter that removes 99% of the comparisons beats a 20× faster
comparison. See [Fuzzy name matching](../recipes/fuzzy-matching.md).

**They do not cover the other crates.** No tokenizer, phonetics, n-gram,
normalizer, inflector or trie comparison has been published. Do not extrapolate.

**They say nothing about memory.** Allocation counts and peak RSS are not yet
instrumented.

## Reproducing

```bash
python3 tools/bench-data/generate.py        # shared inputs (run once)
cargo bench -p verbora-distance          # Verbora, via Criterion
```

Full detail in [Reproducing them](reproducing.md).
