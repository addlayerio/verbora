//! Stop-word lists for sixteen languages.
//!
//! # English is different from the other fifteen
//!
//! The reference `stopwords` exports one array that every stemmer's
//! `addStopWord`/`removeStopWord` mutates and that the stemmers, the phonetics
//! helpers and TF-IDF all read — process-wide, by identity. That shared mutable
//! state is modelled in [`verbora_core::stopwords`], which this module
//! re-exports rather than duplicates: two copies of a process-global would
//! defeat the point of having one.
//!
//! The per-language lists have no such machinery in the reference. Nothing
//! mutates them, so they are compile-time statics here.
//!
//! # German has no file
//!
//! There is no `stopwords_de`. `stemmers/stemmer_de` reaches for the
//! `stopwords-iso` package and takes `stopwords.de`, binding it to a
//! variable named — misleadingly — `englishStopWords`. [`Language::De`] carries
//! that package's German list, so the port needs no external dependency.

pub use verbora_core::stopwords::{
    DEFAULT_EN, StopWords, add_global_stopword, add_global_stopwords, global_stopwords,
    is_default_stopword, remove_global_stopword, remove_global_stopwords, reset_global_stopwords,
};

use crate::data;

/// A language with a stop-word list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum Language {
    /// English — the process-global, mutable list.
    En,
    /// German, via the `stopwords-iso` package (see the module note).
    De,
    /// Spanish.
    Es,
    /// Persian.
    Fa,
    /// French.
    Fr,
    /// Indonesian.
    Id,
    /// Italian.
    It,
    /// Japanese.
    Ja,
    /// Dutch.
    Nl,
    /// Norwegian.
    No,
    /// Polish.
    Pl,
    /// Portuguese.
    Pt,
    /// Russian.
    Ru,
    /// Swedish.
    Sv,
    /// Ukrainian.
    Uk,
    /// Chinese.
    Zh,
}

/// Every language with a stop-word list, in ISO-code order after English.
pub static LANGUAGES: &[Language] = &[
    Language::En,
    Language::De,
    Language::Es,
    Language::Fa,
    Language::Fr,
    Language::Id,
    Language::It,
    Language::Ja,
    Language::Nl,
    Language::No,
    Language::Pl,
    Language::Pt,
    Language::Ru,
    Language::Sv,
    Language::Uk,
    Language::Zh,
];

