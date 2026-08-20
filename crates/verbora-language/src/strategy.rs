//! Mapping a [`Language`] or [`Script`] to a phonetic-encoding strategy.
//!
//! User-facing prose lives on [`recommend`], [`PhoneticStrategy`] and the
//! enums below, not in this private module's `//!` block.

use verbora_transliterators::transliterate_ja;

use crate::{Language, Script};

/// A Verbora phonetic encoder, named without constructing one.
///
/// This is a plain, `Copy` enum rather than a boxed
/// [`PhoneticEncoder`](verbora_phonetics::PhoneticEncoder) so a caller can
/// `match` on a recommendation, log it, or store it, without paying for an
/// encoder they may not use — and so this module needs no generic parameter
/// just to name a choice.
///
/// Every variant names an encoder whose own publication or rule corpus is
/// cited in `verbora-phonetics`; [`recommend`]'s table says which of them
/// applies where, and [`PhoneticStrategy::basis`] says how firmly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PhoneticRecommendation {
    /// [`verbora_phonetics::SoundEx`] — Russell's 1918 census code: cheap,
    /// coarse, one letter and three digits. English-oriented, and the
    /// lowest common denominator everywhere else.
    SoundEx,
    /// [`verbora_phonetics::Metaphone`] — Philips 1990. One key, English
    /// orthography, better precision than Soundex for the same cost shape.
    Metaphone,
    /// [`verbora_phonetics::DoubleMetaphone`] — Philips 2000. One or two
    /// keys, with the article's own branch conditions for Slavo-Germanic,
    /// Romance, Greek and Chinese *spellings* of names written in the Latin
    /// alphabet.
    DoubleMetaphone,
    /// [`verbora_phonetics::DaitchMokotoff`] — Daitch and Mokotoff 1985,
    /// written because plain Soundex garbles the Slavic and Yiddish
    /// spellings of Ashkenazi surnames. Branching: it returns every reading
    /// of an ambiguous cluster.
    DaitchMokotoff,
    /// [`verbora_phonetics::Cologne`] — Postel's 1969 Kölner Phonetik, the
    /// one encoder in the workspace designed for a language other than
    /// English, and that language is German.
    Cologne,
    /// [`verbora_phonetics::BeiderMorse`], restricted to one of its own
    /// languages' rule files.
    ///
    /// The only multilingual encoder in the workspace: its rule corpus
    /// carries a separate table per language, several of which read a
    /// **native script** directly (Cyrillic, Hebrew, Greek and Arabic) —
    /// which is why it, and not a Latin-alphabet encoder, is what
    /// [`recommend`] names for those scripts.
    BeiderMorse {
        /// A language tag from
        /// [`NameType::Generic`](verbora_phonetics::NameType::Generic)'s own
        /// list, ready to pass straight to
        /// [`BeiderMorse::encode_language`](verbora_phonetics::BeiderMorse::encode_language).
        ///
        /// Every tag this crate emits is checked against that list by
        /// `every_beider_morse_tag_this_crate_emits_is_a_real_one`, which
        /// walks every recommendation of every language and every script
        /// through the real encoder — a tag that does not resolve would
        /// otherwise silently degrade to no candidates at all.
        language: &'static str,
    },
}

/// How firmly a [`PhoneticStrategy`] is grounded — the difference between
/// "an encoder was designed for this language" and "an encoder will run on
/// it".
///
/// This distinction is the whole reason this crate exists: recommending
/// Double Metaphone for Finnish and recommending Cologne for German are not
/// the same kind of statement, and collapsing them into one
/// `Option<PhoneticRecommendation>` would hide exactly the uncertainty a
/// caller needs in order to decide whether to trust a phonetic key at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum StrategyBasis {
    /// The primary encoder's own publication or rule corpus **names this
    /// language**. The strongest claim this crate makes.
    Named,
    /// No encoder names this language. The recommendation follows from the
    /// script or alphabet it is written in: the encoder will read the text
    /// and produce a stable key, but it was not tuned for this language's
    /// phonology, and a key it produces means less than one from a
    /// [`Named`](Self::Named) strategy.
    Script,
    /// Nothing in Verbora fits. [`PhoneticStrategy::primary`] is `None` and
    /// [`PhoneticStrategy::alternatives`] is empty.
    ///
    /// Match on this and fall back to a different technique (exact
    /// matching, a language-specific external tool) rather than trust a
    /// phonetic key Verbora cannot honestly produce.
    NoFit,
}

