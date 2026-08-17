//! Generalised iterative scaling: the training loop.

use std::cell::RefCell;
use std::rc::Rc;

use crate::dynval::DynValue;
use crate::maxent::MaxEntError;
use crate::maxent::distribution::Distribution;
use crate::maxent::element::Element;
use crate::maxent::feature::Feature;
use crate::maxent::feature_set::FeatureSet;
use crate::maxent::sample::Sample;
use crate::ordmap::OrderedMap;
use crate::transcendental;

/// The scaler state the correction feature closes over.
///
/// It is behind an `Rc<RefCell<…>>` because the reference closure captures the
/// scaler itself and **outlives it**: the feature is appended to the shared
/// feature set and stays there, so a second training run — whose own correction
/// feature `addFeature` rejects as a duplicate — still evaluates against the
/// first run's `C` and `featureSums`.
#[derive(Debug, Default, Clone)]
pub struct ScalerState {
    /// The maximum total feature activation over the sample.
    pub c: f64,
    /// Per-element total feature activation, keyed by `Element.toString()`.
    pub feature_sums: OrderedMap<f64>,
}

/// The generalised iterative scaling trainer.
#[derive(Debug)]
pub struct GISScaler {
    feature_set: Rc<RefCell<FeatureSet>>,
    sample: Rc<RefCell<Sample>>,
    state: Rc<RefCell<ScalerState>>,
    iteration: i64,
    improvement: f64,
}

impl GISScaler {
    /// A scaler over a shared feature set and sample. Both are mutated by
    /// [`Self::run`].
    pub fn new(feature_set: Rc<RefCell<FeatureSet>>, sample: Rc<RefCell<Sample>>) -> Self {
        Self {
            feature_set,
            sample,
            state: Rc::new(RefCell::new(ScalerState::default())),
            iteration: 0,
            improvement: 0.0,
        }
    }

    /// Iterations performed by the last [`Self::run`]. Always at least 1.
    pub fn iteration(&self) -> i64 {
        self.iteration
    }

    /// The last iteration's improvement in Kullback-Leibler distance.
    pub fn improvement(&self) -> f64 {
        self.improvement
    }

    /// `C`, the exponent denominator.
    pub fn c(&self) -> f64 {
        self.state.borrow().c
    }

    /// Per-element feature sums.
    pub fn feature_sums(&self) -> OrderedMap<f64> {
        self.state.borrow().feature_sums.clone()
    }

    /// `calculateMaxSumOfFeatures()`: sets `C` and `featureSums`.
    ///
    /// Returns whether a correction feature is needed — that is, whether the
    /// per-element feature sums are not all equal.
    ///
    /// Two details are faithfully odd. The "seen this element?" guard is a
    /// truthiness test on the stored sum, so an element whose features all fire
    /// zero is reprocessed on every occurrence; and the emptiness case compares
    /// `undefined !== undefined`, which is `false`, so an empty sample needs no
    /// correction feature and leaves `C` at 0 — making `1/C` infinite
    /// downstream.
    pub fn calculate_max_sum_of_features(&self) -> bool {
        {
            let mut state = self.state.borrow_mut();
            state.c = 0.0;
            state.feature_sums = OrderedMap::new();
        }

        let elements: Vec<Rc<Element>> = self.sample.borrow().elements().to_vec();
        // Cloned handles rather than a held borrow: a correction feature added
        // by an earlier run of *this* scaler reads the same state cell, and
        // holding a mutable borrow across `apply` would deadlock the RefCell
        // where the reference merely reads half-updated values.
        let features: Vec<Rc<Feature>> = self.feature_set.borrow().features().to_vec();
        let mut min = f64::NAN;
        let mut max = f64::NAN;
        let mut any = false;

        for x in &elements {
            let key = x.to_key();
            let seen = self
                .state
                .borrow()
                .feature_sums
                .get(&key)
                .copied()
                .is_some_and(|v| v != 0.0 && !v.is_nan());
            if seen {
                continue;
            }
            let mut sum = 0.0;
            for f in &features {
                sum += f.apply(x);
            }
            {
                let mut state = self.state.borrow_mut();
                if sum > state.c {
                    state.c = sum;
                }
                state.feature_sums.insert(key, sum);
            }
            // The reference collects every sum, sorts numerically and compares
            // the ends; tracking min and max is the same answer in one pass.
            if !any || sum < min {
                min = sum;
            }
            if !any || sum > max {
                max = sum;
            }
            any = true;
        }

        any && min != max
    }

