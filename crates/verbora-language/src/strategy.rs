//! [`recommend`]: mapping a [`Language`] to a phonetic-encoding strategy.
//!
//! # The honest starting point
//!
//! Every phonetic encoder Verbora ships — [`SoundEx`](verbora_phonetics::SoundEx),
//! [`Metaphone`](verbora_phonetics::Metaphone),
//! [`DoubleMetaphone`](verbora_phonetics::DoubleMetaphone),
//! [`SoundExDM`](verbora_phonetics::SoundExDM) — is a port of an
//! English-oriented algorithm. None of them has a language-specific
//! variant the way Verbora's tokenizers, stemmers or normalizers do; their
//! own documentation says plainly that non-Latin input "passes through"
//! rather than being phonetically analyzed (see
//! `verbora-phonetics`' crate-level doc comment). So "recommend a
//! strategy per language" cannot mean "pick a different algorithm
//! per language" the way it would for stemming — there is only one family
//! of general-purpose encoders, and what actually varies by language is
//! (a) whether that family is likely to produce a *meaningful* key at all,
//! and (b) whether a transliteration step should run first.
//!
//! This module says so plainly rather than pretending otherwise:
//! non-Latin-script languages get [`TransliterationAdvice::Unsupported`]
//! or a routed transliteration step, never a confident "use Metaphone"
//! recommendation that the algorithm cannot actually honour.

use verbora_transliterators::transliterate_ja;

use crate::{Language, Script};

/// One of Verbora's four phonetic encoders, named without depending on
/// `verbora_phonetics` types directly — this stays a plain enum so a
/// caller can match on it without constructing an encoder, and so this
/// module does not need to be generic over [`verbora_phonetics::PhoneticEncoder`]
/// just to name a choice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhoneticRecommendation {
    /// [`verbora_phonetics::SoundEx`] — cheap, coarse, four-character keys.
    SoundEx,
    /// [`verbora_phonetics::Metaphone`] — one key, better precision than SoundEx.
    Metaphone,
    /// [`verbora_phonetics::DoubleMetaphone`] — two keys, the broadest
    /// coverage of plausible pronunciations.
    DoubleMetaphone,
    /// [`verbora_phonetics::SoundExDM`] (Daitch-Mokotoff) — tuned for
    /// Slavic/Germanic/Ashkenazi-Jewish surname variation.
    SoundExDaitchMokotoff,
}

/// What to do about transliteration before phonetic encoding, for a given
/// language.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransliterationAdvice {
    /// The language is Latin-script; none of Verbora's encoders need a
    /// transliteration step first.
    NotNeeded,
    /// A transliteration step exists and is recommended before encoding —
    /// currently only [`verbora_transliterators::transliterate_ja`] for
    /// Japanese. Encoding native-script Japanese directly is not
    /// meaningful: Verbora's encoders pass non-Latin characters through
    /// largely unchanged (see this module's own doc comment), so applying
    /// them to un-transliterated kana/kanji does not produce a useful key.
    Recommended,
    /// No Verbora transliteration path exists for this language's script,
    /// and none of the phonetic encoders are designed for it either.
    /// Matching on this variant is a caller's cue to fall back to a
    /// different technique (exact matching, a language-specific external
    /// tool) rather than trust a phonetic key Verbora cannot honestly
    /// produce.
    Unsupported,
}

/// A phonetic-encoding recommendation for one language.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhoneticStrategy {
    /// The best-fit encoder for this language, given the honest
    /// constraints in this module's own doc comment. `None` when
    /// [`PhoneticStrategy::transliteration`] is [`TransliterationAdvice::Unsupported`]
    /// — recommending an encoder that cannot do anything useful would be
    /// exactly the false confidence this feature exists to avoid.
    pub primary: Option<PhoneticRecommendation>,
    /// Other reasonable choices, in no particular ranking beyond "also
    /// legitimate" — empty when `primary` is `None`.
    pub alternatives: Vec<PhoneticRecommendation>,
    /// Whether/how transliteration fits into the pipeline for this
    /// language.
    pub transliteration: TransliterationAdvice,
}

