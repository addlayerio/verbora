# Competitive benchmarks

Where does Verbora stand against the Rust ecosystem? This page reports
like-for-like, version-pinned measurements — same input, equivalent result,
and a public loss whenever another library is faster.

<div class="callout callout-note">
<strong>Read by capability, not by one global score.</strong> A tokenizer, a
stemmer and a string-distance function solve different problems. Each section
therefore shows the measured workload, the result and its limits.
</div>

| Coverage | What it means |
|---|---|
| 314 timed comparisons | 13 capabilities with a fair Rust comparison |
| 3 capabilities without a fair peer | WordNet, sentence analysis and sentiment are documented on their feature pages instead |
| 1 primary metric | Criterion median, single-threaded, on the hardware below |
| 9 capabilities awaiting a re-run | [Distance](#distance), [Tokenizers](#tokenizers), [N-Grams](#n-grams), [Stemmers](#stemmers), [Normalizers](#normalizers), [Trie](#trie), [POS tagging](#pos-tagging), [TF-IDF](#tf-idf) and [Spellcheck](#spellcheck) carry Verbora-side figures that no longer describe the code that runs — each section says so at its head. For [Trie](#trie) and [POS tagging](#pos-tagging) it is part of the section, not all of it: the trie's read path and the tagger's cold start still stand |

Start with [Phonetics](#phonetics), [Trie](#trie),
[Language detection](#language-detection) or [POS tagging](#pos-tagging). Exact
harnesses and raw data are linked from each section;
[reproduction instructions](#reproducing-these-numbers) are at the end.

<details>
<summary>Methodology and audit details</summary>

## Benchmark methodology

| | |
|---|---|
| CPU | Intel(R) Core(TM) i9-14900KF (32 threads) |
| Memory | 125 GiB |
| OS | Linux 7.0.11-76070011-generic |
| rustc | 1.97.1, `--release` (`opt-level = 3`, `lto = "thin"`, `codegen-units = 16`) — identical `[profile.release]`/`[profile.bench]` to the main Verbora workspace, not a tuned profile for this audit |
| Node.js | v25.9.0 (used only for the three JavaScript-library-only modules linked above, not for the tables on this page) |
| Verbora commit | Not resolvable — `af1aee9d9da2b1d1b750f0761ef250d4c290b48c` matches no object in this repository's history, on any branch, or on GitHub (checked directly against the GitHub API). Crate version 0.1.0 and the Date row below are what this run is still pinned to |
| Datasets | Shared word/name/pair lists from `benches/data/*.json` (`tools/bench-data/generate.py`, one generator read by every implementation); the 13-language, 4-tier UDHR corpus for language-detection accuracy (sourced below) |
| Warmup | Criterion's own warmup phase before every measured sample (400 ms–1 s per group; see below) |
| Samples | Criterion's default 100 per benchmark, reduced for the most expensive groups: 30 for language-detection-by-length, 20 for spellcheck construction, 15 for POS-tagging cold start (model load) |
| Metric | **Median**, per Criterion's own robust-statistics estimate — not mean, per this project's own `PRIMARY METRIC` policy |
| Threads | **1 (single-threaded)** for every benchmark on this page — no parallel API is exercised anywhere in this audit; see [Thread counts](#thread-counts) |
| Source | [`benchmarks/competitive/rust-competitors/benches/*.rs`](https://github.com/addlayerio/verbora/tree/main/benchmarks/competitive/rust-competitors/benches) (one file per module), raw Criterion output under [`benchmarks/competitive/results/raw/`](https://github.com/addlayerio/verbora/tree/main/benchmarks/competitive/results/raw), joined into [`results/results.json`](https://github.com/addlayerio/verbora/blob/main/benchmarks/competitive/results/results.json) |
| Date | Results captured 2026-08-15/17 (`results/metadata.json`'s timestamp: `2026-08-15T23:50:55Z`; the Metaphone, N-Grams, and the eight byte-exact phonetics groups are the freshest, from 2026-08-17). Unrestricted Damerau-Levenshtein is the one group with no current timing on this page — its measurement is pending, and no stale figure stands in for it. |

Every number on this page is read from Criterion's own saved estimates —
joined into that `results.json` file for most modules, and read directly
from the same per-group raw `estimates.json` files for the freshest groups
named in the Date row — none is retyped from memory or rounded
inconsistently; the relative-speedup figures are computed from the raw
`median_ns` values at page-generation time. See
[Reproducing these numbers](#reproducing-these-numbers) for the exact
commands that regenerate all of it from a clean checkout.

**One machine, one run.** Per the project's own environment policy
(`results/metadata.json`'s own note): "a single dedicated run on one physical
machine with no other significant workload active" — CPU affinity pinning,
thermal monitoring and containerization were evaluated and deliberately not
built, since the spec marks all three optional. Treat exact figures as one
machine's numbers and the ratios/orders-of-magnitude as the more portable
signal, exactly as [String distance results](distance.md) already asks of
its own numbers.

### Thread counts

No benchmark on this page compares a parallel implementation against a
sequential one. Every call measured here — Verbora's and every competitor's
— runs on a single thread; where Verbora exposes a `parallel`-feature batch
API (phonetics' `par_encode_batch`, language detection's `par_detect_batch`),
that comparison is sequential-vs-sequential too, because no competitor in
this audit exposes an equivalent batch-parallel API to compare against.
Verbora's own sequential-vs-parallel numbers, with thread counts disclosed,
live on the [Parallelism](../performance/parallelism.md) page instead — a
different question from this one.

### How to read these tables

- **Time (median)** is Criterion's median estimate for one call of the
  benchmarked operation (or, where noted, one call over a fixed-size batch —
  the input-size column says which).
- **Throughput** is `1 ÷ median time` — calls of *this exact operation* per
  second at the measured latency. It is a call-rate figure, not a
  per-item/per-token throughput, because this audit does not have a verified
  items-per-call count for every operation; where a benchmark already
  operates over a fixed-size batch (e.g. "1024 words"), throughput is still
  batches/second, not words/second — read the input-size column together
  with it.
- **Relative** is each row's time divided by the fastest row's time in that
  same table, labeled `slower` above 1.00×. Rows are **ordered by time, not
  by library** — Verbora is not pinned to the top row, and several tables on
  this page show it losing.
- Where a comparison is explicitly **not** a fair like-for-like ratio (a
  different dictionary, a different corpus), no Relative column is shown at
  all, and the table says so — see [Spellcheck](#spellcheck) for the two
  cases this applies to.
- A figure is published only while the code it measured is the code that
  runs. When a Verbora implementation changes underneath a measurement, that
  measurement is **withdrawn**, and the page distinguishes two cases. A
  section marked **pending re-measurement** still has a live comparison and a
  benchmark group waiting to answer it; its recorded medians stay visible,
  flagged, so the next run has something to diff against, and they must not be
  quoted as current. A capability marked **withdrawn** has no Verbora side
  left, so there is nothing to re-measure and no table at all. Neither is ever
  filled in with an estimate, an interpolation, or an inference from the
  direction of the change.

## How these numbers were audited

The page's audited core — 290 of the 314 comparisons — was cleared by an
independent fairness audit that read every benchmark file and correctness
test in this workspace, re-ran the Rust suite and the language-accuracy
report itself, and cross-referenced all 133 "Verbora loses" rows in
`results.json` (each `(benchmark, competitor)` pair where Verbora's median
is slower than that competitor's, the same per-row comparison the Relative
column below uses) against
[`docs/PERFORMANCE_GAPS.md`](https://github.com/addlayerio/verbora/blob/main/docs/PERFORMANCE_GAPS.md).
Its verdict: **every comparison in that set is FAIR** — same input, genuinely
equivalent (or honestly narrowed and labeled) semantics, `black_box` on every
call's input and output, correctness-before-performance tests that were run
and passed, and version pins of `=x.y.z` on every third-party crate. Two
items the audit flagged as borderline rather than unfair are called out
inline where they occur: the normalizers' accented-input case
([Normalizers](#normalizers)) and the `WhatlangDetector` wrapper-overhead
check ([Language detection](#language-detection)), neither of which is a
ranked "X beats Y" comparison in the first place.

The remaining 24 comparisons — the byte-exact encoder table in
[Phonetics](#phonetics) — carry a stronger correctness check in place of
that audit's narrowed-semantics review: byte-exact output equality with the
competitor itself, asserted in
[`tests/phonetics_correctness.rs`](https://github.com/addlayerio/verbora/blob/main/benchmarks/competitive/rust-competitors/tests/phonetics_correctness.rs)
and independently re-verified by an adversarial audit that differentially
fuzzed 104,114 inputs per encoder against `rphonetic` with zero mismatches,
proved every documented divergence exactly as narrow as each module's own
documentation claims, and mutation-tested the correctness suites.

</details>

## Results by capability

Per this project's `OVERALL SCORE` policy, there is no combined ranking
anywhere on this page. Tokenization, stemming, spell-correction and every
other capability below are different workloads solving different problems —
mixing their numbers into one score would hide more than it reveals.

### Distance

Verbora's edit-distance functions (`docs/COMPETITIVE_BENCHMARKS.md`
[§1.8](https://github.com/addlayerio/verbora/blob/main/docs/COMPETITIVE_BENCHMARKS.md#18-distances))
against [strsim](https://github.com/rapidfuzz/strsim-rs) 0.11.1 and
[rapidfuzz](https://github.com/rapidfuzz/rapidfuzz-rs) 0.5.0 — the Rust
ecosystem's de-facto-standard string-similarity crate (strsim: ~990M
downloads) and the tightest single-crate algorithm match found. Both index by
Unicode scalar value, exactly as Verbora does, and are restricted to ASCII
input here in any case. Unlike the heuristic encoders elsewhere on this page, Levenshtein,
Damerau-Levenshtein, Hamming, Jaro and Jaro-Winkler are exact, well-specified
integer/float functions with a single correct answer per input. Their
equivalence is nonetheless pinned by a runtime correctness suite, not by
algorithm research alone: every timed implementation is asserted against
Verbora on the shared corpus, on the near-identical and planted-needle
derived shapes, and on a seeded randomized sweep, before any timing number
on this page is accepted.

`rapidfuzz` implements Myers/Hyyrö bit-parallel Levenshtein (`O(nm/64)`).
Verbora matches that algorithmic class with a single-word bit-vector fast
path plus a multi-word block extension (Hyyrö's 2003 generalisation,
following `rapidfuzz`'s own `hyrroe2003_block` structure directly rather
than re-deriving it — verified independently line-by-line, then
adversarially fuzz- and mutation-tested against the trusted scalar DP
before being trusted for anything). The kernels' pattern-match (Peq) tables
are flat/packed bit tables rather than a hash map, and the single-word gate
covers 1–64 units. Bit-parallelism extends beyond plain Levenshtein too:
restricted-Damerau/OSA kernels (Hyyrö's 2003 transposition extension of
Myers, single-word and multi-word block, gated to unit costs), and
Jaro/Jaro-Winkler match-flagging kernels in Verbora's own greedy
orientation. Unrestricted Damerau-Levenshtein has no bit-vector
formulation, so it runs Zhao–Sahni's linear-space algorithm instead of a
full cost+parent matrix: three rows and a last-occurrence table, kept on
the stack for short operands, with a table-free stack matrix for byte
operands of at most 8 units. Common-prefix/suffix trimming runs ahead of
the unrestricted-Damerau and OSA kernels alike. Every kernel is
parity-verified by differential tests against the retained scalar
implementations, plus an independent adversarial audit with mutation
testing, before being trusted; Hamming has no bit-parallel kernel.

<div class="callout callout-good">
<strong>Plain Levenshtein beats all five Rust competitors at every size
from 4 to 1024 characters</strong> (Verbora is 1.09× faster at 1024
characters, 1.77× at 16). Restricted Damerau/OSA beats every competitor at
every size, and Jaro/Jaro-Winkler wins every size too. Unrestricted
Damerau-Levenshtein is being re-measured against its current kernels; no
timing claim is made for it here until that run lands — see its own
section below. See
<a href="https://github.com/addlayerio/verbora/blob/main/docs/PERFORMANCE_GAPS.md#26-levenshtein-distance--verbora-vs-stringmetrics-triple_accel-and-editdistancek-rust">PERFORMANCE_GAPS.md
entry 26</a> for the mechanism and verification story these kernels build
on.
</div>

<div class="callout callout-note">
<strong>Levenshtein below is freshly measured; the rest of this section is not.</strong>
The Levenshtein tables were taken on 2026-08-20 against commit
<code>313d025</code>, on the machine described in <a href="#benchmark-methodology">Methodology</a>.
Every other Verbora median in this section still predates two changes: the cost
dispatch in front of the kernels was removed, and the text unit moved from the
UTF-16 code unit to the Unicode scalar. The kernels themselves are unchanged —
the flat <code>BitPeq</code> tables, the 1–64-unit single-word gate and the
multi-word block path are the same code — so the conclusion those figures
support (bit-parallel beats scalar dynamic programming, by margins that widen
with length) does not depend on them; the per-call medians do. Competitor
figures are unaffected, since no competitor version moved. Those medians stay
visible so the next run has something to diff against, and none may be quoted
as current. See
<a href="https://github.com/addlayerio/verbora/blob/main/docs/PERFORMANCE_GAPS.md">PERFORMANCE_GAPS.md</a>
entries 1 and 26–29.
</div>

#### Levenshtein

`stringmetrics` 2.2.2 joins as a fourth full-equivalence (`Yes`/`Yes`)
competitor here and in Hamming below — char-indexed like `strsim`/
`rapidfuzz`, just without a Damerau-Levenshtein implementation at all (its
`damerau` module is commented out of both `mod` and `pub use` in the
published crate, confirmed by reading `stringmetrics-2.2.2`'s own source —
not merely unused, genuinely uncompiled).

| Library | Version | Language | Time (median, 1024 chars) | Throughput | Relative |
|---|---|---|---:|---:|---:|
| Verbora | 0.1.0 | Rust | 27.74 µs | 36.1K/s | **1.00×** |
| rapidfuzz | 0.5.0 | Rust | 33.01 µs | 30.3K/s | 1.19× slower |
| strsim | 0.11.1 | Rust | 636.72 µs | 1.6K/s | 22.96× slower |
| stringmetrics | 2.2.2 | Rust | 919.33 µs | 1.1K/s | 33.15× slower |

**Random pairs** — two independently generated strings, so the edit script is
long and every implementation does its full work:

| Input | Verbora | rapidfuzz | strsim | stringmetrics |
|---:|--:|--:|--:|--:|
| 4 | **10.2 ns** | 37.2 ns | 18.6 ns | 25.3 ns |
| 16 | **41.2 ns** | 74.1 ns | 171.2 ns | 162.3 ns |
| 64 | **160.2 ns** | 247.6 ns | 2.79 µs | 2.99 µs |
| 256 | **2.04 µs** | 3.27 µs | 39.76 µs | 56.00 µs |
| 1024 | **27.74 µs** | 33.01 µs | 636.72 µs | 919.33 µs |

**Near-identical pairs** (`d = 1`) — the shape spell-checking and deduplication
actually feed a distance metric, and the one where the implementations diverge
most:

| Input | Verbora | rapidfuzz | strsim | stringmetrics |
|---:|--:|--:|--:|--:|
| 4 | **10.2 ns** | 20.6 ns | 17.8 ns | 16.0 ns |
| 16 | **11.3 ns** | 38.2 ns | 171.0 ns | 19.9 ns |
| 64 | **13.2 ns** | 138.3 ns | 2.74 µs | 47.0 ns |
| 256 | **20.4 ns** | 258.8 ns | 40.03 µs | 162.3 ns |
| 1024 | **40.9 ns** | 804.0 ns | 620.52 µs | 564.0 ns |

Verbora is the **fastest implementation at every size and both shapes** —
against all four char-indexed competitors here and the two byte-level ones
below. But the two tables answer different questions, and the result only has
its real shape if you read both.

On random pairs the lead over `rapidfuzz` — the closest competitor and the
only other bit-parallel implementation here — runs **3.65× at 4 characters,
1.80× at 16, 1.55× at 64, 1.61× at 256 and 1.19× at 1024**. It is widest at
small sizes, where the flat `[u64; 256]` Peq tables and the 1–64-unit
single-word gate keep per-call setup low, and narrowest at 1024, where both
sides run the same class of multi-word block algorithm. Against `strsim` the
win is 1.82× (4) up to 22.96× (1024), against `stringmetrics` 2.48× up to
33.15× — neither scalar design has a bit-vector formulation to close with.

On near-identical pairs the margin moves the other way: it *widens* with
length, from 1.6× to 5.0× against the best competitor at each size. Common
prefixes and suffixes are trimmed before the kernel runs, so Verbora's cost
rises only from 10.2 ns to 40.9 ns across a 256-fold increase in input length,
while `strsim` — which evaluates the full matrix regardless of how similar the
inputs are — rises to 620 µs, a factor of 15,000.

The random rows are intentionally retained as their own workload. They do
not show the common-affix optimization: independent strings have almost no
affix to remove. The same competitive harness also measures a 1,024-unit
pair with one central substitution (`1024-near`):

| Library | Median (`1024-near`) | Relative to Verbora |
|---|---:|---:|
| Verbora | **40.9 ns** | **1.00×** |
| `editdistancek` | 184.7 ns | 4.52× slower |
| `stringmetrics` | 564.0 ns | 13.79× slower |
| `rapidfuzz` | 804.0 ns | 19.66× slower |
| `triple_accel` | 509.75 µs | 12,463× slower |
| `strsim` | 620.52 µs | 15,172× slower |

Two different designs sit behind those numbers, and the spread separates them
cleanly. `editdistancek` is the only competitor built for this shape — its
bounded-edit algorithm stops early when the distance is small, which is why it
lands two orders of magnitude ahead of the full-matrix implementations.
`triple_accel` and `strsim` evaluate the whole matrix whether the inputs differ
by one character or by a thousand, so similarity buys them nothing.

Verbora reaches 40.9 ns by removing the common prefix and suffix before the
kernel sees anything: on a 1,024-unit pair differing in one position, the
kernel runs over what is left, not over the pair. The cost is therefore set by
the size of the difference rather than the size of the input — 10.2 ns at 4
units, 40.9 ns at 1,024.

#### Competitive shape suite

The competitive harness also has a dedicated
`levenshtein_edge_shapes` group. It uses the exact same lowercase-ASCII,
unit-cost inputs for every implementation, so both char-indexed and
byte-indexed competitors remain directly comparable. It keeps the
shape-sensitive results separate from the random-size table above:

| Case | Operands | What it verifies |
|---|---|---|
| `near/1024` | 1,024 vs. 1,024; one central substitution | Common-prefix/suffix trimming |
| `disjoint/1024` | 1,024 vs. 1,024; no character overlap | Disjoint-alphabet early exit |
| `late-overlap/65x10000` | 65 vs. 10,000; overlap only at the end | The disjoint probe's worst placement |

This is deliberately a separate group, rather than an extra median in the
table above: random pairs, near pairs and disjoint alphabets exercise
different valid algorithmic shortcuts, and blending them would hide that
trade-off. It benchmarks Verbora, `strsim`, `rapidfuzz`, `stringmetrics`,
`triple_accel` and `editdistancek`; the accompanying correctness test checks
that all six return the same distance on every timed shape. Reproduce it
with:

```bash
cd benchmarks/competitive/rust-competitors
cargo test --test distance_correctness levenshtein_competitors_agree_on_the_timed_edge_shapes
cargo bench --bench distance -- levenshtein_edge_shapes
```

#### Damerau–Levenshtein (unrestricted)

`damerau_levenshtein` computes canonical unrestricted
Damerau-Levenshtein — the Lowrance–Wagner distance, where a transposed
pair of adjacent characters costs one edit and may be edited again
afterwards. It is a true metric: symmetric, and satisfying the triangle
inequality. `strsim`'s `damerau_levenshtein` and `rapidfuzz`'s
`distance::damerau_levenshtein` compute the same function, so agreement
between the three is a property of the algorithm rather than of the corpus
they were compared on — and it is checked that way: **202,000 randomized
pairs**, zero divergences, across alphabets of 2, 3, 4 and 26 letters,
lengths 1–25 including unequal and empty operands, and mutation chains of
up to eight edits over a binary alphabet (the shape that actually
separates unrestricted Damerau from OSA). Reproduce it with:

```bash
cd benchmarks/competitive/rust-competitors
cargo test --release --test distance_correctness \
  unrestricted_damerau_agrees_with_both_competitors_over_a_wide_randomized_sweep
```

Verbora runs Zhao–Sahni's linear-space formulation of that recurrence:
three rows plus a last-occurrence table rather than a full cost+parent
matrix, on the stack for short operands and a single allocation beyond,
with a table-free stack matrix for byte operands of at most 8 units.
Because the distance is symmetric, the shorter operand becomes the column
operand, which is what sets the row width and therefore the kernel's whole
memory footprint. Operands are stripped of their maximal common prefix and
suffix first — a genuine algorithmic reduction rather than a
micro-optimisation, since a pair differing in one interior position
collapses to work proportional to the differing region alone. Weighted
(non-unit-cost) calls take the full-matrix path instead, which evaluates
the same recurrence with per-operation weights and doubles as the
differential oracle for every fast path.

<div class="callout callout-warn">
<strong>No timing comparison is published for this metric yet.</strong>
Every figure on this page is read from a Criterion run against the code
that ships, and the run covering these kernels is still outstanding — so
this one section carries no table rather than an approximate one. The
correctness result above is measured and current; so is every other table
on this page.
</div>

#### Damerau–Levenshtein (restricted / OSA)

| Library | Version | Language | Time (median, 1024 chars) | Throughput | Relative |
|---|---|---|---:|---:|---:|
| Verbora | 0.1.0 | Rust | 32.47 µs | 30.8K/s | **1.00×** |
| rapidfuzz | 0.5.0 | Rust | 45.06 µs | 22.2K/s | 1.39× slower |
| strsim | 0.11.1 | Rust | 2.43 ms | 411.5/s | 74.84× slower |

| Input size | Verbora | rapidfuzz | strsim |
|---:|--:|--:|--:|
| 4 | 15.9 ns | 30.2 ns | 67.5 ns |
| 16 | 46.2 ns | 83.6 ns | 321.4 ns |
| 64 | 179.3 ns | 264.2 ns | 4.56 µs |
| 256 | 2.39 µs | 3.02 µs | 140.64 µs |
| 1024 | 32.47 µs | 45.06 µs | 2.43 ms |

Verbora is the **fastest at every size**. Restricted Damerau's
one-transposition-back reach needs more state than plain Levenshtein's
two-row shape, so it gets its own bit-parallel kernels implementing Hyyrö's
2003 transposition extension of Myers' algorithm — a single-word kernel
plus a multi-word block generalisation, gated to unit-cost options, with
the scalar three-row DP retained for every non-unit-cost call and as the
differential-test oracle. Against `rapidfuzz`, the only other bit-parallel
OSA here: **1.90× faster at 4 characters, 1.81× at 16, 1.47× at 64, 1.26×
at 256, 1.39× at 1024**. Against `strsim`'s scalar implementation the
margin runs 4.25× (4 chars) up to 74.8× (1024). `triple_accel`'s byte-level
`rdamerau` is covered in the byte-level subsection below.

#### Hamming

| Library | Version | Language | Time (median, 1024 chars) | Throughput | Relative |
|---|---|---|---:|---:|---:|
| Verbora | 0.1.0 | Rust | 272.9 ns | 3.7M/s | **1.00×** |
| strsim | 0.11.1 | Rust | 552.2 ns | 1.8M/s | 2.02× slower |
| stringmetrics | 2.2.2 | Rust | 567.1 ns | 1.8M/s | 2.08× slower |
| rapidfuzz | 0.5.0 | Rust | 603.9 ns | 1.7M/s | 2.21× slower |

Verbora **wins** Hamming against every char-indexed competitor here from 16
characters up — a fixed per-call setup cost (still small in absolute terms,
nanoseconds) makes Verbora the *slowest* of the four specifically at 4
characters, the one exception, before it pulls ahead and stays ahead. No
bit-parallel state to build on either side, so this stays an
apples-to-apples scalar-vs-scalar race outside that one small-input
crossover. (`triple_accel`'s genuinely SIMD-accelerated Hamming is a
different story — see the byte-level subsection below, where Verbora loses
decisively instead.)

| Input size | Verbora | rapidfuzz | strsim | stringmetrics |
|---:|--:|--:|--:|--:|
| 4 | 6.9 ns | 6.3 ns | 4.8 ns | 2.4 ns |
| 16 | 9.2 ns | 17.7 ns | 14.0 ns | 15.1 ns |
| 64 | 21.0 ns | 53.0 ns | 43.4 ns | 43.9 ns |
| 256 | 70.1 ns | 165.8 ns | 149.8 ns | 145.5 ns |
| 1024 | 272.9 ns | 603.9 ns | 552.2 ns | 567.1 ns |

#### Jaro / Jaro–Winkler

| Library | Version | Language | Time (median, 1024 chars) | Throughput | Relative |
|---|---|---|---:|---:|---:|
| Verbora | 0.1.0 | Rust | 10.32 µs | 96.9K/s | **1.00×** |
| rapidfuzz | 0.5.0 | Rust | 13.00 µs | 76.9K/s | 1.26× slower |
| strsim | 0.11.1 | Rust | 330.91 µs | 3.0K/s | 32.06× slower |

Verbora **beats both competitors at every size**. `rapidfuzz`'s
Jaro/Jaro-Winkler is bit-parallelized
(`rapidfuzz-0.5.0/src/distance/jaro.rs`); Verbora matches that with its own
bit-parallel match-flagging kernels (word-sized plus multi-word block) in
its own greedy match orientation, with the scalar loop retained for inputs
of at most 16 units and as the differential-test oracle, and the
fractional-transposition semantics preserved exactly. Against `rapidfuzz`:
**3.42× faster at 4 characters, 1.10× at 16, 2.06× at 64, 1.13× at 256,
1.26× at 1024.**

| Input size | Verbora | rapidfuzz | strsim |
|---:|--:|--:|--:|
| 4 | 10.6 ns | 36.3 ns | 27.8 ns |
| 16 | 74.9 ns | 82.7 ns | 165.6 ns |
| 64 | 124.7 ns | 257.3 ns | 1.52 µs |
| 256 | 1.82 µs | 2.06 µs | 22.92 µs |
| 1024 | 10.32 µs | 13.00 µs | 330.91 µs |

Sørensen–Dice against a Rust crate, and plain Jaro against a JavaScript
library, are **not** benchmarked (plain Jaro against the Rust competitors
above is — see the table just before this paragraph) — the matrix records
both as narrowed/no-fair-competitor per `docs/COMPETITIVE_BENCHMARKS.md`
§1.8. See
[`benches/distance.rs`](https://github.com/addlayerio/verbora/blob/main/benchmarks/competitive/rust-competitors/benches/distance.rs)'s
own module doc comment for the full accounting of every row this module
benchmarks and every row it deliberately does not.

#### `triple_accel` and `editdistancek` — byte-level, ASCII-only-fair

Kept separate from every table above rather than merged into them:
`triple_accel` and `editdistancek` operate on raw `&[u8]` bytes, not
Unicode scalars like Verbora/`strsim`/`rapidfuzz`/`stringmetrics` —
numerically identical to the char-indexed approach on the ASCII-only corpus
this whole module shares, but genuinely different on non-ASCII input,
which the research matrix marks `Partial`/`Selected cases` rather than the
full `Yes`/`Yes` equivalence every row above carries. `triple_accel` is
genuinely SIMD-accelerated (AVX2/SSE4.1); `editdistancek` is a Myers-style
banded/diagonal algorithm over `isize` buffers. Byte-identical correctness
against Verbora verified in
[`tests/distance_correctness.rs`](https://github.com/addlayerio/verbora/blob/main/benchmarks/competitive/rust-competitors/tests/distance_correctness.rs)
before any number below was trusted — with one deliberate exception. The
restricted-Damerau equality is asserted only on the shared corpus and not
on randomized input, because `triple_accel`'s restricted Damerau carries a
real, independently-confirmed upstream defect (see [Upstream bugs
found](#upstream-bugs-found)) that the corpus happens never to trigger. The
timing row stays: both sides do the same shape of work on the pairs
actually benchmarked. The correctness claim is scoped to match.

**Levenshtein** — the same bit-vector kernels above win here too, by wider
margins than against `rapidfuzz`/`strsim`:

| Library | Version | Time (median, 1024 chars) | Relative |
|---|---|---:|---:|
| Verbora | 0.1.0 | 27.74 µs | **1.00×** |
| triple_accel | 0.4.0 | 497.89 µs | 17.95× slower |
| editdistancek | 1.0.2 | 1.06 ms | 38.19× slower |

Random pairs:

| Input size | Verbora | triple_accel | editdistancek |
|---:|--:|--:|--:|
| 4 | **10.2 ns** | 64.0 ns | 45.7 ns |
| 16 | **41.2 ns** | 221.3 ns | 372.9 ns |
| 64 | **160.2 ns** | 1.64 µs | 4.79 µs |
| 256 | **2.04 µs** | 35.56 µs | 72.31 µs |
| 1024 | **27.74 µs** | 497.89 µs | 1.06 ms |

Near-identical pairs (`d = 1`), where `editdistancek`'s banded algorithm is at
its strongest and `triple_accel`'s SIMD full-matrix pass gains nothing:

| Input size | Verbora | triple_accel | editdistancek |
|---:|--:|--:|--:|
| 4 | **10.2 ns** | 64.0 ns | 21.7 ns |
| 16 | **11.3 ns** | 221.4 ns | 22.1 ns |
| 64 | **13.2 ns** | 1.64 µs | 69.4 ns |
| 256 | **20.4 ns** | 36.08 µs | 101.1 ns |
| 1024 | **40.9 ns** | 509.75 µs | 184.7 ns |

Verbora wins **at every size and both shapes, outright** — 5.4× at 16
characters on random pairs (41.2 ns vs. 221.3 ns), the flat-table Peq setup and
1–64-unit single-word gate keeping per-call overhead low.

The near-identical rows are worth reading beside the random ones, because they
separate the two competitors rather than confirming the first table.
`triple_accel` is unmoved by similarity — it runs the same vectorized full
matrix either way, so 1024 costs it ~500 µs regardless — while
`editdistancek`'s banded algorithm is built for exactly this case and drops to
184.7 ns. Verbora is faster still at 40.9 ns, but against a competitor doing
the right thing rather than one doing the wrong thing quickly.

**Restricted Damerau-Levenshtein** — Verbora's OSA bit-parallel kernels win
**at every size, by a margin that widens with input**: 4.80× faster at 4
characters, 5.64× at 16, 11.6× at 64, 21.1× at 256, and 22.7× at 1024
(**32.47 µs** vs. `triple_accel`'s **737.06 µs**).

**Hamming** — the widest gap in this whole module: `triple_accel`'s
Hamming is a vectorized XOR-and-popcount over the whole string with no
data-dependent branching, versus Verbora's scalar per-position comparison
loop — **18.6× faster** at 1024 characters (**14.7 ns** vs. Verbora's
**272.9 ns**), the gap widening steadily from a modest 2.3× at 4 characters.

---

### Tokenizers

<div class="callout callout-warn">
<strong>No Verbora tokenizer figure is currently backed.</strong> Verbora
exposes three tokenizers over one token shape — <code>WordTokenizer</code>,
<code>SegmentTokenizer</code>, <code>SentenceTokenizer</code>, every token a
borrowed <code>&amp;str</code> substring of the input — and both benchmarked
tokenizers were rebuilt on <a href="../features/tokenizers">UAX #29</a>
segmentation. That is a different algorithm on the timed path, not a
constant-factor change, so every figure this section carried was measured
against a scanner Verbora no longer runs, and all of them are withdrawn. Competitor figures are
unaffected; no competitor version moved. Nothing is estimated in their place,
and the direction of the change must not be inferred — a re-run has to
produce the new numbers. See
<a href="https://github.com/addlayerio/verbora/blob/main/docs/COMPETITIVE_BENCHMARKS.md#11-tokenizers">COMPETITIVE_BENCHMARKS.md
§1.1</a> for the per-row reasoning.
</div>

#### Whitespace tokenization — capability withdrawn

Verbora performs no whitespace or pattern-based tokenization at any API. The
comparison against [tantivy](https://github.com/quickwit-oss/tantivy)
0.26.1's `WhitespaceTokenizer` and [Hugging Face
`tokenizers`](https://github.com/huggingface/tokenizers) 0.23.1's
`WhitespaceSplit` pre-tokenizer has no Verbora side left, so the group is
gone rather than pending: there is nothing to re-measure. A caller who wants
whitespace splitting should use `regex` or `str::split_whitespace` directly.

#### Word tokenization — pending re-measurement

`WordTokenizer` against tantivy 0.26.1's `SimpleTokenizer` and Hugging Face
`tokenizers` 0.23.1's `Whitespace` pre-tokenizer, called in isolation (never
through HF's full BPE pipeline). This is still a genuine rival comparison and
the harness still runs it: the workload is narrowed to punctuation-free ASCII
text, and boundary-exact agreement (not just token-count agreement) is
re-proved against the current implementation in
[`tests/tokenizers_correctness.rs`](https://github.com/addlayerio/verbora/blob/main/benchmarks/competitive/rust-competitors/tests/tokenizers_correctness.rs)
before any timing is trusted.

Only Verbora's side of it changed, and it changed categorically:
`WordTokenizer` now performs full UAX #29 word segmentation (WB1–WB999)
rather than a character-class test, so it does strictly more work per
boundary than the scanner the recorded medians measured. The
`word_tokenization` group is **pending re-measurement**
([`PERFORMANCE_GAPS.md` entry 4](https://github.com/addlayerio/verbora/blob/main/docs/PERFORMANCE_GAPS.md#4-word-tokenization--verbora-vs-tantivysimpletokenizer-rust-a-size-dependent-crossover-not-a-one-sided-loss));
no table is published for it here until that run happens.

#### Sentence tokenization — pending re-measurement

`SentenceTokenizer` against [segtok](https://github.com/xamgore/segtok)
0.1.5, on the narrowed plain-declarative-sentence domain both sides agree on
(no abbreviations/URIs/digits/quotes/brackets), with boundary-exact agreement
proven in the same correctness test before any timing is trusted. The
`sentence_tokenization` and `sentence_tokenization_boundary_density` groups
are **pending re-measurement**: `SentenceTokenizer` is now built directly on
`split_sentence_bound_indices()` with no placeholder mask, no unmask pass and
no trimming, and every recorded figure measured the mask-and-restore
algorithm that replaced.

The `unicode-segmentation` pairing that used to appear here is not pending —
it is gone, and for a reason worth stating plainly. `SentenceTokenizer` is
built on `unicode-segmentation`, so timing the two against each other
measures Verbora against its own dependency, which is a wrapper-overhead
question rather than a competitive one. Those rows moved to a
`sentence_tokenization_wrapper_overhead` group whose numbers state what the
wrapper costs over the primitive, and they may never be reported as Verbora
beating or losing to `unicode-segmentation`. The same applies to
`WordTokenizer`, which *is* `str::unicode_words()`.

---

### N-Grams

Character n-gram generation with frequency counting (`docs/COMPETITIVE_BENCHMARKS.md`
[§1.2](https://github.com/addlayerio/verbora/blob/main/docs/COMPETITIVE_BENCHMARKS.md#12-n-grams))
against [ngrammatic](https://github.com/compenguy/ngrammatic) 0.7.0's
`Ngram`/`NgramBuilder` — the character n-gram + frequency-count generator its
headline `Corpus`/`search` fuzzy-matching feature is itself built on. Only
that generator is benchmarked here: `Corpus`/`search` solves a different
problem (fuzzy corpus search) with no Verbora equivalent, and is not
compared. Both sides pad with `arity - 1` copies of the same character and
slide an identical window over every word in the shared 20,000-word list;
byte-identical `(gram, count)` output is proven at arity 2 and arity 3 in
[`tests/ngrams_correctness.rs`](https://github.com/addlayerio/verbora/blob/main/benchmarks/competitive/rust-competitors/tests/ngrams_correctness.rs)
before any number below was trusted.

<div class="callout callout-warn">
<strong>Both groups' Verbora figures are pending re-measurement.</strong>
<code>verbora-ngrams</code> was rebuilt on borrowed windows:
<code>ngrams</code> now yields sub-slices of the caller's own slice with
nothing allocated per window, where the timed path used to build a
<code>Vec&lt;Cow&lt;[char]&gt;&gt;</code> and fold each gram into a
<code>String</code>. That accumulation shape is exactly what the split-by-arity reading
below rests on, so whether trigrams remain a loss at all is an open question
rather than a recorded result. Competitor figures are unaffected; no
competitor version moved.
</div>

<div class="callout callout-note">
<strong>A genuine split by arity, not a clean sweep.</strong> Verbora wins
bigram generation on every one of 3 independent runs (~1.07×–1.16× faster)
and loses trigram generation on every one of the same 3 runs, by a smaller
but equally consistent margin (ngrammatic ~1.03×–1.08× faster). See
<a href="https://github.com/addlayerio/verbora/blob/main/docs/PERFORMANCE_GAPS.md#38-character-n-gram-generation-trigrams--verbora-vs-ngrammatic-rust-a-small-but-consistent-loss-alongside-a-clear-bigram-win">PERFORMANCE_GAPS.md
entry 38</a> for the full 3-run reading and the profiling-backed theory for
why the two arities diverge.
</div>

| Library | Version | Language | Time (median, bigrams, 20,000 words) | Throughput | Relative |
|---|---|---|---:|---:|---:|
| Verbora | 0.1.0 | Rust | 8.50 ms | 2.35M/s | **1.00×** |
| ngrammatic | 0.7.0 | Rust | 9.88 ms | 2.02M/s | 1.16× slower |

| Library | Version | Language | Time (median, trigrams, 20,000 words) | Throughput | Relative |
|---|---|---|---:|---:|---:|
| ngrammatic | 0.7.0 | Rust | 11.66 ms | 1.72M/s | **1.00×** |
| Verbora | 0.1.0 | Rust | 11.98 ms | 1.67M/s | 1.03× slower |

Both implementations do the same conceptual work — pad, slide a window,
fold into a `(gram, count)` map — over the same input, so the residual is
plausibly a small-string-accumulation difference rather than an algorithmic
one: `ngrammatic` accumulates directly into a `HashMap<SmolStr, usize>`,
whose small-string optimization skips a heap allocation for any gram that
fits inline (every bigram and trigram over this word list does), while
Verbora's benchmarked path builds each gram through its generic `ngrams()`
engine and folds the result into a `HashMap<String, usize>` with one
`String` allocation per unique gram. That difference plausibly narrows (and
eventually reverses) as grams get longer relative to the inline-capacity
boundary — a source-read hypothesis, not yet confirmed with a profiler; see
PERFORMANCE_GAPS.md entry 38 above for the full accounting.

---

### Stemmers

Nine canonical Snowball-algorithm languages (`de es fr it nl no pt ru sv`)
against two independent Snowball-to-Rust ports:
[rust-stemmers](https://github.com/CurrySoftware/rust-stemmers) 1.2.0 (the
official Snowball compiler's own output, and the Rust ecosystem's de-facto
Snowball crate) and
[snowball_stemmers_rs](https://github.com/SeekStorm/snowball-stemmers-rs)
1.0.1 (a second, independently-generated port from the same compiler,
published by the original SymSpell author). English is benchmarked
separately against [nltk-porter](https://github.com/VoiceLessQ/nltk-porter)
0.1.0 and [porter-stemmer](https://github.com/samgiles/porter-stemmer) 0.1.2:
both competitors' "English" is a documented *different algorithm* from
`rust-stemmers`' Snowball Porter2 and matches Verbora's original-1980 Porter
instead (`docs/COMPETITIVE_BENCHMARKS.md`
[§1.3](https://github.com/addlayerio/verbora/blob/main/docs/COMPETITIVE_BENCHMARKS.md#13-stemmers)).
Byte-exact agreement on every benchmarked word verified in
[`tests/stemmers_correctness.rs`](https://github.com/addlayerio/verbora/blob/main/benchmarks/competitive/rust-competitors/tests/stemmers_correctness.rs)
— including several real, narrow divergences found and excluded from the
benchmarked domain rather than hidden (Russian `ё`→`е` folding, Dutch's
sticky cross-call state, `porter-stemmer`'s single isolated `"sky"`→`"ski"`
bug).

<div class="callout callout-warn">
<strong>No Verbora Snowball figure is currently backed.</strong> Verbora's
suffix matching is the Snowball runtime's own
<code>find_among</code>/<code>find_among_b</code> binary search
(<code>crates/verbora-stemmers/src/among.rs</code>): a table sorted by
reversed scalar sequence, <code>common_i</code>/<code>common_j</code>
prefix tracking so no unit is compared twice, and
<code>substring_i</code>-style links so one search replaces a whole guarded
else-if chain. Ten of the eleven language modules route through it —
<code>de</code>, <code>en</code>, <code>es</code>, <code>fr</code>,
<code>it</code>, <code>nl</code>, <code>no</code>, <code>pt</code>,
<code>ru</code>, <code>sv</code>. That is the same algorithm both competitors
get from the official Snowball compiler, so the per-language ratios this
section carried were measured against a linear scan the crate no longer runs,
and all of them are withdrawn. Competitor figures are unaffected; no competitor version
moved. Which rows flip, and by how much, must not be inferred from the
direction of the change — a re-run has to produce them. See
<a href="https://github.com/addlayerio/verbora/blob/main/docs/PERFORMANCE_GAPS.md#34-snowball-stemmers-per-language--verbora-vs-rust-stemmers-and-snowball_stemmers_rs-rust">PERFORMANCE_GAPS.md
entry 34</a>.
</div>

What the retirement does not touch is the correctness record, which is what
made the comparison admissible in the first place: byte-exact agreement per
language against both ports, re-proved before any timing is accepted.

#### `snowball_stemmers_rs` — a second, independently-generated Snowball port

Languages are never averaged together, so this is a second, independent data
point rather than a repeat of the `rust-stemmers` comparison — and its
timings are withdrawn on exactly the same ground. What it establishes without
reference to any measurement is a correctness finding:
`snowball_stemmers_rs`'s `russian.sbl` carries the same ё→е fold Verbora's
stemmer does, so Russian agrees 100% byte-exact *including* `ёлка`, where
`rust-stemmers` does not. Dutch needs `Algorithm::DutchPorter` specifically;
the crate's plainly-named `Algorithm::Dutch` is actually Kraaij–Pohlmann, a
different, non-canonical stemmer, confirmed by reading the crate's own
algorithm list rather than assumed from the name.

#### English — `nltk-porter` and `porter-stemmer`

Two independent original-1980-Porter ports, since `rust-stemmers`' own
"English" is Snowball Porter2, a different algorithm (excluded from the
Snowball comparison above). Verbora's English module routes through
`among.rs` like the other ten, so this group's timings are withdrawn with
them and no table stands in for them until the re-run
(<a href="https://github.com/addlayerio/verbora/blob/main/docs/PERFORMANCE_GAPS.md">PERFORMANCE_GAPS.md</a>
entry 24).

The correctness finding stands on its own: `porter-stemmer` operates on
grapheme clusters rather than Unicode scalar values, an architectural difference that
turns out not to matter on this plain-ASCII corpus — 63 of 64 benchmarked
words agree byte-exact, the one mismatch a real, isolated `porter-stemmer`
bug (`"sky"`→`"ski"`), unrelated to graphemes and excluded from the sample.

#### Japanese — `lindera-analysis`

Verbora's `StemmerJa` (trailing katakana U+30FC drop, minimum 4 Unicode
scalar values) against `lindera-analysis`'s `JapaneseKatakanaStemTokenFilter`,
`min = 3` (the filter's own default) — verified to reproduce Verbora's
`>= 4`-scalar threshold exactly on the shared word list before any number
below was trusted.

| Input size | Verbora | lindera-analysis | Faster |
|---:|--:|--:|--:|
| 4 | 34.75 ns | 456.58 ns | **Verbora, 13.14×** |
| 16 | 160.62 ns | 2.11 µs | **Verbora, 13.14×** |
| 64 | 597.05 ns | 8.99 µs | **Verbora, 15.06×** |
| 256 | 2.30 µs | 35.27 µs | **Verbora, 15.33×** |
| 1024 | 9.38 µs | 140.42 µs | **Verbora, 14.97×** |

A clean, decisive win on both time and allocations — 0 vs. 6 over the
7-word correctness list. Verbora's algorithm borrows and allocates nothing;
`lindera-analysis`'s `Vec<Token>`-batch filter API always allocates at least
the `Vec`, on top of running through a full dictionary-backed tokenizer
pipeline Verbora's purpose-built stemmer doesn't need.

#### Indonesian — `sastrawi`

Verbora's `StemmerId` (Sastrawi/Nazief–Adriani) against `sastrawi`
(iDevoid/rust-sastrawi) 0.1.1 — genuine shared lineage, not a coincidence:
both implement the same published PHP Sastrawi algorithm, and both
dictionaries hold exactly 29,932 root words, confirmed directly. Real
correctness gaps found in `sastrawi` during verification, excluded from the
benchmarked sample rather than hidden: no hyphenated-reduplication/
compound-plural handling at all, and only a single (not iterated-up-to-3×)
prefix-stripping pass — 13 of 16 benchmarked words still agree byte-exact.

| Input size | Verbora | sastrawi | Faster |
|---:|--:|--:|--:|
| 4 | 3.53 µs | 969.23 ns | sastrawi, 3.64× |
| 16 | 54.63 µs | 8.59 µs | sastrawi, 6.36× |
| 64 | 243.09 µs | 35.87 µs | sastrawi, 6.78× |
| 256 | 954.89 µs | 152.38 µs | sastrawi, 6.27× |
| 1024 | 3.97 ms | 583.38 µs | sastrawi, 6.80× |

<div class="callout callout-note">
A real, decisive loss on per-word time. <code>sastrawi</code>'s own one-time
<code>Dictionary::new()</code> + <code>Stemmer::new()</code> construction
cost (~47K allocations, ~21 MB) is real but paid once; Verbora's
<code>StemmerId::new()</code> is a zero-sized unit struct backed entirely by
compiled-in static data, needing no runtime construction at all — a
trade-off the per-word numbers above don't capture.
</div>

`CarryStemmerFr` (French, Carry variant — a distinct 3-pass suffix-table
algorithm, not standard Snowball French) has no fair Rust competitor: no
crate in the ecosystem implements it, confirmed by checking that
`rust-stemmers`' `Algorithm::French` is standard Snowball French rather than
Carry.

---

### Normalizers

`remove_diacritics` (`docs/COMPETITIVE_BENCHMARKS.md`
[§1.4](https://github.com/addlayerio/verbora/blob/main/docs/COMPETITIVE_BENCHMARKS.md#14-normalizers))
against [diacritics](https://github.com/YesSeri/diacritics) 0.2.2, chosen
over five other Rust candidates as the closest semantic match — case-preserving,
non-decomposing table lookup, not NFD-based like `unaccent` and not
forced-lowercasing like `secular`. Byte-exact agreement on ASCII, precomposed
accented Latin, Cyrillic rejection and the shared `ß`/`ẞ`/`ſ`/`İ`/`ı`/`œ`
table quirks verified in
[`tests/normalizers_correctness.rs`](https://github.com/addlayerio/verbora/blob/main/benchmarks/competitive/rust-competitors/tests/normalizers_correctness.rs)
before any timing was trusted — one real divergence found and deliberately
excluded from the benchmarked domain: `diacritics` silently strips standalone
Unicode combining marks, which `remove_diacritics` never decomposes and so
leaves untouched.

<div class="callout callout-warn">
<strong>Both groups' Verbora figures are pending re-measurement.</strong>
Every function in <code>verbora-normalizers</code> was reimplemented, so the
medians below were taken against code the crate no longer runs. Competitor
figures are unaffected; no competitor version moved. The medians stay visible
so the next run has something to diff against, and none may be quoted as
current. What does not depend on a timing, and is therefore stated as fact
rather than measured, is the shape of the ASCII path: <code>remove_diacritics</code>
returns <code>Cow::Borrowed</code> immediately for pure-ASCII input, with no
scan and no allocation.
</div>

#### Pure-ASCII input

| Library | Version | Language | Time (median, 1024 B) | Throughput | Relative |
|---|---|---|---:|---:|---:|
| Verbora | 0.1.0 | Rust | 40.1 ns | 24.95M/s | **1.00×** |
| diacritics | 0.2.2 | Rust | 10.75 µs | 93.0K/s | 268.28× slower |

| Input size | Verbora | diacritics | Verbora vs. diacritics |
|---:|--:|--:|--:|
| 4 | 3.1 ns | 182.4 ns | **59.75× faster** |
| 16 | 10.6 ns | 178.8 ns | **16.85× faster** |
| 64 | 10.7 ns | 740.9 ns | **69.55× faster** |
| 256 | 34.6 ns | 2.72 µs | **78.85× faster** |
| 1024 | 40.1 ns | 10.75 µs | **268.28× faster** |

Verbora's one-line `s.is_ascii()` fast path returns `Cow::Borrowed`
immediately — no scan, no allocation — while `diacritics::remove_diacritics`
folds through its `char` match unconditionally, even on input it will not
change.

#### Accented (working) input

<div class="callout callout-note">
<strong>Inconclusive — reported as such, not forced to a verdict.</strong>
Two independent reruns of this specific case disagreed on direction at the
largest size (1024 B): <code>diacritics</code> is slightly faster (1–13%) at
four of five sizes, and one of the two reruns flipped at 1024 B. The
magnitude is small enough, and the run-to-run variance real enough, that the
fairness audit judged forcing a numeric verdict here would misrepresent the
evidence — see the numbers below from this page's own canonical run, and
treat them as one data point rather than a settled ranking.
</div>

| Input size | Verbora | diacritics | Verbora vs. diacritics |
|---:|--:|--:|--:|
| 4 | 532.2 ns | 491.7 ns | 1.08× slower |
| 16 | 2.09 µs | 1.91 µs | 1.09× slower |
| 64 | 10.20 µs | 9.03 µs | 1.13× slower |
| 256 | 33.50 µs | 30.90 µs | 1.08× slower |
| 1024 | 134.42 µs | 125.01 µs | 1.08× slower |

No `PERFORMANCE_GAPS.md` entry follows from this case, consistent with that
file's own policy of recording settled findings, not noise near the
measurement floor.

---

### Inflectors

`OrdinalInflector::nth` — English ordinal suffixing (1st/2nd/3rd/…) —
against [ordinal](https://github.com/heaths/ordinal-rs) 0.4.0
(`docs/COMPETITIVE_BENCHMARKS.md`
[§1.5](https://github.com/addlayerio/verbora/blob/main/docs/COMPETITIVE_BENCHMARKS.md#15-inflectors)),
the single full-equivalence (`Yes`) row in the whole Inflectors group and the
only one benchmarked here. Two real divergences were found and excluded from
the benchmarked domain before any timing was trusted, in
[`tests/inflectors_correctness.rs`](https://github.com/addlayerio/verbora/blob/main/benchmarks/competitive/rust-competitors/tests/inflectors_correctness.rs):
negative integers (different rounding conventions), and **a real bug in
`ordinal` 0.4.0 itself**: its teens exception uses `n % 20` where it needs
`n % 100`, misformatting
12% of non-negative integers (`31.to_ordinal_string()` returns `"31th"`, not
`"31st"`). The benchmarked domain verifiably avoids every affected value.

| Library | Version | Language | Time (median, 1024-int batch) | Throughput | Relative |
|---|---|---|---:|---:|---:|
| Verbora | 0.1.0 | Rust | 14.94 µs | 67.0K/s | **1.00×** |
| ordinal | 0.4.0 | Rust | 27.53 µs | 36.3K/s | 1.84× slower |

| Input size | Verbora | ordinal | Verbora vs. ordinal |
|---:|--:|--:|--:|
| 4 | 58.1 ns | 100.5 ns | **1.73× faster** |
| 16 | 226.5 ns | 433.5 ns | **1.91× faster** |
| 64 | 916.3 ns | 1.70 µs | **1.85× faster** |
| 256 | 3.61 µs | 6.69 µs | **1.85× faster** |
| 1024 | 14.94 µs | 27.53 µs | **1.84× faster** |

A flat ~1.8×–1.9× regardless of batch size: both sides are `O(1)` per
integer, so this is a genuine constant-factor difference — `ordinal` formats
through `Display`/`format!`, Verbora writes digits directly into a
pre-sized buffer.

---

### Trie

Generic prefix-search throughput. Verbora's trie keys one node per Unicode
scalar and enumerates in ascending scalar order, which for well-formed Rust
strings is `<str as Ord>`; competitors key by byte or by nybble and order their
own way, so the comparison is timed on equal work rather than asserted
element-for-element.

The competitors are
[trie-rs](https://github.com/laysakura/trie-rs) 0.4.2 (highest download
count of any competitor in the whole audit: 5.9M) and
[qp-trie](https://github.com/sdleffler/qp-trie-rs) 0.8.2
(`docs/COMPETITIVE_BENCHMARKS.md`
[§1.18](https://github.com/addlayerio/verbora/blob/main/docs/COMPETITIVE_BENCHMARKS.md#118-trie)).
Order-blind set-equality of every operation's result proven in
[`tests/trie_correctness.rs`](https://github.com/addlayerio/verbora/blob/main/benchmarks/competitive/rust-competitors/tests/trie_correctness.rs)
before any timing was trusted; "build" is timed as push-then-compile for
`trie-rs`'s LOUDS architecture, matching how that crate is actually used.

<div class="callout callout-warn">
<strong>Verbora's <code>build</code> medians are pending re-measurement.</strong>
<code>insert_all</code> now clamps the reservation it takes from the iterator's
<code>size_hint</code> at 4,096 nodes, so this 20,000-word bulk load pre-sizes
for 4,096 and reaches the rest by amortised growth, where the recorded medians
were taken against a build that reserved the whole hint up front. The clamp is
there because a <code>size_hint</code> is a hint and not a bound: an iterator
that overstates it could otherwise turn a bulk load into an unbounded
allocation. This applies to every <code>build</code> row in this section — the
<code>qp-trie</code>/<code>trie-rs</code> table below, the
<code>fast_radix_trie</code> table, and the <code>fst</code> comparison. The
read-path figures — <code>contains</code>, <code>common_prefix_search</code>,
<code>predictive_search</code> — are unaffected, and no competitor version
moved. The build medians stay visible so the next run has something to diff
against, and none may be quoted as current; the direction of the change must
not be inferred either.
</div>

| Library | Version | Language | Time (median, 20,000-word build, random) | Throughput | Relative |
|---|---|---|---:|---:|---:|
| Verbora | 0.1.0 | Rust | 1.65 ms | 607.5/s | **1.00×** |
| qp-trie | 0.8.2 | Rust | 3.04 ms | 328.8/s | 1.85× slower |
| trie-rs | 0.4.2 | Rust | 11.58 ms | 86.3/s | 7.04× slower |

Verbora **wins build** against both competitors (1.85×–7.04× at random keys;
2.11×–2.64× at prefix-heavy keys) and **wins every operation against
`trie-rs`** by 99×–492×. Against `qp-trie` specifically, the read path
inverts:

| Operation | Verbora | qp-trie | Verdict |
|---|--:|--:|---|
| `contains` (hit, 20K words) | 1.22 ms | 869.48 µs | qp-trie 1.41× faster |
| `contains` (miss, 20K words) | 1.22 ms | 796.63 µs | qp-trie 1.54× faster |
| `common_prefix_search` | 256.34 µs | — (not implemented) | — |
| `predictive_search` (1-char prefix) | 1.17 ms | 123.72 µs | qp-trie 9.47× faster |
| `predictive_search` (empty prefix, all 20K) | 1.77 ms | 121.88 µs | qp-trie 14.56× faster |

<div class="callout callout-warn">
<strong>A real, disclosed loss on the read path.</strong> See
<a href="https://github.com/addlayerio/verbora/blob/main/docs/PERFORMANCE_GAPS.md#2-trie-lookup-and-prefix-enumeration--verbora-vs-qp-trie-rust">PERFORMANCE_GAPS.md
entry 2</a>: <code>qp-trie</code> is a crit-bit/PATRICIA-style radix trie with
path compression (lookup depth bounded by <em>distinguishing</em> nybbles
between stored keys, not key length) and stores each key whole at its leaf
(no reconstruction cost on enumeration); Verbora's arena trie has neither —
one hop per Unicode scalar, and every enumerated word is rebuilt one scalar
at a time. The same two properties invert for <code>build</code>, which is
why Verbora wins there instead.
</div>

`common_prefix_search` has no `qp-trie` competitor: that crate implements no
"stored words that are prefixes of a query" operation at all.

#### `fast_radix_trie` — a path-compressed radix map, and Verbora's own `FrozenTrie` answer

[fast_radix_trie](https://crates.io/crates/fast_radix_trie) 1.2.0 is a
path-compressed radix map created 2025-10-30. It uses `unsafe` internally
(dynamically-sized nodes via raw pointers, `miri`-tested per its own docs);
Verbora's own trie has zero `unsafe` anywhere.

| Operation | Verbora | fast_radix_trie | Verdict |
|---|--:|--:|---|
| `build` (random) | **1.567 ms** | 2.415 ms | Verbora 1.54× faster |
| `contains` (hit) | **1.136 ms** | 1.340 ms | Verbora 1.18× faster |
| `contains` (miss) | **1.249 ms** | 1.335 ms | Verbora 1.07× faster |
| `predictive_search` (1-char prefix) | 1.145 ms | **696.8 µs** | fast_radix_trie 1.64× faster |
| `predictive_search` (empty prefix, all 20K) | 1.453 ms | **663.4 µs** | fast_radix_trie 2.19× faster |

A genuine split, not a one-sided result: Verbora wins `build` and `contains`,
`fast_radix_trie`'s path compression wins prefix enumeration — fewer
node-hops per query, the exact property path compression buys.

<div class="callout callout-note">
<strong>Verbora's own answer: <code>FrozenTrie</code>.</strong> Given this
real evidence, <code>Trie::freeze()</code> now exists — a safe-Rust (zero
<code>unsafe</code>), path-compressed, read-only representation built once
from a <code>Trie</code>. It closes most of the gap and <strong>overtakes</strong>
<code>fast_radix_trie</code> on the realistic autocomplete shape:
</div>

| Operation | `FrozenTrie` vs. arena `Trie` | `FrozenTrie` vs. `fast_radix_trie` |
|---|--:|--:|
| `predictive_search` (1-char prefix) | **1.89× faster** | **1.06× faster** — overtakes |
| `predictive_search` (empty prefix, all 20K) | **1.49× faster** | 1.45× slower — narrowed, not closed |
| `contains` (hit/miss) | 1.5×–1.7× **slower** | also loses — arena `Trie` wins this one |

An honest trade-off, not a clean win: `FrozenTrie` closes most of the
`predictive_search` gap and beats `fast_radix_trie` outright on single-letter
prefixes (the realistic autocomplete shape), but still trails on full-corpus
enumeration, and its own `contains` genuinely regresses relative to the plain
arena. Neither representation replaces the other — `Trie` for
lookup-heavy code, `FrozenTrie` (frozen once after bulk load) for
enumeration-heavy code. See
[PERFORMANCE_GAPS.md entry 32](https://github.com/addlayerio/verbora/blob/main/docs/PERFORMANCE_GAPS.md#32-trie-prefix-enumeration--verbora-vs-fast_radix_trie-rust-path-compressed-radix)
for the full numbers and reasoning.

#### `fst` — a frozen finite-state transducer

[fst](https://crates.io/crates/fst) 0.4.7 (Andrew Gallant's) is architecturally
nothing like a trie: a finite-state transducer built once from sorted input,
queried via a `Streamer`, never mutated again. Two separate comparisons:

| Comparison | Result |
|---|---|
| `build`/`contains`/`predictive_search` (vs. plain `Trie`) | Verbora wins every operation, 1.21×–4.49× |
| Levenshtein-automaton fuzzy lookup (vs. `FuzzyIndex`) | a double crossover — see below |

`fst`'s own Levenshtein automaton (via its `levenshtein` feature) answers the
same fuzzy-candidate question as `verbora_spellcheck::FuzzyIndex` — a genuine
double crossover on both construction and query:

| Words | Construction: `FuzzyIndex` | Construction: `fst` | Query: `FuzzyIndex` | Query: `fst` |
|---:|--:|--:|--:|--:|
| 100 | **31.6 µs** | 59.4 µs | **563.7 µs** | 30.6 ms |
| 1,000 | 778.3 µs | **473.8 µs** | **10.44 ms** | 68.4 ms |
| 10,000 | 12.30 ms | **3.96 ms** | **96.89 ms** | 102.3 ms |
| 20,000 | 25.90 ms | **7.08 ms** | 187.4 ms | **128.7 ms** |

`FuzzyIndex` wins construction small, `fst` wins from 1,000 words up; query
runs the opposite direction — `FuzzyIndex` wins small and mid-size corpora
(dramatically at 100 words), `fst` overtakes only at the largest size tested.
`fst` is also one of three crates in this audit found to carry a real,
independently-confirmed upstream defect — see
[Upstream bugs found](#upstream-bugs-found).

---

### Phonetics

Eleven encoder types (`docs/COMPETITIVE_BENCHMARKS.md`
[§1.6](https://github.com/addlayerio/verbora/blob/main/docs/COMPETITIVE_BENCHMARKS.md#16-phonetics))
against [rphonetic](https://github.com/Dalvany/rphonetic) 3.0.6 — the one
actively-maintained Rust crate covering the same phonetic-algorithm
families, in the Apache commons-codec lineage, in a single crate. The
comparison runs in two regimes with two different equivalence claims, both
verified in
[`tests/phonetics_correctness.rs`](https://github.com/addlayerio/verbora/blob/main/benchmarks/competitive/rust-competitors/tests/phonetics_correctness.rs)
before any number was trusted:

- **Three variant encoders — throughput-only (`Partial`, never `Yes`).**
  `SoundEx`, `Metaphone` and `DoubleMetaphone` implement Verbora's own
  documented variants (condense-before-drop Soundex, a documented Metaphone
  stage-ordering quirk), rphonetic the textbook originals — byte-exact output
  is never asserted, only that both sides do the same *shape* of work.
- **Eight byte-exact encoders — full output equivalence (`Yes`).**
  `Cologne`, `Nysiis`, `Caverphone1`/`Caverphone2`, `Phonex`,
  `RefinedSoundex`, `MatchRatingApproach` and `DaitchMokotoff` produce output
  **byte-identical** to rphonetic's on every input rphonetic handles without
  panicking — a stronger claim than any `Partial` row on this page carries.

#### The three variant encoders — throughput only

rphonetic's Metaphone/Double Metaphone default to a 4-character max code
length; both are reconfigured to `Some(32)` here to match Verbora's real
default of 32 — independently verified by test to actually change
rphonetic's output length, not silently still capped at 4.

| Algorithm | 1 name | 10,000 names | 100,000 names |
|---|--:|--:|--:|
| Soundex | **2.79× faster** | **2.17× faster** | **2.08× faster** |
| Metaphone | **1.43× faster** | **1.03× faster** | **1.10× faster** |
| Double Metaphone | **3.27× faster** | **2.18× faster** | **4.22× faster** |

<div class="callout callout-warn">
<strong>A fourth row is retired rather than re-labelled.</strong> This group
used to carry a non-branching Daitch–Mokotoff encoder measured against
rphonetic's non-branching <code>encode()</code>. Verbora has one
Daitch–Mokotoff type, and it branches: <code>DaitchMokotoff::process</code>
returns every reading of an ambiguous spelling, pipe-joined, and
<code>DaitchMokotoff::codes</code> returns the same list structured. There is
no separate single-branch encoder for that row to describe, so the row is gone
rather than pointed at a different algorithm — its figures measured work
Verbora no longer does in that shape. The branching comparison, which is a
different pairing and a stronger claim, is in the byte-exact table below.
</div>

Verbora wins all three algorithms at every benchmarked size. Metaphone is
the closest of the three:

| Library | Version | Language | Time (median, 100,000 names) | Throughput | Relative |
|---|---|---|---:|---:|---:|
| Verbora | 0.1.0 | Rust | 6.89 ms | 145.1/s | **1.00×** |
| rphonetic | 3.0.6 | Rust | 7.57 ms | 132.2/s | 1.10× slower |

<div class="callout callout-good">
<strong>Metaphone — a clean sweep at every size.</strong> Verbora's
<code>Metaphone</code> runs as a single skip-gated driver, fused from the
original 21 ordered whole-string rewrite stages, over per-thread pooled
scratch: letter-mask gates decide which rules can possibly fire on a given
word, window edits plus fused rules replace whole-string rewrites, and the
pipeline's two scratch buffers are reused across calls — an ASCII token
folds lowercase directly into pooled scratch, so a steady-state call's
only allocation is the returned code. The original 21-stage implementation
is retained internally as the differential-test oracle it is checked
against, over a ~900K-comparison corpus. rphonetic's <code>Metaphone</code>
is a single indexed forward scan (<code>O(n)</code>); Verbora wins anyway:
<strong>1.43× at a single name</strong> (51.5 ns vs. 73.5 ns), 1.03× at
10,000 names (711.91 vs. 735.77 µs), 1.10× at 100,000 (6.89 vs. 7.57 ms).
See
<a href="https://github.com/addlayerio/verbora/blob/main/docs/PERFORMANCE_GAPS.md#6-metaphone-encoding--verbora-vs-rphonetic-rust">PERFORMANCE_GAPS.md
entry 6</a> for the full mechanism.
</div>

Full per-algorithm, per-size data for these groups:
[`results/results.json`](https://github.com/addlayerio/verbora/blob/main/benchmarks/competitive/results/results.json)
(module `"phonetics"`).

#### The eight byte-exact encoders

Verification for this table goes beyond the shape-parity check above,
because here byte-exact equality **is** the claim:
`tests/phonetics_correctness.rs` asserts identical output over the shared
653-name corpus plus per-algorithm extras for every encoder; for
`MatchRatingApproach` it additionally checks the real MRA match *decision*
(`compare`, not just the code) over every ordered pair of corpus names
(~426K pairs); and Daitch–Mokotoff is checked three ways at once — the
pipe-joined `process` string, the `codes` vector, and the first-branch code
against rphonetic's non-branching `encode`. An independent adversarial audit
then differentially fuzzed 104,114 inputs per encoder against rphonetic with
zero mismatches, proved every documented divergence exactly as narrow as
claimed, and mutation-tested the suites. The only divergences are inputs on
which rphonetic itself panics — see
[Upstream bugs found](#upstream-bugs-found); those input shapes are excluded
from the benchmark domain per this page's fairness pattern, and the
ASCII-only shared corpus never reaches them anyway. Configuration is
identical on both sides: `Nysiis` runs strict (the commons-codec default),
`Phonex` at its default max code length of 4.

**Verbora is faster in all 24 cells** (Verbora vs. rphonetic, Criterion
medians):

| Encoder | 1 name | 10,000 names | 100,000 names |
|---|--:|--:|--:|
| Cologne | **17.4 ns** vs. 72.2 ns (4.16×) | **314.99 µs** vs. 738.89 µs (2.35×) | **3.250 ms** vs. 7.321 ms (2.25×) |
| NYSIIS | **29.7 ns** vs. 224.5 ns (7.56×) | **266.53 µs** vs. 1.889 ms (7.09×) | **2.688 ms** vs. 20.517 ms (7.63×) |
| Caverphone 1.0 | **176.4 ns** vs. 914.3 ns (5.18×) | **1.907 ms** vs. 10.317 ms (5.41×) | **19.013 ms** vs. 99.617 ms (5.24×) |
| Caverphone 2.0 | **153.9 ns** vs. 811.8 ns (5.27×) | **1.770 ms** vs. 9.262 ms (5.23×) | **17.748 ms** vs. 88.009 ms (4.96×) |
| Phonex | **41.3 ns** vs. 145.7 ns (3.53×) | **483.95 µs** vs. 1.358 ms (2.81×) | **4.608 ms** vs. 12.933 ms (2.81×) |
| Refined Soundex | **14.8 ns** vs. 114.2 ns (7.73×) | **129.36 µs** vs. 972.06 µs (7.51×) | **1.298 ms** vs. 9.630 ms (7.42×) |
| Match Rating Approach | **31.9 ns** vs. 489.3 ns (15.32×) | **323.12 µs** vs. 5.344 ms (16.54×) | **3.059 ms** vs. 53.249 ms (17.41×) |
| Daitch–Mokotoff (branching) | **154.9 ns** vs. 363.4 ns (2.35×) | **1.788 ms** vs. 3.720 ms (2.08×) | **16.834 ms** vs. 37.170 ms (2.21×) |

The mechanism is consistent across all eight groups rather than one trick:
Verbora's encoders run single-pass scans over one reused buffer, with one
heap allocation per call for the returned code (the branching
Daitch–Mokotoff adds a small branch list), against static compiled-in rule
tables. rphonetic's implementations allocate intermediate `String`s as
they go (Caverphone's rewrite cascade is one freshly allocated `String` per
step there) and, for Daitch–Mokotoff, parse the rules text with a `nom`
grammar at builder time and walk a `BTreeMap` per lookup where Verbora
indexes a pre-sorted static array. The margins range from 2.08×
(Daitch–Mokotoff at 10,000 names — the one algorithm where both sides
spend most of their time in the same branching walk) to 17.41× (Match
Rating at 100,000).

The Daitch–Mokotoff row compares Verbora's branching
`DaitchMokotoff::process` against rphonetic's own pipe-joined `soundex()` —
output-format identical, which is what makes byte-exact equality the claim
here rather than shape parity. Four rphonetic Daitch–Mokotoff behavioral quirks
are reproduced deliberately for byte-parity and documented in
`crates/verbora-phonetics/src/daitch_mokotoff.rs`'s own module
documentation. Raw Criterion estimates for these eight groups live in the
same Criterion tree `cargo bench` writes — see
[Reproducing these numbers](#reproducing-these-numbers).

#### A second Double Metaphone implementation — C++, not Rust

Every other competitor on this page is a Rust crate. `pixelglow/double_metaphone`
is different: a header-only C++11 implementation of Lawrence Philips'
Double Metaphone algorithm, vendored into the workspace and compiled by
`build.rs`, then called through a thin `extern "C"` shim — the only
non-Cargo, non-Rust competitor benchmarked anywhere on this page. Every
measured call crosses the Rust/C++ boundary once (one `CString`
construction, one FFI call, two bounded buffer copies, one UTF-8
validation on the way back), and that cost is measured as part of the
number, not subtracted out.

Correctness here is `Partial`, not byte-exact: 584 of 653 real English
surnames (89.4%) produce identical primary and secondary keys on both
sides. The one confirmed, dominant rule difference: Verbora silences a
trailing `S` whenever it is preceded by `A` or `I`, while the C++ library
only silences a trailing `S` in the narrower pattern where `I` or `Y` is
immediately followed by `S` then `L` — `island`, `isle`, `carlisle`. Both
sides agree `Isle` should lose its `S`; they disagree on names like
`Davis`, which keeps its `S` on the C++ side but loses it on Verbora's,
encoding the same way `Isle` does. Neither reading is more correct;
Double Metaphone's own published algorithm write-up never fully
disambiguates this case.

| Library | Version | Language | Time (median, 653 names) | Throughput | Relative |
|---|---|---|---:|---:|---:|
| Verbora | 0.1.0 | Rust | 47.23 µs | 21.2K/s | **1.00×** |
| pixelglow/double_metaphone | 79dd226 (2014) | C++11 | 85.30 µs | 11.7K/s | 1.81× slower |

Verbora is 1.81× faster (medians, full Criterion defaults). This is a
result about one specific, vendored C++ implementation, benchmarked once
and disclosed honestly — not a claim about C++ Double Metaphone
implementations in general.

---

### Language detection

Statistical language detection over free text
(`docs/COMPETITIVE_BENCHMARKS.md`
[§1.9](https://github.com/addlayerio/verbora/blob/main/docs/COMPETITIVE_BENCHMARKS.md#19-language-detection))
— Verbora's `WhatlangDetector` against
[lingua](https://github.com/pemistahl/lingua-rs) 1.8.0 (built with
`from_languages()`, restricted to the 21-language overlap with Verbora, never
its default 75) and [whichlang](https://github.com/quickwit-oss/whichlang)
0.1.1 (13-language overlap, and — disclosed explicitly, not folded silently
into the accuracy numbers — it cannot abstain: `detect_language` always
returns a guess). A widely-used JavaScript NLP library has no general
statistical language-detection module (verified from source, not assumed),
so it does not appear here.

<div class="callout callout-note">
<strong>Speed alone is not enough for this capability</strong> — this
project's own policy requires accuracy alongside speed wherever correctness
has a statistical dimension. See <a href="#accuracy">Accuracy</a> below
before reading the speed table as the whole story.
</div>

#### Speed, by input length (English)

| Tier | Verbora | lingua | whichlang |
|---|--:|--:|--:|
| short word (~6 B) | 38.09 µs | 49.95 µs | **92.6 ns** |
| short phrase (~30 B) | 36.97 µs | 449.73 µs | **290.8 ns** |
| sentence (~140 B) | 32.12 µs | 331.66 µs | **757.1 ns** |
| paragraph (~500 B) | 103.87 µs | 650.09 µs | **6.53 µs** |

| Library | Version | Language | Time (median, paragraph) | Throughput | Relative |
|---|---|---|---:|---:|---:|
| whichlang | 0.1.1 | Rust | 6.53 µs | 153.2K/s | **1.00×** |
| Verbora | 0.1.0 | Rust | 103.87 µs | 9.6K/s | 15.91× slower |
| lingua | 1.8.0 | Rust | 650.09 µs | 1.5K/s | 99.59× slower |

<div class="callout callout-warn">
<strong>The largest gap on this page — 16×–411× slower than
<code>whichlang</code>, reported in full.</strong> See
<a href="https://github.com/addlayerio/verbora/blob/main/docs/PERFORMANCE_GAPS.md#7-language-detection--verbora-via-whatlang-vs-whichlang-rust">PERFORMANCE_GAPS.md
entry 7</a>: <code>whichlang</code> is a zero-allocation, single-pass hashed
linear model over 16 languages; the <code>whatlang</code> engine
<code>WhatlangDetector</code> wraps runs a combined alphabet-filter *and*
trigram-frequency pass (measured 25 heap allocations per call) over 22
languages. Verbora beats <code>lingua</code> consistently (6.3×–12.2×) in
the same table. The gap narrows as input grows, because
<code>whatlang</code>'s ~25-allocation overhead is largely length-independent
and gets amortized, while <code>whichlang</code>'s per-feature cost scales
with length. See the accuracy table below for what that speed buys —
or does not.
</div>

#### Speed, by language (sentence tier)

| Language | Verbora | lingua | whichlang | Verbora vs. whichlang |
|---|--:|--:|--:|--:|
| German | 56.89 µs | 234.51 µs | 816.1 ns | 69.7× slower |
| English | 30.97 µs | 333.48 µs | 567.2 ns | 54.6× slower |
| Spanish | 41.39 µs | 188.17 µs | 1.85 µs | 22.4× slower |
| French | 32.11 µs | 304.65 µs | 620.6 ns | 51.7× slower |
| Hindi | 9.64 µs | 10.62 µs | 571.3 ns | 16.9× slower |
| Italian | 33.33 µs | 251.00 µs | 634.7 ns | 52.5× slower |
| Japanese | **292.6 ns** | 9.37 µs | 330.6 ns | **1.1× faster** |
| Dutch | 31.18 µs | 427.23 µs | 609.3 ns | 51.2× slower |
| Portuguese | 31.55 µs | 280.65 µs | 1.41 µs | 22.4× slower |
| Russian | 8.44 µs | 118.54 µs | 855.7 ns | 9.9× slower |
| Swedish | 31.93 µs | 271.52 µs | 552.2 ns | 57.8× slower |
| Vietnamese | 30.49 µs | 310.10 µs | 615.1 ns | 49.6× slower |
| Chinese | **166.9 ns** | 3.43 µs | 159.7 ns | ~tied |

Japanese and Chinese are the one real exception: `whatlang`'s alphabet
pre-filter recognizes CJK codepoints immediately and short-circuits before
the far more expensive trigram pass, so those two rows barely pay the
25-allocation cost that dominates every Latin-script row.

#### Accuracy

13 languages (the triple overlap all three detectors can be scored on
identically) × 4 length tiers, sourced from the OHCHR UDHR Translation
Project (public-domain UN text; full sourcing and per-tier extraction rule in
[`datasets/README.md`](https://github.com/addlayerio/verbora/blob/main/benchmarks/competitive/datasets/README.md)).
Reproduced with `cargo run --release --example language_accuracy`.

| Detector | short word | short phrase | sentence | paragraph | **Overall** |
|---|--:|--:|--:|--:|--:|
| lingua (21-language restricted) | 92.3% (12/13) | 100% | 100% | 100% | **98.1%** (51/52) |
| Verbora (`WhatlangDetector`) | 76.9% (10/13, 1 abstained) | 100% | 100% | 100% | **94.2%** (49/52) |
| whichlang (13-language, cannot abstain) | 69.2% (9/13) | 100% | 100% | 100% | **92.3%** (48/52) |

**At phrase length and longer, this 13-language test set does not
distinguish the three detectors at all** — every one is perfect. The entire
accuracy gap lives at the single-word tier, where `lingua`'s dedicated
short-input model has a real edge, Verbora abstains once rather than guess
wrong (scored as incorrect here, per this report's own coverage-vs-accuracy
distinction), and `whichlang` — which cannot abstain — has the lowest raw
accuracy at exactly this tier, the same tier where it is 400×+ faster. Read
alongside the speed table above: `whichlang`'s speed is real, and its
accuracy cost at the hardest tier is real too — neither is quoted here
without the other. 13 languages is a small test set (one extra mistake
swings the short-word percentage by ~8 points) — see that dataset's own
README for this caveat stated in full.

#### `WhatlangDetector` wrapper overhead — not a ranked comparison

<div class="callout callout-note">
Isolates the cost of Verbora's own wrapper around <code>whatlang::Detector</code>
— it is <strong>not</strong> "Verbora vs. whatlang," because
<code>WhatlangDetector</code> literally constructs a
<code>whatlang::Detector</code> and calls <code>.detect()</code> on it.
Numbers below are this page's own single run. The noisy ratios include a tier where the
wrapper measured <em>faster</em> than the bare call it makes, which is
structurally impossible as a real effect. Read as noise from a shared
benchmark machine, not a finding.
</div>

| Tier | Verbora (`WhatlangDetector`) | whatlang (raw crate) | Ratio |
|---|--:|--:|--:|
| short word | 39.15 µs | 26.99 µs | 1.45× |
| short phrase | 53.57 µs | 52.34 µs | 1.02× |
| sentence | 37.03 µs | 32.21 µs | 1.15× |
| paragraph | 102.01 µs | 146.49 µs | 0.70× |

---

### Script detection

Verbora's `detect_script` against
[`whatlang::detect_script`](https://github.com/greyblake/whatlang-rs) 0.18.0
(`docs/COMPETITIVE_BENCHMARKS.md`
[§1.10](https://github.com/addlayerio/verbora/blob/main/docs/COMPETITIVE_BENCHMARKS.md#110-script-detection))
— a real, public, standalone function doing the same conceptual work
(per-codepoint Unicode-range classification, majority vote), just over a
wider set (25 scripts vs. Verbora's 10). A widely-used JavaScript NLP
library has no script-detection module at all (verified from source).

| Tier | Verbora | whatlang | Verbora advantage |
|---|--:|--:|--:|
| short word | 9.7 ns | 55.1 ns | **5.7×** |
| short phrase | 28.4 ns | 59.1 ns | **2.1×** |
| sentence | 70.4 ns | 114.3 ns | **1.6×** |
| paragraph | 786.0 ns | 1.05 µs | **1.3×** |

| Library | Version | Language | Time (median, paragraph) | Throughput | Relative |
|---|---|---|---:|---:|---:|
| Verbora | 0.1.0 | Rust | 786.0 ns | 1.27M/s | **1.00×** |
| whatlang | 0.18.0 | Rust | 1.05 µs | 951.8K/s | 1.34× slower |

Verbora wins at every length (narrowing as input grows, since both are
`O(chars × scripts)` with different constants). By language (sentence
tier), Verbora is faster or tied in 12 of 13; the one exception is honestly
reported rather than dropped:

| Language | Verbora | whatlang | Faster |
|---|--:|--:|---|
| German | 69.4 ns | 118.3 ns | Verbora |
| English | 70.6 ns | 111.3 ns | Verbora |
| Spanish | 183.6 ns | 342.9 ns | Verbora |
| French | 80.9 ns | 127.4 ns | Verbora |
| Hindi | 180.7 ns | 180.4 ns | ~tied |
| Italian | 74.9 ns | 121.0 ns | Verbora |
| Japanese | 231.6 ns | 276.5 ns | Verbora |
| Dutch | 83.1 ns | 122.9 ns | Verbora |
| Portuguese | 115.5 ns | 124.1 ns | Verbora |
| **Russian** | **163.9 ns** | **126.3 ns** | whatlang, 1.3× |
| Swedish | 97.6 ns | 202.7 ns | Verbora |
| Vietnamese | 105.6 ns | 147.3 ns | Verbora |
| Chinese | 115.6 ns | 147.6 ns | Verbora |

Russian's small loss is disclosed rather than dropped; no source-level cause
that would predict Cyrillic specifically was found, and it is plausibly this
run's reduced-sample noise floor.

---

### Transliteration

Japanese kana→romaji, throughput only — against
[wana_kana](https://github.com/PSeitz/wana_kana_rust) 5.0.0
(`docs/COMPETITIVE_BENCHMARKS.md`
[§1.11](https://github.com/addlayerio/verbora/blob/main/docs/COMPETITIVE_BENCHMARKS.md#111-transliteration)),
the only Rust kana↔romaji crate with real current adoption/maintenance
found — every alternative investigated is scope-mismatched or effectively
abandoned. **Never an output-correctness comparison**: `wana_kana` uses a
doubled-vowel convention (`"スーパー"` → `"suupaa"`) while Verbora uses
modified Hepburn with macrons (`"tōkyō"`) — a real, executed divergence
proven in
[`tests/transliteration_convention_diff.rs`](https://github.com/addlayerio/verbora/blob/main/benchmarks/competitive/rust-competitors/tests/transliteration_convention_diff.rs),
not merely asserted.

| Repeats | Verbora | wana_kana | Verbora advantage |
|---:|--:|--:|--:|
| 1× | 724.6 ns | 1.00 µs | **1.4×** |
| 16× | 5.59 µs | 12.55 µs | **2.2×** |
| 256× | 88.51 µs | 206.78 µs | **2.3×** |

| Library | Version | Language | Time (median, 256×) | Throughput | Relative |
|---|---|---|---:|---:|---:|
| Verbora | 0.1.0 | Rust | 88.51 µs | 11.3K/s | **1.00×** |
| wana_kana | 5.0.0 | Rust | 206.78 µs | 4.8K/s | 2.34× slower |

---

### POS tagging

`BrillTagger` (transformation-based, English) against
[postagger](https://github.com/shubham0204/postagger.rs) 0.0.3 (a pretrained
averaged-perceptron model, NLTK weights) and
[rust-bert](https://github.com/guillaume-be/rust-bert) 0.23.0's `POSModel`
(a MobileBERT transformer pipeline) — the most widely adopted general Rust
NLP crate found in this whole audit (254K downloads, 3,077 stars)
(`docs/COMPETITIVE_BENCHMARKS.md`
[§1.16](https://github.com/addlayerio/verbora/blob/main/docs/COMPETITIVE_BENCHMARKS.md#116-pos-tagging)).
Both are genuinely different algorithm classes — a trained classifier and a
transformer forward pass, not rival implementations of the same rule table —
so this is reported as a technique comparison, with cold start and steady
state kept strictly separate.

<div class="callout callout-warn">
<strong>Verbora's steady-state medians are pending re-measurement.</strong>
Four of the eighteen bundled English rules test whether the current token looks
like a URL, and that predicate was rewritten: a token is URL-like when one of
its full stops joins two word-shaped labels, a stricter test than the one the
recorded medians ran, which any interior full stop satisfied. Abbreviations
such as <code>Ph.D.</code> and
<code>W.Va.</code> therefore keep their lexicon tag instead of being retagged
<code>URL</code> — so both the answers and the per-token work changed underneath
the recorded medians. The cold-start figures are unaffected: construction reads
the same packed table with the same entry count and the same eighteen rules.
Competitor figures are unaffected too; no competitor version moved. The
steady-state medians stay visible so the next run has something to diff against,
and none may be quoted as current; the direction of the change must not be
inferred either. What survives is the structural finding this section is about —
a fixed rule table needs no deserialization step, and a deterministic rule
lookup is categorically less arithmetic than a feature-weighted vote or a
transformer forward pass.
</div>

#### Cold start — everything needed before tagging one sentence

| Library | Version | Language | Time (median) | Relative |
|---|---|---|---:|---:|
| Verbora (`Lexicon` + `RuleSet` + `BrillTagger`) | 0.1.0 | Rust | 7.47 µs | **1.00×** |
| postagger (parses a 5.6 MB weights file) | 0.0.3 | Rust | 121.61 ms | 16,283× slower |
| rust-bert (loads a ~94 MB MobileBERT checkpoint) | 0.23.0 | Rust | 1.281 s | 171,501× slower |

Expected and by design: a fixed rule table needs no deserialization step;
a pretrained model must load its weights first.

#### Steady state — per-call latency, tagger already constructed

| Library | Version | Language | Time (median, 9 tokens) | Time (median, 20 tokens) | Time (median, batch of 8×9-tok) |
|---|---|---|---:|---:|---:|
| Verbora | 0.1.0 | Rust | **1.75 µs** | **4.23 µs** | **14.08 µs** |
| postagger | 0.0.3 | Rust | 62.40 µs (35.6× slower) | 80.46 µs (19.0× slower) | 549.50 µs (39.0× slower) |
| rust-bert | 0.23.0 | Rust | 17.26 ms (9,854× slower) | 10.93 ms (2,583× slower) | 24.62 ms (1,749× slower) |

Verbora wins every steady-state row — the expected result once the technique
gap is named, not a surprising one: `postagger` still evaluates a
feature-weighted vote per token, and `rust-bert`'s MobileBERT pass is a full
transformer forward pass, categorically more arithmetic than a deterministic
rule lookup. **This is not an accuracy claim for either technique** — see
`docs/COMPETITIVE_BENCHMARKS.md` §1.16 for why both rows are `Partial`, not
`Yes`; this audit makes no tagging-quality comparison for POS tagging.

---

### Spellcheck

`Spellcheck::corrections` and `::is_correct` against three genuinely
different algorithms (`docs/COMPETITIVE_BENCHMARKS.md`
[§1.17](https://github.com/addlayerio/verbora/blob/main/docs/COMPETITIVE_BENCHMARKS.md#117-spellcheck)):
[symspell](https://github.com/reneklacan/symspell) 0.5.2 (precomputed
deletion dictionary), [harper-core](https://github.com/Automattic/harper)
2.8.0 (FST + Levenshtein automaton; by far the most widely adopted
standalone spellchecking crate found, 14,470 GitHub stars on its parent
repo), and [spellbook](https://github.com/helix-editor/spellbook) 0.4.2
(Hunspell affix-rule morphology).

<div class="callout callout-warn">
<strong>Every Verbora median in this section is pending re-measurement.</strong>
The crate's correction contract changed — <code>corrections</code> returns
<code>Vec&lt;Correction&gt;</code>, carrying the frequency and distance behind
each ranking rather than words alone, and <code>correction_words</code> is the
separate call for owned words — so the recorded medians measured a different
return shape from the one that runs. <code>verbora-spellcheck</code>'s own
documentation publishes no speed figure for the same reason. Competitor figures are
unaffected; no competitor version moved. The medians stay visible so the next
run has something to diff against, and none may be quoted as current; the
direction of the change must not be inferred either. What is unaffected is the
structural finding each table below is really about — which algorithm class
pays at construction and which pays per query — because that follows from the
designs, not from a timing.
</div>

#### `symspell` and `harper-core` — same corpus as Verbora

Both loaded with the identical `words.json` corpus and per-word frequencies
Verbora uses.

| Group | Corpus | Verbora | symspell | harper-core |
|---|--:|--:|--:|--:|
| construction (`new`) | 100 | **14.05 µs** | 402.42 µs (28.6× slower) | 77.97 µs (5.6× slower) |
| construction (`new`) | 20,000 | **3.58 ms** | 115.42 ms (32.3× slower) | 10.95 ms (3.1× slower) |
| `is_correct` (hit) | 20,000 | **226.16 µs** | 310.15 µs (1.4× slower) | 271.89 µs (1.2× slower) |
| `corrections`, distance 1 | 100 | 22.00 µs | **921 ns** (23.9× faster) | 5.02 µs (4.4× faster) |
| `corrections`, distance 1 | 20,000 | 22.95 µs | **857 ns** (26.8× faster) | 36.87 µs (1.6× slower) |
| `corrections`, distance 2 | 1,000 | 4.96 ms | **2.31 µs** (2,152× faster) | 34.79 µs (142.7× faster) |
| `corrections`, distance 2 | 20,000 | 5.80 ms | **3.50 µs** (1,657× faster) | 333.83 µs (17.4× faster) |

Verbora wins construction and membership-testing at every size against both
competitors. `symspell` and, at the largest corpus, `harper-core` win
correction generation — by enormous margins at distance 2.

<div class="callout callout-warn">
<strong>Two real, disclosed losses, with the matching cost trade-off shown
alongside each.</strong> See
<a href="https://github.com/addlayerio/verbora/blob/main/docs/PERFORMANCE_GAPS.md#8-spellcheck-corrections--verbora-vs-symspell-and-harper-core-rust">PERFORMANCE_GAPS.md
entry 8</a>: <code>symspell</code>'s query speed is bought with 29×–32× more
expensive construction — the delete-dictionary is precomputed once at load
instead of generated per query. <code>harper-core</code>
(<a href="https://github.com/addlayerio/verbora/blob/main/docs/PERFORMANCE_GAPS.md#8b-the-same-query-against-harper-core--a-crossover-not-a-one-sided-loss">entry
8b</a>) is a genuine <em>crossover</em>, not a one-sided loss: it wins at
small corpora and at distance 2 throughout, but Verbora wins distance-1
correction once the corpus reaches 10,000–20,000 words.
</div>

#### `spellbook` — matched-workload timing only, not a fair ratio

<div class="callout callout-note">
Hunspell's <code>.aff</code>/<code>.dic</code> format has no concept of a
flat frequency corpus — <code>spellbook</code> cannot load Verbora's corpus,
and Verbora cannot load a Hunspell dictionary. This is a timing comparison of
two different dictionaries doing conceptually the same job, <strong>never</strong>
presented as a ratio. No Relative column below.
</div>

| Operation | Library | Version | Dictionary | Time (median) |
|---|---|---|---|--:|
| `check` / `is_correct`, hit | spellbook | 0.4.2 | real `en_US` Hunspell | 372.5 ns |
| `check` / `is_correct`, near-miss typo | spellbook | 0.4.2 | real `en_US` Hunspell | 3.25 µs |
| `is_correct`, hit | Verbora | 0.1.0 | own 20,000-word corpus | 41.71 µs |
| `suggest` / `corrections`, one typo (4 probes) | spellbook | 0.4.2 | real `en_US` Hunspell | 5.28–9.05 ms |
| `corrections`, one typo (`typo8`) | Verbora | 0.1.0 | own 20,000-word corpus | 23.83 µs |

`spellbook`'s `check` is a curated, bundled FST/hash lookup — sub-microsecond
as expected for a fixed, pre-built dictionary — while its full affix-aware
`suggest` costs milliseconds, the opposite trade-off from its own `check`.

#### `fast_symspell` — a second deletion-index crate, and Verbora's own answer to it

[fast_symspell](https://crates.io/crates/fast_symspell) 0.1.10 is a second,
independent SymSpell-family implementation. Its published metadata carries
no linked repository, but its source is real and readable via crates.io's
own tarball — a near-verbatim (confirmed line-for-line) fork of `symspell`
0.5.2 with three real deltas: `ahash` hashing, a `triple_accel`-backed
verification pass (which
carries its own real, independently-confirmed bug — see
[Upstream bugs found](#upstream-bugs-found)), and an `rkyv`
zero-copy archived-load path. Loaded with Verbora's own corpus, same
discipline as `symspell` above.

| Group | Corpus | Verbora | fast_symspell |
|---|--:|--:|--:|
| construction | 100 | **12.8 µs** | 356.8 µs (27.8× slower) |
| construction | 20,000 | **3.60 ms** | 122.3 ms (34.0× slower) |
| `corrections`, distance 1 | 100 | 21.19 µs | **766 ns** (27.7× faster) |
| `corrections`, distance 1 | 20,000 | 23.15 µs | **896 ns** (25.9× faster) |
| `corrections`, distance 2 | 1,000 | 5.97 ms | **2.16 µs** (2,769× faster) |
| `corrections`, distance 2 | 20,000 | 5.54 ms | **3.29 µs** (1,686× faster) |

The same shape as `symspell` above, more extreme at distance 2 — a
delete-precomputation index trades expensive, size-scaling construction for
near-flat query cost, and Verbora's own combinatorial edit generation is the
side paying a large, roughly size-independent cost per call once distance
reaches 2.

<div class="callout callout-note">
<strong>Verbora's own answer: <code>DeletionIndex</code>.</strong> Given this
real, repeated evidence that a deletion index wins query speed by a widening
margin, <code>verbora_spellcheck::DeletionIndex</code> now exists — a
SymSpell-style index built in-house, offered alongside the existing
<code>FuzzyIndex</code> BK-tree rather than replacing it. See
<a href="#fuzzyindex-vs-deletionindex-%E2%80%94-two-verbora-native-structures">FuzzyIndex
vs. DeletionIndex</a> below for the real, measured trade-off between them.
</div>

#### `FuzzyIndex` vs. `DeletionIndex` — two Verbora-native structures

Neither of these has a reference counterpart — both are Verbora-native
extensions answering the same question (*which stored words are within edit
distance `k` of this query?*) via different mechanisms.
[`FuzzyIndex`](https://github.com/addlayerio/verbora/blob/main/crates/verbora-spellcheck/src/fuzzy_index.rs)
is a BK-tree, `max_distance` chosen per query.
[`DeletionIndex`](https://github.com/addlayerio/verbora/blob/main/crates/verbora-spellcheck/src/deletion_index.rs)
is a SymSpell-style deletion index, `max_distance` fixed once at
construction — built after real evidence (the `fast_symspell` comparison
above) showed the query-speed trade-off was worth having available, not
built speculatively ahead of that evidence.

<div class="callout callout-warn">
<strong>Every figure below is pending re-measurement.</strong>
<code>DeletionIndex</code>'s map is now keyed on a 64-bit hash of each
deletion sequence rather than on the sequence itself, which takes the cost
of indexing one <em>n</em>-scalar word from cubic in <em>n</em> to
quadratic — a change of cost class, not a constant factor. The numbers
below were measured against the sequence-keyed map and describe a structure
that no longer exists. The table and the paragraph beneath it stay visible
so a future run has something to diff against, and none of their figures
may be quoted as current; the direction of the change must not be inferred
either. See
<a href="https://github.com/addlayerio/verbora/blob/main/docs/PERFORMANCE_GAPS.md#35-spellcheck-a-real-architectural-trade-off-not-a-one-sided-loss--verbora-vs-fast_symspell-rust-symspell-family-deletion-index">PERFORMANCE_GAPS.md
entry 35</a>.
</div>

| Words | Construction: `FuzzyIndex` | Construction: `DeletionIndex` | Query: `FuzzyIndex` | Query: `DeletionIndex` |
|---:|--:|--:|--:|--:|
| 100 | **38.7 µs** | 977.6 µs (25.3× slower) | **589.9 µs** | 1.018 ms (1.73× slower) |
| 1,000 | **779.9 µs** | 11.83 ms (15.2× slower) | 10.93 ms | **2.23 ms** (4.9× faster) |
| 10,000 | **12.39 ms** | 162.6 ms (13.1× slower) | 93.28 ms | **2.64 ms** (35.3× faster) |
| 20,000 | **26.97 ms** | 407.0 ms (15.1× slower) | 174.1 ms | **3.21 ms** (54.3× faster) |

As recorded against that now-superseded map: a genuine crossover on the
query side, `FuzzyIndex` faster at the smallest corpus tested (100 words),
where a shallow BK-tree beats a deletion index's fixed per-query overhead,
and `DeletionIndex` winning from 1,000 words up by a rapidly widening
margin; construction the honest cost throughout, `DeletionIndex` 13×–25×
slower to build at every size. What survives without a fresh run is the
shape alone, since it follows from the two designs rather than either
number: dearer construction bought against a query cost that grows far more
slowly with corpus size. Neither structure replaces the other — `FuzzyIndex`
stays the default (cheaper, more predictable, no build-time distance
ceiling); reach for `DeletionIndex` when the dictionary is large,
`max_distance` is known ahead of time, and query volume is high enough to
amortize the one-time build.

---

### TF-IDF

Corpus build/ingestion and query/scoring
(`docs/COMPETITIVE_BENCHMARKS.md`
[§1.12](https://github.com/addlayerio/verbora/blob/main/docs/COMPETITIVE_BENCHMARKS.md#112-tf-idf))
against [tfidf (afshinm)](https://github.com/afshinm/tf-idf) 0.3.0 (a
stateful `add()`/`idf()`/`tfidf()` struct — the architecturally closest Rust
match found, but a genuinely different, unsmoothed weighting formula) and
[rust-tfidf](https://github.com/ferristseng/rust-tfidf) 1.1.1
(query/scoring only — it has no ingestion step, nothing to time as "build").
Both comparisons are explicitly **build/query speed only** — neither crate's
output values are compared against Verbora's, since the weighting formulas
differ by design.

<div class="callout callout-warn">
<strong>Verbora's build and query medians are pending re-measurement.</strong>
Both timed paths tokenize inside the measured region, and
<code>verbora-tfidf</code>'s tokenizer and its build path were both rewritten
underneath these numbers. Competitor figures are unaffected — <code>tfidf</code>
(afshinm) is still pinned at 0.3.0 and does not tokenize at all, which is the
asymmetry this section documents and which has not changed. The two
<em>findings</em> are structural and survive independently of any timing:
ingestion does strictly more work on Verbora's side, and it buys an index
whose query cost does not grow with the corpus, which neither competitor has
an equivalent for. Only their magnitudes are unbacked. See
<a href="https://github.com/addlayerio/verbora/blob/main/docs/PERFORMANCE_GAPS.md#13-tf-idf-ingestion--verbora-vs-tfidf-afshinm-rust">PERFORMANCE_GAPS.md</a>
entries 13 and 14.
</div>

| Library | Version | Language | Time (median, build, 256 docs) | Throughput | Relative |
|---|---|---|---:|---:|---:|
| tfidf (afshinm) | 0.3.0 | Rust | 75.19 ms | 13.3/s | **1.00×** |
| Verbora | 0.1.0 | Rust | 522.49 ms | 1.9/s | 6.95× slower |

| Docs | Verbora | tfidf (afshinm) |
|---:|--:|--:|
| 4 | 8.73 ms | 1.10 ms |
| 16 | 33.99 ms | 4.54 ms |
| 64 | 134.79 ms | 18.76 ms |
| 256 | 522.49 ms | 75.19 ms |

<div class="callout callout-warn">
<strong>A real, disclosed ingestion loss — with the matching query-time win
it buys, shown right below.</strong> See
<a href="https://github.com/addlayerio/verbora/blob/main/docs/PERFORMANCE_GAPS.md#13-tf-idf-ingestion--verbora-vs-tfidf-afshinm-rust">PERFORMANCE_GAPS.md
entry 13</a>: <code>tfidf</code>'s <code>add()</code> is a single space-split
pass with zero allocation, no lowercasing, no real tokenizer, no stop-word
filtering. Verbora's <code>add_document</code> runs its own full
pipeline — lowercasing, real word-boundary tokenization, stop-word
filtering, interning — because that is what its own behaviour contract and its
own <code>O(1)</code> query-time payoff (below) require.
</div>

| Library | Version | Language | Time (median, `tfidf()` query, 256 docs) | Throughput | Relative |
|---|---|---|---:|---:|---:|
| Verbora | 0.1.0 | Rust | 64.7 ns | 15.46M/s | **1.00×** |
| rust-tfidf | 1.1.1 | Rust | 1.03 ms | 971.9/s | 15,904× slower |
| tfidf (afshinm) | 0.3.0 | Rust | 235.99 ms | 4.2/s | ~3.65M× slower |

| Docs | Verbora | tfidf (afshinm) | rust-tfidf |
|---:|--:|--:|--:|
| 4 | 65.1 ns | 4.33 ms | 12.47 µs |
| 16 | 65.0 ns | 15.79 ms | 41.86 µs |
| 64 | 63.8 ns | 59.39 ms | 131.99 µs |
| 256 | 64.7 ns | 235.99 ms | 1.03 ms |

Verbora's query cost is **flat regardless of corpus size** (the interned,
incrementally-maintained document-frequency table this crate's own build
cost pays for); both competitors rescan the whole corpus on every query, so
their cost grows linearly with it. `idf()` shows the same pattern (module
`"tfidf"`, group `"idf"`, in `results.json`). This is the same trade-off in
both directions: expensive-but-thorough ingestion buying near-free,
corpus-size-independent queries — not a one-sided result either way.

---

### Classifiers

`BayesClassifier` training and prediction against
[smartcore](https://github.com/smartcorelib/smartcore) 0.6.5's
`MultinomialNB` (by far the most downloaded classifier candidate found, 476K
downloads, actively maintained) and
[linfa-bayes](https://github.com/rust-ml/linfa) 0.8.1's `MultinomialNb`
(`docs/COMPETITIVE_BENCHMARKS.md`
[§1.13](https://github.com/addlayerio/verbora/blob/main/docs/COMPETITIVE_BENCHMARKS.md#113-classifiers)).
Both operate on a pre-built dense count matrix, not raw text — vocabulary
construction is placed *inside* the timed closure on both sides specifically
to match Verbora's own text-in/model-out boundary, per this module's own
bench doc comment.

| Library | Version | Language | Time (median, train, 1024 docs) | Throughput | Relative |
|---|---|---|---:|---:|---:|
| smartcore | 0.6.5 | Rust | 1.52 ms | 658.3/s | **1.00×** |
| linfa-bayes | 0.8.1 | Rust | 2.89 ms | 346.5/s | 1.90× slower |
| Verbora | 0.1.0 | Rust | 8.90 ms | 112.4/s | 5.86× slower |

| Docs | Verbora | smartcore | linfa-bayes |
|---:|--:|--:|--:|
| 4 | 32.07 µs | **5.35 µs** | 422.85 µs |
| 16 | 136.63 µs | **24.62 µs** | 1.06 ms |
| 64 | 583.80 µs | **89.92 µs** | 1.76 ms |
| 256 | 2.30 ms | **313.92 µs** | 2.04 ms |
| 1024 | 8.90 ms | **1.52 ms** | 2.89 ms |

| Library | Version | Language | Time (median, predict) | Throughput | Relative |
|---|---|---|---:|---:|---:|
| linfa-bayes | 0.8.1 | Rust | 1.02 µs | 979.9K/s | **1.00×** |
| smartcore | 0.6.5 | Rust | 3.71 µs | 269.3K/s | 3.64× slower |
| Verbora | 0.1.0 | Rust | 9.31 µs | 107.4K/s | 9.12× slower |

<div class="callout callout-warn">
<strong>A real, disclosed loss — a clean one against smartcore, a genuine
crossover against linfa-bayes.</strong> See
<a href="https://github.com/addlayerio/verbora/blob/main/docs/PERFORMANCE_GAPS.md#15-naive-bayes-training-and-prediction--verbora-vs-smartcore-and-linfa-bayes-rust">PERFORMANCE_GAPS.md
entry 15</a>: Verbora is 6.0×–6.9× slower than smartcore at every size, but
<em>faster</em> than linfa-bayes below 256 docs (13× at 4 docs) and slower
above it. Verbora's per-document cost includes real, specified
tokenization, Porter stemming and stop-word filtering; neither competitor's
benchmark adapter does any of that — a whitespace split and a lowercase is
the whole preprocessing step on both competing sides.
</div>

#### Accuracy: is the slower classifier at least more correct?

A separate, signal-bearing corpus (four non-overlapping topical
vocabularies, generated by `tools/bench-data/generate.py`) was built
specifically because the training corpus above is shape-only random data —
useless for accuracy. `cargo test --test classifiers_accuracy` trains all
three implementations at each size and scores them against a fixed,
disjoint 128-document test set:

| Train size | Verbora | smartcore | linfa-bayes |
|---|--:|--:|--:|
| 4 | **98.4%** | 93.0% | 93.0% |
| 16 | 100.0% | 100.0% | 100.0% |
| 64 | 100.0% | 100.0% | 100.0% |
| 256 | 100.0% | 100.0% | 100.0% |
| 1024 | 100.0% | 100.0% | 100.0% |

All three converge to a perfect score by 16 training documents. Read
alongside the speed table above: this is neither "Verbora is slower but more
correct" nor "faster but less correct" — accuracy is statistically
indistinguishable between the three at every size that matters, so the speed
numbers stand as measured, not offset by a quality difference that is not
actually there on this test set. (This corpus's four vocabularies share no
words at all — an easy separation problem, good for catching a broken
implementation, not sharp enough to meaningfully discriminate real-world
accuracy past the smallest size.)

---

## No Rust competitor exists: WordNet, analyzers, sentiment

Three of the workspace's 19 audited modules have **no fair Rust competitor at
all** — every candidate found was investigated and rejected on maintenance,
adoption or scope grounds (WordNet: the one candidate is abandoned since
2017; analyzers and sentiment: no Rust crate performs the specific composed
task). Per this project's `NO FAIR COMPETITOR FOUND` policy, none is forced.
A widely-used JavaScript NLP library remains the available baseline for those
modules. Reproduce or publish any comparison using the method on this site;
do not infer a Rust ranking where no equivalent Rust implementation exists.

N-grams has a real Rust competitor for character n-gram generation — see
[N-Grams](#n-grams) above. Its separate comparison against the JavaScript
library is a separate result set from the Rust comparison documented here.

Full reasoning for every rejected candidate:
[`docs/COMPETITIVE_BENCHMARKS.md` § 3](https://github.com/addlayerio/verbora/blob/main/docs/COMPETITIVE_BENCHMARKS.md#3-modules--sub-capabilities-with-no-fair-competitor-identified).

**Phonetic Index / Phonetic Neighbors** (`PhoneticIndex`) has zero
competitors of any kind — a Verbora-native extension with no upstream
equivalent to compare against. Its own internal build/query benchmark suite
lives on the
[Phonetic neighbors](../features/phonetic-index.md#performance-characteristics)
feature page instead of here.

## Upstream bugs found

Two verification disciplines surfaced real, reproducible defects in
third-party dependencies — none in Verbora's own code: re-verifying crates
flagged as stale or abandoned before trusting their numbers (this audit's
own "do not trust marketing benchmarks — reproduce locally" rule), and the
differential fuzzing behind the byte-exact phonetics table
([Phonetics](#phonetics)). Disclosed here, not filed upstream without
separate confirmation.

- **`triple_accel` 0.4.0** — `rdamerau("tac", "tatc")` returns **2**; the
  correct restricted-Damerau-Levenshtein (OSA) distance is **1**. It
  over-counts an insertion adjacent to a repeated character. Both of the
  crate's restricted-Damerau entry points carry it — `rdamerau` and
  `rdamerau_exp` alike — confirmed against a from-scratch three-row OSA
  implementation, `strsim::osa_distance`, `rapidfuzz`'s `distance::osa` and
  Verbora's `osa` (all four return 1), and found by a randomized sweep
  rather than by inspection. Real impact: `fast_symspell` uses this family
  as its post-lookup verification pass, so it can silently miss or misrank a
  correction on an ordinary doubled-letter typo.
- **`fst` 0.4.7** — its `Levenshtein` automaton silently returns *incomplete*
  results for same-byte-length multi-byte UTF-8 substitutions (e.g. Cyrillic
  characters one substitution apart). Matches a still-open upstream issue,
  [BurntSushi/fst#38](https://github.com/BurntSushi/fst/issues/38), opened
  2017. The ASCII-only corpus this page's own `fst` comparisons use never
  exercises it.
- **`eddie` 0.4.2** — its internal buffer code violates a
  `slice::get_unchecked_mut` safety precondition on *ordinary* input (the
  textbook Wikipedia Jaro example, not an edge case), aborting any
  debug-profile build on a modern Rust toolchain. Does not reproduce in
  `--release`; the numbers on this page (a `--release` audit throughout) are
  unaffected. Real, if latent, evidence for the "abandoned since 2020" caveat
  this page already carries for that crate.
- **`rphonetic` 3.0.6** — several encoders panic on realistic non-ASCII
  input, all reproduced against 3.0.6 release builds during the differential
  fuzzing above: `Nysiis` in strict mode byte-slices its code at offset 6
  with no character-boundary check, panicking whenever a longer code's byte
  6 splits a multi-byte character (4,233 of the 104,114 fuzzed inputs);
  `Caverphone1`/`Caverphone2` panic the same way at their fixed 6-/10-byte
  code cut; `RefinedSoundex` indexes a 26-entry mapping table out of bounds
  for any alphabetic character whose uppercase form leaves `A`–`Z` (`é`,
  Cyrillic, CJK, the Kelvin sign); and `MatchRatingApproach` can panic on
  both its encode path (mid-character truncation) and its compare path (an
  empty-encoding underflow: `("ab", "..")` panics where `("..", "ab")`
  returns `false`). The ASCII-only shared corpus never exercises any of
  these; on every one of those inputs Verbora's own eight byte-exact
  encoders return a documented substitute output instead of panicking — the
  one place they deliberately do not match rphonetic.

## Library coverage summary

What each library actually covers, not what its scope implies. ✓ = genuine,
broadly-equivalent coverage exists; **P** = coverage exists but only for a
narrowed input domain, a reconfigured competitor, or part of the module; **—**
= no fair competitor was found in that ecosystem for this capability at all.
This does not claim other libraries are trying to be all-in-one — it shows
honestly that none of them are, without implying Verbora's breadth makes it
better at any one of these than a specialist crate necessarily is.

| Capability | Verbora | JS library | Rust ecosystem |
|---|:--:|:--:|:--:|
| Tokenizers | ✓ | ✓ | P |
| N-grams | ✓ | ✓ | P |
| Stemmers | ✓ | ✓ | P |
| Normalizers | ✓ | ✓ | P |
| Inflectors | ✓ | ✓ | P |
| Phonetics | ✓ | ✓ | P |
| Phonetic index / neighbors | ✓ | — | — |
| Distances | ✓ | ✓ | ✓ |
| Language detection | ✓ | — | P |
| Script detection | ✓ | — | P |
| Transliteration | ✓ | ✓ | P |
| TF-IDF | ✓ | ✓ | P |
| Classifiers (Bayes / logistic / MaxEnt) | ✓ | ✓ | P |
| Sentiment | ✓ | ✓ | — |
| WordNet | ✓ | ✓ | — |
| POS tagging | ✓ | ✓ | P |
| Spellcheck | ✓ | ✓ | P |
| Trie | ✓ | ✓ | P |
| Analyzers | ✓ | ✓ | — |

**19 of 19 — Verbora.** **16 of 19 — the JS library** (missing language
detection, script detection, and phonetic indexing, which it never
implemented). **0 of 19 — any single Rust crate** at full, unqualified
equivalence across a whole module; the Rust ecosystem's real strength shows
up *inside* individual algorithms instead — `strsim`/`rapidfuzz` are genuine
`Yes`-equivalence competitors for most of Distances, `rust-stemmers` for 9 of
Verbora's 16 stemmers — not as one library matching Verbora's combined scope.
That fragmentation is the whole reason this audit went module-by-module
rather than searching for one all-in-one Rust rival: no such rival exists,
and claiming one would misrepresent the comparison.

## Competitors — attribution

Every library compared anywhere on this page, with its official repository,
package registry page, and documentation.

| Library | Language | Version | License | Repository | Package | Docs |
|---|---|---|---|---|---|---|
| JavaScript NLP library | JavaScript | 8.1.1 | MIT | — | — | repository README |
| ngrammatic | Rust | 0.7.0 | MIT | [GitHub](https://github.com/compenguy/ngrammatic) | [crates.io](https://crates.io/crates/ngrammatic) | [docs.rs](https://docs.rs/ngrammatic/0.7.0) |
| strsim | Rust | 0.11.1 | MIT | [GitHub](https://github.com/rapidfuzz/strsim-rs) | [crates.io](https://crates.io/crates/strsim) | [docs.rs](https://docs.rs/strsim/0.11.1) |
| rapidfuzz | Rust | 0.5.0 | MIT | [GitHub](https://github.com/rapidfuzz/rapidfuzz-rs) | [crates.io](https://crates.io/crates/rapidfuzz) | [docs.rs](https://docs.rs/rapidfuzz/0.5.0) |
| triple_accel | Rust | 0.4.0 | MIT | [GitHub](https://github.com/Daniel-Liu-c0deb0t/triple_accel) | [crates.io](https://crates.io/crates/triple_accel) | [docs.rs](https://docs.rs/triple_accel/0.4.0) |
| eddie | Rust | 0.4.2 | MIT | [GitHub](https://github.com/thaumant/eddie) | [crates.io](https://crates.io/crates/eddie) | [docs.rs](https://docs.rs/eddie/0.4.2) |
| tantivy | Rust | 0.26.1 | MIT | [GitHub](https://github.com/quickwit-oss/tantivy) | [crates.io](https://crates.io/crates/tantivy) | [docs.rs](https://docs.rs/tantivy/0.26.1) |
| tokenizers (Hugging Face) | Rust | 0.23.1 | Apache-2.0 | [GitHub](https://github.com/huggingface/tokenizers) | [crates.io](https://crates.io/crates/tokenizers) | [docs.rs](https://docs.rs/tokenizers/0.23.1) |
| rust-stemmers | Rust | 1.2.0 | MIT / BSD-3-Clause | [GitHub](https://github.com/CurrySoftware/rust-stemmers) | [crates.io](https://crates.io/crates/rust-stemmers) | [docs.rs](https://docs.rs/rust-stemmers/1.2.0) |
| snowball_stemmers_rs | Rust | 1.0.1 | MIT | [GitHub](https://github.com/SeekStorm/snowball-stemmers-rs) | [crates.io](https://crates.io/crates/snowball_stemmers_rs) | [docs.rs](https://docs.rs/snowball_stemmers_rs/1.0.1) |
| nltk-porter | Rust | 0.1.0 | Apache-2.0 | [GitHub](https://github.com/VoiceLessQ/nltk-porter) | [crates.io](https://crates.io/crates/nltk-porter) | [docs.rs](https://docs.rs/nltk-porter/0.1.0) |
| porter-stemmer | Rust | 0.1.2 | MPL-2.0 | [GitHub](https://github.com/samgiles/porter-stemmer) | [crates.io](https://crates.io/crates/porter-stemmer) | [docs.rs](https://docs.rs/porter-stemmer/0.1.2) |
| lindera-analysis | Rust | 5.2.0 | MIT | [GitHub](https://github.com/lindera/lindera) | [crates.io](https://crates.io/crates/lindera-analysis) | [docs.rs](https://docs.rs/lindera-analysis/5.2.0) |
| sastrawi | Rust | 0.1.1 | MIT | [GitHub](https://github.com/idevoid/rust-sastrawi) | [crates.io](https://crates.io/crates/sastrawi) | [docs.rs](https://docs.rs/sastrawi/0.1.1) |
| diacritics | Rust | 0.2.2 | GPL-3.0 | [GitHub](https://github.com/YesSeri/diacritics) | [crates.io](https://crates.io/crates/diacritics) | [docs.rs](https://docs.rs/diacritics/0.2.2) |
| ordinal | Rust | 0.4.0 | MPL-2.0 | [GitHub](https://github.com/heaths/ordinal-rs) | [crates.io](https://crates.io/crates/ordinal) | [docs.rs](https://docs.rs/ordinal/0.4.0) |
| trie-rs | Rust | 0.4.2 | MIT OR Apache-2.0 | [GitHub](https://github.com/laysakura/trie-rs) | [crates.io](https://crates.io/crates/trie-rs) | [docs.rs](https://docs.rs/trie-rs/0.4.2) |
| qp-trie | Rust | 0.8.2 | MPL-2.0 | [GitHub](https://github.com/sdleffler/qp-trie-rs) | [crates.io](https://crates.io/crates/qp-trie) | [docs.rs](https://docs.rs/qp-trie/0.8.2) |
| fast_radix_trie | Rust | 1.2.0 | MIT | [GitHub](https://github.com/bluecatengineering/fast_radix_trie) | [crates.io](https://crates.io/crates/fast_radix_trie) | [docs.rs](https://docs.rs/fast_radix_trie/1.2.0) |
| fst | Rust | 0.4.7 | MIT OR Unlicense | [GitHub](https://github.com/BurntSushi/fst) | [crates.io](https://crates.io/crates/fst) | [docs.rs](https://docs.rs/fst/0.4.7) |
| rphonetic | Rust | 3.0.6 | Apache-2.0 | [GitHub](https://github.com/Dalvany/rphonetic) | [crates.io](https://crates.io/crates/rphonetic) | [docs.rs](https://docs.rs/rphonetic/3.0.6) |
| whatlang | Rust | 0.18.0 | MIT | [GitHub](https://github.com/greyblake/whatlang-rs) | [crates.io](https://crates.io/crates/whatlang) | [docs.rs](https://docs.rs/whatlang/0.18.0) |
| lingua | Rust | 1.8.0 | Apache-2.0 | [GitHub](https://github.com/pemistahl/lingua-rs) | [crates.io](https://crates.io/crates/lingua) | [docs.rs](https://docs.rs/lingua/1.8.0) |
| whichlang | Rust | 0.1.1 | MIT | [GitHub](https://github.com/quickwit-oss/whichlang) | [crates.io](https://crates.io/crates/whichlang) | [docs.rs](https://docs.rs/whichlang/0.1.1) |
| wana_kana | Rust | 5.0.0 | MIT | [GitHub](https://github.com/PSeitz/wana_kana_rust) | [crates.io](https://crates.io/crates/wana_kana) | [docs.rs](https://docs.rs/wana_kana/5.0.0) |
| postagger | Rust | 0.0.3 | Apache-2.0 | [GitHub](https://github.com/shubham0204/postagger.rs) | [crates.io](https://crates.io/crates/postagger) | [docs.rs](https://docs.rs/postagger/0.0.3) |
| rust-bert | Rust | 0.23.0 | Apache-2.0 | [GitHub](https://github.com/guillaume-be/rust-bert) | [crates.io](https://crates.io/crates/rust-bert) | [docs.rs](https://docs.rs/rust-bert/0.23.0) |
| symspell | Rust | 0.5.2 | MIT | [GitHub](https://github.com/reneklacan/symspell) | [crates.io](https://crates.io/crates/symspell) | [docs.rs](https://docs.rs/symspell/0.5.2) |
| harper-core | Rust | 2.8.0 | Apache-2.0 | [GitHub](https://github.com/Automattic/harper) | [crates.io](https://crates.io/crates/harper-core) | [docs.rs](https://docs.rs/harper-core/2.8.0) |
| spellbook | Rust | 0.4.2 | MPL-2.0 | [GitHub](https://github.com/helix-editor/spellbook) | [crates.io](https://crates.io/crates/spellbook) | [docs.rs](https://docs.rs/spellbook/0.4.2) |
| fast_symspell | Rust | 0.1.10 | MIT | no repository URL in its published metadata — re-verified via cargo's registry-cache tarball rather than taken on trust, see the Spellcheck section above | [crates.io](https://crates.io/crates/fast_symspell) | [docs.rs](https://docs.rs/fast_symspell/0.1.10) |
| tfidf (afshinm) | Rust | 0.3.0 | MIT | [GitHub](https://github.com/afshinm/tf-idf) | [crates.io](https://crates.io/crates/tfidf) | [docs.rs](https://docs.rs/tfidf/0.3.0) |
| rust-tfidf | Rust | 1.1.1 | MIT OR Apache-2.0 | [GitHub](https://github.com/ferristseng/rust-tfidf) | [crates.io](https://crates.io/crates/rust-tfidf) | [docs.rs](https://docs.rs/rust-tfidf/1.1.1) |
| smartcore | Rust | 0.6.5 | Apache-2.0 | [GitHub](https://github.com/smartcorelib/smartcore) | [crates.io](https://crates.io/crates/smartcore) | [docs.rs](https://docs.rs/smartcore/0.6.5) |
| linfa-bayes | Rust | 0.8.1 | MIT OR Apache-2.0 | [GitHub](https://github.com/rust-ml/linfa) | [crates.io](https://crates.io/crates/linfa-bayes) | [docs.rs](https://docs.rs/linfa-bayes/0.8.1) |

Full research dossier for every candidate considered — including every
crate investigated and *not* selected, and why — lives in
[`docs/COMPETITIVE_BENCHMARKS.md`](https://github.com/addlayerio/verbora/blob/main/docs/COMPETITIVE_BENCHMARKS.md).

## Reproducing these numbers

Everything on this page regenerates from a clean checkout:

```bash
# Shared inputs both sides read (run once)
python3 tools/bench-data/generate.py

cd benchmarks/competitive

# Third-party model/dictionary assets for POS tagging and spellcheck
./scripts/fetch-models.sh

# Every module's Criterion benchmarks (this page's numbers)
cargo bench --release

# Machine metadata (results/metadata.json)
./scripts/machine-metadata.sh

# Join Criterion's raw output into results/results.json + results/raw/
python3 scripts/collect-results.py distance levenshtein:verbora,strsim,rapidfuzz ...
# (see benchmarks/competitive/README.md for the full per-module command list)

# Language-detection accuracy report
cargo run --release --example language_accuracy
```

`../../scripts/competitive-benchmarks.sh` (repo root) drives all of the
above in one command. Full detail, including the exact `collect-results.py`
invocation for every module, is in
[`benchmarks/competitive/README.md`](https://github.com/addlayerio/verbora/blob/main/benchmarks/competitive/README.md)
and in each module's own dossier in
[`docs/COMPETITIVE_BENCHMARKS.md`](https://github.com/addlayerio/verbora/blob/main/docs/COMPETITIVE_BENCHMARKS.md).

## Related

- [String distance results](distance.md) — the JavaScript-library baseline
  this page's Distance section extends with real Rust competitors.
- [Benchmark method](index.md) — warmup, sample-count and regression-tracking
  conventions this page inherits.
- [Parallelism](../performance/parallelism.md) — Verbora's own
  sequential-vs-parallel numbers, thread counts disclosed, for the APIs this
  page's competitors have no equivalent to compare against.
- [`docs/PERFORMANCE_GAPS.md`](https://github.com/addlayerio/verbora/blob/main/docs/PERFORMANCE_GAPS.md) —
  every real loss on this page, with its investigated likely cause and — where
  one exists — a flagged, not-yet-implemented optimization opportunity.
- [`docs/COMPETITIVE_BENCHMARKS.md`](https://github.com/addlayerio/verbora/blob/main/docs/COMPETITIVE_BENCHMARKS.md) —
  the full research matrix: every competitor considered, selected or
  rejected, and why.
