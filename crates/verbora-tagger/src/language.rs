//! The two languages Verbora ships Brill data for.

use crate::data;
use crate::tag::Tag;

/// A language whose lexicon and rule set are bundled in the binary.
///
/// There is no string-keyed constructor and no fallback language: a caller names
/// the language it wants, and an unknown name is a compile error rather than a
/// silent default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Language {
    /// English: 92,661 lexicon entries (Penn Treebank tags plus Verbora's
    /// `URL`) and 18 transformation rules.
    English,
    /// Dutch: 11,699 lexicon entries and 285 transformation rules.
    Dutch,
}

impl Language {
    /// The tag an unknown, uncapitalised token takes.
    ///
    /// Brill (1995) §2 specifies the initial-state annotator for unknown words
    /// as "assume a word is a noun, or a proper noun if capitalised"; these are
    /// that rule expressed in each bundled tag set.
    ///
    /// | Language | default | capitalised default |
    /// |---|---|---|
    /// | English | `NN` | `NNP` |
    /// | Dutch | `N(soort,ev,neut)` | `N(eigen,ev,neut)` |
    #[must_use]
    pub const fn default_tag(self) -> Tag {
        match self {
            Self::English => Tag::from_static("NN"),
            Self::Dutch => Tag::from_static("N(soort,ev,neut)"),
        }
    }

    /// The tag an unknown, capitalised token takes. See [`Self::default_tag`].
    #[must_use]
    pub const fn capitalized_default_tag(self) -> Tag {
        match self {
            Self::English => Tag::from_static("NNP"),
            Self::Dutch => Tag::from_static("N(eigen,ev,neut)"),
        }
    }

    /// The bundled rule strings, in the order they are applied.
    #[must_use]
    pub const fn rule_strings(self) -> &'static [&'static str] {
        match self {
            Self::English => data::ENGLISH_RULES,
            Self::Dutch => data::DUTCH_RULES,
        }
    }

    /// The packed dictionary for this language.
    pub(crate) const fn lexicon(self) -> data::StaticLexicon {
        match self {
            Self::English => data::StaticLexicon::english(),
            Self::Dutch => data::StaticLexicon::dutch(),
        }
    }
}

/// The ten rules published in Brill (1992), Table 1, learned from the Brown
/// corpus and written in that paper's tag set.
///
/// They are bundled as a worked example of the rule format and of a rule set
/// that is *not* Verbora's own; they are not the tag set the bundled English
/// lexicon uses, so a tagger pairing the two will not agree with the paper.
#[must_use]
pub const fn brill_paper_rule_strings() -> &'static [&'static str] {
    data::BRILL_PAPER_RULES
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The default tags are built with the unchecked `Tag::from_static`, so the
    /// literal contract is asserted here instead.
    #[test]
    fn bundled_defaults_satisfy_the_literal_contract() {
        for lang in [Language::English, Language::Dutch] {
            for t in [lang.default_tag(), lang.capitalized_default_tag()] {
                assert!(Tag::new(t.as_str().to_owned()).is_ok(), "{t}");
            }
        }
    }

    /// Each language's defaults must be tags its own lexicon actually uses,
    /// or the initial-state annotator would emit a tag no rule can match.
    #[test]
    fn defaults_appear_in_their_own_lexicon() {
        for lang in [Language::English, Language::Dutch] {
            let lex = lang.lexicon();
            for want in [lang.default_tag(), lang.capitalized_default_tag()] {
                let found = (0..lex.len()).any(|i| lex.tags(i).any(|t| t == want.as_str()));
                assert!(found, "{want} is not a tag in the {lang:?} lexicon");
            }
        }
    }

    #[test]
    fn rule_string_counts() {
        assert_eq!(Language::English.rule_strings().len(), 18);
        assert_eq!(Language::Dutch.rule_strings().len(), 285);
        assert_eq!(brill_paper_rule_strings().len(), 10);
    }
}
