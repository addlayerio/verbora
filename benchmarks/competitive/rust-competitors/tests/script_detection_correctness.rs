//! `CORRECTNESS BEFORE PERFORMANCE` for `benches/language.rs`'s script
//! detection groups.
//!
//! Verbora's [`verbora_language::Script`] (10 variants) and `whatlang`'s
//! `Script` (25 variants) are not the same enum, and were never claimed to
//! be — `docs/COMPETITIVE_BENCHMARKS.md` §1.10 documents the scope
//! difference explicitly (Thai/Armenian/Georgian/etc. all fall into
//! Verbora's `Other` bucket). What this test checks is the honest, narrower
//! claim `benches/language.rs`'s own doc comment makes: on the scripts both
//! classifiers *do* have a matching variant for, they agree — on every one
//! of this dataset's 13 languages, not a hand-picked example.

use verbora_language::{Script as VerboraScript, detect_script};
use whatlang::Script as WhatlangScript;

use competitive_rust::language_support::load_dataset;

/// `true` if `verbora`'s answer and `whatlang`'s answer describe the same
/// writing system, given the two crates use differently-named/differently-
/// scoped enums for it. `None` on either side is only equal to `None` on
/// the other — this function never treats "no script detected" as
/// matching "some script detected".
fn scripts_agree(verbora: Option<VerboraScript>, whatlang: Option<WhatlangScript>) -> bool {
    match (verbora, whatlang) {
        (None, None) => true,
        (Some(VerboraScript::Latin), Some(WhatlangScript::Latin)) => true,
        (Some(VerboraScript::Cyrillic), Some(WhatlangScript::Cyrillic)) => true,
        (Some(VerboraScript::Greek), Some(WhatlangScript::Greek)) => true,
        (Some(VerboraScript::Arabic), Some(WhatlangScript::Arabic)) => true,
        (Some(VerboraScript::Hebrew), Some(WhatlangScript::Hebrew)) => true,
        (Some(VerboraScript::Devanagari), Some(WhatlangScript::Devanagari)) => true,
        (Some(VerboraScript::Hiragana), Some(WhatlangScript::Hiragana)) => true,
        (Some(VerboraScript::Katakana), Some(WhatlangScript::Katakana)) => true,
        (Some(VerboraScript::Hangul), Some(WhatlangScript::Hangul)) => true,
        // Verbora's "Han" covers CJK ideographs; whatlang names the same
        // Unicode block "Mandarin" (its own naming choice, not a different
        // classification target -- see whatlang-0.18.0/src/scripts/script.rs).
        (Some(VerboraScript::Han), Some(WhatlangScript::Mandarin)) => true,
        _ => false,
    }
}

#[test]
fn agree_on_every_dataset_language_at_the_sentence_tier() {
    let dataset = load_dataset();
    let mut mismatches = Vec::new();

    for entry in &dataset {
        let text = entry.items.get("sentence");
        let verbora_script = detect_script(text);
        let whatlang_script = whatlang::detect_script(text);
        if !scripts_agree(verbora_script, whatlang_script) {
            mismatches.push(format!(
                "{} ({}): verbora={verbora_script:?} whatlang={whatlang_script:?} text={text:?}",
                entry.iso639_1, entry.verbora_language
            ));
        }
    }

    assert!(
        mismatches.is_empty(),
        "script detectors disagree on {} of {} dataset languages:\n{}",
        mismatches.len(),
        dataset.len(),
        mismatches.join("\n")
    );
}

#[test]
fn agree_across_every_tier_for_a_representative_language_per_script() {
    // One language per script family this dataset actually covers, checked
    // at all four length tiers -- a real "range of input lengths and
    // scripts" sweep (the matrix's own words), not just one fixed size.
    let dataset = load_dataset();
    let representatives = [
        "en", // Latin
        "ru", // Cyrillic
        "hi", // Devanagari
        "zh", // Han / Mandarin
    ];

    for iso in representatives {
        let entry = dataset
            .iter()
            .find(|l| l.iso639_1 == iso)
            .unwrap_or_else(|| panic!("{iso} missing from dataset"));
        for tier in competitive_rust::language_support::TIERS {
            let text = entry.items.get(tier);
            let verbora_script = detect_script(text);
            let whatlang_script = whatlang::detect_script(text);
            assert!(
                scripts_agree(verbora_script, whatlang_script),
                "{iso}/{tier}: verbora={verbora_script:?} whatlang={whatlang_script:?} text={text:?}"
            );
        }
    }
}

#[test]
fn neither_detector_panics_on_the_full_dataset() {
    let dataset = load_dataset();
    for tier in competitive_rust::language_support::TIERS {
        for entry in &dataset {
            let text = entry.items.get(tier);
            let _ = detect_script(text);
            let _ = whatlang::detect_script(text);
        }
    }
}
