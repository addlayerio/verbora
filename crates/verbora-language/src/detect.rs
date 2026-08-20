//! The detection abstraction and its output shape.
//!
//! This module is private; every user-facing sentence below lives on a
//! public item, not in this `//!` block, so that it reaches docs.rs.

use std::cmp::{Ordering, Reverse};
use std::fmt;

use crate::Language;

/// How sure a detector is, as a number in `0.0..=1.0` that is never `NaN`.
///
/// # Why this is a type and not an `f32`
///
/// A bare `f32` confidence has two values that make a detection
/// meaningless: `NaN`, which is neither above nor below any threshold and
/// so silently turns every comparison false, and anything outside
/// `0.0..=1.0`, which makes "confidence" mean nothing at all. Both are
/// unrepresentable here: [`Confidence::new`] is the only way in from a
/// float and it returns [`None`] for either. Everything downstream —
/// sorting candidates, comparing against a threshold — is then total, with
/// no `unwrap`, no `partial_cmp` fallback, and no `NaN` escape.
///
/// # What the number means
///
/// **Nothing, on its own.** Confidence is defined by the detector that
/// produced it, and two detectors' values are not comparable to each other:
/// [`WhatlangDetector`](crate::WhatlangDetector)'s is `whatlang`'s
/// relative-margin score, `HashedLinearDetector`'s is a squash of a linear
/// model's score margin, and a caller's own detector may report something
/// else again. Each type documents its own. What *is* guaranteed across all
/// of them is the direction: within one detector, higher means more sure.
///
/// ```
/// use verbora_language::Confidence;
///
/// assert!(Confidence::new(0.5).is_some());
/// assert_eq!(Confidence::new(f32::NAN), None);
/// assert_eq!(Confidence::new(1.5), None);
/// assert_eq!(Confidence::new(-0.0), Some(Confidence::ZERO));
/// assert!(Confidence::CERTAIN > Confidence::ZERO);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Confidence(f32);

impl Confidence {
    /// No confidence at all — the lowest value this type can hold.
    ///
    /// A candidate carrying it is still a candidate: "the detector reports
    /// zero confidence" and "the detector reports nothing" are different
    /// facts, and the second one is an empty [`LanguageDetection`].
    pub const ZERO: Self = Self(0.0);

    /// Full confidence. No detector in this crate ever reports it — a
    /// statistical model that claims certainty is claiming more than it
    /// measured — but it is the natural upper bound for a caller's own
    /// detector that resolves some inputs by construction rather than by
    /// scoring.
    pub const CERTAIN: Self = Self(1.0);

    /// A confidence, or [`None`] if `value` is `NaN` or outside
    /// `0.0..=1.0`.
    ///
    /// `-0.0` is accepted and normalised to `0.0`, so [`Confidence::ZERO`]
    /// has exactly one bit pattern and equality means what it looks like.
    #[must_use]
    pub fn new(value: f32) -> Option<Self> {
        // `contains` is `start <= value && value <= end`, both of which a
        // `NaN` fails — so this rejects `NaN` without a separate branch.
        if (0.0..=1.0).contains(&value) {
            Some(Self(value + 0.0))
        } else {
            None
        }
    }

    /// The underlying `f32`, always in `0.0..=1.0` and never `NaN`.
    #[must_use]
    pub const fn get(self) -> f32 {
        self.0
    }

    /// Half this confidence — exact in binary floating point for every
    /// value this type can hold, and still in range, so it needs no
    /// re-validation.
    ///
    /// Exists because "the model answered, but its own reliability signal
    /// says don't lean on it" is a real state a detector needs to express
    /// without either hiding the candidate or inventing a new number; see
    /// [`WhatlangDetector`](crate::WhatlangDetector) for the one use in
    /// this crate.
    #[must_use]
    pub const fn halved(self) -> Self {
        Self(self.0 * 0.5)
    }
}

impl Eq for Confidence {}

