//! Rayon-backed batch detection, behind the `parallel` feature.
//!
//! User-facing prose lives on [`par_detect_batch`], not in this private
//! module's `//!` block.

use rayon::prelude::*;

use crate::{LanguageDetection, LanguageDetector};

/// Detects every text in `texts` in parallel, one
/// [`LanguageDetector::detect`] call per text, **preserving input order**:
/// `out[i]` is the detection for `texts[i]`, exactly what
/// `texts.iter().map(|t| detector.detect(t.as_ref())).collect()` would
/// produce.
///
/// # Choosing the right API
///
/// | You have | Call | Why |
/// |---|---|---|
/// | one text | [`LanguageDetector::detect`] | **the default.** Detecting one word's language does not benefit from a thread pool, and this crate will never make it use one behind your back |
/// | a handful of texts, or very short ones | `texts.iter().map(...)` | Rayon's fork-join overhead dominates |
/// | a large corpus of realistic-length texts | `par_detect_batch` | one guess per document, thousands of times over — the only shape where the fan-out pays |
///
/// The `parallel` feature is not in `default`, and turning it on changes
/// nothing about [`LanguageDetector::detect`]: only this function ever
/// touches Rayon. See `benches/language.rs`'s `par_batch` group for the
/// crossover this project measured.
///
/// # Equivalence
///
/// This is a fan-out and nothing else — its body is
/// `par_iter().map(detect).collect()` over the same
/// [`LanguageDetector::detect`] the sequential path calls, so it cannot
/// drift out of sync with single-text behaviour.
/// `matches_the_sequential_loop_in_order` asserts that against the plain
/// loop.
///
/// # Thread safety
///
/// `detector` is borrowed, not consumed — one instance serves every
/// thread, which is why `D` must be [`Sync`]. Both detectors this crate
/// ships are zero-sized and stateless, so they satisfy it automatically; a
/// detector with real internal state must guarantee read-only, lock-free
/// access itself, as [`LanguageDetector`]'s own purity clause requires.
///
/// `texts` is any slice of things that borrow as `&str`, so `&[&str]`,
/// `&[String]` and `&[Cow<'_, str>]` all work without a conversion pass.
pub fn par_detect_batch<D, S>(detector: &D, texts: &[S]) -> Vec<LanguageDetection>
where
    D: LanguageDetector + Sync,
    S: AsRef<str> + Sync,
{
    texts
        .par_iter()
        .map(|text| detector.detect(text.as_ref()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Confidence, Language};

    struct FixedDetector;
    impl LanguageDetector for FixedDetector {
        fn detect(&self, input: &str) -> LanguageDetection {
            LanguageDetection::single(
                if input.is_empty() {
                    Language::German
                } else {
                    Language::English
                },
                Confidence::new(0.9).expect("0.9 is in range"),
            )
        }
    }

    #[test]
    fn matches_the_sequential_loop_in_order() {
        let detector = FixedDetector;
        let texts = ["hello", "", "world", "another"];
        let sequential: Vec<LanguageDetection> = texts.iter().map(|t| detector.detect(t)).collect();
        let parallel = par_detect_batch(&detector, &texts);
        assert_eq!(sequential, parallel);
    }

    #[test]
    fn empty_batch_produces_empty_output() {
        let detector = FixedDetector;
        assert!(par_detect_batch(&detector, &[] as &[&str]).is_empty());
    }

    #[test]
    fn accepts_owned_strings_without_a_conversion_pass() {
        let detector = FixedDetector;
        let texts: Vec<String> = vec!["hello".into(), String::new(), "world".into()];
        let sequential: Vec<LanguageDetection> = texts.iter().map(|t| detector.detect(t)).collect();
        assert_eq!(par_detect_batch(&detector, &texts), sequential);
    }

    #[test]
    fn a_large_batch_stays_in_order() {
        // Order preservation is the contract that makes the result usable
        // as a parallel array beside the input; a large batch is what
        // actually splits across threads.
        let detector = FixedDetector;
        let texts: Vec<String> = (0..1_000)
            .map(|i| {
                if i % 3 == 0 {
                    String::new()
                } else {
                    format!("text {i}")
                }
            })
            .collect();
        let sequential: Vec<LanguageDetection> = texts.iter().map(|t| detector.detect(t)).collect();
        assert_eq!(par_detect_batch(&detector, &texts), sequential);
    }
}
