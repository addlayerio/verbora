//! Learning a rule set from an annotated corpus. The algorithm and its
//! guarantees are documented on [`Trainer`], since this module is private.

use std::num::NonZeroU32;

use rustc_hash::FxHashMap;

use crate::condition::Condition;
use crate::corpus::Corpus;
use crate::lexicon::Lexicon;
use crate::rule::{Rule, TagPattern};
use crate::ruleset::RuleSet;
use crate::tag::{Tag, TaggedToken};
use crate::template::Template;

/// The score threshold [`Trainer::new`] starts from.
///
/// Two rather than one because a rule that fixes exactly one more token than it
/// breaks is, on any realistic corpus, memorising a single site.
const DEFAULT_MIN_SCORE: NonZeroU32 = NonZeroU32::new(2).expect("2 is not zero");

/// One rule the trainer accepted, with the evidence for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrainingStep {
    /// The rule.
    pub rule: Rule,
    /// Tokens it changed from wrong to right, corpus-wide, when it was chosen.
    pub corrections: usize,
    /// Tokens it changed from right to wrong.
    pub errors: usize,
}

impl TrainingStep {
    /// `corrections - errors`, the value the trainer maximised.
    ///
    /// Signed because a proposal's score can be negative; an *accepted* step's
    /// score is always at least the trainer's threshold.
    #[must_use]
    pub fn score(&self) -> i64 {
        self.corrections as i64 - self.errors as i64
    }
}

/// What one training run produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Training {
    rules: RuleSet,
    steps: Vec<TrainingStep>,
    errors_before: usize,
    errors_after: usize,
    tokens: usize,
}

impl Training {
    /// The learned rules, in the order they must be applied.
    #[inline]
    #[must_use]
    pub const fn rules(&self) -> &RuleSet {
        &self.rules
    }

    /// Consumes the result, yielding the rule set.
    #[inline]
    #[must_use]
    pub fn into_rules(self) -> RuleSet {
        self.rules
    }

    /// One entry per accepted rule, in acceptance order, with its score.
    #[inline]
    #[must_use]
    pub fn steps(&self) -> &[TrainingStep] {
        &self.steps
    }

    /// Tokens the initial-state annotator got wrong.
    #[inline]
    #[must_use]
    pub const fn errors_before(&self) -> usize {
        self.errors_before
    }

    /// Tokens still wrong once every learned rule has been applied.
    #[inline]
    #[must_use]
    pub const fn errors_after(&self) -> usize {
        self.errors_after
    }

    /// Tokens in the training corpus.
    #[inline]
    #[must_use]
    pub const fn tokens(&self) -> usize {
        self.tokens
    }
}

/// Learns a [`RuleSet`] from an annotated [`Corpus`].
///
/// # The algorithm
///
/// Transformation-based error-driven learning, as specified in Eric Brill,
/// *Transformation-Based Error-Driven Learning and Natural Language Processing:
/// A Case Study in Part-of-Speech Tagging*, Computational Linguistics 21(4),
/// 1995, §2:
///
/// ```text
/// annotate the corpus with the initial-state annotator (the lexicon)
/// loop
///     for every template, at every site the current tagging gets wrong,
///         propose "rewrite <current tag> as <gold tag> when <condition>"
///     score every proposal over the whole corpus:
///         score = tokens it fixes - tokens it breaks
///     stop when the best score falls below the threshold
///     apply the best-scoring rule to the whole corpus and append it
/// ```
///
/// # What it does not do
///
/// * **It does not touch the corpus.** [`Trainer::train`] takes `&Corpus`; the
///   gold annotation is read and never written. Learning keeps its own working
///   copy, so a corpus can be trained on twice and evaluated against afterwards.
/// * **It does not seed the rule set.** Training starts from no rules at all, so
///   every rule that comes back was learned from the corpus in front of it.
/// * **It is deterministic.** Proposals are enumerated in template order and,
///   within a template, in position order; ties in score go to the proposal
///   enumerated first. The same corpus and lexicon always produce the same
///   rules, on any platform.
///
/// # Termination
///
/// Every accepted rule scores at least [`Trainer::min_score`], which is a
/// [`NonZeroU32`], so every iteration removes at least one error from a finite
/// corpus. Training therefore always terminates, and [`Trainer::max_rules`] is a
/// budget rather than a safety net.
///
/// # Example
///
/// ```
/// use verbora_tagger::{Corpus, Tag, Trainer};
///
/// // "book" is a noun in the lexicon and a verb after "to".
/// let corpus = Corpus::parse_brown("to_TO book_VB\nto_TO fly_VB\na_AT book_NN")?;
/// let mut lexicon = corpus.build_lexicon(Tag::new("NN")?)?;
/// lexicon.insert("book", vec![Tag::new("NN")?])?;
/// lexicon.insert("fly", vec![Tag::new("NN")?])?;
///
/// let training = Trainer::new().train(&corpus, &lexicon);
/// assert_eq!(training.rules().rules()[0].to_string(), "NN VB PREV-TAG TO");
/// assert_eq!(training.steps()[0].score(), 2);
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trainer {
    templates: Vec<Template>,
    min_score: NonZeroU32,
    max_rules: Option<usize>,
}

