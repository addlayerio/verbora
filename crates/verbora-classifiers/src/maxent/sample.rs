//! The training sample: observed elements and their frequencies.

use std::collections::HashMap;
use std::rc::Rc;

use crate::dynval::DynValue;
use crate::maxent::MaxEntError;
use crate::maxent::context::Context;
use crate::maxent::element::Element;
use crate::maxent::feature_set::FeatureSet;
use crate::ordmap::OrderedMap;

/// A collection of observed elements, with class and frequency indices.
///
/// Elements are kept **with duplicates and in insertion order**: every
/// summation in the algorithm iterates this vector, so its order is what fixes
/// the floating-point accumulation. Classes are likewise a `Vec` in
/// first-appearance order, never a set — `classify()` breaks ties among equal
/// scores by that order.
#[derive(Debug, Default)]
pub struct Sample {
    elements: Vec<Rc<Element>>,
    frequency_of_context: OrderedMap<f64>,
    frequency: OrderedMap<f64>,
    classes: Vec<String>,
    /// A side index for the `classes.indexOf(...)` membership test. Purely a
    /// speed-up; `classes` remains the observable order.
    class_index: HashMap<String, usize>,
}

impl Sample {
    /// An empty sample.
    pub fn new() -> Self {
        Self::default()
    }

    /// `new Sample(elements)`.
    ///
    /// # Errors
    ///
    /// [`MaxEntError::SampleAnalyseIsBroken`] for any **non-empty** slice. The
    /// constructor calls `analyse()`, whose `forEach` callback is a plain
    /// `function` with no `thisArg` in a `'use strict'` module, so `this` is
    /// `undefined` and the first statement throws on the first element. Only
    /// `new Sample([])` is usable, which is exactly the form
    /// `MECorpus.generateSample` uses.
    pub fn with_elements(elements: &[Rc<Element>]) -> Result<Self, MaxEntError> {
        if elements.is_empty() {
            Ok(Self::new())
        } else {
            Err(MaxEntError::SampleAnalyseIsBroken)
        }
    }

    /// `addElement(x)`: records the element and updates the indices.
    pub fn add_element(&mut self, x: impl Into<Rc<Element>>) {
        let x: Rc<Element> = x.into();
        let context_key = x.b.map_key();
        let element_key = x.to_key();
        let class = x.a.clone();
        self.elements.push(x);

        bump(&mut self.frequency_of_context, &context_key);
        bump(&mut self.frequency, &element_key);

        if !self.class_index.contains_key(&class) {
            self.class_index.insert(class.clone(), self.classes.len());
            self.classes.push(class);
        }
    }

    /// The elements, in insertion order, duplicates included.
    pub fn elements(&self) -> &[Rc<Element>] {
        &self.elements
    }

    /// `size()`.
    pub fn size(&self) -> usize {
        self.elements.len()
    }

    /// Whether the sample is empty.
    pub fn is_empty(&self) -> bool {
        self.elements.is_empty()
    }

    /// `getClasses()`: class labels in first-appearance order.
    pub fn classes(&self) -> &[String] {
        &self.classes
    }

    /// Per-element observation counts, keyed by `Element.toString()`.
    pub fn frequency(&self) -> &OrderedMap<f64> {
        &self.frequency
    }

    /// Per-context observation counts, keyed by `Context.toString()`.
    pub fn frequency_of_context(&self) -> &OrderedMap<f64> {
        &self.frequency_of_context
    }

    /// `observedProbabilityOfContext(context)`.
    ///
    /// The guard is a truthiness test, so an unknown context — and a count that
    /// somehow reached zero — yields `0` rather than dividing.
    pub fn observed_probability_of_context(&self, context: &Context) -> f64 {
        match self.frequency_of_context.get(&context.map_key()) {
            Some(f) if *f != 0.0 && !f.is_nan() => f / self.elements.len() as f64,
            _ => 0.0,
        }
    }

    /// `observedProbability(x)`.
    pub fn observed_probability(&self, x: &Element) -> f64 {
        match self.frequency.get(&x.to_key()) {
            Some(f) if *f != 0.0 && !f.is_nan() => f / self.elements.len() as f64,
            _ => 0.0,
        }
    }

    /// `generateFeatures(featureSet)`: lets every element contribute.
    ///
    /// Walks all elements, duplicates included; the feature set absorbs the
    /// repeats.
    ///
    /// # Errors
    ///
    /// [`MaxEntError::ElementCannotGenerateFeatures`] if any element is a base
    /// [`Element`], which has no `generateFeatures`.
    pub fn generate_features(&self, feature_set: &mut FeatureSet) -> Result<(), MaxEntError> {
        for x in &self.elements {
            x.generate_features(feature_set)?;
        }
        Ok(())
    }

