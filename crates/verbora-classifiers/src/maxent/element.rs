//! A `(class, context)` pair — one observation in the event space.

use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;

use crate::dynval::DynValue;
use crate::maxent::MaxEntError;
use crate::maxent::context::Context;
use crate::maxent::feature_set::FeatureSet;

/// Generates the features a subclass of `Element` contributes.
///
/// In the reference this is a method a user's `Element` subclass overrides; the
/// reference ships two implementations ([`SEElement`](crate::SEElement) and
/// [`POSElement`](crate::POSElement)) and `Sample.load` requires the caller to
/// pass their own class. Implement this to supply one.
pub trait GenerateFeatures {
    /// Adds this element's features to `feature_set`.
    ///
    /// # Errors
    ///
    /// Returns an error when the element's context does not have the shape the
    /// feature functions expect.
    fn generate_features(
        &self,
        element: &Element,
        feature_set: &mut FeatureSet,
    ) -> Result<(), MaxEntError>;
}

/// Which `Element` subclass this is — that is, which `generateFeatures` it has.
#[derive(Clone)]
pub(crate) enum ElementKind {
    /// The base class, which has no `generateFeatures` at all.
    Base,
    /// `SEElement`.
    Simple,
    /// `POSElement`.
    Pos,
    /// A caller-supplied subclass.
    Custom(Rc<dyn GenerateFeatures>),
}

impl fmt::Debug for ElementKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Base => "Base",
            Self::Simple => "SEElement",
            Self::Pos => "POSElement",
            Self::Custom(_) => "Custom",
        })
    }
}

/// One observation: a class `a` and a context `b`.
///
/// The context is shared (`Rc`), not copied — the reference passes the same
/// `Context` object into every `new Element(a, bi)` it builds while estimating
/// expectations, so they all share one memoised key.
#[derive(Debug, Clone)]
pub struct Element {
    /// The class.
    pub a: String,
    /// The context.
    pub b: Rc<Context>,
    pub(crate) kind: ElementKind,
    key: RefCell<Option<String>>,
}

impl Element {
    fn build(a: String, b: Rc<Context>, kind: ElementKind) -> Self {
        Self {
            a,
            b,
            kind,
            key: RefCell::new(None),
        }
    }

    /// A base `Element`, which cannot generate features.
    pub fn new(a: impl Into<String>, b: Rc<Context>) -> Self {
        Self::build(a.into(), b, ElementKind::Base)
    }

    /// An element of one of the built-in subclasses.
    pub(crate) fn with_kind(a: impl Into<String>, b: Rc<Context>, kind: ElementKind) -> Self {
        Self::build(a.into(), b, kind)
    }

    /// An element of a caller-defined subclass.
    pub fn with_generator(
        a: impl Into<String>,
        b: Rc<Context>,
        generator: Rc<dyn GenerateFeatures>,
    ) -> Self {
        Self::build(a.into(), b, ElementKind::Custom(generator))
    }

    /// `toString()`: `this.a + this.b.toString()`.
    ///
    /// A reference `+` with **no separator**, which is why distinct
    /// `(class, context)` pairs can collide — `a = "x\""` with data `0"` and
    /// `a = "x"` with data `"0"` reach the same key. Preserved deliberately: a
    /// tuple key would remove the collisions the reference has.
    pub fn to_key(&self) -> String {
        let cached = self.key.borrow().clone();
        if let Some(k) = cached.filter(|k| !k.is_empty()) {
            return k;
        }
        // `String + undefined` is "…undefined", which is what a context with an
        // `undefined` payload produces.
        let computed = format!("{}{}", self.a, self.b.map_key());
        *self.key.borrow_mut() = Some(computed.clone());
        computed
    }

    /// The cached key, without computing one.
    pub fn cached_key(&self) -> Option<String> {
        self.key.borrow().clone()
    }

    /// Adds this element's features to `feature_set`.
    ///
    /// # Errors
    ///
    /// [`MaxEntError::ElementCannotGenerateFeatures`] for a base element — the
    /// reference throws `TypeError: x.generateFeatures is not a function`.
    pub fn generate_features(&self, feature_set: &mut FeatureSet) -> Result<(), MaxEntError> {
        match &self.kind {
            ElementKind::Base => Err(MaxEntError::ElementCannotGenerateFeatures),
            ElementKind::Simple => crate::maxent::simple::generate(self, feature_set),
            ElementKind::Pos => crate::maxent::pos::generate(self, feature_set),
            ElementKind::Custom(g) => Rc::clone(g).generate_features(self, feature_set),
        }
    }

    /// The serialised shape `save()` writes: `{ a, b, key? }`.
    pub fn to_value(&self) -> DynValue {
        let mut fields = vec![
            ("a".to_owned(), DynValue::Str(self.a.clone())),
            ("b".to_owned(), self.b.to_value()),
        ];
        if let Some(k) = self.cached_key() {
            fields.push(("key".to_owned(), DynValue::Str(k)));
        }
        DynValue::Obj(fields)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SEElement;

    fn ctx(data: DynValue) -> Rc<Context> {
        Rc::new(Context::new(data))
    }

    #[test]
    fn keys_are_concatenated_without_a_separator() {
        assert_eq!(
            Element::new("x", ctx(DynValue::Str("0".into()))).to_key(),
            "x\"0\""
        );
        assert_eq!(Element::new("5", ctx(DynValue::Num(3.0))).to_key(), "53");
        assert_eq!(Element::new("x", ctx(DynValue::Null)).to_key(), "xnull");
        assert_eq!(
            Element::new("", ctx(DynValue::Str(String::new()))).to_key(),
            "\"\""
        );
        assert_eq!(
            Element::new("x", ctx(DynValue::Undefined)).to_key(),
            "xundefined"
        );
    }

    #[test]
    fn the_key_is_memoised() {
        let e = Element::new("x", ctx(DynValue::Str("0".into())));
        assert_eq!(e.cached_key(), None);
        assert_eq!(e.to_key(), "x\"0\"");
        assert_eq!(e.cached_key().as_deref(), Some("x\"0\""));
    }

    #[test]
    fn a_base_element_cannot_generate_features() {
        let mut fs = FeatureSet::new();
        assert_eq!(
            Element::new("x", ctx(DynValue::Str("0".into()))).generate_features(&mut fs),
            Err(MaxEntError::ElementCannotGenerateFeatures)
        );
        assert!(
            SEElement::new("x", ctx(DynValue::Str("0".into())))
                .generate_features(&mut fs)
                .is_ok()
        );
        assert_eq!(fs.size(), 2);
    }
}
