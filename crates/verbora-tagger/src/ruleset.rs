//! An ordered, duplicate-free sequence of transformation rules.

use std::fmt;
use std::str::FromStr;

use rustc_hash::FxHashSet;

use crate::language::Language;
use crate::parse::RuleParseError;
use crate::rule::Rule;

/// Why a multi-rule text did not parse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleSetParseError {
    /// One-based line number of the offending rule.
    pub line: usize,
    /// What was wrong with it.
    pub cause: RuleParseError,
}

impl fmt::Display for RuleSetParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.cause)
    }
}

impl std::error::Error for RuleSetParseError {}

/// The rules a [`BrillTagger`](crate::BrillTagger) applies, in order.
///
/// # Order is the meaning
///
/// Rules are applied **one at a time, each to the whole sentence**, in the order
/// they appear here — the order Brill (1995) §2 specifies. Two rule sets with
/// the same members in a different order are different taggers:
///
/// ```
/// use verbora_tagger::{BrillTagger, Lexicon, RuleSet, Tag};
///
/// let lexicon = Lexicon::new(Tag::new("NN")?);
/// let forward: RuleSet = "NN VB PREV-TAG DT\nNN JJ NEXT-TAG NN".parse()?;
/// let reverse: RuleSet = "NN JJ NEXT-TAG NN\nNN VB PREV-TAG DT".parse()?;
///
/// let words = ["the", "a", "b"];
/// let mut seed = Lexicon::new(Tag::new("NN")?);
/// seed.insert("the", vec![Tag::new("DT")?])?;
///
/// let a = BrillTagger::new(&seed, &forward).tag(words);
/// let b = BrillTagger::new(&seed, &reverse).tag(words);
/// assert_eq!(a[1].tag().as_str(), "VB");
/// assert_eq!(b[1].tag().as_str(), "JJ");
/// # let _ = lexicon;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
///
/// # Duplicates
///
/// A rule already present — same pattern, same new tag, same condition — is not
/// added a second time, and the first occurrence keeps its position. Identity is
/// structural, on the three fields; there is no string key to collide, which
/// matters because Dutch tags contain commas and a comma-joined key really can
/// merge two different rules.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RuleSet {
    rules: Vec<Rule>,
    seen: FxHashSet<Rule>,
}

impl RuleSet {
    /// An empty rule set.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The rules Verbora bundles for `language`.
    ///
    /// # Panics
    ///
    /// Never: every bundled rule string is parsed by
    /// `tests::every_bundled_rule_parses`, which enumerates all 313 of them.
    #[must_use]
    pub fn bundled(language: Language) -> Self {
        Self::parse_lines(language.rule_strings())
            .unwrap_or_else(|e| panic!("bundled rule set does not parse: {e}"))
    }

    /// Parses one rule string per element, in order.
    ///
    /// # Errors
    ///
    /// The first rule that does not parse, with its index as the line number.
    pub fn parse_lines<S: AsRef<str>>(sources: &[S]) -> Result<Self, RuleSetParseError> {
        let mut set = Self::new();
        for (i, s) in sources.iter().enumerate() {
            let rule = s
                .as_ref()
                .parse::<Rule>()
                .map_err(|cause| RuleSetParseError { line: i + 1, cause })?;
            set.push(rule);
        }
        Ok(set)
    }

    /// Appends a rule, returning `false` if an identical one is already present.
    pub fn push(&mut self, rule: Rule) -> bool {
        if !self.seen.insert(rule.clone()) {
            return false;
        }
        self.rules.push(rule);
        true
    }

    /// Whether an identical rule is present.
    #[must_use]
    pub fn contains(&self, rule: &Rule) -> bool {
        self.seen.contains(rule)
    }

    /// The rules, in application order.
    #[inline]
    #[must_use]
    pub fn rules(&self) -> &[Rule] {
        &self.rules
    }

    /// Consumes the set, yielding its rules in application order.
    #[inline]
    #[must_use]
    pub fn into_rules(self) -> Vec<Rule> {
        self.rules
    }

    /// Keeps only the rules `keep` accepts, preserving order.
    pub fn retain(&mut self, mut keep: impl FnMut(&Rule) -> bool) {
        self.rules.retain(|r| {
            let k = keep(r);
            if !k {
                self.seen.remove(r);
            }
            k
        });
    }

    /// Number of rules.
    #[must_use]
    pub fn len(&self) -> usize {
        self.rules.len()
    }

    /// Whether the set is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    /// How far the whole set can read from a site, as `(left, right)`.
    ///
    /// Each rule is applied to the entire sentence before the next one runs, so
    /// rule *k*'s answer at position *i* can depend on rule *k-1*'s answers
    /// within its own reach, and the reaches add. The sum is exactly the context
    /// [`BrillTagger::tag_stream`](crate::BrillTagger::tag_stream) must buffer
    /// on each side for its output to equal
    /// [`BrillTagger::tag`](crate::BrillTagger::tag)'s.
    #[must_use]
    pub fn context_span(&self) -> (usize, usize) {
        self.rules.iter().fold((0, 0), |(l, r), rule| {
            let (a, b) = rule.reach();
            (l + a, r + b)
        })
    }
}

impl fmt::Display for RuleSet {
    /// One rule per line, each followed by a newline. Parses back to an equal
    /// rule set.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for r in &self.rules {
            writeln!(f, "{r}")?;
        }
        Ok(())
    }
}

impl FromStr for RuleSet {
    type Err = RuleSetParseError;

