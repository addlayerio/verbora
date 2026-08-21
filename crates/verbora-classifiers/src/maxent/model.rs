//! The fitted model: outcomes, sparse parameters, and the distribution they
//! define.

use std::sync::Arc;

use rustc_hash::FxHashMap;

use crate::dynval::DynValue;
use crate::maxent::sample::Sample;
use crate::transcendental::{exp, log};

/// A fitted conditional maximum-entropy model.
///
/// See [`MaxEntClassifier`](crate::MaxEntClassifier) for the conditional
/// model this fits, what training optimises, and the guarantees it keeps.
///
/// The model is **immutable, owned and `Send + Sync`**: fitting produces one,
/// and evaluating it never mutates it, so a single model can be shared across
/// threads behind an [`Arc`] with no lock. Nothing is memoised at prediction
/// time, which is what makes that true.
///
/// Parameters are stored as a compressed sparse row: one row per predicate, and
/// inside it the `(outcome, weight)` pairs that predicate was observed with.
/// A predicate the training sample never paired with an outcome has no weight
/// for it at all, rather than a stored zero.
///
/// ```
/// use verbora_classifiers::{Gis, MaxEntClassifier};
///
/// let mut classifier = MaxEntClassifier::new();
/// for _ in 0..2 { classifier.add("sun", ["sky=blue"]); }
/// classifier.add("rain", ["sky=blue"]);
/// for _ in 0..2 { classifier.add("rain", ["sky=grey"]); }
/// classifier.add("sun", ["sky=grey"]);
/// classifier.train_with(Gis::new(500, 1e-12).unwrap()).unwrap();
///
/// let model = classifier.model().unwrap();
/// assert_eq!(model.outcomes(), ["sun", "rain"]);
/// // One predicate per context, so the fit reproduces the empirical
/// // conditional exactly: two of the three grey days rained.
/// let p = model.distribution(["sky=grey"]);
/// assert!((p[1] - 2.0 / 3.0).abs() < 1e-6, "{p:?}");
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct MaxEntModel {
    /// Outcome labels, in the sample's first-appearance order.
    outcomes: Vec<String>,
    /// Row `i`'s predicate name.
    predicates: Vec<Arc<str>>,
    /// Row `i` owns features `row_start[i] .. row_start[i + 1]`.
    row_start: Vec<u32>,
    /// Feature `k`'s outcome, as an index into `outcomes`.
    feature_outcome: Vec<u32>,
    /// Feature `k`'s weight.
    weight: Vec<f64>,
    /// Predicate name to row. Shares its keys with `predicates` rather than
    /// copying them, which is why the names are `Arc<str>`.
    index: FxHashMap<Arc<str>, u32>,
}

impl MaxEntModel {
    /// Assembles a model from a built index. The caller guarantees `row_start`
    /// is ascending, ends at `weight.len()`, and that every weight is finite.
    pub(crate) fn assemble(
        outcomes: Vec<String>,
        predicates: Vec<Arc<str>>,
        row_start: Vec<u32>,
        feature_outcome: Vec<u32>,
        weight: Vec<f64>,
    ) -> Self {
        let index = predicates
            .iter()
            .enumerate()
            .map(|(i, name)| (Arc::clone(name), i as u32))
            .collect();
        Self {
            outcomes,
            predicates,
            row_start,
            feature_outcome,
            weight,
            index,
        }
    }

    /// The outcome labels, in the order the training sample first saw them.
    ///
    /// [`Self::distribution`] is aligned with this slice, and ties in
    /// [`MaxEntClassifier::classify`](crate::MaxEntClassifier::classify) are
    /// broken by it.
    pub fn outcomes(&self) -> &[String] {
        &self.outcomes
    }

    /// The predicates the model carries weights for, in first-appearance order.
    pub fn predicates(&self) -> impl ExactSizeIterator<Item = &str> {
        self.predicates.iter().map(AsRef::as_ref)
    }

    /// How many `(predicate, outcome)` features the model holds.
    pub fn feature_count(&self) -> usize {
        self.weight.len()
    }

