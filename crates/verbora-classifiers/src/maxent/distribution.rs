//! The model: a weight per element, and the memo tables that cache them.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::dynval::DynValue;
use crate::maxent::MaxEntError;
use crate::maxent::context::Context;
use crate::maxent::element::Element;
use crate::maxent::feature_set::FeatureSet;
use crate::maxent::sample::Sample;
use crate::ordmap::OrderedMap;
use crate::transcendental;

/// A maximum-entropy distribution: parameters `alpha`, over a feature set and a
/// sample.
///
/// # The scores are not probabilities
///
/// `calculateAPriori` returns `∏ⱼ αⱼ^fⱼ(x)` — the normalising division is
/// commented out in the reference source. The values routinely exceed 1 and do
/// not sum to 1 (the ten-element example's `checkSum` is 11.298…). Normalising
/// "correctly" changes every score, the Kullback-Leibler trajectory, and the
/// iteration at which training stops. `entropy()` is likewise `+Σ p log p` over
/// unnormalised weights, and `KullbackLieblerDistance()` iterates duplicates and
/// so is typically negative.
#[derive(Debug)]
pub struct Distribution {
    alpha: RefCell<Vec<f64>>,
    feature_set: Rc<RefCell<FeatureSet>>,
    sample: Rc<RefCell<Sample>>,
    a_prioris_before_normalisation: RefCell<OrderedMap<f64>>,
    a_priori_normalisation_constant: Cell<f64>,
    a_posteriori_normalisation_constants: RefCell<OrderedMap<f64>>,
}

/// The reference truthiness for a memoised weight: `0` and `NaN` are recomputed.
#[inline]
fn truthy(v: f64) -> bool {
    v != 0.0 && !v.is_nan()
}

impl Distribution {
    /// A distribution over `alpha`, sharing `feature_set` and `sample`.
    pub fn new(
        alpha: Vec<f64>,
        feature_set: Rc<RefCell<FeatureSet>>,
        sample: Rc<RefCell<Sample>>,
    ) -> Self {
        Self {
            alpha: RefCell::new(alpha),
            feature_set,
            sample,
            a_prioris_before_normalisation: RefCell::new(OrderedMap::new()),
            a_priori_normalisation_constant: Cell::new(0.0),
            a_posteriori_normalisation_constants: RefCell::new(OrderedMap::new()),
        }
    }

    /// The current parameter vector.
    pub fn alpha(&self) -> Vec<f64> {
        self.alpha.borrow().clone()
    }

    /// Overwrites the parameter vector.
    pub fn set_alpha(&self, alpha: Vec<f64>) {
        *self.alpha.borrow_mut() = alpha;
    }

    /// `weight(x)`: `∏ⱼ αⱼ^fⱼ(x)`.
    ///
    /// Iterates **alpha** and indexes the feature list, not the other way round.
    /// A short alpha therefore silently ignores the surplus features (one alpha
    /// of 3 against two features gives 3² = 9), while a long one dereferences
    /// past the end.
    ///
    /// # Errors
    ///
    /// [`MaxEntError::AlphaLongerThanFeatures`] when alpha outruns the feature
    /// list — a `TypeError` in the reference.
    pub fn weight(&self, x: &Element) -> Result<f64, MaxEntError> {
        let alpha = self.alpha.borrow();
        let features = self.feature_set.borrow();
        let mut product = 1.0;
        for (j, &alpha_j) in alpha.iter().enumerate() {
            let feature = features
                .features()
                .get(j)
                .ok_or(MaxEntError::AlphaLongerThanFeatures)?;
            product *= transcendental::pow(alpha_j, feature.apply(x));
        }
        Ok(product)
    }

    /// `calculateAPriori(x)`: the memoised, **unnormalised** weight.
    ///
    /// # Errors
    ///
    /// Propagates [`Self::weight`].
    pub fn calculate_a_priori(&self, x: &Element) -> Result<f64, MaxEntError> {
        let key = x.to_key();
        let memo = self
            .a_prioris_before_normalisation
            .borrow()
            .get(&key)
            .copied();
        if let Some(v) = memo.filter(|v| truthy(*v)) {
            return Ok(v);
        }
        let w = self.weight(x)?;
        self.a_prioris_before_normalisation
            .borrow_mut()
            .insert(key, w);
        Ok(w)
    }

    /// `prepareWeights()`: recomputes every sample element's weight.
    ///
    /// Both memo tables are reset, so entries added by `getClassifications` on
    /// unseen contexts are discarded. The normalisation constant sums duplicates
    /// once per occurrence — and is then never used by anything.
    ///
    /// # Errors
    ///
    /// Propagates [`Self::weight`].
    pub fn prepare_weights(&self) -> Result<(), MaxEntError> {
        *self.a_prioris_before_normalisation.borrow_mut() = OrderedMap::new();
        self.a_priori_normalisation_constant.set(0.0);
        let mut sum = 0.0;
        let elements: Vec<Rc<Element>> = self.sample.borrow().elements().to_vec();
        for x in &elements {
            let w = self.weight(x)?;
            self.a_prioris_before_normalisation
                .borrow_mut()
                .insert(x.to_key(), w);
            sum += w;
        }
        self.a_priori_normalisation_constant.set(sum);
        Ok(())
    }

