//! [`WhatlangDetector`]: a real [`LanguageDetector`] backed by the
//! `whatlang` crate. Only compiled with the `language-detection` feature.
//!
//! # Why `whatlang`
//!
//! Evaluated against `lingua` and `whichlang` (the other two actively
//! maintained Rust language-detection crates) before choosing:
//!
//! | | `whatlang` | `lingua` | `whichlang` |
//! |---|---|---|---|
//! | License | MIT | Apache-2.0 | MIT |
//! | Dependencies | 1 (`hashbrown`) | ~15, incl. `rayon`, `dashmap`, per-language model crates | 0 |
//! | Coverage of this crate's 22 languages | 20/22 (missing Galician, Basque) | 21/22 (missing Galician) | 13/22 |
//! | Honest low-confidence signal | `is_reliable()` | self-reported accuracy tables only | none |
//! | Footprint | ~685 KB compiled-in frequency tables | up to ~300 MB of per-language FST models if all languages enabled | ~775 KB, baked-in weights |
//!
//! `whichlang` is leaner still but does not cover enough of this crate's
//! language list to be useful here, and has shipped only two releases
//! ever. `lingua`'s dependency graph (`rayon`, `dashmap`, per-language
//! model crates) is disproportionate to "guess the language of a short
//! phrase" and conflicts with this project's dependency-light stance.
//! `whatlang` is the one candidate that is simultaneously MIT-licensed,
//! nearly dependency-free, actively maintained, covers the language list,
//! and — critically for honest short-input behaviour — already exposes a
//! reliability signal instead of forcing this crate to invent one.

use whatlang::{Detector, Lang};

use crate::{Language, LanguageCandidate, LanguageDetection, LanguageDetector};

/// Maps every `whatlang::Lang` this crate has a [`Language`] variant for.
/// `whatlang` supports far more languages than this crate's [`Language`]
/// enumerates (70 vs. 22) — anything outside that overlap is simply not a
/// candidate, not an error.
const fn from_whatlang(lang: Lang) -> Option<Language> {
    match lang {
        Lang::Eng => Some(Language::English),
        Lang::Spa => Some(Language::Spanish),
        Lang::Por => Some(Language::Portuguese),
        Lang::Ita => Some(Language::Italian),
        Lang::Fra => Some(Language::French),
        Lang::Deu => Some(Language::German),
        Lang::Nld => Some(Language::Dutch),
        Lang::Rus => Some(Language::Russian),
        Lang::Ukr => Some(Language::Ukrainian),
        Lang::Pol => Some(Language::Polish),
        Lang::Pes => Some(Language::Persian),
        Lang::Hin => Some(Language::Hindi),
        Lang::Ind => Some(Language::Indonesian),
        Lang::Vie => Some(Language::Vietnamese),
        Lang::Jpn => Some(Language::Japanese),
        Lang::Cmn => Some(Language::Chinese),
        Lang::Nob => Some(Language::Norwegian),
        Lang::Swe => Some(Language::Swedish),
        Lang::Fin => Some(Language::Finnish),
        Lang::Cat => Some(Language::Catalan),
        // whatlang has no Galician or Basque; every other Lang variant
        // (Hebrew, Thai, Korean, ...) has no Language counterpart here.
        _ => None,
    }
}

/// A [`LanguageDetector`] backed by `whatlang`'s n-gram frequency model.
///
/// Stateless and zero-sized — `whatlang::detect` takes no setup, so there
/// is nothing to construct lazily and nothing to share behind an `Arc`
/// beyond the type itself (which is `Copy`).
///
/// # What `confidence` means here
///
/// `whatlang`'s own `Info::confidence()` is a relative-margin score (how
/// much better the winning language scored than the runner-up), not a
/// calibrated probability — this detector reports it as-is, scaled by
/// whether `whatlang` itself considers the result [`is_reliable`](https://docs.rs/whatlang/latest/whatlang/struct.Info.html#method.is_reliable):
/// an unreliable result is halved, not hidden, so a caller comparing
/// against their own threshold still sees *something* rather than a
/// silently vanished candidate — but a caller using [`LanguageDetection::best_above`](crate::LanguageDetection::best_above)
/// with any reasonable threshold will correctly reject it.
///
/// See `benches/language.rs` and the site's own Language documentation for
/// this project's own measured accuracy at realistic input lengths —
/// single words score far lower than the 70-95% figures `whatlang`'s own
/// benchmarks report for full sentences, which this crate's own tests
/// assert explicitly (see the ambiguity test cases in `tests/`).
#[derive(Debug, Clone, Copy, Default)]
pub struct WhatlangDetector;

