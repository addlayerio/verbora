#!/usr/bin/env python3
"""Reads Criterion's raw estimates for every benchmarked (group, competitor,
size) triple and emits:

  results/results.json     -- structured summary (Fase 6's own schema)
  results/raw/<bench>.json -- one raw Criterion estimates.json copy per
                              (group, id) pair, so results.json's numbers can
                              be re-derived, not just trusted.

Deliberately reads Criterion's OWN computed estimates (median, mean, standard
deviation already come from Criterion's own bootstrap/outlier methodology,
documented at criterion.rs) rather than recomputing anything -- this project
already treats Criterion's statistics as authoritative everywhere else (every
crates/verbora-<name> crate's own benches directory), and Fase 6 does not ask
for a second statistics engine, only that the methodology be documented
("Si la herramienta utilizada elimina outliers: documentar la metodologia").

Usage: python3 scripts/collect-results.py <module> <group>:<ids...> [...]
Example:
  python3 scripts/collect-results.py distance \\
    levenshtein:verbora,strsim,rapidfuzz \\
    damerau_levenshtein_unrestricted:verbora,strsim,rapidfuzz \\
    damerau_levenshtein_restricted_osa:verbora,strsim,rapidfuzz \\
    jaro:verbora,strsim,rapidfuzz \\
    jaro_winkler:verbora,strsim,rapidfuzz \\
    hamming:verbora,strsim,rapidfuzz
"""

import json
import datetime
import os
import subprocess
import sys

ROOT = os.path.normpath(os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))


def git_commit():
    """Short commit the numbers were measured at, or None outside a checkout."""
    try:
        out = subprocess.run(
            ["git", "rev-parse", "--short", "HEAD"],
            cwd=ROOT,
            check=True,
            capture_output=True,
            text=True,
        )
    except (subprocess.CalledProcessError, FileNotFoundError):
        return None
    return out.stdout.strip() or None


def criterion_dir():
    meta = json.loads(
        subprocess.run(
            ["cargo", "metadata", "--format-version", "1", "--no-deps"],
            cwd=ROOT,
            check=True,
            capture_output=True,
            text=True,
        ).stdout
    )
    return os.path.join(meta["target_directory"], "criterion")


def min_mtime():
    """Epoch seconds before which a Criterion estimate is considered stale.

    Criterion writes `<group>/<id>/<size>/new/estimates.json` and leaves it
    there forever. A target that fails to build — or that is not run at all —
    therefore leaves its *previous* run's file in place, and a naive collector
    republishes those numbers as if this campaign had produced them. Nothing
    downstream can tell the difference: a results row carries no provenance.

    The campaign driver exports `VERBORA_BENCH_STARTED_AT` before step 5, so
    anything older than the run that is supposed to have produced it is
    refused rather than silently carried forward. Unset means no filtering,
    which keeps ad-hoc invocations working.
    """
    raw = os.environ.get("VERBORA_BENCH_STARTED_AT")
    if not raw:
        return None
    try:
        return float(raw)
    except ValueError:
        print(
            f"warning: VERBORA_BENCH_STARTED_AT={raw!r} is not a number; "
            "collecting without a staleness filter",
            file=sys.stderr,
        )
        return None


STALE = []


def _fresh(path):
    """True when `path` exists and is not older than the current run."""
    if not os.path.exists(path):
        return False
    cutoff = min_mtime()
    if cutoff is not None and os.path.getmtime(path) < cutoff:
        STALE.append(path)
        return False
    return True


def read_estimates(directory, group, ident, size):
    path = os.path.join(directory, group, ident, str(size), "new", "estimates.json")
    if not _fresh(path):
        return None
    with open(path, encoding="utf-8") as fh:
        return json.load(fh)


def read_bare_estimates(directory, group, ident):
    """Same as `read_estimates`, for a bare `bench_function(id, ...)` with no
    `BenchmarkId`/size dimension at all -- `group/id/new/estimates.json`
    directly, one level shallower. Used as the fallback when `list_sizes` finds
    no numeric subdirectories for `ident` (e.g. `classifiers.rs`'s
    `bayes_predict` group, one fixed corpus, no size sweep)."""
    path = os.path.join(directory, group, ident, "new", "estimates.json")
    if not _fresh(path):
        return None
    with open(path, encoding="utf-8") as fh:
        return json.load(fh)


def _size_key(name):
    """Numeric ordering where the directory name is a number, lexicographic
    otherwise -- numeric names always sort before non-numeric ones."""
    try:
        return (0, float(name), "")
    except ValueError:
        return (1, 0.0, name)


def list_sizes(directory, group, ident):
    """Every size Criterion actually recorded for `group`/`ident`, discovered
    from the directories it wrote rather than a hardcoded list.

    The distance module's sizes are string lengths (4..1024); a later module
    (phonetics) measures batches of names (1, 10000, 100000) -- a different
    unit entirely. Rather than growing one hardcoded `SIZES` array with every
    module's own scale (and silently mis-collecting whichever module didn't
    match it), each `(group, ident)` reports its own sizes, so this script
    stays correct for whatever scale a future module's bench file chooses.
    """
    id_dir = os.path.join(directory, group, ident)
    if not os.path.exists(id_dir):
        return []
    out = []
    for entry in os.listdir(id_dir):
        if entry == "report":
            continue
        if not os.path.isdir(os.path.join(id_dir, entry)):
            continue
        if not os.path.exists(os.path.join(id_dir, entry, "new", "estimates.json")):
            continue
        out.append(entry)
    return sorted(out, key=_size_key)


