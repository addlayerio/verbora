//! The `apparatus` naive-Bayes engine.
//!
//! Small enough to read in one sitting, and every line of it matters:
//!
//! * Counts start at `1 + smoothing`, not at 1, and the "already seen?" test is
//!   a **truthiness** check — so a stored count of exactly `0` (reachable with
//!   `smoothing == -1`) is treated as absent and reset rather than incremented.
//! * `probabilityOfClass` accumulates its log-sum from the **highest** set
//!   feature index **down** to zero. Summing the other way changes the last bits
//!   of the result, and the results are then sorted, so a near-tie can flip.
//! * The class labels are the reference object keys, so `getClassifications`
//!   enumerates them integer-like-first — which is *not* the order they were
//!   added in.

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
/// assert_eq!(c.classify("did the tests pass?").unwrap(), "other");
/// ```
pub type BayesClassifier = Classifier<BayesEngine>;

/// The naive-Bayes engine: per-class feature counts plus Laplace smoothing.
#[derive(Debug, Clone)]
pub struct BayesEngine {
    /// Label -> feature index -> count. The inner map is keyed by the numeric
    /// feature index; a `BTreeMap` iterates it in ascending numeric order, which
    /// is exactly how the reference enumerates array-index keys — and therefore how
    /// the counts appear in the serialised form, regardless of the descending
    /// order in which `addExample` wrote them.
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

/// The reference truthiness for a stored count.
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
    /// index downwards. Verified against the reference on a real dataset where
    /// ascending accumulation produces a different double for the same class.
    ///
    /// # Panics
    ///
    /// Panics if `label` was never trained; the reference throws a `TypeError`
    /// in the same situation, and `getClassifications` only ever passes labels
    /// it just enumerated.
    pub fn probability_of_class(&self, observation: &[u8], label: &str) -> f64 {
        let total = *self
            .class_totals
            .get(label)
            .expect("label was enumerated from classFeatures");
        let features = self
            .class_features
            .get(label)
            .expect("label was enumerated from classFeatures");

        let mut prob = 0.0;
        let mut i = observation.len();
        while i > 0 {
            i -= 1;
            if observation[i] != 0 {
                // `this.classFeatures[label][i] || this.smoothing` — the
                // fallback fires for an absent index *and* for a stored zero.
                let count = features
                    .get(&(i as u32))
                    .copied()
                    .filter(|v| truthy(*v))
                    .unwrap_or(self.smoothing);
                prob += transcendental::log(count / total);
            }
        }
        (total / self.total_examples) * transcendental::exp(prob)
    }
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
        let mut labels: Vec<Classification> = self
            .class_features
            .enumeration_order()
            .into_iter()
            .map(|label| Classification {
                label: label.to_owned(),
                value: self.probability_of_class(observation, label),
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
    /// **resets the smoothing to 1.0** — the reference does the same, and the
    /// recorded fixtures pin it.
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

    #[test]
    fn readme_example_classifies() {
        let c = trained();
        assert_eq!(c.classify("did the tests pass?").unwrap(), "other");
        let scores = c.get_classifications("did the tests pass?").unwrap();
        assert_eq!(scores[0].label, "other");
        // Recorded from the reference.
        assert_eq!(scores[0].value, 0.399_999_999_999_999_97);
        assert_eq!(scores[1].value, 0.199_999_999_999_999_98);
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

    #[test]
    fn serialised_shape_matches_the_reference() {
        let c = trained();
        assert_eq!(
            c.to_json(),
            r#"{"_events":{},"_eventsCount":0,"classifier":{"classFeatures":{"software":{"0":2,"1":2,"2":2,"3":2,"4":2},"other":{"5":2,"6":2,"7":2,"8":2,"9":2,"10":2,"11":2}},"classTotals":{"software":3,"other":3},"totalExamples":5,"smoothing":1},"docs":[{"label":"software","text":["unit-test","fail"]},{"label":"software","text":["tri","program","buggi"]},{"label":"other","text":["tomorrow","will","standard","test"]},{"label":"other","text":["drive","2tb","capac"]}],"features":{"unit-test":1,"fail":1,"tri":1,"program":1,"buggi":1,"tomorrow":1,"will":1,"standard":1,"test":1,"drive":1,"2tb":1,"capac":1},"stemmer":{},"lastAdded":4,"Threads":null}"#
        );
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
