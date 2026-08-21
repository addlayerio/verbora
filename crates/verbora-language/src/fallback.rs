//! The two-detector composition. User-facing prose lives on
//! [`FallbackDetector`], not in this private module's `//!` block.

use crate::{LanguageDetection, LanguageDetector};

/// Runs `primary`, and falls back to `secondary` only when `primary`
/// returns no candidate at all.
///
/// Generic over any two [`LanguageDetector`]s and gated behind no feature
/// of its own: pure composition, with no model, no table and no
/// dependency.
///
/// # Why this exists
///
/// This crate ships two real detectors with opposite trade-offs:
/// `HashedLinearDetector` is several hundred times faster but abstains
/// (and, on single words, sometimes commits wrongly), while
/// [`WhatlangDetector`](crate::WhatlangDetector) is this crate's accuracy
/// reference at a per-call cost that does not shrink with the input.
/// Neither is "the right one" for every caller, and choosing between them
/// per call is exactly the glue every caller would otherwise write by
/// hand.
///
/// # The rule, and why it is the narrowest one
///
/// Fall back **only when the primary returned no candidate at all** —
/// never on a candidate the secondary might disagree with. That needs no
/// cross-detector confidence comparison, which
/// [`Confidence`](crate::Confidence) says is not meaningful in the first
/// place: a detector's abstention is a statement in its own terms ("my
/// margin was below my own calibrated threshold"), and this type does
/// nothing but honour it. A low-confidence answer is an answer, and is
/// passed through untouched.
///
/// # What this composition is measured to do, and what it is not
///
/// On the workspace's published 13-language, 4-tier UDHR evaluation set
/// (`benchmarks/competitive/datasets/language-accuracy`), with
/// `HashedLinearDetector` as primary and
/// [`WhatlangDetector`](crate::WhatlangDetector) as secondary:
///
/// | tier | hashed alone | whatlang alone | this composition |
/// |---|---|---|---|
/// | short_word | 7/13 | 10/13 | 10/13 |
/// | short_phrase | 12/13 | 13/13 | 13/13 |
/// | sentence | 13/13 | 13/13 | 13/13 |
/// | paragraph | 13/13 | 13/13 | 13/13 |
/// | **total** | **45/52** | **49/52** | **49/52** |
///
/// That is 52 items — enough to show the composition recovers the
/// abstentions that cost the fast detector its short-input accuracy, and
/// **not** enough to claim general accuracy parity with
/// [`WhatlangDetector`](crate::WhatlangDetector). It does not fix a
/// *wrong* answer from the primary: at `short_word` this composition and
/// [`WhatlangDetector`](crate::WhatlangDetector) score the same 10/13 on
/// partly different items (`fr`/`it` are the fast model's confident
/// misses; `nl` is one [`WhatlangDetector`](crate::WhatlangDetector)
/// abstains on and the fast model gets right). A caller who needs the
/// reference answer on hard, short input should use
/// [`WhatlangDetector`](crate::WhatlangDetector) directly.
///
/// # Cost
///
/// The primary's, plus the secondary's only on abstention — so the saving
/// is real exactly where the primary is confident (sentences and longer),
/// and there is none at all on input both detectors find hard.
///
/// ```
/// # #[cfg(all(feature = "fast-language-detection", feature = "language-detection"))]
/// # {
/// use verbora_language::{
///     FallbackDetector, HashedLinearDetector, LanguageDetector, WhatlangDetector,
/// };
///
/// let detector = FallbackDetector::new(HashedLinearDetector::new(), WhatlangDetector::new());
/// let detection = detector.detect("Das Wetter ist heute schön und die Kinder spielen draußen.");
/// assert!(detection.best().is_some());
/// # }
/// ```
#[derive(Debug, Clone, Copy, Default)]
pub struct FallbackDetector<P, S> {
    primary: P,
    secondary: S,
}

impl<P, S> FallbackDetector<P, S> {
    /// Pairs a primary detector with the one to consult when it abstains.
    ///
    /// Order is the whole contract: `primary` is the one that runs on
    /// every call, so it should be the cheaper of the two, and `secondary`
    /// should be the one whose judgement you want on input the first
    /// declines to judge.
    pub const fn new(primary: P, secondary: S) -> Self {
        Self { primary, secondary }
    }

