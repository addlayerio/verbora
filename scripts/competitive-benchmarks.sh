#!/usr/bin/env bash
# Single reproducibility entrypoint for Fase 6's competitive benchmark suite.
# Per Fase 6 Benchmark.md's REPRODUCIBILITY section: prepares dependencies,
# builds release, runs benchmarks, saves raw data, regenerates the
# structured summary — no hidden manual steps.
#
# Usage: ./scripts/competitive-benchmarks.sh [module ...]
#   No args: every module listed in MODULE_SPECS below.
#   One or more module names: restricts steps 4 and 5 — Verbora's own benches
#   and the structured-result collection — to those modules. Step 3 always
#   runs every competitor bench target, because MODULE_SPECS is a strict
#   subset of them (3 modules vs. 15 targets): filtering step 3 by it would
#   silently narrow a full campaign to a fifth of its coverage.
#
# ---------------------------------------------------------------------------
# One target at a time, and one target's failure is not the run's failure
# ---------------------------------------------------------------------------
#
# A campaign costs hours; the work it measures costs minutes. That asymmetry
# is the whole design constraint here, and it has one concrete consequence:
# nothing a single benchmark target does may be allowed to destroy the
# measurements of the others.
#
# This script used to run `cargo bench --release` **once** over the entire
# competitive crate, inside a subshell, under `set -euo pipefail`. Any single
# bench target that failed to compile, panicked, or crashed took the subshell
# down, `set -e` took the script down, and steps 4 and 5 — Verbora's own
# benches and the structured-result collection — never ran. A run that had
# already measured fourteen targets correctly produced nothing at all, for
# any of them.
#
# That is not hypothetical. `docs/design/rust-native-migration.md`'s standing
# findings record a HuggingFace tokenizer bench segfaulting once under a
# full-suite run and proving non-reproducible in isolation across 544
# measurements — and both `CLAUDE.md` and that document state the resulting
# rule: *run benchmarks one target at a time, never the whole workspace at
# once, so a crash in one target cannot abort the campaign.* A target that
# simply does not compile is the cheaper and likelier version of the same
# accident.
#
# So, deliberately:
#
#   * Each competitor bench is invoked on its own via `--bench <name>`. That
#     isolates compilation too: `cargo bench --bench distance` builds the
#     `distance` target and its dependencies, so a `benches/tfidf.rs` that
#     does not compile no longer prevents `distance` from being measured.
#   * Every invocation's exit status is *captured* rather than propagated.
#     `set -e` is switched off around the invocation itself and back on
#     immediately after (see `run_step`), so a failure is recorded as data
#     and the loop continues.
#   * The run ends with an explicit per-target verdict — what succeeded, what
#     was skipped and why, what failed and with which status — and a pointer
#     to that target's own log.
#   * The exit status is non-zero when anything failed, but only after
#     everything that could run has run and the summary has been printed. A
#     partial campaign reports as partial; it is never silently a success and
#     never discarded wholesale.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
ROOT="$(pwd)"
COMPETITIVE="$ROOT/benchmarks/competitive"

# The instant this campaign began, in epoch seconds. `collect-results.py` uses
# it to refuse any Criterion estimate written before now: a target that fails
# to build leaves its previous run's `estimates.json` in place, and without
# this the collector republishes those stale numbers as if this campaign had
# produced them. Exported once so every step-5 invocation shares one cutoff.
export VERBORA_BENCH_STARTED_AT="$(date +%s)"
# One log per target, so "which target failed, and why" survives an unattended
# overnight run whose terminal scrollback does not. Runtime artifacts, written
# beside the other runtime artifacts under results/.
LOGS="$COMPETITIVE/results/logs"

