//! Learning transformation rules from a tagged corpus.
//!
//! The algorithm is Ramshaw & Marcus's incremental variant of Brill's learner:
//! tag the corpus with the lexicon, propose every rule that would fix a wrong
//! tag, score each proposal over the whole corpus, then repeatedly apply the
//! highest-scoring rule and re-score its neighbourhood.
//!
//! # Three parts of the reference do not do what they say
//!
//! Each one changes the output, so each one is reproduced.
//!
//! ## 1. `neighbourhood()` always returns the empty list
//!
//! ```text
//! neighbourhood (i, j) {
//!   const sentenceLength = this.corpus.sentences[i].length  // Sentence has no .length
//!   if (this.index > 2) { list.push([i, j - 3]) }           // this.index is never assigned
//!   …
//!   if (this.index < sentenceLength - 1) { … }              // undefined < NaN
//! }
//! ```
//!
//! `this.index` is written nowhere in the class and `Sentence` has no `length`
//! property, so every guard is `undefined > 2` or `undefined < NaN` — all false.
//! The incremental half of the algorithm is therefore **dead code**: no rule is
//! ever re-scored, no site is ever disconnected, and no rule is discovered after
//! the initial scan. The loop degenerates into "walk the rules in descending
//! score order, applying each one wherever it was found to apply".
//!
//! A port that fixed `neighbourhood` would produce a *better* tagger and a
//! completely different rule set. `mapSiteToRules`, whose only readers are
//! `getRules` and `siteIsAssociatedWithRule`, is likewise reachable only from
//! that dead branch, so this implementation does not build it — an observation,
//! not a shortcut: nothing in the exported surface can read it.
//!
//! ## 2. `new RuleSet()` is not an empty rule set
//!
//! `RuleSet`'s constructor pre-initialises `data` to the English rule file and
//! its `switch` has no `default`, so the zero-argument call loads all **18
//! English rules**. The trainer starts `positiveRules` that way, which is why a
//! run over a Brown-tagged corpus returns rules like `NN VBG
//! CURRENT-WORD-ENDS-WITH ing` that no template proposed, and why a training run
//! with a threshold of `-1` returns 50 rules where 32 were learned.
//!
//! ## 3. Scoring reads a tag the rule itself just overwrote
//!
//! `isApplicableAt` writes `newTag` onto the **corpus** sentence while reading
//! the hypothesis, and `scoreRule` then compares `tag`, `testTag` and `newTag` on
//! that same corpus token. `applyAt` later overwrites the corpus `tag` and copies
//! the pre-application value into `testTag`. The gold annotation is progressively
//! corrupted, and the fixture records the corrupted corpus.
//!
//! # A deliberate deviation, with an argument
//!
//! `generatePositiveRules` builds its per-site `new RuleSet()` — those 18 English
//! rules again — for every token of every sentence, then hands the union to
//! `positiveRules`. This implementation starts that per-site set **empty**.
//!
//! The result is identical, and provably so: `positiveRules` already contains all
//! 18 keys before the scan begins, so a generated rule that collides with an
//! English key is rejected either way — by the per-site set in the reference, by
//! `positiveRules` here — and the surviving object is the English rule in both.
//! The only other reader of the per-site set is `newRules.hasRule`, inside the
//! dead branch of item 1. Skipping the seeding avoids constructing 18 rules per
//! token (25,578 of them on the Brown excerpt) that are all immediately
//! discarded.
//!
//! The argument was also checked rather than trusted: seeding the per-site set
//! with the English rules and re-running `tests/parity.rs` reproduces all 32,306
//! recorded cases unchanged.

use std::collections::{BTreeMap, BTreeSet};

use rustc_hash::FxHashMap;

use crate::corpus::Corpus;
use crate::error::TaggerError;
use crate::lexicon::Lexicon;
use crate::rule::TransformationRule;
use crate::ruleset::{Language, RuleSet};
use crate::sentence::{Sentence, Tag, TaggedWord};
use crate::templates::{self, RuleTemplate};

/// Training stops once no unselected rule scores above this.
const MIN_SCORE: i64 = 0;

