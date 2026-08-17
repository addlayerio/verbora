//! The slice of the reference value semantics that `TfIdf`'s behaviour rests on.
//!
//! `TfIdf` stores its documents as **the reference objects** and its keys as
//! **arbitrary the reference values**, and it reads them back with bare property
//! access and bare `?:` truthiness. That makes a surprising amount of the
//! language observable:
//!
//! * `document[term]` walks the prototype chain, so a term named `toString`
//!   yields a *function*, and `__proto__` yields the prototype object.
//! * `document[term] ? … : 0` is truthiness, not "is present": a stored `0`,
//!   `''`, `NaN`, `null` and `false` all read as absent.
//! * `document[term] + 1` is the reference `+`, so a string key concatenates.
//! * `JSON.stringify` renders numbers with `Number::toString`, which is neither
//!   Rust's `{}` nor its `{:e}` past the 1e21 / 1e-7 thresholds.
//!
//! Everything here exists to reproduce one of those, and nothing here is a
//! general-purpose the reference interpreter.

use std::fmt;
use std::sync::Arc;

use rustc_hash::FxHashMap;
use serde::de::{self, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};

// ---------------------------------------------------------------------------
// JsonValue
// ---------------------------------------------------------------------------

/// A JSON value with its object keys in **document order**.
///
/// `serde_json::Value` stores objects in a `BTreeMap` unless the crate-wide
/// `preserve_order` feature is on, which would re-order a deserialized corpus
/// alphabetically — and `listTerms`' output order is derived from insertion
/// order, so that is a parity bug waiting to happen. Turning the feature on
/// would also change `serde_json` for every other crate in the workspace
/// through feature unification. This type sidesteps both problems: its
/// [`Deserialize`] impl reads entries straight out of `MapAccess`, which yields
/// them in the order they appear in the text.
#[derive(Debug, Clone, PartialEq)]
pub enum JsonValue {
    /// `null`.
    Null,
    /// `true` / `false`.
    Bool(bool),
    /// A number. JSON has no `NaN`/`Infinity`, but a value built in Rust may.
    Num(f64),
    /// A string.
    Str(String),
    /// An array.
    Arr(Vec<JsonValue>),
    /// An object, in the order its keys first appeared.
    Obj(Vec<(String, JsonValue)>),
}

