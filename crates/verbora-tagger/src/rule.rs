//! A transformation rule: "rewrite tag *a* as *b* wherever *condition* holds".

use std::fmt;
use std::str::FromStr;

use crate::condition::Condition;
use crate::parse::{self, RuleParseError};
use crate::tag::{Tag, TaggedToken};

/// Which tags a rule rewrites.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TagPattern {
    /// `*` — any tag at all.
    Any,
    /// Exactly this tag.
    Is(Tag),
}

impl TagPattern {
    /// Whether `tag` is rewritten by this pattern.
    #[inline]
    #[must_use]
    pub fn matches(&self, tag: &Tag) -> bool {
        match self {
            Self::Any => true,
            Self::Is(t) => t == tag,
        }
    }
}

impl fmt::Display for TagPattern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Any => f.write_str("*"),
            Self::Is(t) => fmt::Display::fmt(t, f),
        }
    }
}

/// A Brill transformation rule.
///
/// # Rule strings
///
/// A rule is written as whitespace-separated fields:
///
/// ```text
/// rule      = pattern new_tag condition_name argument*
/// pattern   = "*" | tag
/// argument  = tag | word | "YES" | "NO"
/// ```
///
/// Fields are the maximal runs of scalars **without** the Unicode `White_Space`
/// property, exactly as [`str::split_whitespace`] yields them, so leading,
/// trailing and repeated whitespace are all insignificant. There are no
/// comments, no quoting and no escapes: a field is its own text. That is why a
/// [`Tag`] and a [`Word`](crate::Word) may not contain whitespace, and why
/// `rule.to_string().parse::<Rule>()` round-trips.
///
/// The condition name fixes how many arguments follow and what each one is. A
/// mismatched count is an error rather than a silently ignored or silently
/// invented argument, and an unrecognised condition name is an error too: a rule
/// that could never fire is a typo, not a rule. See [`RuleParseError`].
///
/// ```
/// use verbora_tagger::{Condition, Rule, Tag, TagPattern};
///
/// let rule: Rule = "NN VB PREV-TAG TO".parse()?;
/// assert_eq!(rule.from, TagPattern::Is(Tag::new("NN")?));
/// assert_eq!(rule.to, Tag::new("VB")?);
/// assert_eq!(rule.condition, Condition::PrevTag(Tag::new("TO")?));
/// assert_eq!(rule.to_string(), "NN VB PREV-TAG TO");
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
///
/// See [`Condition`] for the full table of condition names, their aliases and
/// their arguments.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Rule {
    /// The tag the rule rewrites.
    pub from: TagPattern,
    /// The tag it assigns.
    pub to: Tag,
    /// The condition it tests.
    pub condition: Condition,
}

impl Rule {
    /// Builds a rule from its three parts.
    #[must_use]
    pub const fn new(from: TagPattern, to: Tag, condition: Condition) -> Self {
        Self {
            from,
            to,
            condition,
        }
    }

    /// Whether the rule fires at position `i` of `words`.
    ///
    /// `false` for an `i` outside `words`, and for a site whose tag the pattern
    /// does not match. Never panics.
    #[inline]
    #[must_use]
    pub fn applies_at(&self, words: &[TaggedToken<'_>], i: usize) -> bool {
        words
            .get(i)
            .is_some_and(|w| self.from.matches(&w.tag) && self.condition.holds(words, i))
    }

    /// How far the rule reads, as `(left, right)` positions from the site.
    ///
    /// Never more than three either way; see [`Condition::reach`].
    #[inline]
    #[must_use]
    pub const fn reach(&self) -> (usize, usize) {
        self.condition.reach()
    }
}

impl fmt::Display for Rule {
    /// The rule string that parses back to this rule.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {} {}", self.from, self.to, self.condition)
    }
}

impl FromStr for Rule {
    type Err = RuleParseError;

    fn from_str(s: &str) -> Result<Self, RuleParseError> {
        parse::rule(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tag::Word;

    fn tok(t: &'static str, g: &'static str) -> TaggedToken<'static> {
        TaggedToken::new(t, Tag::new(g).unwrap())
    }

    #[test]
    fn wildcard_matches_every_tag() {
        let rule: Rule = "* RB CURRENT-WORD-ENDS-WITH ly".parse().unwrap();
        assert_eq!(rule.from, TagPattern::Any);
        let s = vec![tok("quickly", "NN")];
        assert!(rule.applies_at(&s, 0));
        let s = vec![tok("quickly", "ZZ")];
        assert!(rule.applies_at(&s, 0));
        let s = vec![tok("quick", "NN")];
        assert!(!rule.applies_at(&s, 0));
    }

    #[test]
    fn an_exact_pattern_matches_only_itself() {
        let rule: Rule = "NN VB PREV-TAG TO".parse().unwrap();
        let s = vec![tok("to", "TO"), tok("book", "NN")];
        assert!(rule.applies_at(&s, 1));
        let s = vec![tok("to", "TO"), tok("book", "VB")];
        assert!(!rule.applies_at(&s, 1));
    }

    #[test]
    fn out_of_range_sites_do_not_apply() {
        let rule: Rule = "NN VB PREV-TAG TO".parse().unwrap();
        let s = vec![tok("book", "NN")];
        assert!(!rule.applies_at(&s, 0), "no previous word");
        assert!(!rule.applies_at(&s, 1));
        assert!(!rule.applies_at(&s, usize::MAX));
        assert!(!rule.applies_at(&[], 0));
    }

    #[test]
    fn display_round_trips() {
        for source in [
            "NN VB PREV-TAG TO",
            "* RB CURRENT-WORD-ENDS-WITH ly",
            "NN CD CURRENT-WORD-IS-NUMBER YES",
            "NP NN CURRENT-WORD-IS-CAP NO",
            "A B SURROUND-TAG X Y",
            "A B WORD-AND-PREV-TAG X y",
        ] {
            let rule: Rule = source.parse().unwrap();
            assert_eq!(rule.to_string(), source);
            assert_eq!(rule.to_string().parse::<Rule>().unwrap(), rule);
        }
    }

    /// The round-trip property holds for every rule that can be **built**, not
    /// only for every rule that can be parsed. A rule assembled through
    /// [`Rule::new`] never prints as a different rule than the one it is.
    #[test]
    fn every_constructible_rule_round_trips() {
        // Every string the `Tag` contract admits is fair game as a `from`
        // pattern; the ones it rejects cannot reach `Rule::new` at all.
        for candidate in [
            "NN",
            "Adj(attr,stell,onverv)",
            "*",
            "**",
            "-LRB-",
            "PRP$",
            "JJ|NN",
            "?",
        ] {
            let Ok(from) = Tag::new(candidate) else {
                continue;
            };
            let rule = Rule::new(
                TagPattern::Is(from),
                Tag::new("VB").unwrap(),
                Condition::PrevTag(Tag::new("TO").unwrap()),
            );
            assert_eq!(
                rule.to_string().parse::<Rule>().unwrap(),
                rule,
                "{candidate:?} does not round-trip"
            );
        }
    }

    #[test]
    fn reach_is_the_conditions_reach() {
        let rule = Rule::new(
            TagPattern::Any,
            Tag::new("X").unwrap(),
            Condition::PrevTagWithin3(Tag::new("Y").unwrap()),
        );
        assert_eq!(rule.reach(), (3, 0));
        let rule = Rule::new(
            TagPattern::Any,
            Tag::new("X").unwrap(),
            Condition::CurrentWordEndsWith(Word::new("s").unwrap()),
        );
        assert_eq!(rule.reach(), (0, 0));
    }
}