/// Sites at which one rule was found applicable: sentence index → token indices.
///
/// `BTreeMap`/`BTreeSet` rather than hashes because `getSites` reads
/// `Object.keys` of an object whose keys are integers, which the reference
/// enumerates in **ascending numeric order** — and the order decides the
/// sequence of `applyAt` calls, which decides how the corpus ends up corrupted.
type Sites = BTreeMap<usize, BTreeSet<usize>>;

/// Learns a rule set from an annotated corpus.
#[derive(Debug, Clone)]
pub struct BrillPosTrainer {
    /// Rules scoring below this are pruned after training.
    ///
    /// `new BrillPOSTrainer(t)` stores `t` only when it is **truthy**, so `0`
    /// becomes `1` while `-1` stays `-1`. See [`BrillPosTrainer::new`].
    pub rule_score_threshold: i64,
    positive_rules: RuleSet,
    sites: FxHashMap<Box<str>, Sites>,
}

impl Default for BrillPosTrainer {
    /// `new BrillPOSTrainer()` — threshold 1.
    fn default() -> Self {
        Self::new(0)
    }
}

impl BrillPosTrainer {
    /// `new BrillPOSTrainer(ruleScoreThreshold)`.
    ///
    /// The guard is `if (ruleScoreThreshold)`, a truthiness test, so a threshold
    /// of **`0` is silently replaced by `1`** while `-1` is kept. Passing `0` and
    /// passing nothing are the same request.
    #[must_use]
    pub fn new(rule_score_threshold: i64) -> Self {
        Self {
            rule_score_threshold: if rule_score_threshold == 0 {
                1
            } else {
                rule_score_threshold
            },
            positive_rules: RuleSet::empty(),
            sites: FxHashMap::default(),
        }
    }

    /// Trains on `corpus`, using `templates` to propose rules and `lexicon` for
    /// the initial tagging.
    ///
    /// `corpus` is **rewritten in place**: `testTag` and `newTag` are filled in,
    /// and the gold `tag` of every site the winning rules touch is overwritten
    /// (see item 3 in the module documentation). Pass a fresh corpus per run.
    ///
    /// The returned rule set is a snapshot taken after pruning;
    /// [`BrillPosTrainer::print_rules_with_scores`] reports on the same rules
    /// with their scores.
    ///
    /// # Errors
    ///
    /// Nine of the 36 templates cannot be trained at all: their parameter-value
    /// helpers index the `Sentence` object instead of its `taggedWords` array and
    /// throw `Cannot read properties of undefined (reading 'token')`. Predicate
    /// errors propagate too — an empty token under `CURRENT-WORD-IS-CAP`, say.
    pub fn train(
        &mut self,
        corpus: &mut Corpus<'_>,
        templates: &[RuleTemplate],
        lexicon: &Lexicon,
    ) -> Result<RuleSet, TaggerError> {
        // `new RuleSet()` loads the English rules; see item 2 above.
        self.positive_rules = RuleSet::for_language(Language::English);
        self.sites.clear();

        corpus.tag(lexicon);
        self.scan_for_positive_rules(corpus, templates)?;
        self.scan_for_sites(corpus)?;

        while let Some(idx) = self.select_high_rule() {
            if self.positive_rules.rules()[idx].score() <= MIN_SCORE {
                break;
            }
            let rule = &self.positive_rules.rules()[idx];
            if let Some(sites) = self.sites.get(rule.key()) {
                for (&i, tokens) in sites {
                    for &j in tokens {
                        rule.apply_at(&mut corpus.sentences[i], j as isize)?;
                    }
                }
            }
            // No re-scoring and no rediscovery: `neighbourhood` is empty.
        }

        let threshold = self.rule_score_threshold;
        self.positive_rules.retain(|r| r.score() >= threshold);
        Ok(self.positive_rules.clone())
    }

    /// The rules learned so far, with their live training counters.
    #[inline]
    #[must_use]
    pub fn positive_rules(&self) -> &RuleSet {
        &self.positive_rules
    }

    /// `printRulesWithScores()`: `score`, `positive`, `negative`, `neutral` and
    /// the rule, tab-separated, one line per rule, highest score first.
    ///
    /// The sort is stable and the comparator returns 0 for equal scores, so rules
    /// that tie keep their insertion order — which for a trained set is discovery
    /// order. `Vec::sort_by`, never `sort_unstable_by`.
    #[must_use]
    pub fn print_rules_with_scores(&self) -> String {
        let mut rules: Vec<&TransformationRule> = self.positive_rules.rules().iter().collect();
        rules.sort_by_key(|r| std::cmp::Reverse(r.score()));
        let mut out = String::new();
        for r in rules {
            out.push_str(&format!(
                "{}\t{}\t{}\t{}\t{r}\n",
                r.score(),
                r.positive,
                r.negative,
                r.neutral
            ));
        }
        out
    }

