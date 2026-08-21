//! The composed detect-then-recommend path. User-facing prose lives on
//! [`AutoPhoneticStrategy`] and [`AutoResult`], not in this private
//! module's `//!` block.

use crate::{Confidence, LanguageDetection, LanguageDetector, PhoneticStrategy, recommend};

/// Detection and [`recommend`] in one call, gated by a threshold **you**
/// choose.
///
/// This is the *automatic* path, and it is a distinct opt-in type rather
/// than something [`recommend`] does implicitly, because automatic
/// detection is the expensive and fallible half of the job: a caller who
/// already knows the language should call [`recommend`] and pay nothing for
/// a guess they do not need.
///
/// Generic over any [`LanguageDetector`] — pair it with
/// [`WhatlangDetector`](crate::WhatlangDetector), `HashedLinearDetector`,
/// a [`FallbackDetector`](crate::FallbackDetector) composition, or your
/// own.
///
/// # Choosing the right API
///
/// | You have | Call | Cost |
/// |---|---|---|
/// | the language already | [`recommend`] | a closed `match`; no detector, no feature, no model |
/// | only a [`Script`](crate::Script) | [`recommend_for_script`](crate::recommend_for_script) | one pass of [`detect_script`](crate::detect_script), then a `match` |
/// | raw text and no idea | `AutoPhoneticStrategy::detect_and_recommend` | a full statistical detection, plus the `match` |
///
/// The three are ordered by cost and by how much they can tell you, and
/// they are not interchangeable: the first two cannot be wrong about the
/// language because you supplied it, and the third can.
pub struct AutoPhoneticStrategy<D> {
    detector: D,
    threshold: Confidence,
}

/// What [`AutoPhoneticStrategy::detect_and_recommend`] answers: the raw
/// detection, so a caller can inspect *why* a recommendation was or was not
/// made, plus the strategy when confidence cleared the threshold.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoResult {
    /// The detector's raw output — every candidate it considered, however
    /// low-confidence.
    pub detection: LanguageDetection,
    /// `Some` only when `detection.best()`'s confidence met the configured
    /// threshold.
    ///
    /// `None` here means "not confident enough to recommend anything
    /// automatically" — it does *not* mean the detector found nothing;
    /// check [`detection`](Self::detection) for that. The two states are
    /// different and a caller may well want to handle them differently
    /// (retry with more text vs. fall back to exact matching).
    pub strategy: Option<PhoneticStrategy>,
}

impl<D: LanguageDetector> AutoPhoneticStrategy<D> {
    /// Builds an auto-strategy around `detector`, treating any candidate
    /// with confidence `>= threshold` as safe to act on automatically.
    ///
    /// There is no default threshold, and this crate will not invent one:
    /// detector confidence is defined by the detector (see
    /// [`Confidence`]), and how much of it is enough depends on what a
    /// wrong guess costs *you*. Pick a value deliberately.
    #[must_use]
    pub const fn new(detector: D, threshold: Confidence) -> Self {
        Self {
            detector,
            threshold,
        }
    }

    /// The threshold this instance was built with.
    #[must_use]
    pub const fn threshold(&self) -> Confidence {
        self.threshold
    }

    /// The detector this instance was built with.
    #[must_use]
    pub const fn detector(&self) -> &D {
        &self.detector
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
    use crate::Language;

    fn c(value: f32) -> Confidence {
        Confidence::new(value).expect("test value is in 0.0..=1.0")
    }

    struct FixedDetector(f32);
    impl LanguageDetector for FixedDetector {
        fn detect(&self, _input: &str) -> LanguageDetection {
            LanguageDetection::single(Language::English, c(self.0))
        }
    }

    #[test]
    fn confident_detection_produces_a_strategy() {
        let auto = AutoPhoneticStrategy::new(FixedDetector(0.9), c(0.5));
        let result = auto.detect_and_recommend("anything");
        assert!(result.strategy.is_some());
        assert_eq!(result.detection.best().unwrap().language, Language::English);
    }

    #[test]
    fn low_confidence_produces_no_strategy_but_keeps_the_detection() {
        let auto = AutoPhoneticStrategy::new(FixedDetector(0.1), c(0.5));
        let result = auto.detect_and_recommend("anything");
        assert!(result.strategy.is_none());
        // The raw detection is still visible -- "not confident enough to
        // act automatically" is not the same as "found nothing".
        assert_eq!(result.detection.best().unwrap().language, Language::English);
    }

    #[test]
    fn a_candidate_exactly_at_the_threshold_is_accepted() {
        // `best_above` compares with `>=`; this pins that the composed path
        // uses the same rule rather than a stricter one of its own.
        let auto = AutoPhoneticStrategy::new(FixedDetector(0.5), c(0.5));
        assert!(auto.detect_and_recommend("anything").strategy.is_some());
    }

    struct NothingDetector;
    impl LanguageDetector for NothingDetector {
        fn detect(&self, _input: &str) -> LanguageDetection {
            LanguageDetection::none()
        }
    }

    #[test]
    fn no_candidates_at_all_produces_no_strategy() {
        let auto = AutoPhoneticStrategy::new(NothingDetector, Confidence::ZERO);
        let result = auto.detect_and_recommend("");
        assert!(result.strategy.is_none());
        assert!(result.detection.is_empty());
    }

    #[test]
    fn a_zero_threshold_still_rejects_an_abstention() {
        // The lowest possible threshold accepts any candidate — but an
        // empty detection has no candidate to accept, so the two outcomes
        // stay distinguishable at every threshold.
        let auto = AutoPhoneticStrategy::new(FixedDetector(0.0), Confidence::ZERO);
        assert!(auto.detect_and_recommend("x").strategy.is_some());
        let auto = AutoPhoneticStrategy::new(NothingDetector, Confidence::ZERO);
        assert!(auto.detect_and_recommend("x").strategy.is_none());
    }

    #[test]
    fn accessors_report_what_it_was_built_with() {
        let auto = AutoPhoneticStrategy::new(FixedDetector(0.9), c(0.75));
        assert_eq!(auto.threshold(), c(0.75));
        assert!((auto.detector().0 - 0.9).abs() < f32::EPSILON);
    }
}
