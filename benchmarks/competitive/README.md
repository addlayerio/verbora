# Competitive benchmarks

Fase 6's competitive performance audit: Verbora against real, pinned,
widely-adopted third-party libraries — Rust crates and the reference — running
**exactly the same work on exactly the same input**. See
`docs/COMPETITIVE_BENCHMARKS.md` for the full research matrix (which
competitors were selected, and why) and `Fase 6 Benchmark.md` (repo root) for
the governing spec this directory implements.

## Why this lives outside the main Cargo workspace

The root `Cargo.toml`'s `[workspace] members` lists only the 21 published
`verbora-*` crates. This directory is deliberately **not** one of them, and
has its own `[workspace]` (`Cargo.toml` here). A published library crate's
`Cargo.toml` must never grow a dependency on a rival implementation just
because a benchmark exists for it — that would leak competitor crates into
every downstream consumer's dependency tree for no reason they asked for.
`rust-competitors/` depends on the real `verbora-*` crates via `path = "../../crates/..."` (the genuine published code, not a copy) *and* on real,
version-pinned third-party crates — isolated here, nowhere else.

## Layout

```
benchmarks/competitive/
├── README.md              (this file)
├── Cargo.toml              its own [workspace], NOT part of the root one
├── Cargo.lock               COMMITTED (see below) — pins every competitor
├── rust-competitors/        one Criterion bench file per module, e.g.
│   ├── Cargo.toml            benches/distance.rs, benches/phonetics.rs, ...
│   └── benches/*.rs          each pitting Verbora against its real, pinned
│                              Rust competitor(s) on shared input
├── manifests/
│   └── competitors.json     machine-readable competitor manifest (name,
│                              language, version, repo, license) — generated
│                              from docs/COMPETITIVE_BENCHMARKS.md's matrix
├── scripts/
│   ├── machine-metadata.sh  writes results/metadata.json
│   ├── fetch-models.sh      fetches third-party model/dictionary assets
│   │                          (POS-tagging, spellcheck) too large/separately
│   │                          licensed to vendor — see below
│   └── collect-results.py   reads Criterion's estimates.json for a module's
│                              benchmarks, writes results/results.json +
│                              results/raw/*.json
├── models/                  fetched third-party assets (gitignored) — a
│                              pretrained POS-tagging model, a MobileBERT
│                              checkpoint, a Hunspell dictionary; populated by
│                              scripts/fetch-models.sh, never committed
└── results/
    ├── metadata.json        machine/software metadata for the current run
    ├── results.json         structured summary (Fase 6's own schema)
    └── raw/                 one raw Criterion estimates.json per (module,
                               group, competitor, size) — so every number in
                               results.json can be re-derived, not just
                               trusted; never hand-edit these
```

The **the reference side is not duplicated here.** It already has a proven,
working harness of its own (warmup, calibrated
batch size — see `docs/PERFORMANCE.md`'s own "Methodology" section for why
this design is fair to both a JIT and an AOT-compiled competitor). This
directory extends that existing harness with new per-module scripts where
one doesn't exist yet, rather than rebuilding it.

## Adding a new module's competitive benchmark

1. Confirm the competitor(s) and exact pinned version(s) in
   `docs/COMPETITIVE_BENCHMARKS.md`'s matrix (§1) — never add a dependency
   that isn't already recorded there with its research dossier.
2. Add the competitor crate(s) to `rust-competitors/Cargo.toml` under
   `[dev-dependencies]`, pinned with `=x.y.z` (exact version — no implicit
   `latest`, no `^`/`~` ranges that could silently drift).
3. Add the Verbora crate itself to `rust-competitors/Cargo.toml` under
   `[dependencies]` if it isn't already there (path dependency).
4. Write `rust-competitors/benches/<module>.rs`. Follow
   `benches/distance.rs`'s pattern exactly:
   - Read the *same* input data the in-workspace Verbora bench and (where one
     exists) the reference harness already read from `benches/data/*.json`
     — generate new shared data via `tools/bench-data/generate.py` if a
     module needs data that doesn't exist yet, rather than inventing a
     separate dataset only this benchmark sees.
   - One Criterion `benchmark_group` per algorithm/sub-capability (never mix
     semantically different operations in one group).
   - Every implementation wrapped in `black_box` on both input and output.
   - A module doc comment stating exactly which matrix rows are and are not
     benchmarked here, and why (mirror `benches/distance.rs`'s own — e.g. it
     explains why Sørensen-Dice is excluded despite candidates existing).
   - Only benchmark rows the matrix marks `Yes` or `Selected cases`/`Partial`
     with a **documented, narrowed, genuinely-fair input domain** — never a
     row marked `No`.