    /// `addCorrectionFeature()`: appends `C - featureSum(x)` if needed.
    ///
    /// The closure reads the scaler's live state, and its existence check is a
    /// strict `!== undefined` — unlike every other guard in the module — so a
    /// stored sum of exactly 0 correctly yields `C`, while an **unseen** element
    /// yields 0 rather than `C`. That is why an unfamiliar context weighs 1 for
    /// every class and `classify()` returns the empty string for it.
    pub fn add_correction_feature(&self) {
        if !self.calculate_max_sum_of_features() {
            return;
        }
        let state = Rc::clone(&self.state);
        let evaluate = Rc::new(move |x: &Element| {
            let state = state.borrow();
            match state.feature_sums.get(&x.to_key()) {
                Some(sum) => state.c - sum,
                None => 0.0,
            }
        });
        self.feature_set
            .borrow_mut()
            .add_feature(Rc::new(Feature::new(
                evaluate,
                "Correction feature",
                vec![],
            )));
    }

    /// `run(maxIterations, minImprovement)`: trains and returns the model.
    ///
    /// The loop is a `do…while`, so **at least one iteration always runs** —
    /// `max_iterations` of 0 or a negative number still produces the
    /// post-iteration-1 parameters, where a Rust `for _ in 0..max` would leave
    /// alpha at all ones.
    ///
    /// The update is `alpha[i] * pow(observed / approximated, 1 / C)` with no
    /// guards: `0/0` gives `NaN` (single-class samples do this), and `C == 0`
    /// gives an infinite exponent.
    ///
    /// # Errors
    ///
    /// Propagates [`MaxEntError::AlphaLongerThanFeatures`].
    pub fn run(
        &mut self,
        max_iterations: i64,
        min_improvement: f64,
    ) -> Result<Rc<Distribution>, MaxEntError> {
        self.iteration = 0;
        self.improvement = 0.0;

        self.add_correction_feature();

        // Sized after the correction feature was (or was not) added.
        let alpha = vec![1.0; self.feature_set.borrow().size()];
        let p = Rc::new(Distribution::new(
            alpha,
            Rc::clone(&self.feature_set),
            Rc::clone(&self.sample),
        ));
        p.prepare()?;
        let mut kl_distance = p.kullback_liebler_distance()?;

        // `new Array(size)` of holes, assigned into by index each iteration.
        let mut new_alpha = vec![1.0; self.feature_set.borrow().size()];

        loop {
            let size = self.feature_set.borrow().size();
            if new_alpha.len() < size {
                new_alpha.resize(size, 1.0);
            }
            let c = self.state.borrow().c;
            // `p.alpha` is not written until the whole sweep is done, so reading
            // it once is the same numbers the reference re-reads each time.
            let alpha = p.alpha();
            for (i, slot) in new_alpha.iter_mut().enumerate().take(size) {
                let feature = Rc::clone(&self.feature_set.borrow().features()[i]);
                let sample = Rc::clone(&self.sample);
                let observed = feature.observed_expectation(&sample.borrow());
                let approximated = feature.expectation_approx(&p, &sample.borrow())?;
                *slot = alpha[i] * transcendental::pow(observed / approximated, 1.0 / c);
            }
            p.set_alpha(new_alpha[..size].to_vec());
            p.prepare()?;

            self.iteration += 1;
            let new_kl = p.kullback_liebler_distance()?;
            self.improvement = kl_distance - new_kl;
            kl_distance = new_kl;

            if !(self.iteration < max_iterations && self.improvement > min_improvement) {
                break;
            }
        }

        Ok(p)
    }

