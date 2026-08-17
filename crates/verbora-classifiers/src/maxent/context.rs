//! The observable half of an event: everything except the class.

use std::cell::RefCell;

use crate::dynval::DynValue;

/// A classification context, keyed by its `safe-stable-stringify` rendering.
///
/// The key is what every frequency table, weight memo and normalisation
/// constant is stored under, so two contexts are "the same" precisely when
/// their stringifications match. It is computed lazily and **never
/// invalidated**: mutating `data` after the first `to_key()` leaves the stale
/// key in place forever, and the reference behaves the same way.
#[derive(Debug)]
pub struct Context {
    /// The context payload.
    pub data: DynValue,
    key: RefCell<Option<String>>,
}

impl Context {
    /// A context over `data`.
    ///
    /// ```
    /// use verbora_classifiers::{Context, DynValue};
    ///
    /// assert_eq!(Context::new(DynValue::Str("0".into())).to_key(), Some("\"0\"".to_owned()));
    /// assert_eq!(Context::new(DynValue::Num(0.0)).to_key(), Some("0".to_owned()));
    /// ```
    pub fn new(data: DynValue) -> Self {
        Self {
            data,
            key: RefCell::new(None),
        }
    }

    /// A context over a string payload, the SimpleExample shape.
    pub fn of_str(data: &str) -> Self {
        Self::new(DynValue::Str(data.to_owned()))
    }

    /// `toString()`.
    ///
    /// `None` when the payload is `undefined`, mirroring the reference function
    /// returning the *value* `undefined` rather than a string — which is also
    /// why the cache never engages in that case.
    pub fn to_key(&self) -> Option<String> {
        // `if (!this.key)`: a falsy cached key is recomputed. Only `undefined`
        // and the empty string are falsy, and stringify never yields "".
        let cached = self.key.borrow().clone();
        if let Some(k) = cached.filter(|k| !k.is_empty()) {
            return Some(k);
        }
        let computed = self.data.stable_stringify();
        *self.key.borrow_mut() = computed.clone();
        computed
    }

    /// The key as the reference would coerce it when used as a property name:
    /// `undefined` becomes the literal string `"undefined"`.
    pub fn map_key(&self) -> String {
        self.to_key().unwrap_or_else(|| "undefined".to_owned())
    }

    /// The cached key, without computing one.
    pub fn cached_key(&self) -> Option<String> {
        self.key.borrow().clone()
    }

    /// The serialised shape `save()` writes: `{ data, key? }`, with `key`
    /// present only if `toString()` has ever been called.
    pub fn to_value(&self) -> DynValue {
        let mut fields = vec![("data".to_owned(), self.data.clone())];
        if let Some(k) = self.cached_key() {
            fields.push(("key".to_owned(), DynValue::Str(k)));
        }
        DynValue::Obj(fields)
    }
}

impl Clone for Context {
    fn clone(&self) -> Self {
        Self {
            data: self.data.clone(),
            key: RefCell::new(self.key.borrow().clone()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_match_the_recorded_reference_values() {
        for (data, want) in [
            (DynValue::Str("0".into()), Some("\"0\"")),
            (DynValue::Num(0.0), Some("0")),
            (DynValue::Null, Some("null")),
            (DynValue::Num(f64::NAN), Some("null")),
            (DynValue::Num(f64::INFINITY), Some("null")),
            (DynValue::Num(-0.0), Some("0")),
            (DynValue::Str("café😀".into()), Some("\"café😀\"")),
            (DynValue::Undefined, None),
        ] {
            assert_eq!(
                Context::new(data.clone()).to_key().as_deref(),
                want,
                "{data:?}"
            );
        }
    }

    #[test]
    fn object_keys_are_sorted_by_utf16_code_unit() {
        let data = DynValue::Obj(vec![
            ("b".to_owned(), DynValue::Num(1.0)),
            ("a".to_owned(), DynValue::Num(2.0)),
            ("-1".to_owned(), DynValue::Str("z".into())),
            ("0".to_owned(), DynValue::Str("q".into())),
            ("2".to_owned(), DynValue::Str("w".into())),
        ]);
        assert_eq!(
            Context::new(data).to_key().unwrap(),
            r#"{"-1":"z","0":"q","2":"w","a":2,"b":1}"#
        );
    }

    #[test]
    fn the_key_is_memoised_and_never_invalidated() {
        let mut c = Context::of_str("0");
        assert_eq!(c.to_key().unwrap(), "\"0\"");
        c.data = DynValue::Str("changed".into());
        assert_eq!(c.to_key().unwrap(), "\"0\"", "stale key must survive");
    }

    #[test]
    fn undefined_defeats_the_cache_and_maps_to_the_string_undefined() {
        let c = Context::new(DynValue::Undefined);
        assert_eq!(c.to_key(), None);
        assert_eq!(c.cached_key(), None);
        assert_eq!(c.map_key(), "undefined");
    }
}
