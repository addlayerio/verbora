//! Ordered, deduplicated collections of transformation rules.
//!
//! The reference stores rules in a plain object keyed by `rule.key()`, so the
//! collection is *insertion-ordered* and *first-wins* on duplicates — and both
//! properties are observable. Rule order decides the outcome, because every rule
//! runs at every position and later rules see earlier rules' edits:
//!
//! ```text
//! (the/DT a/NN b/NN) + ["NN VB PREV-TAG DT", "NN JJ NEXT-TAG NN"] -> DT VB NN
//! (the/DT a/NN b/NN) + ["NN JJ NEXT-TAG NN", "NN VB PREV-TAG DT"] -> DT JJ NN
//! ```
//!
//! No rule key is array-index-like (every key contains commas), so the reference's
//! integer-key hoisting never applies here and object order is exactly insertion
//! order — which is why `RuleSet::new(Language::Dutch).rules()` reproduces
//! `brill_CONTEXTRULES.json` line for line.

use rustc_hash::FxHashMap;

use crate::data;
use crate::parser::SyntaxError;
use crate::rule::TransformationRule;

/// Which bundled data set to load.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Language {
    /// `'EN'` — 18 rules, 92,662 lexicon entries.
    #[default]
    English,
    /// `'DU'` — 285 rules, 11,699 lexicon entries.
    Dutch,
}

impl Language {
    /// Resolves a language code the way `RuleSet`'s `switch` does.
    ///
    /// The switch has **no `default` case** and `data` is pre-initialised to the
    /// English rule set, so every unrecognised code — including `undefined` and
    /// `''` — yields English. `Lexicon`'s switch *does* have a default, and it
    /// selects **Dutch**; the two disagree, which is why
    /// [`Language::for_lexicon`] exists separately.
    #[must_use]
    pub fn for_rule_set(code: Option<&str>) -> Self {
        match code {
            Some("DU") => Self::Dutch,
            _ => Self::English,
        }
    }

    /// Resolves a language code the way `Lexicon`'s `switch` does.
    ///
    /// `'EN'` is English; **everything else**, `undefined` included, is Dutch.
    /// `new Lexicon()` is therefore a Dutch lexicon — which is how
    /// [`crate::Corpus::build_lexicon`] ends up returning one.
    #[must_use]
    pub fn for_lexicon(code: Option<&str>) -> Self {
        match code {
            Some("EN") => Self::English,
            _ => Self::Dutch,
        }
    }

    /// The bundled rule strings for this language.
    #[must_use]
    pub const fn rule_strings(self) -> &'static [&'static str] {
        match self {
            Self::English => data::ENGLISH_RULES,
            Self::Dutch => data::DUTCH_RULES,
        }
    }
}

/// An insertion-ordered set of transformation rules.
#[derive(Debug, Clone, Default)]
pub struct RuleSet {
    rules: Vec<TransformationRule>,
    index: FxHashMap<Box<str>, usize>,
}

impl RuleSet {
    /// An empty rule set.
    ///
    /// # Not `new RuleSet()`
    ///
    /// The reference's zero-argument constructor loads the **English** rules, which
    /// is why `BrillPOSTrainer` seeds all eighteen of them into every rule set it
    /// derives. Use [`RuleSet::for_language`] with
    /// [`Language::for_rule_set`] to reproduce that; this constructor is the
    /// genuinely empty one, which the reference has no way to express.
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    /// Loads a bundled rule set.
    ///
    /// # Panics
    ///
    /// Never, for the bundled data: all 303 rule strings are parsed at start-up
    /// in a debug assertion and in the crate's tests.
    #[must_use]
    pub fn for_language(language: Language) -> Self {
        let mut set = Self::empty();
        for source in language.rule_strings() {
            let rule = TransformationRule::parse(source)
                .unwrap_or_else(|e| panic!("bundled rule {source:?} does not parse: {e}"));
            set.add_rule(rule);
        }
        set
    }

    /// Parses and adds each rule string in order.
    ///
    /// # Errors
    ///
    /// Returns the first syntax error, exactly as the reference constructor
    /// propagates a `peg$SyntaxError` out of itself.
    pub fn from_rule_strings<S: AsRef<str>>(sources: &[S]) -> Result<Self, SyntaxError> {
        let mut set = Self::empty();
        for s in sources {
            set.add_rule(TransformationRule::parse(s.as_ref())?);
        }
        Ok(set)
    }

    /// Adds a rule, returning `false` if its key is already present.
    ///
    /// First-wins: the existing rule keeps its position *and* its training
    /// counters.
    pub fn add_rule(&mut self, rule: TransformationRule) -> bool {
        if self.index.contains_key(rule.key()) {
            return false;
        }
        self.index.insert(rule.key().into(), self.rules.len());
        self.rules.push(rule);
        true
    }

    /// Removes the rule with the same key, if present, preserving the order of
    /// the rest.
    pub fn remove_rule(&mut self, rule: &TransformationRule) {
        self.remove_key(rule.key());
    }