    /// The serialised shape `save()` writes.
    pub fn to_value(&self) -> DynValue {
        DynValue::Obj(vec![
            (
                "frequencyOfContext".to_owned(),
                map_to_value(&self.frequency_of_context),
            ),
            ("frequency".to_owned(), map_to_value(&self.frequency)),
            (
                "classes".to_owned(),
                DynValue::Arr(self.classes.iter().cloned().map(DynValue::Str).collect()),
            ),
            (
                "elements".to_owned(),
                DynValue::Arr(self.elements.iter().map(|e| e.to_value()).collect()),
            ),
        ])
    }

    /// Rebuilds a sample from `save()` output, as `Sample.prototype.load` does.
    ///
    /// The stored `frequency`, `frequencyOfContext` and `classes` are
    /// **ignored** and rebuilt from the elements, and each element is revived
    /// through the caller's element constructor — which is why the reference
    /// requires an `ElementClass` argument.
    pub fn from_value(
        value: &DynValue,
        mut revive: impl FnMut(&str, Rc<Context>) -> Rc<Element>,
    ) -> Self {
        let mut sample = Self::new();
        if let Some(DynValue::Arr(elements)) = value.get("elements") {
            for element in elements {
                let a = element.get("a").map(DynValue::to_text).unwrap_or_default();
                let data = element
                    .get("b")
                    .and_then(|b| b.get("data"))
                    .cloned()
                    .unwrap_or(DynValue::Undefined);
                sample.add_element(revive(&a, Rc::new(Context::new(data))));
            }
        }
        sample
    }
}

/// `if (!map[key]) map[key] = 0; map[key]++`.
fn bump(map: &mut OrderedMap<f64>, key: &str) {
    let current = map
        .get(key)
        .copied()
        .filter(|v| *v != 0.0 && !v.is_nan())
        .unwrap_or(0.0);
    map.insert(key.to_owned(), current + 1.0);
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
    use crate::SEElement;

    fn reference_sample() -> Sample {
        let mut s = Sample::new();
        let zero = Rc::new(Context::of_str("0"));
        let one = Rc::new(Context::of_str("1"));
        for _ in 0..3 {
            s.add_element(SEElement::new("x", Rc::clone(&zero)));
        }
        for _ in 0..3 {
            s.add_element(SEElement::new("y", Rc::clone(&zero)));
        }
        s.add_element(SEElement::new("x", Rc::clone(&one)));
        for _ in 0..3 {
            s.add_element(SEElement::new("y", Rc::clone(&one)));
        }
        s
    }

    #[test]
    fn frequencies_match_the_reference_golden_values() {
        let s = reference_sample();
        assert_eq!(s.size(), 10);
        assert_eq!(s.classes(), ["x", "y"]);
        assert_eq!(s.frequency_of_context().get("\"0\""), Some(&6.0));
        assert_eq!(s.frequency_of_context().get("\"1\""), Some(&4.0));
        assert_eq!(s.frequency().get("x\"0\""), Some(&3.0));
        assert_eq!(s.frequency().get("y\"1\""), Some(&3.0));
        assert_eq!(s.frequency().get("x\"1\""), Some(&1.0));
    }

    #[test]
    fn observed_probabilities_match() {
        let s = reference_sample();
        let x0 = SEElement::new("x", Rc::new(Context::of_str("0")));
        assert_eq!(s.observed_probability(&x0), 0.3);
        assert_eq!(
            s.observed_probability_of_context(&Context::of_str("0")),
            0.6
        );
        assert_eq!(
            s.observed_probability_of_context(&Context::of_str("unseen")),
            0.0
        );
    }

    #[test]
    fn a_non_empty_constructor_argument_is_rejected() {
        let e = Rc::new(SEElement::new("x", Rc::new(Context::of_str("0"))));
        assert_eq!(
            Sample::with_elements(&[e]).err(),
            Some(MaxEntError::SampleAnalyseIsBroken)
        );
        assert!(Sample::with_elements(&[]).is_ok());
    }

    #[test]
    fn an_empty_sample_never_divides_by_zero() {
        let s = Sample::new();
        assert_eq!(
            s.observed_probability_of_context(&Context::of_str("0")),
            0.0
        );
    }
}