    /// The serialised shape `save()` writes for `classifier.scaler`.
    pub fn to_value(&self) -> DynValue {
        let state = self.state.borrow();
        DynValue::Obj(vec![
            (
                "featureSet".to_owned(),
                self.feature_set.borrow().to_value(),
            ),
            ("sample".to_owned(), self.sample.borrow().to_value()),
            ("iteration".to_owned(), DynValue::Num(self.iteration as f64)),
            ("improvement".to_owned(), DynValue::Num(self.improvement)),
            ("C".to_owned(), DynValue::Num(state.c)),
            (
                "featureSums".to_owned(),
                DynValue::Obj(
                    state
                        .feature_sums
                        .ordered_entries()
                        .into_iter()
                        .map(|(k, v)| (k.to_owned(), DynValue::Num(*v)))
                        .collect(),
                ),
            ),
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SEElement;
    use crate::maxent::context::Context;

    fn reference() -> (Rc<RefCell<FeatureSet>>, Rc<RefCell<Sample>>) {
        let mut sample = Sample::new();
        let zero = Rc::new(Context::of_str("0"));
        let one = Rc::new(Context::of_str("1"));
        for _ in 0..3 {
            sample.add_element(SEElement::new("x", Rc::clone(&zero)));
        }
        for _ in 0..3 {
            sample.add_element(SEElement::new("y", Rc::clone(&zero)));
        }
        sample.add_element(SEElement::new("x", Rc::clone(&one)));
        for _ in 0..3 {
            sample.add_element(SEElement::new("y", Rc::clone(&one)));
        }
        let mut fs = FeatureSet::new();
        sample.generate_features(&mut fs).unwrap();
        (Rc::new(RefCell::new(fs)), Rc::new(RefCell::new(sample)))
    }

    #[test]
    fn max_sum_matches_the_reference_golden_values() {
        let (fs, sample) = reference();
        let scaler = GISScaler::new(fs, sample);
        assert!(scaler.calculate_max_sum_of_features());
        assert_eq!(scaler.c(), 1.0);
        let sums = scaler.feature_sums();
        assert_eq!(sums.get("x\"0\""), Some(&1.0));
        assert_eq!(sums.get("y\"0\""), Some(&0.0));
        assert_eq!(sums.get("x\"1\""), Some(&0.0));
        assert_eq!(sums.get("y\"1\""), Some(&1.0));
    }

    #[test]
    fn training_reproduces_the_recorded_alpha_trajectory() {
        let (fs, sample) = reference();
        let mut scaler = GISScaler::new(Rc::clone(&fs), sample);
        let p = scaler.run(20, 0.01).unwrap();
        assert_eq!(scaler.iteration(), 3);
        assert_eq!(scaler.improvement(), 0.008_269_762_368_828_815);
        assert_eq!(
            p.alpha(),
            vec![
                0.844_285_714_285_714_2,
                1.885_178_571_428_571_1,
                0.777_505_117_061_545
            ]
        );
        assert_eq!(
            p.kullback_liebler_distance().unwrap(),
            -3.647_602_435_924_899
        );
        assert_eq!(p.log_likelihood().unwrap(), 0.166_617_354_945_467_34);
        assert_eq!(p.entropy().unwrap(), 2.374_331_367_488_561);
        assert_eq!(p.check_sum().unwrap(), 11.298_413_325_389_037);
    }

    #[test]
    fn zero_iterations_still_performs_one() {
        let (fs, sample) = reference();
        let mut scaler = GISScaler::new(fs, sample);
        let p = scaler.run(0, 0.01).unwrap();
        assert_eq!(scaler.iteration(), 1);
        // The post-iteration-1 values, not the all-ones initialisation.
        assert_eq!(p.alpha(), vec![1.0, 1.499_999_999_999_999_8, 0.8]);
    }

    #[test]
    fn an_empty_sample_needs_no_correction_feature() {
        let fs = Rc::new(RefCell::new(FeatureSet::new()));
        let sample = Rc::new(RefCell::new(Sample::new()));
        let mut scaler = GISScaler::new(Rc::clone(&fs), sample);
        let p = scaler.run(20, 0.01).unwrap();
        assert_eq!(scaler.c(), 0.0);
        assert_eq!(scaler.iteration(), 1);
        assert_eq!(scaler.improvement(), 0.0);
        assert!(p.alpha().is_empty());
        assert_eq!(fs.borrow().size(), 0);
    }
}