5. Add the new `[[bench]]` entry to `rust-competitors/Cargo.toml`.
6. Run it, then run `python3 scripts/collect-results.py <module> <group>:<id,id,...> ...` to populate `results/results.json` and `results/raw/`.
7. If a competitor's output needs correctness-checking against Verbora's
   before its *speed* is trusted (per the spec's `CORRECTNESS BEFORE
   PERFORMANCE` rule), add that check as its own `#[test]` in the same crate
   — not inside the benchmark itself.

## Running

```bash
python3 ../../tools/bench-data/generate.py   # shared inputs, if not already generated
./scripts/fetch-models.sh                 # third-party model/dictionary assets, if not already fetched
cargo bench --release                     # every module's rust-competitors benches
./scripts/machine-metadata.sh             # results/metadata.json
python3 scripts/collect-results.py <module> <group>:<ids> ...   # per module, after its bench runs
```

### Fetched model/dictionary assets

Two modules need a real third-party asset too large or too separately
licensed to vendor into this repository, the same reasoning
`crates/verbora-wordnet` already applies to the WordNet database itself:

- **POS tagging** (`benches/pos_tagging.rs`) — `postagger`'s pretrained
  averaged-perceptron weights (not published on crates.io) and `rust-bert`'s
  MobileBERT English POS checkpoint.
- **Spellcheck** (`benches/spellcheck.rs`) — a real Hunspell `en_US`
  `.aff`/`.dic` pair for `spellbook`.

Run `./scripts/fetch-models.sh` once (or `./scripts/fetch-models.sh
<postagger|rust-bert-pos|hunspell-en-us>` for just one) to populate
`models/` (gitignored). Every bench group that needs one of these skips
cleanly, with a printed notice, if it has not been fetched — a missing
licence-restricted asset never fails `cargo bench` for everyone else's
groups.

`../../scripts/competitive-benchmarks.sh` (repo root `scripts/`) is the
single top-level reproducibility entrypoint the spec's `REPRODUCIBILITY`
section asks for — it drives all of the above plus the reference harness
and accuracy scripts in one command.

## Cargo.lock is committed here (unlike the root workspace's)

The root workspace's `Cargo.lock` is gitignored — standard practice for a
published *library*, where downstream consumers resolve their own versions.
This directory is the opposite case: it is a fixed, reproducible **benchmark
application** whose entire point is measuring specific, pinned competitor
versions. Its `Cargo.lock` is committed (see the root `.gitignore`'s
exception for this one path) so `cargo bench` here reproduces the exact same
dependency graph on any machine, per the spec's own `VERSION PINNING`
section ("Generar un lockfile reproducible").

## Fairness discipline

Every file here answers to the same rules as the rest of this project's
benchmarking (`AGENTS.md`'s `# Performance Evidence Requirement`, extended by
Fase 6's own policy additions) plus these, specific to *competitive*
benchmarking:

- **No cherry-picking.** A benchmark that runs and shows Verbora losing gets
  published exactly like one where it wins — see `docs/PERFORMANCE_GAPS.md`.
- **Same input, same work.** Every implementation in a given benchmark group
  reads the literal same bytes/strings and answers the literal same
  question. A row exists in `docs/COMPETITIVE_BENCHMARKS.md` marked `Yes` or
  a narrowed, honestly-documented `Selected cases` before it gets a
  benchmark here — never the reverse.
- **Independent fairness audit.** Nothing here is considered publishable
  until a dedicated, skeptical audit (mirroring `Fase 6 Benchmark.md`'s own
  `RESULT VALIDATION AGENT` section) has tried to find a reason each specific
  benchmark is unfair and failed to. See `docs/COMPETITIVE_BENCHMARKS.md`'s
  own audit trail once that pass exists.