    /// The weight `λ` of the feature `(predicate, outcome)`, if there is one.
    ///
    /// `None` means the training sample never observed the two together, which
    /// is not the same as a weight of zero: an absent feature can never be given
    /// one by fitting, while a stored zero is a fitted value.
    pub fn weight(&self, predicate: &str, outcome: &str) -> Option<f64> {
        let outcome = self.outcome_index(outcome)?;
        let row = *self.index.get(predicate)? as usize;
        let (from, to) = (
            self.row_start[row] as usize,
            self.row_start[row + 1] as usize,
        );
        (from..to)
            .find(|&k| self.feature_outcome[k] == outcome)
            .map(|k| self.weight[k])
    }

    /// `p(y | x)` for every outcome `y`, aligned with [`Self::outcomes`].
    ///
    /// The values are non-negative and sum to `1` up to rounding. Predicates the
    /// model does not know contribute nothing — a context of entirely unknown
    /// predicates is scored by the uniform distribution, which is the
    /// maximum-entropy answer when no constraint applies. Repeated predicates
    /// are counted once, and scores accumulate over the predicates in the
    /// order given; floating-point addition is not associative, so an
    /// equivalent context in a different order is not guaranteed to reproduce
    /// the same score bit for bit.
    ///
    /// A restored weight large enough to push a score to `±∞` cannot produce a
    /// `NaN` here: an infinite score takes the whole probability mass — split
    /// evenly among any outcomes tied at `+∞` — and a context whose every score
    /// is `-∞` is scored uniformly instead.
    ///
    /// ```
    /// use verbora_classifiers::{Gis, MaxEntClassifier};
    ///
    /// let mut classifier = MaxEntClassifier::new();
    /// classifier.add("a", ["p"]);
    /// classifier.add("b", ["q"]);
    /// classifier.train().unwrap();
    ///
    /// let p = classifier.model().unwrap().distribution(["nothing-known"]);
    /// assert_eq!(p, vec![0.5, 0.5]);
    /// ```
    pub fn distribution(&self, predicates: impl IntoIterator<Item = impl AsRef<str>>) -> Vec<f64> {
        let mut out = Vec::with_capacity(self.outcomes.len());
        self.distribution_into(predicates, &mut out);
        out
    }

    /// [`Self::distribution`] into a caller-owned buffer, which is cleared
    /// first and left holding one probability per outcome.
    ///
    /// Reusing one buffer across a batch of predictions removes the only
    /// allocation a prediction otherwise makes per outcome vector.
    pub fn distribution_into(
        &self,
        predicates: impl IntoIterator<Item = impl AsRef<str>>,
        out: &mut Vec<f64>,
    ) {
        self.scores_into(predicates, out);
        normalise(out);
    }

    /// The mean conditional log-likelihood `(1/N) Σᵢ log p(yᵢ | xᵢ)` of
    /// `sample` under this model.
    ///
    /// This is the quantity generalised iterative scaling maximises, so it is
    /// how one fitted model is compared with another over held-out events.
    ///
    /// Returns `None` — rather than an infinity — when the quantity is not
    /// defined: an empty sample, an event whose outcome the model does not
    /// declare, or a context whose scores are not all finite. Weights written by
    /// training and accepted by [`MaxEntClassifier::restore`](crate::MaxEntClassifier::restore)
    /// are finite, so the third case needs a context large enough to overflow a
    /// sum of them.
    pub fn log_likelihood(&self, sample: &Sample) -> Option<f64> {
        if sample.is_empty() {
            return None;
        }
        let mut scores = Vec::with_capacity(self.outcomes.len());
        let mut total = 0.0;
        for event in sample.events() {
            let outcome = self.outcome_index(event.outcome())? as usize;
            self.scores_into(event.predicates(), &mut scores);
            let term = log_probability(&scores, outcome)?;
            total += term;
        }
        Some(total / sample.len() as f64)
    }

