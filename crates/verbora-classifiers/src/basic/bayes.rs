use std::collections::BTreeMap;

use crate::basic::classifier::{
    Classification, Classifier, ClassifierError, Engine, sort_descending,
};
use crate::dynval::DynValue;
use crate::ordmap::OrderedMap;
use crate::transcendental;

/// A document classifier over the naive-Bayes engine.
///
/// ```
/// use verbora_classifiers::BayesClassifier;
///
/// let mut c = BayesClassifier::new();
/// c.add_document("my unit-tests failed.", "software");
/// c.add_document("tomorrow we will do standard tests", "other");
/// c.train().unwrap();
/// assert_eq!(c.classify("did the standard tests pass?").unwrap(), "other");
/// ```
///
/// Note that `"tests"` alone would *not* discriminate here: the tokenizer cuts
/// `"unit-tests"` at the hyphen (U+002D is `Word_Break=Other`), so `test` is a
/// feature of both classes and the two scores tie.
pub type BayesClassifier = Classifier<BayesEngine>;

/// The naive-Bayes engine: per-class feature counts plus Laplace smoothing.
/// The naive-Bayes engine: per-class feature counts plus additive smoothing.
///
/// Small enough to read in one sitting, and every line of it matters:
///
/// * Counts start at `1 + smoothing`, not at 1, and the "already seen?" test is
///   a **truthiness** check — so a stored count of exactly `0` (reachable with
///   `smoothing == -1`) is treated as absent and reset rather than incremented.
/// * `probabilityOfClass` accumulates its log-sum from the **highest** set
///   feature index **down** to zero. Summing the other way changes the last bits
///   of the result, and the results are then sorted, so a near-tie can flip.
/// * Class labels are enumerated by [`OrderedMap`](crate::OrderedMap), which
///   puts integer-like keys first in ascending numeric order — which is *not*
///   the order they were added in, and is what `get_classifications` returns
///   ties in.
///
#[derive(Debug, Clone)]
pub struct BayesEngine {
    /// Label -> feature index -> count. The inner map is keyed by the numeric
    /// feature index; a `BTreeMap` iterates it in ascending numeric order, which
    /// matches how a feature index is enumerated everywhere else in this crate
    /// — and therefore how the counts appear in the serialised form, regardless
    /// of the descending order `add_example` wrote them in.
    class_features: OrderedMap<BTreeMap<u32, f64>>,
    class_totals: OrderedMap<f64>,
    total_examples: f64,
    smoothing: f64,
}

impl Default for BayesEngine {
    fn default() -> Self {
        Self {
            class_features: OrderedMap::new(),
            class_totals: OrderedMap::new(),
            // Starts at one to smooth.
            total_examples: 1.0,
            smoothing: 1.0,
        }
    }
}

/// Whether a stored count is a real observation.
///
/// A count of exactly zero is reachable with a smoothing constant of `-1`, and
/// is treated as absent rather than as "seen zero times" — the same test the
/// per-class lookup applies, kept in one place so the two cannot disagree.
#[inline]
fn truthy(v: f64) -> bool {
    v != 0.0 && !v.is_nan()
}

impl BayesEngine {
    /// An engine with a custom smoothing constant.
    ///
    /// Reproduces `if (smoothing && isFinite(smoothing))`: the value is used
    /// only when it is **truthy and finite**, so `0`, `-0`, `NaN` and both
    /// infinities all fall back to `1.0`, while `-1` is accepted.
    pub fn with_smoothing(smoothing: f64) -> Self {
        Self {
            smoothing: if smoothing != 0.0 && smoothing.is_finite() {
                smoothing
            } else {
                1.0
            },
            ..Self::default()
        }
    }

    /// The smoothing constant in force.
    pub fn smoothing(&self) -> f64 {
        self.smoothing
    }

    /// `totalExamples`, which starts at 1 rather than 0.
    pub fn total_examples(&self) -> f64 {
        self.total_examples
    }

    /// Per-class example totals, each pre-incremented by one for smoothing.
    pub fn class_totals(&self) -> &OrderedMap<f64> {
        &self.class_totals
    }

    /// Per-class feature counts.
    pub fn class_features(&self) -> &OrderedMap<BTreeMap<u32, f64>> {
        &self.class_features
    }

    /// `probabilityOfClass(observation, label)`.
    ///
    /// The loop runs `while (i--)`, so the logs are added from the highest set
    /// index downwards, because floating-point addition is not associative and
    /// the scores are then sorted against each other: on a real corpus,
    /// ascending accumulation produces a different double for the same class.
    ///
    /// `None` when `label` was never trained: an untrained class has no prior
    /// and no counts, so there is no quantity to report. This used to be a
    /// panic, on the reasoning that `get_classifications` only ever passes
    /// labels it has just enumerated — true of that caller, and irrelevant to
    /// this one, which is `pub` and takes the label as an ordinary argument.
    #[must_use]
    pub fn probability_of_class(&self, observation: &[u8], label: &str) -> Option<f64> {
        self.probability_of_class_at(&set_indices_descending(observation), label)
    }

