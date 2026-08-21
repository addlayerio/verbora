//! Sanity check for `benches/pos_tagging.rs`.
//!
//! Verbora, `postagger` and `rust-bert` are three genuinely different
//! algorithm classes (matrix: both competitors `Partial`, never `Yes`) — no
//! tag-for-tag equivalence is claimed anywhere in this crate, so
//! `CORRECTNESS BEFORE PERFORMANCE`'s "same algorithmic answer" clause does
//! not apply. What this file checks, once and outside the timed code, is
//! that every implementation actually runs to completion on the shared
//! canonical sentence and returns one tag per token — the minimum bar for
//! trusting that the benchmark measures live, working calls rather than a
//! silently-empty or panicking one.

use std::path::{Path, PathBuf};

use postagger::PerceptronTagger;
use rust_bert::pipelines::common::{ModelResource, ModelType};
use rust_bert::pipelines::pos_tagging::{POSConfig, POSModel};
use rust_bert::pipelines::token_classification::{
    LabelAggregationOption, TokenClassificationConfig,
};
use rust_bert::resources::LocalResource;
use tch::Device;
use verbora_tagger::{BrillTagger, Language, Lexicon, RuleSet};

const SENTENCE: &str = "the quick brown fox jumps over the lazy dog";

fn model_dir(name: &str) -> Option<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(1)?
        .join("models")
        .join(name);
    dir.is_dir().then_some(dir)
}

#[test]
fn verbora_tags_every_token() {
    let lexicon = Lexicon::bundled(Language::English);
    let rules = RuleSet::bundled(Language::English);
    let tagger = BrillTagger::new(&lexicon, &rules);
    let tokens: Vec<&str> = SENTENCE.split_whitespace().collect();

    let tagged = tagger.tag(tokens.iter().copied());

    // `tag` returns `Vec<TaggedToken>` directly now — it used to return a
    // `Result` whose error was a failing rule predicate, and `TaggedToken::tag`
    // used to be `Option<Tag>`. Both are gone: `tag.rs` documents the tag as
    // "Always present: the initial-state annotator assigns a lexicon tag or the
    // lexicon's default, never nothing." The old test asserted `is_some()` on
    // every tag; that is now a type-level guarantee, so what is left to check
    // is that no token is dropped, that each keeps its own text, and that no
    // tag is empty.
    assert_eq!(tagged.len(), tokens.len());
    for (tagged_token, original) in tagged.iter().zip(&tokens) {
        assert_eq!(tagged_token.token(), *original);
        assert!(
            !tagged_token.tag.as_str().is_empty(),
            "empty tag for {original:?}"
        );
    }
}

#[test]
fn postagger_tags_every_token() {
    let Some(dir) = model_dir("postagger") else {
        eprintln!(
            "postagger smoke test skipped — model not found.\n\
             Fetch it with: benchmarks/competitive/scripts/fetch-models.sh postagger"
        );
        return;
    };
    let tagger = PerceptronTagger::new(
        dir.join("weights.json").to_str().unwrap(),
        dir.join("classes.txt").to_str().unwrap(),
        dir.join("tags.json").to_str().unwrap(),
    );
    let tags = tagger.tag(SENTENCE);
    assert_eq!(tags.len(), SENTENCE.split_whitespace().count());
    assert!(
        tags.iter().all(|t| !t.tag.is_empty()),
        "every token should receive a non-empty tag"
    );
}

#[test]
fn rust_bert_tags_every_token() {
    let Some(dir) = model_dir("mobilebert-pos") else {
        eprintln!(
            "rust-bert smoke test skipped — model not found.\n\
             Fetch it with: benchmarks/competitive/scripts/fetch-models.sh rust-bert-pos"
        );
        return;
    };
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
    let model = POSModel::new(POSConfig::from(cfg)).expect("MobileBERT POS model loads");
    let output = model.predict(&[SENTENCE]);
    assert_eq!(output.len(), 1);
    assert!(
        !output[0].is_empty(),
        "rust-bert should tag at least one sub-token for a 9-word sentence"
    );
    assert!(
        output[0].iter().all(|t| !t.label.is_empty()),
        "every tagged sub-token should have a non-empty label"
    );
}