impl Ord for Confidence {
    /// Total, because [`Confidence::new`] has already excluded the one
    /// value that makes `f32` ordering partial.
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.total_cmp(&other.0)
    }
}

impl PartialOrd for Confidence {
    /// Always `Some`: written in terms of [`Ord`] rather than derived, so
    /// the two orderings cannot disagree.
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Display for Confidence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

impl From<Confidence> for f32 {
    fn from(confidence: Confidence) -> Self {
        confidence.0
    }
}

/// One candidate language, with the confidence its detector assigned it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LanguageCandidate {
    /// The candidate language.
    pub language: Language,
    /// How sure the detector is — see [`Confidence`] for what that does and
    /// does not mean across detectors.
    pub confidence: Confidence,
}

impl LanguageCandidate {
    /// A candidate.
    #[must_use]
    pub const fn new(language: Language, confidence: Confidence) -> Self {
        Self {
            language,
            confidence,
        }
    }
}

/// A [`LanguageDetector`]'s answer: zero or more candidates, most confident
/// first.
///
/// # Empty is an answer
///
/// For input with no usable signal — too short, no supported language
/// matched, script-only content — an empty `LanguageDetection` is what a
/// caller gets, not a low-confidence guess dressed up as a real answer.
/// [`LanguageDetection::is_empty`] and [`LanguageDetection::best`] both
/// report it plainly.
///
/// # Choosing the right read
///
/// | You want | Call | It gives you |
/// |---|---|---|
/// | the answer, if the detector is sure enough for *your* purpose | [`best_above`](Self::best_above) | `None` unless the top candidate clears your threshold. **The default.** |
/// | the top candidate whatever its confidence | [`best`](Self::best) | `None` only when there are no candidates at all; you are on your own for how much to trust it |
/// | to rank, display or re-score every candidate yourself | [`candidates`](Self::candidates) | the whole list, most confident first |
/// | to know whether the detector answered at all | [`is_empty`](Self::is_empty) | `true` when it abstained |
///
/// There is no built-in default threshold anywhere in this crate. What
/// counts as "confident enough" depends on your own tolerance for a wrong
/// guess, and on which detector produced the number (see [`Confidence`]);
/// this crate does not assume it knows either.
///
/// # The ordering is an invariant, not a convention
///
/// `candidates` is private and every constructor establishes descending
/// confidence, so [`best`](Self::best) is the first element *and* the
/// maximum — those cannot come apart. [`ranked`](Self::ranked) sorts what
/// it is given; that is its stated job, not a silent rewrite.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LanguageDetection {
    /// Descending by confidence; ties keep insertion order.
    candidates: Vec<LanguageCandidate>,
}

impl LanguageDetection {
    /// No candidates at all — the honest answer for input with no usable
    /// signal. Allocates nothing.
    #[must_use]
    pub const fn none() -> Self {
        Self {
            candidates: Vec::new(),
        }
    }

    /// Exactly one candidate — what every detector in this crate produces,
    /// since none of them ranks runners-up.
    #[must_use]
    pub fn single(language: Language, confidence: Confidence) -> Self {
        Self {
            candidates: vec![LanguageCandidate::new(language, confidence)],
        }
    }

    /// Several candidates, sorted into this type's descending-confidence
    /// order.
    ///
    /// The sort is stable, so candidates of equal confidence come out in
    /// the order they went in — a detector that has its own reason to
    /// prefer one of two equally-scored languages keeps it.
    #[must_use]
    pub fn ranked(candidates: impl IntoIterator<Item = LanguageCandidate>) -> Self {
        let mut candidates: Vec<LanguageCandidate> = candidates.into_iter().collect();
        // Total, no `partial_cmp`: `Confidence` is `Ord`. `sort_by_key` is
        // stable, which is what keeps equal-confidence candidates in the
        // order the detector produced them.
        candidates.sort_by_key(|candidate| Reverse(candidate.confidence));
        Self { candidates }
    }