impl JsonValue {
    /// Looks up an own property of an object, or `None` for any other shape.
    pub fn own(&self, key: &str) -> Option<&JsonValue> {
        match self {
            Self::Obj(fields) => fields.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    /// Takes an own property out of an object by value, dropping the rest.
    ///
    /// The by-value counterpart of [`Self::own`], for a caller that already
    /// owns the value and wants to move one field out of it rather than clone
    /// it — [`crate::tfidf::TfIdf::from_json`] uses this so restoring a corpus
    /// from disk copies each document's data once, not twice.
    pub fn into_own(self, key: &str) -> Option<JsonValue> {
        match self {
            Self::Obj(fields) => fields.into_iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    /// The reference truthiness.
    pub fn is_truthy(&self) -> bool {
        match self {
            Self::Null => false,
            Self::Bool(b) => *b,
            Self::Num(n) => *n != 0.0 && !n.is_nan(),
            Self::Str(s) => !s.is_empty(),
            Self::Arr(_) | Self::Obj(_) => true,
        }
    }

    /// `ToNumber`, as `*` and `>` apply it.
    pub fn to_number(&self) -> f64 {
        match self {
            Self::Null => 0.0,
            Self::Bool(b) => f64::from(u8::from(*b)),
            Self::Num(n) => *n,
            Self::Str(s) => string_to_number(s),
            // `ToPrimitive` on an array joins it; on a plain object it is
            // "[object Object]", which never parses.
            Self::Arr(_) => string_to_number(&self.to_text()),
            Self::Obj(_) => f64::NAN,
        }
    }

    /// `ToString`, as the `+` operator and property-key coercion apply it.
    pub fn to_text(&self) -> String {
        match self {
            Self::Null => "null".into(),
            Self::Bool(b) => if *b { "true" } else { "false" }.into(),
            Self::Num(n) => number_to_string(*n),
            Self::Str(s) => s.clone(),
            // `Array.prototype.join(',')`, in which null/undefined become ''.
            Self::Arr(items) => items
                .iter()
                .map(|v| match v {
                    Self::Null => String::new(),
                    other => other.to_text(),
                })
                .collect::<Vec<_>>()
                .join(","),
            Self::Obj(_) => "[object Object]".into(),
        }
    }

    /// Appends this value's `JSON.stringify` rendering to `out`.
    ///
    /// Mirrors the real algorithm's two lossy cases: a non-finite number
    /// becomes `null`, and object keys are emitted in their stored order.
    pub fn write_json(&self, out: &mut String) {
        match self {
            Self::Null => out.push_str("null"),
            Self::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Self::Num(n) => {
                if n.is_finite() {
                    out.push_str(&number_to_string(*n));
                } else {
                    out.push_str("null");
                }
            }
            Self::Str(s) => write_json_string(s, out),
            Self::Arr(items) => {
                out.push('[');
                for (i, v) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    v.write_json(out);
                }
                out.push(']');
            }
            Self::Obj(fields) => {
                out.push('{');
                for (i, (k, v)) in fields.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    write_json_string(k, out);
                    out.push(':');
                    v.write_json(out);
                }
                out.push('}');
            }
        }
    }
}

impl<'de> Deserialize<'de> for JsonValue {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;

        impl<'de> Visitor<'de> for V {
            type Value = JsonValue;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("any JSON value")
            }

            fn visit_unit<E: de::Error>(self) -> Result<JsonValue, E> {
                Ok(JsonValue::Null)
            }
            fn visit_none<E: de::Error>(self) -> Result<JsonValue, E> {
                Ok(JsonValue::Null)
            }
            fn visit_bool<E: de::Error>(self, b: bool) -> Result<JsonValue, E> {
                Ok(JsonValue::Bool(b))
            }
            fn visit_i64<E: de::Error>(self, n: i64) -> Result<JsonValue, E> {
                #[expect(clippy::cast_precision_loss, reason = "JSON numbers are f64")]
                Ok(JsonValue::Num(n as f64))
            }
            fn visit_u64<E: de::Error>(self, n: u64) -> Result<JsonValue, E> {
                #[expect(clippy::cast_precision_loss, reason = "JSON numbers are f64")]
                Ok(JsonValue::Num(n as f64))
            }
            fn visit_f64<E: de::Error>(self, n: f64) -> Result<JsonValue, E> {
                Ok(JsonValue::Num(n))
            }
            fn visit_str<E: de::Error>(self, s: &str) -> Result<JsonValue, E> {
                Ok(JsonValue::Str(s.to_owned()))
            }
            fn visit_string<E: de::Error>(self, s: String) -> Result<JsonValue, E> {
                Ok(JsonValue::Str(s))
            }

            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<JsonValue, A::Error> {
                let mut items = Vec::with_capacity(seq.size_hint().unwrap_or(0));
                while let Some(v) = seq.next_element()? {
                    items.push(v);
                }
                Ok(JsonValue::Arr(items))
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<JsonValue, A::Error> {
                let hint = map.size_hint().unwrap_or(0);
                let mut fields: Vec<(String, JsonValue)> = Vec::with_capacity(hint);
                // A duplicate key in the text keeps its FIRST position but takes
                // its LAST value, which is what a reference object literal
                // does. Checking that with `fields.iter().find(...)` is what a
                // first draft of this did, and it is O(n) per key — quadratic
                // over an object's whole key count. A `TfIdf` document is
                // exactly such an object, one entry per distinct term, so a
                // restored corpus with a few thousand terms per document was
                // spending most of `from_json` right here. This index turns the
                // lookup into a hash probe and costs one extra small allocation
                // per *distinct* key, not a rescan per key.
                let mut index: FxHashMap<String, usize> =
                    FxHashMap::with_capacity_and_hasher(hint, rustc_hash::FxBuildHasher);
                while let Some((k, v)) = map.next_entry::<String, JsonValue>()? {
                    if let Some(&i) = index.get(&k) {
                        fields[i].1 = v;
                    } else {
                        index.insert(k.clone(), fields.len());
                        fields.push((k, v));
                    }
                }
                Ok(JsonValue::Obj(fields))
            }
        }

        d.deserialize_any(V)
    }
}

// ---------------------------------------------------------------------------
// Built-in prototypes
// ---------------------------------------------------------------------------

/// Which built-in prototype a value inherits from.
///
/// Only reachable through the `__proto__` accessor and through `constructor`,
/// both of which a document term can name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Proto {
    /// `Object.prototype`.
    Object,
    /// `Array.prototype` — itself an array, so it renders as `[]`.
    Array,
    /// `String.prototype`.
    String,
    /// `Number.prototype`.
    Number,
    /// `Boolean.prototype`.
    Boolean,
}