impl Default for Trainer {
    fn default() -> Self {
        Self::new()
    }
}

impl Trainer {
    /// A trainer over [`Template::ALL`], with a score threshold of 2 and no rule
    /// budget.
    ///
    /// Two rather than one because a rule that fixes exactly one more token than
    /// it breaks is, on any realistic corpus, memorising a single site. Raise or
    /// lower it with [`Trainer::with_min_score`].
    #[must_use]
    pub fn new() -> Self {
        Self {
            templates: Template::ALL.to_vec(),
            min_score: DEFAULT_MIN_SCORE,
            max_rules: None,
        }
    }

    /// Restricts the templates proposals are drawn from.
    ///
    /// [`Template::CONTEXTUAL`] is the right choice for a small corpus; see its
    /// documentation.
    ///
    /// **Repeats are dropped**, keeping the first occurrence of each template,
    /// so [`Trainer::templates`] can be shorter than what was passed in. A
    /// template listed twice would instantiate twice at every site and propose
    /// the same [`Condition`] twice, and the trainer credits a corrected token
    /// once per proposal: one site fixed would report two corrections, clearing
    /// a [`min_score`](Trainer::with_min_score) of 2 that exists precisely to
    /// stop a rule from memorising a single site. A set of templates is what the
    /// algorithm means, so the duplicate is dropped rather than counted.
    #[must_use]
    pub fn with_templates(mut self, templates: impl Into<Vec<Template>>) -> Self {
        self.templates = dedup(templates.into());
        self
    }

    /// Sets the score a rule must reach to be accepted.
    ///
    /// Non-zero by type: a rule with a score of zero removes no errors, so
    /// accepting one would let training run forever.
    #[must_use]
    pub const fn with_min_score(mut self, min_score: NonZeroU32) -> Self {
        self.min_score = min_score;
        self
    }

    /// Caps how many rules are learned. `None` means "until the threshold".
    #[must_use]
    pub const fn with_max_rules(mut self, max_rules: Option<usize>) -> Self {
        self.max_rules = max_rules;
        self
    }

    /// The templates proposals are drawn from, without repeats.
    #[inline]
    #[must_use]
    pub fn templates(&self) -> &[Template] {
        &self.templates
    }

    /// The score threshold.
    #[inline]
    #[must_use]
    pub const fn min_score(&self) -> NonZeroU32 {
        self.min_score
    }

    /// The rule budget.
    #[inline]
    #[must_use]
    pub const fn max_rules(&self) -> Option<usize> {
        self.max_rules
    }