    /// `aPosterioriNormalisation(b)`: the sum of weights over all classes.
    ///
    /// # Errors
    ///
    /// Propagates [`Self::weight`].
    pub fn a_posteriori_normalisation(&self, b: &Rc<Context>) -> Result<f64, MaxEntError> {
        let classes: Vec<String> = self.sample.borrow().classes().to_vec();
        let mut sum = 0.0;
        for a in classes {
            sum += self.weight(&Element::new(a, Rc::clone(b)))?;
        }
        Ok(sum)
    }

    /// `prepareAPosterioris()`: one normalisation constant per distinct context.
    ///
    /// # Errors
    ///
    /// Propagates [`Self::weight`].
    pub fn prepare_a_posterioris(&self) -> Result<(), MaxEntError> {
        *self.a_posteriori_normalisation_constants.borrow_mut() = OrderedMap::new();
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        let elements: Vec<Rc<Element>> = self.sample.borrow().elements().to_vec();
        for element in &elements {
            let key = element.b.map_key();
            if seen.insert(key.clone()) {
                let value = self.a_posteriori_normalisation(&element.b)?;
                self.a_posteriori_normalisation_constants
                    .borrow_mut()
                    .insert(key, value);
            }
        }
        Ok(())
    }

    /// `calculateAPosteriori(x)`: a genuine conditional probability in `[0, 1]`
    /// — for a distribution whose alphas are positive.
    ///
    /// An alpha of zero drives numerator and denominator to zero together, and
    /// the division returns `Ok(NaN)`: an unorderable value, reported rather
    /// than an error, with no `log` involved. A negative alpha reaches `NaN` the
    /// other way, through `log`. Both are documented in this crate's "`NaN` is
    /// computable, not merely restorable" section; neither is filtered here,
    /// because a `NaN` a caller can see is better than a substituted number
    /// they cannot.
    ///
    /// # Errors
    ///
    /// Propagates [`Self::weight`].
    pub fn calculate_a_posteriori(&self, x: &Element) -> Result<f64, MaxEntError> {
        let key = x.to_key();
        let memo = self
            .a_prioris_before_normalisation
            .borrow()
            .get(&key)
            .copied();
        if !memo.is_some_and(truthy) {
            let w = self.weight(x)?;
            self.a_prioris_before_normalisation
                .borrow_mut()
                .insert(key.clone(), w);
        }

        let context_key = x.b.map_key();
        let memo = self
            .a_posteriori_normalisation_constants
            .borrow()
            .get(&context_key)
            .copied();
        if !memo.is_some_and(truthy) {
            let value = self.a_posteriori_normalisation(&x.b)?;
            self.a_posteriori_normalisation_constants
                .borrow_mut()
                .insert(context_key.clone(), value);
        }

        let numerator = *self
            .a_prioris_before_normalisation
            .borrow()
            .get(&key)
            .expect("just inserted");
        let denominator = *self
            .a_posteriori_normalisation_constants
            .borrow()
            .get(&context_key)
            .expect("just inserted");
        Ok(numerator / denominator)
    }

    /// `prepare()`: refresh both memo tables, weights first.
    ///
    /// # Errors
    ///
    /// Propagates [`Self::weight`].
    pub fn prepare(&self) -> Result<(), MaxEntError> {
        self.prepare_weights()?;
        self.prepare_a_posterioris()
    }

    /// `KullbackLieblerDistance()`.
    ///
    /// Iterates elements **with duplicates**, so each distinct element's term is
    /// counted once per occurrence: this is not a true KL divergence, and it is
    /// normally negative. `observedProbability(x)` is evaluated twice per
    /// element, once inside the log and once outside, exactly as written.
    ///
    /// # Errors
    ///
    /// Propagates [`Self::weight`].
    pub fn kullback_liebler_distance(&self) -> Result<f64, MaxEntError> {
        let elements: Vec<Rc<Element>> = self.sample.borrow().elements().to_vec();
        let mut sum = 0.0;
        for x in &elements {
            let observed = self.sample.borrow().observed_probability(x);
            let a_priori = self.calculate_a_priori(x)?;
            sum += observed * transcendental::log(observed / a_priori);
        }
        Ok(sum)
    }

    /// `logLikelihood()`.
    ///
    /// # Errors
    ///
    /// Propagates [`Self::weight`].
    pub fn log_likelihood(&self) -> Result<f64, MaxEntError> {
        let elements: Vec<Rc<Element>> = self.sample.borrow().elements().to_vec();
        let mut sum = 0.0;
        for x in &elements {
            let observed = self.sample.borrow().observed_probability(x);
            sum += observed * transcendental::log(self.calculate_a_priori(x)?);
        }
        Ok(sum)
    }

