#!/usr/bin/env bash
# Single reproducibility entrypoint for Fase 6's competitive benchmark suite.
# Per Fase 6 Benchmark.md's REPRODUCIBILITY section: prepares dependencies,
# builds release, runs benchmarks, saves raw data, regenerates the
# structured summary — no hidden manual steps.
#
# Usage: ./scripts/competitive-benchmarks.sh [module ...]
#   No args: runs every module listed in MODULES below.
#   One or more module names: runs only those (e.g. "distance phonetics").
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
ROOT="$(pwd)"
COMPETITIVE="$ROOT/benchmarks/competitive"

# Each entry: module name, then the collect-results.py group:ids specs for
# that module's rust-competitors bench. Extend this array as new modules
# gain a benchmarks/competitive/rust-competitors/benches/<module>.rs file —
# see benchmarks/competitive/README.md's "Adding a new module" section.
declare -A MODULE_SPECS=(
  [distance]="levenshtein:verbora,strsim,rapidfuzz damerau_levenshtein_unrestricted:verbora,strsim,rapidfuzz damerau_levenshtein_restricted_osa:verbora,strsim,rapidfuzz jaro:verbora,strsim,rapidfuzz jaro_winkler:verbora,strsim,rapidfuzz hamming:verbora,strsim,rapidfuzz"
  [tokenizers]="whitespace_tokenization:verbora,tantivy,huggingface word_tokenization:verbora,tantivy,huggingface"
)

MODULES=("${@:-${!MODULE_SPECS[@]}}")

echo "== 1. Shared benchmark inputs =="
python3 "$ROOT/tools/bench-data/generate.py"

echo "== 2. Machine metadata =="
( cd "$COMPETITIVE" && ./scripts/machine-metadata.sh )

echo "== 3. Rust competitors (this workspace) =="
( cd "$COMPETITIVE" && cargo bench --release )

echo "== 4. Verbora's own in-workspace benches =="
for m in "${MODULES[@]}"; do
  echo "-- verbora: $m --"
  cargo bench -p "verbora-$m" --bench "$m" || echo "   (no crates/verbora-$m/benches/$m.rs — check the crate's real bench file name)"
done

echo "== 5. Collect structured results =="
for m in "${MODULES[@]}"; do
  spec="${MODULE_SPECS[$m]:-}"
  if [ -n "$spec" ]; then
    ( cd "$COMPETITIVE" && python3 scripts/collect-results.py "$m" $spec )
  fi
done

echo
echo "Done. Structured results: $COMPETITIVE/results/results.json"
echo "Raw results:              $COMPETITIVE/results/raw/"
echo "Machine metadata:         $COMPETITIVE/results/metadata.json"
echo "Next: run the independent fairness audit before publishing any number."