/// Whether a transliteration step belongs in the pipeline before encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum TransliterationAdvice {
    /// The primary encoder reads this language's own script directly. Feed
    /// it the text as it stands.
    ///
    /// True for every Latin-alphabet language, and also for the scripts
    /// [`PhoneticRecommendation::BeiderMorse`] has native rule tables for.
    NotNeeded,
    /// Verbora has a transliterator for this language's script, and the
    /// primary encoder needs it: run
    /// [`apply_transliteration`] first.
    ///
    /// Currently only Japanese, via
    /// [`verbora_transliterators::transliterate_ja`]. Encoding kana or
    /// kanji with a Latin-alphabet encoder is not meaningful — those
    /// encoders read `A`–`Z` and skip everything else, so the key would be
    /// empty.
    Recommended,
    /// The primary encoder needs romanized input, and Verbora has no
    /// transliterator for this script — so romanizing is the **caller's**
    /// job, with a tool of their choosing, before anything here applies.
    ///
    /// Also the advice whenever [`PhoneticStrategy::basis`] is
    /// [`StrategyBasis::NoFit`]: there is nothing to transliterate *for*.
    Unsupported,
}

/// A phonetic-encoding strategy for one language or script.
///
/// `Copy` and allocation-free: [`alternatives`](Self::alternatives) is a
/// `&'static` slice into the table [`recommend`] is compiled from, not a
/// `Vec` built per call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PhoneticStrategy {
    /// The best-fit encoder, or `None` when nothing fits — see
    /// [`basis`](Self::basis), which is
    /// [`StrategyBasis::NoFit`] exactly when this is `None`.
    pub primary: Option<PhoneticRecommendation>,
    /// Other legitimate choices, most useful first. Empty when
    /// [`primary`](Self::primary) is `None`, and never contains
    /// [`primary`](Self::primary) itself.
    pub alternatives: &'static [PhoneticRecommendation],
    /// How firmly [`primary`](Self::primary) is grounded.
    pub basis: StrategyBasis,
    /// Whether a transliteration step belongs before encoding.
    pub transliteration: TransliterationAdvice,
}

impl PhoneticStrategy {
    /// The strategy for "nothing fits" — `None`, no alternatives,
    /// [`StrategyBasis::NoFit`], [`TransliterationAdvice::Unsupported`].
    const NO_FIT: Self = Self {
        primary: None,
        alternatives: &[],
        basis: StrategyBasis::NoFit,
        transliteration: TransliterationAdvice::Unsupported,
    };

    /// Every encoder this strategy names, primary first.
    ///
    /// Convenience for a caller that wants to try each in turn (build a
    /// key, look it up, fall through on a miss) without special-casing the
    /// primary. Empty for a [`StrategyBasis::NoFit`] strategy.
    pub fn encoders(&self) -> impl Iterator<Item = PhoneticRecommendation> + '_ {
        self.primary
            .into_iter()
            .chain(self.alternatives.iter().copied())
    }
}

