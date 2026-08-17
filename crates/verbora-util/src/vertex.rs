//! Graph vertices, modelled as the reference values.
//!
//! In the reference a vertex is whatever the caller passed to
//! `EdgeWeightedDigraph#add`, and the graph code uses it in two different ways
//! that only *usually* agree:
//!
//! * as a **property key**, when indexing the sparse arrays `adj`, `distTo` and
//!   `edgeTo` — so the number `1` and the string `'1'` address the same slot;
//! * as a **value**, when `Topological` compares with `===` and `indexOf` — so
//!   the number `1` and the string `'1'` are two different vertices.
//!
//! Both halves are observable at once. `new EdgeWeightedDigraph()` fed
//! `add(1, 2, 0.5)` and `add('1', 3, 0.25)` puts both edges in a single bag
//! (`getAdj(1)` and `getAdj('1')` each return both) while `Topological#order`
//! reports four vertices, `['1', 3, 1, 2]`. A port that models a vertex as
//! "just a `u32`" or "just a `String`" cannot reproduce that, so [`Vertex`]
//! keeps the reference distinction and derives the property key from it.

use std::borrow::Cow;
use std::fmt;

use crate::numfmt;

/// A graph vertex: a reference number or a reference string.
///
/// # Equality is the reference's `===`
///
/// The derived [`PartialEq`] is exactly strict equality, including the two
/// consequences that matter to `Topological`:
///
/// * `Vertex::Num(f64::NAN) != Vertex::Num(f64::NAN)`, so a `NaN` vertex is
///   never found by the reference's `indexOf` scans and is pushed onto the
///   vertex list once per occurrence;
/// * `Vertex::Num(0.0) == Vertex::Num(-0.0)`, since `0 === -0`.
///
/// Because of the first point `Vertex` deliberately implements neither [`Eq`]
/// nor [`std::hash::Hash`]; hashing goes through [`VertexKey`], which excludes
/// `NaN` by construction.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum Vertex {
    /// A numeric vertex.
    Num(f64),
    /// A string vertex.
    Str(String),
}

impl Vertex {
    /// The property key this vertex addresses, i.e. The reference's
    /// `ToPropertyKey`.
    ///
    /// Numbers convert with `Number::toString`, so `1` yields `"1"`, `1.5`
    /// yields `"1.5"` and `-0` yields `"0"`.
    pub fn property_key(&self) -> Cow<'_, str> {
        match self {
            Self::Num(n) => Cow::Owned(numfmt::number_to_string(*n)),
            Self::Str(s) => Cow::Borrowed(s),
        }
    }

    /// The array index this vertex addresses, if it addresses one.
    ///
    /// A reference array index is a property key that is the canonical decimal
    /// form of an integer in `0..=(2^32 - 2)`. Everything else — `1.5`, `-1`,
    /// `"01"`, `"a"`, `4294967295` — becomes an ordinary string property, which
    /// is why `EdgeWeightedDigraph#v()` (`adj.length`) stays 0 for a graph whose
    /// vertices are all strings or all fractional.
    pub fn array_index(&self) -> Option<u32> {
        match self {
            // `-0.0` is included: `ToString(-0)` is `"0"`, so `a[-0]` is `a[0]`.
            Self::Num(n) => {
                let n = *n;
                if n.is_finite() && n.fract() == 0.0 && n >= 0.0 && n < f64::from(u32::MAX) {
                    Some(n as u32)
                } else {
                    None
                }
            }
            Self::Str(s) => canonical_array_index(s),
        }
    }

    /// A hashable projection of this vertex, or `None` for `NaN`.
    ///
    /// `None` is not an error: it is the whole point. `indexOf` uses strict
    /// equality, `NaN !== NaN`, so a `NaN` vertex must never be *found* — which
    /// is precisely what refusing to put it in a lookup table achieves.
    pub fn key(&self) -> Option<VertexKey<'_>> {
        match self {
            Self::Num(n) if n.is_nan() => None,
            // Normalise -0 to +0 so the two hash alike, matching `0 === -0`.
            Self::Num(n) => Some(VertexKey::Num(
                if *n == 0.0 { 0.0f64 } else { *n }.to_bits(),
            )),
            Self::Str(s) => Some(VertexKey::Str(s)),
        }
    }

    /// Renders this vertex the way `JSON.stringify` would.
    ///
    /// `Topological`'s cycle report is `'Cyclic dependency:' +
    /// JSON.stringify(vertex)`, so a numeric vertex contributes bare digits and
    /// a string vertex contributes a *quoted* string: the message for a cycle
    /// through `'b'` is `Cyclic dependency:"b"`.
    pub fn to_json(&self) -> String {
        match self {
            Self::Num(n) => numfmt::json_number(*n),
            // `serde_json` escapes exactly what `JSON.stringify` escapes.
            Self::Str(s) => serde_json::to_string(s).unwrap_or_else(|_| format!("{s:?}")),
        }
    }
}

