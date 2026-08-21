# Benchmark method

This is the one section of the site where Verbora is measured against other
implementations. Every other page describes what Verbora does; these pages show
what it costs, side by side with pinned third-party crates and — for the few
capabilities where no comparable Rust crate exists — a widely-used JavaScript
NLP library.

Every number here was produced by the commands on
[Reproducing the benchmarks](reproducing.md), never estimated, and the
results that went the wrong way are published next to the ones that went the
right way.

<div class="callout callout-warn">
<strong>Nothing on these four pages has been re-measured against Verbora
0.2.0.</strong> The benchmark campaign for that release has not run yet, and
several of the kernels these numbers measured have since been replaced —
plain Levenshtein search among them, which now runs a bit-parallel kernel
instead of the full matrix its own figures were captured against (see
<a href="distance#where-the-levenshtein-win-comes-from">Where the
Levenshtein win comes from</a>). Where a table or row is marked **pending
re-measurement**, treat its number as historical rather than current; running
the commands on <a href="reproducing">Reproducing the benchmarks</a> today
will report whatever the current code does, not the number printed there. See
<a href="../getting-started/upgrading">Upgrading from 0.1 to 0.2</a> for what
changed, and <a href="competitive">Competitive benchmarks</a>'s own
coverage table for exactly which capabilities are still waiting on a re-run.
</div>

## The three pages

| Page | What it holds |
|---|---|
| [Competitive benchmarks](competitive.md) | The broad, version-pinned comparison, capability by capability |
| [String distance results](distance.md) | The focused distance measurements and the analysis behind them |
| [Reproducing them](reproducing.md) | The exact commands, from a clean checkout |

## Comparisons by capability

| Capability | Comparison |
|---|---|
| String distance | [Distance](competitive.md#distance) · [focused results](distance.md) |
| Tokenizers | [Tokenizers](competitive.md#tokenizers) |
| Stemmers | [Stemmers](competitive.md#stemmers) |
| N-grams | [N-grams](competitive.md#n-grams) |
| Normalizers | [Normalizers](competitive.md#normalizers) |
| Inflectors | [Inflectors](competitive.md#inflectors) |
| Trie | [Trie](competitive.md#trie) |
| Phonetics | [Phonetics](competitive.md#phonetics) |
| Spellcheck | [Spellcheck](competitive.md#spellcheck) |
| Language detection | [Language detection](competitive.md#language-detection) |
| Script detection | [Script detection](competitive.md#script-detection) |
| Transliteration | [Transliteration](competitive.md#transliteration) |
| POS tagging | [POS tagging](competitive.md#pos-tagging) |
| TF-IDF | [TF-IDF](competitive.md#tf-idf) |
| Classifiers | [Classifiers](competitive.md#classifiers) |

WordNet, sentence analysis and sentiment have no comparable Rust crate to
measure against. The [competitive report](competitive.md) says so explicitly,
naming every candidate it investigated and rejected, rather than forcing an
unfair pairing to fill the row.

## Test environment

| | |
|---|---|
| CPU | Intel Core i9-14900KF (24 cores / 32 threads) |
| Memory | 125 GiB |
| OS | Linux 7.0.11 |
| rustc | 1.97.1, `--release` (`opt-level=3`, `lto="thin"`, `codegen-units=16`) |
| Node.js | v25.9.0, JIT warmed before measurement |
| JavaScript NLP library | v8.1.1 |

These are single-machine numbers. Treat the ratios as indicative and the method
as the thing to copy.

## What makes the comparison fair

**Both sides read the same input files.** `benches/data/` is generated once by
`tools/bench-data/generate.py`. No harness generates its own data, so none can
be tuned to a friendlier distribution.

**The comparison is like-for-like.** The competitive suite asserts that the
implementations produce the same values for the benchmarked inputs before it
times anything. A benchmark whose faster side computes something cheaper is not
a benchmark.

**The JavaScript side is measured warm.** The harness runs a warm-up phase,
then calibrates a batch size large enough to dwarf timer overhead and reports
the best batch — Criterion's own convention. Measuring it cold, still
interpreting every call, would flatter Rust, and that is exactly the
benchmark-gaming this project forbids.

**Small wins are published.** The shortest inputs show ratios close to 1×. At
four characters the work is a handful of comparisons, both runtimes are
dominated by call overhead, and a JIT optimises that shape well. A large
reported gap at that size would be evidence of a rigged benchmark, not of a fast
library.

**Losses are published.** Every capability section shows the rows where another
library is faster, and
[a measured regression and its fix](distance.md#a-measured-regression-and-its-fix)
is written up rather than quietly corrected.

## What has not been measured

No capability's table on these pages carries a memory column: allocation
counts and peak RSS are not part of this section's own suite. The one
instrument that exists lives outside it — `verbora-spellcheck`'s
`counting_alloc`, a `#[cfg(test)]` global allocator its own memory-bound
tests measure peak bytes with. It is scoped to that crate's test build, is
not compiled into any published library, and produced the one set of
peak-RSS figures this project has published:
[Upgrading from 0.1 to 0.2](../getting-started/upgrading.md#resource-behaviour)
records `DeletionIndex` construction falling from 4.0 GB to 32.4 MB of peak
RSS for an 800-scalar token, once its deletion generation moved from cubic to
quadratic in word length.

The data-heavy modules that instrument does not reach — WordNet's index, the
3.9 MB Brill lexicon, the ~7 MB sentiment lexicons — remain unmeasured for
footprint, where it matters at least as much as throughput. The distance
metrics hold no persistent state and have an `O(m)` working set in their fast
paths, so there is little to report for them regardless.

The [allocation reference](../performance/allocation.md) describes what the
rest of the code does structurally rather than what a profiler measured, and
labels itself as such.

## Next

- [Competitive benchmarks](competitive.md) — Verbora against strsim, rapidfuzz,
  tantivy, rust-stemmers and other pinned Rust crates, every loss shown as a
  loss.
- [String distance results](distance.md) — the focused measurements, with the
  analysis.
- [Reproducing them](reproducing.md) — the exact commands.
