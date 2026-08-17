//! The ambiguity test cases `Fase 5 Language.md` (section 28) explicitly
//! requires: single words that are legitimate content in more than one of
//! this crate's 22 languages, plus short proper names, must never be
//! reported as a single confidently-detected language.
//!
//! None of these assert a specific `Language`. That is the point: the spec's
//! own wording is "los tests no deben exigir un idioma específico si
//! lingüísticamente es ambiguo" — asserting one language would itself be the
//! false-confidence bug this module exists to prevent. What every test here
//! verifies instead is that the *uncertainty is represented honestly*:
//! either no candidate at all, or a candidate whose confidence a caller can
//! actually reject at a normal threshold.
//!
//! Only compiled when the `language-detection` feature is enabled (there is
//! no real detector to exercise otherwise): `cargo test -p verbora-language
//! --features language-detection`.
#![cfg(feature = "language-detection")]

use verbora_language::{LanguageDetector, WhatlangDetector};

/// A threshold a caller could reasonably pick for "safe to act on
/// automatically". Not a value this crate bakes in anywhere itself (see
/// [`verbora_language::AutoPhoneticStrategy::new`]'s own doc comment) — it
/// exists only so these tests have one fixed number to check ambiguous
/// input against.
const REASONABLE_THRESHOLD: f32 = 0.6;

/// Asserts `word`, detected alone, never clears [`REASONABLE_THRESHOLD`] —
/// the honest outcome for a word this short and this ambiguous, whichever
/// language `whatlang` happens to lean towards.
fn assert_ambiguous(word: &str) {
    let detection = WhatlangDetector::new().detect(word);
    assert!(
        detection.best_above(REASONABLE_THRESHOLD).is_none(),
        "{word:?} resolved to a single confident language ({:?}); a single \
         short word shared across multiple languages must not clear a \
         normal confidence threshold",
        detection.best()
    );
}

// Each of these five words is orthographically valid, common vocabulary in
// at least English and Spanish (several also in French/Italian/Portuguese);
// "hotel" and "radio" are near-identical across most of Western Europe.
// Fase 5's own spec names exactly these five as its example set.

#[test]
fn hotel_is_ambiguous() {
    assert_ambiguous("hotel");
}

#[test]
fn radio_is_ambiguous() {
    assert_ambiguous("radio");
}

#[test]
fn piano_is_ambiguous() {
    assert_ambiguous("piano");
}

#[test]
fn normal_is_ambiguous() {
    assert_ambiguous("normal");
}

#[test]
fn color_is_ambiguous() {
    assert_ambiguous("color");
}

/// Short proper names: the spec's other named category ("nombres
/// cortos/propios"). These are also the direct test of the "names are not
/// language" principle from the crate-level doc comment — a name with a
/// clear national/ethnic association (Italian surname, Japanese given name)
/// must not make this *language* detector confident, because appearing in
/// running text and *being* that text's language are different questions,
/// and a bare name carries almost no of the trigram signal whatlang scores.
#[test]
fn short_proper_names_are_ambiguous() {
    for name in ["Panichella", "Mueller", "Kenji", "Marie", "Ivan"] {
        assert_ambiguous(name);
    }
}

/// The flip side of every test above: this crate must not simply refuse to
/// detect anything. A long, unambiguous sentence should clear the
/// threshold — otherwise "ambiguous input is uncertain" would be
/// indistinguishable from "detection is broken."
#[test]
fn long_unambiguous_text_is_not_treated_as_ambiguous() {
    let detection = WhatlangDetector::new().detect(
        "This is a long, grammatically complete English sentence, written \
         specifically to be unambiguous to any reasonable language \
         detector, with enough distinct English function words to score \
         clearly above every other candidate language.",
    );
    assert!(
        detection.best_above(REASONABLE_THRESHOLD).is_some(),
        "a long, clearly English paragraph must clear a normal confidence \
         threshold — otherwise ambiguity handling would be indistinguishable \
         from detection simply not working"
    );
}

/// Empty input is the most extreme case of "no signal" — it must report
/// exactly that, not a guess.
#[test]
fn empty_input_has_no_candidates_at_all() {
    let detection = WhatlangDetector::new().detect("");
    assert!(detection.candidates.is_empty());
}