    /// [`Self::probability_of_class`] over an already-extracted index list.
    ///
    /// The `while (i--)` loop reads nothing but the *set* positions, and which
    /// positions those are does not depend on the label — so scoring `c`
    /// classes with one shared list does exactly the per-class work, minus
    /// `c - 1` re-scans of a mostly-zero vector. `set_desc`
    /// must be strictly descending for the sum to land in the same order, and
    /// therefore on the same bits.
    fn probability_of_class_at(&self, set_desc: &[u32], label: &str) -> Option<f64> {
        let total = *self.class_totals.get(label)?;
        let features = self.class_features.get(label)?;

        let mut prob = 0.0;
        for &i in set_desc {
            // `this.classFeatures[label][i] || this.smoothing` — the
            // fallback fires for an absent index *and* for a stored zero.
            let count = features
                .get(&i)
                .copied()
                .filter(|v| truthy(*v))
                .unwrap_or(self.smoothing);
            prob += transcendental::log(count / total);
        }
        Some((total / self.total_examples) * transcendental::exp(prob))
    }
}

/// The set positions of `observation`, highest first.
///
/// Descending because `probabilityOfClass` accumulates its log-sum with
/// `while (i--)`, and floating-point addition is not associative: summing the
/// same terms the other way round changes the last bits of a score that is
/// then sorted against the other classes'.
fn set_indices_descending(observation: &[u8]) -> Vec<u32> {
    let mut out = Vec::new();
    let mut i = observation.len();
    while i > 0 {
        i -= 1;
        if observation[i] != 0 {
            out.push(i as u32);
        }
    }
    out
}

impl Engine for BayesEngine {
    /// Bayes accumulates: repeated `train()` calls do not re-add documents.
    const RESETS_ON_TRAIN: bool = false;

    fn add_example(&mut self, observation: &[u8], label: &str) {
        if !self.class_features.contains_key(label) {
            self.class_features.insert(label, BTreeMap::new());
            // An extra one for smoothing.
            self.class_totals.insert(label, 1.0);
        }
        self.total_examples += 1.0;
        if let Some(total) = self.class_totals.get_mut(label) {
            *total += 1.0;
        }

        let smoothing = self.smoothing;
        let features = self
            .class_features
            .get_mut(label)
            .expect("just inserted if absent");
        let mut i = observation.len();
        while i > 0 {
            i -= 1;
            if observation[i] != 0 {
                let idx = i as u32;
                match features.get(&idx).copied() {
                    Some(v) if truthy(v) => features.insert(idx, v + 1.0),
                    // First occurrence: an extra one for smoothing.
                    _ => features.insert(idx, 1.0 + smoothing),
                };
            }
        }
    }

    /// The engine's own `train()` has an empty body.
    fn fit(&mut self) -> Result<(), ClassifierError> {
        Ok(())
    }

    fn classifications(&self, observation: &[u8]) -> Result<Vec<Classification>, ClassifierError> {
        // Every class reads the same set positions out of the same
        // observation, so the extraction is hoisted out of the per-class loop.
        let set_desc = set_indices_descending(observation);
        let mut labels: Vec<Classification> = self
            .class_features
            .enumeration_order()
            .into_iter()
            .filter_map(|label| {
                // Every label here was just enumerated out of `class_features`,
                // so the lookup cannot miss; `filter_map` states that rather
                // than asserting it.
                Some(Classification {
                    label: label.to_owned(),
                    value: self.probability_of_class_at(&set_desc, label)?,
                })
            })
            .collect();
        sort_descending(&mut labels);
        Ok(labels)
    }

    fn to_value(&self) -> DynValue {
        DynValue::Obj(vec![
            (
                "classFeatures".to_owned(),
                DynValue::Obj(
                    self.class_features
                        .ordered_entries()
                        .into_iter()
                        .map(|(label, counts)| {
                            (
                                label.to_owned(),
                                DynValue::Obj(
                                    counts
                                        .iter()
                                        .map(|(i, c)| (i.to_string(), DynValue::Num(*c)))
                                        .collect(),
                                ),
                            )
                        })
                        .collect(),
                ),
            ),
            (
                "classTotals".to_owned(),
                DynValue::Obj(
                    self.class_totals
                        .ordered_entries()
                        .into_iter()
                        .map(|(label, total)| (label.to_owned(), DynValue::Num(*total)))
                        .collect(),
                ),
            ),
            (
                "totalExamples".to_owned(),
                DynValue::Num(self.total_examples),
            ),
            ("smoothing".to_owned(), DynValue::Num(self.smoothing)),
        ])
    }