impl Proto {
    /// The name of the corresponding constructor function.
    pub const fn constructor_name(self) -> &'static str {
        match self {
            Self::Object => "Object",
            Self::Array => "Array",
            Self::String => "String",
            Self::Number => "Number",
            Self::Boolean => "Boolean",
        }
    }

    /// The method names this prototype adds on top of `Object.prototype`.
    pub const fn own_methods(self) -> &'static [&'static str] {
        match self {
            Self::Object => &[],
            Self::Array => ARRAY_PROTOTYPE_METHODS,
            Self::String => STRING_PROTOTYPE_METHODS,
            Self::Number => NUMBER_PROTOTYPE_METHODS,
            Self::Boolean => &[],
        }
    }
}

/// `Object.getOwnPropertyNames(Object.prototype)` minus the `__proto__`
/// accessor, which is handled separately because it yields an object.
///
/// Dumped from Node 22 rather than transcribed:
/// `node -e "console.log(Object.getOwnPropertyNames(Object.prototype))"`.
pub const OBJECT_PROTOTYPE_METHODS: &[&str] = &[
    "__defineGetter__",
    "__defineSetter__",
    "__lookupGetter__",
    "__lookupSetter__",
    "constructor",
    "hasOwnProperty",
    "isPrototypeOf",
    "propertyIsEnumerable",
    "toLocaleString",
    "toString",
    "valueOf",
];

/// `Object.getOwnPropertyNames(Array.prototype)`, minus `length` (a number, not
/// a function) and the names `Object.prototype` already provides.
const ARRAY_PROTOTYPE_METHODS: &[&str] = &[
    "at",
    "concat",
    "copyWithin",
    "entries",
    "every",
    "fill",
    "filter",
    "find",
    "findIndex",
    "findLast",
    "findLastIndex",
    "flat",
    "flatMap",
    "forEach",
    "includes",
    "indexOf",
    "join",
    "keys",
    "lastIndexOf",
    "map",
    "pop",
    "push",
    "reduce",
    "reduceRight",
    "reverse",
    "shift",
    "slice",
    "some",
    "sort",
    "splice",
    "toReversed",
    "toSorted",
    "toSpliced",
    "unshift",
    "values",
    "with",
];

/// `Object.getOwnPropertyNames(String.prototype)`, same exclusions.
const STRING_PROTOTYPE_METHODS: &[&str] = &[
    "anchor",
    "at",
    "big",
    "blink",
    "bold",
    "charAt",
    "charCodeAt",
    "codePointAt",
    "concat",
    "endsWith",
    "fixed",
    "fontcolor",
    "fontsize",
    "includes",
    "indexOf",
    "isWellFormed",
    "italics",
    "lastIndexOf",
    "link",
    "localeCompare",
    "match",
    "matchAll",
    "normalize",
    "padEnd",
    "padStart",
    "repeat",
    "replace",
    "replaceAll",
    "search",
    "slice",
    "small",
    "split",
    "startsWith",
    "strike",
    "sub",
    "substr",
    "substring",
    "sup",
    "toLocaleLowerCase",
    "toLocaleUpperCase",
    "toLowerCase",
    "toUpperCase",
    "toWellFormed",
    "trim",
    "trimEnd",
    "trimLeft",
    "trimRight",
    "trimStart",
];

/// `Object.getOwnPropertyNames(Number.prototype)`, same exclusions.
const NUMBER_PROTOTYPE_METHODS: &[&str] = &["toExponential", "toFixed", "toPrecision"];

