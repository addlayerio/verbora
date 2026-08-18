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
//! 3. The inputs `benches/language.rs` derives beyond the dataset's own
//!    tiers (whole-paragraph x4/x16/x64 repetitions — its length-ladder
//!    extension) don't push any detector out of its correct-answer regime:
//!    a repeated paragraph has the identical n-gram distribution as the
//!    paragraph itself, and this file *executes* that claim per detector
//!    per language rather than asserting it in prose.
//! 4. The wrapper-overhead group's premise — `WhatlangDetector` is raw
//!    `whatlang` plus only a 22-language mapping — is checked as an exact
//!    per-item contract over the whole dataset, and each detector is
//!    checked to be deterministic call-over-call (Criterion's model:
//!    identical input, identical work).

use competitive_rust::language_support::{
    TIERS, lingua_iso, lingua_restricted_languages, load_dataset, whichlang_iso,
};
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
            let _ = whichlang_iso(answer);
        }
    }
}

/// Extends `all_three_detectors_agree_on_english_paragraph`'s wiring check
/// from 1 case to 78: at the `sentence` and `paragraph` tiers, all three
/// detectors answer *correctly* on all 13 dataset languages (verified by a
/// probe run before being asserted here). This is still a harness-wiring
/// test, not an accuracy claim about hard inputs — the two short tiers,
/// where the detectors genuinely (and legitimately) disagree, stay the
/// exclusive domain of `examples/language_accuracy.rs`'s scored report and
/// are deliberately NOT asserted here.
#[test]
fn all_three_detectors_agree_on_every_language_at_sentence_and_paragraph_tiers() {
    let dataset = load_dataset();
    let verbora = WhatlangDetector::new();
    let lingua_detector =
        lingua::LanguageDetectorBuilder::from_languages(&lingua_restricted_languages()).build();

    let mut failures = Vec::new();
    for tier in ["sentence", "paragraph"] {
        for entry in &dataset {
            let text = entry.items.get(tier);
            let expected = entry.iso639_1.as_str();

            let verbora_answer = verbora
                .detect(text)
                .best()
                .map(|c| c.language.iso639_1().to_owned());
            if verbora_answer.as_deref() != Some(expected) {
                failures.push(format!(
                    "verbora {expected}/{tier}: got {verbora_answer:?} text={text:?}"
                ));
            }

            let lingua_answer = lingua_detector.detect_language_of(text).map(lingua_iso);
            if lingua_answer.as_deref() != Some(expected) {
                failures.push(format!(
                    "lingua {expected}/{tier}: got {lingua_answer:?} text={text:?}"
                ));
            }

            let whichlang_answer = whichlang_iso(whichlang::detect_language(text));
            if whichlang_answer != expected {
                failures.push(format!(
                    "whichlang {expected}/{tier}: got {whichlang_answer:?} text={text:?}"
                ));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "detector(s) wrong on the easy tiers ({} of {} detector-item cells):\n{}",
        failures.len(),
        dataset.len() * 2 * 3,
        failures.join("\n")
    );
}

/// `benches/language.rs`'s length ladder extends past the dataset's own
/// `paragraph` tier by whole-paragraph repetition (x4/x16/x64). Before any
/// timing number from those cells is trusted, this executes the fairness
/// claim the bench's doc comment makes: repetition preserves the n-gram
/// distribution, so every detector must still produce the *correct* answer
/// on every repeated paragraph — 117 detection calls, zero tolerance.
#[test]
fn detection_stays_correct_under_the_benchs_paragraph_repetition_ladder() {
    let dataset = load_dataset();
    let verbora = WhatlangDetector::new();
    let lingua_detector =
        lingua::LanguageDetectorBuilder::from_languages(&lingua_restricted_languages()).build();

    let mut failures = Vec::new();
    for entry in &dataset {
        let paragraph = entry.items.get("paragraph");
        let expected = entry.iso639_1.as_str();
        for reps in [4usize, 16, 64] {
            let text = paragraph.repeat(reps);

            let verbora_answer = verbora
                .detect(&text)
                .best()
                .map(|c| c.language.iso639_1().to_owned());
            if verbora_answer.as_deref() != Some(expected) {
                failures.push(format!(
                    "verbora {expected} x{reps}: got {verbora_answer:?}"
                ));
            }

            let lingua_answer = lingua_detector
                .detect_language_of(text.as_str())
                .map(lingua_iso);
            if lingua_answer.as_deref() != Some(expected) {
                failures.push(format!("lingua {expected} x{reps}: got {lingua_answer:?}"));
            }

            let whichlang_answer = whichlang_iso(whichlang::detect_language(&text));
            if whichlang_answer != expected {
                failures.push(format!(
                    "whichlang {expected} x{reps}: got {whichlang_answer:?}"
                ));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "repetition changed a detector's answer — the bench's extended sizes \
         would be timing a different classification problem:\n{}",
        failures.join("\n")
    );
}

/// The exact contract behind `benches/language.rs`'s wrapper-overhead group
/// ("`WhatlangDetector` is raw `whatlang` plus a 22-language mapping, so
/// this group measures wrapper overhead, not a rival algorithm"), executed
/// per item over the full dataset x all four tiers rather than cited:
///
/// - raw `whatlang` abstains => the wrapper abstains;
/// - raw `whatlang` answers a language Verbora has a variant for => the
///   wrapper answers the *same* language;
/// - raw `whatlang` answers a language outside Verbora's 22 (a real case:
///   the Dutch `short_word` tier detects as Afrikaans, which Verbora does
///   not model) => the wrapper abstains — a mapping consequence, never a
///   different detection.
#[test]
fn wrapper_answers_exactly_raw_whatlang_filtered_to_verboras_languages() {
    // whatlang's `Lang::code()` (ISO 639-3) -> Verbora's `iso639_1()` for
    // the 20-language overlap `crates/verbora-language/src/whatlang_detector.rs`
    // maps (its own `from_whatlang` table, restated in ISO-code form; the
    // point of this test is to catch the two drifting apart).
    const OVERLAP: [(&str, &str); 20] = [
        ("eng", "en"),
        ("spa", "es"),
        ("por", "pt"),
        ("ita", "it"),
        ("fra", "fr"),
        ("deu", "de"),
        ("nld", "nl"),
        ("rus", "ru"),
        ("ukr", "uk"),
        ("pol", "pl"),
        ("pes", "fa"),
        ("hin", "hi"),
        ("ind", "id"),
        ("vie", "vi"),
        ("jpn", "ja"),
        ("cmn", "zh"),
        ("nob", "no"),
        ("swe", "sv"),
        ("fin", "fi"),
        ("cat", "ca"),
    ];
    let expected_wrapper_answer = |raw_code: &str| -> Option<&'static str> {
        OVERLAP
            .iter()
            .find(|(code, _)| *code == raw_code)
            .map(|(_, iso)| *iso)
    };

    let dataset = load_dataset();
    let verbora = WhatlangDetector::new();
    for tier in TIERS {
        for entry in &dataset {
            let text = entry.items.get(tier);
            let wrapper = verbora
                .detect(text)
                .best()
                .map(|c| c.language.iso639_1().to_owned());
            let raw = whatlang::Detector::new().detect(text);
            let expected = raw
                .as_ref()
                .and_then(|info| expected_wrapper_answer(info.lang().code()));
            assert_eq!(
                wrapper.as_deref(),
                expected,
                "{}/{tier}: wrapper={wrapper:?} raw={raw:?} — the wrapper did \
                 something other than map raw whatlang's answer, so the \
                 wrapper-overhead group would not be measuring pure overhead",
                entry.iso639_1
            );
        }
    }
}

/// Criterion's model requires identical work on identical input across
/// iterations. All three detectors are deterministic pure functions of
/// their input (no RNG, no time, no global mutable state) — executed here
/// as three repeated calls per (item, detector) over the full dataset
/// rather than assumed, so a hypothetical nondeterministic competitor
/// could never quietly turn a timing distribution bimodal.
#[test]
fn every_detector_is_deterministic_call_over_call() {
    let dataset = load_dataset();
    let verbora = WhatlangDetector::new();
    let lingua_detector =
        lingua::LanguageDetectorBuilder::from_languages(&lingua_restricted_languages()).build();

    for tier in TIERS {
        for entry in &dataset {
            let text = entry.items.get(tier);

            let verbora_first = verbora
                .detect(text)
                .best()
                .map(|c| c.language.iso639_1().to_owned());
            let lingua_first = lingua_detector.detect_language_of(text);
            let whichlang_first = whichlang::detect_language(text);
            for _ in 0..2 {
                assert_eq!(
                    verbora
                        .detect(text)
                        .best()
                        .map(|c| c.language.iso639_1().to_owned()),
                    verbora_first,
                    "verbora nondeterministic on {}/{tier}",
                    entry.iso639_1
                );
                assert_eq!(
                    lingua_detector.detect_language_of(text),
                    lingua_first,
                    "lingua nondeterministic on {}/{tier}",
                    entry.iso639_1
                );
                assert_eq!(
                    whichlang::detect_language(text),
                    whichlang_first,
                    "whichlang nondeterministic on {}/{tier}",
                    entry.iso639_1
                );
            }
        }
    }
}

/// Degenerate inputs (empty, whitespace-only, digits, punctuation, emoji)
/// — none is a benchmarked input, but a panic anywhere near the input
/// domain would undermine trust in the harness. The two detectors that
/// *can* abstain both abstain on all of them (executed, giving the
/// abstention half of `whichlang_never_abstains_on_the_dataset`'s
/// structural contrast a concrete footing); `whichlang` still returns
/// some `Lang` by construction — its forced guess on empty-feature input
/// is an implementation detail deliberately not asserted beyond "it maps
/// to a known variant".
#[test]
fn abstaining_detectors_abstain_on_scriptless_input_and_none_panics() {
    let verbora = WhatlangDetector::new();
    let lingua_detector =
        lingua::LanguageDetectorBuilder::from_languages(&lingua_restricted_languages()).build();

    for text in ["", "   ", "\t\n", "1234 5678", "!!!", "🙂🙂🙂", "..."] {
        assert!(
            verbora.detect(text).best().is_none(),
            "verbora unexpectedly committed to a language on {text:?}"
        );
        assert_eq!(
            lingua_detector.detect_language_of(text),
            None,
            "lingua unexpectedly committed to a language on {text:?}"
        );
        let _ = whichlang_iso(whichlang::detect_language(text));
    }
}