# Each entry: module name, then the collect-results.py group:ids specs for
# that module's rust-competitors bench. Extend this array as new modules
# gain a benchmarks/competitive/rust-competitors/benches/<module>.rs file —
# see benchmarks/competitive/README.md's "Adding a new module" section.
#
# NOTE on the `*_wrapper_overhead` groups: they are collected like any other,
# but they are NOT rival-implementation comparisons — Verbora's word and
# sentence tokenizers are built on `unicode-segmentation`, so those rows
# measure wrapper cost over the primitive. See benches/tokenizers.rs's own
# module doc comment; never report them as Verbora winning or losing against
# unicode-segmentation.
#
# `whitespace_tokenization` is gone: `RegexpTokenizer`/`Pattern` were deleted
# by the text-shaping migration and Verbora performs no whitespace or regex
# tokenization at any API, so the row had no Verbora side left.
#
# `eddie` is absent from `distance`'s `jaro`/`jaro_winkler` id lists, and must
# stay absent. `eddie` 0.4.2's published `str` API executes undefined
# behaviour on every call (`Buffer::store` writes through
# `get_unchecked_mut` past a zeroed length), so no timing number derived from
# it is reportable; `benches/distance.rs` no longer emits an `eddie` row at
# all. It survives only as a correctness oracle, through its sound slice API,
# in `rust-competitors/tests/distance_correctness.rs`. See
# `benchmarks/competitive/README.md`'s "Resolved: `eddie` 0.4.2 is unsound"
# section.
#
# `linfa_bayes` is absent from `classifiers`' `bayes_train`/`bayes_predict` id
# lists, and must stay absent — the same retirement, for a different fault.
# `linfa-bayes` 0.8.1 ships a `dbg!` in non-test code: its
# `MultinomialNb::fit_with` training loop
# (`linfa-bayes-0.8.1/src/multinomial_nb.rs:78`) writes a whole
# `ClassHistogram` to stderr once per class on every `.fit()`, and a Criterion
# group calls `fit` millions of times. One campaign produced a **1.5 GB log
# file** that could not be pushed to GitHub. It cannot be measured as
# published; and patching a competitor locally to measure it would make the
# comparison unfair, because the number would then describe code nobody can
# install. So it is retired from timing and kept for correctness, exactly as
# `eddie` is: `benches/classifiers.rs` no longer emits a `linfa_bayes` row in
# either group, while `tests/classifiers_accuracy.rs` (which fits once per
# corpus size, not per iteration) still reports its accuracy. `smartcore` and
# `naivebayes` remain as the module's Naive Bayes timing competitors. See
# `benchmarks/competitive/README.md`'s "Retired: `linfa-bayes` 0.8.1 cannot be
# timed as published" section.
#
# `linfa_logistic` is a DIFFERENT crate and stays in the two `logistic_*` id
# lists below. Nothing above applies to it.
declare -A MODULE_SPECS=(
  [classifiers]="bayes_train:verbora,smartcore,naivebayes bayes_predict:verbora,smartcore,naivebayes logistic_train:verbora,smartcore,linfa_logistic,rustlearn logistic_predict:verbora,smartcore,linfa_logistic,rustlearn"
  [distance]="levenshtein:verbora,strsim,rapidfuzz,stringmetrics,triple_accel,editdistancek damerau_levenshtein_unrestricted:verbora,strsim,rapidfuzz damerau_levenshtein_restricted_osa:verbora,strsim,rapidfuzz,triple_accel jaro:verbora,strsim,rapidfuzz,eddie jaro_winkler:verbora,strsim,rapidfuzz,eddie hamming:verbora,strsim,rapidfuzz,stringmetrics,triple_accel fuzzy_substring_search:verbora,triple_accel"
  [inflectors]="count_inflector_nth:verbora,ordinal count_inflector_nth_str:verbora,inflector noun_inflector_pluralize:verbora,pluralizer,inflector noun_inflector_singularize:verbora,pluralizer,inflector"
  [language]="whatlang_wrapper_overhead:verbora_wrapper,raw_whatlang language_detection_by_length:verbora,lingua,whichlang language_detection_by_language:verbora,lingua,whichlang script_detection_by_length:verbora,whatlang script_detection_by_language:verbora,whatlang transliteration_ja:verbora,wana_kana"
  [ngrams]="bigrams:verbora,ngrammatic trigrams:verbora,ngrammatic"
  [normalizers]="remove_diacritics_ascii:verbora,diacritics-crate remove_diacritics_accented:verbora,diacritics-crate ja_hiragana_to_katakana:verbora,unicode-jp ja_katakana_to_hiragana:verbora,unicode-jp ja_katakana_halfwidth_to_fullwidth:verbora,kana-converter"
  [phonetics]="soundex:verbora,rphonetic metaphone:verbora,rphonetic double_metaphone:verbora,rphonetic dm_soundex:verbora,rphonetic"
  [pos_tagging]="pos_cold_start:verbora,postagger,rust_bert pos_tag_sentence:verbora,postagger,rust_bert pos_tag_batch:verbora,postagger,rust_bert"
  # §1.14 Sentiment. One group only, and its input domain is deliberately
  # narrow: `sentiment` 0.1.1 embeds AFINN-111 where Verbora ships AFINN-165,
  # implements no negation rule, and tokenizes internally. The corpus is built
  # from the 2,438 keys the two tables agree on exactly, contains no negation
  # word, and is lowercase ASCII words joined by single spaces so both
  # tokenizers cut it identically — `rust-competitors/tests/
  # sentiment_correctness.rs` asserts all three, and the two crates return the
  # same number on every document this group times. Do not widen the corpus
  # without re-reading `benches/sentiment.rs`'s doc comment: on arbitrary
  # English these two do not compute the same function.
  [sentiment]="sentiment_score_document:verbora,sentiment"
  # §1.15 WordNet. Needs the separately-licensed Princeton database —
  # `benchmarks/competitive/scripts/fetch-models.sh wordnet-en`, or
  # $WORDNET_DB_PATH. Every group skips cleanly without it, so this module
  # collects nothing rather than failing.
  #
  # Six ids per group: Verbora's four `Storage` strategies against
  # `wordnet-db` 0.1.3's two `LoadMode`s. The like-for-like pair is
  # `verbora_resident` against `wordnet_db_owned` (both read the files into
  # owned buffers); `wordnet_db_mmap` is the competitor's default and
  # `verbora_lazy` is Verbora's no-`unsafe` answer to the same problem.
  # `wordnet_open` and `wordnet_lookup` are a build-cost/query-cost pair and
  # must be published together — `wordnet-db` parses the whole database at
  # load so its queries are hash hits, Verbora binary-searches on demand.
  #
  # These rows describe the ~91% of the dictionary Verbora can currently read:
  # `verbora-wordnet` rejects the 13,606 index entries (8.8%) whose
  # `ptr_symbol` list uses the bare `;`/`-` domain pointers Princeton's index
  # files actually write. See `benches/wordnet.rs`'s "A Verbora defect narrows
  # this benchmark's domain" section — the defect is in `crates/`, not here.
  [wordnet]="wordnet_open:verbora_pread,verbora_lazy,verbora_resident,verbora_indexed,wordnet_db_mmap,wordnet_db_owned wordnet_cold:verbora_pread,verbora_lazy,verbora_resident,verbora_indexed,wordnet_db_mmap,wordnet_db_owned wordnet_lookup:verbora_pread,verbora_lazy,verbora_resident,verbora_indexed,wordnet_db_mmap,wordnet_db_owned wordnet_index_entry:verbora_pread,verbora_lazy,verbora_resident,verbora_indexed,wordnet_db_mmap,wordnet_db_owned"
  [spellcheck]="spellcheck_new:verbora,harper_core,symspell,fast_symspell spellcheck_get_corrections_d1:verbora,harper_core,symspell,fast_symspell spellcheck_get_corrections_d2:verbora,harper_core,symspell,fast_symspell spellcheck_is_correct:verbora,harper_core,symspell,fast_symspell spellcheck_spellbook_is_correct:spellbook,verbora_own_corpus spellcheck_spellbook_suggest:spellbook,verbora_own_corpus spellcheck_batch_correction:verbora,harper_core,symspell,fast_symspell spellcheck_fuzzyindex_construction:fuzzy_index,fast_symspell spellcheck_fuzzyindex_query:fuzzy_index,fast_symspell fst_fuzzy_construction:fuzzy_index,fst fst_fuzzy_query:fuzzy_index,fst"
  [stemmers]="porter_de:verbora,rust-stemmers,snowball-stemmers-rs porter_en:verbora,nltk-porter,porter-stemmer porter_es:verbora,rust-stemmers,snowball-stemmers-rs porter_fr:verbora,rust-stemmers,snowball-stemmers-rs porter_it:verbora,rust-stemmers,snowball-stemmers-rs porter_nl:verbora,rust-stemmers,snowball-stemmers-rs porter_no:verbora,rust-stemmers,snowball-stemmers-rs porter_pt:verbora,rust-stemmers,snowball-stemmers-rs porter_ru:verbora,rust-stemmers,snowball-stemmers-rs porter_sv:verbora,rust-stemmers,snowball-stemmers-rs stemmer_id:verbora,sastrawi stemmer_ja:verbora,lindera"
  [tfidf]="build:verbora,afshinm idf:verbora,afshinm,rust_tfidf tfidf:verbora,afshinm,rust_tfidf"
  [tokenizers]="whitespace_tokenization:verbora,tantivy,huggingface word_tokenization:verbora,tantivy,huggingface word_tokenization_unicode_segmentation:verbora,unicode-words,unicode-bounds aggressive_tokenization_en:verbora,unicode-words sentence_tokenization:verbora,unicode-sentences,unicode-bounds,segtok"
  [trie]="build:verbora,qp_trie,trie_rs,fast_radix_trie,fst common_prefix_search:verbora,trie_rs contains_hit:verbora,qp_trie,trie_rs,fast_radix_trie,fst contains_miss:verbora,qp_trie,trie_rs,fast_radix_trie,fst predictive_search:verbora,qp_trie,trie_rs,fast_radix_trie,fst"
)