def _implementation(raw_out, raw_file, ident, est):
    with open(os.path.join(raw_out, raw_file), "w", encoding="utf-8") as fh:
        fh.write(json.dumps(est, indent=2))
    return {
        "name": ident,
        "median_ns": est["median"]["point_estimate"],
        "mean_ns": est["mean"]["point_estimate"],
        "stddev_ns": est["std_dev"]["point_estimate"],
        "raw_file": f"raw/{raw_file}",
    }


def main():
    if len(sys.argv) < 3:
        print(
            "Usage: python3 collect-results.py <module> <group>:<id,id,...> [...]",
            file=sys.stderr,
        )
        return 1

    module_name = sys.argv[1]
    specs = sys.argv[2:]

    directory = criterion_dir()
    raw_out = os.path.join(ROOT, "results", "raw")
    os.makedirs(raw_out, exist_ok=True)

    benchmarks = []
    for spec in specs:
        group, _, id_list = spec.partition(":")
        ids = id_list.split(",")

        seen = []
        for ident in ids:
            for size in list_sizes(directory, group, ident):
                if size not in seen:
                    seen.append(size)
        sizes = sorted(seen, key=_size_key)

        # No numeric subdirectory found for any id in this group: either the
        # group genuinely has no size dimension (a bare `bench_function` per
        # id, one fixed workload -- e.g. `bayes_predict`), or nothing has been
        # benchmarked yet. Try the one-level-shallower bare path before giving
        # up on the group entirely.
        if not sizes:
            implementations = []
            for ident in ids:
                est = read_bare_estimates(directory, group, ident)
                if est is None:
                    continue
                raw_file = f"{module_name}-{group}-{ident}.json"
                implementations.append(_implementation(raw_out, raw_file, ident, est))
            if implementations:
                benchmarks.append(
                    {
                        "benchmark": f"{module_name}_{group}",
                        "module": module_name,
                        "group": group,
                        "input_size": None,
                        "implementations": implementations,
                    }
                )
            continue

        for size in sizes:
            implementations = []
            for ident in ids:
                est = read_estimates(directory, group, ident, size)
                if est is None:
                    continue
                raw_file = f"{module_name}-{group}-{ident}-{size}.json"
                implementations.append(_implementation(raw_out, raw_file, ident, est))
            if implementations:
                try:
                    input_size = int(size)
                except ValueError:
                    input_size = size
                benchmarks.append(
                    {
                        "benchmark": f"{module_name}_{group}_{size}",
                        "module": module_name,
                        "group": group,
                        "input_size": input_size,
                        "implementations": implementations,
                    }
                )

    results_path = os.path.join(ROOT, "results", "results.json")
    existing = {"benchmarks": []}
    if os.path.exists(results_path):
        with open(results_path, encoding="utf-8") as fh:
            existing = json.load(fh)

    if not benchmarks:
        # Writing an empty set would *delete* this module's existing rows,
        # because the merge below replaces the module wholesale. A target that
        # failed to build would therefore destroy the evidence it failed to
        # replace -- strictly worse than the stale-data problem this collector
        # was hardened against. Leave the file alone and fail loudly instead.
        print(
            f'error: collected no fresh measurements for module "{module_name}"; '
            "results.json left untouched",
            file=sys.stderr,
        )
        if STALE:
            print(
                f"  {len(STALE)} Criterion estimate(s) were refused as older than "
                "this run -- the benchmark did not produce new data",
                file=sys.stderr,
            )
            for path in STALE[:10]:
                print(f"    {os.path.relpath(path)}", file=sys.stderr)
            if len(STALE) > 10:
                print(f"    ... and {len(STALE) - 10} more", file=sys.stderr)
        else:
            print(
                "  no Criterion output was found at all -- check the bench target "
                "actually ran and the group/id names match",
                file=sys.stderr,
            )
        return 1

    # Stamp every row with the run that produced it. Without this a reader
    # cannot tell a figure measured minutes ago from one carried over from a
    # campaign months and several rewrites earlier -- which is exactly how
    # pre-migration numbers came to sit in this file looking current.
    measured_at = datetime.datetime.now(datetime.timezone.utc).isoformat(
        timespec="seconds"
    )
    commit = git_commit()
    for entry in benchmarks:
        entry["measured_at"] = measured_at
        if commit:
            entry["commit"] = commit

    # Replace any prior entries for this module, keep everything else --
    # running one module's collection must not erase another's results.
    existing["benchmarks"] = [
        b for b in existing["benchmarks"] if b.get("module") != module_name
    ] + benchmarks

    with open(results_path, "w", encoding="utf-8") as fh:
        fh.write(json.dumps(existing, indent=2))

    print(
        f'wrote {len(benchmarks)} benchmark entries for module "{module_name}" '
        "to results/results.json"
    )

    if STALE:
        # Refused, not collected. Saying so is the point: a module that
        # measured nothing must not look like one that measured zero change.
        print(
            f"warning: refused {len(STALE)} Criterion estimate(s) older than this "
            f"run -- those benchmarks produced no fresh measurement:",
            file=sys.stderr,
        )
        for path in STALE[:10]:
            print(f"  {os.path.relpath(path)}", file=sys.stderr)
        if len(STALE) > 10:
            print(f"  ... and {len(STALE) - 10} more", file=sys.stderr)

    return 0


if __name__ == "__main__":
    sys.exit(main())