    /// The sites `rule` was found applicable at, as `(sentence, token)` pairs in
    /// ascending order.
    ///
    /// Empty for a rule that was never applicable. The reference instead throws
    /// `Cannot convert undefined or null to object` there, because it calls
    /// `Object.keys` on a missing map entry — unreachable, since only a rule with
    /// a positive score reaches this call and a positive score implies a site.
    #[must_use]
    pub fn sites_of(&self, rule: &TransformationRule) -> Vec<(usize, usize)> {
        self.sites
            .get(rule.key())
            .map(|sites| {
                sites
                    .iter()
                    .flat_map(|(&i, tokens)| tokens.iter().map(move |&j| (i, j)))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// `selectHighRule()`: the index of the highest-scoring rule not yet chosen.
    ///
    /// Marks the winner as chosen — including when its score is too low to be
    /// used, which is what stops the caller's loop from spinning forever. Ties go
    /// to the earliest rule, because the upgrade test is a strict `>`.
    fn select_high_rule(&mut self) -> Option<usize> {
        let rules = self.positive_rules.rules();
        let mut best: Option<usize> = None;
        for (i, rule) in rules.iter().enumerate() {
            if rule.has_been_selected_as_high_rule_before {
                continue;
            }
            match best {
                None => best = Some(i),
                Some(b) if rule.score() > rules[b].score() => best = Some(i),
                Some(_) => {}
            }
        }
        if let Some(b) = best {
            self.positive_rules.rules_mut()[b].has_been_selected_as_high_rule_before = true;
        }
        best
    }

    /// `scanForPositiveRules()`: propose a rule for every mis-tagged token.
    fn scan_for_positive_rules(
        &mut self,
        corpus: &Corpus<'_>,
        templates: &[RuleTemplate],
    ) -> Result<(), TaggerError> {
        let mut proposals = Vec::new();
        for sentence in &corpus.sentences {
            for j in 0..sentence.len() {
                proposals.clear();
                generate_positive_rules(sentence, j, templates, &mut proposals)?;
                for rule in proposals.drain(..) {
                    self.positive_rules.add_rule(rule);
                }
            }
        }
        Ok(())
    }

    /// `scanForSites()`: find and score every (site, rule) pair.
    fn scan_for_sites(&mut self, corpus: &mut Corpus<'_>) -> Result<(), TaggerError> {
        for (i, sentence) in corpus.sentences.iter_mut().enumerate() {
            // The hypothesis is a *separate* sentence carrying the lexicon's
            // guesses; predicates read it while side effects land on the gold one.
            let hypothesis: Vec<TaggedWord<'_>> = sentence
                .tagged_words
                .iter()
                .map(|w| TaggedWord::new(w.token.clone(), w.test_tag.owned()))
                .collect();

            for j in 0..sentence.tagged_words.len() {
                for k in 0..self.positive_rules.len() {
                    let applies = self.positive_rules.rules()[k].is_applicable_at(
                        &mut sentence.tagged_words,
                        &hypothesis,
                        j as isize,
                    )?;
                    if !applies.is_truthy() {
                        continue;
                    }
                    let token = &sentence.tagged_words[j];
                    let (right, old, new) = (
                        token.tag.owned(),
                        token.test_tag.owned(),
                        token.new_tag.owned(),
                    );
                    let key: Box<str> = self.positive_rules.rules()[k].key().into();
                    self.sites
                        .entry(key)
                        .or_default()
                        .entry(i)
                        .or_default()
                        .insert(j);
                    score_rule(
                        &mut self.positive_rules.rules_mut()[k],
                        right.as_deref(),
                        old.as_deref(),
                        new.as_deref(),
                    );
                }
            }
        }
        Ok(())
    }
}

/// `scoreRule()`: one rule's contribution at one site.
///
/// `newTag` is whatever the *last* applicable rule wrote onto this token, which
/// — because the caller scores immediately after `isApplicableAt` — is this
/// rule's own new category. Both branches that change the score also clear
/// `hasBeenSelectedAsHighRuleBefore`, so a rule can be selected again once its
/// score moves.
fn score_rule(
    rule: &mut TransformationRule,
    right: Option<&str>,
    old: Option<&str>,
    new: Option<&str>,
) {
    if right != old {
        if new == right {
            rule.positive += 1;
            rule.has_been_selected_as_high_rule_before = false;
        } else {
            rule.neutral += 1;
        }
    } else if new == right {
        rule.neutral += 1;
    } else {
        rule.negative += 1;
        rule.has_been_selected_as_high_rule_before = false;
    }
}

/// `generatePositiveRules(i, j)`: every rule that would rewrite this token's
/// current guess into its gold tag.
///
/// Returns nothing when the guess is already right — no rule proposed there could
/// ever score positively.
fn generate_positive_rules(
    sentence: &Sentence<'_>,
    j: usize,
    templates: &[RuleTemplate],
    out: &mut Vec<TransformationRule>,
) -> Result<(), TaggerError> {
    let token = &sentence.tagged_words[j];
    let old_tag = token.test_tag.owned();
    let new_tag = token.tag.owned();
    if old_tag.as_deref() == new_tag.as_deref() {
        return Ok(());
    }

    let site = j as isize;
    let words = &sentence.tagged_words;
    // A local set, because a template can propose the same rule twice (two of the
    // three-way `prev1Or2Or3Tag` reads can return the same tag) and the reference
    // deduplicates before handing the batch on.
    let mut seen = RuleSet::empty();
    let mut p1: Vec<Option<Tag>> = Vec::new();
    let mut p2: Vec<Option<Tag>> = Vec::new();

    for template in templates {
        if !template.window_fits_site(sentence, site) {
            continue;
        }
        let meta = template.meta;
        match meta.nr_parameters {
            1 => {
                p1.clear();
                if let Some(src) = meta.parameter1_values {
                    templates::parameter_values(src, words, site, &mut p1)?;
                }
                for v in p1.drain(..) {
                    seen.add_rule(TransformationRule::new(
                        old_tag.clone(),
                        new_tag.clone(),
                        template.predicate_name,
                        v,
                        None,
                    ));
                }
            }
            2 => {
                p1.clear();
                p2.clear();
                if let Some(src) = meta.parameter1_values {
                    templates::parameter_values(src, words, site, &mut p1)?;
                }
                if let Some(src) = meta.parameter2_values {
                    templates::parameter_values(src, words, site, &mut p2)?;
                }
                for a in &p1 {
                    for b in &p2 {
                        seen.add_rule(TransformationRule::new(
                            old_tag.clone(),
                            new_tag.clone(),
                            template.predicate_name,
                            a.clone(),
                            b.clone(),
                        ));
                    }
                }
            }
            // Zero parameters: one rule, both parameters `undefined`.
            _ => {
                seen.add_rule(TransformationRule::new(
                    old_tag.clone(),
                    new_tag.clone(),
                    template.predicate_name,
                    None,
                    None,
                ));
            }
        }
    }
    out.extend(seen.into_rules());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::corpus::sentence_from_pairs;

    fn corpus() -> Corpus<'static> {
        // "to book" is tagged NN by the lexicon and VB by the gold standard, so
        // `NN VB PREV-TAG TO` is the rule a working trainer must find.
        Corpus::from_sentences(vec![
            sentence_from_pairs(&[("to", Some("TO")), ("book", Some("VB"))]),
            sentence_from_pairs(&[("to", Some("TO")), ("fly", Some("VB"))]),
        ])
    }

    fn lexicon() -> Lexicon {
        let mut l = Lexicon::empty();
        l.add_word("to", vec!["TO".into()]);
        l.add_word("book", vec!["NN".into()]);
        l.add_word("fly", vec!["NN".into()]);
        l
    }

    #[test]
    fn learns_the_textbook_rule() {
        let mut c = corpus();
        let templates = [RuleTemplate::named("PREV-TAG").unwrap()];
        let mut trainer = BrillPosTrainer::new(1);
        let rules = trainer.train(&mut c, &templates, &lexicon()).unwrap();
        let keys: Vec<&str> = rules.rules().iter().map(TransformationRule::key).collect();
        assert_eq!(keys, ["NN,VB,PREV-TAG,TO,"]);
        assert_eq!(
            trainer.print_rules_with_scores(),
            "2\t2\t0\t0\tNN VB PREV-TAG TO\n"
        );
    }

    #[test]
    fn a_zero_threshold_means_one() {
        assert_eq!(BrillPosTrainer::new(0).rule_score_threshold, 1);
        assert_eq!(BrillPosTrainer::default().rule_score_threshold, 1);
        assert_eq!(BrillPosTrainer::new(-1).rule_score_threshold, -1);
        assert_eq!(BrillPosTrainer::new(5).rule_score_threshold, 5);
    }

    #[test]
    fn a_negative_threshold_keeps_the_eighteen_seeded_english_rules() {
        // The clearest evidence that `new RuleSet()` is not empty: nothing here
        // proposes a `CURRENT-WORD-IS-URL` rule, yet one comes back.
        let mut c = corpus();
        let templates = [RuleTemplate::named("PREV-TAG").unwrap()];
        let rules = BrillPosTrainer::new(-1)
            .train(&mut c, &templates, &lexicon())
            .unwrap();
        assert_eq!(rules.len(), 19, "18 English rules plus the learned one");
        assert_eq!(rules.rules()[0].key(), "VBD,NN,PREV-TAG,DT,");
        assert!(rules.get("NN,URL,CURRENT-WORD-IS-URL,YES,").is_some());
    }

    #[test]
    fn training_corrupts_the_gold_tags_it_rewrites() {
        let mut c = corpus();
        let templates = [RuleTemplate::named("PREV-TAG").unwrap()];
        BrillPosTrainer::new(1)
            .train(&mut c, &templates, &lexicon())
            .unwrap();
        let w = &c.sentences[0].tagged_words[1];
        assert_eq!(w.tag(), Some("VB"));
        // applyAt copies the pre-apply gold tag into testTag, overwriting the
        // lexicon's guess of NN.
        assert_eq!(w.test_tag.value(), Some("VB"));
        assert_eq!(w.new_tag.value(), Some("VB"));
    }

    #[test]
    fn the_broken_templates_cannot_be_trained() {
        // The mis-tagged token sits at index 2 of four, so every window from
        // [-2, 0] to [0, 2] fits and the parameter helpers are actually reached.
        let wide = || {
            Corpus::from_sentences(vec![sentence_from_pairs(&[
                ("a", Some("AT")),
                ("to", Some("TO")),
                ("book", Some("VB")),
                ("now", Some("RB")),
            ])])
        };
        let mut wide_lexicon = lexicon();
        wide_lexicon.add_word("a", vec!["AT".into()]);
        wide_lexicon.add_word("now", vec!["RB".into()]);

        for name in [
            "CURWD",
            "PREV1OR2WD",
            "WDNEXTTAG",
            "WDAND2TAGBFR",
            "RBIGRAM",
        ] {
            let mut c = wide();
            let templates = [RuleTemplate::named(name).unwrap()];
            let lexicon = || wide_lexicon.clone();
            assert_eq!(
                BrillPosTrainer::new(1)
                    .train(&mut c, &templates, &lexicon())
                    .err(),
                Some(TaggerError::undefined("token")),
                "{name}"
            );
        }
    }

    #[test]
    fn an_empty_template_list_still_scores_the_seeded_rules() {
        let mut c = corpus();
        let rules = BrillPosTrainer::new(1)
            .train(&mut c, &[], &lexicon())
            .unwrap();
        assert!(rules.is_empty(), "none of the English rules can score here");
        let rules = BrillPosTrainer::new(-1)
            .train(&mut c, &[], &lexicon())
            .unwrap();
        assert_eq!(rules.len(), 18);
    }

    #[test]
    fn an_empty_corpus_trains_to_nothing() {
        let mut c = Corpus::new();
        let templates = [RuleTemplate::named("PREV-TAG").unwrap()];
        let rules = BrillPosTrainer::new(1)
            .train(&mut c, &templates, &lexicon())
            .unwrap();
        assert!(rules.is_empty());
        assert_eq!(BrillPosTrainer::new(1).print_rules_with_scores(), "");
    }
}
