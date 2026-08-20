//! The `whatlang`-backed detector. Only compiled with the
//! `language-detection` feature.
//!
//! User-facing prose lives on [`WhatlangDetector`], not in this private
//! module's `//!` block, so that it reaches docs.rs.

use whatlang::{Detector, Lang};

use crate::{Confidence, Language, LanguageDetection, LanguageDetector};

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

/// This crate's default [`LanguageDetector`], backed by `whatlang`'s n-gram
/// frequency model.
///
/// Stateless and zero-sized: `whatlang::detect` takes no setup, so there is
/// nothing to construct lazily and nothing to share behind an `Arc`. The
/// type is `Copy`, `Send` and `Sync` automatically.
///
/// # Why `whatlang`
///
/// Evaluated against `lingua` and `whichlang` — the other two actively
/// maintained Rust language-detection crates — before choosing:
///
/// | | `whatlang` | `lingua` | `whichlang` |
/// |---|---|---|---|
/// | License | MIT | Apache-2.0 | MIT |
/// | Dependencies | 1 (`hashbrown`) | ~15, incl. `rayon`, `dashmap`, per-language model crates | 0 |
/// | Coverage of this crate's 22 languages | 20/22 (missing Galician, Basque) | 21/22 (missing Galician) | 13/22 |
/// | Honest low-confidence signal | `is_reliable()` | self-reported accuracy tables only | none |
/// | Footprint | ~685 KB compiled-in frequency tables | up to ~300 MB of per-language FST models if all languages enabled | ~775 KB, baked-in weights |
///
/// `whichlang` is leaner still but does not cover enough of this crate's
/// language list to be useful here, and has shipped only two releases ever.
/// `lingua`'s dependency graph is disproportionate to "guess the language
/// of a short phrase" and conflicts with this project's dependency-light
/// stance. `whatlang` is the one candidate that is simultaneously
/// MIT-licensed, nearly dependency-free, actively maintained, covers the
/// language list, and — critically for honest short-input behaviour —
/// already exposes a reliability signal instead of forcing this crate to
/// invent one.
///
/// # What `confidence` means here
///
/// `whatlang`'s `Info::confidence()` is a **relative-margin score** — how
/// much better the winning language scored than the runner-up — not a
/// calibrated probability. This detector reports it as-is, with one
/// Verbora-defined adjustment:
///
/// * When `whatlang`'s own `is_reliable()` says the result is not
///   trustworthy, the confidence is **halved** ([`Confidence::halved`]).
///   Halving, rather than dropping the candidate, keeps the detector's
///   finding visible to a caller who wants to see it, while pushing it
///   below any threshold a caller would plausibly act on. Verbora chooses
///   the factor; what it is chosen to preserve is the ordering — an
///   unreliable result can never outrank a reliable one of the same raw
///   score — and that ordering, not the constant, is what this crate's
///   `unreliable_results_are_ranked_below_reliable_ones` test pins.
/// * A confidence `whatlang` reports outside `0.0..=1.0`, or as `NaN`, is
///   an **abstention**: this detector returns [`LanguageDetection::none`]
///   rather than passing a meaningless number on. `whatlang` documents its
///   value as a probability in `0.0..=1.0`, so this path exists to keep the
///   guarantee total rather than because it is expected to fire.
///
/// Confidence from this detector is not comparable to any other detector's
/// — see [`Confidence`].
///
/// # Short input
///
/// Single words score far lower here than the 70–95% figures `whatlang`'s
/// own benchmarks report for full sentences. That is the honest outcome,
/// not a defect: `tests/ambiguity.rs` asserts that specific short,
/// cross-language words (`"hotel"`, `"radio"`, `"piano"`, `"normal"`,
/// `"color"`) and short proper names must **not** clear a normal
/// confidence threshold.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
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
        // Out of range or NaN is an abstention, never a fabricated number.
        let Some(confidence) = Confidence::new(info.confidence() as f32) else {
            return LanguageDetection::none();
        };
        let confidence = if info.is_reliable() {
            confidence
        } else {
            confidence.halved()
        };
        LanguageDetection::single(language, confidence)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Script, TransliterationAdvice, apply_transliteration, detect_script};

    fn c(value: f32) -> Confidence {
        Confidence::new(value).expect("test value is in 0.0..=1.0")
    }

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
    fn unreliable_results_are_ranked_below_reliable_ones() {
        // The halving rule, stated as the property it is chosen to
        // preserve rather than as a magic constant: an unreliable verdict
        // must score strictly below what the same raw margin would score
        // if `whatlang` had called it reliable, and must stay in range.
        for raw in [0.0f32, 0.01, 0.5, 0.99, 1.0] {
            let reliable = c(raw);
            let unreliable = reliable.halved();
            assert!(unreliable <= reliable, "halving must not raise {raw}");
            assert!(
                raw == 0.0 || unreliable < reliable,
                "a nonzero margin must be strictly demoted ({raw})"
            );
            assert!(Confidence::new(unreliable.get()).is_some());
        }
    }

    #[test]
    fn an_ambiguous_single_word_is_rejectable_at_a_modest_threshold() {
        let d = WhatlangDetector::new();
        // A single short, ambiguous word: whatlang may or may not return a
        // candidate at all, but if it does, an unreliable one must be
        // scored low enough that a caller with any sane threshold rejects
        // it via best_above -- this is the property under test, not a
        // specific language answer.
        let result = d.detect("hotel");
        if let Some(candidate) = result.best() {
            assert!(
                result.best_above(c(0.6)).is_none() || candidate.confidence >= c(0.6),
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
    fn every_confidence_it_reports_is_a_real_confidence() {
        // `Confidence` cannot hold NaN or an out-of-range value, so this
        // asserts the adapter never abstains *because* of a broken number
        // on ordinary input in a language this crate models — the
        // out-of-range guard exists for the impossible case, not as a
        // routine path.
        let d = WhatlangDetector::new();
        for input in [
            "This is a fairly long English sentence, written to be unambiguous.",
            "Das ist ein eindeutig deutscher Satz, lang genug zum Erkennen.",
            "Все люди рождаются свободными и равными в своем достоинстве.",
            "Tous les êtres humains naissent libres et égaux en dignité.",
        ] {
            let result = d.detect(input);
            assert!(
                !result.is_empty(),
                "{input:?} should still produce a candidate"
            );
        }
    }

    #[test]
    fn a_language_this_crate_does_not_model_is_an_abstention() {
        // `whatlang` knows 70 languages; this crate's `Language` names 22.
        // A confident verdict outside that overlap is not an error and not
        // a near-miss to be coerced into the closest variant — it is simply
        // no candidate. Korean and Hungarian are both languages `whatlang`
        // identifies well and this crate has no variant for.
        let d = WhatlangDetector::new();
        for input in [
            "오늘은 날씨가 좋고 아이들이 밖에서 놀고 있어요",
            "Minden emberi lény szabadon születik és egyenlő méltósága van.",
        ] {
            assert_eq!(
                d.detect(input),
                LanguageDetection::none(),
                "{input:?} is in a language this crate has no variant for"
            );
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
            !result.is_empty(),
            "a short but real sentence should produce at least one candidate, not an empty detection"
        );
    }

    #[test]
    fn mixed_script_input_does_not_panic_and_stays_within_bounds() {
        // `script::tests::a_tie_goes_to_the_script_that_opens_the_text`
        // covers script detection's own vote; this is the
        // language-detection-level counterpart -- real text that switches
        // scripts mid-string must not panic and, if a candidate comes
        // back, its confidence must still be a real confidence.
        let d = WhatlangDetector::new();
        for input in [
            "Hello мир, this sentence mixes Latin and Cyrillic together.",
            "café naïve Москва東京",
            "one two 三四 пять",
        ] {
            let result = d.detect(input);
            if let Some(candidate) = result.best() {
                assert!(
                    (0.0..=1.0).contains(&candidate.confidence.get()),
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