MODULES=("${@:-${!MODULE_SPECS[@]}}")

# ---------------------------------------------------------------------------
# Per-target bookkeeping
# ---------------------------------------------------------------------------

SUCCEEDED=()
FAILED=()
SKIPPED=()

# run_step <label> <working-dir> <command> [args...]
#
# Runs one command, records its verdict, and always returns 0 so that neither
# `set -e` nor a `for` loop can be aborted by it.
#
# The `set +e` / `set -e` pair around the invocation is the deliberate part.
# Under `set -e` a failing command anywhere in the loop body kills the whole
# script, which is exactly the failure mode this script exists to avoid; and
# the usual shorthands are each worse here. `cmd || true` throws the status
# away, so the summary could not say *how* a target failed. `if cmd; then` or
# a function called in a condition context suppresses `set -e` for everything
# nested inside it, silently, well beyond the one command intended. Turning
# it off for exactly one line and back on for the next says what is meant and
# nothing else.
run_step() {
  local label=$1 dir=$2
  shift 2
  local log="$LOGS/${label//[^A-Za-z0-9._-]/-}.log"
  local rc=0

  echo "-- $label --"
  set +e
  ( cd "$dir" && "$@" ) 2>&1 | tee "$log"
  rc=${PIPESTATUS[0]} # cargo's status, never tee's.
  set -e

  if [ "$rc" -eq 0 ]; then
    SUCCEEDED+=("$label")
  else
    FAILED+=("$label (exit $rc, log: ${log#"$ROOT"/})")
    # Loud, on stderr, and immediately — an operator watching a long run
    # should not have to wait for the summary to learn something broke.
    echo "   !! FAILED: $label (exit $rc) — see ${log#"$ROOT"/}" >&2
  fi
  return 0
}