/// The phonetic strategy for `language`.
///
/// # What this answers, and what it cannot
///
/// Verbora ships twelve phonetic encoders, and exactly one of them was
/// designed for a language other than English:
/// [`Cologne`](verbora_phonetics::Cologne), for German. Two more are
/// grounded in something wider than one language —
/// [`DaitchMokotoff`](verbora_phonetics::DaitchMokotoff) in Slavic and
/// Yiddish surname spellings, [`BeiderMorse`](verbora_phonetics::BeiderMorse)
/// in a per-language rule corpus covering eighteen. Everything else is an
/// English-oriented algorithm that reads `A`–`Z` and skips the rest.
///
/// So "recommend a strategy per language" cannot mean "a different
/// algorithm for each of twenty-two languages" the way it would for
/// stemming. What it means here is two facts a caller cannot get from the
/// encoders themselves:
///
/// 1. **Which encoder, if any, was actually designed for this language**
///    — reported by [`PhoneticStrategy::basis`], which distinguishes
///    "designed for it" ([`StrategyBasis::Named`]) from "will run on it"
///    ([`StrategyBasis::Script`]) from "don't" ([`StrategyBasis::NoFit`]).
/// 2. **Whether a transliteration step has to run first** — reported by
///    [`PhoneticStrategy::transliteration`].
///
/// This is a lookup table over a closed set, not a statistical process:
/// there is nothing to be uncertain about *given* the language. Call it
/// after your own language determination — explicit, or via a
/// [`LanguageDetector`](crate::LanguageDetector) — never as a replacement
/// for one.
///
/// # The table
///
/// | Language | Primary | Basis | Why |
/// |---|---|---|---|
/// | German | [`Cologne`](PhoneticRecommendation::Cologne) | [`Named`](StrategyBasis::Named) | Postel 1969 is German phonetics, by construction |
/// | English | [`DoubleMetaphone`](PhoneticRecommendation::DoubleMetaphone) | [`Named`](StrategyBasis::Named) | Philips 2000 is specified over English orthography |
/// | Polish | [`DaitchMokotoff`](PhoneticRecommendation::DaitchMokotoff) | [`Named`](StrategyBasis::Named) | Daitch–Mokotoff exists for Slavic surname spellings; Polish `RS`/`RZ` is one of its named branches |
/// | Dutch, French, Italian, Spanish, Portuguese | [`BeiderMorse`](PhoneticRecommendation::BeiderMorse) | [`Named`](StrategyBasis::Named) | each is one of Beider-Morse's own eighteen rule languages |
/// | Russian, Ukrainian | [`BeiderMorse`](PhoneticRecommendation::BeiderMorse) (`cyrillic`) | [`Script`](StrategyBasis::Script) | Beider-Morse's own `russian` table is written over *Latin transliterations* of Russian names and returns nothing for Cyrillic input; the table that reads Cyrillic is the script-level `cyrillic` one, and it is not Russian-specific |
/// | Norwegian, Swedish, Finnish, Galician, Catalan, Basque, Indonesian, Vietnamese | [`DoubleMetaphone`](PhoneticRecommendation::DoubleMetaphone) | [`Script`](StrategyBasis::Script) | no encoder names them; they are written in the Latin alphabet, which is all Double Metaphone requires |
/// | Japanese | [`DoubleMetaphone`](PhoneticRecommendation::DoubleMetaphone) | [`Script`](StrategyBasis::Script) | after [`transliterate_ja`](verbora_transliterators::transliterate_ja), romanized Japanese is Latin-alphabet text like any other |
/// | Persian | [`BeiderMorse`](PhoneticRecommendation::BeiderMorse) (`arabic`) | [`Script`](StrategyBasis::Script) | Beider-Morse's Arabic rules read the Arabic script Persian is written in; they were written for Arabic names, not Persian ones |
/// | Hindi, Chinese | — | [`NoFit`](StrategyBasis::NoFit) | no encoder reads Devanagari or Han, and no Verbora transliterator romanizes them |
///
/// The Hindi and Chinese rows are the point of the whole type. Naming an
/// encoder for them would produce a key — every encoder here is total, so
/// something always comes back — and that key would be worthless, because
/// no rule in any of them mentions a Devanagari or Han character. A
/// recommendation that cannot be honoured is exactly the false confidence
/// this module exists to avoid.
///
/// # Cost
///
/// A closed `match` over 22 arms returning `Copy` data. No allocation, no
/// I/O, no model, no feature flag.
///
/// ```
/// use verbora_language::{Language, StrategyBasis, recommend};
///
/// let de = recommend(Language::German);
/// assert_eq!(de.basis, StrategyBasis::Named);
///
/// let fi = recommend(Language::Finnish);
/// assert_eq!(fi.basis, StrategyBasis::Script); // it runs; it wasn't designed for it
///
/// let zh = recommend(Language::Chinese);
/// assert_eq!(zh.basis, StrategyBasis::NoFit);
/// assert!(zh.primary.is_none());
/// ```
#[must_use]
pub fn recommend(language: Language) -> PhoneticStrategy {
    use PhoneticRecommendation::{
        BeiderMorse, Cologne, DaitchMokotoff, DoubleMetaphone, Metaphone, SoundEx,
    };

    /// The Latin-alphabet fallback: no encoder names the language, but
    /// Double Metaphone needs nothing but `A`–`Z`.
    const LATIN_ALPHABET: PhoneticStrategy = PhoneticStrategy {
        primary: Some(DoubleMetaphone),
        alternatives: &[Metaphone, SoundEx],
        basis: StrategyBasis::Script,
        transliteration: TransliterationAdvice::NotNeeded,
    };

    /// One of Beider-Morse's own languages, plus the Latin-alphabet
    /// encoders as lighter alternatives.
    const fn beider_morse(tag: &'static str, basis: StrategyBasis) -> PhoneticStrategy {
        PhoneticStrategy {
            primary: Some(BeiderMorse { language: tag }),
            alternatives: &[DoubleMetaphone, SoundEx],
            basis,
            transliteration: TransliterationAdvice::NotNeeded,
        }
    }

    match language {
        // Philips specified Double Metaphone over English orthography, and
        // Metaphone/Soundex are the same lineage at lower resolution.
        // Beider-Morse's `english` table is a legitimate second opinion for
        // surnames specifically.
        Language::English => PhoneticStrategy {
            primary: Some(DoubleMetaphone),
            alternatives: &[
                Metaphone,
                SoundEx,
                BeiderMorse {
                    language: "english",
                },
            ],
            basis: StrategyBasis::Named,
            transliteration: TransliterationAdvice::NotNeeded,
        },
        // The one language in this list with an encoder built for it.
        Language::German => PhoneticStrategy {
            primary: Some(Cologne),
            alternatives: &[
                DaitchMokotoff,
                BeiderMorse { language: "german" },
                DoubleMetaphone,
            ],
            basis: StrategyBasis::Named,
            transliteration: TransliterationAdvice::NotNeeded,
        },
        // Daitch-Mokotoff was written for Slavic and Yiddish surname
        // spellings; its branch list names Polish RS/RZ explicitly.
        Language::Polish => PhoneticStrategy {
            primary: Some(DaitchMokotoff),
            alternatives: &[BeiderMorse { language: "polish" }, DoubleMetaphone],
            basis: StrategyBasis::Named,
            transliteration: TransliterationAdvice::NotNeeded,
        },
        Language::Dutch => beider_morse("dutch", StrategyBasis::Named),
        Language::French => beider_morse("french", StrategyBasis::Named),
        Language::Italian => beider_morse("italian", StrategyBasis::Named),
        Language::Spanish => beider_morse("spanish", StrategyBasis::Named),
        Language::Portuguese => beider_morse("portuguese", StrategyBasis::Named),
        // Beider-Morse *does* have a `russian` table, and it is written over
        // Latin transliterations of Russian names, not Cyrillic:
        // `encode_language("Иванов", "russian")` returns one empty
        // spelling. The table that reads the script Russian is actually
        // written in is the script-level `cyrillic` one -- so that is the
        // primary, and the basis is Script rather than Named.
        // `every_primary_produces_a_real_key_for_its_own_language` is what
        // holds this to a working recommendation rather than a plausible
        // one.
        Language::Russian | Language::Ukrainian => PhoneticStrategy {
            primary: Some(BeiderMorse {
                language: "cyrillic",
            }),
            alternatives: &[],
            basis: StrategyBasis::Script,
            transliteration: TransliterationAdvice::NotNeeded,
        },
        // Persian shares the Arabic script that Beider-Morse's `arabic`
        // table reads, and shares none of the language those rules were
        // written for. Script-level, and labelled as such.
        Language::Persian => PhoneticStrategy {
            primary: Some(BeiderMorse { language: "arabic" }),
            alternatives: &[],
            basis: StrategyBasis::Script,
            transliteration: TransliterationAdvice::NotNeeded,
        },
        // Latin-alphabet languages no encoder was written for. They get a
        // working encoder and an honest basis, not a pretend pedigree.
        Language::Norwegian
        | Language::Swedish
        | Language::Finnish
        | Language::Galician
        | Language::Catalan
        | Language::Basque
        | Language::Indonesian
        | Language::Vietnamese => LATIN_ALPHABET,
        // Romanize first, then it is Latin-alphabet text like any other.
        Language::Japanese => PhoneticStrategy {
            transliteration: TransliterationAdvice::Recommended,
            ..LATIN_ALPHABET
        },
        // Devanagari and Han: no encoder reads them, no transliterator
        // romanizes them.
        Language::Hindi | Language::Chinese => PhoneticStrategy::NO_FIT,
    }
}

