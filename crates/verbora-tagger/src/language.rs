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
    /// English: 92,538 lexicon entries and 18 transformation rules.
    ///
    /// The lexicon's tag set is 122 strings, and only 48 of them are single
    /// tags: the Penn Treebank word classes (`NN`, `VBD`, `JJR`, …), a set of
    /// punctuation labels (`.`, `,`, `:`, `;`, `!`, `#`, `$`, `(`, `)`, and two
    /// quote marks), and `JJSS` and `PRP$R`, which the Penn tag set does not
    /// define. The remaining 74 are `|`-joined **ambiguity classes** — `JJ|NN`,
    /// `NNPS|VBZ`, `IN|RB` — recording that the source annotation could not
    /// choose between two tags. They are not parts of speech, and a token that
    /// takes one has not been disambiguated.
    ///
    /// Two tags the rules assign are absent from the lexicon: `CD`, and
    /// Verbora's own `URL`. A token receives either one only by a rule firing on
    /// it, never from the initial-state annotator.
    English,
    /// Dutch: 11,699 lexicon entries (194 tags, none of them ambiguity classes)
    /// and 273 transformation rules.
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

    /// Every bundled rule can fire, and every word it names is a word.
    ///
    /// A rule is dead when its `from` pattern names a tag nothing in the
    /// language's own configuration can produce, or when one of its condition's
    /// **tag** arguments does: `Condition::holds` is then false at every site,
    /// for every input, and the rule costs a full pass over each sentence to
    /// change nothing.
    ///
    /// The tags a configuration can produce are exactly: the primary tag of some
    /// lexicon entry (what the initial-state annotator assigns), the two
    /// defaults, and the `to` tag of some rule. Secondary lexicon tags are not
    /// among them — [`Lexicon::tag_of`](crate::Lexicon::tag_of) reads the first
    /// tag of an entry and no other.
    ///
    /// **Word** arguments are checked differently, because a word is compared
    /// against caller-supplied token text and no bundled data constrains that.
    /// What the bundled data does constrain is itself: a bundled rule set and
    /// the bundled lexicon beside it come from the same corpus, so a word the
    /// lexicon has never seen is corpus markup that leaked out of the training
    /// data — `STAART`, the Dutch corpus's sentence-boundary marker, reached the
    /// shipped rule set exactly that way and appears in no lexicon entry, in no
    /// source file and in no test. A `CURRENT-WORD-ENDS-WITH` argument is a
    /// suffix rather than a token, so it is required to be the suffix of some
    /// key instead of a key.
    #[test]
    fn every_bundled_rule_can_fire() {
        use crate::rule::TagPattern;
        use std::collections::BTreeSet;

        for lang in [Language::English, Language::Dutch] {
            let lex = lang.lexicon();
            let rules = crate::ruleset::RuleSet::bundled(lang);
            let mut producible: BTreeSet<&str> = BTreeSet::new();
            for i in 0..lex.len() {
                if let Some(primary) = lex.tags(i).next() {
                    producible.insert(primary);
                }
            }
            let defaults = [lang.default_tag(), lang.capitalized_default_tag()];
            for d in &defaults {
                producible.insert(d.as_str());
            }
            for rule in rules.rules() {
                producible.insert(rule.to.as_str());
            }

            for rule in rules.rules() {
                if let TagPattern::Is(t) = &rule.from {
                    assert!(
                        producible.contains(t.as_str()),
                        "{lang:?}: {rule} rewrites {t}, which nothing produces"
                    );
                }
                for t in rule.condition.tag_arguments().into_iter().flatten() {
                    assert!(
                        producible.contains(t.as_str()),
                        "{lang:?}: {rule} tests for {t}, which nothing produces"
                    );
                }
                for w in rule.condition.word_arguments().into_iter().flatten() {
                    assert!(
                        lex.find(w.as_str()).is_some(),
                        "{lang:?}: {rule} tests for the word {w}, \
                         which is not a token of the bundled lexicon"
                    );
                }
                if let Some(suffix) = rule.condition.suffix_argument() {
                    assert!(
                        (0..lex.len()).any(|i| lex.key(i).ends_with(suffix.as_str())),
                        "{lang:?}: {rule} tests for the suffix {suffix}, \
                         which ends no key of the bundled lexicon"
                    );
                }
            }
        }
    }

    /// What the bundled lexicons' tag sets actually are.
    ///
    /// Pinned because [`Language`]'s own documentation describes them, and a
    /// description of a data file goes stale silently.
    #[test]
    fn the_bundled_lexicon_tag_sets_are_as_documented() {
        use std::collections::BTreeSet;

        let lex = Language::English.lexicon();
        let mut tags: BTreeSet<&str> = BTreeSet::new();
        for i in 0..lex.len() {
            tags.extend(lex.tags(i));
        }
        assert_eq!(tags.len(), 122);
        assert_eq!(tags.iter().filter(|t| t.contains('|')).count(), 74);
        // The two tags the rules assign and the lexicon never does.
        assert!(!tags.contains("URL"));
        assert!(!tags.contains("CD"));
        for produced_by_rules in ["URL", "CD"] {
            assert!(
                Language::English
                    .rule_strings()
                    .iter()
                    .any(|r| r.split_whitespace().nth(1) == Some(produced_by_rules)),
                "{produced_by_rules} is not produced by a rule either"
            );
        }
        // The Dutch tag set is 194 strings and holds no ambiguity class.
        let nl = Language::Dutch.lexicon();
        let mut nl_tags: BTreeSet<&str> = BTreeSet::new();
        for i in 0..nl.len() {
            nl_tags.extend(nl.tags(i));
        }
        assert_eq!(nl_tags.len(), 194);
        assert!(!nl_tags.iter().any(|t| t.contains('|')));
    }

    #[test]
    fn rule_string_counts() {
        assert_eq!(Language::English.rule_strings().len(), 18);
        assert_eq!(Language::Dutch.rule_strings().len(), 273);
        assert_eq!(brill_paper_rule_strings().len(), 10);
    }
}
