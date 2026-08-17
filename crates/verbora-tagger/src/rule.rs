//! Transformation rules: "rewrite tag *A* as *B* when *predicate* holds".

use std::borrow::Cow;
use std::fmt;

use crate::error::TaggerError;
use crate::parser::{self, ParsedRule, SyntaxError};
use crate::sentence::{Prop, Sentence, Tag, TaggedWord};
use crate::templates::{self, PredValue, PredicateKind};

/// The old-category value that matches any tag.
pub const CATEGORY_WILDCARD: &str = "*";

/// A predicate bound to its arguments.
///
/// The template lookup happens here, once, rather than on every evaluation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Predicate {
    /// The name as written in the rule.
    pub name: Box<str>,
    /// The resolved template. See [`PredicateKind::PrototypeLeak`] for the case
    /// where a name resolves to something that is not a predicate at all.
    pub kind: PredicateKind,
    /// First argument. Stored regardless of the template's declared arity — the
    /// reference's arity guards are commented out, and two templates depend on
    /// that.
    pub parameter1: Option<Tag>,
    /// Second argument, likewise always stored.
    pub parameter2: Option<Tag>,
}

impl Predicate {
    /// Binds `name` to its arguments, resolving the template.
    #[must_use]
    pub fn new(name: &str, parameter1: Option<Tag>, parameter2: Option<Tag>) -> Self {
        Self {
            name: name.into(),
            kind: templates::resolve(name),
            parameter1,
            parameter2,
        }
    }

    /// Evaluates the predicate against `words` at `position`.
    ///
    /// # Errors
    ///
    /// Propagates the `TypeError` the predicate function throws; see
    /// [`templates::evaluate`].
    #[inline]
    pub fn evaluate(
        &self,
        words: &[TaggedWord<'_>],
        position: isize,
    ) -> Result<PredValue, TaggerError> {
        templates::evaluate(
            self.kind,
            words,
            position,
            self.parameter1.as_deref(),
            self.parameter2.as_deref(),
        )
    }
}

/// A Brill transformation rule.
#[derive(Debug, Clone)]
pub struct TransformationRule {
    /// The tag the rule replaces. `Some("*")` is the wildcard; `None` is the
    /// `undefined` the trainer produces from an untagged token.
    pub old_category: Option<Tag>,
    /// The tag the rule assigns.
    pub new_category: Option<Tag>,
    /// The condition.
    pub predicate: Predicate,
    /// `literal.toString()`, computed once at construction.
    key: Box<str>,
    /// Training counter: sites where the rule changed nothing that mattered.
    pub neutral: u32,
    /// Training counter: sites where the rule made the tagging worse.
    pub negative: u32,
    /// Training counter: sites where the rule fixed a wrong tag.
    pub positive: u32,
    /// Whether the trainer has already picked this rule as the highest scoring
    /// one. Reset to `false` whenever the score changes.
    pub has_been_selected_as_high_rule_before: bool,
}

impl PartialEq for TransformationRule {
    /// Rules compare by identity of their five-field literal, which is exactly
    /// what [`Self::key`] encodes; training counters are excluded.
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
    }
}
impl Eq for TransformationRule {}

impl TransformationRule {
    /// Builds a rule from its five literal fields.
    #[must_use]
    pub fn new(
        c1: Option<Tag>,
        c2: Option<Tag>,
        predicate: &str,
        parameter1: Option<Tag>,
        parameter2: Option<Tag>,
    ) -> Self {
        let key = build_key(
            c1.as_deref(),
            c2.as_deref(),
            predicate,
            parameter1.as_deref(),
            parameter2.as_deref(),
        );
        Self {
            old_category: c1,
            new_category: c2,
            predicate: Predicate::new(predicate, parameter1, parameter2),
            key,
            neutral: 0,
            negative: 0,
            positive: 0,
            has_been_selected_as_high_rule_before: false,
        }
    }

    /// Parses a rule string with the `TF_Parser` grammar.
    ///
    /// # Errors
    ///
    /// Returns the PEG syntax error for input the grammar rejects.
    pub fn parse(source: &str) -> Result<Self, SyntaxError> {
        let ParsedRule {
            category1,
            category2,
            predicate,
            parameter1,
            parameter2,
        } = parser::parse(source)?;
        Ok(Self::new(
            Some(Cow::Owned(category1)),
            Some(Cow::Owned(category2)),
            &predicate,
            parameter1.map(Cow::Owned),
            parameter2.map(Cow::Owned),
        ))
    }

    /// `literal.toString()` — the identity a [`crate::RuleSet`] deduplicates on.
    ///
    /// This is `Array.prototype.join(',')`, in which `undefined` renders as the
    /// empty string. A one-parameter rule therefore ends in a **trailing comma**
    /// (`"VBD,NN,PREV-TAG,DT,"`) and a zero-parameter rule in two. Because Dutch
    /// categories contain commas themselves, two structurally different rules can
    /// in principle collide on one key; none of the 285 bundled Dutch rules do.
    #[inline]
    #[must_use]
    pub fn key(&self) -> &str {
        &self.key
    }

