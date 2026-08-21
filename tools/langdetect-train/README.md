# langdetect-train

Training pipeline for `verbora-language`'s `HashedLinearDetector`
(`fast-language-detection` feature): corpus preparation, multinomial
logistic-regression training, abstention calibration, weight-table
codegen, and evaluation.

This tool is deliberately **not** a member of the repository workspace
(see the `[workspace]` table in its `Cargo.toml` for why). Nothing here
ships; its output is the generated source file
`crates/verbora-language/src/hashed_linear_weights.rs` plus the
`manifest.json` committed next to this README.

## Pipeline

```bash
# 1. Fetch the Tatoeba per-language exports (~95 MB total) for the 18
#    statistically-modeled languages:
mkdir -p /path/to/corpus && cd /path/to/corpus
for l in eng spa por ita fra deu nld pol ind vie nob swe fin glg cat eus rus ukr; do
  curl -s -O "https://downloads.tatoeba.org/exports/per_language/$l/${l}_sentences.tsv.bz2"
done

# 2. Filter/dedup/split (needs `bzcat` and `sha256sum` on PATH):
cargo run --release -- prepare --corpus-dir /path/to/corpus --data-dir /path/to/workdir

# 3. Train both models, calibrate abstention, generate the weights source
#    and the manifest (deterministic: fixed seed, single-threaded):
cargo run --release -- train --data-dir /path/to/workdir

# 4. Rebuild so the *compiled* crate carries the new weights, then
#    evaluate end to end (held-out + the repo's UDHR tier dataset).
#    Both halves are optional: drop --data-dir to evaluate only the
#    committed dataset (no 95 MB corpus required), drop --dataset for
#    only the held-out splits.
cargo build --release
cargo run --release -- eval --data-dir /path/to/workdir \
  --dataset ../../benchmarks/competitive/datasets/language-accuracy/dataset.json

# 5. Re-pin the crate's golden tests (they assert exact f32 bit patterns,
#    which legitimately change on retrain): feed the GOLDEN inputs from
#    src/hashed_linear.rs as a JSON string array on stdin and paste the
#    printed rows back into the test table.
cargo run --release -- golden < golden_inputs.json

# 6. Run both test suites:
cargo test --release                      # this tool (differential + alloc tests)
cargo test -p verbora-language --release --features fast-language-detection
```

## The weights and the feature extractors are one artifact

A trained weight is a number about a *bucket*, and a bucket means nothing
except relative to the function that emitted it. Editing
`hashed_features` or `hashed_features_cyrillic` in `verbora-language`
without re-running steps 2–5 therefore leaves every committed weight
aimed at a bucket that no longer means what it meant — while the crate
still compiles and the detector still answers.

`codegen` closes that by writing
`FEATURE_EXTRACTOR_FINGERPRINT` into the generated weights file: a digest
of both extractors' output over a fixed codepoint sweep, taken at the
moment of training. `verbora-language`'s
`committed_weights_were_trained_through_this_extractor` recomputes it from
the shipped extractors and fails when the two stop matching.

Note that this tool's `tests/tokenizer_differential.rs` does **not** cover
the same ground, and cannot. It pins each extractor against an independent
statement of what the features should be, so it catches an extractor
drifting *away* from its definition — but the edit that strands the
weights is the opposite one, a fix that brings the extractor back *to* its
definition. That edit turns the differential test from red to green at
exactly the moment the weights go stale.

When the fingerprint test fails, retrain. Do not re-record the constant:
it is generated, and it is not the thing that went wrong.

## Training data — exactly what the current weights were trained on

- **Corpus:** Tatoeba per-language sentence exports downloaded
  2026-08-18 (`https://downloads.tatoeba.org/exports/per_language/`),
  18 languages. Per-file SHA-256 hashes and per-language sentence counts
  are in `manifest.json` (`prepare` section).
- **License/attribution:** Tatoeba sentence data is **CC-BY 2.0 FR**;
  attribution: *Tatoeba contributors, https://tatoeba.org*. The generated
  weights file carries this attribution in its header.
- **Filtering:** sentences whose `detect_script` majority does not match
  the language's script are dropped (removes mislabeled entries and
  foreign-script quotes); exact-duplicate sentences are dropped; length
  capped at 400 chars. Training capped at 120k sentences/language, 5k
  held out (every 10th kept sentence). Thin languages use everything
  Tatoeba has: gl ~8.2k, ca ~11.0k, eu ~6.4k, no ~18.1k raw sentences.
