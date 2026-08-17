//! Scoring a tagger against an annotated corpus.

use crate::corpus::Corpus;
use crate::error::TaggerError;
use crate::tagger::BrillPosTagger;

/// Measures how often a tagger reproduces a corpus's annotations.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BrillPosTester;

/// Percentages correct: before the transformation rules, and after them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Accuracy {
    /// Percentage the lexicon alone got right.
    pub lexicon: f64,
    /// Percentage after the rule set ran.
    pub after_rules: f64,
}

impl BrillPosTester {
    /// A tester. The reference's class has no state either.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// `test(corpus, tagger)`: percentage of tags reproduced, lexicon-only and
    /// after rules.
    ///
    /// Both figures are `100 * correct / total` in that order, over integer
    /// counters, so the arithmetic is exact until the single final division —
    /// there is no accumulation order to get wrong. An **empty corpus scores
    /// `NaN`**, not `0`: `0/0`.
    ///
    /// The corpus is only read. The tagger tags a fresh copy of each sentence's
    /// tokens, so nothing the rules do leaks between sentences.
    ///
    /// # Errors
    ///
    /// Propagates predicate errors from the rule pass — reachable with a corpus
    /// containing an empty token and a rule set using `CURRENT-WORD-IS-CAP`.
    pub fn test(
        &self,
        corpus: &Corpus<'_>,
        tagger: &BrillPosTagger<'_>,
    ) -> Result<Accuracy, TaggerError> {
        let mut total: u64 = 0;
        let mut correct_lexicon: u64 = 0;
        let mut correct_after_rules: u64 = 0;

        for sentence in &corpus.sentences {
            let tokens = sentence.tagged_words.iter().map(|w| &*w.token);
            let mut tagged = tagger.tag_with_lexicon(tokens);

            for (i, token) in sentence.tagged_words.iter().enumerate() {
                total += 1;
                if token.tag() == tagged.tagged_words[i].tag() {
                    correct_lexicon += 1;
                }
            }

            tagger.apply_rules(&mut tagged)?;

            for (i, token) in sentence.tagged_words.iter().enumerate() {
                if token.tag() == tagged.tagged_words[i].tag() {
                    correct_after_rules += 1;
                }
            }
        }

        #[allow(clippy::cast_precision_loss)] // counts stay far below 2^53
        Ok(Accuracy {
            lexicon: 100.0 * correct_lexicon as f64 / total as f64,
            after_rules: 100.0 * correct_after_rules as f64 / total as f64,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::corpus::sentence_from_pairs;
    use crate::lexicon::Lexicon;
    use crate::ruleset::RuleSet;

    #[test]
    fn an_empty_corpus_scores_nan() {
        let lex = Lexicon::empty();
        let rules = RuleSet::empty();
        let tagger = BrillPosTagger::new(&lex, &rules);
        let got = BrillPosTester::new().test(&Corpus::new(), &tagger).unwrap();
        assert!(got.lexicon.is_nan() && got.after_rules.is_nan());
    }

    #[test]
    fn rules_can_improve_or_worsen_the_lexicon_score() {
        let mut lex = Lexicon::empty();
        lex.add_word("to", vec!["TO".into()]);
        lex.add_word("book", vec!["NN".into()]);
        let corpus = Corpus::from_sentences(vec![sentence_from_pairs(&[
            ("to", Some("TO")),
            ("book", Some("VB")),
        ])]);

        let none = RuleSet::empty();
        let tagger = BrillPosTagger::new(&lex, &none);
        let got = BrillPosTester::new().test(&corpus, &tagger).unwrap();
        assert_eq!(got.lexicon, 50.0);
        assert_eq!(got.after_rules, 50.0);

        let fix = RuleSet::from_rule_strings(&["NN VB PREV-TAG TO"]).unwrap();
        let tagger = BrillPosTagger::new(&lex, &fix);
        let got = BrillPosTester::new().test(&corpus, &tagger).unwrap();
        assert_eq!(got.lexicon, 50.0, "the lexicon score is measured first");
        assert_eq!(got.after_rules, 100.0);

        let break_it = RuleSet::from_rule_strings(&["TO IN NEXT-TAG NN"]).unwrap();
        let tagger = BrillPosTagger::new(&lex, &break_it);
        let got = BrillPosTester::new().test(&corpus, &tagger).unwrap();
        assert_eq!(got.after_rules, 0.0);
    }

    #[test]
    fn predicate_errors_propagate() {
        let lex = Lexicon::empty();
        let rules = RuleSet::from_rule_strings(&["* B CURRENT-WORD-IS-CAP YES"]).unwrap();
        let tagger = BrillPosTagger::new(&lex, &rules);
        let corpus = Corpus::from_sentences(vec![sentence_from_pairs(&[("", Some("A"))])]);
        assert_eq!(
            BrillPosTester::new().test(&corpus, &tagger),
            Err(TaggerError::undefined("toUpperCase"))
        );
    }
}
