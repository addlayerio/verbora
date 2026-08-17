//! Known-abbreviation lists, ported from `abbreviations_en` and
//! `abbreviations_es`.
//!
//! `SentenceTokenizer` joins these into a case-insensitive regex alternation, so
//! **order is load-bearing**: alternation is leftmost-first in both the reference
//! and the Rust `regex` crate, and the English list deliberately puts `'e.g.'`
//! ahead of `'i.e.'`. The Spanish list additionally contains one duplicate
//! (`'loc. cit.'`) and entries with internal spaces (`'et al.'`, `'a
//! posteriori.'`). All of that is preserved verbatim.

use crate::data;

/// English abbreviations: The reference `abbreviations`.
pub use crate::data::ABBREVIATIONS_EN;
/// Spanish abbreviations: The reference `abbreviations_es`.
///
/// Note the runtime key is snake_case. `index.d.ts` declares `abbreviationsEs`,
/// which does not exist at runtime.
pub use crate::data::ABBREVIATIONS_ES;

/// A language with an abbreviation list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum AbbreviationLanguage {
    /// English.
    En,
    /// Spanish.
    Es,
}

impl AbbreviationLanguage {
    /// The ISO 639-1 code.
    pub fn code(self) -> &'static str {
        match self {
            Self::En => "en",
            Self::Es => "es",
        }
    }

    /// The list in source order, duplicates included.
    pub fn abbreviations(self) -> &'static [&'static str] {
        match self {
            Self::En => data::ABBREVIATIONS_EN,
            Self::Es => data::ABBREVIATIONS_ES,
        }
    }

    /// Whether `text` is a known abbreviation, compared case-sensitively.
    ///
    /// The arrays themselves are case-sensitive data; it is `SentenceTokenizer`
    /// that later folds case when it builds its regex, not the list.
    pub fn contains(self, text: &str) -> bool {
        let sorted = match self {
            Self::En => data::ABBREVIATIONS_EN_SORTED,
            Self::Es => data::ABBREVIATIONS_ES_SORTED,
        };
        sorted.binary_search(&text).is_ok()
    }

    /// The index of `text` in the source-order list, or `None`.
    ///
    /// Mirrors `knownAbbreviations.indexOf(text)`, first match wins.
    pub fn index_of(self, text: &str) -> Option<usize> {
        self.abbreviations().iter().position(|a| *a == text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn english_list_shape() {
        let list = AbbreviationLanguage::En.abbreviations();
        assert_eq!(list.len(), 24);
        assert_eq!(list[0], "approx.");
        assert_eq!(list[list.len() - 1], "i.e.");
        // 'e.g.' must precede 'i.e.' — leftmost-first alternation depends on it.
        let eg = AbbreviationLanguage::En.index_of("e.g.").unwrap();
        let ie = AbbreviationLanguage::En.index_of("i.e.").unwrap();
        assert!(eg < ie);
    }

    #[test]
    fn spanish_list_keeps_its_duplicate() {
        let list = AbbreviationLanguage::Es.abbreviations();
        assert_eq!(list.len(), 108);
        assert_eq!(list.iter().filter(|a| **a == "loc. cit.").count(), 2);
        // Multi-word entries and accents survive.
        assert!(list.contains(&"et al."));
        assert!(list.contains(&"a posteriori."));
        assert!(list.iter().any(|a| a.contains('í')));
    }

    #[test]
    fn membership_is_case_sensitive() {
        assert!(AbbreviationLanguage::En.contains("Dr."));
        assert!(!AbbreviationLanguage::En.contains("dr."));
        assert!(!AbbreviationLanguage::En.contains("Dr"));
        assert!(AbbreviationLanguage::Es.contains("Sr."));
        assert!(!AbbreviationLanguage::Es.contains(""));
    }

    #[test]
    fn index_of_reports_the_first_match() {
        let first = AbbreviationLanguage::Es.index_of("loc. cit.").unwrap();
        let all: Vec<usize> = AbbreviationLanguage::Es
            .abbreviations()
            .iter()
            .enumerate()
            .filter(|(_, a)| **a == "loc. cit.")
            .map(|(i, _)| i)
            .collect();
        assert_eq!(all.len(), 2);
        assert_eq!(first, all[0]);
    }

    #[test]
    fn sorted_lists_stay_in_step() {
        for lang in [AbbreviationLanguage::En, AbbreviationLanguage::Es] {
            for a in lang.abbreviations() {
                assert!(lang.contains(a), "{} lost {a:?}", lang.code());
            }
        }
    }
}
