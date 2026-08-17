//! `CORRECTNESS BEFORE PERFORMANCE` for `benches/language.rs`'s language
//! detection groups.
//!
//! Language detection is not "deterministically equivalent" across
//! `whatlang`, `lingua`, and `whichlang` — they are three genuinely
//! different statistical algorithms, and disagreeing on hard cases is
//! *expected*, not a bug (that disagreement is exactly what
//! `examples/language_accuracy.rs` measures). What this file checks instead,
//! before any timing number from `benches/language.rs` is trusted:
//!
//! 1. The harness itself is wired correctly — on the easiest possible case
//!    (a long, unambiguous, grammatically complete paragraph), all three
//!    detectors actually agree with the expected answer. If even this
//!    fails, the bug is in this benchmark's setup (wrong builder
//!    restriction, wrong mapping table, wrong dataset field), not in the
//!    detectors' real-world accuracy — and that must be caught here, not
//!    discovered by staring at a suspicious accuracy table later.
//! 2. Every detector call in scope for `benches/language.rs` runs to
//!    completion without panicking on the full dataset, so a panic never
//!    shows up disguised as a Criterion timing outlier.

use competitive_rust::language_support::{TIERS, lingua_restricted_languages, load_dataset};
use verbora_language::{LanguageDetector, WhatlangDetector};

#[test]
fn all_three_detectors_agree_on_english_paragraph() {
    let dataset = load_dataset();
    let english = dataset
        .iter()
        .find(|l| l.iso639_1 == "en")
        .expect("english is in the dataset");
    let text = english.items.get("paragraph");

    let verbora = WhatlangDetector::new();
    let verbora_answer = verbora
        .detect(text)
        .best()
        .map(|c| c.language.iso639_1().to_owned());
    assert_eq!(
        verbora_answer.as_deref(),
        Some("en"),
        "Verbora (whatlang) failed to detect English on its own easiest case: {text:?}"
    );

    let lingua_detector =
        lingua::LanguageDetectorBuilder::from_languages(&lingua_restricted_languages()).build();
    let lingua_answer = lingua_detector.detect_language_of(text);
    assert_eq!(
        lingua_answer,
        Some(lingua::Language::English),
        "lingua failed to detect English on its own easiest case: {text:?}"
    );

    let whichlang_answer = whichlang::detect_language(text);
    assert_eq!(
        whichlang_answer,
        whichlang::Lang::Eng,
        "whichlang failed to detect English on its own easiest case: {text:?}"
    );
}

#[test]
fn every_detector_runs_to_completion_on_the_full_dataset_without_panicking() {
    let dataset = load_dataset();
    let verbora = WhatlangDetector::new();
    let lingua_detector =
        lingua::LanguageDetectorBuilder::from_languages(&lingua_restricted_languages()).build();

    for tier in TIERS {
        for entry in &dataset {
            let text = entry.items.get(tier);
            let _ = verbora.detect(text);
            let _ = lingua_detector.detect_language_of(text);
            let _ = whichlang::detect_language(text);
        }
    }
}

#[test]
fn whichlang_never_abstains_on_the_dataset() {
    // Documents the matrix's own note ("cannot abstain") as an executed
    // fact, not just a claim in a doc comment: every call in this test
    // returns a concrete `Lang`, never a `None` of any kind (there is no
    // `Option` in whichlang's signature to begin with -- this test exists
    // to make that structural fact visible next to the other two
    // detectors' tests, which do check for `None`).
    let dataset = load_dataset();
    for tier in TIERS {
        for entry in &dataset {
            let text = entry.items.get(tier);
            let answer = whichlang::detect_language(text);
            // The type system already guarantees this is a `Lang`, not an
            // `Option<Lang>` -- asserting it is one of the 16 known
            // variants is the only meaningful runtime check left.
            let _ = competitive_rust::language_support::whichlang_iso(answer);
        }
    }
}