    /// Learns a rule set from `corpus`, starting from `lexicon`'s tagging.
    ///
    /// `corpus` is read only.
    #[must_use]
    pub fn train(&self, corpus: &Corpus<'_>, lexicon: &Lexicon) -> Training {
        // The working tagging: the corpus tokens with the lexicon's guesses.
        let mut working: Vec<Vec<TaggedToken<'_>>> = corpus
            .sentences()
            .iter()
            .map(|s| {
                s.iter()
                    .map(|w| TaggedToken::new(w.token.clone(), lexicon.tag_of(w.token())))
                    .collect()
            })
            .collect();

        let tokens = corpus.token_count();
        let errors_before = count_errors(corpus, &working);
        let mut errors = errors_before;
        let mut rules = RuleSet::new();
        let mut steps = Vec::new();
        let budget = self.max_rules.unwrap_or(usize::MAX);

        let mut conditions = Vec::new();
        let mut sites = Vec::new();
        while steps.len() < budget && errors > 0 {
            let candidates = self.propose(corpus, &working, &mut conditions);
            let Some(best) = candidates.best() else { break };
            if best.score() < i64::from(self.min_score.get()) {
                break;
            }
            if !rules.push(best.rule.clone()) {
                // A rule already learned cannot score again without the corpus
                // changing under it; stopping is the only safe answer.
                break;
            }
            apply(&best.rule, &mut working, &mut sites);
            errors = count_errors(corpus, &working);
            steps.push(best);
        }

        Training {
            rules,
            steps,
            errors_before,
            errors_after: errors,
            tokens,
        }
    }

    /// Builds and scores every candidate rule over the whole corpus.
    fn propose(
        &self,
        corpus: &Corpus<'_>,
        working: &[Vec<TaggedToken<'_>>],
        conditions: &mut Vec<Condition>,
    ) -> Candidates {
        let mut candidates = Candidates::default();
        // Pass one: every site the current tagging gets wrong proposes a rule.
        for (gold_sentence, hyp) in corpus.sentences().iter().zip(working) {
            for (i, (gold, got)) in gold_sentence.iter().zip(hyp).enumerate() {
                if gold.tag == got.tag {
                    continue;
                }
                conditions.clear();
                for template in &self.templates {
                    template.instantiate(hyp, i, conditions);
                }
                for condition in conditions.iter() {
                    candidates.credit(&got.tag, condition, &gold.tag);
                }
            }
        }
        // Pass two, over the *whole* corpus again: every site the tagging
        // already gets right is a site some proposal would break. This has to be
        // a second pass, not the other arm of the first one — a correct site
        // reached before the proposal that breaks it was created would otherwise
        // go uncounted, and the scores would overstate every such rule.
        for (gold_sentence, hyp) in corpus.sentences().iter().zip(working) {
            for (i, (gold, got)) in gold_sentence.iter().zip(hyp).enumerate() {
                if gold.tag != got.tag {
                    continue;
                }
                conditions.clear();
                for template in &self.templates {
                    template.instantiate(hyp, i, conditions);
                }
                for condition in conditions.iter() {
                    candidates.penalise(&got.tag, condition);
                }
            }
        }
        candidates
    }
}

/// Drops repeated templates, keeping the first occurrence of each.
///
/// Quadratic in the *deduplicated* length, which [`Template`] bounds at the
/// number of its variants, so this is linear in what the caller passed.
fn dedup(mut templates: Vec<Template>) -> Vec<Template> {
    let mut kept = 0;
    while kept < templates.len() {
        if templates[..kept].contains(&templates[kept]) {
            templates.remove(kept);
        } else {
            kept += 1;
        }
    }
    templates
}

/// Applies one rule to every sentence, simultaneously within each.
fn apply(rule: &Rule, working: &mut [Vec<TaggedToken<'_>>], sites: &mut Vec<usize>) {
    for sentence in working {
        sites.clear();
        sites.extend((0..sentence.len()).filter(|&i| rule.applies_at(sentence, i)));
        for &i in sites.iter() {
            sentence[i].tag = rule.to.clone();
        }
    }
}

fn count_errors(corpus: &Corpus<'_>, working: &[Vec<TaggedToken<'_>>]) -> usize {
    corpus
        .sentences()
        .iter()
        .zip(working)
        .map(|(gold, hyp)| gold.iter().zip(hyp).filter(|(g, h)| g.tag != h.tag).count())
        .sum()
}

/// Candidate rules, grouped by the `(tag rewritten, condition)` they share.
///
/// Grouping is what keeps scoring linear: every rule in a group is broken by
/// exactly the same correct sites, so one counter per group scores them all.
#[derive(Debug, Default)]
struct Candidates {
    /// Groups, in first-proposal order.
    groups: Vec<Group>,
    index: FxHashMap<(Tag, Condition), usize>,
}

#[derive(Debug)]
struct Group {
    from: Tag,
    condition: Condition,
    /// New tags proposed for this group, in first-proposal order, with how many
    /// tokens each would fix.
    to: Vec<(Tag, usize)>,
    /// Correct tokens any rule in this group would break.
    breaks: usize,
}

impl Candidates {
    fn slot(&mut self, from: &Tag, condition: &Condition) -> usize {
        if let Some(k) = self.index.get(&(from.clone(), condition.clone())) {
            return *k;
        }
        let k = self.groups.len();
        self.groups.push(Group {
            from: from.clone(),
            condition: condition.clone(),
            to: Vec::new(),
            breaks: 0,
        });
        self.index.insert((from.clone(), condition.clone()), k);
        k
    }