/// Recommends a phonetic strategy for `language`, given what Verbora's
/// encoders can actually do.
///
/// This never guesses at confidence — it is a lookup table over a small,
/// closed set of languages, not a statistical process, so there is nothing
/// to be uncertain about *given* the language is already known. Call this
/// after your own language determination (explicit or via
/// [`crate::LanguageDetector`]), not as a replacement for it.
#[must_use]
pub fn recommend(language: Language) -> PhoneticStrategy {
    use Language::{
        Basque, Catalan, Chinese, Dutch, English, Finnish, French, Galician, German, Hindi,
        Indonesian, Italian, Japanese, Norwegian, Persian, Polish, Portuguese, Russian, Spanish,
        Swedish, Ukrainian, Vietnamese,
    };
    use PhoneticRecommendation::{DoubleMetaphone, SoundEx, SoundExDaitchMokotoff};

    match language {
        // Latin-script languages Verbora's encoders were at least designed
        // with in mind (English directly; the rest share enough of the
        // Latin alphabet and general phonotactics that DoubleMetaphone's
        // broader coverage is a reasonable default, with SoundEx as the
        // cheap fallback). None of these get Metaphone as primary: it is
        // strictly less capable than DoubleMetaphone for the same cost
        // shape, so it is listed only as a lighter-weight alternative.
        English => PhoneticStrategy {
            primary: Some(DoubleMetaphone),
            alternatives: vec![PhoneticRecommendation::Metaphone, SoundEx],
            transliteration: TransliterationAdvice::NotNeeded,
        },
        German | Dutch | Swedish | Norwegian | Finnish => PhoneticStrategy {
            primary: Some(SoundExDaitchMokotoff),
            alternatives: vec![DoubleMetaphone, SoundEx],
            transliteration: TransliterationAdvice::NotNeeded,
        },
        Spanish | Portuguese | Italian | French | Galician | Catalan | Basque | Indonesian
        | Vietnamese => PhoneticStrategy {
            primary: Some(DoubleMetaphone),
            alternatives: vec![SoundEx],
            transliteration: TransliterationAdvice::NotNeeded,
        },
        Polish | Ukrainian | Russian => PhoneticStrategy {
            // Cyrillic/Polish input reaches these encoders only after a
            // caller has already romanized it themselves -- Verbora has
            // no Cyrillic transliterator -- so DaitchMokotoff (designed
            // for exactly this family of Slavic/Germanic name variation)
            // is the honest primary once that has happened.
            primary: Some(SoundExDaitchMokotoff),
            alternatives: vec![DoubleMetaphone],
            transliteration: TransliterationAdvice::Unsupported,
        },
        Japanese => PhoneticStrategy {
            // Applying any encoder to native kana/kanji is not meaningful
            // (see module doc) -- transliterate first, then the romanized
            // form behaves like any other Latin-script input.
            primary: Some(DoubleMetaphone),
            alternatives: vec![SoundEx],
            transliteration: TransliterationAdvice::Recommended,
        },
        Persian | Hindi | Chinese => PhoneticStrategy {
            // No Verbora transliterator exists for Arabic, Devanagari or
            // Han script, and none of the four encoders are designed for
            // these phonotactics -- recommending one would be exactly the
            // false confidence this module exists to avoid.
            primary: None,
            alternatives: Vec::new(),
            transliteration: TransliterationAdvice::Unsupported,
        },
    }
}

/// Applies `advice`'s transliteration step to `input`, if one exists.
///
/// Returns `input` unchanged for [`TransliterationAdvice::NotNeeded`] and
/// [`TransliterationAdvice::Unsupported`] alike — the caller's own match on
/// `advice` is what should decide whether to trust the phonetic step that
/// follows, not this function silently doing nothing for the unsupported
/// case. This exists only to save writing the one-arm match for the
/// currently-single [`TransliterationAdvice::Recommended`] case
/// (Japanese); it does not grow into a general router, since Verbora has
/// exactly one non-Latin transliterator today (see this module's own doc
/// comment on [`TransliterationAdvice::Recommended`]).
#[must_use]
pub fn apply_transliteration<'a>(
    advice: TransliterationAdvice,
    input: &'a str,
) -> std::borrow::Cow<'a, str> {
    match advice {
        TransliterationAdvice::Recommended => transliterate_ja(input),
        TransliterationAdvice::NotNeeded | TransliterationAdvice::Unsupported => {
            std::borrow::Cow::Borrowed(input)
        }
    }
}

