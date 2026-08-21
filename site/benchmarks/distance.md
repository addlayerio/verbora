# String distance results

Historical paired snapshot: 26 benchmarks, `verbora-distance` against a
widely-used JavaScript NLP library (v8.1.1) on identical inputs. **Median
speedup 23.4×**, range **1.4×–3307.4×**. The current Verbora-only shape suite
is reported separately below rather than being mixed into this paired result.

<div class="callout callout-good">
<strong>The Levenshtein-family and Jaro–Winkler rows below run on
bit-parallel kernels.</strong> Plain <code>levenshtein</code> uses
Myers'/Hyyrö's bit-vector algorithm, with flat pattern-preprocessing tables
(no hash map) and a single-word fast path covering every operand from 1 to
64 units. <code>osa</code> (optimal string alignment, the restricted Damerau
rule) has its own bit-parallel kernels (Hyyrö's 2003 transposition extension
of Myers, unit costs only). <code>damerau_levenshtein</code> — the canonical
unrestricted rule — computes its distances with Zhao–Sahni's linear-space
algorithm at unit costs instead of the full cost + parent matrices, and falls
back to the full matrix only for weighted costs. Jaro and Jaro–Winkler use bit-parallel
match-flagging kernels. See
<a href="competitive#levenshtein">the competitive benchmarks page</a> for
the mechanisms and how parity against the scalar implementations was
verified. The JavaScript library is measured as shipped, unmodified; every
number below comes from the Rust side.
</div>

The [method](index.md) matters more than the numbers: both sides read the same
files, the JavaScript library is measured warm, and the test suite proves both
compute the same values.

## All 26

| Benchmark | JS library | Verbora | Speedup |
|---|--:|--:|--:|
| `levenshtein/ascii/4` | 791.0 ns | 14.7 ns | **53.8×** |
| `levenshtein/ascii/16` | 11.07 µs | 41.8 ns | **264.8×** |
| `levenshtein/ascii/64` | 173.85 µs | 165.1 ns | **1053.0×** |
| `levenshtein/ascii/256` | 3.08 ms | 2.13 µs | **1446.0×** |
| `levenshtein/ascii/1024` | 96.18 ms | 29.08 µs | **3307.4×** |
| `levenshtein/cyrillic/16` | 12.23 µs | 266.4 ns | **45.9×** |
| `levenshtein/cyrillic/256` | 3.69 ms | 3.97 µs | **929.5×** |
| `levenshtein_variants/plain_myers_unit` | 177.17 µs | 166.1 ns | **1066.6×** |
| `levenshtein_variants/osa_bit_vector` | 190.07 µs | 179.4 ns | **1059.5×** |
| `levenshtein_variants/damerau_zhao_sahni` | 304.11 µs | 7.75 µs | **39.2×** † |
| `levenshtein_variants/search_matrix` | 176.86 µs | 12.79 µs | **13.8×** ‡ |
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

† The `damerau_zhao_sahni` row's Verbora figure — and therefore its speedup —
was captured while unrestricted Damerau still ran the old row-snapshot kernel
of a pinned, non-canonical recurrence, before the move to Zhao–Sahni's
linear-space algorithm with common-affix trimming. The JavaScript side is
unaffected. The pair is retained as the record of that run and is **pending
re-measurement**; no replacement number has been invented for it.

‡ The `search_matrix` row's Verbora figure — and therefore its speedup — was
captured while plain-Levenshtein search still built the full cost + parent
matrix, before the move to the same per-column bit-vector deltas the
distance-mode kernel uses. The JavaScript side is unaffected. The pair is
retained as the record of that run and is **pending re-measurement**; no
replacement number has been invented for it.

## Current Levenshtein shape suite

The paired table above is a version-pinned JavaScript comparison. The cases
below are deliberately **not** folded into its median: they exercise new
Verbora-only shapes for which this repository has not run an equivalent
JavaScript workload. They are Criterion median estimates from the current
working tree on an Intel i9-14900KF, rustc 1.97.1, release profile. Reproduce
them with:

```bash
cargo bench -p verbora-distance --bench distance -- 'levenshtein_shapes|levenshtein_weighted'
```

| Benchmark | Input shape | Verbora |
|---|---|--:|
| `levenshtein_shapes/near/1024` | ASCII, one central substitution | 0.35 µs |
| `levenshtein_shapes/near_unicode/1024` | Cyrillic, one central substitution | 0.50 µs |
| `levenshtein_shapes/empty_ascii/1024` | empty → 1024 ASCII units | 8.8 ns |
| `levenshtein_shapes/empty_unicode/1024` | empty → 1024 Cyrillic units | 0.39 µs |
| `levenshtein_shapes/disjoint/1024` | two disjoint ASCII alphabets | 1.18 µs |
| `levenshtein_shapes/late_overlap/65x10000` | first shared unit near the end | 2.55 µs |
| `levenshtein_weighted/16` | substitution cost 0.5 | 0.33 µs |
| `levenshtein_weighted/64` | substitution cost 0.5 | 6.18 µs |
| `levenshtein_weighted/256` | substitution cost 0.5 | 131.8 µs |
| `levenshtein_weighted/rectangular/16x1024` | substitution cost 0.5 | 39.5 µs |

The unit-cost rows demonstrate exact fast paths, not a weaker distance:
common prefixes and suffixes are removed before Myers runs, all-disjoint
alphabets return directly, and a leading non-matching run initializes the
same Myers state without scanning it twice. Weighted inputs deliberately use
the scalar rolling-row recurrence, which preserves arbitrary option costs.

## Where the Levenshtein win comes from

The JavaScript library always materialises a full `(n+1)×(m+1)` matrix of
heap-allocated cell objects — each holding a cost and a parent coordinate —
even when the caller wants only the final scalar. That is `O(nm)` allocations
of pointer-chased objects.

Verbora picks the smallest structure that can answer the question asked, and —
wherever a faster *algorithm* exists — the fastest algorithm, not just the
fastest data structure:

| Mode | Working set | Why |
|---|---|---|
| distance, no Damerau, unit cost | **bit-vector** (one `u64` word per 64 units of the shorter operand) | Myers'/Hyyrö's bit-parallel algorithm computes the same answer in `O(nm/64)` bitwise operations rather than `O(nm)` scalar cell updates; the pattern-preprocessing table is a flat array on the byte path (no hashing), and the single-word path covers operands of 1–64 units — see [the competitive benchmarks page](competitive.md#levenshtein) for the full story |
| distance, no Damerau (fallback, weighted costs) | **1 row** | each cell needs only `up`, `left`, `diag` |
| distance, OSA, unit cost | **bit-vector** (word + block) | Hyyrö's 2003 transposition extension of Myers computes OSA in the same `O(nm/64)` bitwise style |
| distance, OSA (fallback, weighted costs) | **3 rows** | transposition reaches row − 2 |
| distance, unrestricted Damerau, unit cost | **3 rolling rows + one saved-cell row** | transposition reaches an arbitrary earlier row, but Zhao–Sahni's linear-space algorithm shows only the no-column-gap and no-row-gap candidates can win, so one remembered cell each replaces the cost + parent matrices (byte operands of at most 8 units run a table-free stack matrix instead) |
| distance, unrestricted Damerau (fallback, weighted costs) | full matrix | a weighted transposition reaches an arbitrary earlier row at an arbitrary price |
| search, Levenshtein, unit cost | per-column bit-vector deltas (no matrix) | `search_bits` recovers every cell's cost, and every parent choice, from the same Myers/Hyyrö `Pv`/`Mv` words the distance-mode kernel already produces, so the backtrace never reads a stored parent |
| search (every other combination — OSA or unrestricted Damerau at any cost, or a weighted search of any variant) | full matrix | a transposition's parent depends on state (`last_row_map`) that cell costs alone cannot recover, and weighted costs have no bit-vector form at all |

The bit-parallel kernels are why `levenshtein/ascii/1024` posts **3307.4×** —
the largest gap on this page by a wide margin. The `levenshtein_variants` rows
are named for the kernels they exercised when they were measured, which is why
none of them reads as a row count — or a matrix — any more: `plain_myers_unit`
and `osa_bit_vector` both run bit-parallel kernels at 64 characters (their
fixed input size) — hence **1066.6×** and **1059.5×**, not what a literal
two-row or three-row scalar sweep would produce. `damerau_zhao_sahni` never
builds a matrix either; its **39.2×** predates the Zhao–Sahni kernel it is now
named for and is pending re-measurement, as the footnote to the table above
records. `search_matrix` no longer builds one either: the plain, unit-cost
search it benchmarks now runs the same per-column bit-vector deltas as the
distance-mode kernel, recomputing each backtrack step from cell costs instead
of reading a stored parent. Its **13.8×** predates that move and is pending
re-measurement too, for the same reason `damerau_zhao_sahni`'s is — see the
second footnote to the table above.

Where the full matrix *is* required — every search except plain Levenshtein
at unit cost, plus unrestricted-Damerau distance at weighted costs — it is
stored struct-of-arrays: costs in one flat `Vec<f64>`, parents in another. The
hot cost sweep stays contiguous, and the parents — touched only during
backtracking — never pollute a cache line during it.

## Where the wins are smaller, and why

**`hamming/4` (1.4×) and `jaro_winkler/4` (1.8×).** At four characters the work
is a handful of comparisons; both runtimes are dominated by call overhead, and
the JavaScript engine optimises this shape very well. Small, genuine wins are
the honest expectation here.

**Jaro–Winkler beyond four characters (6.7×–57.5×).** Jaro and Jaro–Winkler
run on bit-parallel match-flagging kernels (a single-word path, then a block
extension, with a scalar loop kept for inputs of 16 units or fewer), which
preserve the fractional transposition semantics exactly. The ratio *rises*
with input size instead of falling: a scalar implementation would do the
same quadratic work as the JavaScript library at 1024 units, while the
bit-parallel kernels do not.

**Dice (3.2×–7.5×).** Dominated by hashing. Verbora hashes `(char, char)` tuples
with `FxHashSet` instead of allocating a `String` per bigram the way the
JavaScript library does; the win grows with input size as that allocation
pressure compounds.

**Cyrillic vs ASCII.** `levenshtein/cyrillic/256` at 3.97 µs against
`levenshtein/ascii/256` at 2.13 µs — about 86% slower. Promoting non-ASCII
operands to `Vec<char>` for exact scalar semantics is a fixed cost, and the
`char` bit-vector kernel builds its pattern-preprocessing table in a hash
map where the byte path uses a flat 256-entry array. The absolute
difference is still under two microseconds — and the Cyrillic row still
wins by **929.5×**.

## A measured regression, and its fix

The first run recorded `jaro_winkler/4` at **0.6×** — Verbora *slower* than
a widely-used JavaScript NLP library.

The cause was two `vec![false; len]` allocations per call, for the match flags.
The JavaScript engine's `new Array(4)` is nearly free; `malloc` is not.

Moving the match flags to a stack buffer for inputs up to 128 units took
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

**They say nothing about memory.** Verbora's distance metrics hold no
persistent state and have an `O(m)` working set in their fast paths, so
there's little for a memory benchmark to show for them specifically, and none
of the figures above carry one. Memory *is* instrumented elsewhere in the
workspace — `verbora-spellcheck`'s `counting_alloc`, a `#[cfg(test)]` global
allocator its own memory-bound tests measure peak bytes with — but it is
scoped to that crate's test build and does not reach the metrics on this
page; see the [allocation reference](../performance/allocation.md).

## Reproducing

```bash
python3 tools/bench-data/generate.py        # shared inputs (run once)
cargo bench -p verbora-distance          # Verbora, via Criterion
```

Full detail in [Reproducing them](reproducing.md).