/// The phonetic strategy for `script` alone, for a caller who ran
/// [`detect_script`](crate::detect_script) and has no language guess.
///
/// Coarser than [`recommend`] by construction — a whole script maps to one
/// strategy — so its answers carry [`StrategyBasis::Script`] at best, never
/// [`StrategyBasis::Named`]. A caller who can determine the actual
/// [`Language`] should prefer [`recommend`].
///
/// [`Script::Han`] is the one case worth reading twice: Han is ambiguous
/// between Chinese and Japanese kanji, so this cannot recommend the
/// Japanese transliteration step from the script alone. Kana
/// ([`Script::Hiragana`], [`Script::Katakana`]) is unambiguous and does get
/// it.
#[must_use]
pub fn recommend_for_script(script: Script) -> PhoneticStrategy {
    use PhoneticRecommendation::{BeiderMorse, DoubleMetaphone, Metaphone, SoundEx};

    const LATIN: PhoneticStrategy = PhoneticStrategy {
        primary: Some(DoubleMetaphone),
        alternatives: &[Metaphone, SoundEx],
        basis: StrategyBasis::Script,
        transliteration: TransliterationAdvice::NotNeeded,
    };

    /// Beider-Morse reading a native script directly — the only encoder in
    /// the workspace that does.
    const fn native(tag: &'static str) -> PhoneticStrategy {
        PhoneticStrategy {
            primary: Some(BeiderMorse { language: tag }),
            alternatives: &[],
            basis: StrategyBasis::Script,
            transliteration: TransliterationAdvice::NotNeeded,
        }
    }

    match script {
        Script::Latin => LATIN,
        Script::Cyrillic => native("cyrillic"),
        Script::Greek => native("greek"),
        Script::Hebrew => native("hebrew"),
        Script::Arabic => native("arabic"),
        // Kana is unambiguously Japanese: romanize, then treat as Latin.
        Script::Hiragana | Script::Katakana => PhoneticStrategy {
            transliteration: TransliterationAdvice::Recommended,
            ..LATIN
        },
        // Han could be Chinese or Japanese kanji. Guessing Japanese here
        // would be a language claim this function has no evidence for.
        Script::Han | Script::Hangul | Script::Devanagari | Script::Other => {
            PhoneticStrategy::NO_FIT
        }
    }
}

