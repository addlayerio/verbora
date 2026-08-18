# Reproducing the benchmarks

Every number in this section can be regenerated from a clean checkout. If one
cannot be reproduced, that is a bug.

## Prerequisites

| | |
|---|---|
| Rust | 1.85+ (the workspace is edition 2024) |
| Python | 3.8+ (generates the shared benchmark inputs) |

```bash
git clone https://github.com/addlayerio/verbora
cd verbora
```

## 1. Generate the shared inputs

Every harness reads the same files. Run this once:

```bash
python3 tools/bench-data/generate.py
```

It writes `benches/data/`:

```text
benches/data/words.json            word lists at several lengths
benches/data/distance-pairs.json   the string pairs the metrics compare
```

No harness generates its own inputs, so none can be tuned to a distribution
that flatters it.

## 2. Run a crate's own benchmarks

```bash
cargo bench -p verbora-distance
```

Criterion writes HTML reports to `target/criterion/` and stores a baseline it
compares against on the next run. The other benched crates:

```bash
cargo bench -p verbora-inflectors
cargo bench -p verbora-ngrams
cargo bench -p verbora-normalizers
cargo bench -p verbora-phonetics
cargo bench -p verbora-tokenizers
cargo bench -p verbora-trie
```

## 3. Run the competitive suite

Head-to-head against the pinned third-party crates, with its own structured
results:

```bash
./scripts/competitive-benchmarks.sh              # every module
./scripts/competitive-benchmarks.sh distance     # one module
```

It writes:

```text
benchmarks/competitive/results/results.json      structured summary
benchmarks/competitive/results/raw/              Criterion's own estimates
benchmarks/competitive/results/metadata.json     machine and toolchain
```

The raw estimates are kept so the summary can be re-derived rather than taken on
trust.

## Comparing two runs

Criterion compares against its stored baseline automatically and reports
"Performance has improved" or "has regressed" per benchmark. To name a baseline
explicitly:

```bash
cargo bench -p verbora-distance -- --save-baseline before
# ... make a change ...
cargo bench -p verbora-distance -- --baseline before
```

This is how [the Jaro–Winkler
fix](distance.md#a-measured-regression-and-its-fix) was confirmed. Small
movements between runs are noise and are not gated in CI; material changes to
tokenizer throughput, stemming, Levenshtein, TF-IDF, WordNet lookup, classifier
prediction and sentiment analysis are recorded with the benchmark result and the
commit that caused them.

## Profiling

The `bench` profile inherits `release` and adds debug symbols, so `perf` and
`samply` resolve frames:

```bash
cargo bench -p verbora-distance --no-run
perf record -g ./target/release/deps/distance-<hash> --bench
perf report
```

For maximum runtime speed at a significant compile-time cost:

```bash
cargo bench --profile release-max -p verbora-distance
```

Published tables use the ordinary `release` settings (`opt-level = 3`,
`lto = "thin"`, `codegen-units = 16`), because that is what most people build.

## Before you trust a result

**Re-run the test suite.** A benchmark whose faster side computes something
cheaper is not a benchmark:

```bash
cargo test --workspace
```

**Quiet the machine.** Close other work, and prefer a fixed CPU governor. On
Linux:

```bash
sudo cpupower frequency-set --governor performance
```

**Run it more than once.** Criterion's confidence intervals tell you whether a
difference is real.

**Check the input size.** A ratio measured on four-character strings and a ratio
measured on 1024-character strings are both true statements about the same
function, and they can differ by orders of magnitude. Quoting either alone is
misleading.

## Adding a benchmark

1. Add the Criterion group to `crates/<crate>/benches/<name>.rs`.
2. If a competitor exists, add it to
   `benchmarks/competitive/rust-competitors/benches/<name>.rs` and register the
   module in `scripts/competitive-benchmarks.sh`'s `MODULE_SPECS`.
3. If new inputs are needed, generate them in `tools/bench-data/generate.py`, so
   every harness reads the same bytes.
4. Run it, and publish the reviewed result with the hardware, the toolchain
   versions and the commands — see
   [Documentation is part of the code](../reference/docs-are-code.md).