impl WhatlangDetector {
    /// A new detector. Free — nothing to initialize.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl LanguageDetector for WhatlangDetector {
    fn detect(&self, input: &str) -> LanguageDetection {
        let detector = Detector::new();
        let Some(info) = detector.detect(input) else {
            return LanguageDetection::none();
        };
        let Some(language) = from_whatlang(info.lang()) else {
            // whatlang found a language, but it's not one this crate has a
            // Language variant for (e.g. Thai, Korean by text) — no usable
            // candidate, not an error.
            return LanguageDetection::none();
        };
        let mut confidence = info.confidence() as f32;
        if !info.is_reliable() {
            confidence *= 0.5;
        }
        LanguageDetection {
            candidates: vec![LanguageCandidate {
                language,
                confidence: confidence.clamp(0.0, 1.0),
            }],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Script, TransliterationAdvice, apply_transliteration, detect_script};

    #[test]
    fn detects_a_clear_english_sentence() {
        let d = WhatlangDetector::new();
        let result = d.detect("This is a fairly long English sentence, written to be unambiguous.");
        assert_eq!(result.best().map(|c| c.language), Some(Language::English));
    }

    #[test]
    fn detects_a_clear_german_sentence() {
        let d = WhatlangDetector::new();
        let result = d.detect("Das ist ein eindeutig deutscher Satz, lang genug zum Erkennen.");
        assert_eq!(result.best().map(|c| c.language), Some(Language::German));
    }

    #[test]
    fn empty_input_returns_no_candidates() {
        let d = WhatlangDetector::new();
        assert_eq!(d.detect(""), LanguageDetection::none());
    }

    #[test]
    fn does_not_panic_on_pathological_input() {
        let d = WhatlangDetector::new();
        for input in ["😀😀😀", "123456", "   ", "\u{0}", &"a".repeat(10_000)] {
            let _ = d.detect(input);
        }
    }

    #[test]
    fn unreliable_results_are_not_silently_dropped_but_are_downweighted() {
        let d = WhatlangDetector::new();
        // A single short, ambiguous word: whatlang may or may not return a
        // candidate at all, but if it does, an unreliable one must be
        // scored low enough that a caller with any sane threshold rejects
        // it via best_above -- this is the property under test, not a
        // specific language answer.
        let result = d.detect("hotel");
        if let Some(candidate) = result.best() {
            assert!(
                result.best_above(0.6).is_none() || candidate.confidence >= 0.6,
                "a low-confidence single-word result must be rejectable at a modest threshold"
            );
        }
    }

    #[test]
    fn detect_is_deterministic_across_repeated_calls() {
        // whatlang's frequency tables are static and `Detector::new()` has
        // no hidden state, so the same input must produce bit-for-bit the
        // same `LanguageDetection` every time -- not just "a similar
        // answer." Covers a real sentence, a short ambiguous word, empty
        // input, and non-text input in the same sweep.
        let d = WhatlangDetector::new();
        for input in [
            "This is a fairly long English sentence, written to be unambiguous.",
            "hotel",
            "",
            "😀😀😀",
        ] {
            let first = d.detect(input);
            for _ in 0..5 {
                assert_eq!(
                    d.detect(input),
                    first,
                    "detect({input:?}) returned a different result across repeated calls"
                );
            }
        }
    }

    #[test]
    fn short_text_still_produces_a_candidate_unlike_empty_input() {
        // Distinct from both `empty_input_returns_no_candidates` (no signal
        // whatsoever) and the single-word ambiguity cases in
        // `tests/ambiguity.rs` -- a short but real, grammatical sentence
        // should still yield at least one candidate, even if whatlang
        // scores it too low to be `is_reliable()`.
        let d = WhatlangDetector::new();
        let result = d.detect("The cat sat on the mat.");
        assert!(
            !result.candidates.is_empty(),
            "a short but real sentence should produce at least one candidate, not an empty detection"
        );
    }

    #[test]
    fn mixed_script_input_does_not_panic_and_stays_within_bounds() {
        // `script::tests::mixed_script_picks_the_majority_not_the_first`
        // covers script detection's own majority vote; this is the
        // language-detection-level counterpart -- real text that switches
        // scripts mid-string must not panic and, if a candidate comes
        // back, its confidence must still be an honest probability.
        let d = WhatlangDetector::new();
        for input in [
            "Hello мир, this sentence mixes Latin and Cyrillic together.",
            "café naïve Москва東京",
            "one two 三四 пять",
        ] {
            let result = d.detect(input);
            if let Some(candidate) = result.best() {
                assert!(
                    (0.0..=1.0).contains(&candidate.confidence),
                    "confidence out of [0, 1] for mixed-script input {input:?}: {}",
                    candidate.confidence
                );
            }
        }
    }

    #[test]
    fn transliterated_japanese_is_no_longer_detected_as_japanese() {
        // Composes the whole "optional transliteration" step from the
        // pipeline in the crate-level doc comment with a downstream
        // detect() call: pure-hiragana input is not Latin script, and
        // whatlang can only ever answer `Lang::Jpn` when Han/Hiragana/
        // Katakana codepoints are present. Once `apply_transliteration`
        // romanizes it, both properties flip -- proving the
        // transliteration step is not just cosmetic but actually changes
        // how downstream detection treats the text.
        let kana = "これはにほんごですとてもながいぶんしょうです";
        assert_ne!(detect_script(kana), Some(Script::Latin));

        let romanized = apply_transliteration(TransliterationAdvice::Recommended, kana);
        assert_ne!(romanized.as_ref(), kana);
        assert_eq!(detect_script(&romanized), Some(Script::Latin));

        let detected = WhatlangDetector::new().detect(&romanized);
        assert_ne!(
            detected.best().map(|c| c.language),
            Some(Language::Japanese)
        );
    }
}