    /// One rule per line. Blank and whitespace-only lines are skipped, so a
    /// trailing newline is fine.
    fn from_str(s: &str) -> Result<Self, RuleSetParseError> {
        let mut set = Self::new();
        for (i, line) in s.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let rule = line
                .parse::<Rule>()
                .map_err(|cause| RuleSetParseError { line: i + 1, cause })?;
            set.push(rule);
        }
        Ok(set)
    }
}

impl FromIterator<Rule> for RuleSet {
    fn from_iter<T: IntoIterator<Item = Rule>>(iter: T) -> Self {
        let mut set = Self::new();
        for r in iter {
            set.push(r);
        }
        set
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::language::brill_paper_rule_strings;

    /// Every bundled rule string parses, and every one of them round-trips
    /// through its canonical form. Enumerated over all 313 strings, which is
    /// what makes `RuleSet::bundled`'s `# Panics` note true.
    #[test]
    fn every_bundled_rule_parses() {
        let mut checked = 0;
        for source in Language::English
            .rule_strings()
            .iter()
            .chain(Language::Dutch.rule_strings())
            .chain(brill_paper_rule_strings())
        {
            let rule: Rule = source
                .parse()
                .unwrap_or_else(|e| panic!("{source:?} does not parse: {e}"));
            assert_eq!(
                rule.to_string().parse::<Rule>().unwrap(),
                rule,
                "{source:?} does not round-trip"
            );
            checked += 1;
        }
        assert_eq!(checked, 18 + 285 + 10);
        assert_eq!(RuleSet::bundled(Language::English).len(), 18);
        assert_eq!(RuleSet::bundled(Language::Dutch).len(), 285);
    }

    #[test]
    fn duplicates_are_dropped_and_order_is_kept() {
        let mut rs: RuleSet = "A B PREV-TAG X\nC D NEXT-TAG Y".parse().unwrap();
        assert_eq!(rs.len(), 2);
        assert!(!rs.push("A B PREV-TAG X".parse().unwrap()));
        assert_eq!(rs.len(), 2);
        assert!(rs.push("E F PREV-TAG X".parse().unwrap()));
        assert_eq!(
            rs.rules()
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
            ["A B PREV-TAG X", "C D NEXT-TAG Y", "E F PREV-TAG X"]
        );
    }

    /// Rules whose comma-containing tags would collide under a comma-joined
    /// string key stay distinct under structural identity.
    ///
    /// Both rules below flatten to the same key `A,B,WORD-AND-PREV-TAG,x,y,z`
    /// when their five fields are concatenated with commas, which is how the
    /// pre-migration implementation deduplicated — and Dutch tags such as
    /// `N(soort,mv,neut)` contain commas, so the collision is reachable from
    /// real data rather than only from a contrived one.
    #[test]
    fn comma_bearing_tags_do_not_collide() {
        let mut rs = RuleSet::new();
        assert!(rs.push("A B WORD-AND-PREV-TAG x,y z".parse().unwrap()));
        assert!(rs.push("A B WORD-AND-PREV-TAG x y,z".parse().unwrap()));
        assert_eq!(rs.len(), 2);
        assert_ne!(rs.rules()[0], rs.rules()[1]);
    }

    #[test]
    fn display_round_trips() {
        let rs = RuleSet::bundled(Language::Dutch);
        let text = rs.to_string();
        assert_eq!(text.parse::<RuleSet>().unwrap(), rs);
        assert_eq!(text.lines().count(), 285);
    }

    #[test]
    fn parse_reports_the_offending_line() {
        let err = "A B PREV-TAG X\nA B NOPE X".parse::<RuleSet>().unwrap_err();
        assert_eq!(err.line, 2);
        assert_eq!(
            err.cause,
            RuleParseError::UnknownCondition {
                name: "NOPE".to_owned()
            }
        );
        assert_eq!(err.to_string(), "line 2: unknown condition \"NOPE\"");
    }

    #[test]
    fn blank_lines_are_skipped() {
        let rs: RuleSet = "\nA B PREV-TAG X\n\n   \nC D NEXT-TAG Y\n".parse().unwrap();
        assert_eq!(rs.len(), 2);
    }

    #[test]
    fn context_span_is_the_sum_of_the_rule_reaches() {
        let rs: RuleSet = "A B PREV-1-OR-2-OR-3-TAG X\nC D NEXT-TAG Y"
            .parse()
            .unwrap();
        assert_eq!(rs.context_span(), (3, 1));
        assert_eq!(RuleSet::new().context_span(), (0, 0));
        // Both bundled sets stay small enough to stream comfortably.
        assert_eq!(RuleSet::bundled(Language::English).context_span(), (4, 0));
        let (l, r) = RuleSet::bundled(Language::Dutch).context_span();
        assert!(l > 0 && r > 0, "Dutch reads in both directions");
    }

    #[test]
    fn retain_keeps_membership_consistent() {
        let mut rs: RuleSet = "A B PREV-TAG X\nC D NEXT-TAG Y".parse().unwrap();
        let dropped: Rule = "C D NEXT-TAG Y".parse().unwrap();
        rs.retain(|r| r != &dropped);
        assert_eq!(rs.len(), 1);
        assert!(!rs.contains(&dropped));
        assert!(rs.push(dropped), "the dropped rule can be added again");
    }

    #[test]
    fn empty_set() {
        let rs = RuleSet::new();
        assert!(rs.is_empty());
        assert_eq!(rs.to_string(), "");
        assert_eq!("".parse::<RuleSet>().unwrap(), rs);
    }
}