    /// The unnormalised scores `Σⱼ λⱼ fⱼ(x, y)`, one per outcome.
    ///
    /// `out` is cleared, resized to the outcome count and filled. Predicates are
    /// visited in the caller's order, skipping unknown ones and repeats; each
    /// row's features are added in the order the row stores them.
    pub(crate) fn scores_into(
        &self,
        predicates: impl IntoIterator<Item = impl AsRef<str>>,
        out: &mut Vec<f64>,
    ) {
        out.clear();
        out.resize(self.outcomes.len(), 0.0);
        // A context is a set. Contexts are small — tens of predicates at most in
        // every published feature set — so a linear scan over the rows already
        // applied beats building a hash set per prediction.
        let mut applied: Vec<u32> = Vec::new();
        for predicate in predicates {
            let Some(&row) = self.index.get(predicate.as_ref()) else {
                continue;
            };
            if applied.contains(&row) {
                continue;
            }
            applied.push(row);
            let (from, to) = (
                self.row_start[row as usize] as usize,
                self.row_start[row as usize + 1] as usize,
            );
            for k in from..to {
                out[self.feature_outcome[k] as usize] += self.weight[k];
            }
        }
    }

    fn outcome_index(&self, outcome: &str) -> Option<u32> {
        self.outcomes
            .iter()
            .position(|o| o == outcome)
            .map(|i| i as u32)
    }

    /// The serialised shape a saved classifier carries under `"model"`.
    pub(crate) fn to_value(&self) -> DynValue {
        let parameters = self
            .predicates
            .iter()
            .enumerate()
            .map(|(row, name)| {
                let (from, to) = (
                    self.row_start[row] as usize,
                    self.row_start[row + 1] as usize,
                );
                let weights = (from..to)
                    .map(|k| {
                        (
                            self.outcomes[self.feature_outcome[k] as usize].clone(),
                            DynValue::Num(self.weight[k]),
                        )
                    })
                    .collect();
                (name.to_string(), DynValue::Obj(weights))
            })
            .collect();
        DynValue::Obj(vec![
            (
                "outcomes".to_owned(),
                DynValue::Arr(self.outcomes.iter().cloned().map(DynValue::Str).collect()),
            ),
            ("parameters".to_owned(), DynValue::Obj(parameters)),
        ])
    }

    /// Reads a model back from [`Self::to_value`] output.
    ///
    /// # Errors
    ///
    /// A [`ModelDefect`] naming the first thing that does not describe a
    /// distribution. Nothing is repaired or defaulted: a model that would score
    /// differently from the one that was saved is refused rather than loaded.
    pub(crate) fn from_value(value: &DynValue) -> Result<Self, ModelDefect> {
        let Some(DynValue::Arr(listed)) = value.get("outcomes") else {
            return Err(ModelDefect::Outcomes);
        };
        let mut outcomes: Vec<String> = Vec::with_capacity(listed.len());
        for item in listed {
            let Some(label) = item.as_str() else {
                return Err(ModelDefect::Outcomes);
            };
            if outcomes.iter().any(|seen| seen == label) {
                return Err(ModelDefect::Duplicate(label.to_owned()));
            }
            outcomes.push(label.to_owned());
        }
        if outcomes.is_empty() {
            return Err(ModelDefect::Outcomes);
        }

        let Some(DynValue::Obj(rows)) = value.get("parameters") else {
            return Err(ModelDefect::Parameters);
        };
        let mut predicates: Vec<Arc<str>> = Vec::with_capacity(rows.len());
        let mut row_start: Vec<u32> = Vec::with_capacity(rows.len() + 1);
        let mut feature_outcome: Vec<u32> = Vec::new();
        let mut weight: Vec<f64> = Vec::new();
        row_start.push(0);
        for (name, entry) in rows {
            if predicates.iter().any(|seen| seen.as_ref() == name.as_str()) {
                return Err(ModelDefect::Duplicate(name.clone()));
            }
            let DynValue::Obj(weights) = entry else {
                return Err(ModelDefect::Parameters);
            };
            let row_from = feature_outcome.len();
            for (label, value) in weights {
                let Some(outcome) = outcomes.iter().position(|o| o == label) else {
                    return Err(ModelDefect::UnknownOutcome(label.clone()));
                };
                let outcome = outcome as u32;
                if feature_outcome[row_from..].contains(&outcome) {
                    return Err(ModelDefect::Duplicate(label.clone()));
                }
                let DynValue::Num(lambda) = value else {
                    return Err(ModelDefect::NonFiniteWeight(name.clone()));
                };
                if !lambda.is_finite() {
                    return Err(ModelDefect::NonFiniteWeight(name.clone()));
                }
                feature_outcome.push(outcome);
                weight.push(*lambda);
            }
            predicates.push(Arc::from(name.as_str()));
            row_start.push(feature_outcome.len() as u32);
        }
        Ok(Self::assemble(
            outcomes,
            predicates,
            row_start,
            feature_outcome,
            weight,
        ))
    }
}

