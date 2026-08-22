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
| 614 measurements | 15 benchmarked modules, every figure traced to [`results/results.json`](https://github.com/addlayerio/verbora/blob/main/benchmarks/competitive/results/results.json) |
| 14 modules with a fair Verbora-vs-Rust comparison | every module below except POS tagging, where the comparison is withdrawn (see below) |
| 2 modules with no Rust peer at all | Phonetic Index/Neighbors and sentence analysis — documented on their own feature pages instead |
| 1 comparison withdrawn | [POS tagging](#pos-tagging) — `verbora-tagger` 0.3.0 ships no lexicon, so the competitor figures stand alone |

Start with [Phonetics](#phonetics), [Trie](#trie) or
[Language detection](#language-detection). Exact
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
| Verbora tree | Tagged [`bench-2026-08-22`](https://github.com/addlayerio/verbora/releases/tag/bench-2026-08-22). The tag is the durable anchor: a branch commit does not survive a squash merge, and this page once cited a hash that resolves nowhere. Within it, every module except Phonetics was measured at `80c302b` and Phonetics re-measured at `0313eae` — one commit later, touching `scripts/` and `site/` only, no `crates/` change, so this is the same phonetics code measured a second time, not a different implementation. Crate versions: `verbora-tagger` 0.3.0, `verbora-wordnet` 0.3.0, every other crate measured on this page 0.2.0 |
| Datasets | Shared word/name/pair lists from `benches/data/*.json` (`tools/bench-data/generate.py`, one generator read by every implementation); the 13-language, 4-tier UDHR corpus for language-detection accuracy (sourced below); the 2,438-word AFINN-111/AFINN-165 intersection for sentiment; a real Princeton WordNet 3.1 `dict/` distribution for WordNet |
| Warmup | Criterion's own warmup phase before every measured sample (400 ms–1 s per group; see below) |
| Samples | Criterion's default 100 per benchmark, reduced for the most expensive groups: 30 for language/script detection and transliteration, 20 for spellcheck batch-correction and distance-2 corrections, 15 for POS-tagging cold start (`rust-bert`'s model load), 10 for WordNet's `open`/`cold` groups (real-file I/O) |
| Metric | **Median**, per Criterion's own robust-statistics estimate — not mean, per this project's own `PRIMARY METRIC` policy |
| Threads | **1 (single-threaded)** for every benchmark on this page — no parallel API is exercised anywhere in this audit; see [Thread counts](#thread-counts) |
| Source | [`benchmarks/competitive/rust-competitors/benches/*.rs`](https://github.com/addlayerio/verbora/tree/main/benchmarks/competitive/rust-competitors/benches) (one file per module), raw Criterion output under [`benchmarks/competitive/results/raw/`](https://github.com/addlayerio/verbora/tree/main/benchmarks/competitive/results/raw), joined into [`results/results.json`](https://github.com/addlayerio/verbora/blob/main/benchmarks/competitive/results/results.json) |
| Date | Every measurement on this page is stamped `2026-08-22`. Both commits inside the tag were cut the same day and differ only outside `crates/`, so this is one campaign's figures, not a mixed-vintage set |

Every number on this page is read directly from `results.json`'s saved
`median_ns` values; the relative-speedup figures are computed from those same
values at page-generation time — none is retyped from memory or rounded
inconsistently. See [Reproducing these numbers](#reproducing-these-numbers)
for the exact commands that regenerate all of it from a clean checkout.

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
  all, and the table says so — see [Spellcheck](#spellcheck) for the cases
  this applies to.
- A figure is published only while the code and the measurement behind it
  are current. Nothing on this page is filled in with an estimate, an
  interpolation, or an inference from a prior run's direction of change.

## How these numbers were audited

This page's structure — which comparisons are fair, which are narrowed, and
why — was cleared by an independent fairness audit that read every benchmark
file and correctness test in this workspace, re-ran the Rust suite and the
language-accuracy report itself, and cross-referenced every disclosed loss
against
[`docs/PERFORMANCE_GAPS.md`](https://github.com/addlayerio/verbora/blob/main/docs/PERFORMANCE_GAPS.md).
Its verdict: every comparison retained on this page is **FAIR** — same
input, genuinely equivalent (or honestly narrowed and labeled) semantics,
`black_box` on every call's input and output, correctness-before-performance
tests that were run and passed, and version pins of `=x.y.z` on every
third-party crate. Items the audit flagged as borderline rather than unfair
are called out inline where they occur: the normalizers' accented-input case
([Normalizers](#normalizers)) and the `WhatlangDetector` wrapper-overhead
check ([Language detection](#language-detection)), neither of which is a
ranked "X beats Y" comparison in the first place.

The byte-exact phonetics encoder table in [Phonetics](#phonetics) carries a
stronger correctness check than the rest of the page: byte-exact output
equality with the competitor itself, asserted in
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
verified independently line-by-line, then adversarially fuzz- and
mutation-tested against the trusted scalar DP before being trusted for
anything). The kernels' pattern-match (Peq) tables are flat/packed bit
tables rather than a hash map, and the single-word gate covers 1–64 units.
Bit-parallelism extends beyond plain Levenshtein too: restricted-Damerau/OSA
kernels (Hyyrö's 2003 transposition extension of Myers, single-word and
multi-word block, gated to unit costs), and Jaro/Jaro-Winkler match-flagging
kernels in Verbora's own greedy orientation. Unrestricted Damerau-Levenshtein
has no bit-vector formulation, so it runs Zhao–Sahni's linear-space
algorithm instead of a full cost+parent matrix: three rows and a
last-occurrence table, kept on the stack for short operands, with a
table-free stack matrix for byte operands of at most 8 units. Common-prefix/
suffix trimming runs ahead of the unrestricted-Damerau and OSA kernels alike.
Every kernel is parity-verified by differential tests against the retained
scalar implementations, plus an independent adversarial audit with mutation
testing, before being trusted; Hamming has no bit-parallel kernel.

<div class="callout callout-good">
<strong>Plain Levenshtein beats all six Rust competitors at every size
from 4 to 1024 characters</strong> (Verbora is 1.15× faster than the closest
competitor at 1024 characters, 1.83× at 16). Restricted Damerau/OSA beats
every competitor at every size, and Jaro/Jaro-Winkler wins every size too.
Unrestricted Damerau-Levenshtein is the one metric in this section with a
genuine size-dependent crossover — see its own section below. See
<a href="https://github.com/addlayerio/verbora/blob/main/docs/PERFORMANCE_GAPS.md#26-levenshtein-distance--verbora-vs-stringmetrics-triple_accel-and-editdistancek-rust">PERFORMANCE_GAPS.md
entry 26</a> for the mechanism and verification story these kernels build
on.
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
| Verbora | 0.2.0 | Rust | 27.72 µs | 36.1K/s | **1.00×** |
| rapidfuzz | 0.5.0 | Rust | 31.85 µs | 31.4K/s | 1.15× slower |
| strsim | 0.11.1 | Rust | 648.83 µs | 1.5K/s | 23.41× slower |
| stringmetrics | 2.2.2 | Rust | 973.08 µs | 1.0K/s | 35.10× slower |

**Random pairs** — two independently generated strings, so the edit script is
long and every implementation does its full work:

| Input | Verbora | rapidfuzz | strsim | stringmetrics |
|---:|--:|--:|--:|--:|
| 4 | **10.9 ns** | 38.1 ns | 18.3 ns | 25.8 ns |
| 16 | **41.2 ns** | 75.5 ns | 182.7 ns | 176.5 ns |
| 64 | **160.8 ns** | 258.6 ns | 2.89 µs | 3.13 µs |
| 256 | **2.20 µs** | 3.37 µs | 41.38 µs | 56.62 µs |
| 1024 | **27.72 µs** | 31.85 µs | 648.83 µs | 973.08 µs |

**Near-identical pairs** (`d = 1`) — the shape spell-checking and deduplication
actually feed a distance metric, and the one where the implementations diverge
most:

| Input | Verbora | rapidfuzz | strsim | stringmetrics |
|---:|--:|--:|--:|--:|
| 4 | **10.5 ns** | 21.0 ns | 18.1 ns | 16.3 ns |
| 16 | **11.7 ns** | 40.1 ns | 177.3 ns | 19.5 ns |
| 64 | **13.7 ns** | 138.9 ns | 2.90 µs | 48.7 ns |
| 256 | **20.0 ns** | 277.1 ns | 42.43 µs | 166.5 ns |
| 1024 | **41.5 ns** | 868.8 ns | 660.07 µs | 593.6 ns |

Verbora is the **fastest implementation at every size and both shapes** —
against all four char-indexed competitors here and the two byte-level ones
below. But the two tables answer different questions, and the result only has
its real shape if you read both.

On random pairs the lead over `rapidfuzz` — the closest competitor and the
only other bit-parallel implementation here — runs **3.50× at 4 characters,
1.83× at 16, 1.61× at 64, 1.53× at 256 and 1.15× at 1024**. It is widest at
small sizes, where the flat `[u64; 256]` Peq tables and the 1–64-unit
single-word gate keep per-call setup low, and narrowest at 1024, where both
sides run the same class of multi-word block algorithm. Against `strsim` the
win is 1.68× (4) up to 23.41× (1024), against `stringmetrics` 2.37× up to
35.10× — neither scalar design has a bit-vector formulation to close with.

On near-identical pairs the margin moves the other way: it *widens* with
length, from roughly 1.5× to 16× against the best char-indexed competitor at
each size. Common prefixes and suffixes are trimmed before the kernel runs,
so Verbora's cost rises only from 10.5 ns to 41.5 ns across a 256-fold
increase in input length, while `strsim` — which evaluates the full matrix
regardless of how similar the inputs are — rises to 660 µs, a factor of
roughly 16,000.

The random rows are intentionally retained as their own workload. They do
not show the common-affix optimization: independent strings have almost no
affix to remove. The same competitive harness also measures a 1,024-unit
pair with one central substitution (`1024-near`):

| Library | Median (`1024-near`) | Relative to Verbora |
|---|---:|---:|
| Verbora | **41.5 ns** | **1.00×** |
| `editdistancek` | 192.3 ns | 4.64× slower |
| `stringmetrics` | 593.6 ns | 14.32× slower |
| `rapidfuzz` | 868.8 ns | 20.96× slower |
| `triple_accel` | 525.17 µs | 12,655× slower |
| `strsim` | 660.07 µs | 15,904× slower |

Two different designs sit behind those numbers, and the spread separates them
cleanly. `editdistancek` is the only competitor built for this shape — its
bounded-edit algorithm stops early when the distance is small, which is why it
lands two orders of magnitude ahead of the full-matrix implementations.
`triple_accel` and `strsim` evaluate the whole matrix whether the inputs differ
by one character or by a thousand, so similarity buys them nothing.

Verbora reaches 41.5 ns by removing the common prefix and suffix before the
kernel sees anything: on a 1,024-unit pair differing in one position, the
kernel runs over what is left, not over the pair. The cost is therefore set by
the size of the difference rather than the size of the input — 10.5 ns at 4
units, 41.5 ns at 1,024.

#### Competitive shape suite

The competitive harness also has a dedicated
`levenshtein_edge_shapes` group. It uses the exact same lowercase-ASCII,
unit-cost inputs for every implementation, so both char-indexed and
byte-indexed competitors remain directly comparable. It keeps the
shape-sensitive results separate from the random-size table above:

| Case | Operands | Verbora | Fastest competitor | Slowest competitor |
|---|---|--:|--:|--:|
| `near/1024` | 1,024 vs. 1,024; one central substitution | **42.2 ns** | editdistancek, 186.0 ns (4.40×) | strsim, 626.73 µs (14,834×) |
| `disjoint/1024` | 1,024 vs. 1,024; no character overlap | **1.20 µs** | rapidfuzz, 28.45 µs (23.70×) | editdistancek, 1.40 ms (1,167×) |
| `late-overlap/65x10000` | 65 vs. 10,000; overlap only at the end | **1.94 µs** | rapidfuzz, 44.72 µs (23.00×) | editdistancek, 51.12 ms (26,296×) |

Verbora wins all three shapes against all five competitors. This is
deliberately a separate group, rather than an extra median in the table
above: random pairs, near pairs and disjoint alphabets exercise different
valid algorithmic shortcuts, and blending them would hide that trade-off.
The accompanying correctness test checks that all six implementations return
the same distance on every timed shape. Reproduce it with:

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

<div class="callout callout-note">
<strong>A genuine size-dependent crossover on random pairs, and a clean win
on near-identical pairs.</strong> No bit-vector formulation exists for the
unrestricted recurrence, so Verbora's linear-space scalar algorithm competes
directly against `strsim` and `rapidfuzz`'s own scalar implementations —
and the three are close enough on random input that the ranking flips with
size.
</div>

| Input | Verbora | rapidfuzz | strsim |
|---:|--:|--:|--:|
| 4 | **27.6 ns** | 74.1 ns | 60.3 ns |
| 16 | **260.3 ns** | 522.5 ns | 510.1 ns |
| 64 | **4.63 µs** | 8.17 µs | 7.76 µs |
| 256 | 134.80 µs | 135.57 µs | **133.86 µs** |
| 1024 | 2.50 ms | 2.22 ms | **2.03 ms** |

Verbora wins at 4, 16 and 64 characters (1.8×–2.7× faster than the closer of
the two competitors), is essentially tied with `strsim` at 256 (1.007×
slower — within this run's noise floor), and loses at 1024 characters
(1.23× slower than `strsim`, 1.13× slower than `rapidfuzz`). On the
near-identical shape, where common-prefix/suffix trimming collapses the
work to the size of the actual edit rather than the size of the operand,
Verbora wins outright at every size, including 1024 (**47.0 ns** vs.
`rapidfuzz`'s 852.2 ns, 18.1× faster, and `strsim`'s 2.08 ms, over 44,000×
slower on this shape since it evaluates the full matrix regardless of
similarity).

#### Damerau–Levenshtein (restricted / OSA)

| Library | Version | Language | Time (median, 1024 chars) | Throughput | Relative |
|---|---|---|---:|---:|---:|
| Verbora | 0.2.0 | Rust | 35.97 µs | 27.8K/s | **1.00×** |
| rapidfuzz | 0.5.0 | Rust | 43.28 µs | 23.1K/s | 1.20× slower |
| strsim | 0.11.1 | Rust | 2.39 ms | 419.3/s | 66.32× slower |

| Input size | Verbora | rapidfuzz | strsim |
|---:|--:|--:|--:|
| 4 | 16.6 ns | 27.9 ns | 69.4 ns |
| 16 | 48.8 ns | 78.9 ns | 306.2 ns |
| 64 | 179.8 ns | 255.2 ns | 4.35 µs |
| 256 | 2.59 µs | 3.02 µs | 138.38 µs |
| 1024 | 35.97 µs | 43.28 µs | 2.39 ms |

Verbora is the **fastest at every size**. Restricted Damerau's
one-transposition-back reach needs more state than plain Levenshtein's
two-row shape, so it gets its own bit-parallel kernels implementing Hyyrö's
2003 transposition extension of Myers' algorithm — a single-word kernel
plus a multi-word block generalisation, gated to unit-cost options, with
the scalar three-row DP retained for every non-unit-cost call and as the
differential-test oracle. Against `rapidfuzz`, the only other bit-parallel
OSA here: **1.69× faster at 4 characters, 1.62× at 16, 1.42× at 64, 1.17×
at 256, 1.20× at 1024**. Against `strsim`'s scalar implementation the
margin runs 4.19× (4 chars) up to 66.3× (1024). `triple_accel`'s byte-level
`rdamerau` is covered in the byte-level subsection below.

#### Hamming

| Library | Version | Language | Time (median, 1024 chars) | Throughput | Relative |
|---|---|---|---:|---:|---:|
| Verbora | 0.2.0 | Rust | 22.6 ns | 44.3M/s | **1.00×** |
| stringmetrics | 2.2.2 | Rust | 580.2 ns | 1.72M/s | 25.67× slower |
| strsim | 0.11.1 | Rust | 580.4 ns | 1.72M/s | 25.68× slower |
| rapidfuzz | 0.5.0 | Rust | 620.1 ns | 1.61M/s | 27.44× slower |

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
| 4 | 3.8 ns | 5.7 ns | 4.6 ns | **2.4 ns** |
| 16 | **3.8 ns** | 18.2 ns | 13.4 ns | 16.1 ns |
| 64 | **4.6 ns** | 50.4 ns | 43.4 ns | 42.1 ns |
| 256 | **7.4 ns** | 162.5 ns | 143.4 ns | 142.6 ns |
| 1024 | **22.6 ns** | 620.1 ns | 580.4 ns | 580.2 ns |

#### Jaro / Jaro–Winkler

| Library | Version | Language | Time (median, 1024 chars) | Throughput | Relative |
|---|---|---|---:|---:|---:|
| Verbora | 0.2.0 | Rust | 8.55 µs | 116.9K/s | **1.00×** |
| rapidfuzz | 0.5.0 | Rust | 15.35 µs | 65.2K/s | 1.79× slower |
| strsim | 0.11.1 | Rust | 332.31 µs | 3.0K/s | 38.85× slower |

Verbora **beats both competitors at every size**. `rapidfuzz`'s
Jaro/Jaro-Winkler is bit-parallelized
(`rapidfuzz-0.5.0/src/distance/jaro.rs`); Verbora matches that with its own
bit-parallel match-flagging kernels (word-sized plus multi-word block) in
its own greedy match orientation, with the scalar loop retained for inputs
of at most 16 units and as the differential-test oracle, and the
fractional-transposition semantics preserved exactly. Against `rapidfuzz`
on Jaro-Winkler: **2.09× faster at 4 characters, 1.14× at 16, 2.04× at 64,
1.17× at 256, 1.79× at 1024.**

| Input size | Verbora | rapidfuzz | strsim |
|---:|--:|--:|--:|
| 4 | 13.0 ns | 32.3 ns | 27.2 ns |
| 16 | 74.6 ns | 84.9 ns | 155.6 ns |
| 64 | 136.8 ns | 278.9 ns | 1.46 µs |
| 256 | 1.86 µs | 2.18 µs | 24.10 µs |
| 1024 | 8.55 µs | 15.35 µs | 332.31 µs |

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
| Verbora | 0.2.0 | 27.72 µs | **1.00×** |
| triple_accel | 0.4.0 | 527.10 µs | 19.01× slower |
| editdistancek | 1.0.2 | 1.07 ms | 38.46× slower |

Random pairs:

| Input size | Verbora | triple_accel | editdistancek |
|---:|--:|--:|--:|
| 4 | **10.9 ns** | 102.5 ns | 46.3 ns |
| 16 | **41.2 ns** | 339.2 ns | 370.0 ns |
| 64 | **160.8 ns** | 1.67 µs | 4.93 µs |
| 256 | **2.20 µs** | 35.43 µs | 78.63 µs |
| 1024 | **27.72 µs** | 527.10 µs | 1.07 ms |

Near-identical pairs (`d = 1`), where `editdistancek`'s banded algorithm is at
its strongest and `triple_accel`'s SIMD full-matrix pass gains nothing:

| Input size | Verbora | triple_accel | editdistancek |
|---:|--:|--:|--:|
| 4 | **10.5 ns** | 101.9 ns | 22.3 ns |
| 16 | **11.7 ns** | 336.3 ns | 23.5 ns |
| 64 | **13.7 ns** | 1.64 µs | 73.5 ns |
| 256 | **20.0 ns** | 37.33 µs | 104.9 ns |
| 1024 | **41.5 ns** | 525.17 µs | 192.3 ns |

Verbora wins **at every size and both shapes, outright**. The near-identical
rows are worth reading beside the random ones, because they separate the
two competitors rather than confirming the first table. `triple_accel` is
unmoved by similarity — it runs the same vectorized full matrix either way,
so 1024 costs it ~525 µs regardless — while `editdistancek`'s banded
algorithm is built for exactly this case and drops to 192.3 ns. Verbora is
faster still at 41.5 ns, but against a competitor doing the right thing
rather than one doing the wrong thing quickly.

**Restricted Damerau-Levenshtein** — Verbora's OSA bit-parallel kernels win
**at every size**, against `triple_accel`'s byte-level `rdamerau`
(**35.97 µs** vs. `triple_accel`'s **782.29 µs** at 1024 characters, 21.75×
faster).

**Hamming** — the one byte-level case where Verbora loses: `triple_accel`'s
Hamming is a vectorized XOR-and-popcount over the whole string with no
data-dependent branching, versus Verbora's scalar per-position comparison
loop. `triple_accel` is **2.05× faster at 1024 characters** (11.0 ns vs.
Verbora's 22.6 ns), a gap that widens with length: Verbora is actually
faster at 16 characters (3.8 ns vs. 4.5 ns) and only slightly behind at 4
and 64, before `triple_accel`'s vectorization advantage compounds at longer
input.

---

### Tokenizers

`WordTokenizer` and `SentenceTokenizer` both perform full [UAX #29
segmentation](../features/tokenizers), and both are now measured against
their Rust rivals on that current implementation.

#### Word tokenization — a genuine size-dependent crossover

`WordTokenizer` against tantivy 0.26.1's `SimpleTokenizer` and Hugging Face
`tokenizers` 0.23.1's `Whitespace` pre-tokenizer, called in isolation (never
through HF's full BPE pipeline). The workload is narrowed to punctuation-free
ASCII text, and boundary-exact agreement (not just token-count agreement) is
proved against the current implementation in
[`tests/tokenizers_correctness.rs`](https://github.com/addlayerio/verbora/blob/main/benchmarks/competitive/rust-competitors/tests/tokenizers_correctness.rs)
before any timing is trusted. A `verbora-lazy` variant (an iterator that
yields tokens without collecting them into a `Vec` first) runs alongside
the default, allocating `Vec`-returning `WordTokenizer`.

| Library | Version | Language | Time (median, 311,023 B) | Throughput | Relative |
|---|---|---|---:|---:|---:|
| tantivy | 0.26.1 | Rust | 488.18 µs | 2.0K/s | **1.00×** |
| Verbora | 0.2.0 | Rust | 488.67 µs | 2.0K/s | 1.00× slower |
| Verbora (lazy) | 0.2.0 | Rust | 491.36 µs | 2.0K/s | 1.01× slower |
| huggingface | 0.23.1 | Rust | 8.70 ms | 114.9/s | 17.82× slower |

| Input (bytes) | Verbora | Verbora (lazy) | tantivy | huggingface |
|---:|--:|--:|--:|--:|
| 123 | 211.9 ns | 159.9 ns | **128.3 ns** | 2.15 µs |
| 566 | 790.9 ns | 631.2 ns | **563.6 ns** | 8.94 µs |
| 1,187 | 1.56 µs | 1.20 µs | **1.15 µs** | 18.63 µs |
| 4,751 | 5.83 µs | 4.90 µs | **4.64 µs** | 78.48 µs |
| 9,709 | 12.57 µs | 11.86 µs | **11.16 µs** | 169.84 µs |
| 38,764 | 60.04 µs | **59.32 µs** | 60.52 µs | 981.43 µs |
| 77,684 | 120.95 µs | **119.16 µs** | 127.50 µs | 1.87 ms |
| 311,023 | 488.67 µs | 491.36 µs | **488.18 µs** | 8.70 ms |

`tantivy`'s `SimpleTokenizer` wins at small-to-medium input (up to ~1.4× at
the shortest text), and Verbora's lazy iterator overtakes it from roughly
38,000 bytes up, with the two effectively tied at 311,023 bytes. Both beat
Hugging Face's `Whitespace` pre-tokenizer by more than an order of magnitude
at every size — that pre-tokenizer is not the crate's fast path; it exists
as one stage of a full BPE pipeline, and calling it standalone times
overhead a real HF user would not pay in isolation. Full UAX #29 word
segmentation does strictly more work per boundary than a character-class
scan, so this crossover is the honest cost of that correctness: Verbora
wins on long input despite doing more per byte, and loses on short input
where per-call setup dominates. See [`PERFORMANCE_GAPS.md`
entry 4](https://github.com/addlayerio/verbora/blob/main/docs/PERFORMANCE_GAPS.md#4-word-tokenization--verbora-vs-tantivysimpletokenizer-rust-a-size-dependent-crossover-not-a-one-sided-loss).

#### Sentence tokenization

`SentenceTokenizer` against [segtok](https://github.com/xamgore/segtok)
0.1.5, on the narrowed plain-declarative-sentence domain both sides agree on
(no abbreviations/URIs/digits/quotes/brackets), with boundary-exact agreement
proven in the same correctness test before any timing is trusted.

| Library | Version | Language | Time (median, 474,752 B) | Throughput | Relative |
|---|---|---|---:|---:|---:|
| Verbora (lazy) | 0.2.0 | Rust | 5.39 ms | 185.7/s | **1.00×** |
| Verbora | 0.2.0 | Rust | 5.42 ms | 184.5/s | 1.01× slower |
| segtok | 0.1.5 | Rust | 104.91 ms | 9.5/s | 19.48× slower |

Verbora wins at **every size measured**, by roughly 14.7×–20.8× depending
on input length, whether or not the lazy iterator is used. `SentenceTokenizer`
is built directly on `split_sentence_bound_indices()` with no placeholder
mask, no unmask pass and no trimming.

The `unicode-segmentation` pairing does not appear here, and for a reason
worth stating plainly: `SentenceTokenizer` is built on `unicode-segmentation`
and `WordTokenizer` *is* `str::unicode_words()`, so timing either against its
own dependency measures Verbora against the primitive it delegates to, which
is a wrapper-overhead question rather than a competitive one. Those rows live
in the `*_wrapper_overhead` groups below instead, and are never reported as
Verbora beating or losing to `unicode-segmentation`.

#### Wrapper overhead — not ranked comparisons

| Group | Verbora (default) | Verbora (lazy) | Primitive(s) |
|---|--:|--:|--:|
| Word tokenization, 311,023 B | 526.31 µs | **486.69 µs** | `unicode-words` 492.65 µs, `unicode-bounds` 2.46 ms |
| Sentence tokenization, 474,752 B | 5.42 ms | **5.41 ms** | `unicode-bounds` 5.48 ms, `unicode-sentences` 5.57 ms |

Both wrappers cost roughly 0–8% over the bare `unicode-segmentation` call
they make, well within this run's measurement noise at most sizes — the
default (`Vec`-collecting) `WordTokenizer` pays the most at short input
(up to ~1.5× the lazy iterator's cost at 123 bytes, from the extra
allocation), converging with the lazy variant as input grows. Neither row is
a rival-implementation comparison: `unicode-words`/`unicode-bounds`/
`unicode-sentences` are the exact primitives `WordTokenizer`/
`SentenceTokenizer` call.

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

<div class="callout callout-good">
<strong>Verbora wins both arities.</strong> <code>verbora-ngrams</code>'s
<code>ngrams</code> yields borrowed sub-slices of the caller's own slice,
with nothing allocated per window.
</div>

| Library | Version | Language | Time (median, bigrams, 20,000 words) | Throughput | Relative |
|---|---|---|---:|---:|---:|
| Verbora | 0.2.0 | Rust | 7.87 ms | 127.0/s | **1.00×** |
| ngrammatic | 0.7.0 | Rust | 10.32 ms | 96.9/s | 1.31× slower |

| Library | Version | Language | Time (median, trigrams, 20,000 words) | Throughput | Relative |
|---|---|---|---:|---:|---:|
| Verbora | 0.2.0 | Rust | 10.12 ms | 98.8/s | **1.00×** |
| ngrammatic | 0.7.0 | Rust | 11.16 ms | 89.6/s | 1.10× slower |

Verbora wins bigram generation by 1.31× and trigram generation by 1.10× — a
clean win at both arities. Both implementations do the same conceptual work — pad, slide a window,
fold into a `(gram, count)` map — over the same input, so the residual
reflects each side's small-string-accumulation strategy rather than an
algorithmic difference: `ngrammatic` accumulates directly into a
`HashMap<SmolStr, usize>`, whose small-string optimization skips a heap
allocation for any gram that fits inline, while Verbora's borrowed-window
`ngrams()` engine now avoids building an owned `Vec`/`String` per gram
altogether.

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

Verbora's suffix matching is the Snowball runtime's own
`find_among`/`find_among_b` binary search
(`crates/verbora-stemmers/src/among.rs`): a table sorted by
reversed scalar sequence, `common_i`/`common_j`
prefix tracking so no unit is compared twice, and
`substring_i`-style links so one search replaces a whole guarded
else-if chain. Ten of the eleven language modules route through it —
`de`, `en`, `es`, `fr`,
`it`, `nl`, `no`, `pt`,
`ru`, `sv`. That is the same algorithm both competitors
get from the official Snowball compiler.

| Language | Verbora (median, 1024 chars) | rust-stemmers | snowball_stemmers_rs | Verbora advantage (best competitor) |
|---|--:|--:|--:|--:|
| German (`de`) | **97.36 µs** | 137.30 µs | 187.35 µs | 1.41× |
| Spanish (`es`) | **73.80 µs** | 130.04 µs | 94.02 µs | 1.27× |
| French (`fr`) | **109.19 µs** | 192.25 µs | 156.98 µs | 1.44× |
| Italian (`it`) | **94.49 µs** | 228.51 µs | 196.43 µs | 2.08× |
| Dutch (`nl`) | **93.21 µs** | 236.45 µs | 97.52 µs | 1.05× |
| Norwegian (`no`) | 45.02 µs | 48.61 µs | **33.51 µs** | 1.34× slower |
| Portuguese (`pt`) | 115.05 µs | 174.15 µs | **94.45 µs** | 1.22× slower |
| Russian (`ru`) | 122.61 µs | **82.82 µs** | 81.02 µs | 1.51× slower |
| Swedish (`sv`) | 53.53 µs | 52.27 µs | **41.25 µs** | 1.30× slower |

Verbora wins five of nine languages outright (German, Spanish, French,
Italian, Dutch) and loses four (Norwegian, Portuguese, Russian, Swedish) —
to `snowball_stemmers_rs` in three of those four cases and to
`rust-stemmers` in the fourth (Russian). Neither competitor is uniformly
faster than the other across the four Verbora loses; this is a genuine
per-language split rather than a single systematic gap.

#### `snowball_stemmers_rs` — a second, independently-generated Snowball port

Languages are never averaged together, so this is a second, independent data
point rather than a repeat of the `rust-stemmers` comparison. What it
establishes without reference to any measurement is a correctness finding:
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
`among.rs` like the other ten.

| Library | Version | Language | Time (median, 1024 chars) | Throughput | Relative |
|---|---|---|---:|---:|---:|
| Verbora | 0.2.0 | Rust | 211.44 µs | 4.7K/s | **1.00×** |
| porter-stemmer | 0.1.2 | Rust | 318.60 µs | 3.1K/s | 1.51× slower |
| nltk-porter | 0.1.0 | Rust | 1.87 ms | 534.6/s | 8.85× slower |

Verbora wins at **every size measured** against both competitors, by
1.10×–1.50× against `porter-stemmer` and 7.82×–10.54× against `nltk-porter`.

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
| 4 | 26.7 ns | 454.4 ns | **Verbora, 17.00×** |
| 1024 | 8.29 µs | 138.25 µs | **Verbora, 16.68×** |

A clean, decisive win at every size, 15.8×–20.3× depending on length —
Verbora's algorithm borrows and allocates nothing, while `lindera-analysis`'s
`Vec<Token>`-batch filter API always allocates at least the `Vec`, on top of
running through a full dictionary-backed tokenizer pipeline Verbora's
purpose-built stemmer doesn't need.

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
| 4 | 1.08 µs | 879.9 ns | sastrawi, 1.23× |
| 1024 | 631.63 µs | 617.35 µs | sastrawi, 1.02× |

<div class="callout callout-note">
A real, narrow loss on per-word time (1.02×–1.15× across the sizes measured).
<code>sastrawi</code>'s own
one-time <code>Dictionary::new()</code> + <code>Stemmer::new()</code>
construction cost is real but paid once; Verbora's <code>StemmerId::new()</code>
is a zero-sized unit struct backed entirely by compiled-in static data,
needing no runtime construction at all — a trade-off the per-word numbers
above don't capture.
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

#### Pure-ASCII input

| Library | Version | Language | Time (median, 1024 B) | Throughput | Relative |
|---|---|---|---:|---:|---:|
| Verbora | 0.2.0 | Rust | 39.6 ns | 25.27M/s | **1.00×** |
| diacritics | 0.2.2 | Rust | 10.42 µs | 96.0K/s | 263.28× slower |

| Input size | Verbora | diacritics | Verbora vs. diacritics |
|---:|--:|--:|--:|
| 4 | 2.9 ns | 95.5 ns | **32.78× faster** |
| 16 | 6.3 ns | 170.5 ns | **27.20× faster** |
| 64 | 8.9 ns | 700.0 ns | **78.44× faster** |
| 256 | 23.4 ns | 2.66 µs | **113.92× faster** |
| 1024 | 39.6 ns | 10.42 µs | **263.28× faster** |

Verbora's one-line `s.is_ascii()` fast path returns `Cow::Borrowed`
immediately — no scan, no allocation — while `diacritics::remove_diacritics`
folds through its `char` match unconditionally, even on input it will not
change.

#### Accented (working) input

| Input size | Verbora | diacritics | Verbora vs. diacritics |
|---:|--:|--:|--:|
| 4 | 3.05 µs | 492.1 ns | 6.21× slower |
| 16 | 12.90 µs | 1.89 µs | 6.83× slower |
| 64 | 48.15 µs | 7.91 µs | 6.09× slower |
| 256 | 201.98 µs | 31.60 µs | 6.39× slower |
| 1024 | 746.16 µs | 121.39 µs | 6.15× slower |

<div class="callout callout-warn">
<strong>A real, disclosed loss on accented input, consistent across every
size measured.</strong> Verbora's <code>remove_diacritics</code> decomposes
accented characters (its ASCII fast path above is the payoff of that design
on input that needs no decomposition); <code>diacritics</code> uses a direct
table lookup with no decomposition step, which is faster whenever the input
actually contains accented characters to remove. See
<a href="https://github.com/addlayerio/verbora/blob/main/docs/PERFORMANCE_GAPS.md">PERFORMANCE_GAPS.md</a>
for this module's tracked entries.
</div>

---

### Inflectors

`OrdinalInflector::nth` — English ordinal suffixing (1st/2nd/3rd/…) —
against [ordinal](https://github.com/heaths/ordinal-rs) 0.4.0
(`docs/COMPETITIVE_BENCHMARKS.md`
[§1.5](https://github.com/addlayerio/verbora/blob/main/docs/COMPETITIVE_BENCHMARKS.md#15-inflectors)),
the single full-equivalence (`Yes`) row in the whole Inflectors group. Two real
divergences were found and excluded from the benchmarked domain before any
timing was trusted, in
[`tests/inflectors_correctness.rs`](https://github.com/addlayerio/verbora/blob/main/benchmarks/competitive/rust-competitors/tests/inflectors_correctness.rs):
negative integers (different rounding conventions), and a real bug in
`ordinal` 0.4.0 itself: its teens exception uses `n % 20` where it needs
`n % 100`, misformatting 12% of non-negative integers
(`31.to_ordinal_string()` returns `"31th"`, not `"31st"`). The benchmarked
domain verifiably avoids every affected value.

| Library | Version | Language | Time (median, 1024-int batch) | Throughput | Relative |
|---|---|---|---:|---:|---:|
| Verbora | 0.2.0 | Rust | 14.56 µs | 68.7K/s | **1.00×** |
| ordinal | 0.4.0 | Rust | 25.89 µs | 38.6K/s | 1.78× slower |

| Input size | Verbora | ordinal | Verbora vs. ordinal |
|---:|--:|--:|--:|
| 4 | 53.7 ns | 97.7 ns | **1.82× faster** |
| 16 | 223.2 ns | 419.4 ns | **1.88× faster** |
| 64 | 838.7 ns | 1.58 µs | **1.88× faster** |
| 256 | 3.72 µs | 6.94 µs | **1.87× faster** |
| 1024 | 14.56 µs | 25.89 µs | **1.78× faster** |

A flat ~1.8× regardless of batch size: both sides are `O(1)` per
integer, so this is a genuine constant-factor difference — `ordinal` formats
through `Display`/`format!`, Verbora writes digits directly into a
pre-sized buffer.

`NounInflector` (`pluralize`/`singularize`) against
[inflector](https://crates.io/crates/Inflector) 0.11.4 and
[pluralizer](https://crates.io/crates/pluralizer) 0.5.0:

| Operation | Verbora (median, 1024 words) | inflector | pluralizer |
|---|--:|--:|--:|
| pluralize | **226.97 µs** | 463.95 µs (2.04× slower) | 603.21 µs (2.66× slower) |
| singularize | **311.85 µs** | 735.69 µs (2.36× slower) | 701.67 µs (2.25× slower) |

Verbora wins both operations against both competitors at every size
measured (4 to 1024 words), by roughly 1.3×–2.7× depending on operation and
size.

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
`insert_all` clamps the reservation it takes from an iterator's `size_hint`
at 4,096 nodes and reaches the rest by amortised growth — a `size_hint` is a
hint, not a bound, and an iterator that overstates it could otherwise turn a
bulk load into an unbounded allocation.

| Library | Version | Language | Time (median, 20,000-word build, random) | Throughput | Relative |
|---|---|---|---:|---:|---:|
| Verbora | 0.2.0 | Rust | 2.13 ms | 469.0/s | **1.00×** |
| qp-trie | 0.8.2 | Rust | 3.05 ms | 328.1/s | 1.43× slower |
| trie-rs | 0.4.2 | Rust | 10.95 ms | 91.3/s | 5.14× slower |

Verbora **wins build** against both competitors (1.43×–5.14× at random keys;
similar margins at prefix-heavy and sorted-key shapes) and **wins every
operation against `trie-rs`** by roughly two orders of magnitude. Against
`qp-trie` specifically, the read path inverts:

| Operation | Verbora | qp-trie | Verdict |
|---|--:|--:|---|
| `contains` (hit, 20K words) | 469.74 µs | 855.31 µs | Verbora 1.82× faster |
| `contains` (miss, 20K words) | 417.62 µs | 772.24 µs | Verbora 1.85× faster |
| `common_prefix_search` | 256.74 µs | — (not implemented) | — |
| `predictive_search` (1-char prefix) | 286.3 ns | 118.76 µs | Verbora 414.9× faster |
| `predictive_search` (empty prefix, all 20K) | 1.15 ms | 123.88 µs | qp-trie 9.28× faster |

<div class="callout callout-note">
<strong>A split read path.</strong>
Verbora's arena trie wins <code>contains</code> and single-character
<code>predictive_search</code> outright against <code>qp-trie</code>; only
full-corpus enumeration (empty-prefix <code>predictive_search</code>)
favours <code>qp-trie</code>'s path-compressed radix structure, which stores
each key whole at its leaf and pays no per-scalar reconstruction cost when
enumerating everything. See
<a href="https://github.com/addlayerio/verbora/blob/main/docs/PERFORMANCE_GAPS.md#2-trie-lookup-and-prefix-enumeration--verbora-vs-qp-trie-rust">PERFORMANCE_GAPS.md
entry 2</a> for the underlying mechanism.
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
| `build` (random) | **2.13 ms** | 2.20 ms | Verbora 1.03× faster |
| `contains` (hit, sorted keys) | **559.74 µs** | 1.02 ms | Verbora 1.82× faster |
| `contains` (miss, first-char probe) | **49.45 µs** | 175.53 µs | Verbora 3.55× faster |
| `predictive_search` (1-char prefix) | **286.3 ns** | 500.20 µs | Verbora 1,748× faster |
| `predictive_search` (empty prefix, all 20K) | 1.15 ms | — | — (not part of this group's competitor set) |

Verbora's arena trie wins `build` and every measured operation against
`fast_radix_trie` — including single-character prefix enumeration, despite
`fast_radix_trie`'s own path compression.

<div class="callout callout-note">
<strong>Verbora's own answer: <code>FrozenTrie</code>.</strong>
<code>Trie::freeze()</code> is a safe-Rust (zero <code>unsafe</code>),
path-compressed, read-only representation built once from a
<code>Trie</code>, offered for the realistic autocomplete shape where
enumeration dominates. See
<a href="../features/trie">Trie</a> for its own usage and characteristics.
</div>

#### `fst` — a frozen finite-state transducer

[fst](https://crates.io/crates/fst) 0.4.7 (Andrew Gallant's) is architecturally
nothing like a trie: a finite-state transducer built once from sorted input,
queried via a `Streamer`, never mutated again.

| Operation | Verbora | fst | Verdict |
|---|--:|--:|---|
| `build` (random) | **2.13 ms** | 6.80 ms | Verbora 3.19× faster |
| `contains` (hit) | **469.74 µs** | 1.75 ms | Verbora 3.74× faster |
| `predictive_search` (1-char prefix) | **286.3 ns** | 1.84 ms | Verbora 6,432× faster |

`fst`'s own Levenshtein automaton (via its `levenshtein` feature) answers the
same fuzzy-candidate question as `verbora_spellcheck::FuzzyIndex` — a genuine
double crossover on both construction and query:

| Words | Construction: `FuzzyIndex` | Construction: `fst` | Query: `FuzzyIndex` | Query: `fst` |
|---:|--:|--:|--:|--:|
| 100 | **24.07 µs** | 60.73 µs | **281.48 µs** (d1) | 5.22 ms (d1) |
| 1,000 | 759.04 µs | **471.54 µs** | **4.00 ms** (d1) | 12.95 ms (d1) |
| 10,000 | 11.45 ms | **3.89 ms** | 24.78 ms (d1) | **14.78 ms** (d1) |
| 20,000 | 26.90 ms | **6.98 ms** | 40.62 ms (d1) | **20.30 ms** (d1) |

`FuzzyIndex` wins construction small, `fst` wins from 1,000 words up; the
distance-1 query race is closer than before but keeps the same shape —
`FuzzyIndex` wins small corpora, `fst` overtakes as the corpus grows large.
`fst` is also one of three crates in this audit found to carry a real,
independently-confirmed upstream defect — see
[Upstream bugs found](#upstream-bugs-found).

---

### Phonetics

Against [rphonetic](https://github.com/Dalvany/rphonetic) 3.0.6 — the one
actively-maintained Rust crate covering the same phonetic-algorithm
families, in the Apache commons-codec lineage, in a single crate
(`docs/COMPETITIVE_BENCHMARKS.md`
[§1.6](https://github.com/addlayerio/verbora/blob/main/docs/COMPETITIVE_BENCHMARKS.md#16-phonetics)).
Twelve encoder families are measured, in three regimes with three
different equivalence claims, all verified in
[`tests/phonetics_correctness.rs`](https://github.com/addlayerio/verbora/blob/main/benchmarks/competitive/rust-competitors/tests/phonetics_correctness.rs)
before any number was trusted:

- **Three variant encoders — throughput-only (`Partial`, never `Yes`).**
  `SoundEx`, `Metaphone` and `DoubleMetaphone` implement Verbora's own
  documented variants (condense-before-drop Soundex, a documented Metaphone
  stage-ordering quirk), rphonetic the textbook originals — byte-exact output
  is never asserted, only that both sides do the same *shape* of work.
- **Eight byte-exact encoders — full output equivalence (`Yes`).**
  `Cologne`, `Nysiis`, `Caverphone1`/`Caverphone2`, `Phonex`,
  `RefinedSoundex`, `MatchRatingApproach` and the branching `DaitchMokotoff`
  produce output **byte-identical** to rphonetic's on every input rphonetic
  handles without panicking — a stronger claim than any `Partial` row on
  this page carries.
- **`BeiderMorse` — throughput-only, `Partial`, and by far the heaviest
  encoder in the module.** Covered in its own subsection below.

#### The three variant encoders — throughput only

rphonetic's Metaphone/Double Metaphone default to a 4-character max code
length; both are reconfigured to `Some(32)` here to match Verbora's real
default of 32 — independently verified by test to actually change
rphonetic's output length, not silently still capped at 4.

| Algorithm | 1 name | 10,000 names | 100,000 names |
|---|--:|--:|--:|
| Soundex | **6.44× faster** | **3.27× faster** | **3.31× faster** |
| Metaphone | **2.28× faster** | **1.55× faster** | **1.53× faster** |
| Double Metaphone | **2.77× faster** | **1.94× faster** | **1.88× faster** |

Verbora wins all three algorithms at every benchmarked size.

| Library | Version | Language | Time (median, 100,000 names) | Throughput | Relative |
|---|---|---|---:|---:|---:|
| Verbora | 0.2.0 | Rust | 5.04 ms | 198.2/s | **1.00×** |
| rphonetic | 3.0.6 | Rust | 7.72 ms | 129.5/s | 1.53× slower |

<div class="callout callout-good">
<strong>Metaphone — a clean sweep at every size.</strong> Verbora's
<code>Metaphone</code> runs as a single skip-gated driver, fused from the
original 21 ordered whole-string rewrite stages, over per-thread pooled
scratch: letter-mask gates decide which rules can possibly fire on a given
word, window edits plus fused rules replace whole-string rewrites, and the
pipeline's two scratch buffers are reused across calls — an ASCII token
folds lowercase directly into pooled scratch, so a steady-state call's
only allocation is the returned code. rphonetic's <code>Metaphone</code>
is a single indexed forward scan (<code>O(n)</code>); Verbora wins anyway:
<strong>2.28× at a single name</strong> (32.8 ns vs. 74.9 ns), 1.55× at
10,000 names (487.15 vs. 756.74 µs), 1.53× at 100,000 (5.04 vs. 7.72 ms).
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
| Cologne | **17.0 ns** vs. 66.8 ns (3.93×) | **318.24 µs** vs. 765.23 µs (2.40×) | **3.32 ms** vs. 7.24 ms (2.18×) |
| NYSIIS | **30.6 ns** vs. 224.0 ns (7.31×) | **274.32 µs** vs. 1.89 ms (6.90×) | **2.67 ms** vs. 19.01 ms (7.12×) |
| Caverphone 1.0 | **169.4 ns** vs. 924.9 ns (5.46×) | **1.92 ms** vs. 9.98 ms (5.19×) | **19.07 ms** vs. 101.27 ms (5.31×) |
| Caverphone 2.0 | **152.8 ns** vs. 803.8 ns (5.26×) | **1.68 ms** vs. 9.17 ms (5.47×) | **17.20 ms** vs. 92.66 ms (5.39×) |
| Phonex | **30.4 ns** vs. 133.0 ns (4.38×) | **337.95 µs** vs. 1.20 ms (3.56×) | **3.64 ms** vs. 12.42 ms (3.41×) |
| Refined Soundex | **18.5 ns** vs. 116.9 ns (6.31×) | **151.28 µs** vs. 973.22 µs (6.43×) | **1.48 ms** vs. 9.76 ms (6.60×) |
| Match Rating Approach | **47.4 ns** vs. 526.1 ns (11.10×) | **460.10 µs** vs. 5.39 ms (11.72×) | **4.54 ms** vs. 53.99 ms (11.90×) |
| Daitch–Mokotoff (branching) | **152.7 ns** vs. 380.1 ns (2.49×) | **1.72 ms** vs. 3.93 ms (2.29×) | **15.94 ms** vs. 36.89 ms (2.31×) |

The mechanism is consistent across all eight groups rather than one trick:
Verbora's encoders run single-pass scans over one reused buffer, with one
heap allocation per call for the returned code (the branching
Daitch–Mokotoff adds a small branch list), against static compiled-in rule
tables. rphonetic's implementations allocate intermediate `String`s as
they go (Caverphone's rewrite cascade is one freshly allocated `String` per
step there) and, for Daitch–Mokotoff, parse the rules text with a `nom`
grammar at builder time and walk a `BTreeMap` per lookup where Verbora
indexes a pre-sorted static array. The margins range from 2.29×
(Daitch–Mokotoff at 10,000 names — the one algorithm where both sides
spend most of their time in the same branching walk) to 11.90× (Match
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

#### `BeiderMorse` — throughput only, and the heaviest encoder in the module

`BeiderMorse` (19-language auto-detecting Beider–Morse phonetic matching,
with no equivalent in Verbora's other reference points) against rphonetic's
`BeiderMorseBuilder` (`features = ["embedded_bm"]`). This is a coverage
asymmetry, not a fully equivalent comparison: rphonetic's `embedded_bm`
feature ships only the `"any"`/`"common"` rule files per `NameType`, not the
full per-language corpus, so its `ConfigFiles::default()` can never resolve
a specific guessed language and always falls back to `"any"` — Verbora's
full 18/10/5-language (Generic/Ashkenazi/Sephardic) `LangGuesser`
auto-detection has no equivalent on the rphonetic side to compare against.
Throughput only; output equivalence is never asserted, since both sides are
textbook-derived but independently implemented.

<div class="callout callout-warn">
<strong>Read this table's scale column before comparing it to any other row
on this page.</strong> <code>BeiderMorse</code> is dramatically heavier per
call than every other encoder in this module — a single name costs Verbora
7.24 µs here, against 32.8 ns for <code>Metaphone</code> and 17.0 ns for
<code>Cologne</code> on the same one-name input (roughly 220× and 426×
respectively). This module's own benchmark caps <code>BeiderMorse</code>'s
sweep at 1,000 names rather than the 100,000 every other encoder reaches,
because the algorithm's own per-name cost makes a 100,000-name sweep
impractically slow on either side.
</div>

| Names | Verbora | rphonetic | Verbora advantage |
|---:|--:|--:|--:|
| 1 | **7.24 µs** | 17.66 µs | 2.44× |
| 100 | **881.35 µs** | 1.66 ms | 1.88× |
| 1,000 | **9.71 ms** | 20.60 ms | 2.12× |

Verbora wins at every scale measured, by 1.68×–2.44× depending on size —
narrower and noisier than the byte-exact encoders' margins above, consistent
with both sides doing substantially more per-call work here (rule-table
lookups across an auto-detected language guess) than the single-pass scans
the rest of this module runs. Two additional input shapes are measured at
100 names: `compound_100` (hyphenated/compound surnames, **2.24 ms** vs.
8.16 ms, 3.65× faster) and `prefixed_100` (surnames with a common prefix,
**2.28 ms** vs. 6.00 ms, 2.63× faster) — both still clear Verbora wins, at
wider margins than the plain 100-name row, since rphonetic's per-call
allocation cost scales with the extra rule-table backtracking these shapes
trigger more than Verbora's does.

#### A second Double Metaphone implementation — C++, not Rust

Every other competitor on this page is a Rust crate. `pixelglow/double_metaphone`
is different: a header-only C++11 implementation of Lawrence Philips'
Double Metaphone algorithm, vendored into the workspace and compiled by
`build.rs`, then called through a thin `extern "C"` shim — the only
non-Cargo, non-Rust competitor benchmarked anywhere on this page. Every
measured call crosses the Rust/C++ boundary once (one `CString`
construction, one FFI call, two bounded buffer copies, one UTF-8
validation on the way back), and that cost is measured as part of the
number, not subtracted out. This comparison did not run as part of this
campaign; the figures below are carried forward from the last run that
measured it and are not re-verified against the current commit.

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
| Verbora | 0.2.0 | Rust | 47.23 µs | 21.2K/s | **1.00×** |
| pixelglow/double_metaphone | 79dd226 (2014) | C++11 | 85.30 µs | 11.7K/s | 1.81× slower |

---

### Language detection

Statistical language detection over free text
(`docs/COMPETITIVE_BENCHMARKS.md`
[§1.9](https://github.com/addlayerio/verbora/blob/main/docs/COMPETITIVE_BENCHMARKS.md#19-language-detection))
against [lingua](https://github.com/pemistahl/lingua-rs) 1.8.0 (built with
`from_languages()`, restricted to the 21-language overlap with Verbora, never
its default 75) and [whichlang](https://github.com/quickwit-oss/whichlang)
0.1.1 (13-language overlap, and — disclosed explicitly, not folded silently
into the accuracy numbers — it cannot abstain: `detect_language` always
returns a guess). A widely-used JavaScript NLP library has no general
statistical language-detection module (verified from source, not assumed),
so it does not appear here.

<div class="callout callout-good">
<strong>Verbora now ships three detector strategies, and two of them beat
<code>whichlang</code> outright.</strong> <code>WhatlangDetector</code>
remains the crate's default (best accuracy). <code>HashedLinearDetector</code>
(a zero-allocation, stack-only linear model, opt-in behind the
<code>fast-language-detection</code> feature) and
<code>FallbackDetector&lt;HashedLinearDetector, WhatlangDetector&gt;</code>
(the fast model as primary, deferring to <code>WhatlangDetector</code> only
where it declines to judge) are both new, and both are measured here
alongside the default.
</div>

#### Speed, by input length (English)

| Tier | HashedLinearDetector | FallbackDetector | WhatlangDetector (default) | whichlang | lingua |
|---|--:|--:|--:|--:|--:|
| short word (~6 B) | **44.2 ns** | 23.55 µs | 23.28 µs | 62.6 ns | 48.36 µs |
| short phrase (~30 B) | **150.2 ns** | 152.3 ns | 38.08 µs | 203.5 ns | 90.10 µs |
| sentence (~140 B) | **308.6 ns** | 312.6 ns | 32.16 µs | 531.4 ns | 199.29 µs |
| paragraph (~500 B) | 2.91 µs | **2.80 µs** | 103.35 µs | 6.00 µs | 608.82 µs |

| Library | Version | Language | Time (median, paragraph) | Throughput | Relative |
|---|---|---|---:|---:|---:|
| FallbackDetector⟨Hashed, Whatlang⟩ | 0.2.0 | Rust | 2.80 µs | 357.4K/s | **1.00×** |
| HashedLinearDetector | 0.2.0 | Rust | 2.91 µs | 343.6K/s | 1.04× slower |
| whichlang | 0.1.1 | Rust | 6.00 µs | 166.7K/s | 2.14× slower |
| WhatlangDetector (default) | 0.2.0 | Rust | 103.35 µs | 9.7K/s | 36.94× slower |
| lingua | 1.8.0 | Rust | 608.82 µs | 1.6K/s | 217.4× slower |

<div class="callout callout-note">
<strong>Only at the single-word tier does the story flip.</strong>
<code>HashedLinearDetector</code> alone is fastest everywhere, including
word tier (44.2 ns, still beating <code>whichlang</code>'s 62.6 ns). But
<code>FallbackDetector</code> costs 23.55 µs at word tier — almost the full
price of the default detector — because that is exactly the tier where its
accuracy is weakest and it defers to <code>WhatlangDetector</code> most
often. See the accuracy table below for why that trade exists.
</div>

#### Speed, by language (sentence tier)

| Language | HashedLinearDetector | whichlang | lingua | Faster |
|---|--:|--:|--:|---|
| German | 299.2 ns | 523.2 ns | 208.53 µs | **Verbora, 1.75×** |
| English | 305.7 ns | 504.8 ns | 174.03 µs | **Verbora, 1.65×** |
| Spanish | 742.4 ns | 1.36 µs | 183.16 µs | **Verbora, 1.83×** |
| French | 352.6 ns | 565.6 ns | 237.21 µs | **Verbora, 1.60×** |
| Hindi | 3.06 µs | 527.8 ns | 5.25 µs | whichlang, 5.80× |
| Italian | 335.0 ns | 561.3 ns | 236.51 µs | **Verbora, 1.68×** |
| Japanese | 416.4 ns | 295.3 ns | 5.86 µs | whichlang, 1.41× |
| Dutch | 316.7 ns | 577.2 ns | 259.56 µs | **Verbora, 1.82×** |
| Portuguese | 330.6 ns | 585.1 ns | 249.21 µs | **Verbora, 1.77×** |
| Russian | 2.35 µs | 403.2 ns | 73.81 µs | whichlang, 5.83× |
| Swedish | 308.2 ns | 478.1 ns | 253.87 µs | **Verbora, 1.55×** |
| Vietnamese | 793.9 ns | 467.0 ns | 222.65 µs | whichlang, 1.70× |
| Chinese | 267.9 ns | 142.9 ns | 3.00 µs | whichlang, 1.88× |

`HashedLinearDetector` wins 9 of 13 languages — every Latin-script language
in this set — and loses the four where a hashed-bucket linear model gets
less signal per byte: Hindi, Japanese, Russian and Vietnamese. `whichlang`'s
own hand-tuned per-language feature weights hold an edge on those four.

#### Accuracy

13 languages (the triple overlap all detectors can be scored on
identically) × 4 length tiers, sourced from the OHCHR UDHR Translation
Project (public-domain UN text; full sourcing and per-tier extraction rule in
[`datasets/README.md`](https://github.com/addlayerio/verbora/blob/main/benchmarks/competitive/datasets/README.md)).
Reproduced with `cargo run --release --example language_accuracy`, and
re-scored as an executed test in `crates/verbora-language/tests/default_detector.rs`.

| Detector | short word | short phrase | sentence | paragraph | **Overall** |
|---|--:|--:|--:|--:|--:|
| lingua (21-language restricted) | 92.3% (12/13) | 100% | 100% | 100% | **98.1%** (51/52) |
| WhatlangDetector (**default**) / FallbackDetector | 76.9% (10/13) | 100% | 100% | 100% | **94.2%** (49/52) |
| whichlang (13-language, cannot abstain) | 69.2% (9/13) | 100% | 100% | 100% | **92.3%** (48/52) |
| HashedLinearDetector | 53.8% (7/13) | 92.3% (12/13) | 100% | 100% | **86.5%** (45/52) |

`FallbackDetector` scores identically to the default `WhatlangDetector` on
every tier — that equivalence is the point of composing it, not a
coincidence — and it beats `whichlang` on both accuracy (94.2% vs. 92.3%)
and speed (every tier except the single-word one, where it defers to the
slower default). `HashedLinearDetector` alone trades 4 of 52 correct
answers, concentrated entirely in the two hardest, shortest tiers, for
being the fastest detector in this whole audit at every tier including
that one. **This is why the fastest detector is not the default**: shipping
it unqualified would understate exactly the cost that makes it fast.

#### `WhatlangDetector` wrapper overhead — not a ranked comparison

<div class="callout callout-note">
Isolates the cost of Verbora's own wrapper around <code>whatlang::Detector</code>
— it is <strong>not</strong> "Verbora vs. whatlang," because
<code>WhatlangDetector</code> literally constructs a
<code>whatlang::Detector</code> and calls <code>.detect()</code> on it.
The noisy ratios include a tier where the wrapper measured <em>faster</em>
than the bare call it makes, which is structurally impossible as a real
effect. Read as noise from a shared benchmark machine, not a finding.
</div>

| Tier | Verbora (`WhatlangDetector`) | whatlang (raw crate) | Ratio |
|---|--:|--:|--:|
| short word | 23.54 µs | 23.93 µs | 0.98× |
| short phrase | 39.94 µs | 39.93 µs | 1.00× |
| sentence | 30.32 µs | 30.28 µs | 1.00× |
| paragraph | 103.60 µs | 99.32 µs | 1.04× |

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
| short word | 8.8 ns | 37.0 ns | **4.2×** |
| short phrase | 14.1 ns | 60.6 ns | **4.3×** |
| sentence | 27.1 ns | 121.4 ns | **4.5×** |
| paragraph | 250.0 ns | 1.12 µs | **4.5×** |

| Library | Version | Language | Time (median, paragraph) | Throughput | Relative |
|---|---|---|---:|---:|---:|
| Verbora | 0.2.0 | Rust | 250.0 ns | 4.00M/s | **1.00×** |
| whatlang | 0.18.0 | Rust | 1.12 µs | 889.4K/s | 4.50× slower |

Verbora wins at every length tested, by a fairly steady ~4.2×–4.5×. By
language (sentence tier), Verbora is faster in 9 of 13 and loses 4:

| Language | Verbora | whatlang | Faster |
|---|--:|--:|--:|
| German | 43.0 ns | 132.3 ns | Verbora, 3.08× |
| English | 27.3 ns | 126.1 ns | Verbora, 4.62× |
| Spanish | 87.3 ns | 310.3 ns | Verbora, 3.56× |
| French | 60.6 ns | 136.2 ns | Verbora, 2.25× |
| Hindi | 2.96 µs | 188.9 ns | whatlang, 15.68× |
| Italian | 43.7 ns | 134.6 ns | Verbora, 3.08× |
| Japanese | 371.8 ns | 284.8 ns | whatlang, 1.31× |
| Dutch | 29.2 ns | 131.1 ns | Verbora, 4.50× |
| Portuguese | 35.3 ns | 142.4 ns | Verbora, 4.04× |
| Russian | 1.14 µs | 140.2 ns | whatlang, 8.14× |
| Swedish | 73.8 ns | 130.8 ns | Verbora, 1.77× |
| Vietnamese | 547.5 ns | 159.8 ns | whatlang, 3.43× |
| Chinese | 123.8 ns | 158.8 ns | Verbora, 1.28× |

Verbora's per-codepoint Unicode-range classifier loses on Hindi, Japanese,
Russian and Vietnamese — the same four languages `HashedLinearDetector`
loses in the language-detection table above, consistent with those scripts
needing more per-codepoint classification work in Verbora's 10-script
model than in `whatlang`'s wider 25-script one.

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
| 1× | 758.4 ns | 1.87 µs | **2.5×** |
| 16× | 2.75 µs | 6.71 µs | **2.4×** |
| 256× | 46.49 µs | 100.26 µs | **2.2×** |

| Library | Version | Language | Time (median, 1024×) | Throughput | Relative |
|---|---|---|---:|---:|---:|
| Verbora | 0.2.0 | Rust | 184.66 µs | 5.4K/s | **1.00×** |
| wana_kana | 5.0.0 | Rust | 417.82 µs | 2.4K/s | 2.26× slower |

Verbora wins at every size measured, 2.16×–2.46× depending on length.

---

### POS tagging

`verbora-tagger` (Brill, transformation-based) against
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
<strong>This comparison has no Verbora side.</strong>
<code>verbora-tagger</code> ships no lexicon: the English dictionary and rule set
it carried through 0.2 were removed in 0.3 for licensing reasons, and the
build-time packed table whose construction a cold-start figure would time went
with them. Reinstating the comparison is a measurement-design decision before it is a benchmark run:
the harness has to pick a lexicon and hand the same one to every side, or the
two are not answering the same question. The competitor measurements below
stand.
</div>

#### Cold start — everything needed before tagging one sentence

| Library | Version | Language | Time (median) |
|---|---|---|---:|
| postagger (parses a 5.6 MB weights file) | 0.0.3 | Rust | 109.18 ms |
| rust-bert (loads a ~94 MB MobileBERT checkpoint) | 0.23.0 | Rust | 151.58 ms |

#### Steady state — per-call latency, tagger already constructed

| Library | Version | Language | Time (median, 9 tokens) | Time (median, 20 tokens) | Time (median, batch of 8×9-tok) |
|---|---|---|---:|---:|---:|
| postagger | 0.0.3 | Rust | 58.95 µs | 75.72 µs | 451.73 µs |
| rust-bert | 0.23.0 | Rust | 12.45 ms | 9.22 ms | 13.41 ms |

What the two tables show on their own is the price of the technique each crate
chose: a pretrained model must deserialize its weights before it can answer
anything, and then spends a feature-weighted vote or a full transformer forward
pass per token. A rule-based tagger pays neither — its construction cost is
whatever reading its lexicon and rule set costs, and its per-token work is a
dictionary probe plus one pass per rule. That is a structural difference, not a
measured one, and this section deliberately puts no number on it until the
Verbora configuration is defined again.

**This is not an accuracy claim for either technique** — see
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
(Hunspell affix-rule morphology). `corrections` returns `Vec<Correction>`,
each entry carrying the candidate word alongside the frequency and edit
distance behind its ranking; `correction_words` is the separate call for
plain owned `String`s where the ranking metadata isn't needed.

<div class="callout callout-good">
<strong>Verbora wins construction, membership testing and correction
generation against all three competitors, at every size measured.</strong>
</div>

#### `symspell` and `harper-core` — same corpus as Verbora

Both loaded with the identical `words.json` corpus and per-word frequencies
Verbora uses.

| Group | Corpus | Verbora | symspell | harper-core |
|---|--:|--:|--:|--:|
| construction (`new`) | 100 | **5.25 µs** | 388.13 µs (73.9× slower) | 74.04 µs (14.1× slower) |
| construction (`new`) | 20,000 | **1.50 ms** | 118.51 ms (78.9× slower) | 10.63 ms (7.1× slower) |
| `is_correct` (hit) | 20,000 | **20.85 µs** | 296.46 µs (14.2× slower) | 115.57 µs (5.5× slower) |
| `corrections`, distance 1 | 100 | **173.4 ns** | 853.1 ns (4.9× slower) | 5.01 µs (28.9× slower) |
| `corrections`, distance 1 | 20,000 | **190.6 ns** | 920.5 ns (4.8× slower) | 37.40 µs (196.2× slower) |
| `corrections`, distance 2 | 1,000 | **301.3 ns** | 2.29 µs (7.6× slower) | 32.25 µs (107.1× slower) |
| `corrections`, distance 2 | 20,000 | **642.5 ns** | 3.34 µs (5.2× slower) | 335.64 µs (522.4× slower) |

Verbora wins **every row in this table**, at every size measured. A
`verbora-borrowed` variant (returning views rather than owned corrections)
runs alongside `verbora` in every group above and is marginally faster
still (e.g. 164.5 ns vs. 173.4 ns at distance 1, 100-word corpus).

#### `spellbook` — matched-workload timing only, not a fair ratio

<div class="callout callout-note">
Hunspell's <code>.aff</code>/<code>.dic</code> format has no concept of a
flat frequency corpus — <code>spellbook</code> cannot load Verbora's corpus,
and Verbora cannot load a Hunspell dictionary. Each side is timed on its own
inputs: <code>spellbook</code>'s hit and near-miss-typo cases against
Verbora's own hit case, and four different <code>spellbook</code> typos
against Verbora's own single typo case. This is a timing comparison of two
different dictionaries doing conceptually the same job, <strong>never</strong>
presented as a ratio. No Relative column below.
</div>

| Operation | Library | Version | Dictionary | Time (median) |
|---|---|---|---|--:|
| `check` / `is_correct`, hit | spellbook | 0.4.2 | real `en_US` Hunspell | 358.4 ns |
| `check` / `is_correct`, near-miss typo | spellbook | 0.4.2 | real `en_US` Hunspell | 3.15 µs |
| `is_correct`, hit | Verbora | 0.2.0 | own 20,000-word corpus | 4.86 µs |
| `suggest`, `"helo"`/`"korrect"`/`"wrold"`/`"beleive"` | spellbook | 0.4.2 | real `en_US` Hunspell | 4.79–8.20 ms |
| `corrections`, one typo (`typo8`) | Verbora | 0.2.0 | own 20,000-word corpus | 178.7 ns |

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
zero-copy archived-load path.

| Group | Corpus | Verbora | fast_symspell |
|---|--:|--:|--:|
| construction (`FuzzyIndex`) | 100 | **23.97 µs** | 361.09 µs (15.1× slower) |
| construction (`FuzzyIndex`) | 20,000 | **25.48 ms** | 110.84 ms (4.4× slower) |
| `corrections`, distance 1 | 100 | **173.4 ns** | 817.7 ns (4.7× slower) |
| `corrections`, distance 1 | 20,000 | **190.6 ns** | 844.6 ns (4.4× slower) |
| `corrections`, distance 2 | 1,000 | **301.3 ns** | 2.19 µs (7.3× slower) |
| `corrections`, distance 2 | 20,000 | **642.5 ns** | 3.50 µs (5.4× slower) |

Verbora wins both construction and correction generation against
`fast_symspell` at every size measured.

<div class="callout callout-note">
<strong>Verbora's own answer: <code>DeletionIndex</code>.</strong>
<code>verbora_spellcheck::DeletionIndex</code> is a SymSpell-style index
built in-house, offered alongside the existing <code>FuzzyIndex</code>
BK-tree rather than replacing it. Its own head-to-head figures against
<code>FuzzyIndex</code> are not part of this campaign — see the note below.
</div>

#### `FuzzyIndex` vs. `DeletionIndex` — not part of this campaign

<div class="callout callout-warn">
<strong>No fresh measurement exists for this comparison.</strong>
<code>DeletionIndex</code>'s internal map was changed to key on a 64-bit
hash of each deletion sequence rather than the sequence itself, taking the
cost of indexing one word from cubic to quadratic in its length — but this
campaign's benchmark run did not include a <code>FuzzyIndex</code>-vs-
<code>DeletionIndex</code> group, so no current figures exist to publish
here. Neither structure replaces the other — <code>FuzzyIndex</code> stays
the default (cheaper, more predictable, no build-time distance ceiling);
<code>DeletionIndex</code> is offered for a large dictionary with
<code>max_distance</code> known ahead of time and high query volume. A
timing comparison between them awaits a future run.
</div>

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

| Library | Version | Language | Time (median, build, 256 docs) | Throughput | Relative |
|---|---|---|---:|---:|---:|
| afshinm | 0.3.0 | Rust | 59.50 ms | 16.8/s | **1.00×** |
| Verbora | 0.2.0 | Rust | 140.05 ms | 7.1/s | 2.35× slower |

| Docs | Verbora | afshinm |
|---:|--:|--:|
| 4 | 2.11 ms | 806.41 µs |
| 16 | 8.47 ms | 3.03 ms |
| 64 | 35.55 ms | 15.03 ms |
| 256 | 140.05 ms | 59.50 ms |

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

A second construction shape, `build_many_small` — many short, per-document
~200-word chunks rather than a few large documents — isolates per-document
overhead (interner/document-frequency bookkeeping for Verbora, one push per
document for `afshinm`):

| Docs | Verbora | afshinm | Relative |
|---:|--:|--:|--:|
| 4 | 13.69 µs | 3.26 µs | 4.20× slower |
| 64 | 214.31 µs | 57.40 µs | 3.73× slower |
| 256 | 853.75 µs | 224.31 µs | 3.81× slower |
| 1024 | 3.42 ms | 1.19 ms | 2.87× slower |

`afshinm` wins this shape too, by a narrower and fairly stable margin
(2.9×–4.2×) than the few-large-documents shape above.

| Library | Version | Language | Time (median, `tfidf()` query, 256 docs) | Throughput | Relative |
|---|---|---|---:|---:|---:|
| Verbora | 0.2.0 | Rust | 50.5 ns | 19.82M/s | **1.00×** |
| rust-tfidf | 1.1.1 | Rust | 1.49 µs | 672.7K/s | 29.5× slower |
| tfidf (afshinm) | 0.3.0 | Rust | 248.97 ms | 4.0/s | ~4.9M× slower |

| Docs | Verbora | afshinm | rust-tfidf |
|---:|--:|--:|--:|
| 4 | 50.4 ns | 4.95 ms | 52.0 ns |
| 16 | 50.5 ns | 17.29 ms | 101.7 ns |
| 64 | 50.4 ns | 64.03 ms | 375.3 ns |
| 256 | 50.5 ns | 248.97 ms | 1.49 µs |

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
[naivebayes](https://crates.io/crates/naivebayes) 0.1.2 (ruivieira) — a
pre-tokenized, fixed-smoothing-floor Naive Bayes implementation
(`docs/COMPETITIVE_BENCHMARKS.md`
[§1.13](https://github.com/addlayerio/verbora/blob/main/docs/COMPETITIVE_BENCHMARKS.md#113-classifiers)).
`linfa-bayes` no longer appears in the timing rows below: its published
`fit_with` calls an unconditional `dbg!` once per class on every training
call, which turned a training-loop benchmark into gigabytes of stderr
output — a defect in the published crate, not something this harness works
around. `linfa-bayes` stays in the accuracy comparison below, where it is
called far less often, and in `linfa-logistic`'s own, unrelated rows further
down, which are unaffected.

| Library | Version | Language | Time (median, train, 1024 docs) | Throughput | Relative |
|---|---|---|---:|---:|---:|
| naivebayes | 0.1.2 | Rust | 935.45 µs | 1.1K/s | **1.00×** |
| smartcore | 0.6.5 | Rust | 1.58 ms | 631.3/s | 1.69× slower |
| Verbora | 0.2.0 | Rust | 2.06 ms | 484.5/s | 2.21× slower |

| Docs | Verbora | smartcore | naivebayes |
|---:|--:|--:|--:|
| 4 | 17.75 µs | **5.60 µs** | 7.67 µs |
| 16 | 58.79 µs | **24.12 µs** | 29.19 µs |
| 64 | 197.03 µs | 92.40 µs | **89.84 µs** |
| 256 | 639.89 µs | 337.91 µs | **292.40 µs** |
| 1024 | 2.06 ms | 1.58 ms | **935.45 µs** |

| Library | Version | Language | Time (median, predict) | Throughput | Relative |
|---|---|---|---:|---:|---:|
| Verbora | 0.2.0 | Rust | 1.78 µs | 560.7K/s | **1.00×** |
| naivebayes | 0.1.2 | Rust | 3.08 µs | 324.4K/s | 1.73× slower |
| smartcore | 0.6.5 | Rust | 3.93 µs | 254.5K/s | 2.20× slower |

<div class="callout callout-note">
<strong>A genuinely mixed result.</strong> Verbora loses training at every
size (2.2×–3.2× slower than the faster of the two competitors, narrowing
with corpus size) but <strong>wins prediction</strong> against both — the
one row of this section where Verbora comes out fastest. Verbora's
per-document training cost includes real, specified tokenization, Porter
stemming and stop-word filtering; neither competitor's benchmark adapter
does any of that.
</div>

#### Logistic Regression

`LogisticRegressionClassifier` against
[smartcore](https://github.com/smartcorelib/smartcore) 0.6.5's
`linear::logistic_regression`,
[linfa-logistic](https://github.com/rust-ml/linfa) 0.8.1, and
[rustlearn](https://github.com/maciejkula/rustlearn) 0.5.0 (SGD-based,
unmaintained since 2018, included for historical prominence and flagged as
stale).

| Docs | Verbora | smartcore | linfa-logistic | rustlearn |
|---:|--:|--:|--:|--:|
| 4 | **24.87 µs** | 284.69 µs | 71.40 µs | 4.64 µs |
| 8 | **55.43 µs** | 381.38 µs | 96.84 µs | 10.53 µs |
| 12 | **103.68 µs** | 489.53 µs | 144.64 µs | 14.61 µs |
| 16 | 141.55 µs | 645.95 µs | **127.48 µs** | 22.31 µs |

<div class="callout callout-good">
<strong>Verbora now beats both smartcore and linfa-logistic at almost every
size tested.</strong> Against <code>smartcore</code>, Verbora wins at every
size, by a widening margin as corpus size shrinks (4.6×–11.4×). Against
<code>linfa-logistic</code>, Verbora wins at 4, 8 and 12 documents (up to
2.9× faster) and loses narrowly only at 16 (1.11× slower) — a real crossover,
but one that now favours Verbora through nearly the whole range measured.
<code>rustlearn</code>'s single-epoch SGD remains the fastest at every size
(5.4×–7.1× faster than Verbora), doing asymptotically less work than every
iterate-to-convergence competitor.
</div>

| Library | Version | Language | Time (median, prediction) | Relative |
|---|---|---|---:|---:|
| rustlearn | 0.5.0 | Rust | 362.1–406.4 ns | **1.00×** |
| linfa-logistic | 0.8.1 | Rust | 426.8–436.7 ns | 1.08×–1.18× slower |
| smartcore | 0.6.5 | Rust | 535.5–593.3 ns | 1.46×–1.48× slower |
| Verbora | 0.2.0 | Rust | 730.9–814.6 ns | 1.80×–2.25× slower |

Verbora loses single-document prediction to all three competitors, by
1.8×–2.25×.

#### Accuracy: is the slower classifier at least more correct?

A separate, signal-bearing corpus (four non-overlapping topical
vocabularies, generated by `tools/bench-data/generate.py`) was built
specifically because the training corpus above is shape-only random data —
useless for accuracy. `cargo test --test classifiers_accuracy` trains
Verbora, `smartcore` and `linfa-bayes` at each size and scores them against
a fixed, disjoint 128-document test set:

| Train size | Verbora | smartcore | linfa-bayes |
|---|--:|--:|--:|
| 4 | **98.4%** | 93.0% | 93.0% |
| 16 | 100.0% | 100.0% | 100.0% |
| 64 | 100.0% | 100.0% | 100.0% |
| 256 | 100.0% | 100.0% | 100.0% |
| 1024 | 100.0% | 100.0% | 100.0% |

All three converge to a perfect score by 16 training documents. Read
alongside the speed table above: accuracy is statistically indistinguishable
between the three at every size that matters, so the speed numbers stand as
measured, not offset by a quality difference that is not actually there on
this test set.

---

### Sentiment

`SentimentAnalyzer`'s AFINN-based document scoring against
[sentiment](https://crates.io/crates/sentiment) 0.1.1 (mount-research) — the
only Rust crate found that scores text against an AFINN-family lexicon,
published once in 2017 with no later release.

<div class="callout callout-note">
<strong>A narrowed comparison, over a corpus built specifically to make it
fair.</strong> <code>sentiment</code> embeds the older, smaller AFINN-111
(2,462 entries); Verbora ships AFINN-165 (3,382 entries) and has no
negation-free mode. Rather than compare the two lexicons on arbitrary text —
which would report a lexicon difference as a speed difference — the
benchmarked corpus is drawn from the <strong>2,438-word intersection</strong>
where the two tables agree exactly (of AFINN-111's 2,462 keys, all but 4 are
also in AFINN-165 with the same polarity), excludes the four words in
Verbora's negation list (<code>not</code>/<code>no</code>/<code>never</code>/
<code>neither</code>, which <code>sentiment</code> does not implement at all),
and uses only lowercase ASCII words joined by single spaces — the one input
shape where <code>sentiment</code>'s internal, non-swappable tokenizer and
Verbora's <code>WordTokenizer</code> produce the same token list. Every
exclusion is proved, not just asserted, in
<a href="https://github.com/addlayerio/verbora/blob/main/benchmarks/competitive/rust-competitors/tests/sentiment_correctness.rs"><code>tests/sentiment_correctness.rs</code></a>,
which runs the identical corpus through both crates and fails if they ever
disagree.
</div>

| Library | Version | Language | Time (median, 1024-word document) | Throughput | Relative |
|---|---|---|---:|---:|---:|
| Verbora | 0.2.0 | Rust | 28.04 µs | 35.7K/s | **1.00×** |
| sentiment | 0.1.1 | Rust | 300.84 µs | 3.3K/s | 10.73× slower |

| Input (words) | Verbora | sentiment |
|---:|--:|--:|
| 4 | 139.2 ns | 33.59 µs |
| 16 | 439.9 ns | 38.63 µs |
| 64 | 1.71 µs | 47.01 µs |
| 256 | 6.84 µs | 97.71 µs |
| 1024 | 28.04 µs | 300.84 µs |

Verbora wins at **every size measured**, by a wide and widening margin at
small input — 241× at 4 words, narrowing to 10.7× at 1024. Two costs are
structural to `sentiment`'s published API and stay inside its measured
region, because a caller cannot avoid them either: `analyze()` tokenizes the
document twice (once each for its internal `positivity`/`negativity` calls)
and compiles four `Regex`es on every call — a fixed per-call cost that is
nearly the whole measurement at 4 words and mostly amortized by 1024.
Verbora's own scoring loop carries negation state and probes for
multi-token phrase keys on every token, a capability this corpus never
exercises but still pays for.

---

### WordNet

Reading the Princeton WordNet database against
[wordnet-db](https://crates.io/crates/wordnet-db) 0.1.3 (johanneswd) — a
reader for the same `index.*`/`data.*` files `verbora-wordnet` reads, from
the same directory, answering the same questions. Both sides read a real
Princeton WordNet 3.1 `dict/` distribution, which this repository does not
vendor (Princeton's own licence, not MIT — see
`crates/verbora-wordnet/LICENSE-WORDNET`).

<div class="callout callout-good">
<strong>Verbora can now read this dictionary completely.</strong> A defect
found while first running this comparison against a real distribution —
<code>PointerSymbol::from_symbol</code> rejected the bare <code>;</code>/
<code>-</code> domain-pointer forms Princeton's index files actually write,
affecting 8.8% of WordNet 3.1's index entries, including common words like
<code>run</code>, <code>cat</code> and <code>water</code> — is fixed in
0.3.0. Every figure below covers the whole dictionary, not a probe list that
had to avoid the unreadable 8.8%.
</div>

The two crates are mechanically opposite, which is what makes the
comparison worth publishing rather than an objection to it: `wordnet-db`
mmaps or reads all eight files **and eagerly parses every index line and
data record into `HashMap`s** at open; `verbora-wordnet` reads bytes (or
not, depending on `Storage`) and binary-searches the index file per query,
paying the parse cost only for the one record a query actually touches.

#### Open and cold start

| Group | Verbora (fastest strategy) | wordnet-db (fastest mode) | Verbora advantage |
|---|--:|--:|--:|
| `open` | 9.83 µs (`Pread`) | 228.17 ms (`Mmap`) | **23,210×** |
| `cold` (open + first lookup, `entity`) | 17.40 µs (`Pread`) | 232.20 ms (`Mmap`) | **13,346×** |

`wordnet-db`'s two `LoadMode`s (`Mmap`, `Owned`) both parse the entire
dictionary eagerly at open — mmapping only defers the OS read, not
`wordnet-db`'s own parse pass — so both cost roughly 228–237 ms regardless
of mode. `verbora-wordnet`'s `Pread`/`LazyResident` strategies defer nearly
everything, which is why `open` and `cold` cost microseconds rather than
milliseconds: the real work only happens once a query actually needs it.

#### Query, once loaded — the headline pair: `Resident` vs. `Owned`

Both sides read all eight files into owned heap buffers at open, with no
`unsafe` and no OS mapping — the only remaining difference is what each does
with the bytes afterwards.

| Lemma | Verbora (`Resident`) | wordnet-db (`Owned`) | Faster |
|---|--:|--:|---|
| `entity` (index entry only) | 357.5 ns | **42.1 ns** | wordnet-db, 8.50× |
| `entity` (full lookup) | 714.4 ns | **90.2 ns** | wordnet-db, 7.92× |
| `dog` (full lookup) | 4.61 µs | **687.0 ns** | wordnet-db, 6.71× |
| `run` (full lookup, 16 senses) | 10.03 µs | **1.39 µs** | wordnet-db, 7.22× |

`wordnet-db` wins every query once both sides are loaded, by roughly
7×–8.5× — the payoff of its eager, fully-parsed `HashMap` representation.
Verbora's `Synset` owns its `String`s; `wordnet-db`'s borrows `&str` out of
the mapped buffer, allocating only the `Vec<Lemma>`/`Vec<Pointer>` spines —
an asymmetry intrinsic to holding the whole file resident, not a shortcut
handed to one side.

#### `LazyResident` vs. `Mmap` — both crates' answer to "don't pay for what you don't touch"

Not the same mechanism — one defers a `read`, the other defers a page fault
— but the two crates' comparable answers to the same question, and
`Mmap` is `wordnet-db`'s own default.

| Lemma | Verbora (`LazyResident`) | wordnet-db (`Mmap`) | Faster |
|---|--:|--:|---|
| `entity` (full lookup) | 735.1 ns | **90.4 ns** | wordnet-db, 8.15× |
| `dog` (full lookup) | 4.60 µs | **638.5 ns** | wordnet-db, 7.20× |

`Pread` and `Indexed` (Verbora-internal strategies with no `wordnet-db`
counterpart) are consistently the slowest of Verbora's four once resident —
`Pread` re-reads from disk on every query — and are carried here only for
the Verbora-internal ranking, not compared against a competitor row.

**The trade-off in one sentence:** Verbora opens roughly four orders of
magnitude faster and wins any workload dominated by a handful of lookups
per process lifetime; `wordnet-db` wins any workload that stays resident
and issues many lookups, by paying its cost once at startup instead of once
per query.

---

## No Rust competitor exists: sentence analysis (Analyzers)

One of the workspace's 15 benchmarked modules has **no fair Rust competitor
at all**: every candidate found for the composed sentence/text-analysis task
Verbora's `Analyzers` module performs was investigated and rejected on scope
grounds — no Rust crate performs the same composed task. Per this project's
`NO FAIR COMPETITOR FOUND` policy, none is forced. A widely-used JavaScript
NLP library remains the available baseline for this module. Reproduce or
publish any comparison using the method on this site; do not infer a Rust
ranking where no equivalent Rust implementation exists.

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
- **`rphonetic` 3.0.6** — several encoders panic on realistic non-ASCII
  input, all reproduced against 3.0.6 release builds during the differential
  fuzzing above: `Nysiis` in strict mode byte-slices its code at offset 6
  with no character-boundary check, panicking whenever a longer code's byte
  6 splits a multi-byte character (4,233 of the 104,114 fuzzed inputs);
  `Caverphone1`/`Caverphone2` panic the same way at their fixed 6-/10-byte
  code cut. The ASCII-only shared corpus never exercises the character-
  boundary panics above; on every one of those inputs Verbora's own
  byte-exact encoders return a documented substitute output instead of
  panicking — the one place they deliberately do not match rphonetic.

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
| Sentiment | ✓ | ✓ | P |
| WordNet | ✓ | ✓ | P |
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
| editdistancek | Rust | 1.0.2 | MIT | [GitHub](https://github.com/nkkarpov/editdistancek) | [crates.io](https://crates.io/crates/editdistancek) | [docs.rs](https://docs.rs/editdistancek/1.0.2) |
| stringmetrics | Rust | 2.2.2 | Apache-2.0 | [GitHub](https://github.com/pluots/stringmetrics) | [crates.io](https://crates.io/crates/stringmetrics) | [docs.rs](https://docs.rs/stringmetrics/2.2.2) |
| tantivy | Rust | 0.26.1 | MIT | [GitHub](https://github.com/quickwit-oss/tantivy) | [crates.io](https://crates.io/crates/tantivy) | [docs.rs](https://docs.rs/tantivy/0.26.1) |
| tokenizers (Hugging Face) | Rust | 0.23.1 | Apache-2.0 | [GitHub](https://github.com/huggingface/tokenizers) | [crates.io](https://crates.io/crates/tokenizers) | [docs.rs](https://docs.rs/tokenizers/0.23.1) |
| segtok | Rust | 0.1.5 | MIT | [GitHub](https://github.com/xamgore/segtok) | [crates.io](https://crates.io/crates/segtok) | [docs.rs](https://docs.rs/segtok/0.1.5) |
| unicode-segmentation | Rust | 1.13.3 | MIT/Apache-2.0 | [GitHub](https://github.com/unicode-rs/unicode-segmentation) | [crates.io](https://crates.io/crates/unicode-segmentation) | [docs.rs](https://docs.rs/unicode-segmentation/1.13.3) |
| rust-stemmers | Rust | 1.2.0 | MIT / BSD-3-Clause | [GitHub](https://github.com/CurrySoftware/rust-stemmers) | [crates.io](https://crates.io/crates/rust-stemmers) | [docs.rs](https://docs.rs/rust-stemmers/1.2.0) |
| snowball_stemmers_rs | Rust | 1.0.1 | MIT | [GitHub](https://github.com/SeekStorm/snowball-stemmers-rs) | [crates.io](https://crates.io/crates/snowball_stemmers_rs) | [docs.rs](https://docs.rs/snowball_stemmers_rs/1.0.1) |
| nltk-porter | Rust | 0.1.0 | Apache-2.0 | [GitHub](https://github.com/VoiceLessQ/nltk-porter) | [crates.io](https://crates.io/crates/nltk-porter) | [docs.rs](https://docs.rs/nltk-porter/0.1.0) |
| porter-stemmer | Rust | 0.1.2 | MPL-2.0 | [GitHub](https://github.com/samgiles/porter-stemmer) | [crates.io](https://crates.io/crates/porter-stemmer) | [docs.rs](https://docs.rs/porter-stemmer/0.1.2) |
| lindera-analysis | Rust | 5.2.0 | MIT | [GitHub](https://github.com/lindera/lindera) | [crates.io](https://crates.io/crates/lindera-analysis) | [docs.rs](https://docs.rs/lindera-analysis/5.2.0) |
| sastrawi | Rust | 0.1.1 | MIT | [GitHub](https://github.com/idevoid/rust-sastrawi) | [crates.io](https://crates.io/crates/sastrawi) | [docs.rs](https://docs.rs/sastrawi/0.1.1) |
| diacritics | Rust | 0.2.2 | GPL-3.0 | [GitHub](https://github.com/YesSeri/diacritics) | [crates.io](https://crates.io/crates/diacritics) | [docs.rs](https://docs.rs/diacritics/0.2.2) |
| kana-converter | Rust | 0.1.2 | MIT | [GitHub](https://github.com/kitsuneninetails/kana-converter) | [crates.io](https://crates.io/crates/kana-converter) | [docs.rs](https://docs.rs/kana-converter/0.1.2) |
| ordinal | Rust | 0.4.0 | MPL-2.0 | [GitHub](https://github.com/heaths/ordinal-rs) | [crates.io](https://crates.io/crates/ordinal) | [docs.rs](https://docs.rs/ordinal/0.4.0) |
| Inflector | Rust | 0.11.4 | BSD-2-Clause | [GitHub](https://github.com/whatisinternet/inflector) | [crates.io](https://crates.io/crates/Inflector) | [docs.rs](https://docs.rs/Inflector/0.11.4) |
| pluralizer | Rust | 0.5.0 | MIT/Apache-2.0 | [GitHub](https://github.com/KennethGomez/pluralizer) | [crates.io](https://crates.io/crates/pluralizer) | [docs.rs](https://docs.rs/pluralizer/0.5.0) |
| trie-rs | Rust | 0.4.2 | MIT OR Apache-2.0 | [GitHub](https://github.com/laysakura/trie-rs) | [crates.io](https://crates.io/crates/trie-rs) | [docs.rs](https://docs.rs/trie-rs/0.4.2) |
| qp-trie | Rust | 0.8.2 | MPL-2.0 | [GitHub](https://github.com/sdleffler/qp-trie-rs) | [crates.io](https://crates.io/crates/qp-trie) | [docs.rs](https://docs.rs/qp-trie/0.8.2) |
| fast_radix_trie | Rust | 1.2.0 | MIT | [GitHub](https://github.com/bluecatengineering/fast_radix_trie) | [crates.io](https://crates.io/crates/fast_radix_trie) | [docs.rs](https://docs.rs/fast_radix_trie/1.2.0) |
| fst | Rust | 0.4.7 | MIT OR Unlicense | [GitHub](https://github.com/BurntSushi/fst) | [crates.io](https://crates.io/crates/fst) | [docs.rs](https://docs.rs/fst/0.4.7) |
| rphonetic | Rust | 3.0.6 | Apache-2.0 | [GitHub](https://github.com/Dalvany/rphonetic) | [crates.io](https://crates.io/crates/rphonetic) | [docs.rs](https://docs.rs/rphonetic/3.0.6) |
| pixelglow/double_metaphone | C++11 | 79dd226 (2014) | BSD-2-Clause | [GitHub](https://github.com/pixelglow/double_metaphone) | vendored, no registry package | header comment |
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
| naivebayes | Rust | 0.1.2 | Apache-2.0 | [GitLab](https://gitlab.com/ruivieira/naive-bayes) | [crates.io](https://crates.io/crates/naivebayes) | [docs.rs](https://docs.rs/naivebayes/0.1.2) |
| linfa-logistic | Rust | 0.8.1 | MIT OR Apache-2.0 | [GitHub](https://github.com/rust-ml/linfa) | [crates.io](https://crates.io/crates/linfa-logistic) | [docs.rs](https://docs.rs/linfa-logistic/0.8.1) |
| rustlearn | Rust | 0.5.0 | Apache-2.0 | [GitHub](https://github.com/maciejkula/rustlearn) | [crates.io](https://crates.io/crates/rustlearn) | [docs.rs](https://docs.rs/rustlearn/0.5.0) |
| sentiment | Rust | 0.1.1 | MIT | [crates.io](https://crates.io/crates/sentiment) | [crates.io](https://crates.io/crates/sentiment) | [docs.rs](https://docs.rs/sentiment/0.1.1) |
| wordnet-db | Rust | 0.1.3 | MIT OR Apache-2.0 | [GitHub](https://github.com/johanneswd/crosswordsolver) | [crates.io](https://crates.io/crates/wordnet-db) | [docs.rs](https://docs.rs/wordnet-db/0.1.3) |

Full research dossier for every candidate considered — including every
crate investigated and *not* selected, and why — lives in
[`docs/COMPETITIVE_BENCHMARKS.md`](https://github.com/addlayerio/verbora/blob/main/docs/COMPETITIVE_BENCHMARKS.md).

## Reproducing these numbers

Everything on this page regenerates from a clean checkout:

```bash
# Shared inputs both sides read (run once)
python3 tools/bench-data/generate.py

cd benchmarks/competitive

# Third-party model/dictionary assets for POS tagging, spellcheck and WordNet
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