    fn credit(&mut self, from: &Tag, condition: &Condition, to: &Tag) {
        let k = self.slot(from, condition);
        let group = &mut self.groups[k];
        match group.to.iter_mut().find(|(t, _)| t == to) {
            Some((_, n)) => *n += 1,
            None => group.to.push((to.clone(), 1)),
        }
    }

    fn penalise(&mut self, from: &Tag, condition: &Condition) {
        // Only a group some site already proposed can be penalised: a group with
        // no proposals has no candidate rule to score.
        if let Some(k) = self.index.get(&(from.clone(), condition.clone())) {
            self.groups[*k].breaks += 1;
        }
    }

    /// The highest-scoring candidate; ties go to the one proposed first.
    fn best(&self) -> Option<TrainingStep> {
        let mut best: Option<TrainingStep> = None;
        for group in &self.groups {
            for (to, corrections) in &group.to {
                let step = TrainingStep {
                    rule: Rule::new(
                        TagPattern::Is(group.from.clone()),
                        to.clone(),
                        group.condition.clone(),
                    ),
                    corrections: *corrections,
                    errors: group.breaks,
                };
                if best.as_ref().is_none_or(|b| step.score() > b.score()) {
                    best = Some(step);
                }
            }
        }
        best
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tagger::BrillTagger;

    fn tag(s: &'static str) -> Tag {
        Tag::new(s).unwrap()
    }

    fn textbook() -> (Corpus<'static>, Lexicon) {
        let corpus = Corpus::parse_brown("to_TO book_VB\nto_TO fly_VB\na_AT book_NN").unwrap();
        let mut lexicon = Lexicon::new(tag("NN"));
        for (w, t) in [("to", "TO"), ("a", "AT"), ("book", "NN"), ("fly", "NN")] {
            lexicon.insert(w, vec![tag(t)]).unwrap();
        }
        (corpus, lexicon)
    }

    /// The textbook transformation. Its score is arithmetic, not a recording:
    /// the lexicon tags `book` and `fly` as `NN`, the corpus says `VB` after
    /// `to` twice and `NN` after `a` once, so `NN VB PREV-TAG TO` fixes two
    /// tokens and breaks none.
    #[test]
    fn learns_the_textbook_rule_with_the_arithmetic_score() {
        let (corpus, lexicon) = textbook();
        let training = Trainer::new()
            .with_templates(Template::CONTEXTUAL)
            .train(&corpus, &lexicon);
        assert_eq!(training.rules().len(), 1);
        assert_eq!(training.rules().rules()[0].to_string(), "NN VB PREV-TAG TO");
        assert_eq!(training.steps()[0].corrections, 2);
        assert_eq!(training.steps()[0].errors, 0);
        assert_eq!(training.steps()[0].score(), 2);
        assert_eq!(training.errors_before(), 2);
        assert_eq!(training.errors_after(), 0);
        assert_eq!(training.tokens(), 6);
    }

    /// One corrected token is one correction, however many times the template
    /// that found it was listed.
    ///
    /// `Template::instantiate` deduplicates only within one template's own
    /// contribution, so a template repeated in [`Trainer::with_templates`] used
    /// to propose the same condition once per repetition and have the same
    /// corrected token credited once per proposal. The corpus below holds
    /// exactly one mis-tagged token, so no rule can honestly score more than 1 —
    /// and with the default threshold of 2 no rule can honestly be learned at
    /// all, which is what the inflated score used to defeat.
    #[test]
    fn a_repeated_template_credits_one_corrected_token_once() {
        let corpus = Corpus::parse_brown("to_TO book_VB").unwrap();
        let mut lexicon = Lexicon::new(tag("NN"));
        lexicon.insert("to", vec![tag("TO")]).unwrap();
        lexicon.insert("book", vec![tag("NN")]).unwrap();
        let repeated = vec![Template::PrevTag, Template::PrevTag, Template::PrevTag];

        // The repeat never reaches the proposal loop.
        let trainer = Trainer::new().with_templates(repeated.clone());
        assert_eq!(trainer.templates(), [Template::PrevTag]);

        let training = trainer
            .with_min_score(NonZeroU32::new(1).expect("1 is not zero"))
            .train(&corpus, &lexicon);
        assert_eq!(training.rules().rules()[0].to_string(), "NN VB PREV-TAG TO");
        assert_eq!(training.steps()[0].corrections, 1);
        assert_eq!(training.steps()[0].errors, 0);
        assert_eq!(training.steps()[0].score(), 1);

        // At the default threshold the single site is rejected, exactly as it is
        // when the template is listed once.
        for templates in [vec![Template::PrevTag], repeated] {
            let training = Trainer::new()
                .with_templates(templates)
                .train(&corpus, &lexicon);
            assert!(training.rules().is_empty());
        }
    }

    /// The learned rules must actually improve the tagger they were learned for.
    #[test]
    fn learned_rules_reduce_the_error_count_they_claim_to() {
        let (corpus, lexicon) = textbook();
        let training = Trainer::new().train(&corpus, &lexicon);
        let before = BrillTagger::new(&lexicon, &RuleSet::new()).evaluate(&corpus);
        let after = BrillTagger::new(&lexicon, training.rules()).evaluate(&corpus);
        assert_eq!(
            before.tokens - before.correct_after_rules,
            training.errors_before()
        );
        assert_eq!(
            after.tokens - after.correct_after_rules,
            training.errors_after()
        );
        assert!(after.accuracy() > before.accuracy());
    }

    /// Training starts from nothing: a corpus with no errors yields no rules,
    /// and no rule appears that the corpus did not justify.
    #[test]
    fn training_starts_from_an_empty_rule_set() {
        let corpus = Corpus::parse_brown("to_TO book_NN").unwrap();
        let lexicon = corpus.build_lexicon(tag("NN")).unwrap();
        let training = Trainer::new().train(&corpus, &lexicon);
        assert!(training.rules().is_empty());
        assert_eq!(training.errors_before(), 0);
        assert_eq!(training.steps(), &[]);
    }

    #[test]
    fn an_empty_corpus_trains_to_nothing() {
        let lexicon = Lexicon::new(tag("NN"));
        let training = Trainer::new().train(&Corpus::new(), &lexicon);
        assert!(training.rules().is_empty());
        assert_eq!(training.tokens(), 0);
        assert_eq!(training.errors_before(), 0);
    }

    /// The corpus is read-only: the gold annotation survives training intact.
    #[test]
    fn training_does_not_modify_the_corpus() {
        let (corpus, lexicon) = textbook();
        let before = corpus.clone();
        let _ = Trainer::new().train(&corpus, &lexicon);
        assert_eq!(corpus, before);
    }

    #[test]
    fn the_threshold_and_the_budget_are_honoured() {
        let (corpus, lexicon) = textbook();
        let high = Trainer::new()
            .with_min_score(NonZeroU32::new(3).unwrap())
            .train(&corpus, &lexicon);
        assert!(high.rules().is_empty(), "nothing scores 3 on this corpus");

        let capped = Trainer::new()
            .with_max_rules(Some(0))
            .train(&corpus, &lexicon);
        assert!(capped.rules().is_empty());
    }

    /// Two runs over the same input produce byte-identical rule sets.
    #[test]
    fn training_is_deterministic() {
        let text = "\
The_AT dog_NN runs_VBZ\n\
The_AT cat_NN sleeps_VBZ\n\
to_TO book_VB a_AT flight_NN\n\
to_TO fly_VB a_AT kite_NN\n\
a_AT book_NN is_BEZ good_JJ\n";
        let corpus = Corpus::parse_brown(text).unwrap();
        let mut lexicon = corpus.build_lexicon(tag("NN")).unwrap();
        lexicon.insert("book", vec![tag("NN")]).unwrap();
        lexicon.insert("fly", vec![tag("NN")]).unwrap();
        let a = Trainer::new().train(&corpus, &lexicon);
        let b = Trainer::new().train(&corpus, &lexicon);
        assert_eq!(a, b);
        assert_eq!(a.rules().to_string(), b.rules().to_string());
    }

    /// Every learned rule can be written out and read back, so a trained model
    /// survives a round trip through text.
    #[test]
    fn learned_rules_round_trip_through_text() {
        let text = "\
to_TO book_VB a_AT flight_NN\n\
to_TO fly_VB a_AT kite_NN\n\
a_AT book_NN is_BEZ good_JJ\n\
the_AT running_VBG dogs_NNS bark_VB\n";
        let corpus = Corpus::parse_brown(text).unwrap();
        let mut lexicon = corpus.build_lexicon(tag("NN")).unwrap();
        for w in ["book", "fly", "running", "dogs"] {
            lexicon.insert(w, vec![tag("NN")]).unwrap();
        }
        let training = Trainer::new().train(&corpus, &lexicon);
        assert!(!training.rules().is_empty(), "something must be learned");
        let text = training.rules().to_string();
        assert_eq!(&text.parse::<RuleSet>().unwrap(), training.rules());
    }

    /// Every accepted step's recorded score is at least the threshold, and the
    /// error count falls by at least that much. Enumerated over every step.
    #[test]
    fn every_step_pays_for_itself() {
        let text = "\
to_TO book_VB a_AT flight_NN\n\
to_TO fly_VB a_AT kite_NN\n\
a_AT book_NN is_BEZ good_JJ\n\
the_AT running_VBG dogs_NNS bark_VB\n\
the_AT walking_VBG cats_NNS purr_VB\n";
        let corpus = Corpus::parse_brown(text).unwrap();
        let mut lexicon = corpus.build_lexicon(tag("NN")).unwrap();
        for w in ["book", "fly", "running", "walking", "dogs", "cats"] {
            lexicon.insert(w, vec![tag("NN")]).unwrap();
        }
        let training = Trainer::new().train(&corpus, &lexicon);
        assert!(!training.steps().is_empty());
        let mut claimed = 0i64;
        for step in training.steps() {
            assert!(step.score() >= 2, "{} scored {}", step.rule, step.score());
            claimed += step.score();
        }
        assert_eq!(
            training.errors_before() as i64 - training.errors_after() as i64,
            claimed,
            "the recorded scores must add up to the errors actually removed"
        );
    }
}