    /// `positive - negative`. Can be negative, hence the signed type.
    #[inline]
    #[must_use]
    pub fn score(&self) -> i64 {
        i64::from(self.positive) - i64::from(self.negative)
    }

    /// Applies the rule at `position`, mutating the tag in place.
    ///
    /// The wildcard old-category `*` matches **any** tag, including `undefined`.
    /// The predicate's result is used for *truthiness*, not compared with `true`:
    /// `currentWordIsNumber` can return a number and `nextWordIs` can return
    /// `undefined`.
    ///
    /// # Errors
    ///
    /// Propagates predicate errors, and reports reading `tag` off `undefined`
    /// when `position` is outside the sentence.
    pub fn apply(&self, words: &mut [TaggedWord<'_>], position: isize) -> Result<(), TaggerError> {
        let idx = usize::try_from(position).ok().filter(|i| *i < words.len());
        let Some(idx) = idx else {
            return Err(TaggerError::undefined("tag"));
        };
        let matches = words[idx].tag() == self.old_category.as_deref()
            || self.old_category.as_deref() == Some(CATEGORY_WILDCARD);
        if matches && self.predicate.evaluate(words, position)?.is_truthy() {
            // An assignment, so the property exists afterwards even when the new
            // category is `undefined`.
            words[idx].tag = Prop::assigned(self.new_category.clone());
        }
        Ok(())
    }

    /// Whether the rule applies at a training site, recording `newTag` if it does.
    ///
    /// Two things differ from [`Self::apply`], and both are load-bearing:
    ///
    /// * the `*` **wildcard is not honoured** here — the comparison is a plain
    ///   `===` — so a wildcard rule can never be applicable during training;
    /// * the predicate reads `hypothesis` (the current guess) while the side
    ///   effect is written to `gold` (the corpus sentence).
    ///
    /// The returned value is whatever `&&` produced, so it can be a number or
    /// `undefined` rather than a boolean; call
    /// [`PredValue::is_truthy`] to branch on it.
    ///
    /// # Errors
    ///
    /// Propagates predicate errors and out-of-range reads.
    pub fn is_applicable_at(
        &self,
        gold: &mut [TaggedWord<'_>],
        hypothesis: &[TaggedWord<'_>],
        i: isize,
    ) -> Result<PredValue, TaggerError> {
        let idx = usize::try_from(i).ok().filter(|k| *k < hypothesis.len());
        let Some(idx) = idx else {
            return Err(TaggerError::undefined("tag"));
        };
        if hypothesis[idx].tag() != self.old_category.as_deref() {
            // `&&` short-circuits: the predicate is not evaluated at all.
            return Ok(PredValue::Bool(false));
        }
        let applies = self.predicate.evaluate(hypothesis, i)?;
        if applies.is_truthy() {
            let Some(slot) = gold.get_mut(idx) else {
                return Err(TaggerError::undefined("newTag"));
            };
            slot.new_tag = Prop::assigned(self.new_category.clone());
        }
        Ok(applies)
    }

    /// Applies the rule to a corpus sentence and records what the tag *was*.
    ///
    /// The order is the whole point: the clone captures the pre-apply state,
    /// [`Self::apply`] then overwrites `tag` — which in the trainer's
    /// representation is the **gold** tag — and only afterwards is the old gold
    /// tag copied into `testTag`. So `(to/TO, book/NN testTag=NN)` under
    /// `NN VB PREV-TAG TO` becomes `{token: "book", tag: "VB", testTag: "NN"}`:
    /// the corpus annotation has been corrupted, on purpose, and the trainer's
    /// subsequent scoring depends on it.
    ///
    /// # Errors
    ///
    /// Propagates predicate errors and out-of-range reads.
    pub fn apply_at(&self, sentence: &mut Sentence<'_>, i: isize) -> Result<(), TaggerError> {
        let before = sentence.clone_tags_only();
        self.apply(&mut sentence.tagged_words, i)?;
        let idx = usize::try_from(i).expect("apply would have failed for a negative index");
        sentence.tagged_words[idx].test_tag = Prop::assigned(before.tagged_words[idx].tag.owned());
        Ok(())
    }
}

impl fmt::Display for TransformationRule {
    /// `prettyPrint()`: `old new predicate [p1 [p2]]`.
    ///
    /// The parameter guards are *truthiness* tests, so an empty-string parameter
    /// 1 suppresses **both** parameters. An absent category renders as the
    /// literal text `undefined`, because the reference is concatenating it into a
    /// string.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} {} {}",
            self.old_category.as_deref().unwrap_or("undefined"),
            self.new_category.as_deref().unwrap_or("undefined"),
            self.predicate.name
        )?;
        if let Some(p1) = self
            .predicate
            .parameter1
            .as_deref()
            .filter(|s| !s.is_empty())
        {
            write!(f, " {p1}")?;
            if let Some(p2) = self
                .predicate
                .parameter2
                .as_deref()
                .filter(|s| !s.is_empty())
            {
                write!(f, " {p2}")?;
            }
        }
        Ok(())
    }
}

/// `[c1, c2, pred, p1, p2].toString()`.
fn build_key(
    c1: Option<&str>,
    c2: Option<&str>,
    pred: &str,
    p1: Option<&str>,
    p2: Option<&str>,
) -> Box<str> {
    let parts = [
        c1.unwrap_or(""),
        c2.unwrap_or(""),
        pred,
        p1.unwrap_or(""),
        p2.unwrap_or(""),
    ];
    let mut s = String::with_capacity(parts.iter().map(|p| p.len() + 1).sum());
    for (i, p) in parts.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        s.push_str(p);
    }
    s.into_boxed_str()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tag(s: &'static str) -> Option<Tag> {
        Some(Cow::Borrowed(s))
    }