    /// `entropy()`: `+Σ p log p` over unnormalised weights, so it is not an
    /// entropy and can be large and positive.
    ///
    /// # Errors
    ///
    /// Propagates [`Self::weight`].
    pub fn entropy(&self) -> Result<f64, MaxEntError> {
        let elements: Vec<Rc<Element>> = self.sample.borrow().elements().to_vec();
        let mut sum = 0.0;
        for x in &elements {
            let p = self.calculate_a_priori(x)?;
            sum += p * transcendental::log(p);
        }
        Ok(sum)
    }

    /// `checkSum()`: the total weight over the sample, duplicates included.
    ///
    /// # Errors
    ///
    /// Propagates [`Self::weight`].
    pub fn check_sum(&self) -> Result<f64, MaxEntError> {
        let elements: Vec<Rc<Element>> = self.sample.borrow().elements().to_vec();
        let mut sum = 0.0;
        for x in &elements {
            sum += self.calculate_a_priori(x)?;
        }
        Ok(sum)
    }

    /// The a-priori weight memo table.
    pub fn a_prioris_before_normalisation(&self) -> OrderedMap<f64> {
        self.a_prioris_before_normalisation.borrow().clone()
    }

    /// The sum of the sample's weights, computed by `prepare()` and then never
    /// read by anything.
    pub fn a_priori_normalisation_constant(&self) -> f64 {
        self.a_priori_normalisation_constant.get()
    }

    /// The per-context normalisation constants.
    pub fn a_posteriori_normalisation_constants(&self) -> OrderedMap<f64> {
        self.a_posteriori_normalisation_constants.borrow().clone()
    }

    /// The serialised shape `save()` writes.
    pub fn to_value(&self) -> DynValue {
        DynValue::Obj(vec![
            (
                "alpha".to_owned(),
                DynValue::Arr(
                    self.alpha
                        .borrow()
                        .iter()
                        .copied()
                        .map(DynValue::Num)
                        .collect(),
                ),
            ),
            (
                "featureSet".to_owned(),
                self.feature_set.borrow().to_value(),
            ),
            ("sample".to_owned(), self.sample.borrow().to_value()),
            (
                "aPriorisBeforeNormalisation".to_owned(),
                map_to_value(&self.a_prioris_before_normalisation.borrow()),
            ),
            (
                "aPrioriNormalisationConstant".to_owned(),
                DynValue::Num(self.a_priori_normalisation_constant.get()),
            ),
            (
                "aPosterioriNormalisationConstants".to_owned(),
                map_to_value(&self.a_posteriori_normalisation_constants.borrow()),
            ),
        ])
    }
}

fn map_to_value(map: &OrderedMap<f64>) -> DynValue {
    DynValue::Obj(
        map.ordered_entries()
            .into_iter()
            .map(|(k, v)| (k.to_owned(), DynValue::Num(*v)))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::maxent::feature::{Feature, FeatureFn};

    fn constant(v: f64) -> FeatureFn {
        Rc::new(move |_: &Element| v)
    }

    fn setup(alpha: Vec<f64>, feature_values: &[f64]) -> Distribution {
        let mut fs = FeatureSet::new();
        for (i, v) in feature_values.iter().enumerate() {
            fs.add_feature(Rc::new(Feature::new(constant(*v), format!("f{i}"), vec![])));
        }
        Distribution::new(
            alpha,
            Rc::new(RefCell::new(fs)),
            Rc::new(RefCell::new(Sample::new())),
        )
    }

    fn element() -> Element {
        Element::new("x", Rc::new(Context::of_str("0")))
    }

    #[test]
    fn a_short_alpha_ignores_the_surplus_features() {
        // One alpha of 3 against two features whose value is 2 -> 3^2 == 9.
        let d = setup(vec![3.0], &[2.0, 5.0]);
        assert_eq!(d.weight(&element()).unwrap(), 9.0);
    }

    #[test]
    fn an_empty_alpha_weighs_everything_at_one() {
        let d = setup(vec![], &[1.0, 1.0]);
        assert_eq!(d.weight(&element()).unwrap(), 1.0);
    }

    #[test]
    fn a_long_alpha_is_a_type_error() {
        let d = setup(vec![1.0, 1.0, 1.0], &[1.0]);
        assert_eq!(
            d.weight(&element()),
            Err(MaxEntError::AlphaLongerThanFeatures)
        );
    }

    #[test]
    fn nan_alpha_with_a_zero_exponent_still_weighs_one() {
        // Math.pow(NaN, 0) === 1, which is what keeps degenerate models finite.
        let d = setup(vec![f64::NAN], &[0.0]);
        assert_eq!(d.weight(&element()).unwrap(), 1.0);
    }
}
