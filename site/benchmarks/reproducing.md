# Reproducing the benchmarks

Everything on the [results page](distance.md) can be regenerated from a clean
checkout. If a number here cannot be reproduced, that is a bug.

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

## 2. The Rust side

```bash
cargo bench -p verbora-distance
```

Criterion writes HTML reports to `target/criterion/` and stores a baseline it
will compare against on the next run.

Other crates:

```bash
cargo bench -p verbora-inflectors
cargo bench -p verbora-ngrams
cargo bench -p verbora-normalizers
cargo bench -p verbora-phonetics
cargo bench -p verbora-tokenizers
cargo bench -p verbora-trie
```

## 3. The competitive suite

Head-to-head against pinned third-party Rust crates, with its own structured
results:

```bash
./scripts/competitive-benchmarks.sh              # every module
./scripts/competitive-benchmarks.sh distance     # one module
```

It writes `benchmarks/competitive/results/results.json` (structured summary),
`results/raw/` (Criterion's own estimates, so the summary can be re-derived)
and `results/metadata.json` (machine and toolchain attribution).

## Profiles

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

The published table uses the ordinary `release` settings (`opt-level = 3`,
`lto = "thin"`, `codegen-units = 16`), because that is what most users build.

## Comparing two of your own runs

Criterion compares against its stored baseline automatically. To name a baseline
explicitly:

```bash
cargo bench -p verbora-distance -- --save-baseline before
# ... make a change ...
cargo bench -p verbora-distance -- --baseline before
```

It will report "Performance has improved" or "has regressed" per benchmark. That
is how the Jaro–Winkler fix was confirmed.

## Before you trust a result

**Re-run the test suite.** A benchmark that computes something cheaper is not a
benchmark:

```bash
cargo test --workspace
```

**Quiet the machine.** Close other work, and prefer a fixed CPU governor. On
Linux:

```bash
sudo cpupower frequency-set --governor performance
```

**Run it more than once.** Criterion's confidence intervals tell you whether a
difference is real. Small movements are noise.

**Check the input sizes.** A 53.8× ratio at four characters and a 3307.7× ratio
at 1024 are both true statements about the same function. Quoting either alone
is misleading.

## Adding a benchmark

1. Add the Criterion group to `crates/<crate>/benches/<name>.rs`.
2. If a competitor exists, add it to
   `benchmarks/competitive/rust-competitors/benches/<name>.rs` and register the
   module in `scripts/competitive-benchmarks.sh`'s `MODULE_SPECS`.
3. If new inputs are needed, generate them in `tools/bench-data/generate.py` so
   every harness reads the same bytes.
4. Run it, and put the table in `docs/PERFORMANCE.md` **and** on this site —
   with the hardware, toolchain versions and commands.

See [Documentation is part of the code](../reference/docs-are-code.md) for why
step 4 is not optional.
