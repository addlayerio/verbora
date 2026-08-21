//! Every public entry point, over one pathological corpus.
//!
//! Each of the crate's own modules tests its own contract in detail. This
//! file tests the property they all share and none of them can prove
//! alone: **no public function in this crate fails or panics on any
//! `&str`.** There is no error type anywhere in the crate's surface, so a
//! panic would be the only way an input could be rejected — and text a
//! caller feeds an NLP library is arbitrary by definition.
//!
//! The corpus deliberately includes the shapes a byte- or code-unit-indexed
//! implementation gets wrong: astral scalars (four UTF-8 bytes), combining
//! marks separated from their base, a lone `U+FFFD`, mixed scripts inside
//! one word, and inputs whose character count and byte count differ by a
//! factor of four.

use verbora_language::{
    AutoPhoneticStrategy, Confidence, Language, LanguageDetection, LanguageDetector, Script,
    StrategyBasis, TransliterationAdvice, apply_transliteration, detect_script, recommend,
    recommend_for_script,
};

/// Inputs chosen so that byte length, scalar count and grapheme count all
/// disagree, and so that every branch of the script classifier is reached.
const PATHOLOGICAL: &[&str] = &[
    "",
    " ",
    "\t\n\r",
    "a",
    "é",
    "\u{0}",
    "\u{FFFD}",
    "\u{10FFFF}",
    "😀",
    "😀😀😀😀",
    "a😀b😀c",
    "e\u{301}",    // base + combining acute, unnormalized
    "\u{301}",     // a combining mark with no base at all
    "\u{3099}",    // Japanese voiced-sound mark, alone
    "か\u{3099}",  // kana + combining voiced-sound mark
    "×÷·",         // symbols inside script blocks
    "123 !@# ...", // no letters at all
    "ﬁ",           // a ligature
    "ǅ",           // titlecase letter
    "ẞ",           // capital sharp s
    "hello world",
    "café müller",
    "Приветствую вас",
    "こんにちは世界",
    "日本語テキスト",
    "中文文本",
    "हिन्दी में लिखा",
    "العربية نص",
    "עברית טקסט",
    "ελληνικά κείμενο",
    "한국어 문장",
    "Tiếng Việt có dấu",
    "ก ข ค ง",
    "Hello мир 世界 مرحبا",
    "aЖ",
    "Жa",
    "\u{2028}\u{00A0}\u{FEFF}", // line separator, NBSP, BOM
];

/// Long inputs, built rather than written out.
fn long_inputs() -> Vec<String> {
    vec![
        "a".repeat(10_000),
        "😀".repeat(2_000),
        "日本語".repeat(3_000),
        "aЖ".repeat(5_000),
        "\u{0}".repeat(1_000),
    ]
}

fn every_input() -> Vec<String> {
    PATHOLOGICAL
        .iter()
        .map(|s| (*s).to_owned())
        .chain(long_inputs())
        .collect()
}

#[test]
fn script_detection_is_total() {
    for input in every_input() {
        let detected = detect_script(&input);
        // Whatever comes back, it must agree with the per-scalar rule the
        // public API documents: `None` exactly when no scalar votes.
        let any_voter = input.chars().any(|c| Script::of(c).is_some());
        assert_eq!(
            detected.is_some(),
            any_voter,
            "detect_script({input:?}) disagrees with Script::of about whether anything voted"
        );
    }
}

#[test]
fn script_detection_is_idempotent_and_pure() {
    for input in every_input() {
        let first = detect_script(&input);
        for _ in 0..3 {
            assert_eq!(detect_script(&input), first, "{input:?}");
        }
    }
}

#[test]
fn script_recommendation_is_total_over_every_script_a_corpus_can_produce() {
    for input in every_input() {
        let Some(script) = detect_script(&input) else {
            continue;
        };
        let strategy = recommend_for_script(script);
        assert_eq!(
            strategy.basis == StrategyBasis::NoFit,
            strategy.primary.is_none(),
            "{input:?} -> {script:?}"
        );
        // A script alone never justifies a language-level claim.
        assert_ne!(strategy.basis, StrategyBasis::Named, "{script:?}");
    }
}

