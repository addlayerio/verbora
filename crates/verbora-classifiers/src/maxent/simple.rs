//! `SEElement`: the two-feature toy model the reference's own spec is built on.

use std::rc::Rc;

use crate::dynval::DynValue;
use crate::maxent::MaxEntError;
use crate::maxent::context::Context;
use crate::maxent::element::{Element, ElementKind};
use crate::maxent::feature::Feature;
use crate::maxent::feature_set::FeatureSet;

/// Constructs elements that generate the SimpleExample feature pair.
///
/// ```
/// use std::rc::Rc;
/// use verbora_classifiers::{Context, FeatureSet, SEElement};
///
/// let mut features = FeatureSet::new();
/// SEElement::new("x", Rc::new(Context::of_str("0")))
///     .generate_features(&mut features)
///     .unwrap();
/// assert_eq!(features.pretty_print(), "isZero | 0\nisOne | 1\n");
/// ```
#[derive(Debug, Clone, Copy)]
pub struct SEElement;

impl SEElement {
    /// An element whose `generateFeatures` adds `isZero` and `isOne`.
    ///
    /// Returns an [`Element`], not an `SEElement`: in the reference `SEElement`
    /// *is* an `Element` — a subclass adding one method — and modelling that as
    /// a separate Rust type would fork every API that takes an element.
    #[allow(clippy::new_ret_no_self)]
    pub fn new(a: impl Into<String>, b: Rc<Context>) -> Element {
        Element::with_kind(a, b, ElementKind::Simple)
    }
}

/// `SEElement.prototype.generateFeatures`.
///
/// Adds exactly two features **regardless of this element's own class and
/// context** — the closures capture nothing from `this`, so any SEElement in the
/// sample contributes the same pair and the whole sample yields two features.
///
/// Both comparisons are strict `===`, so a numeric context payload of `0` does
/// *not* satisfy `x.b.data === '0'`: a sample built over numbers scores zero on
/// every feature.
pub(crate) fn generate(
    _element: &Element,
    feature_set: &mut FeatureSet,
) -> Result<(), MaxEntError> {
    let is_zero = Rc::new(|x: &Element| {
        if x.a == "x" && matches!(&x.b.data, DynValue::Str(s) if s == "0") {
            1.0
        } else {
            0.0
        }
    });
    feature_set.add_feature(Rc::new(Feature::new(
        is_zero,
        "isZero",
        vec!["0".to_owned()],
    )));

    let is_one = Rc::new(|x: &Element| {
        if x.a == "y" && matches!(&x.b.data, DynValue::Str(s) if s == "1") {
            1.0
        } else {
            0.0
        }
    });
    feature_set.add_feature(Rc::new(Feature::new(is_one, "isOne", vec!["1".to_owned()])));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn features_are_added_whatever_the_element_is() {
        let mut fs = FeatureSet::new();
        SEElement::new("zzz", Rc::new(Context::of_str("qqq")))
            .generate_features(&mut fs)
            .unwrap();
        assert_eq!(fs.size(), 2);
        assert_eq!(fs.pretty_print(), "isZero | 0\nisOne | 1\n");
    }

    #[test]
    fn a_numeric_payload_fails_the_strict_comparison() {
        let mut fs = FeatureSet::new();
        let numeric = Rc::new(Context::new(DynValue::Num(0.0)));
        let element = SEElement::new("x", Rc::clone(&numeric));
        element.generate_features(&mut fs).unwrap();
        let applied: Vec<f64> = fs.features().iter().map(|f| f.apply(&element)).collect();
        assert_eq!(applied, vec![0.0, 0.0]);

        let string_element = SEElement::new("x", Rc::new(Context::of_str("0")));
        let applied: Vec<f64> = fs
            .features()
            .iter()
            .map(|f| f.apply(&string_element))
            .collect();
        assert_eq!(applied, vec![1.0, 0.0]);
    }
}