# skip_step <label> <reason>
skip_step() {
  SKIPPED+=("$1 ($2)")
  echo "   (skipped $1: $2)"
}

# The competitor bench targets, from Cargo itself rather than from a list kept
# by hand here: a hand-maintained list is one more thing that silently goes
# stale, and a target missing from it would simply never be measured. Falls
# back to the benches/ directory listing if `cargo metadata` is unavailable,
# and refuses to continue if neither yields anything — an empty target list
# must not be mistaken for "step 3 passed".
competitive_bench_targets() {
  local meta
  set +e
  meta=$(cd "$COMPETITIVE" && cargo metadata --format-version 1 --no-deps 2>/dev/null)
  local rc=$?
  set -e

  if [ "$rc" -eq 0 ] && [ -n "$meta" ]; then
    printf '%s' "$meta" | python3 -c '
import json, sys

meta = json.load(sys.stdin)
names = {
    target["name"]
    for package in meta.get("packages", [])
    for target in package.get("targets", [])
    if "bench" in target.get("kind", [])
}
for name in sorted(names):
    print(name)
' && return 0
  fi

  echo "   (cargo metadata unavailable; falling back to benches/*.rs)" >&2
  local file
  for file in "$COMPETITIVE"/rust-competitors/benches/*.rs; do
    [ -e "$file" ] || continue
    basename "$file" .rs
  done
}

echo "== 1. Shared benchmark inputs =="
# Fatal on purpose, unlike everything below: this generates the corpora every
# later step measures against. A campaign run on absent or half-written inputs
# does not produce partial results, it produces wrong ones.
python3 "$ROOT/tools/bench-data/generate.py"

mkdir -p "$LOGS"

echo "== 2. Machine metadata =="
# Recorded and survived rather than fatal. No number may be published without
# machine metadata, but this takes seconds and can be re-run afterwards,
# whereas the measurements below cannot — losing hours of them to a metadata
# script would be the exact trade this file is written to prevent.
run_step "metadata" "$COMPETITIVE" ./scripts/machine-metadata.sh

echo "== 3. Rust competitors (this workspace) =="
mapfile -t COMPETITIVE_TARGETS < <(competitive_bench_targets)
if [ "${#COMPETITIVE_TARGETS[@]}" -eq 0 ]; then
  echo "No competitor bench targets found under $COMPETITIVE/rust-competitors." >&2
  echo "Refusing to report an empty step 3 as a success." >&2
  exit 2
fi
echo "   ${#COMPETITIVE_TARGETS[@]} competitor bench targets, run one at a time."
for target in "${COMPETITIVE_TARGETS[@]}"; do
  run_step "competitive:$target" "$COMPETITIVE" cargo bench --bench "$target"
done

echo "== 4. Verbora's own in-workspace benches =="
for m in "${MODULES[@]}"; do
  # A competitive module name is not always the crate that implements it:
  # POS tagging is measured under `pos_tagging` but lives in `verbora-tagger`.
  # Assuming the two coincide silently skipped that module's own benches for
  # a whole campaign, reported as "no such file" rather than as a mapping gap.
  case "$m" in
    pos_tagging) crate="tagger" ;;
    *)           crate="$m" ;;
  esac
  bench="$ROOT/crates/verbora-$crate/benches/$crate.rs"
  if [ -f "$bench" ]; then
    run_step "verbora:$m" "$ROOT" cargo bench -p "verbora-$crate" --bench "$crate"
  else
    # Distinguished from a failure on purpose: the old `|| echo` reported a
    # bench that crashed and a bench that does not exist with the same line.
    skip_step "verbora:$m" "no crates/verbora-$crate/benches/$crate.rs — add a module-to-crate mapping above if the names differ"
  fi
done

echo "== 5. Collect structured results =="
for m in "${MODULES[@]}"; do
  spec="${MODULE_SPECS[$m]:-}"
  if [ -n "$spec" ]; then
    # shellcheck disable=SC2086 # $spec is a deliberate list of group:ids args.
    run_step "collect:$m" "$COMPETITIVE" python3 scripts/collect-results.py "$m" $spec
  else
    skip_step "collect:$m" "no MODULE_SPECS entry"
  fi
done

echo
echo "== Summary =="
printf '   %d succeeded, %d failed, %d skipped\n' \
  "${#SUCCEEDED[@]}" "${#FAILED[@]}" "${#SKIPPED[@]}"
if [ "${#SUCCEEDED[@]}" -gt 0 ]; then
  echo "   ok:"
  printf '     %s\n' "${SUCCEEDED[@]}"
fi
if [ "${#SKIPPED[@]}" -gt 0 ]; then
  echo "   skipped:"
  printf '     %s\n' "${SKIPPED[@]}"
fi
if [ "${#FAILED[@]}" -gt 0 ]; then
  echo "   FAILED:"
  printf '     %s\n' "${FAILED[@]}"
fi

echo
echo "Structured results: $COMPETITIVE/results/results.json"
echo "Raw results:        $COMPETITIVE/results/raw/"
echo "Machine metadata:   $COMPETITIVE/results/metadata.json"
echo "Per-target logs:    $LOGS/"
echo "Next: run the independent fairness audit before publishing any number."

if [ "${#FAILED[@]}" -gt 0 ]; then
  echo >&2
  echo "Partial run: ${#FAILED[@]} target(s) failed. Everything else above was" >&2
  echo "still measured and collected; only the failed targets are missing." >&2
  echo "Do not publish a number for a target that is not in the ok list." >&2
  exit 1
fi