#[test]
fn transliteration_is_total_under_every_advice() {
    for input in every_input() {
        for advice in [
            TransliterationAdvice::NotNeeded,
            TransliterationAdvice::Recommended,
            TransliterationAdvice::Unsupported,
        ] {
            let out = apply_transliteration(advice, &input);
            if advice != TransliterationAdvice::Recommended {
                assert_eq!(out, input, "{advice:?} must not rewrite {input:?}");
            }
            // The output is valid UTF-8 by type, but it must also never be
            // longer than the transliterator's own contract allows to
            // invent — this only checks it is inspectable without panic.
            let _ = out.chars().count();
        }
    }
}

#[test]
fn recommendation_is_total_and_self_consistent_for_every_language() {
    for language in Language::ALL {
        let strategy = recommend(language);
        assert_eq!(
            strategy.basis == StrategyBasis::NoFit,
            strategy.primary.is_none(),
            "{language:?}"
        );
        assert_eq!(
            strategy.encoders().count(),
            usize::from(strategy.primary.is_some()) + strategy.alternatives.len(),
            "{language:?}"
        );
    }
}

/// A detector that answers with a fixed candidate, so the composed path can
/// be exercised without either optional feature.
struct Always;

impl LanguageDetector for Always {
    fn detect(&self, input: &str) -> LanguageDetection {
        if input.is_empty() {
            LanguageDetection::none()
        } else {
            LanguageDetection::single(Language::English, Confidence::CERTAIN)
        }
    }
}

#[test]
fn the_composed_path_is_total_without_any_optional_feature() {
    let auto = AutoPhoneticStrategy::new(Always, Confidence::ZERO);
    for input in every_input() {
        let result = auto.detect_and_recommend(&input);
        assert_eq!(
            result.strategy.is_some(),
            !result.detection.is_empty(),
            "{input:?}: at a zero threshold, a candidate must always yield a strategy"
        );
    }
}

#[cfg(feature = "language-detection")]
#[test]
fn the_default_detector_is_total() {
    use verbora_language::DefaultDetector;

    let detector = DefaultDetector::new();
    for input in every_input() {
        let detection = detector.detect(&input);
        for candidate in &detection {
            assert!(
                (0.0..=1.0).contains(&candidate.confidence.get()),
                "{input:?}"
            );
        }
        // Determinism, on the same pathological corpus.
        assert_eq!(detector.detect(&input), detection, "{input:?}");
    }
}

#[cfg(feature = "fast-language-detection")]
#[test]
fn the_fast_detector_is_total() {
    use verbora_language::HashedLinearDetector;

    let detector = HashedLinearDetector::new();
    for input in every_input() {
        let detection = detector.detect(&input);
        for candidate in &detection {
            assert!(
                (0.0..=1.0).contains(&candidate.confidence.get()),
                "{input:?}"
            );
        }
        assert_eq!(detector.detect(&input), detection, "{input:?}");
    }
}

#[cfg(all(feature = "fast-language-detection", feature = "language-detection"))]
#[test]
fn the_fallback_composition_is_total() {
    use verbora_language::{FallbackDetector, HashedLinearDetector, WhatlangDetector};

    let detector = FallbackDetector::new(HashedLinearDetector::new(), WhatlangDetector::new());
    for input in every_input() {
        let detection = detector.detect(&input);
        // The composition never invents: it returns one of its two halves'
        // answers verbatim.
        assert!(
            detection == HashedLinearDetector::new().detect(&input)
                || detection == WhatlangDetector::new().detect(&input),
            "{input:?}: the composition produced an answer neither half gave"
        );
    }
}

#[cfg(feature = "parallel")]
#[test]
fn the_parallel_batch_matches_the_sequential_loop_on_the_whole_corpus() {
    use verbora_language::par_detect_batch;

    let inputs = every_input();
    let sequential: Vec<LanguageDetection> = inputs.iter().map(|t| Always.detect(t)).collect();
    assert_eq!(par_detect_batch(&Always, &inputs), sequential);
}