/// Returns `Some(i)` when `s` is the canonical decimal form of an array index.
///
/// "Canonical" rules out `"01"`, `"+1"`, `"1.0"`, `" 1"` and `"1e2"`, all of
/// which the reference keeps as string properties.
fn canonical_array_index(s: &str) -> Option<u32> {
    let bytes = s.as_bytes();
    if bytes.is_empty() || !bytes.iter().all(u8::is_ascii_digit) {
        return None;
    }
    if bytes.len() > 1 && bytes[0] == b'0' {
        return None;
    }
    match s.parse::<u32>() {
        // 2^32 - 1 is the maximum *length*, so it is not a valid index.
        Ok(i) if i != u32::MAX => Some(i),
        _ => None,
    }
}

/// A hashable stand-in for a [`Vertex`], used for `indexOf`-shaped lookups.
///
/// Numbers are keyed by their bit pattern with `-0` normalised to `+0`, which
/// makes hash equality coincide with `===` for every value a `VertexKey` can
/// hold (`NaN`, the one value where they would differ, cannot be one).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VertexKey<'a> {
    /// A numeric vertex, keyed by `f64::to_bits`.
    Num(u64),
    /// A string vertex.
    Str(&'a str),
}

impl fmt::Display for Vertex {
    /// Renders this vertex the way `util.format('%s', v)` does.
    ///
    /// That is what `DirectedEdge::toString` uses, so `-0` prints as `-0` —
    /// unlike `String(-0)`, which is `"0"`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Num(n) => f.write_str(&numfmt::format_s(*n)),
            Self::Str(s) => f.write_str(s),
        }
    }
}

impl From<f64> for Vertex {
    fn from(v: f64) -> Self {
        Self::Num(v)
    }
}

impl From<&str> for Vertex {
    fn from(v: &str) -> Self {
        Self::Str(v.to_owned())
    }
}

impl From<String> for Vertex {
    fn from(v: String) -> Self {
        Self::Str(v)
    }
}

/// Generates the integer conversions. Every one of these is exact as an `f64`,
/// which is what the reference would have stored.
macro_rules! vertex_from_int {
    ($($t:ty),*) => {
        $(impl From<$t> for Vertex {
            fn from(v: $t) -> Self {
                Self::Num(f64::from(v))
            }
        })*
    };
}
vertex_from_int!(i8, i16, i32, u8, u16, u32);

impl From<usize> for Vertex {
    /// Converts through `f64`, exactly as the reference would have stored it.
    ///
    /// Values above 2^53 lose precision — the same loss the reference incurs, since
    /// it has no integer type to lose it from.
    fn from(v: usize) -> Self {
        Self::Num(v as f64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strict_equality_semantics() {
        assert_eq!(Vertex::Num(1.0), Vertex::Num(1.0));
        assert_eq!(Vertex::Num(0.0), Vertex::Num(-0.0));
        assert_ne!(Vertex::Num(f64::NAN), Vertex::Num(f64::NAN));
        // A number and the string of that number are different *values*...
        assert_ne!(Vertex::Num(1.0), Vertex::from("1"));
        // ...but the same property key.
        assert_eq!(
            Vertex::Num(1.0).property_key(),
            Vertex::from("1").property_key()
        );
    }

    #[test]
    fn nan_has_no_key() {
        assert!(Vertex::Num(f64::NAN).key().is_none());
        assert_eq!(Vertex::Num(0.0).key(), Vertex::Num(-0.0).key());
    }

    #[test]
    fn array_index_rules() {
        assert_eq!(Vertex::Num(0.0).array_index(), Some(0));
        assert_eq!(Vertex::Num(-0.0).array_index(), Some(0));
        assert_eq!(Vertex::Num(10.0).array_index(), Some(10));
        assert_eq!(Vertex::Num(1.5).array_index(), None);
        assert_eq!(Vertex::Num(-1.0).array_index(), None);
        assert_eq!(Vertex::Num(f64::NAN).array_index(), None);
        assert_eq!(Vertex::from("0").array_index(), Some(0));
        assert_eq!(Vertex::from("10").array_index(), Some(10));
        assert_eq!(Vertex::from("01").array_index(), None);
        assert_eq!(Vertex::from("").array_index(), None);
        assert_eq!(Vertex::from("1.0").array_index(), None);
        assert_eq!(Vertex::from("a").array_index(), None);
        assert_eq!(Vertex::from("4294967295").array_index(), None);
        assert_eq!(
            Vertex::from("4294967294").array_index(),
            Some(4_294_967_294)
        );
    }

    #[test]
    fn display_and_json() {
        assert_eq!(Vertex::Num(0.4).to_string(), "0.4");
        assert_eq!(Vertex::Num(-0.0).to_string(), "-0");
        assert_eq!(Vertex::Num(-0.0).to_json(), "0");
        assert_eq!(Vertex::from("b").to_json(), "\"b\"");
        assert_eq!(Vertex::Num(1.0).to_json(), "1");
        assert_eq!(Vertex::from("a\"b").to_json(), "\"a\\\"b\"");
    }
}
