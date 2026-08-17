//! [`LanguageDetector`]: the detection abstraction, and its honest output
//! shape.

use crate::Language;

/// Something that can guess which language a string is written in.
///
/// Implemented by [`crate::WhatlangDetector`] (behind the
/// `language-detection` feature). The core `verbora-language` API —
/// [`crate::recommend`], [`Script`](crate::Script) detection — works
/// without any implementor of this trait at all; only *automatic*
/// detection needs one.
pub trait LanguageDetector {
    /// Guesses `input`'s language. Never panics; an input this detector
    /// has no useful signal for returns an empty [`LanguageDetection`],
    /// not a made-up answer.
    fn detect(&self, input: &str) -> LanguageDetection;
}

/// One candidate language, with an honest confidence.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LanguageCandidate {
    /// The candidate language.
    pub language: Language,
    /// `0.0..=1.0`. What this means depends on the detector — see the
    /// implementing type's own doc comment. It is a real signal from that
    /// detector's own scoring, never a fabricated precision. Two
    /// detectors' confidence values are not necessarily comparable to each
    /// other.
    pub confidence: f32,
}

/// A [`LanguageDetector`]'s answer: zero or more candidates, most likely
/// first.
///
/// Empty is a valid, expected answer — for input with no useful signal
/// (too short, no supported language matched, script-only content), an
/// empty `LanguageDetection` is what a caller should get back, not a
/// low-confidence guess dressed up as a real answer. See
/// [`LanguageDetection::best_above`] for the threshold-gated read a caller
/// should normally use instead of blindly trusting
/// [`LanguageDetection::best`].
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LanguageDetection {
    /// Descending by confidence.
    pub candidates: Vec<LanguageCandidate>,
}

impl LanguageDetection {
    /// No candidates at all — the honest answer for input with no usable
    /// signal.
    #[must_use]
    pub fn none() -> Self {
        Self {
            candidates: Vec::new(),
        }
    }

    /// The single most likely candidate, regardless of how low its
    /// confidence is. Prefer [`LanguageDetection::best_above`] unless you
    /// have your own reason to trust an arbitrarily uncertain guess.
    #[must_use]
    pub fn best(&self) -> Option<&LanguageCandidate> {
        self.candidates.first()
    }

    /// The best candidate, but only if its confidence clears `threshold`.
    ///
    /// There is no built-in default threshold — what counts as "confident
    /// enough" depends on the caller's own tolerance for a wrong guess,
    /// and this crate does not assume it knows that for you. See
    /// `docs/PERFORMANCE_MATRIX.md`/the site's own Language documentation
    /// for the calibration data this project measured, if you want a
    /// starting point rather than picking blind.
    #[must_use]
    pub fn best_above(&self, threshold: f32) -> Option<&LanguageCandidate> {
        self.best().filter(|c| c.confidence >= threshold)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_detection_has_no_best() {
        let d = LanguageDetection::none();
        assert_eq!(d.best(), None);
        assert_eq!(d.best_above(0.0), None);
    }

    #[test]
    fn best_above_respects_the_threshold() {
        let d = LanguageDetection {
            candidates: vec![LanguageCandidate {
                language: Language::English,
                confidence: 0.4,
            }],
        };
        assert_eq!(
            d.best_above(0.3).map(|c| c.language),
            Some(Language::English)
        );
        assert_eq!(d.best_above(0.5), None);
    }

    #[test]
    fn best_ignores_threshold() {
        let d = LanguageDetection {
            candidates: vec![LanguageCandidate {
                language: Language::German,
                confidence: 0.01,
            }],
        };
        assert_eq!(d.best().map(|c| c.language), Some(Language::German));
    }
}