    fn sentence(pairs: &[(&'static str, Option<&'static str>)]) -> Sentence<'static> {
        Sentence::from_words(
            pairs
                .iter()
                .map(|(t, g)| TaggedWord::new(*t, g.map(Cow::Borrowed)))
                .collect(),
        )
    }

    #[test]
    fn keys_carry_trailing_commas() {
        assert_eq!(
            TransformationRule::parse("VBD NN PREV-TAG DT")
                .unwrap()
                .key(),
            "VBD,NN,PREV-TAG,DT,"
        );
        assert_eq!(TransformationRule::parse("A B P").unwrap().key(), "A,B,P,,");
        assert_eq!(
            TransformationRule::parse("A B P x y").unwrap().key(),
            "A,B,P,x,y"
        );
        // Undefined categories render as the empty string in the key...
        let r = TransformationRule::new(None, tag("NN"), "NEXT-TAG", tag("DT"), None);
        assert_eq!(r.key(), ",NN,NEXT-TAG,DT,");
        // ...but as the text "undefined" in prettyPrint, because that is string
        // concatenation rather than Array#join.
        assert_eq!(r.to_string(), "undefined NN NEXT-TAG DT");
    }

    #[test]
    fn pretty_print_parameter_guards_are_truthiness() {
        let r = TransformationRule::new(tag("A"), tag("B"), "P", tag(""), tag("y"));
        assert_eq!(r.to_string(), "A B P", "empty p1 suppresses both");
        let r = TransformationRule::new(tag("A"), tag("B"), "P", tag("x"), tag(""));
        assert_eq!(r.to_string(), "A B P x");
    }

    #[test]
    fn wildcard_applies_but_is_never_applicable() {
        let rule = TransformationRule::parse("* RB CURRENT-WORD-ENDS-WITH ly").unwrap();
        let mut s = sentence(&[("quickly", Some("NN"))]);
        rule.apply(&mut s.tagged_words, 0).unwrap();
        assert_eq!(s.tagged_words[0].tag(), Some("RB"));

        // Same rule, same sentence, through the training entry point: the
        // wildcard is compared with `===` there and cannot match "NN".
        let mut gold = sentence(&[("quickly", Some("NN"))]);
        let hyp = sentence(&[("quickly", Some("NN"))]);
        let applies = rule
            .is_applicable_at(&mut gold.tagged_words, &hyp.tagged_words, 0)
            .unwrap();
        assert!(!applies.is_truthy());
        assert_eq!(gold.tagged_words[0].new_tag, Prop::Absent);
    }

    #[test]
    fn wildcard_matches_an_undefined_tag() {
        let rule = TransformationRule::parse("* RB CURRENT-WORD-ENDS-WITH ly").unwrap();
        let mut s = sentence(&[("quickly", None)]);
        rule.apply(&mut s.tagged_words, 0).unwrap();
        assert_eq!(s.tagged_words[0].tag(), Some("RB"));
    }

    #[test]
    fn apply_at_corrupts_the_gold_tag() {
        let rule = TransformationRule::parse("NN VB PREV-TAG TO").unwrap();
        let mut s = sentence(&[("to", Some("TO")), ("book", Some("NN"))]);
        s.tagged_words[1].test_tag = Prop::Set(Cow::Borrowed("NN"));
        rule.apply_at(&mut s, 1).unwrap();
        assert_eq!(s.tagged_words[1].tag(), Some("VB"));
        assert_eq!(s.tagged_words[1].test_tag, Prop::Set(Cow::Borrowed("NN")));
    }

    #[test]
    fn out_of_range_positions_throw() {
        let rule = TransformationRule::parse("A B P").unwrap();
        let mut s = sentence(&[("x", None)]);
        assert_eq!(
            rule.apply(&mut s.tagged_words, -1),
            Err(TaggerError::undefined("tag"))
        );
        assert_eq!(
            rule.apply(&mut s.tagged_words, 9),
            Err(TaggerError::undefined("tag"))
        );
    }

    #[test]
    fn score_starts_at_zero_and_can_go_negative() {
        let mut r = TransformationRule::parse("A B P").unwrap();
        assert_eq!(r.score(), 0);
        r.negative = 3;
        assert_eq!(r.score(), -3);
    }
}