    fn from_value(value: &DynValue) -> Self {
        let mut out = Self::default();
        if let Some(DynValue::Obj(classes)) = value.get("classFeatures") {
            for (label, counts) in classes {
                let mut inner = BTreeMap::new();
                if let DynValue::Obj(entries) = counts {
                    for (idx, count) in entries {
                        if let (Ok(i), DynValue::Num(c)) = (idx.parse::<u32>(), count) {
                            inner.insert(i, *c);
                        }
                    }
                }
                out.class_features.insert(label.clone(), inner);
            }
        }
        if let Some(DynValue::Obj(totals)) = value.get("classTotals") {
            for (label, total) in totals {
                out.class_totals.insert(
                    label.clone(),
                    if let DynValue::Num(n) = total {
                        *n
                    } else {
                        0.0
                    },
                );
            }
        }
        if let Some(DynValue::Num(n)) = value.get("totalExamples") {
            out.total_examples = *n;
        }
        if let Some(DynValue::Num(n)) = value.get("smoothing") {
            out.smoothing = *n;
        }
        out
    }
}

impl Classifier<BayesEngine> {
    /// A Bayes classifier with a custom smoothing constant.
    ///
    /// Beware that `retrain()` rebuilds the engine with no arguments and so
    /// **resets the smoothing to 1.0**: `retrain` reconstructs the engine with
    /// its default constant. Set it again afterwards if you need it.
    pub fn with_smoothing(smoothing: f64) -> Self {
        let mut out = Self::new();
        out.set_engine(BayesEngine::with_smoothing(smoothing));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trained() -> BayesClassifier {
        let mut c = BayesClassifier::new();
        c.add_document("my unit-tests failed.", "software");
        c.add_document("tried the program, but it was buggy.", "software");
        c.add_document("tomorrow we will do standard tests", "other");
        c.add_document("the drive has a 2TB capacity", "other");
        c.train().unwrap();
        c
    }

    /// A discriminating query, with both scores derived by hand.
    ///
    /// `"did the program crash?"` stems to `["program", "crash"]` after the
    /// stop-word filter; `program` is feature 4 and `crash` is unknown, so the
    /// observation has exactly one set index.
    ///
    /// * software: `classFeatures["software"][4] == 2`, `classTotals == 3`, so
    ///   the log-sum is `ln(2/3)` and the score is `(3/5) * exp(ln(2/3))`.
    /// * other: index 4 is absent, so the smoothing constant `1` is used and
    ///   the score is `(3/5) * exp(ln(1/3))`.
    ///
    /// The doubles below are those two expressions, which are the same
    /// arithmetic the engine performs in the same order.
    #[test]
    fn a_discriminating_query_scores_by_hand_computation() {
        let c = trained();
        assert_eq!(c.classify("did the program crash?").unwrap(), "software");
        let scores = c.get_classifications("did the program crash?").unwrap();
        assert_eq!(scores[0].label, "software");
        assert_eq!(scores[0].value, 0.399_999_999_999_999_97);
        assert_eq!(scores[1].label, "other");
        assert_eq!(scores[1].value, 0.199_999_999_999_999_98);
    }

    /// The boundary change made `"tests"` a shared feature, and a tie is the
    /// honest outcome — pinned so it is not mistaken for a scoring bug later.
    ///
    /// `"unit-tests"` is two words under UAX #29 (U+002D is
    /// `Word_Break=Other`), so `test` is trained into *both* classes with the
    /// same count and the same class total. Ties resolve by enumeration order,
    /// which puts `software` first.
    #[test]
    fn a_shared_feature_ties_and_resolves_by_enumeration_order() {
        let c = trained();
        let scores = c.get_classifications("did the tests pass?").unwrap();
        assert_eq!(
            scores[0].value, scores[1].value,
            "the query does not discriminate"
        );
        assert_eq!(scores[0].label, "software");
        assert_eq!(c.classify("did the tests pass?").unwrap(), "software");
    }

    #[test]
    fn untrained_classify_reports_not_trained() {
        let c = BayesClassifier::new();
        assert_eq!(c.classify("anything"), Err(ClassifierError::NotTrained));
        assert_eq!(c.get_classifications("anything").unwrap(), vec![]);
    }

    #[test]
    fn training_twice_does_not_re_add_documents() {
        let mut c = trained();
        let before = c.engine().total_examples();
        c.train().unwrap();
        assert_eq!(c.engine().total_examples(), before);
    }

    #[test]
    fn smoothing_guard_is_truthy_and_finite() {
        for bad in [0.0, -0.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(BayesEngine::with_smoothing(bad).smoothing(), 1.0);
        }
        assert_eq!(BayesEngine::with_smoothing(-1.0).smoothing(), -1.0);
        assert_eq!(BayesEngine::with_smoothing(0.5).smoothing(), 0.5);
    }

    #[test]
    fn retrain_discards_custom_smoothing() {
        let mut c = BayesClassifier::with_smoothing(0.1);
        c.add_document("my unit-tests failed.", "software");
        c.train().unwrap();
        assert_eq!(c.engine().smoothing(), 0.1);
        c.retrain().unwrap();
        assert_eq!(c.engine().smoothing(), 1.0);
    }

    /// The serialised JSON layout, with every value derived rather than
    /// recorded.
    ///
    /// Tokenization is UAX #29 and stemming is English Porter, so the four
    /// documents reduce as follows (stop words in parentheses are dropped):
    ///
    /// * `"my unit-tests failed."` — (my) unit tests failed
    ///   → `["unit", "test", "fail"]`. `"unit-tests"` is **two** words: U+002D
    ///   is `Word_Break=Other`, so WB999 breaks on both sides of it.
    /// * `"tried the program, but it was buggy."` — tried (the) program (but)
    ///   (it) (was) buggy → `["tri", "program", "buggi"]`.
    /// * `"tomorrow we will do standard tests"` — tomorrow (we) will (do)
    ///   standard tests → `["tomorrow", "will", "standard", "test"]`.
    /// * `"the drive has a 2TB capacity"` — (the) drive (has) (a) 2TB capacity
    ///   → `["drive", "2tb", "capac"]`, lowercased because the English pipeline
    ///   lowercases the document before tokenizing.
    ///
    /// Feature ids are assigned in first-appearance order, so `test` is id 1
    /// and is shared by both classes — which is why `software` holds
    /// `{0..5}` and `other` holds `{1, 6..11}`. Each count is `1 + smoothing`
    /// on first occurrence, hence `2` throughout.
    ///
    /// The leading `_verbora` member is the compatibility stamp; it is rendered
    /// from [`ArtifactStamp::current`] rather than hardcoded because both the
    /// Unicode version and the lowercase fingerprint in it are facts about the
    /// build, and its own shape and refusal behaviour are pinned by
    /// `crate::stamp`'s tests.
    #[test]
    fn the_serialised_shape_is_derived_field_by_field() {
        let c = trained();
        let stamp = crate::ArtifactStamp::for_stemmer(&*crate::default_stemmer());
        let (major, minor, update) = stamp.unicode;
        let lowercase = stamp
            .lowercase
            .expect("this build's stamp always carries a fingerprint");
        let stemmer = stamp
            .stemmer
            .expect("a classifier's stamp always names its stemmer");
        assert_eq!(
            c.to_json(),
            format!(
                r#"{{"_verbora":{{"schema":{},"unicode":"{major}.{minor}.{update}","lowercase":"{lowercase:016x}","stemmer":"{stemmer:016x}"}},"#,
                stamp.schema
            ) + r#""_events":{},"_eventsCount":0,"classifier":{"classFeatures":{"software":{"0":2,"1":2,"2":2,"3":2,"4":2,"5":2},"other":{"1":2,"6":2,"7":2,"8":2,"9":2,"10":2,"11":2}},"classTotals":{"software":3,"other":3},"totalExamples":5,"smoothing":1},"docs":[{"label":"software","text":["unit","test","fail"]},{"label":"software","text":["tri","program","buggi"]},{"label":"other","text":["tomorrow","will","standard","test"]},{"label":"other","text":["drive","2tb","capac"]}],"features":{"unit":1,"test":2,"fail":1,"tri":1,"program":1,"buggi":1,"tomorrow":1,"will":1,"standard":1,"drive":1,"2tb":1,"capac":1},"stemmer":{},"lastAdded":4,"Threads":null}"#
        );
    }

    /// Feature keys are stems of UAX #29 tokens, so a non-ASCII document must
    /// survive as a whole word rather than being cut at every accented letter.
    ///
    /// An ASCII-only fixture set cannot see this at all: ASCII is invariant
    /// under every candidate unit choice, so it can neither detect a unit
    /// change nor a character-class gap.
    #[test]
    fn non_ascii_documents_keep_their_words_whole() {
        let mut c = BayesClassifier::new();
        c.add_document("naïve café crème", "french");
        c.add_document("Москва и Ленинград", "russian");
        c.train().unwrap();
        // `naïve`/`café`/`crème` are single tokens; Porter leaves them alone
        // apart from the lowercasing the English pipeline applies first.
        assert_eq!(c.classify("café").unwrap(), "french");
        assert_eq!(c.classify("москва").unwrap(), "russian");
    }

    #[test]
    fn restore_round_trips() {
        let c = trained();
        let revived = BayesClassifier::restore(&c.to_json()).unwrap();
        assert_eq!(revived.to_json(), c.to_json());
        assert_eq!(
            revived.get_classifications("did the tests pass?").unwrap(),
            c.get_classifications("did the tests pass?").unwrap()
        );
    }
}
