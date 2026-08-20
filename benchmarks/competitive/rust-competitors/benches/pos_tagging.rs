// `criterion_group!` expands to an undocumented function, and a benchmark
// harness is not public API, so the workspace-wide `missing_docs` lint is
// noise here (this crate has no `[lints]` table of its own, but the style is
// kept consistent with the in-workspace benches this file mirrors).
#![allow(missing_docs)]

//! Verbora vs. real, pinned third-party Rust competitors — part-of-speech
//! tagging.
//!
//! See `docs/COMPETITIVE_BENCHMARKS.md` §1.16 for why `postagger` and
//! `rust-bert` were selected, and `../../README.md` for why this crate lives
//! outside the main workspace.
//!
//! # Three different algorithm classes — never blended into one number
//!
//! - **Verbora** (`verbora_tagger::BrillTagger`) — a deterministic Brill
//!   transformation-rule tagger: a lexicon lookup, then 18 hand-authored
//!   context rules applied in sequence.
//! - **`postagger`** (shubham0204/postagger.rs 0.0.3) — a pretrained
//!   averaged-perceptron model, weights extracted directly from NLTK's
//!   `averaged_perceptron_tagger.zip`.
//! - **`rust-bert`** (0.23.0, `POSModel`) — a MobileBERT transformer forward
//!   pass.
//!
//! All three assign Penn-Treebank-family tags to English text, so the *task*
//! is the same, but the *technique* is not (matrix: both competitors are
//! `Partial`, never `Yes`) — no test anywhere in this crate asserts the
//! three produce the same tag for the same word, and none should. What is
//! compared is wall-clock time on byte-identical input sentences.
//!
//! # Cold start vs. steady state — measured and labeled separately
//!
//! Per the spec's `COLD START` and `SYSTEM VS LIBRARY BENCHMARK` sections,
//! model-load cost is never blended into per-sentence latency:
//!
//! - `pos_cold_start` — the full cost of getting each implementation from
//!   "just started" to "ready to tag": for Verbora, constructing a
//!   [`Lexicon`] *and* a [`RuleSet`] *and* a [`BrillTagger`] (the whole
//!   set of objects `postagger`'s and `rust-bert`'s single `::new()` call
//!   already bundles); for `postagger`, `PerceptronTagger::new` (parses a
//!   5.6 MB weights.json); for `rust-bert`, `POSModel::new` (loads a ~94 MB
//!   MobileBERT checkpoint into a `VarStore`). `rust-bert`'s group runs at a
//!   deliberately small `sample_size` — each iteration is tens of
//!   milliseconds, and Criterion's default 100 samples would make this one
//!   group dominate the whole suite's wall-clock time for no extra
//!   precision.
//! - `pos_tag_sentence` — steady-state per-call latency on a warm
//!   (already-constructed) tagger/model, the number that matters for a
//!   long-running process. Never includes construction.
//!
//! # Why `rust-bert` is built without its own "remote" feature
//!
//! `rust-bert = { default-features = false, features = ["download-libtorch"] }`
//! in `Cargo.toml` deliberately drops the crate's default `"remote"` feature.
//! `"remote"` pulls `cached-path` → `indicatif` 0.16.2, whose own
//! `Cargo.toml` requires `console = ">=0.9.1, <1.0.0"` with no upper minor
//! bound; as of this research pass that resolves to `console` 0.16.4, a
//! release that removed the `std` feature from its own default feature set,
//! which breaks `indicatif` 0.16.2's `use console::Term` and two sibling
//! imports at compile time. This is a real, reproducible upstream breakage
//! in the dependency graph — confirmed by attempting the build with
//! `"remote"` enabled and reading the resulting `E0432` errors — not a
//! workaround chosen for convenience. `"download-libtorch"` is unaffected
//! (it never depends on `indicatif`) and is rust-bert's own supported route
//! to the libtorch shared library this crate links against. The model
//! checkpoint that `POSConfig::default()` would otherwise fetch via
//! `RemoteResource` is instead fetched once by
//! `../../scripts/fetch-models.sh rust-bert-pos` and loaded through
//! `rust_bert::resources::LocalResource`, which is not gated by `"remote"`
//! at all — see [`rust_bert_pos_model_dir`].
//!
//! # Fetched assets, and graceful skipping
//!
//! `postagger`'s pretrained weights and `rust-bert`'s MobileBERT checkpoint
//! are both separately-licensed multi-megabyte third-party assets not
//! vendored into this repository — the same reasoning
//! `crates/verbora-wordnet/benches/wordnet.rs` already documents for the
//! WordNet database. Run `../../scripts/fetch-models.sh` once; every group
//! that needs one of these skips cleanly with a printed notice (not a hard
//! failure) if it is absent, so a missing licence-restricted asset never
//! fails `cargo bench` for everyone else's groups.

