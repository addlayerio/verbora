//! An ordered, duplicate-free sequence of transformation rules.

use std::fmt;
use std::str::FromStr;

use rustc_hash::FxHashSet;

use crate::data;
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
/// matters because a [`Tag`](crate::Tag) may contain a comma — morphological tag
/// sets routinely do, as in `N(soort,ev,neut)` — and a comma-joined key really
/// can merge two different rules.
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

    /// The ten transformations of Brill (1992), Table 1 — **written in Brown
    /// corpus tags**.
    ///
    /// # These rules are Brown-tagged, and that is the whole caveat
    ///
    /// They name `AT`, `PPS`, `PPO`, `HVD` and `NP`, which are Brown corpus
    /// tags, not Penn Treebank ones. Pair them with a lexicon whose tags are
    /// Penn (`DT`, `PRP`, `VBD`, `NNP`, …) and almost nothing matches: the
    /// tagger runs, costs a pass per rule, and returns the initial-state
    /// annotation unchanged. That failure is silent — a rule whose condition is
    /// never true is indistinguishable from a rule that never needed to fire —
    /// so it is stated here rather than left to be discovered.
    ///
    /// The thirteen tags the ten rules mention, in full, are `.`, `AT`, `HVD`,
    /// `IN`, `MD`, `NN`, `NP`, `PPO`, `PPS`, `TO`, `VB`, `VBD` and `VBN`. A
    /// lexicon that produces those is one these rules can work on; anything else
    /// wants its own rule set, learned from its own corpus with
    /// [`Trainer`](crate::Trainer).
    ///
    /// # What they are good for
    ///
    /// Three things, all real: a worked example of the rule-string format that
    /// is not invented for the occasion; a published, citable rule set to check
    /// an implementation against; and a starting point for anyone tagging
    /// Brown-annotated text. They are **not** a general-purpose English tagger,
    /// and this crate no longer ships one — see the crate documentation for how
    /// to bring your own lexicon.
    ///
    /// ```
    /// use verbora_tagger::RuleSet;
    ///
    /// let rules = RuleSet::brill_1992();
    /// assert_eq!(rules.len(), 10);
    /// // Brown `AT` (article), not Penn `DT`.
    /// assert_eq!(rules.rules()[0].to_string(), "TO IN NEXT-TAG AT");
    /// ```
    ///
    /// # Provenance
    ///
    /// Eric Brill, *A Simple Rule-Based Part of Speech Tagger*, ANLC '92,
    /// 152–155, Table 1: the first ten transformations his learner acquired from
    /// the Brown corpus. `data/NOTICE.md` records the citation in full.
    ///
    /// # Panics
    ///
    /// Never: all ten rule strings are parsed by
    /// `tests::the_brill_1992_rules_parse_and_round_trip`.
    #[must_use]
    pub fn brill_1992() -> Self {
        Self::parse_lines(data::BRILL_1992_RULES)
            .unwrap_or_else(|e| panic!("the Brill 1992 rule set does not parse: {e}"))
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
    use crate::tag::Tag;

    /// Every Brill 1992 rule string parses, and every one of them round-trips
    /// through its canonical form. Enumerated over all ten, which is what makes
    /// [`RuleSet::brill_1992`]'s `# Panics` note true.
    #[test]
    fn the_brill_1992_rules_parse_and_round_trip() {
        let mut checked = 0;
        for source in data::BRILL_1992_RULES {
            let rule: Rule = source
                .parse()
                .unwrap_or_else(|e| panic!("{source:?} does not parse: {e}"));
            assert_eq!(
                rule.to_string().parse::<Rule>().unwrap(),
                rule,
                "{source:?} does not round-trip"
            );
            // The source spelling is already canonical, so the whole set can be
            // recovered from a `RuleSet` without consulting the JSON.
            assert_eq!(&rule.to_string(), source);
            checked += 1;
        }
        assert_eq!(checked, 10);
        assert_eq!(RuleSet::brill_1992().len(), 10);
        assert_eq!(
            RuleSet::brill_1992().to_string().lines().count(),
            10,
            "no rule was dropped as a duplicate"
        );
    }

    /// The Brill 1992 rules are written in **Brown** tags, and this is the exact
    /// set of them.
    ///
    /// Pinned because [`RuleSet::brill_1992`], `README.md` and `data/NOTICE.md`
    /// all state it, and because it is the one fact that decides whether the set
    /// does anything at all against a given lexicon: a Penn-tagged lexicon
    /// produces `DT`, `PRP` and `NNP`, none of which appear below, so the rules
    /// silently match nothing. A claim that is the difference between "works"
    /// and "no-op" is not left to prose alone.
    #[test]
    fn the_brill_1992_tag_set_is_brown() {
        use crate::rule::TagPattern;
        use std::collections::BTreeSet;

        let set = RuleSet::brill_1992();
        let mut tags: BTreeSet<&str> = BTreeSet::new();
        for rule in set.rules() {
            if let TagPattern::Is(t) = &rule.from {
                tags.insert(t.as_str());
            }
            tags.insert(rule.to.as_str());
            for t in rule.condition.tag_arguments().into_iter().flatten() {
                tags.insert(t.as_str());
            }
        }
        assert_eq!(
            tags.iter().copied().collect::<Vec<_>>(),
            [
                ".", "AT", "HVD", "IN", "MD", "NN", "NP", "PPO", "PPS", "TO", "VB", "VBD", "VBN"
            ]
        );
        // The five that make the set Brown rather than Penn.
        for brown_only in ["AT", "PPS", "PPO", "HVD", "NP"] {
            assert!(tags.contains(brown_only), "{brown_only} is missing");
        }
        // ...and the Penn spellings of the same categories appear nowhere, which
        // is exactly why a Penn lexicon gets nothing out of these rules.
        for penn_only in ["DT", "PRP", "NNP"] {
            assert!(
                !tags.contains(penn_only),
                "{penn_only} unexpectedly present"
            );
        }
    }

    /// No Brill 1992 rule names a token or a suffix — every condition is over
    /// tags or over token *shape*.
    ///
    /// That is what makes the set usable with any Brown-tagged lexicon rather
    /// than only with the Brown corpus itself: nothing in it is keyed to a
    /// vocabulary. It is also why the set survived the removal of this crate's
    /// bundled dictionaries intact, while a lexicalised rule set would not have.
    #[test]
    fn no_brill_1992_rule_is_keyed_to_a_vocabulary() {
        let set = RuleSet::brill_1992();
        for rule in set.rules() {
            assert_eq!(
                rule.condition.word_arguments(),
                [None, None],
                "{rule} names a token"
            );
            assert_eq!(
                rule.condition.suffix_argument(),
                None,
                "{rule} names a suffix"
            );
        }
    }

    /// The reach of the published set, which is what
    /// [`BrillTagger::tag_stream`](crate::BrillTagger::tag_stream) must buffer.
    #[test]
    fn the_brill_1992_set_reads_in_both_directions() {
        let (left, right) = RuleSet::brill_1992().context_span();
        assert!(left > 0 && right > 0, "reads both ways: ({left}, {right})");
    }

    /// A rule set survives `Display` and re-parsing unchanged, including tags
    /// that contain the separator a naive string key would have used.
    #[test]
    fn display_round_trips() {
        let mut rs = RuleSet::brill_1992();
        rs.push(Rule::new(
            crate::rule::TagPattern::Is(Tag::new("N(soort,ev,neut)").unwrap()),
            Tag::new("N(eigen,ev,neut)").unwrap(),
            crate::condition::Condition::CurrentWordIsCapitalized(true),
        ));
        let text = rs.to_string();
        assert_eq!(text.parse::<RuleSet>().unwrap(), rs);
        assert_eq!(text.lines().count(), 11);
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
    /// pre-migration implementation deduplicated. The collision is not contrived:
    /// morphological tag sets spell a tag as a feature bundle — `N(soort,mv,neut)`
    /// — so a comma inside a tag is ordinary, not exotic.
    #[test]
    fn comma_bearing_tags_do_not_collide() {
        let mut rs = RuleSet::new();
        assert!(rs.push("A B WORD-AND-PREV-TAG x,y z".parse().unwrap()));
        assert!(rs.push("A B WORD-AND-PREV-TAG x y,z".parse().unwrap()));
        assert_eq!(rs.len(), 2);
        assert_ne!(rs.rules()[0], rs.rules()[1]);
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
