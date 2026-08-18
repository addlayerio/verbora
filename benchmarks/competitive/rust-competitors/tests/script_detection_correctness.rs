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
//! of this dataset's 13 languages at every one of its four tiers, on the
//! whole-paragraph-repetition inputs the bench's extended length ladder
//! derives from them, on single-script extracts with the majority vote
//! factored out, and (both abstaining) on script-less input — not a
//! hand-picked example.

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

/// The full-grid extension of the two sampled agreement tests above: every
/// one of the dataset's 13 languages at every one of its four tiers — 52
/// (verbora, whatlang) pairs, not 13 (one tier) or 16 (four languages).
/// This includes the one genuinely non-obvious cell the sampled tests
/// skip: the Japanese `short_word` tier is written entirely in kanji
/// (`"尊厳"`), so both classifiers answer Han/Mandarin there rather than
/// the Hiragana they answer on every longer Japanese tier — they *agree*,
/// which is exactly what this test pins down.
#[test]
fn agree_on_every_language_at_every_tier() {
    let dataset = load_dataset();
    let mut mismatches = Vec::new();

    for entry in &dataset {
        for tier in competitive_rust::language_support::TIERS {
            let text = entry.items.get(tier);
            let verbora_script = detect_script(text);
            let whatlang_script = whatlang::detect_script(text);
            if !scripts_agree(verbora_script, whatlang_script) {
                mismatches.push(format!(
                    "{}/{tier}: verbora={verbora_script:?} whatlang={whatlang_script:?} text={text:?}",
                    entry.iso639_1
                ));
            }
        }
    }

    assert!(
        mismatches.is_empty(),
        "script detectors disagree on {} of {} (language, tier) cells:\n{}",
        mismatches.len(),
        dataset.len() * competitive_rust::language_support::TIERS.len(),
        mismatches.join("\n")
    );
}

/// `benches/language.rs`'s script-detection length ladder extends past the
/// dataset's `paragraph` tier by whole-paragraph repetition (x4/x16/x64).
/// Both classifiers are per-codepoint counters with a majority vote, so
/// repetition scales every count by the same factor and cannot change the
/// winner — executed here for all 13 languages x 3 repeat factors (39
/// pairs) before any timing number from those cells is trusted.
#[test]
fn agree_on_every_language_under_the_benchs_paragraph_repetition_ladder() {
    let dataset = load_dataset();
    let mut mismatches = Vec::new();

    for entry in &dataset {
        let paragraph = entry.items.get("paragraph");
        for reps in [4usize, 16, 64] {
            let text = paragraph.repeat(reps);
            let verbora_script = detect_script(&text);
            let whatlang_script = whatlang::detect_script(&text);
            if !scripts_agree(verbora_script, whatlang_script) {
                mismatches.push(format!(
                    "{} x{reps}: verbora={verbora_script:?} whatlang={whatlang_script:?}",
                    entry.iso639_1
                ));
            }
        }
    }

    assert!(
        mismatches.is_empty(),
        "script detectors disagree on repeated-paragraph inputs:\n{}",
        mismatches.join("\n")
    );
}

/// Single-script strings *derived from the dataset itself* (filtered to
/// one Unicode block), so agreement is checked with the majority vote
/// removed from the equation — each input contains exactly one detectable
/// script, and any disagreement would be a raw classification-table
/// difference rather than a vote-threshold difference. The Japanese
/// paragraph contributes its Hiragana and Han extracts (it has no
/// Katakana at any tier — that shape is covered by
/// `tests/transliteration_convention_diff.rs`'s katakana inputs on the
/// transliteration side); English contributes the Latin extract; Russian
/// Cyrillic; Hindi Devanagari.
#[test]
fn agree_on_single_script_extracts_from_the_dataset() {
    let dataset = load_dataset();
    let extract = |iso: &str, filter: fn(&char) -> bool| -> String {
        dataset
            .iter()
            .find(|l| l.iso639_1 == iso)
            .unwrap_or_else(|| panic!("{iso} missing from dataset"))
            .items
            .get("paragraph")
            .chars()
            .filter(filter)
            .collect()
    };

    let extracts = [
        ("latin", extract("en", |c| c.is_ascii_alphabetic())),
        (
            "cyrillic",
            extract("ru", |c| ('\u{0400}'..='\u{04FF}').contains(c)),
        ),
        (
            "devanagari",
            extract("hi", |c| ('\u{0900}'..='\u{097F}').contains(c)),
        ),
        (
            "hiragana",
            extract("ja", |c| ('\u{3041}'..='\u{3096}').contains(c)),
        ),
        (
            "han",
            extract("ja", |c| ('\u{4E00}'..='\u{9FFF}').contains(c)),
        ),
    ];

    for (label, text) in &extracts {
        assert!(
            !text.is_empty(),
            "{label} extract came out empty — the dataset changed shape"
        );
        let verbora_script = detect_script(text);
        let whatlang_script = whatlang::detect_script(text);
        assert!(
            scripts_agree(verbora_script, whatlang_script),
            "{label}: verbora={verbora_script:?} whatlang={whatlang_script:?} text={text:?}"
        );
    }
}

/// `scripts_agree`'s `(None, None)` arm, exercised for real: on input with
/// no script content at all (empty, whitespace, digits, punctuation), both
/// classifiers abstain — neither invents a script from nothing, and
/// neither panics. This is the abstention edge of the agreement domain,
/// which none of the dataset-driven tests above can reach (every dataset
/// item has script content by construction).
#[test]
fn both_abstain_on_scriptless_input() {
    for text in ["", "   ", "\t\n", "1234 5678", "!!!", "..."] {
        assert_eq!(
            detect_script(text),
            None,
            "verbora invented a script for {text:?}"
        );
        assert_eq!(
            whatlang::detect_script(text),
            None,
            "whatlang invented a script for {text:?}"
        );
    }
}
