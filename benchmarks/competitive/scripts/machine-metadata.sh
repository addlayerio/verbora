#!/usr/bin/env bash
# Captures the machine/software metadata every competitive benchmark run
# must be attributed to, per Fase 6 Benchmark.md's MACHINE METADATA section.
# Writes benchmarks/competitive/results/metadata.json.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

VERBORA_COMMIT=$(git -C ../.. rev-parse HEAD)
# The root manifest is a *virtual* workspace: it has no top-level `version`,
# so the original `grep -m1 '^version' ../../Cargo.toml` matched nothing and,
# under `set -o pipefail`, took the whole script — and with it the machine
# metadata no published number may go without — down with exit 1. Read the
# version from a real member crate instead, which is where it actually lives.
VERBORA_VERSION=$(grep -m1 '^version' ../../crates/verbora-distance/Cargo.toml | sed -E 's/.*"(.+)".*/\1/')
RUSTC_VERSION=$(rustc --version)
CPU_MODEL=$(grep -m1 'model name' /proc/cpuinfo | sed -E 's/model name\s*:\s*//')
CPU_CORES=$(nproc)
RAM_TOTAL=$(free -h | awk '/^Mem:/ {print $2}')
OS_NAME=$(uname -s)
KERNEL=$(uname -r)

mkdir -p results
python3 - "$VERBORA_COMMIT" "$VERBORA_VERSION" "$RUSTC_VERSION" \
         "$CPU_MODEL" "$CPU_CORES" "$RAM_TOTAL" "$OS_NAME" "$KERNEL" <<'EOF' > results/metadata.json
import json, sys, datetime
(_, commit, version, rustc, cpu, cores, ram, os_name, kernel) = sys.argv
print(json.dumps({
    "timestamp": datetime.datetime.now(datetime.timezone.utc).isoformat(),
    "verbora_commit": commit,
    "verbora_version": version,
    "rustc": rustc,
    "cpu": cpu,
    "cores": int(cores),
    "ram": ram,
    "os": os_name,
    "kernel": kernel,
    "note": "CPU affinity pinning, thermal monitoring, and Docker were "
            "evaluated (Fase 6 Benchmark.md marks all three optional -- "
            "'cuando sea posible' / 'evaluar si ayuda') and deliberately not "
            "built: this is a single dedicated run on one physical machine "
            "with no other significant workload active, which the spec "
            "accepts as sufficient ('usar la misma maquina; cerrar workloads "
            "externos significativos')."
}, indent=2))
EOF

echo "wrote results/metadata.json"
cat results/metadata.json