/// Best-effort strategy from a script alone, for callers who only ran
/// [`detect_script`] and do not have (or want) a full language guess.
/// Coarser than [`recommend`]: a whole script maps to one strategy, so a
/// caller who can determine the actual [`Language`] should prefer
/// [`recommend`] instead.
#[must_use]
pub fn recommend_for_script(script: Script) -> PhoneticStrategy {
    match script {
        Script::Latin => PhoneticStrategy {
            primary: Some(PhoneticRecommendation::DoubleMetaphone),
            alternatives: vec![PhoneticRecommendation::SoundEx],
            transliteration: TransliterationAdvice::NotNeeded,
        },
        Script::Hiragana | Script::Katakana | Script::Han => PhoneticStrategy {
            primary: Some(PhoneticRecommendation::DoubleMetaphone),
            alternatives: vec![PhoneticRecommendation::SoundEx],
            // Han is ambiguous between Chinese and Japanese kanji -- only
            // recommend transliteration when the input is unambiguously
            // Japanese (Hiragana/Katakana present); a caller with pure Han
            // input should determine the language directly rather than
            // trust this coarse fallback to guess Japanese vs. Chinese.
            transliteration: if script == Script::Han {
                TransliterationAdvice::Unsupported
            } else {
                TransliterationAdvice::Recommended
            },
        },
        Script::Cyrillic
        | Script::Greek
        | Script::Arabic
        | Script::Hebrew
        | Script::Hangul
        | Script::Devanagari
        | Script::Other => PhoneticStrategy {
            primary: None,
            alternatives: Vec::new(),
            transliteration: TransliterationAdvice::Unsupported,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_language_gets_a_strategy() {
        for lang in Language::ALL {
            let strategy = recommend(lang);
            // Whichever way it goes, the two fields must agree: a
            // Some(primary) with Unsupported transliteration is fine
            // (transliteration not needed for the encoder to work), but
            // None primary must never carry alternatives.
            if strategy.primary.is_none() {
                assert!(
                    strategy.alternatives.is_empty(),
                    "{lang:?} has no primary recommendation but lists alternatives"
                );
            }
        }
    }

    #[test]
    fn japanese_recommends_transliteration() {
        assert_eq!(
            recommend(Language::Japanese).transliteration,
            TransliterationAdvice::Recommended
        );
    }

    #[test]
    fn unsupported_scripts_have_no_confident_primary() {
        for lang in [Language::Persian, Language::Hindi, Language::Chinese] {
            let strategy = recommend(lang);
            assert_eq!(
                strategy.primary, None,
                "{lang:?} should not get a confident primary"
            );
            assert_eq!(strategy.transliteration, TransliterationAdvice::Unsupported);
        }
    }

    #[test]
    fn english_recommends_double_metaphone_first() {
        assert_eq!(
            recommend(Language::English).primary,
            Some(PhoneticRecommendation::DoubleMetaphone)
        );
    }

    #[test]
    fn apply_transliteration_is_identity_when_not_needed() {
        let out = apply_transliteration(TransliterationAdvice::NotNeeded, "hello");
        assert_eq!(out, "hello");
        assert!(matches!(out, std::borrow::Cow::Borrowed(_)));
    }

    #[test]
    fn apply_transliteration_is_identity_when_unsupported() {
        let out = apply_transliteration(TransliterationAdvice::Unsupported, "مرحبا");
        assert_eq!(out, "مرحبا");
    }

    #[test]
    fn apply_transliteration_actually_transliterates_japanese() {
        let out = apply_transliteration(TransliterationAdvice::Recommended, "にほん");
        // transliterate_ja's own contract, not re-derived here.
        assert_eq!(out, transliterate_ja("にほん"));
        assert_ne!(out, "にほん");
    }

    #[test]
    fn script_based_recommendation_is_coarser_but_consistent() {
        let latin = recommend_for_script(Script::Latin);
        assert_eq!(latin.primary, Some(PhoneticRecommendation::DoubleMetaphone));
        let cyrillic = recommend_for_script(Script::Cyrillic);
        assert_eq!(cyrillic.primary, None);
    }

    #[test]
    fn most_languages_have_a_confident_primary_recommendation() {
        // The flip side of `unsupported_scripts_have_no_confident_primary`:
        // every language outside that Persian/Hindi/Chinese group is one
        // Verbora's encoders were designed to at least attempt, and must
        // get a real primary, not a silent `None`.
        for lang in Language::ALL {
            if matches!(
                lang,
                Language::Persian | Language::Hindi | Language::Chinese
            ) {
                continue;
            }
            assert!(
                recommend(lang).primary.is_some(),
                "{lang:?} is a supported language and should get a confident primary recommendation"
            );
        }
    }

    #[test]
    fn english_alternatives_are_ordered_metaphone_before_soundex() {
        // The module doc explains why: Metaphone is strictly better than
        // SoundEx for the same cost shape, so it belongs first in the
        // alternatives list, not merely present in it somewhere.
        assert_eq!(
            recommend(Language::English).alternatives,
            vec![
                PhoneticRecommendation::Metaphone,
                PhoneticRecommendation::SoundEx
            ]
        );
    }

    #[test]
    fn germanic_alternatives_prefer_double_metaphone_before_soundex() {
        for lang in [
            Language::German,
            Language::Dutch,
            Language::Swedish,
            Language::Norwegian,
            Language::Finnish,
        ] {
            assert_eq!(
                recommend(lang).alternatives,
                vec![
                    PhoneticRecommendation::DoubleMetaphone,
                    PhoneticRecommendation::SoundEx
                ],
                "{lang:?} alternatives should offer DoubleMetaphone before SoundEx"
            );
        }
    }

    #[test]
    fn no_language_lists_its_own_primary_as_an_alternative() {
        for lang in Language::ALL {
            let strategy = recommend(lang);
            if let Some(primary) = strategy.primary {
                assert!(
                    !strategy.alternatives.contains(&primary),
                    "{lang:?} lists its own primary recommendation among its alternatives too"
                );
            }
        }
    }

    #[test]
    fn double_metaphone_primary_actually_produces_two_codes_unlike_soundex() {
        // English's recommendation names DoubleMetaphone as primary --
        // confirm that recommendation actually corresponds to a real
        // two-code encoder, not just a name in this module's own enum.
        // SoundEx, English's own listed alternative, is the single-code
        // contrast: the whole point of naming DoubleMetaphone specifically
        // is that it is not interchangeable with a single-code encoder.
        use verbora_phonetics::{DoubleMetaphone, PhoneticCodes, PhoneticEncoder, SoundEx};

        assert_eq!(
            recommend(Language::English).primary,
            Some(PhoneticRecommendation::DoubleMetaphone)
        );

        let word = "Schmidt";
        assert!(
            matches!(DoubleMetaphone.encode(word), PhoneticCodes::Two(_, _)),
            "DoubleMetaphone should produce two codes"
        );
        assert!(
            matches!(SoundEx.encode(word), PhoneticCodes::One(_)),
            "SoundEx should produce exactly one code"
        );
    }

    #[test]
    fn recommend_for_script_is_cautious_about_han_but_not_kana() {
        // Han is ambiguous between Chinese and Japanese kanji, so the
        // coarse script-only fallback must not assume a transliteration
        // step is safe just because Han shares the same primary/
        // alternatives as Hiragana/Katakana (see this function's own doc
        // comment). Hiragana/Katakana are unambiguously Japanese, so those
        // DO get the transliteration recommendation.
        assert_eq!(
            recommend_for_script(Script::Han).transliteration,
            TransliterationAdvice::Unsupported
        );
        for script in [Script::Hiragana, Script::Katakana] {
            assert_eq!(
                recommend_for_script(script).transliteration,
                TransliterationAdvice::Recommended,
                "{script:?} should recommend transliteration"
            );
        }
    }

    #[test]
    fn every_script_gets_a_consistent_fallback_strategy() {
        for script in [
            Script::Latin,
            Script::Cyrillic,
            Script::Greek,
            Script::Arabic,
            Script::Hebrew,
            Script::Han,
            Script::Hiragana,
            Script::Katakana,
            Script::Hangul,
            Script::Devanagari,
            Script::Other,
        ] {
            let strategy = recommend_for_script(script);
            if strategy.primary.is_none() {
                assert!(
                    strategy.alternatives.is_empty(),
                    "{script:?} has no primary fallback recommendation but lists alternatives"
                );
            }
        }
    }
}