    /// Every candidate, most confident first.
    #[must_use]
    pub fn candidates(&self) -> &[LanguageCandidate] {
        &self.candidates
    }

    /// Whether the detector abstained.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.candidates.is_empty()
    }

    /// How many candidates there are.
    #[must_use]
    pub fn len(&self) -> usize {
        self.candidates.len()
    }

    /// The most confident candidate, regardless of how low its confidence
    /// is. Prefer [`best_above`](Self::best_above) unless you have your own
    /// reason to trust an arbitrarily uncertain guess.
    #[must_use]
    pub fn best(&self) -> Option<&LanguageCandidate> {
        self.candidates.first()
    }

    /// The most confident candidate, but only if it clears `threshold`.
    ///
    /// The comparison is `>=`, so a threshold a candidate meets exactly
    /// passes.
    #[must_use]
    pub fn best_above(&self, threshold: Confidence) -> Option<&LanguageCandidate> {
        self.best().filter(|c| c.confidence >= threshold)
    }
}

impl<'a> IntoIterator for &'a LanguageDetection {
    type Item = &'a LanguageCandidate;
    type IntoIter = std::slice::Iter<'a, LanguageCandidate>;

    fn into_iter(self) -> Self::IntoIter {
        self.candidates.iter()
    }
}

/// Something that can guess which language a string is written in.
///
/// # The contract every implementor owes
///
/// * **Total.** `detect` never panics and never fails, for any `&str` —
///   empty, one scalar, whitespace, astral scalars, unpaired-looking
///   sequences, megabytes of it. There is no error type because there is no
///   error: input a detector cannot judge is an abstention, not a failure.
/// * **Abstention is a real answer.** Return an empty
///   [`LanguageDetection`] rather than the least-bad guess when there is no
///   usable signal. A caller can act on "I don't know"; it cannot act on a
///   guess that looks like knowledge.
/// * **Deterministic.** The same input must produce the same
///   [`LanguageDetection`] on every call, every thread and every platform.
///   No hashing of iteration order into the result, no time, no randomness.
/// * **Pure.** `detect` takes `&self` and must not mutate observable state,
///   which is what lets [`par_detect_batch`](crate::par_detect_batch) share
///   one detector across threads.
/// * **Confidence in the detector's own terms.** Whatever scale you report
///   on, document it, and keep it monotone in your own certainty (see
///   [`Confidence`]).
///
/// # Which one to use
///
/// Implemented in this crate by
/// [`WhatlangDetector`](crate::WhatlangDetector) (the default, behind
/// `language-detection`), `HashedLinearDetector` (behind
/// `fast-language-detection`) and [`FallbackDetector`](crate::FallbackDetector)
/// (pure composition, no feature). The crate-level documentation carries
/// the measured accuracy and speed table that settles which to reach for.
///
/// The rest of the crate — [`recommend`](crate::recommend),
/// [`detect_script`](crate::detect_script) — needs no implementor of this
/// trait at all. Only *automatic* detection does.
pub trait LanguageDetector {
    /// Guesses `input`'s language, honouring the contract above.
    fn detect(&self, input: &str) -> LanguageDetection;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Shorthand for a confidence known to be in range.
    fn c(value: f32) -> Confidence {
        Confidence::new(value).expect("test value is in 0.0..=1.0")
    }

    #[test]
    fn confidence_rejects_nan_and_out_of_range() {
        assert_eq!(Confidence::new(f32::NAN), None);
        assert_eq!(Confidence::new(f32::INFINITY), None);
        assert_eq!(Confidence::new(f32::NEG_INFINITY), None);
        assert_eq!(Confidence::new(-0.001), None);
        assert_eq!(Confidence::new(1.001), None);
        assert_eq!(Confidence::new(0.0).map(Confidence::get), Some(0.0));
        assert_eq!(Confidence::new(1.0).map(Confidence::get), Some(1.0));
    }

