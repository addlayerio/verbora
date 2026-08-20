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

## Migration debt: what the text-shaping migration removed from this harness

`docs/design/text-shaping-contract.md` deleted a large part of
`verbora-tokenizers`' and `verbora-normalizers`' public surface, and
`docs/design/rust-native-migration.md`'s earlier `verbora-distance` step made
that crate's `levenshtein` module private. Eight targets here stopped
compiling as a result, and nothing in the tree recorded it — the campaign
entrypoint (`scripts/competitive-benchmarks.sh`) died before measuring
anything. This section is that record.

**Benchmark groups deleted, because Verbora no longer has the capability.**
Each one is a `docs/COMPETITIVE_BENCHMARKS.md` §1.1/§1.4 row that now
describes something Verbora does not do; the matrix has not been updated to
match, and that is open documentation debt.

| Group | Was | Why it is gone |
|---|---|---|
| `tokenizers::whitespace_tokenization` | `RegexpTokenizer(\s+)` vs `tantivy::WhitespaceTokenizer` vs HF `WhitespaceSplit` | `RegexpTokenizer`/`Pattern` deleted (contract §3.4), along with `verbora-tokenizers`' `regex` dependency. Verbora performs no whitespace or regex tokenization at any API. **Coverage lost:** both competitor rows now have no Verbora counterpart. |
| `tokenizers::aggressive_tokenization_en` | `AggressiveTokenizer` (en) vs `unicode_words()` | `AggressiveTokenizer` and its 15 variants deleted (§3.4; §4.1 for why). **No coverage lost:** `WordTokenizer` *is* `unicode_words()` and carries the comparison. |
| `normalizers::ja_hiragana_to_katakana`, `normalizers::ja_katakana_to_hiragana` | `ja::converters` vs `unicode-jp` | `ja::converters` deleted in full (§3.2). Kana ↔ kana conversion is a transliteration, not a normalization, and `verbora-transliterators` ships only kana → romaji. **Coverage lost:** the `unicode-jp` rows have no Verbora side; the dependency was dropped. |

**Benchmark groups re-pointed, because the capability survives elsewhere.**

| Group | Now | Comparability note |
|---|---|---|
| `normalizers::nfkc_halfwidth_katakana` (was `ja_katakana_halfwidth_to_fullwidth`) | `nfkc` vs `kana-converter` | NFKC is the general UAX #15 form and does categorically more than a halfwidth-kana table. Fair *for this workload only*; the input domain is narrowed and every excluded divergence is asserted in `tests/normalizers_correctness.rs`. |
| `distance` (all groups) | `verbora_distance::{levenshtein, …}` at the crate root | Import path only; the functions are unchanged re-exports. |

**Rival comparisons split from wrapper-overhead baselines.** `WordTokenizer::tokens`
is literally `str::unicode_words()` and `SentenceTokenizer::tokens` is built on
`str::split_sentence_bound_indices()`, so Verbora now *delegates to*
`unicode-segmentation` rather than competing with it. Those rows moved into
explicit `word_tokenization_wrapper_overhead` and
`sentence_tokenization_wrapper_overhead` groups, following
`benches/language.rs`'s `whatlang_wrapper_overhead` precedent. They report
what the wrapper costs; they are **never** to be published as Verbora beating
or losing to `unicode-segmentation`.

**Every `tokenizers` and `normalizers` figure in `results/` is stale.** Not
approximately right — stale. `WordTokenizer`, `SentenceTokenizer` and
`remove_diacritics` were all reimplemented, and the other rows measure
functions that no longer exist. Per `CLAUDE.md`, no number here may be
republished from anything but a fresh full run. The affected files are
`results/results.json`'s `tokenizers`/`normalizers` entries and
`results/raw/tokenizers-*`, `results/raw/normalizers-*`. They are left in
place as a record of a past run of past code, not as current results.

**Open follow-up: `unaccent` 0.1.1.** It was rejected as a `remove_diacritics`
competitor *because* it decomposes and Verbora did not. Verbora now decomposes,
so `unaccent` has become the mechanism-matched candidate and `diacritics`
0.2.2 the mechanically-opposite one. Pinning it is blocked on the licence
review `AGENTS.md` § Licensing requires (crates.io flags its license field
"non-standard").

## Resolved: `eddie` 0.4.2 is unsound, and is now contained

Repairing `tests/distance_correctness.rs`' import path made the target compile
for the first time since the `verbora-distance` migration — and it then
**aborted the whole test process** in a debug build:

```
$ cd benchmarks/competitive && cargo test --test distance_correctness
thread '…' panicked at eddie-0.4.2/src/utils/buffer.rs:26:31:
unsafe precondition(s) violated: slice::get_unchecked_mut requires that the index
is within the slice
thread caused non-unwinding panic. aborting.
… signal: 6, SIGABRT
```

Not input-dependent, not a Verbora defect. `eddie-0.4.2`'s `utils/buffer.rs`
`Buffer::store` does `buf.clear()` (length becomes `0`), then writes through
`buf.get_unchecked_mut(i)` for `i = 0, 1, 2, …`, and calls `set_len` only
afterwards — every write indexes a slice whose length is still `0`. Rust's
standard library checks that precondition when the *calling* crate has debug
assertions on; release builds compile the check out, which hides the abort but
not the undefined behaviour.