    /// Removes by key.
    pub fn remove_key(&mut self, key: &str) {
        let Some(at) = self.index.remove(key) else {
            return;
        };
        self.rules.remove(at);
        for slot in self.index.values_mut() {
            if *slot > at {
                *slot -= 1;
            }
        }
    }

    /// Keeps only the rules `keep` accepts, preserving order.
    ///
    /// Equivalent to iterating a snapshot and calling [`Self::remove_rule`] on
    /// the rejects — which is what the trainer's pruning step does — but in one
    /// pass instead of one `Vec::remove` per rule.
    pub fn retain(&mut self, mut keep: impl FnMut(&TransformationRule) -> bool) {
        self.rules.retain(|r| keep(r));
        self.reindex();
    }

    fn reindex(&mut self) {
        self.index.clear();
        for (i, r) in self.rules.iter().enumerate() {
            self.index.insert(r.key().into(), i);
        }
    }

    /// Whether a rule with the same key is present.
    #[must_use]
    pub fn has_rule(&self, rule: &TransformationRule) -> bool {
        self.index.contains_key(rule.key())
    }

    /// The rules, in insertion order.
    ///
    /// The reference's `getRules()` materialises a **fresh array on every call**,
    /// and `applyRules` calls it once per token position; returning a borrowed
    /// slice removes O(positions × rules) allocations without changing what is
    /// iterated.
    #[inline]
    #[must_use]
    pub fn rules(&self) -> &[TransformationRule] {
        &self.rules
    }

    /// Mutable access, for the trainer's score counters.
    #[inline]
    pub fn rules_mut(&mut self) -> &mut [TransformationRule] {
        &mut self.rules
    }

    /// Consumes the set, yielding its rules in insertion order.
    #[inline]
    #[must_use]
    pub fn into_rules(self) -> Vec<TransformationRule> {
        self.rules
    }

    /// Looks a rule up by key.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&TransformationRule> {
        self.index.get(key).map(|i| &self.rules[*i])
    }

    /// Mutable lookup by key.
    pub fn get_mut(&mut self, key: &str) -> Option<&mut TransformationRule> {
        let i = *self.index.get(key)?;
        self.rules.get_mut(i)
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

    /// One `prettyPrint()` line per rule, each followed by a newline.
    ///
    /// For the bundled data this round-trips the source JSON strings exactly.
    #[must_use]
    pub fn pretty_print(&self) -> String {
        let mut out = String::new();
        for r in &self.rules {
            out.push_str(&r.to_string());
            out.push('\n');
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_sets_have_the_recorded_sizes() {
        assert_eq!(RuleSet::for_language(Language::English).len(), 18);
        assert_eq!(RuleSet::for_language(Language::Dutch).len(), 285);
    }

    #[test]
    fn unknown_languages_resolve_differently_for_rules_and_lexicons() {
        // The single most surprising asymmetry in this module.
        assert_eq!(Language::for_rule_set(None), Language::English);
        assert_eq!(Language::for_rule_set(Some("FR")), Language::English);
        assert_eq!(Language::for_rule_set(Some("")), Language::English);
        assert_eq!(Language::for_lexicon(None), Language::Dutch);
        assert_eq!(Language::for_lexicon(Some("FR")), Language::Dutch);
        assert_eq!(Language::for_lexicon(Some("EN")), Language::English);
    }

    #[test]
    fn duplicate_keys_are_dropped_first_wins() {
        let mut rs = RuleSet::for_language(Language::English);
        let before = rs.len();
        assert!(!rs.add_rule(TransformationRule::parse("VBD NN PREV-TAG DT").unwrap()));
        assert_eq!(rs.len(), before);
        assert!(rs.add_rule(TransformationRule::parse("QQ ZZ NEXT-TAG DT").unwrap()));
        assert_eq!(rs.len(), before + 1);
    }

    #[test]
    fn removal_preserves_order() {
        let mut rs = RuleSet::from_rule_strings(&["A B P", "C D P", "E F P"]).unwrap();
        rs.remove_key("C,D,P,,");
        let keys: Vec<&str> = rs.rules().iter().map(TransformationRule::key).collect();
        assert_eq!(keys, ["A,B,P,,", "E,F,P,,"]);
        assert!(rs.get("A,B,P,,").is_some());
        assert!(rs.get("E,F,P,,").is_some());
        // Removing something absent is a no-op.
        rs.remove_key("C,D,P,,");
        assert_eq!(rs.len(), 2);
    }

    #[test]
    fn pretty_print_round_trips_the_bundled_rules() {
        let rs = RuleSet::for_language(Language::English);
        let expected: String = data::ENGLISH_RULES
            .iter()
            .map(|r| format!("{r}\n"))
            .collect();
        assert_eq!(rs.pretty_print(), expected);

        let rs = RuleSet::for_language(Language::Dutch);
        let expected: String = data::DUTCH_RULES.iter().map(|r| format!("{r}\n")).collect();
        assert_eq!(rs.pretty_print(), expected);
    }

    #[test]
    fn empty_set() {
        let rs = RuleSet::empty();
        assert!(rs.is_empty());
        assert_eq!(rs.pretty_print(), "");
    }
}
