//! A feature function and the two expectations the scaling algorithm compares.

use std::cell::Cell;
use std::fmt;
use std::rc::Rc;

use crate::dynval::DynValue;
use crate::maxent::MaxEntError;
use crate::maxent::context::Context;
use crate::maxent::distribution::Distribution;
use crate::maxent::element::Element;
use crate::maxent::sample::Sample;

/// A feature function `f(x) -> number`.
///
/// Shipped features return 0 or 1; the correction feature returns a small
/// non-negative integer.
pub type FeatureFn = Rc<dyn Fn(&Element) -> f64>;

/// One feature: a predicate over elements, plus the identity used to deduplicate
/// it inside a [`FeatureSet`](crate::FeatureSet).
pub struct Feature {
    evaluate: FeatureFn,
    /// The feature family's name, e.g. `"wordFeature"`.
    pub name: String,
    /// The parameters that specialise it.
    pub parameters: Vec<String>,
    /// `parameters.join("|")` — half of the deduplication key.
    pub parameters_key: String,
    /// The memoised observed expectation.
    ///
    /// `None` means the field has never been assigned — in the reference it does
    /// not exist yet, and `JSON.stringify` omits it. A `Some(0.0)` or
    /// `Some(NaN)` is a value that *was* assigned but is falsy, so the
    /// `if (this.observedExpect) return …` guard misses it and the sum is
    /// recomputed on every call. The two states are distinguishable in the
    /// saved file, so they cannot be collapsed.
    observed_expect: Cell<Option<f64>>,
}

impl fmt::Debug for Feature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Feature")
            .field("name", &self.name)
            .field("parametersKey", &self.parameters_key)
            .field("observedExpect", &self.observed_expect.get())
            .finish_non_exhaustive()
    }
}

impl Feature {
    /// A feature named `name`, specialised by `parameters`.
    ///
    /// `parametersKey` is `parameters.join("|")`. The reference builds it as
    /// `tmp += par + '|'` and then trims the trailing separator with
    /// `substr(0, len - 1)`; for an empty parameter list that is
    /// `''.substr(0, -1)`, which is `''` — where Rust's `&s[..s.len()-1]` would
    /// panic.
    pub fn new(evaluate: FeatureFn, name: impl Into<String>, parameters: Vec<String>) -> Self {
        let parameters_key = parameters.join("|");
        Self {
            evaluate,
            name: name.into(),
            parameters,
            parameters_key,
            observed_expect: Cell::new(None),
        }
    }

    /// The flat deduplication key, `name + " | " + parametersKey`.
    ///
    /// Both separators are unescaped, so `("a | b", ["c"])` and
    /// `("a", ["b | c"])` collide — verified against the reference, which
    /// rejects the second of the pair.
    pub fn dedup_key(&self) -> String {
        format!("{} | {}", self.name, self.parameters_key)
    }

    /// `apply(x)`: evaluates the feature.
    pub fn apply(&self, x: &Element) -> f64 {
        (self.evaluate)(x)
    }

    /// The memoised observed expectation, or `None` if the field has never been
    /// assigned.
    ///
    /// A `Some(0.0)` or `Some(NaN)` is a real assignment that the reference's
    /// truthiness guard nonetheless fails to hit, so the value is recomputed on
    /// every call — but it is still present in `save()` output.
    pub fn observed_expect(&self) -> Option<f64> {
        self.observed_expect.get()
    }

    /// `observedExpectation(sample)`: the sample average of this feature.
    ///
    /// Memoised into a field that is **never invalidated**. Add an element to
    /// the sample and retrain, and this still reports the old value — verified
    /// against the reference, which keeps reporting `[0.3, 0.3, 0.4]` for an
    /// eleven-element sample.
    ///
    /// The sum runs over `sample.elements` in order, duplicates included.
    pub fn observed_expectation(&self, sample: &Sample) -> f64 {
        // `if (this.observedExpect) return this.observedExpect` — truthiness,
        // so a memoised 0 or NaN does not short-circuit.
        if let Some(memo) = self.observed_expect.get()
            && memo != 0.0
            && !memo.is_nan()
        {
            return memo;
        }
        let n = sample.size() as f64;
        let mut sum = 0.0;
        for x in sample.elements() {
            sum += self.apply(x);
        }
        // An empty sample gives 0/0 = NaN, which is falsy and so never sticks.
        let value = sum / n;
        self.observed_expect.set(Some(value));
        value
    }