/// Resolves `key` against a prototype chain rooted at `proto`.
///
/// Returns the *name* the resulting function reports, which is what a fixture
/// records: every inherited method reports its own name except `constructor`,
/// which reports the constructor's.
pub fn prototype_member(proto: Proto, key: &str) -> Option<DynValue> {
    if key == "__proto__" {
        return Some(DynValue::Prototype(proto));
    }
    if key == "constructor" {
        return Some(DynValue::Function(proto.constructor_name()));
    }
    if let Some(name) = proto.own_methods().iter().find(|m| **m == key) {
        return Some(DynValue::Function(name));
    }
    OBJECT_PROTOTYPE_METHODS
        .iter()
        .find(|m| **m == key)
        .map(|name| DynValue::Function(name))
}

// ---------------------------------------------------------------------------
// DynValue
// ---------------------------------------------------------------------------

/// A value read out of a document, an idf cache or a document key.
///
/// This is deliberately wider than `f64`: `TfIdf.tf` and `TfIdf#idf` both return
/// whatever a property lookup found, and on the reference's prototype-backed maps
/// that can be a function or a prototype object. Collapsing those to `NaN`
/// would be numerically correct — every arithmetic use coerces them to `NaN`
/// anyway — but would erase them from `listTerms`, where they are visible.
#[derive(Debug, Clone)]
pub enum DynValue {
    /// `undefined`.
    Undefined,
    /// `null`.
    Null,
    /// A boolean.
    Bool(bool),
    /// A number.
    Num(f64),
    /// A string.
    Str(Arc<str>),
    /// A built-in function, identified by the name it reports.
    Function(&'static str),
    /// A built-in prototype object, reached through `__proto__`.
    Prototype(Proto),
    /// A value carried over from a deserialized corpus.
    Json(Arc<JsonValue>),
}

impl DynValue {
    /// The reference truthiness — the test `document[term] ? … : 0` performs.
    pub fn is_truthy(&self) -> bool {
        match self {
            Self::Undefined | Self::Null => false,
            Self::Bool(b) => *b,
            Self::Num(n) => *n != 0.0 && !n.is_nan(),
            Self::Str(s) => !s.is_empty(),
            Self::Function(_) | Self::Prototype(_) => true,
            Self::Json(v) => v.is_truthy(),
        }
    }

    /// `ToNumber`. Functions and non-array prototypes are `NaN`; `Array.prototype`
    /// is an empty array, and `Number([])` is `0`.
    pub fn to_number(&self) -> f64 {
        match self {
            Self::Undefined => f64::NAN,
            Self::Null => 0.0,
            Self::Bool(b) => f64::from(u8::from(*b)),
            Self::Num(n) => *n,
            Self::Str(s) => string_to_number(s),
            Self::Function(_) => f64::NAN,
            Self::Prototype(Proto::Array) => 0.0,
            Self::Prototype(_) => f64::NAN,
            Self::Json(v) => v.to_number(),
        }
    }

    /// `ToString`, used by the `+` that corrupts a `__key` slot.
    pub fn to_text(&self) -> String {
        match self {
            Self::Undefined => "undefined".into(),
            Self::Null => "null".into(),
            Self::Bool(b) => if *b { "true" } else { "false" }.into(),
            Self::Num(n) => number_to_string(*n),
            Self::Str(s) => s.to_string(),
            Self::Function(name) => format!("function {name}() {{ [native code] }}"),
            Self::Prototype(Proto::Array) => String::new(),
            Self::Prototype(_) => "[object Object]".into(),
            Self::Json(v) => v.to_text(),
        }
    }