    /// The detector consulted first, on every call.
    pub const fn primary(&self) -> &P {
        &self.primary
    }

    /// The detector consulted only when [`primary`](Self::primary)
    /// abstains.
    pub const fn secondary(&self) -> &S {
        &self.secondary
    }
}

impl<P: LanguageDetector, S: LanguageDetector> LanguageDetector for FallbackDetector<P, S> {
    fn detect(&self, input: &str) -> LanguageDetection {
        let detection = self.primary.detect(input);
        if detection.best().is_some() {
            return detection;
        }
        self.secondary.detect(input)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Confidence, Language};

    /// A detector that always abstains — the "primary declined" half of
    /// every routing case below.
    struct Abstains;

    impl LanguageDetector for Abstains {
        fn detect(&self, _input: &str) -> LanguageDetection {
            LanguageDetection::none()
        }
    }

    /// A detector that always answers with one fixed language, so which
    /// side of the composition produced an answer is visible in the answer
    /// itself.
    struct Always(Language);

    impl LanguageDetector for Always {
        fn detect(&self, _input: &str) -> LanguageDetection {
            LanguageDetection::single(self.0, Confidence::new(0.25).expect("0.25 is in range"))
        }
    }

    #[test]
    fn primarys_answer_is_returned_untouched() {
        // Not just "the right language": the whole detection, confidence
        // included, must be the primary's own — this type never rescores.
        let d = FallbackDetector::new(Always(Language::German), Always(Language::English));
        let got = d.detect("whatever");
        assert_eq!(got, Always(Language::German).detect("whatever"));
    }

    #[test]
    fn secondary_runs_only_when_the_primary_abstains() {
        let d = FallbackDetector::new(Abstains, Always(Language::English));
        assert_eq!(
            d.detect("whatever").best().map(|c| c.language),
            Some(Language::English)
        );
    }

    #[test]
    fn both_abstaining_still_abstains() {
        // The composition must not invent an answer when neither side has
        // one — abstention is a valid result, not a failure to handle.
        let d = FallbackDetector::new(Abstains, Abstains);
        assert_eq!(d.detect("whatever"), LanguageDetection::none());
    }

    #[test]
    fn a_low_confidence_answer_is_not_an_abstention() {
        // The trigger is "no candidate", never "a candidate I dislike":
        // confidence 0.25 from the primary must not reach the secondary,
        // because cross-detector confidence comparison is exactly what
        // this type refuses to do.
        let d = FallbackDetector::new(Always(Language::Polish), Always(Language::English));
        assert_eq!(
            d.detect("whatever").best().map(|c| c.language),
            Some(Language::Polish)
        );
    }

    #[test]
    fn composes_with_auto_phonetic_strategy() {
        // Whatever a caller can do with one detector they can do with a
        // composed one — the trait is the contract.
        let auto = crate::AutoPhoneticStrategy::new(
            FallbackDetector::new(Abstains, Always(Language::German)),
            Confidence::new(0.1).expect("0.1 is in range"),
        );
        let result = auto.detect_and_recommend("irrelevant");
        assert_eq!(
            result.detection.best().map(|c| c.language),
            Some(Language::German)
        );
    }

    #[cfg(all(feature = "fast-language-detection", feature = "language-detection"))]
    #[test]
    fn recovers_the_fast_detectors_abstentions_on_real_text() {
        // The pairing this type exists for, on inputs where the fast
        // detector is known to abstain (single words) and on inputs where
        // it is known to commit (sentences): the composition must answer
        // in both cases, and must not disturb the confident case.
        use crate::{HashedLinearDetector, WhatlangDetector};

        let fast = HashedLinearDetector::new();
        let composed = FallbackDetector::new(HashedLinearDetector::new(), WhatlangDetector::new());

        let sentence = "The weather is beautiful today and the children are playing outside.";
        assert_eq!(composed.detect(sentence), fast.detect(sentence));

        // "dignity" is the UDHR dataset's own English short_word item, and
        // one the fast model abstains on (see the module docs' table).
        assert!(fast.detect("dignity").best().is_none());
        assert_eq!(
            composed.detect("dignity").best().map(|c| c.language),
            Some(Language::English)
        );
    }
}