/// Why a persisted model does not describe a distribution.
///
/// `#[non_exhaustive]`, because this is the payload of
/// [`MaxEntError::MalformedModel`](crate::MaxEntError::MalformedModel) and that
/// enum is `#[non_exhaustive]` too. Marking only the outer enum would have been
/// pointless: a caller who matched `MalformedModel(defect)` and then matched
/// `defect` exhaustively would still be broken by a new defect variant, so the
/// freedom bought at the outer layer is handed straight back at the inner one.
/// A new way for a model file to be malformed is a new way for a *load* to
/// fail, and a caller who cannot state today what it will do about one is
/// exactly the caller who should be writing a `_` arm.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ModelDefect {
    /// `outcomes` is absent, is not an array of strings, or is empty. A model
    /// with no outcomes has no distribution to be the maximum-entropy one.
    Outcomes,
    /// `parameters` is absent, or some entry of it is not an object of weights.
    Parameters,
    /// A weight is attached to an outcome the model does not declare. Carries
    /// the label.
    UnknownOutcome(String),
    /// A weight is absent, is not a number, or is not finite. Carries the
    /// predicate it belongs to.
    NonFiniteWeight(String),
    /// An outcome or a predicate is declared twice, so the file does not say
    /// which weight applies. Carries the repeated name.
    Duplicate(String),
}

impl std::fmt::Display for ModelDefect {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Outcomes => f.write_str("`outcomes` must be a non-empty array of labels"),
            Self::Parameters => f.write_str("`parameters` must map each predicate to its weights"),
            Self::UnknownOutcome(label) => write!(f, "weight for undeclared outcome {label:?}"),
            Self::NonFiniteWeight(predicate) => {
                write!(
                    f,
                    "predicate {predicate:?} carries a weight that is not a finite number"
                )
            }
            Self::Duplicate(name) => write!(f, "{name:?} is declared twice"),
        }
    }
}

/// Turns unnormalised scores into `p(y | x)`, in place.
///
/// The greatest score is subtracted before exponentiating, which is what keeps
/// a large score from overflowing; the shift cancels exactly in the ratio, so it
/// changes no probability. `exp(0) == 1` is always one of the terms, so the
/// divisor is at least `1` and the division is never by zero.
///
/// A score of `±∞` — reachable only from a restored model whose weights sum
/// past `f64::MAX` — is given its limit rather than propagated: `+∞` scores
/// share the whole mass equally, and an all-`-∞` vector is uniform. `NaN` cannot
/// occur, because a sum of finite values that reaches `±∞` stays there.
pub(crate) fn normalise(scores: &mut [f64]) {
    if scores.is_empty() {
        return;
    }
    let mut greatest = f64::NEG_INFINITY;
    for &s in scores.iter() {
        if s > greatest {
            greatest = s;
        }
    }
    if greatest == f64::INFINITY {
        let winners = scores.iter().filter(|s| **s == f64::INFINITY).count() as f64;
        for s in scores.iter_mut() {
            *s = if *s == f64::INFINITY {
                1.0 / winners
            } else {
                0.0
            };
        }
        return;
    }
    if greatest == f64::NEG_INFINITY {
        let uniform = 1.0 / scores.len() as f64;
        for s in scores.iter_mut() {
            *s = uniform;
        }
        return;
    }
    let mut total = 0.0;
    for s in scores.iter_mut() {
        *s = exp(*s - greatest);
        total += *s;
    }
    for s in scores.iter_mut() {
        *s /= total;
    }
}

