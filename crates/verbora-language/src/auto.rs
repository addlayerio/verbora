//! [`AutoPhoneticStrategy`]: detect, then recommend, in one call — with an
//! explicit, caller-chosen confidence threshold, never a silent guess.

use crate::{LanguageDetection, LanguageDetector, PhoneticStrategy, recommend};

/// Combines a [`LanguageDetector`] with [`recommend`], gated by a
/// caller-supplied confidence threshold.
///
/// This is the *automatic* path — see the crate-level doc comment for why
/// it is a distinct, opt-in type rather than something [`recommend`] does
/// for you implicitly. `AutoPhoneticStrategy` is generic over any
/// [`LanguageDetector`], not tied to `whatlang` — construct it with
/// [`crate::WhatlangDetector`] (behind the `language-detection` feature)
/// or your own implementation.
pub struct AutoPhoneticStrategy<D> {
    detector: D,
    threshold: f32,
}

/// What [`AutoPhoneticStrategy::detect_and_recommend`] answers: the raw
/// detection (so a caller can inspect *why* a recommendation was or was
/// not made) plus the strategy, if confidence cleared the threshold.
#[derive(Debug, Clone, PartialEq)]
pub struct AutoResult {
    /// The detector's raw output — every candidate it considered, however
    /// low-confidence.
    pub detection: LanguageDetection,
    /// `Some` only when `detection.best()`'s confidence met the
    /// configured threshold. `None` here means "not confident enough to
    /// recommend anything automatically" — it does *not* mean the
    /// detector found nothing at all; check `detection` for that.
    pub strategy: Option<PhoneticStrategy>,
}

impl<D: LanguageDetector> AutoPhoneticStrategy<D> {
    /// Builds an auto-strategy around `detector`, treating any candidate
    /// with confidence `>= threshold` as safe to act on automatically.
    ///
    /// There is no default threshold: this project measured real
    /// detector behaviour (see `benches/language.rs` and the site's own
    /// Language documentation) and found it varies enough by input length
    /// that a single baked-in number would either be too permissive for
    /// single words or too conservative for full sentences. Pick a value
    /// that matches your own tolerance for an occasional wrong guess.
    #[must_use]
    pub fn new(detector: D, threshold: f32) -> Self {
        Self {
            detector,
            threshold,
        }
    }

    /// Detects `input`'s language and, only if confident enough,
    /// recommends a phonetic strategy for it.
    #[must_use]
    pub fn detect_and_recommend(&self, input: &str) -> AutoResult {
        let detection = self.detector.detect(input);
        let strategy = detection
            .best_above(self.threshold)
            .map(|candidate| recommend(candidate.language));
        AutoResult {
            detection,
            strategy,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Language, LanguageCandidate};

    struct FixedDetector(f32);
    impl LanguageDetector for FixedDetector {
        fn detect(&self, _input: &str) -> LanguageDetection {
            LanguageDetection {
                candidates: vec![LanguageCandidate {
                    language: Language::English,
                    confidence: self.0,
                }],
            }
        }
    }

    #[test]
    fn confident_detection_produces_a_strategy() {
        let auto = AutoPhoneticStrategy::new(FixedDetector(0.9), 0.5);
        let result = auto.detect_and_recommend("anything");
        assert!(result.strategy.is_some());
        assert_eq!(result.detection.best().unwrap().language, Language::English);
    }

    #[test]
    fn low_confidence_produces_no_strategy_but_keeps_the_detection() {
        let auto = AutoPhoneticStrategy::new(FixedDetector(0.1), 0.5);
        let result = auto.detect_and_recommend("anything");
        assert!(result.strategy.is_none());
        // The raw detection is still visible -- "not confident enough to
        // act automatically" is not the same as "found nothing".
        assert_eq!(result.detection.best().unwrap().language, Language::English);
    }

    struct NothingDetector;
    impl LanguageDetector for NothingDetector {
        fn detect(&self, _input: &str) -> LanguageDetection {
            LanguageDetection::none()
        }
    }

    #[test]
    fn no_candidates_at_all_produces_no_strategy() {
        let auto = AutoPhoneticStrategy::new(NothingDetector, 0.0);
        let result = auto.detect_and_recommend("");
        assert!(result.strategy.is_none());
        assert!(result.detection.candidates.is_empty());
    }
}
