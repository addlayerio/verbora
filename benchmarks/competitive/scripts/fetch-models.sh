#!/usr/bin/env bash
# Fetches the third-party model/dictionary assets the POS-tagging and
# spellcheck competitive benches need, but that ship neither on crates.io nor
# in this repository -- same reasoning as WordNet's own database (see
# crates/verbora-wordnet's crate docs and its benches/wordnet.rs
# skip_notice()): a separately-licensed, multi-megabyte third-party asset does
# not belong vendored into version control, so it is fetched on demand into
# benchmarks/competitive/models/ (gitignored) and every bench that needs one
# of these looks there by default, with an env var override, and SKIPS with a
# printed notice (not a hard failure) if it is absent.
#
# Usage: benchmarks/competitive/scripts/fetch-models.sh [target...]
#   targets: postagger | rust-bert-pos | hunspell-en-us | wordnet-en | all
#            (default: all)
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."   # -> benchmarks/competitive/
OUT="$PWD/models"
mkdir -p "$OUT"

fetch_postagger () {
  # postagger (shubham0204/postagger.rs) 0.0.3's pretrained averaged-perceptron
  # weights are NOT published on crates.io (the crate ships only source) --
  # they live in the GitHub repository's `tagger/` directory, extracted
  # directly from NLTK's `averaged_perceptron_tagger.zip` per the crate's own
  # README. Apache-2.0, matching docs/COMPETITIVE_BENCHMARKS.md §1.16.
  local dir="$OUT/postagger"
  if [ -f "$dir/weights.json" ] && [ -f "$dir/classes.txt" ] && [ -f "$dir/tags.json" ]; then
    echo "postagger model already present at $dir"
    return
  fi
  echo "fetching postagger (shubham0204/postagger.rs) pretrained model..."
  local tmp
  tmp=$(mktemp -d)
  git clone --depth 1 https://github.com/shubham0204/postagger.rs.git "$tmp" >/dev/null 2>&1
  mkdir -p "$dir"
  cp "$tmp/tagger/weights.json" "$tmp/tagger/classes.txt" "$tmp/tagger/tags.json" "$dir/"
  rm -rf "$tmp"
  echo "  wrote $dir"
}

fetch_rust_bert_pos () {
  # rust-bert 0.23.0's POSModel::default() (behind the crate's "remote"
  # feature, which this workspace deliberately does NOT enable -- see
  # pos_tagging.rs's own doc comment for why: indicatif 0.16.2's `console`
  # dependency is unconstrained above 1.0.0 and pulls a `console` 0.16 release
  # that removed the `std` feature from its own default set, which no longer
  # compiles as of this research pass) points at
  # huggingface.co/mrm8488/mobilebert-finetuned-pos (MIT-licensed checkpoint,
  # converted to Torch's serialization format by the rust-bert maintainers).
  # Fetched directly and loaded via `LocalResource` instead.
  local dir="$OUT/mobilebert-pos"
  if [ -f "$dir/rust_model.ot" ] && [ -f "$dir/config.json" ] && [ -f "$dir/vocab.txt" ]; then
    echo "rust-bert MobileBERT POS model already present at $dir"
    return
  fi
  echo "fetching rust-bert's MobileBERT English POS checkpoint (~94 MB)..."
  mkdir -p "$dir"
  curl -sL -o "$dir/config.json" "https://huggingface.co/mrm8488/mobilebert-finetuned-pos/resolve/main/config.json"
  curl -sL -o "$dir/vocab.txt" "https://huggingface.co/mrm8488/mobilebert-finetuned-pos/resolve/main/vocab.txt"
  curl -sL -o "$dir/rust_model.ot" "https://huggingface.co/mrm8488/mobilebert-finetuned-pos/resolve/main/rust_model.ot"
  echo "  wrote $dir"
}

