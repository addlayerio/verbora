# Benchmark method

Every number published on this site is reproducible with the commands given.
Nothing is estimated, and the results that went the wrong way are published next
to the ones that went the right way.

## What has been measured

<div class="callout callout-warn">
<strong>This page's own detailed results, so far: <code>verbora-distance</code>
vs. a widely-used JavaScript NLP library.</strong> 26 benchmarks against that
library on identical inputs — see <a href="distance">String distance
results</a>. Every crate in the workspace now has a comparison against it
recorded in <code>docs/PERFORMANCE.md</code>'s own
<code>## Results — &lt;crate&gt;</code> sections (generated from the recorded
Criterion estimates, not hand-typed), but this site has migrated only
distance's table across so far — the rest are linked from there rather than
duplicated here. Separately, <a href="competitive">Competitive
benchmarks</a> covers something this table does not: Verbora against real,
version-pinned <strong>Rust</strong> competitors — strsim, rapidfuzz,
tantivy, rust-stemmers and a dozen others — not just that one JavaScript
library.
</div>

| Crate | Criterion benches | JS baseline recorded | JS comparison | Rust-competitor comparison |
|---|:--:|:--:|:--:|:--:|
| `verbora-distance` | ✅ | ✅ | ✅ [site results](distance.md) | ✅ [site results](competitive.md#distance) |
| `verbora-inflectors` | ✅ | ✅ | ✅ [docs/PERFORMANCE.md](https://github.com/addlayerio/verbora/blob/main/docs/PERFORMANCE.md#results--verbora-inflectors) | ✅ [site results](competitive.md#inflectors) |
| `verbora-ngrams` | ✅ | ✅ | ✅ [docs/PERFORMANCE.md](https://github.com/addlayerio/verbora/blob/main/docs/PERFORMANCE.md#results--verbora-ngrams) | ✅ [site results](competitive.md#n-grams) |
| `verbora-normalizers` | ✅ | ✅ | ✅ [docs/PERFORMANCE.md](https://github.com/addlayerio/verbora/blob/main/docs/PERFORMANCE.md#results--verbora-normalizers) | ✅ [site results](competitive.md#normalizers) |
| `verbora-phonetics` | ✅ | ✅ | ✅ [docs/PERFORMANCE.md](https://github.com/addlayerio/verbora/blob/main/docs/PERFORMANCE.md#results--verbora-phonetics) | ✅ [site results](competitive.md#phonetics) |
| `verbora-trie` | ✅ | ✅ | ✅ [docs/PERFORMANCE.md](https://github.com/addlayerio/verbora/blob/main/docs/PERFORMANCE.md#results--verbora-trie) | ✅ [site results](competitive.md#trie) |
| `verbora-tokenizers` | ✅ | ✅ | ✅ [docs/PERFORMANCE.md](https://github.com/addlayerio/verbora/blob/main/docs/PERFORMANCE.md#results--verbora-tokenizers) | ✅ [site results](competitive.md#tokenizers) |

## Test environment

| | |
|---|---|
| CPU | Intel Core i9-14900KF (24 cores / 32 threads) |
| Memory | 125 GiB |
| OS | Linux 7.0.11 |
| rustc | 1.97.1, `--release` (`opt-level=3`, `lto="thin"`, `codegen-units=16`) |
| Node.js | v25.9.0, JIT warmed before measurement |
| JavaScript NLP library | v8.1.1 |

Single-machine numbers. Treat the ratios as indicative and the method as the
thing to copy.

## The rules

**Both sides read the same input files.** `benches/data/` is generated once by
`tools/bench-data/generate.py`. Neither implementation generates its own data, so
neither can be tuned to a friendlier distribution.

**The JavaScript library is measured warm.** The harness runs a warm-up phase
before measuring, then calibrates a batch size large enough to dwarf timer
overhead and reports the best batch — matching Criterion's convention.
Measuring it cold, still interpreting each call, would flatter Rust, and is
exactly the benchmark-gaming this project forbids.

**Both sides produce the same values.** The test suite asserts that on
recorded cases. A benchmark that computes something cheaper is not a benchmark.

**Small wins are published.** `hamming/4` at 1.4× and `jaro_winkler/4` at 1.8×
are in the table. At four characters the work is a handful of comparisons, both
runtimes are dominated by call overhead, and the JavaScript engine optimises
that shape very well. A large reported gap at that size would be evidence of a
rigged benchmark, not of a fast library.

**Regressions are published.** See
[the Jaro–Winkler story](distance.md#a-measured-regression-and-its-fix).

## The cycle the project mandates

```text
benchmark → profile → optimise → unit tests → full suite → benchmark again
```

No optimisation lands without the test suite re-run, and none is claimed
without a measurement. The second half of that rule has already caught one
regression and one "optimisation" that was 12% slower.

## Regression tracking

Criterion stores baselines under `$CARGO_TARGET_DIR/criterion/` and reports
changes between runs — it flagged the Jaro–Winkler fix as "Performance has
improved" automatically. Small movements are noise and are not gated in CI.

Material changes to tokenizer throughput, stemming throughput, Levenshtein,
TF-IDF, WordNet lookup, classifier prediction and sentiment analysis are meant to
be recorded in `docs/PERFORMANCE.md` with the commit that caused them.

## Memory

Not yet instrumented. Planned, per the project charter: allocation counts and
peak RSS for the data-heavy modules — WordNet's index, the 3.9 MB Brill lexicon,
the ~7 MB sentiment lexicons — where footprint matters at least as much as
throughput. The distance metrics hold no persistent state and have an `O(m)`
working set in their fast paths, so there is little to report for them.

Until that exists, this site's [allocation reference](../performance/allocation.md)
describes what the code does structurally rather than what a profiler measured,
and labels itself accordingly.

## Next

- [String distance results](distance.md) — all 26 benchmarks, with the analysis.
- [Competitive benchmarks](competitive.md) — Verbora against strsim, rapidfuzz,
  tantivy, rust-stemmers and 20 other real Rust crates, plus a JavaScript NLP
  library where no Rust competitor exists — 205 comparisons, every loss shown
  as a loss.
- [Reproducing them](reproducing.md) — the exact commands.
