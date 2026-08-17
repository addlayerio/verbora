//! An ordered, deduplicated collection of features.

use std::rc::Rc;

use crate::dynval::DynValue;
use crate::maxent::feature::Feature;
use crate::ordmap::OrderedMap;

/// The features a model is built over, in insertion order.
///
/// Deduplication is by the flat string `name + " | " + parametersKey`, so a
/// second feature with the same identity is **rejected and discarded** even if
/// its function differs. That is what makes the correction feature unreplaceable
/// across retraining runs; see [`GISScaler`](crate::GISScaler).
#[derive(Debug, Default)]
pub struct FeatureSet {
    features: Vec<Rc<Feature>>,
    /// The dedup index. A `OrderedMap` rather than a `HashSet` because
    /// `prettyPrint()` walks `Object.keys(this.map)` and its order is part of
    /// the recorded output.
    map: OrderedMap<bool>,
}

impl FeatureSet {
    /// An empty feature set.
    pub fn new() -> Self {
        Self::default()
    }

    /// `addFeature(feature)`: adds it unless an identical key is present.
    ///
    /// Returns whether the feature was added.
    pub fn add_feature(&mut self, feature: Rc<Feature>) -> bool {
        let key = feature.dedup_key();
        if self.feature_exists(&feature) {
            return false;
        }
        self.map.insert(key, true);
        self.features.push(feature);
        true
    }

    /// `featureExists(feature)`.
    pub fn feature_exists(&self, feature: &Feature) -> bool {
        self.map.get(&feature.dedup_key()).copied().unwrap_or(false)
    }

    /// The features, in insertion order. Alpha is indexed against this.
    pub fn features(&self) -> &[Rc<Feature>] {
        &self.features
    }

    /// `size()`: the number of features. Note this is `features.length`, not
    /// the size of the dedup map — they can only differ if the map is tampered
    /// with, but the reference reads the array.
    pub fn size(&self) -> usize {
        self.features.len()
    }

    /// Whether the set holds no features.
    pub fn is_empty(&self) -> bool {
        self.features.is_empty()
    }

    /// `prettyPrint()`: every dedup key, each followed by a newline.
    pub fn pretty_print(&self) -> String {
        let mut s = String::new();
        for key in self.map.enumeration_order() {
            s.push_str(key);
            s.push('\n');
        }
        s
    }

    /// The serialised shape `save()` writes: `{ features, map }`.
    pub fn to_value(&self) -> DynValue {
        DynValue::Obj(vec![
            (
                "features".to_owned(),
                DynValue::Arr(self.features.iter().map(|f| f.to_value()).collect()),
            ),
            (
                "map".to_owned(),
                DynValue::Obj(
                    self.map
                        .ordered_entries()
                        .into_iter()
                        .map(|(k, v)| (k.to_owned(), DynValue::Bool(*v)))
                        .collect(),
                ),
            ),
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::maxent::element::Element;
    use crate::maxent::feature::FeatureFn;

    fn one() -> FeatureFn {
        Rc::new(|_: &Element| 1.0)
    }

    #[test]
    fn duplicates_are_rejected_and_the_first_wins() {
        let mut fs = FeatureSet::new();
        assert!(fs.add_feature(Rc::new(Feature::new(
            one(),
            "a",
            vec!["b".into(), "c".into()]
        ))));
        assert!(!fs.add_feature(Rc::new(Feature::new(
            one(),
            "a",
            vec!["b".into(), "c".into()]
        ))));
        assert!(fs.add_feature(Rc::new(Feature::new(one(), "a", vec!["b".into()]))));
        // The unescaped separators collide: ("a | b", ["c"]) and
        // ("a", ["b | c"]) both flatten to the key "a | b | c".
        assert!(fs.add_feature(Rc::new(Feature::new(one(), "a | b", vec!["c".into()]))));
        assert!(!fs.add_feature(Rc::new(Feature::new(one(), "a", vec!["b | c".into()]))));
        assert_eq!(fs.size(), 3);
    }

    #[test]
    fn pretty_print_follows_insertion_order() {
        let mut fs = FeatureSet::new();
        fs.add_feature(Rc::new(Feature::new(one(), "isZero", vec!["0".into()])));
        fs.add_feature(Rc::new(Feature::new(one(), "isOne", vec!["1".into()])));
        assert_eq!(fs.pretty_print(), "isZero | 0\nisOne | 1\n");
    }

    #[test]
    fn an_empty_set_pretty_prints_to_nothing() {
        assert_eq!(FeatureSet::new().pretty_print(), "");
        assert!(FeatureSet::new().is_empty());
    }
}