/// Applies `advice`'s transliteration step to `input`, if one exists.
///
/// Returns `input` **unchanged** — as [`Cow::Borrowed`](std::borrow::Cow),
/// with no allocation — for [`TransliterationAdvice::NotNeeded`] and
/// [`TransliterationAdvice::Unsupported`] alike. That is deliberate and is
/// not a silent success: `Unsupported` means Verbora has no romanization to
/// offer, so there is nothing this function could honestly do, and the
/// caller's own match on `advice` is what should decide whether to trust
/// the phonetic step that follows.
///
/// This exists only to save writing the one-arm match for the currently
/// single [`TransliterationAdvice::Recommended`] case; it will not grow
/// into a general router while Verbora has exactly one non-Latin
/// transliterator.
///
/// ```
/// use verbora_language::{TransliterationAdvice, apply_transliteration};
///
/// let advice = TransliterationAdvice::Recommended;
/// assert_eq!(apply_transliteration(advice, "にほん"), "nihon");
///
/// // Unsupported does nothing at all, and says so by borrowing.
/// let advice = TransliterationAdvice::Unsupported;
/// assert!(matches!(
///     apply_transliteration(advice, "मुझे"),
///     std::borrow::Cow::Borrowed("मुझे")
/// ));
/// ```
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Every [`Script`] variant, so the script-level sweeps below are
    /// enumerations rather than samples. `Script` is `#[non_exhaustive]`,
    /// so a new variant is a deliberate edit here too.
    const ALL_SCRIPTS: [Script; 11] = [
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
    ];

    /// Every strategy this crate can ever return, from both entry points.
    fn every_strategy() -> Vec<(String, PhoneticStrategy)> {
        Language::ALL
            .into_iter()
            .map(|l| (format!("recommend({l:?})"), recommend(l)))
            .chain(ALL_SCRIPTS.into_iter().map(|s| {
                (
                    format!("recommend_for_script({s:?})"),
                    recommend_for_script(s),
                )
            }))
            .collect()
    }

    #[test]
    fn no_fit_and_no_primary_are_the_same_state() {
        // The invariant that keeps `basis` from drifting into a fourth,
        // undocumented state: `NoFit` means exactly "no primary", in both
        // directions, with no alternatives to mislead a caller who checks
        // only one of the two fields.
        for (label, strategy) in every_strategy() {
            assert_eq!(
                strategy.basis == StrategyBasis::NoFit,
                strategy.primary.is_none(),
                "{label}: basis and primary disagree about whether anything fits"
            );
            if strategy.primary.is_none() {
                assert!(
                    strategy.alternatives.is_empty(),
                    "{label}: no primary, yet it lists alternatives"
                );
                assert_eq!(
                    strategy.transliteration,
                    TransliterationAdvice::Unsupported,
                    "{label}: nothing fits, so there is nothing to transliterate for"
                );
            }
        }
    }

    #[test]
    fn no_strategy_lists_its_own_primary_as_an_alternative() {
        for (label, strategy) in every_strategy() {
            if let Some(primary) = strategy.primary {
                assert!(
                    !strategy.alternatives.contains(&primary),
                    "{label} lists its own primary among its alternatives too"
                );
            }
        }
    }

    #[test]
    fn alternatives_are_free_of_duplicates() {
        for (label, strategy) in every_strategy() {
            for (i, a) in strategy.alternatives.iter().enumerate() {
                assert!(
                    !strategy.alternatives[i + 1..].contains(a),
                    "{label} lists {a:?} twice"
                );
            }
        }
    }

    #[test]
    fn encoders_yields_the_primary_then_the_alternatives() {
        let de = recommend(Language::German);
        let listed: Vec<_> = de.encoders().collect();
        assert_eq!(listed.first(), Some(&PhoneticRecommendation::Cologne));
        assert_eq!(listed.len(), 1 + de.alternatives.len());
        assert!(recommend(Language::Chinese).encoders().next().is_none());
    }

    #[test]
    fn every_beider_morse_tag_this_crate_emits_is_a_real_one() {
        // The defect class this test exists for: a language tag spelled the
        // way this crate thinks Beider-Morse spells it, silently resolving
        // to nothing at the encoder. `encode_language` returns `None` for a
        // tag its `NameType` does not know, so walking *every* tag from
        // *every* recommendation of *every* language and script through the
        // real encoder is the only check with no sampling in it.
        use verbora_phonetics::{BeiderMorse, NameType, RuleType};

        let encoder = BeiderMorse::new(NameType::Generic, RuleType::Approx);
        let mut checked = 0usize;
        for (label, strategy) in every_strategy() {
            for recommendation in strategy.encoders() {
                let PhoneticRecommendation::BeiderMorse { language } = recommendation else {
                    continue;
                };
                checked += 1;
                assert!(
                    encoder.encode_language("test", language).is_some(),
                    "{label} names Beider-Morse language {language:?}, \
                     which NameType::Generic does not know"
                );
            }
        }
        assert!(checked > 0, "the sweep found no Beider-Morse tags to check");
    }

    #[test]
    fn every_primary_produces_a_real_key_for_its_own_language() {
        // A recommendation is a claim that the encoder produces a key for
        // text in that language. This runs the primary encoder over a real
        // sample per language (in that language's own script, after the
        // advised transliteration) and requires a non-empty result — the
        // check that would have caught "recommend an A-Z encoder for
        // Cyrillic", which silently yields the empty key.
        use verbora_phonetics::{
            BeiderMorse, Cologne, DaitchMokotoff, DoubleMetaphone, Metaphone, NameType, RuleType,
            SoundEx,
        };

        let samples = [
            (Language::English, "Smith"),
            (Language::Spanish, "García"),
            (Language::Portuguese, "Silva"),
            (Language::Italian, "Rossi"),
            (Language::French, "Renault"),
            (Language::German, "Müller"),
            (Language::Dutch, "Vandenberg"),
            (Language::Russian, "Иванов"),
            (Language::Ukrainian, "Шевченко"),
            (Language::Polish, "Kowalski"),
            (Language::Persian, "محمدی"),
            (Language::Indonesian, "Wijaya"),
            (Language::Vietnamese, "Nguyễn"),
            (Language::Japanese, "たなか"),
            (Language::Norwegian, "Hansen"),
            (Language::Swedish, "Andersson"),
            (Language::Finnish, "Virtanen"),
            (Language::Galician, "Fernández"),
            (Language::Catalan, "Puig"),
            (Language::Basque, "Etxeberria"),
        ];
        assert_eq!(
            samples.len(),
            Language::ALL.len() - 2,
            "every language except the two NoFit ones needs a sample"
        );

        for (language, name) in samples {
            let strategy = recommend(language);
            let primary = strategy
                .primary
                .unwrap_or_else(|| panic!("{language:?} should have a primary"));
            let text = apply_transliteration(strategy.transliteration, name);
            let produced_something = match primary {
                PhoneticRecommendation::SoundEx => !SoundEx::new().process(&text).is_empty(),
                PhoneticRecommendation::Metaphone => !Metaphone::new().process(&text).is_empty(),
                PhoneticRecommendation::DoubleMetaphone => {
                    !DoubleMetaphone::new().process(&text).primary().is_empty()
                }
                PhoneticRecommendation::DaitchMokotoff => {
                    !DaitchMokotoff::new().process(&text).is_empty()
                }
                PhoneticRecommendation::Cologne => !Cologne::new().process(&text).is_empty(),
                PhoneticRecommendation::BeiderMorse { language: tag } => {
                    // A candidate list holding one empty string is not a
                    // key: that is what Beider-Morse returns when no rule
                    // in the chosen table matched a single character, which
                    // is exactly the failure the `russian`-vs-`cyrillic`
                    // note on `recommend` records.
                    BeiderMorse::new(NameType::Generic, RuleType::Approx)
                        .encode_language(&text, tag)
                        .is_some_and(|code| code.spellings.iter().any(|s| !s.is_empty()))
                }
            };
            assert!(
                produced_something,
                "{language:?}: the recommended encoder {primary:?} produced no key for {name:?}"
            );
        }
    }

    #[test]
    fn german_is_the_one_language_with_an_encoder_of_its_own() {
        // Cologne phonetics is Postel 1969, written for German; it is the
        // only encoder in the workspace whose publication names a language
        // other than English, so it is the only non-English `Named` primary
        // that is not Beider-Morse or Daitch-Mokotoff.
        assert_eq!(
            recommend(Language::German).primary,
            Some(PhoneticRecommendation::Cologne)
        );
        assert_eq!(recommend(Language::German).basis, StrategyBasis::Named);
        for language in Language::ALL {
            if language != Language::German {
                assert_ne!(
                    recommend(language).primary,
                    Some(PhoneticRecommendation::Cologne),
                    "{language:?} should not be recommended a German-specific encoder"
                );
            }
        }
    }

    #[test]
    fn languages_with_no_encoder_of_their_own_say_so() {
        // The honesty contract: these eight are written in the Latin
        // alphabet and nothing more, so their basis must be Script -- if a
        // future edit promotes one to Named, it has to name the publication
        // that justifies it, and this test is where it will be noticed.
        for language in [
            Language::Norwegian,
            Language::Swedish,
            Language::Finnish,
            Language::Galician,
            Language::Catalan,
            Language::Basque,
            Language::Indonesian,
            Language::Vietnamese,
        ] {
            assert_eq!(
                recommend(language).basis,
                StrategyBasis::Script,
                "{language:?}: no encoder's publication names this language"
            );
        }
    }

    #[test]
    fn devanagari_and_han_languages_get_nothing() {
        // Both scripts are unreadable to every encoder in the workspace and
        // unromanizable by every transliterator in it. A key would come
        // back -- every encoder is total -- and it would be worthless.
        for language in [Language::Hindi, Language::Chinese] {
            let strategy = recommend(language);
            assert_eq!(strategy, PhoneticStrategy::NO_FIT, "{language:?}");
        }
    }

    #[test]
    fn japanese_is_the_only_language_that_recommends_transliteration() {
        for language in Language::ALL {
            let expected = if language == Language::Japanese {
                TransliterationAdvice::Recommended
            } else if recommend(language).basis == StrategyBasis::NoFit {
                TransliterationAdvice::Unsupported
            } else {
                TransliterationAdvice::NotNeeded
            };
            assert_eq!(
                recommend(language).transliteration,
                expected,
                "{language:?}"
            );
        }
    }

    #[test]
    fn apply_transliteration_is_identity_when_not_needed_or_unsupported() {
        for advice in [
            TransliterationAdvice::NotNeeded,
            TransliterationAdvice::Unsupported,
        ] {
            for input in ["hello", "مرحبا", ""] {
                let out = apply_transliteration(advice, input);
                assert_eq!(out, input);
                assert!(
                    matches!(out, std::borrow::Cow::Borrowed(_)),
                    "{advice:?} must not allocate"
                );
            }
        }
    }

    #[test]
    fn apply_transliteration_actually_transliterates_japanese() {
        let out = apply_transliteration(TransliterationAdvice::Recommended, "にほん");
        // transliterate_ja's own contract, not re-derived here.
        assert_eq!(out, transliterate_ja("にほん"));
        assert_ne!(out, "にほん");
    }

    #[test]
    fn script_level_recommendations_are_never_language_level() {
        // `recommend_for_script` has a script and nothing else, so it can
        // never justify a Named basis -- that would be claiming knowledge
        // it does not have.
        for script in ALL_SCRIPTS {
            assert_ne!(
                recommend_for_script(script).basis,
                StrategyBasis::Named,
                "{script:?}: a script alone cannot justify a language-level claim"
            );
        }
    }

    #[test]
    fn recommend_for_script_is_cautious_about_han_but_not_kana() {
        // Han is ambiguous between Chinese and Japanese kanji, so the
        // coarse script-only fallback must not assume a transliteration
        // step is safe. Hiragana/Katakana are unambiguously Japanese, so
        // those DO get the transliteration recommendation.
        assert_eq!(recommend_for_script(Script::Han), PhoneticStrategy::NO_FIT);
        for script in [Script::Hiragana, Script::Katakana] {
            assert_eq!(
                recommend_for_script(script).transliteration,
                TransliterationAdvice::Recommended,
                "{script:?} should recommend transliteration"
            );
        }
    }

    #[test]
    fn native_script_recommendations_read_the_script_without_transliteration() {
        // The reason Beider-Morse is named for these four scripts at all:
        // its rule corpus has tables written in them. If a table stopped
        // reading its script, this would catch it -- an A-Z encoder on
        // Cyrillic yields the empty key, and `spellings()` would be empty.
        use verbora_phonetics::{BeiderMorse, NameType, RuleType};

        let encoder = BeiderMorse::new(NameType::Generic, RuleType::Approx);
        for (script, sample) in [
            (Script::Cyrillic, "Иванов"),
            (Script::Greek, "Παπας"),
            (Script::Hebrew, "כהן"),
            (Script::Arabic, "محمد"),
        ] {
            let strategy = recommend_for_script(script);
            assert_eq!(
                strategy.transliteration,
                TransliterationAdvice::NotNeeded,
                "{script:?} is read natively, so nothing needs romanizing first"
            );
            let Some(PhoneticRecommendation::BeiderMorse { language }) = strategy.primary else {
                panic!("{script:?} should be routed to Beider-Morse");
            };
            let code = encoder
                .encode_language(sample, language)
                .unwrap_or_else(|| panic!("{language:?} is not a Beider-Morse language"));
            assert!(
                code.spellings.iter().any(|s| !s.is_empty()),
                "{script:?}: Beider-Morse produced no spelling for {sample:?} under {language:?}"
            );
        }
    }

    #[test]
    fn beider_morses_russian_table_does_not_read_cyrillic() {
        // The measurement behind Russian's `Script` basis, kept as a test
        // rather than as a claim in a comment: if an upstream corpus update
        // ever teaches the `russian` table to read Cyrillic, this fails and
        // Russian's recommendation should be re-settled as `Named`.
        use verbora_phonetics::{BeiderMorse, NameType, RuleType};

        let encoder = BeiderMorse::new(NameType::Generic, RuleType::Approx);
        let russian = encoder
            .encode_language("Иванов", "russian")
            .expect("`russian` is one of Generic's languages");
        assert!(
            russian.spellings.iter().all(|s| s.is_empty()),
            "the `russian` table now reads Cyrillic: {:?}",
            russian.spellings
        );
        let cyrillic = encoder
            .encode_language("Иванов", "cyrillic")
            .expect("`cyrillic` is one of Generic's languages");
        assert!(
            cyrillic.spellings.iter().any(|s| !s.is_empty()),
            "the `cyrillic` table must read Cyrillic: {:?}",
            cyrillic.spellings
        );
    }

    #[test]
    fn recommendations_are_pure_and_deterministic() {
        for (label, strategy) in every_strategy() {
            for _ in 0..3 {
                let again = every_strategy()
                    .into_iter()
                    .find(|(l, _)| *l == label)
                    .map(|(_, s)| s);
                assert_eq!(again, Some(strategy), "{label} is not deterministic");
            }
        }
    }
}
