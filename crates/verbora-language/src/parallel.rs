//! Rayon-backed batch detection, behind the `parallel` feature.
//!
//! # Why this exists, and why it is not the default
//!
//! [`LanguageDetector::detect`] is a single-text operation, and Rayon must
//! never become the *implementation* of that operation — detecting one
//! word's language does not benefit from a thread pool, and `Fase 5
//! Language.md`'s own spec says so explicitly ("No utilizar Rayon para
//! detectar el idioma de una palabra"). What does benefit is a caller
//! holding a large, independent corpus — one language guess per document,
//! per review, per record — where the per-text cost repeats thousands of
//! times. [`par_detect_batch`] is exactly that fan-out and nothing else: its
//! body is a `par_iter().map(...)` over the same [`LanguageDetector::detect`]
//! every sequential call uses, so it can never drift out of sync with
//! single-text detection behaviour.
//!
//! Generic over any `D: LanguageDetector + Sync`, not tied to
//! [`crate::WhatlangDetector`] — a caller's own detector benefits the same
//! way, as long as it can be called from multiple threads at once.
//!
//! # When to reach for it instead of the sequential loop
//!
//! See the `par_batch` Criterion group in `benches/language.rs` for the real
//! crossover this project measured. As a rule of thumb, shared with every
//! other `par_*_batch` API across this workspace (see AGENTS.md's Rayon
//! Policy): a handful of texts, or very short texts, is dominated by Rayon's
//! own fork-join overhead — use the sequential loop. A large batch of
//! realistic-length texts is where this wins.

use rayon::prelude::*;

use crate::{LanguageDetection, LanguageDetector};

/// Detects every text in `texts` in parallel, one [`LanguageDetector::detect`]
/// call per text, preserving input order: `out[i]` is the detection for
/// `texts[i]`, exactly as `texts.iter().map(|t| detector.detect(t)).collect()`
/// would produce.
///
/// `detector` is borrowed, not consumed — the same detector instance serves
/// every thread, which is why `D` must be [`Sync`]. [`crate::WhatlangDetector`]
/// is zero-sized and stateless, so it satisfies this trivially; a detector
/// with real internal state needs to guarantee read-only, lock-free access
/// itself (see AGENTS.md's Thread Safety guidance).
pub fn par_detect_batch<D>(detector: &D, texts: &[&str]) -> Vec<LanguageDetection>
where
    D: LanguageDetector + Sync,
{
    texts.par_iter().map(|text| detector.detect(text)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Language, LanguageCandidate};

    struct FixedDetector;
    impl LanguageDetector for FixedDetector {
        fn detect(&self, input: &str) -> LanguageDetection {
            LanguageDetection {
                candidates: vec![LanguageCandidate {
                    language: if input.is_empty() {
                        Language::German
                    } else {
                        Language::English
                    },
                    confidence: 0.9,
                }],
            }
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
        assert!(par_detect_batch(&detector, &[]).is_empty());
    }
}