    /// `value && value > 0` — the reference's `documentHasTerm`.
    ///
    /// Note the `> 0` rather than `!= 0`: a deserialized count of `-2` does not
    /// make the document "have" the term, and neither does a non-numeric string.
    pub fn counts_as_present(&self) -> bool {
        self.is_truthy() && self.to_number() > 0.0
    }
}

// ---------------------------------------------------------------------------
// Number rendering and parsing
// ---------------------------------------------------------------------------

/// The reference language `Number::toString(x, 10)`.
///
/// Rust's `{}` and `{:e}` both produce shortest round-tripping digits, but they
/// disagree with the reference about *when* to use exponential notation: it
/// switches at 1e21 upward and 1e-7 downward and writes the exponent with an
/// explicit sign (`1e+21`), while Rust's `{}` never switches at all
/// (`1e21` prints as `1000000000000000000000`). Since a query term is
/// `terms.toString().toLowerCase()`, the difference decides which tokens a
/// numeric query produces.
pub fn number_to_string(x: f64) -> String {
    if x.is_nan() {
        return "NaN".into();
    }
    if x == 0.0 {
        return "0".into();
    }
    if x < 0.0 {
        return format!("-{}", number_to_string(-x));
    }
    if x.is_infinite() {
        return "Infinity".into();
    }

    // `{:e}` gives "<digits>e<exp>" with the shortest round-tripping mantissa.
    let sci = format!("{x:e}");
    let (mantissa, exp) = sci
        .split_once('e')
        .expect("`{:e}` always emits an exponent");
    let exp: i32 = exp.parse().expect("`{:e}` always emits a decimal exponent");
    let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    let digits = digits.trim_end_matches('0');
    let digits = if digits.is_empty() { "0" } else { digits };

    // ECMA-262 Number::toString notation: the value is `digits * 10^(n - k)`,
    // where `k` is the digit count.
    #[expect(clippy::cast_possible_wrap, reason = "digit counts are tiny")]
    let k = digits.len() as i32;
    let n = exp + 1;

    if k <= n && n <= 21 {
        let mut s = String::from(digits);
        s.extend(std::iter::repeat_n('0', (n - k) as usize));
        s
    } else if 0 < n && n <= 21 {
        let (head, tail) = digits.split_at(n as usize);
        format!("{head}.{tail}")
    } else if -6 < n && n <= 0 {
        let mut s = String::from("0.");
        s.extend(std::iter::repeat_n('0', (-n) as usize));
        s.push_str(digits);
        s
    } else {
        let e = n - 1;
        let sign = if e >= 0 { '+' } else { '-' };
        if k == 1 {
            format!("{digits}e{sign}{}", e.abs())
        } else {
            let (head, tail) = digits.split_at(1);
            format!("{head}.{tail}e{sign}{}", e.abs())
        }
    }
}

/// The reference language `ToNumber` applied to a string.
///
/// Only the parts a document value can plausibly exercise: the reference whitespace
/// trimming, the empty string, `Infinity`, and the radix prefixes. Anything else
/// is `NaN`, which is what a term like `"three"` produces.
pub fn string_to_number(s: &str) -> f64 {
    let t = s.trim_matches(verbora_core::whitespace::is_whitespace);
    if t.is_empty() {
        return 0.0;
    }
    let (signed, sign, body) = match t.strip_prefix('-') {
        Some(rest) => (true, -1.0, rest),
        None => match t.strip_prefix('+') {
            Some(rest) => (true, 1.0, rest),
            None => (false, 1.0, t),
        },
    };
    if body == "Infinity" {
        return sign * f64::INFINITY;
    }
    // Radix literals are only recognised unsigned: `Number('-0x10')` is NaN.
    if !signed && t.len() > 2 {
        let radix = match &t[..2] {
            "0x" | "0X" => Some(16),
            "0o" | "0O" => Some(8),
            "0b" | "0B" => Some(2),
            _ => None,
        };
        if let Some(radix) = radix {
            #[expect(clippy::cast_precision_loss, reason = "matches ToNumber's rounding")]
            return u128::from_str_radix(&t[2..], radix).map_or(f64::NAN, |v| v as f64);
        }
    }
    // Rust accepts "inf"/"NaN"/"1_0" spellings the reference does not.
    if body.is_empty()
        || !body
            .bytes()
            .all(|b| b.is_ascii_digit() || matches!(b, b'.' | b'e' | b'E' | b'+' | b'-'))
    {
        return f64::NAN;
    }
    t.parse().unwrap_or(f64::NAN)
}

/// Appends `JSON.stringify`'s rendering of a string, including its escapes.
pub fn write_json_string(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

/// Whether `key` is an **array index**: the canonical decimal spelling of an
/// integer in `0 ..= 2^32 - 2`.
///
/// This is the rule that hoists keys to the front of `for…in`. It is stricter
/// than "parses as a number": `"01"`, `"1.0"`, `"-1"`, `"1e3"` and
/// `"4294967295"` are all ordinary string keys, as the fixtures confirm.
pub fn array_index(key: &str) -> Option<u32> {
    let bytes = key.as_bytes();
    if bytes.is_empty() || bytes.len() > 10 || !bytes.iter().all(u8::is_ascii_digit) {
        return None;
    }
    if bytes.len() > 1 && bytes[0] == b'0' {
        return None;
    }
    key.parse::<u32>().ok().filter(|n| *n != u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn number_to_string_matches_the_reference() {
        // Values recorded from `String(x)` in Node.
        for (x, want) in [
            (0.0, "0"),
            (-0.0, "0"),
            (1.0, "1"),
            (-1.0, "-1"),
            (0.5, "0.5"),
            (1.5, "1.5"),
            (100.0, "100"),
            (1e6, "1000000"),
            (1e20, "100000000000000000000"),
            (1e21, "1e+21"),
            (1e22, "1e+22"),
            (1.5e21, "1.5e+21"),
            (1e-6, "0.000001"),
            (1e-7, "1e-7"),
            (1e-21, "1e-21"),
            (5e-324, "5e-324"),
            (f64::NAN, "NaN"),
            (f64::INFINITY, "Infinity"),
            (f64::NEG_INFINITY, "-Infinity"),
            (0.1 + 0.2, "0.30000000000000004"),
            (1.0 / 3.0, "0.3333333333333333"),
            (f64::MAX, "1.7976931348623157e+308"),
        ] {
            assert_eq!(number_to_string(x), want, "String({x:?})");
        }
    }

    #[test]
    fn string_to_number_matches_the_reference() {
        for (s, want) in [
            ("", 0.0),
            ("  ", 0.0),
            ("3", 3.0),
            (" 3 ", 3.0),
            ("-2", -2.0),
            ("1e3", 1000.0),
            ("0x10", 16.0),
            ("Infinity", f64::INFINITY),
            ("-Infinity", f64::NEG_INFINITY),
        ] {
            assert_eq!(string_to_number(s), want, "Number({s:?})");
        }
        for s in ["three", "inf", "nan", "1_0", "0x", "--1"] {
            assert!(string_to_number(s).is_nan(), "Number({s:?}) should be NaN");
        }
    }

    #[test]
    fn array_index_matches_the_hoisting_rule() {
        for k in ["0", "1", "10", "999999999", "4294967294"] {
            assert!(array_index(k).is_some(), "{k} should hoist");
        }
        for k in [
            "",
            "00",
            "01",
            "1e3",
            "-1",
            "1.0",
            "4294967295",
            "4294967296",
            "a",
            "1a",
            " 1",
        ] {
            assert!(array_index(k).is_none(), "{k} should not hoist");
        }
    }

    #[test]
    fn deserialization_preserves_key_order() {
        let v: JsonValue = serde_json::from_str(r#"{"z":1,"a":2,"10":3,"a":4}"#).unwrap();
        let JsonValue::Obj(fields) = v else {
            panic!("expected an object")
        };
        let names: Vec<&str> = fields.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(names, ["z", "a", "10"]);
        // A duplicate key keeps its first position and its last value.
        assert_eq!(fields[1].1, JsonValue::Num(4.0));
    }

    #[test]
    fn json_stringify_matches_the_reference() {
        let mut s = String::new();
        JsonValue::Obj(vec![
            ("a".into(), JsonValue::Num(1.0)),
            ("b".into(), JsonValue::Num(f64::NEG_INFINITY)),
            ("c".into(), JsonValue::Str("q\"\n\u{1}".into())),
            ("d".into(), JsonValue::Arr(vec![JsonValue::Null])),
        ])
        .write_json(&mut s);
        assert_eq!(
            s,
            "{\"a\":1,\"b\":null,\"c\":\"q\\\"\\n\\u0001\",\"d\":[null]}"
        );
    }

    #[test]
    fn prototype_lookup_reports_the_right_names() {
        assert!(matches!(
            prototype_member(Proto::Array, "constructor"),
            Some(DynValue::Function("Array"))
        ));
        assert!(matches!(
            prototype_member(Proto::Number, "toString"),
            Some(DynValue::Function("toString"))
        ));
        assert!(matches!(
            prototype_member(Proto::Object, "__proto__"),
            Some(DynValue::Prototype(Proto::Object))
        ));
        assert!(prototype_member(Proto::Object, "map").is_none());
        assert!(matches!(
            prototype_member(Proto::Array, "map"),
            Some(DynValue::Function("map"))
        ));
    }
}