impl Language {
    /// The ISO 639-1 code, matching the `stopwords_XX` file suffix.
    pub fn code(self) -> &'static str {
        match self {
            Self::En => "en",
            Self::De => "de",
            Self::Es => "es",
            Self::Fa => "fa",
            Self::Fr => "fr",
            Self::Id => "id",
            Self::It => "it",
            Self::Ja => "ja",
            Self::Nl => "nl",
            Self::No => "no",
            Self::Pl => "pl",
            Self::Pt => "pt",
            Self::Ru => "ru",
            Self::Sv => "sv",
            Self::Uk => "uk",
            Self::Zh => "zh",
        }
    }

    /// Looks a language up by ISO code.
    pub fn from_code(code: &str) -> Option<Self> {
        LANGUAGES.iter().copied().find(|l| l.code() == code)
    }

    /// The list in reference source order, duplicates included.
    ///
    /// Order is observable — the reference exposes the array — and several lists
    /// really do repeat entries (Spanish has 2, Russian 8, Ukrainian 6). Both are
    /// preserved.
    ///
    /// For [`Language::En`] this is the *unmutated* default. Use
    /// [`global_stopwords`] to read the list including any runtime additions.
    pub fn stopwords(self) -> &'static [&'static str] {
        match self {
            Self::En => DEFAULT_EN,
            Self::De => data::STOPWORDS_DE,
            Self::Es => data::STOPWORDS_ES,
            Self::Fa => data::STOPWORDS_FA,
            Self::Fr => data::STOPWORDS_FR,
            Self::Id => data::STOPWORDS_ID,
            Self::It => data::STOPWORDS_IT,
            Self::Ja => data::STOPWORDS_JA,
            Self::Nl => data::STOPWORDS_NL,
            Self::No => data::STOPWORDS_NO,
            Self::Pl => data::STOPWORDS_PL,
            Self::Pt => data::STOPWORDS_PT,
            Self::Ru => data::STOPWORDS_RU,
            Self::Sv => data::STOPWORDS_SV,
            Self::Uk => data::STOPWORDS_UK,
            Self::Zh => data::STOPWORDS_ZH,
        }
    }

    /// The list sorted by UTF-8 bytes and de-duplicated, for membership tests.
    ///
    /// `None` for [`Language::En`], whose membership must go through
    /// [`is_default_stopword`] so that runtime mutation is honoured.
    fn sorted(self) -> Option<&'static [&'static str]> {
        Some(match self {
            Self::En => return None,
            Self::De => data::STOPWORDS_DE_SORTED,
            Self::Es => data::STOPWORDS_ES_SORTED,
            Self::Fa => data::STOPWORDS_FA_SORTED,
            Self::Fr => data::STOPWORDS_FR_SORTED,
            Self::Id => data::STOPWORDS_ID_SORTED,
            Self::It => data::STOPWORDS_IT_SORTED,
            Self::Ja => data::STOPWORDS_JA_SORTED,
            Self::Nl => data::STOPWORDS_NL_SORTED,
            Self::No => data::STOPWORDS_NO_SORTED,
            Self::Pl => data::STOPWORDS_PL_SORTED,
            Self::Pt => data::STOPWORDS_PT_SORTED,
            Self::Ru => data::STOPWORDS_RU_SORTED,
            Self::Sv => data::STOPWORDS_SV_SORTED,
            Self::Uk => data::STOPWORDS_UK_SORTED,
            Self::Zh => data::STOPWORDS_ZH_SORTED,
        })
    }

    /// Whether `word` is a stop word in this language.
    ///
    /// Case-sensitive exact equality, as in the reference — `indexOf` does no
    /// folding, so `"The"` is not a stop word while `"the"` is.
    ///
    /// O(log n) via binary search, except for [`Language::En`], which consults
    /// the process-global list so that `add_global_stopword` is honoured.
    pub fn is_stopword(self, word: &str) -> bool {
        match self.sorted() {
            Some(sorted) => sorted.binary_search(&word).is_ok(),
            None => is_default_stopword(word),
        }
    }

    /// The index of `word` in the source-order list, or `None`.
    ///
    /// This is the reference's own lookup: TF-IDF filters with
    /// `stopwords.indexOf(term) < 0`. Prefer [`Language::is_stopword`] unless the
    /// position itself matters — this one is a linear scan, exactly as in
    /// the reference.
    pub fn stopword_index(self, word: &str) -> Option<usize> {
        self.stopwords().iter().position(|w| *w == word)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_language_has_a_list() {
        for &lang in LANGUAGES {
            assert!(!lang.stopwords().is_empty(), "{} has no words", lang.code());
            assert_eq!(Language::from_code(lang.code()), Some(lang));
        }
        assert_eq!(LANGUAGES.len(), 16);
        assert_eq!(Language::from_code("xx"), None);
    }

    #[test]
    fn sorted_lists_agree_with_source_order() {
        // Guards against `data.rs` going stale in one half only.
        for &lang in LANGUAGES {
            let Some(sorted) = lang.sorted() else {
                continue;
            };
            assert!(
                sorted.windows(2).all(|w| w[0] < w[1]),
                "{} is not strictly sorted",
                lang.code()
            );
            for word in lang.stopwords() {
                assert!(
                    sorted.binary_search(word).is_ok(),
                    "{} lost {word:?}",
                    lang.code()
                );
            }
            let mut unique: Vec<&str> = lang.stopwords().to_vec();
            unique.sort_unstable();
            unique.dedup();
            assert_eq!(unique.len(), sorted.len(), "{} size mismatch", lang.code());
        }
    }

    #[test]
    fn recorded_lengths_and_duplicate_counts() {
        // The lengths and duplicate counts verified against the reference.
        // Duplicates are in the source and must survive the port.
        for (lang, len, dupes) in [
            (Language::Es, 70, 2),
            (Language::Fa, 26, 0),
            (Language::Fr, 168, 0),
            (Language::Id, 809, 0),
            (Language::It, 290, 0),
            (Language::Ja, 109, 0),
            (Language::Nl, 143, 1),
            (Language::No, 129, 2),
            (Language::Pl, 291, 1),
            (Language::Pt, 117, 0),
            (Language::Ru, 137, 8),
            (Language::Sv, 428, 0),
            (Language::Uk, 124, 6),
            (Language::Zh, 78, 0),
        ] {
            let words = lang.stopwords();
            assert_eq!(words.len(), len, "{} length", lang.code());
            let mut unique = words.to_vec();
            unique.sort_unstable();
            unique.dedup();
            assert_eq!(words.len() - unique.len(), dupes, "{} dupes", lang.code());
        }
        assert_eq!(Language::En.stopwords().len(), 170);
    }

    #[test]
    fn membership_is_case_sensitive() {
        assert!(Language::En.is_stopword("the"));
        assert!(!Language::En.is_stopword("The"));
        assert!(Language::Es.is_stopword("porque"));
        assert!(!Language::Es.is_stopword("PORQUE"));
        assert!(Language::Ru.is_stopword("и"));
        assert!(Language::Zh.is_stopword("的"));
        assert!(!Language::Es.is_stopword(""));
    }

    #[test]
    fn index_reports_the_first_occurrence() {
        // 'un' appears at 1 and again at 15 in the Spanish list.
        assert_eq!(Language::Es.stopword_index("a"), Some(0));
        assert_eq!(Language::Es.stopword_index("un"), Some(1));
        assert_eq!(Language::Es.stopword_index("nope"), None);
    }

    #[test]
    fn non_ascii_lists_survive_the_round_trip() {
        assert!(Language::Fa.stopwords().iter().any(|w| w.contains('ا')));
        assert!(Language::Ja.stopwords().iter().any(|w| !w.is_ascii()));
        assert!(Language::Uk.stopwords().iter().any(|w| w.contains('і')));
    }
}