use std::path::{Path, PathBuf};

use criterion::{BenchmarkId, Criterion, black_box, criterion_group, criterion_main};
use postagger::PerceptronTagger;
use rust_bert::pipelines::common::{ModelResource, ModelType};
use rust_bert::pipelines::pos_tagging::{POSConfig, POSModel};
use rust_bert::pipelines::token_classification::{
    LabelAggregationOption, TokenClassificationConfig,
};
use rust_bert::resources::LocalResource;
use tch::Device;
use verbora_tagger::{BrillTagger, Language, Lexicon, RuleSet};

/// The canonical short sentence: `postagger`'s own README example, and a
/// perfectly ordinary Penn-Treebank-taggable English sentence. Nine tokens.
const SHORT_SENTENCE: &str = "the quick brown fox jumps over the lazy dog";

/// A longer sentence (20 tokens), built from the exact same word list
/// `crates/verbora-tagger/benches/tagger.rs`'s own `english_tokens` cycles —
/// reusing that list rather than inventing a second one, so a reader
/// comparing the two files sees the same words doing the same job.
const LONG_SENTENCE: &str = "the quick brown fox jumps over a lazy dog and would book a flight to Paris quickly www.example.com 5 cats";

fn tokens(sentence: &str) -> Vec<&str> {
    sentence.split_whitespace().collect()
}

/// `benchmarks/competitive/models/postagger/`, or `$POSTAGGER_MODEL_DIR`.
fn postagger_model_dir() -> Option<PathBuf> {
    if let Some(v) = std::env::var_os("POSTAGGER_MODEL_DIR") {
        let p = PathBuf::from(v);
        if p.join("weights.json").is_file() {
            return Some(p);
        }
    }
    let vendored = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(1)?
        .join("models/postagger");
    vendored.join("weights.json").is_file().then_some(vendored)
}

/// `benchmarks/competitive/models/mobilebert-pos/`, or
/// `$RUST_BERT_POS_MODEL_DIR`.
fn rust_bert_pos_model_dir() -> Option<PathBuf> {
    if let Some(v) = std::env::var_os("RUST_BERT_POS_MODEL_DIR") {
        let p = PathBuf::from(v);
        if p.join("rust_model.ot").is_file() {
            return Some(p);
        }
    }
    let vendored = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(1)?
        .join("models/mobilebert-pos");
    vendored.join("rust_model.ot").is_file().then_some(vendored)
}

fn skip_notice(what: &str, target: &str) {
    eprintln!(
        "pos_tagging benches: {what} skipped — model assets not found.\n\
         Fetch them with: benchmarks/competitive/scripts/fetch-models.sh {target}"
    );
}

fn load_postagger(dir: &Path) -> PerceptronTagger {
    PerceptronTagger::new(
        dir.join("weights.json").to_str().unwrap(),
        dir.join("classes.txt").to_str().unwrap(),
        dir.join("tags.json").to_str().unwrap(),
    )
}

fn load_rust_bert_pos(dir: &Path) -> POSModel {
    let cfg = TokenClassificationConfig {
        model_type: ModelType::MobileBert,
        model_resource: ModelResource::Torch(Box::new(LocalResource {
            local_path: dir.join("rust_model.ot"),
        })),
        config_resource: Box::new(LocalResource {
            local_path: dir.join("config.json"),
        }),
        vocab_resource: Box::new(LocalResource {
            local_path: dir.join("vocab.txt"),
        }),
        merges_resource: None,
        lower_case: true,
        strip_accents: Some(true),
        add_prefix_space: None,
        device: Device::Cpu,
        kind: None,
        label_aggregation_function: LabelAggregationOption::First,
        batch_size: 64,
    };
    POSModel::new(POSConfig::from(cfg)).expect("MobileBERT POS model loads")
}

/// Full "just started" → "ready to tag" cost, one implementation per id.
fn bench_cold_start(c: &mut Criterion) {
    let mut g = c.benchmark_group("pos_cold_start");

    g.bench_with_input(BenchmarkId::new("verbora", "cold"), &(), |b, ()| {
        b.iter(|| {
            let lexicon = Lexicon::bundled(Language::English);
            let rules = RuleSet::bundled(Language::English);
            let tagger = BrillTagger::new(&lexicon, &rules);
            black_box(tagger.tag(black_box(tokens(SHORT_SENTENCE))))
        });
    });

    if let Some(dir) = postagger_model_dir() {
        g.bench_with_input(BenchmarkId::new("postagger", "cold"), &dir, |b, dir| {
            b.iter(|| {
                let tagger = load_postagger(dir);
                // `PerceptronTagger::tag`'s return value borrows `tagger`
                // itself (see `Tag<'a>`'s lifetime), which is about to be
                // dropped at the end of this closure — consume it into an
                // owned `usize` here rather than letting a borrow escape.
                black_box(tagger.tag(black_box(SHORT_SENTENCE)).len())
            });
        });
    } else {
        skip_notice("postagger cold-start", "postagger");
    }

    if let Some(dir) = rust_bert_pos_model_dir() {
        // Tens of milliseconds per iteration (a full MobileBERT weight load
        // and `VarStore` materialisation) — a small sample size keeps this
        // group from dominating the suite's wall-clock time for no extra
        // statistical precision than Criterion's default 100 samples would
        // give here.
        g.sample_size(15);
        g.bench_with_input(BenchmarkId::new("rust_bert", "cold"), &dir, |b, dir| {
            b.iter(|| black_box(load_rust_bert_pos(dir)));
        });
    } else {
        skip_notice("rust-bert cold-start", "rust-bert-pos");
    }

    g.finish();
}