/// `log p(outcome | x)` from unnormalised scores, computed in log space.
///
/// `log Z = greatest + log Σ exp(s − greatest)`, so the result is
/// `score − log Z`: finite whenever every score is, however small the
/// probability. `None` when a score is not finite, which is the one case where
/// the difference has no finite value.
pub(crate) fn log_probability(scores: &[f64], outcome: usize) -> Option<f64> {
    if !scores.iter().all(|s| s.is_finite()) {
        return None;
    }
    let greatest = scores.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let mut total = 0.0;
    for &s in scores {
        total += exp(s - greatest);
    }
    Some(scores[outcome] - (greatest + log(total)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two outcomes, one predicate each, with hand-chosen weights.
    fn model() -> MaxEntModel {
        MaxEntModel::assemble(
            vec!["x".to_owned(), "y".to_owned()],
            vec![Arc::from("a"), Arc::from("b")],
            vec![0, 2, 3],
            vec![0, 1, 1],
            vec![(4.0f64 / 3.0).ln(), (2.0f64 / 3.0).ln(), 2.0f64.ln()],
        )
    }

    #[test]
    fn a_distribution_is_a_probability_vector() {
        let p = model().distribution(["a"]);
        // exp(ln(4/3)) : exp(ln(2/3))  =  4 : 2  ->  2/3 and 1/3.
        assert!((p[0] - 2.0 / 3.0).abs() < 1e-12, "{p:?}");
        assert!((p[1] - 1.0 / 3.0).abs() < 1e-12, "{p:?}");
        assert!((p.iter().sum::<f64>() - 1.0).abs() < 1e-15);
    }

    #[test]
    fn an_unknown_predicate_contributes_nothing() {
        assert_eq!(model().distribution(["zzz"]), vec![0.5, 0.5]);
        assert_eq!(model().distribution(Vec::<&str>::new()), vec![0.5, 0.5]);
    }

    #[test]
    fn a_repeated_predicate_is_applied_once() {
        let once = model().distribution(["a"]);
        let twice = model().distribution(["a", "a", "a"]);
        assert_eq!(once, twice);
    }

    #[test]
    fn weights_are_reported_only_where_a_feature_exists() {
        let m = model();
        assert!((m.weight("a", "x").unwrap() - (4.0f64 / 3.0).ln()).abs() < 1e-15);
        assert_eq!(m.weight("b", "x"), None, "b was never seen with x");
        assert_eq!(m.weight("zzz", "x"), None);
        assert_eq!(m.weight("a", "zzz"), None);
        assert_eq!(m.feature_count(), 3);
        assert_eq!(m.predicates().collect::<Vec<_>>(), ["a", "b"]);
    }

    #[test]
    fn normalisation_gives_infinities_their_limit_instead_of_a_nan() {
        let mut scores = vec![f64::INFINITY, 1.0, f64::INFINITY];
        normalise(&mut scores);
        assert_eq!(scores, vec![0.5, 0.0, 0.5]);

        let mut scores = vec![f64::NEG_INFINITY, f64::NEG_INFINITY];
        normalise(&mut scores);
        assert_eq!(scores, vec![0.5, 0.5]);

        // A finite maximum with a -inf beside it: exp(-inf) is 0, not NaN.
        let mut scores = vec![0.0, f64::NEG_INFINITY];
        normalise(&mut scores);
        assert_eq!(scores, vec![1.0, 0.0]);
    }

    #[test]
    fn normalisation_is_invariant_under_a_constant_shift() {
        let base = [0.25, -1.5, 3.0];
        let mut a = base.to_vec();
        normalise(&mut a);
        let mut b: Vec<f64> = base.iter().map(|s| s + 700.0).collect();
        normalise(&mut b);
        for (x, y) in a.iter().zip(&b) {
            assert!((x - y).abs() < 1e-15, "{a:?} vs {b:?}");
        }
    }

    #[test]
    fn a_log_probability_stays_finite_where_the_probability_underflows() {
        // exp(-800) underflows to zero, so log(p) would be -inf; computing in
        // log space keeps the value.
        let scores = [0.0, -800.0];
        let p = log_probability(&scores, 1).unwrap();
        assert!(p.is_finite());
        assert!((p + 800.0).abs() < 1e-9, "{p}");
        assert_eq!(log_probability(&[f64::INFINITY, 0.0], 0), None);
    }

    #[test]
    fn a_model_round_trips_through_its_serialised_shape() {
        let m = model();
        let revived = MaxEntModel::from_value(&m.to_value()).unwrap();
        assert_eq!(revived, m);
    }

    #[test]
    fn a_damaged_model_is_refused_with_the_defect_named() {
        let mut value = model().to_value();
        let DynValue::Obj(fields) = &mut value else {
            unreachable!()
        };
        fields[0].1 = DynValue::Arr(vec![]);
        assert_eq!(
            MaxEntModel::from_value(&value).unwrap_err(),
            ModelDefect::Outcomes
        );

        let mut value = model().to_value();
        let DynValue::Obj(fields) = &mut value else {
            unreachable!()
        };
        fields[1].1 = DynValue::Obj(vec![(
            "a".to_owned(),
            DynValue::Obj(vec![("nope".to_owned(), DynValue::Num(1.0))]),
        )]);
        assert_eq!(
            MaxEntModel::from_value(&value).unwrap_err(),
            ModelDefect::UnknownOutcome("nope".to_owned())
        );

        let mut value = model().to_value();
        let DynValue::Obj(fields) = &mut value else {
            unreachable!()
        };
        fields[1].1 = DynValue::Obj(vec![(
            "a".to_owned(),
            DynValue::Obj(vec![("x".to_owned(), DynValue::Num(f64::NAN))]),
        )]);
        assert_eq!(
            MaxEntModel::from_value(&value).unwrap_err(),
            ModelDefect::NonFiniteWeight("a".to_owned())
        );

        assert_eq!(
            MaxEntModel::from_value(&DynValue::Null).unwrap_err(),
            ModelDefect::Outcomes
        );
    }

    #[test]
    fn a_reused_buffer_gives_the_same_answer_as_the_allocating_call() {
        let m = model();
        let mut buffer = vec![7.0; 9];
        m.distribution_into(["a"], &mut buffer);
        assert_eq!(buffer, m.distribution(["a"]));
        m.distribution_into(["b"], &mut buffer);
        assert_eq!(buffer, m.distribution(["b"]));
        assert_eq!(
            buffer.len(),
            m.outcomes().len(),
            "the buffer is resized, not appended to"
        );
    }

    #[test]
    fn a_log_likelihood_is_defined_only_where_it_has_a_finite_value() {
        let mut sample = Sample::new();
        sample.add("x", ["a"]);
        sample.add("y", ["b"]);
        let m = model();
        let l = m
            .log_likelihood(&sample)
            .expect("both outcomes are declared");
        assert!(l.is_finite() && l < 0.0, "{l}");
        // Hand arithmetic: p(x | {a}) = 2/3 and p(y | {b}) = 2/3, so the mean
        // log probability is ln(2/3).
        assert!((l - (2.0f64 / 3.0).ln()).abs() < 1e-12, "{l}");

        assert_eq!(m.log_likelihood(&Sample::new()), None, "an empty sample");
        let mut foreign = Sample::new();
        foreign.add("not-an-outcome", ["a"]);
        assert_eq!(m.log_likelihood(&foreign), None, "an undeclared outcome");
    }

    #[test]
    fn a_model_defect_says_what_is_wrong() {
        assert_eq!(
            ModelDefect::UnknownOutcome("q".to_owned()).to_string(),
            "weight for undeclared outcome \"q\""
        );
        assert!(ModelDefect::Outcomes.to_string().contains("non-empty"));
        assert!(
            ModelDefect::NonFiniteWeight("p".to_owned())
                .to_string()
                .contains("finite")
        );
        assert!(
            ModelDefect::Duplicate("p".to_owned())
                .to_string()
                .contains("twice")
        );
        assert!(ModelDefect::Parameters.to_string().contains("predicate"));
    }

    #[test]
    fn a_model_is_send_and_sync() {
        fn assert_shareable<T: Send + Sync>() {}
        assert_shareable::<MaxEntModel>();
    }
}