    #[test]
    fn negative_zero_normalises_so_zero_has_one_bit_pattern() {
        let zero = Confidence::new(-0.0).expect("-0.0 is in range");
        assert_eq!(zero, Confidence::ZERO);
        assert_eq!(zero.get().to_bits(), 0.0f32.to_bits());
        // ... and therefore sorts and compares identically.
        assert_eq!(zero.cmp(&Confidence::ZERO), Ordering::Equal);
    }

    #[test]
    fn confidence_ordering_is_total_and_agrees_with_the_float() {
        let values = [0.0f32, 0.25, 0.5, 0.75, 1.0];
        for &a in &values {
            for &b in &values {
                assert_eq!(
                    c(a).cmp(&c(b)),
                    a.partial_cmp(&b).expect("no NaN in this table"),
                    "{a} vs {b}"
                );
            }
        }
    }

    #[test]
    fn halving_stays_in_range_and_is_exact() {
        for &value in &[0.0f32, 0.1, 0.5, 0.7, 1.0] {
            let half = c(value).halved();
            assert_eq!(half.get(), value / 2.0);
            assert!(Confidence::new(half.get()).is_some(), "{value} left range");
        }
    }

    #[test]
    fn empty_detection_has_no_best() {
        let d = LanguageDetection::none();
        assert!(d.is_empty());
        assert_eq!(d.len(), 0);
        assert_eq!(d.best(), None);
        assert_eq!(d.best_above(Confidence::ZERO), None);
        assert!(d.candidates().is_empty());
    }

    #[test]
    fn best_above_respects_the_threshold_and_includes_equality() {
        let d = LanguageDetection::single(Language::English, c(0.4));
        assert_eq!(
            d.best_above(c(0.3)).map(|x| x.language),
            Some(Language::English)
        );
        assert_eq!(
            d.best_above(c(0.4)).map(|x| x.language),
            Some(Language::English),
            "the comparison is >=, so an exact match passes"
        );
        assert_eq!(d.best_above(c(0.5)), None);
    }

    #[test]
    fn best_ignores_threshold() {
        let d = LanguageDetection::single(Language::German, c(0.01));
        assert_eq!(d.best().map(|x| x.language), Some(Language::German));
    }

    #[test]
    fn ranked_sorts_descending_so_best_is_always_the_maximum() {
        let d = LanguageDetection::ranked([
            LanguageCandidate::new(Language::Polish, c(0.2)),
            LanguageCandidate::new(Language::English, c(0.9)),
            LanguageCandidate::new(Language::German, c(0.5)),
        ]);
        assert_eq!(
            d.candidates()
                .iter()
                .map(|x| x.language)
                .collect::<Vec<_>>(),
            vec![Language::English, Language::German, Language::Polish]
        );
        // The invariant the private field buys: first == maximum, always.
        assert_eq!(d.best(), d.candidates().iter().max_by_key(|x| x.confidence));
    }

    #[test]
    fn ranked_is_stable_on_equal_confidence() {
        let d = LanguageDetection::ranked([
            LanguageCandidate::new(Language::Spanish, c(0.5)),
            LanguageCandidate::new(Language::Catalan, c(0.5)),
            LanguageCandidate::new(Language::Galician, c(0.5)),
        ]);
        assert_eq!(
            d.candidates()
                .iter()
                .map(|x| x.language)
                .collect::<Vec<_>>(),
            vec![Language::Spanish, Language::Catalan, Language::Galician]
        );
    }

    #[test]
    fn ranked_of_nothing_is_none() {
        assert_eq!(LanguageDetection::ranked([]), LanguageDetection::none());
    }

    #[test]
    fn iterating_a_detection_yields_its_candidates_in_order() {
        let d = LanguageDetection::ranked([
            LanguageCandidate::new(Language::Polish, c(0.2)),
            LanguageCandidate::new(Language::English, c(0.9)),
        ]);
        assert_eq!(
            (&d).into_iter().map(|x| x.language).collect::<Vec<_>>(),
            vec![Language::English, Language::Polish]
        );
    }
}