    /// `expectationApprox(p, sample)`: the model's expectation of this feature.
    ///
    /// Distinct contexts are visited in the order of their **first occurrence**
    /// among the sample elements, and for each one the classes are visited in
    /// `sample.classes` order. Both orders fix the floating-point accumulation
    /// and are therefore load-bearing.
    ///
    /// `observedProbabilityOfContext(bi)` is recomputed inside the inner loop,
    /// exactly as the reference does — hoisting it would be an algebraically
    /// identical but differently rounded expression.
    ///
    /// # Errors
    ///
    /// Propagates [`MaxEntError::AlphaLongerThanFeatures`] from `weight`.
    pub fn expectation_approx(
        &self,
        p: &Distribution,
        sample: &Sample,
    ) -> Result<f64, MaxEntError> {
        let mut sum = 0.0;
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        let classes = sample.classes().to_vec();
        for element in sample.elements() {
            let bi = Rc::clone(&element.b);
            let key = bi.map_key();
            if seen.insert(key) {
                for a in &classes {
                    let x = Element::new(a.clone(), Rc::clone(&bi));
                    sum += sample.observed_probability_of_context(&bi)
                        * p.calculate_a_posteriori(&x)?
                        * self.apply(&x);
                }
            }
        }
        Ok(sum)
    }

    /// `expectation(p, A, B)`: the direct, intractable form.
    ///
    /// Public in the reference but unused by it. Note that `calculateAPriori`
    /// returns an unnormalised weight, so this is not a probability-weighted
    /// expectation.
    ///
    /// # Errors
    ///
    /// Propagates [`MaxEntError::AlphaLongerThanFeatures`] from `weight`.
    pub fn expectation(
        &self,
        p: &Distribution,
        classes: &[String],
        contexts: &[Rc<Context>],
    ) -> Result<f64, MaxEntError> {
        let mut sum = 0.0;
        for a in classes {
            for b in contexts {
                let x = Element::new(a.clone(), Rc::clone(b));
                sum += p.calculate_a_priori(&x)? * self.apply(&x);
            }
        }
        Ok(sum)
    }

    /// The serialised shape `save()` writes.
    ///
    /// `evaluate` is a function and is dropped by `JSON.stringify`;
    /// `observedExpect` is present only once it has been memoised.
    pub fn to_value(&self) -> DynValue {
        let mut fields = vec![
            ("name".to_owned(), DynValue::Str(self.name.clone())),
            (
                "parameters".to_owned(),
                DynValue::Arr(self.parameters.iter().cloned().map(DynValue::Str).collect()),
            ),
            (
                "parametersKey".to_owned(),
                DynValue::Str(self.parameters_key.clone()),
            ),
        ];
        if let Some(v) = self.observed_expect() {
            fields.push(("observedExpect".to_owned(), DynValue::Num(v)));
        }
        DynValue::Obj(fields)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zero() -> FeatureFn {
        Rc::new(|_: &Element| 0.0)
    }

    #[test]
    fn parameters_key_joins_with_a_pipe() {
        for (params, want) in [
            (vec![], ""),
            (vec!["abc"], "abc"),
            (vec!["a", "b", "c"], "a|b|c"),
            (vec![""], ""),
            (vec!["", ""], "|"),
            (vec!["0", "the", "0", "DT"], "0|the|0|DT"),
        ] {
            let f = Feature::new(
                zero(),
                "n",
                params.iter().map(|s| (*s).to_owned()).collect(),
            );
            assert_eq!(f.parameters_key, want, "{params:?}");
        }
    }

    #[test]
    fn dedup_keys_can_collide() {
        let a = Feature::new(zero(), "a | b", vec!["c".to_owned()]);
        let b = Feature::new(zero(), "a", vec!["b | c".to_owned()]);
        assert_eq!(a.dedup_key(), b.dedup_key());
        assert_eq!(a.dedup_key(), "a | b | c");
    }

    #[test]
    fn empty_parameters_give_a_trailing_space_key() {
        let f = Feature::new(zero(), "Correction feature", vec![]);
        assert_eq!(f.dedup_key(), "Correction feature | ");
    }

    #[test]
    fn a_falsy_expectation_is_stored_but_never_short_circuits() {
        let sample = Sample::new();
        let f = Feature::new(zero(), "n", vec![]);
        assert_eq!(f.observed_expect(), None, "the field starts absent");
        // An empty sample gives 0/0 = NaN. The field is assigned — and so is
        // serialised — but the truthiness guard will recompute it every time.
        assert!(f.observed_expectation(&sample).is_nan());
        assert!(f.observed_expect().is_some_and(f64::is_nan));
    }
}