/// Steady-state per-call latency on an already-constructed tagger/model, at
/// two sentence lengths (9 and 20 tokens) — the number that matters once a
/// process has paid its one-time model-load cost.
fn bench_tag_sentence(c: &mut Criterion) {
    let mut g = c.benchmark_group("pos_tag_sentence");

    let en_lexicon = Lexicon::bundled(Language::English);
    let en_rules = RuleSet::bundled(Language::English);
    let verbora_tagger = BrillTagger::new(&en_lexicon, &en_rules);

    let postagger_tagger = postagger_model_dir().map(|d| load_postagger(&d));
    if postagger_tagger.is_none() {
        skip_notice("postagger steady-state tagging", "postagger");
    }

    let rust_bert_model = rust_bert_pos_model_dir().map(|d| load_rust_bert_pos(&d));
    if rust_bert_model.is_none() {
        skip_notice("rust-bert steady-state tagging", "rust-bert-pos");
    }

    for (label, sentence) in [("9tok", SHORT_SENTENCE), ("20tok", LONG_SENTENCE)] {
        let toks = tokens(sentence);

        g.bench_with_input(BenchmarkId::new("verbora", label), &toks, |b, toks| {
            b.iter(|| black_box(verbora_tagger.tag(black_box(toks.iter().copied()))));
        });

        if let Some(tagger) = &postagger_tagger {
            g.bench_with_input(BenchmarkId::new("postagger", label), sentence, |b, s| {
                b.iter(|| black_box(tagger.tag(black_box(s))));
            });
        }

        if let Some(model) = &rust_bert_model {
            let input = [sentence];
            g.bench_with_input(BenchmarkId::new("rust_bert", label), &input, |b, input| {
                b.iter(|| black_box(model.predict(black_box(input))));
            });
        }
    }
    g.finish();
}

/// `rust-bert`'s `POSModel::predict` takes a batch of sentences at once
/// (`batch_size: 64` in the config above); this shows what a caller gets by
/// actually batching instead of calling `predict` once per sentence, versus
/// Verbora/`postagger` processing the same sentences in a sequential loop
/// (neither has a batched API — each `tag`/`tag` call is independent, which
/// is the fair comparison: what a real caller processing many short
/// documents would write for each library as it is actually shaped).
fn bench_batch(c: &mut Criterion) {
    let mut g = c.benchmark_group("pos_tag_batch");

    let en_lexicon = Lexicon::bundled(Language::English);
    let en_rules = RuleSet::bundled(Language::English);
    let verbora_tagger = BrillTagger::new(&en_lexicon, &en_rules);
    let toks = tokens(SHORT_SENTENCE);

    const BATCH: usize = 8;

    g.bench_with_input(BenchmarkId::new("verbora", BATCH), &toks, |b, toks| {
        b.iter(|| {
            for _ in 0..BATCH {
                black_box(verbora_tagger.tag(black_box(toks.iter().copied())));
            }
        });
    });

    if let Some(dir) = postagger_model_dir() {
        let tagger = load_postagger(&dir);
        g.bench_with_input(
            BenchmarkId::new("postagger", BATCH),
            &SHORT_SENTENCE,
            |b, s| {
                b.iter(|| {
                    for _ in 0..BATCH {
                        black_box(tagger.tag(black_box(s)));
                    }
                });
            },
        );
    } else {
        skip_notice("postagger batch tagging", "postagger");
    }

    if let Some(dir) = rust_bert_pos_model_dir() {
        let model = load_rust_bert_pos(&dir);
        let batch = [SHORT_SENTENCE; BATCH];
        g.bench_with_input(BenchmarkId::new("rust_bert", BATCH), &batch, |b, batch| {
            b.iter(|| black_box(model.predict(black_box(batch.as_slice()))));
        });
    } else {
        skip_notice("rust-bert batch tagging", "rust-bert-pos");
    }

    g.finish();
}

criterion_group!(benches, bench_cold_start, bench_tag_sentence, bench_batch);
criterion_main!(benches);