- **The published evaluation set is excluded by construction:** the
  UDHR-derived `dataset.json` under
  `benchmarks/competitive/datasets/language-accuracy` is never read by
  `prepare`/`train` — it appears only in `eval`, as a test-only set.
- **WiLI-2018 was considered and not used** (the decomposition report
  suggested it as a supplement for thin languages): it derives from
  Wikipedia text (CC-BY-SA / ODbL), whose share-alike terms are murkier
  for baked-in model weights than Tatoeba's plain CC-BY. The thin-language
  caveat below is the accepted cost; supplementing gl/ca/eu with a
  license-clean corpus is the known lever if their held-out numbers need
  to move.

## Model and calibration

Two linear models, matching the detector's script-staged dispatch
(everything else is script-determined or abstains — see
`src/hashed_linear.rs`):

- **Latin, 16 classes** — `whichlang`-style hashed n-gram features
  (4096 buckets, shared with inference via `train_support`).
- **Cyrillic, 2 classes (ru/uk)** — codepoint unigram+bigram features.
  The whichlang feature shape measurably cannot separate two languages in
  one Unicode block (ru/uk scored 52%/64% held-out under it — near
  chance); the codepoint features took the same split to 98.1%/97.8%.

Training is single-threaded SGD (multinomial logistic regression,
balanced per-class sampling with replacement) with a fixed seed —
re-running on the same corpus reproduces the generated file byte for
byte. The abstention margins are calibrated on held-out data: the largest
threshold abstaining on ≤5% of correct held-out predictions, capped by
the median margin of incorrect ones.

## Current accuracy status (this corpus run) — read before claiming anything

Held-out (sentence-length, end-to-end through the shipped detector,
abstentions counted as not-correct): 85.7%–99.6% per language; weakest is
Galician (85.7%, thinnest corpus + heavy es/pt overlap). Full table in
`manifest.json` and in `eval`'s output.

UDHR tier dataset (13 languages, eval-only):

| tier | HashedLinearDetector | WhatlangDetector | FallbackDetector<Hashed, Whatlang> |
|---|---|---|---|
| short_word | 7/13 | 10/13 | 10/13 |
| short_phrase | 12/13 | 13/13 | 13/13 |
| sentence | 13/13 | 13/13 | 13/13 |
| paragraph | 13/13 | 13/13 | 13/13 |
| **total** | **45/52** | **49/52** | **49/52** |

Reproduce with (no corpus needed — `--data-dir` is optional, and the
dataset half is committed):

```bash
cargo run --release -- eval \
  --dataset ../../benchmarks/competitive/datasets/language-accuracy/dataset.json
```

**The standalone short-word number is below the decomposition report's
publishing gate** (≥ whichlang's 9/13; target: whatlang's 10/13).
Accordingly `HashedLinearDetector` alone makes no short-input accuracy
claim anywhere — not in the crate docs, not on the site, not in benchmark
copy — and its own doc comment says so explicitly. Its short-input story
is honest abstention plus `best_above`, not accuracy parity. Levers if
the standalone gate must be met: dimension 8192 (measured +10–35%
latency, still at/under whichlang), corpus supplements for gl/ca/eu, and
short-fragment training augmentation.

What *does* meet the gate is the composition: three of the fast model's
six short-word misses are abstentions, and
`FallbackDetector<HashedLinearDetector, WhatlangDetector>` (in the crate,
behind no feature of its own) recovers exactly those, landing on
`WhatlangDetector`'s own 49/52 total. It is not the same 49: at
`short_word` the composition misses `fr`/`it` (the fast model's confident
misses, which no fallback can catch) and gets `nl` (which
`WhatlangDetector` abstains on). 52 items is enough to show the
abstentions are recoverable, not enough to claim general parity — the
crate's own doc comment says exactly that, and any published copy must
too.

## Reproducibility manifest

`manifest.json` (committed) records: tool version, feature-space
dimension, all hyperparameters and the PRNG seed, calibrated abstention
margins, per-class held-out accuracy, and the `prepare` summary (corpus
file SHA-256 hashes, filter settings, per-language counts).