### What was actually established, by enumeration

Every public `eddie` entry point was called in its own process, so an abort
could only kill that one probe:

| Entry point | Result |
|---|---|
| `eddie::{Levenshtein, DamerauLevenshtein}::{distance, rel_dist, similarity}` | **UB / abort** |
| `eddie::{Jaro, JaroWinkler}::{similarity, rel_dist}` | **UB / abort** |
| `eddie::slice::Levenshtein::distance` | **UB / abort** |
| `eddie::Hamming::{distance, rel_dist, similarity}` | sound |
| `eddie::slice::{DamerauLevenshtein, Hamming}::distance` | sound |
| `eddie::slice::{Jaro, JaroWinkler}::similarity` | sound |

So the blast radius is wider than the Jaro family: it is every `str`-level
metric except `Hamming`, plus slice-level `Levenshtein`. All of them share
`Buffer::store`. Slice-level `Jaro`/`JaroWinkler` never touch it and reach
**zero** `unsafe` on their whole call graph.

`0.4.2` is also the **latest** published version (13 versions, newest
2020-01-18), so "pin or upgrade to a sound release" was not available.

### The decision: isolate for correctness, drop from timing

**Correctness — `eddie` kept, through its slice API only.** `eddie`'s `str`
Jaro is literally `buffer.store(s.chars())` followed by the slice call, so
collecting the `char`s ourselves computes the same function without the
unsound buffer. `tests/distance_correctness.rs`'s `eddie_slice` module is the
only sanctioned access path, `eddie` stays a **dev**-dependency so `src/`
cannot reach it, and
`every_reference_to_eddie_goes_through_the_sound_slice_wrapper` walks every
`.rs` file in the crate and fails the suite if any other `eddie` path appears
in code.

Dropping it outright was considered and rejected, because of what the
verification turned up: **`eddie` is the only Jaro implementation here that
computes the function Verbora specifies.** `docs/design/distance-contract.md`
§3.4 makes `t` exactly half the raw transposition count ("an odd raw count
contributes `x.5`"); `strsim` 0.11.1 and `rapidfuzz` 0.5.0 both truncate with
integer division. Measured over 82,000 random pairs: `eddie` agrees with
Verbora on all of them, `strsim` and `rapidfuzz` disagree on **23,428
(28.6%)**, from operand length 6 upward — inside the benchmarked corpus, and
reproducible on the timed `"<n>-near"` shape at n=64. Minimal fixture:
`jaro("abccba", "abbaca")` is `0.788…` for Verbora and `eddie`, `0.822…` for
the other two.

**Timing — `eddie` removed, and it may not come back.** A timing row must call
the competitor's published API as published, and `eddie`'s published `str` API
is undefined behaviour on every call; a benchmark against UB measures nothing
reportable. Timing the slice wrapper instead would be worse: it would hand
`eddie` pre-decoded `Vec<char>` operands while Verbora's `jaro(&str, &str)`
decodes scalars inside the timed region — exactly the "excluding real costs
from only one implementation" that `AGENTS.md`
§ *Cross-Implementation Benchmark Fairness* forbids. `benches/distance.rs` and
`examples/distance_memory.rs` therefore carry no `eddie` row.

**Coverage lost:** the `eddie` *timing* and *memory* rows for `jaro` and
`jaro_winkler`. `scripts/competitive-benchmarks.sh`'s `MODULE_SPECS` never
collected them (`jaro:verbora,strsim,rapidfuzz`), so `results/results.json`
loses nothing on the next run. The `"eddie"` rows currently in
`results/results.json` and `results/distance-memory.json` were produced by a
release build that executed UB; they are **retired**, and disappear on the
next run rather than being hand-edited out of a machine-generated file.

**Status: `cargo test` in this workspace passes in debug**, 15/15 on
`tests/distance_correctness.rs`. Nothing is `#[ignore]`d.

### The larger finding this uncovered

The two remaining Jaro competitors compute a different function from Verbora
in *two* independent ways — the transposition truncation above, and the
Winkler boost, which §3.4 applies unconditionally while both competitors gate
it behind `sim > 0.7`. `docs/COMPETITIVE_BENCHMARKS.md` §1.8 marks all four
rows `Yes` for algorithmic equivalence, which is wrong on both counts. Neither
invalidates a timing row (the work is comparable), but the `Yes` verdicts need
amending before any of those numbers is published as like-for-like. That file
is outside this harness's ownership; the divergences are pinned as fixtures in
`tests/distance_correctness.rs` so the correction cannot be lost.

A third, smaller correction landed in the same pass: this harness used to
claim that Verbora's match-window clamp made it diverge from `strsim` and
`rapidfuzz` on `jaro("a", "a")`. It does not — `strsim` clamps with
`saturating_sub(1)` and all three return `1.0`. Nothing had ever asserted it.
The degenerate row now enumerates all four implementations.