fetch_hunspell_en_us () {
  # spellbook 0.4.2 is a Hunspell-affix-rule engine with no bundled
  # dictionary -- it needs a real .aff/.dic pair. LibreOffice's en_US
  # (SCOWL-derived, permissive "use/copy/modify/distribute" notice, see
  # README_en_US.txt fetched alongside) is the same dictionary most desktop
  # Hunspell installs ship. Matrix §1.17: spellbook is a matched-workload
  # TIMING comparison only -- Verbora's own frequency corpus is never fed to
  # it, and its suggestions are never compared to Verbora's for equivalence.
  local dir="$OUT/hunspell-en_US"
  if [ -f "$dir/en_US.aff" ] && [ -f "$dir/en_US.dic" ]; then
    echo "hunspell en_US dictionary already present at $dir"
    return
  fi
  echo "fetching LibreOffice's en_US Hunspell dictionary..."
  mkdir -p "$dir"
  curl -sL -o "$dir/en_US.aff" "https://raw.githubusercontent.com/LibreOffice/dictionaries/master/en/en_US.aff"
  curl -sL -o "$dir/en_US.dic" "https://raw.githubusercontent.com/LibreOffice/dictionaries/master/en/en_US.dic"
  curl -sL -o "$dir/README_en_US.txt" "https://raw.githubusercontent.com/LibreOffice/dictionaries/master/en/README_en_US.txt"
  echo "  wrote $dir"
}

fetch_wordnet_en () {
  # §1.15 WordNet. Both sides of benches/wordnet.rs -- verbora-wordnet and
  # `wordnet-db` 0.1.3 -- read the same eight `index.*`/`data.*` files from the
  # same directory, and NEITHER crate ships them: verbora-wordnet deliberately
  # does not (see crates/verbora-wordnet/LICENSE-WORDNET and its crate docs),
  # and wordnet-db is a reader for "prebuilt WordNet database files" it expects
  # the caller to supply. So the dictionary is fetched here like every other
  # separately-licensed asset in this script.
  #
  # WordNet 3.1 rather than 3.0, deliberately: docs/COMPETITIVE_BENCHMARKS.md
  # §1.15 pins the reference's own WordNet row to the npm `wordnet-db` 3.1.14
  # package's data, which is WordNet 3.1. Fetching 3.0 here would put the Rust
  # comparison on a different release from the reference comparison it is
  # published beside, and synset offsets differ between the two releases.
  # `benches/wordnet.rs` and `tests/wordnet_correctness.rs` accept either
  # (they resolve `models/wordnet-3.1/dict` then `models/wordnet-3.0/dict`),
  # so an existing 3.0 install is still usable -- this is the default, not a
  # requirement.
  #
  # LICENCE: Princeton University's own, NOT MIT. It requires the copyright
  # notice to accompany all copies of the database, which is one more reason
  # this is fetched into a gitignored directory rather than vendored. The
  # notice ships inside the tarball as `dict/LICENSE` and is left in place
  # beside the files. It is reproduced verbatim in
  # crates/verbora-wordnet/LICENSE-WORDNET.
  local dir="$OUT/wordnet-3.1"
  if [ -f "$dir/dict/index.noun" ]; then
    echo "WordNet dictionary already present at $dir/dict"
    return
  fi
  echo "fetching Princeton WordNet 3.1 database files (~16 MB)..."
  mkdir -p "$dir"
  local tmp
  tmp=$(mktemp -d)
  curl -sL -o "$tmp/wn.tar.gz" "https://wordnetcode.princeton.edu/wn3.1.dict.tar.gz"
  # The tarball's top-level directory is `dict/`, which is exactly the layout
  # both crates want, so it is extracted as-is rather than flattened.
  tar -xzf "$tmp/wn.tar.gz" -C "$dir"
  rm -rf "$tmp"
  if [ ! -f "$dir/dict/index.noun" ]; then
    echo "  ERROR: $dir/dict/index.noun missing after extraction" >&2
    exit 1
  fi
  echo "  wrote $dir/dict"
}

targets=("${@:-all}")
for t in "${targets[@]}"; do
  case "$t" in
    postagger) fetch_postagger ;;
    rust-bert-pos) fetch_rust_bert_pos ;;
    hunspell-en-us) fetch_hunspell_en_us ;;
    wordnet-en) fetch_wordnet_en ;;
    all) fetch_postagger; fetch_rust_bert_pos; fetch_hunspell_en_us; fetch_wordnet_en ;;
    *) echo "unknown target: $t (want postagger|rust-bert-pos|hunspell-en-us|wordnet-en|all)" >&2; exit 1 ;;
  esac
done
