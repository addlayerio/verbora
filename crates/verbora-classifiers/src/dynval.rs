//! A reference value, its `Number::toString`, and the two serialisers the
//! classifiers depend on.
//!
//! Two apparently-cosmetic pieces of the reference reach directly into the numbers
//! this crate computes:
//!
//! * `Context.toString()` calls **`safe-stable-stringify`**, and the resulting
//!   string is the *hash key* under which every frequency, weight and
//!   normalisation constant is stored. Change one byte of it — sort keys by
//!   Unicode scalar value instead of UTF-16 code unit, print `1e21` as
//!   `1000000000000000000000` — and two contexts that the reference keeps apart
//!   collide, or two that it merges stay separate. The trained model changes.
//! * `save()` calls `JSON.stringify`, and the reference's own spec files read
//!   the bytes back. Key order there is the reference's own-property order, which
//!   is *not* the sorted order `safe-stable-stringify` uses.
//!
//! Both need the reference's `Number::toString`, which differs from Rust's `{}` in
//! exactly the region where the exponent form kicks in:
//!
//! | value | Rust `{}` | the reference |
//! |---|---|---|
//! | `1e21` | `1000000000000000000000` | `1e+21` |
//! | `1e-7` | `0.0000001` | `1e-7` |
//! | `-0.0` | `-0` | `0` |
//!
//! [`number_to_string`] implements the ECMA-262 algorithm on top of Rust's
//! shortest-round-trip digits, so the digits themselves come from a correctly
//! rounded formatter and only the *layout* is reimplemented.

use std::fmt;

/// A reference value, as far as this crate needs one.
///
/// `serde_json::Value` cannot stand in: it has no `undefined`, cannot hold
/// `NaN`/`Infinity` (all three occur in maxent contexts and alpha vectors), and
/// does not distinguish `-0.0`, which `Number::toString` renders as `0`.
#[derive(Debug, Clone, PartialEq)]
pub enum DynValue {
    /// `undefined`.
    Undefined,
    /// `null`.
    Null,
    /// A boolean.
    Bool(bool),
    /// A number. `NaN`, `±Infinity` and `-0.0` are all representable.
    Num(f64),
    /// A string.
    Str(String),
    /// An array.
    Arr(Vec<DynValue>),
    /// An object, in own-property insertion order.
    Obj(Vec<(String, DynValue)>),
}

impl DynValue {
    /// The value's `safe-stable-stringify` rendering, as `Context.toString`
    /// computes it.
    ///
    /// Returns `None` for a top-level `undefined`, mirroring the reference
    /// function returning the value `undefined` rather than a string — which is
    /// why `Context(undefined).toString()` never caches and recomputes forever.
    pub fn stable_stringify(&self) -> Option<String> {
        let mut out = String::new();
        if write_stable(&mut out, self) {
            Some(out)
        } else {
            None
        }
    }

    /// The value's `JSON.stringify` rendering, preserving own-property order.
    ///
    /// Returns `None` for a top-level `undefined`, as `JSON.stringify` does.
    pub fn json_stringify(&self) -> Option<String> {
        let mut out = String::new();
        if write_json(&mut out, self) {
            Some(out)
        } else {
            None
        }
    }

    /// `String(value)` for the operands of `Element.toString`'s `a + b`.
    pub fn to_text(&self) -> String {
        match self {
            Self::Undefined => "undefined".to_owned(),
            Self::Null => "null".to_owned(),
            Self::Bool(b) => b.to_string(),
            Self::Num(n) => number_to_string(*n),
            Self::Str(s) => s.clone(),
            Self::Arr(items) => items
                .iter()
                .map(|v| match v {
                    // Array.prototype.join renders null and undefined as empty.
                    Self::Null | Self::Undefined => String::new(),
                    other => other.to_text(),
                })
                .collect::<Vec<_>>()
                .join(","),
            Self::Obj(_) => "[object Object]".to_owned(),
        }
    }

    /// The same value with every object's properties reordered the way
    /// the reference enumerates them: array-index keys first in ascending numeric
    /// order, then the rest in insertion order.
    ///
    /// Useful when a structure built in source order has to be compared against
    /// something that went through `Object.keys` — a maxent POS context, for
    /// instance, is populated `0, -2, -1, 1, 2` but enumerates `0, 1, 2, -2, -1`.
    pub fn in_own_property_order(&self) -> Self {
        match self {
            Self::Obj(fields) => Self::Obj(
                own_key_order(fields)
                    .into_iter()
                    .map(|(k, v)| (k.clone(), v.in_own_property_order()))
                    .collect(),
            ),
            Self::Arr(items) => Self::Arr(items.iter().map(Self::in_own_property_order).collect()),
            other => other.clone(),
        }
    }

    /// Looks up an own property, if this is an object.
    pub fn get(&self, key: &str) -> Option<&Self> {
        match self {
            Self::Obj(fields) => fields.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    /// The value as a string slice, if it is one.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::Str(s) => Some(s),
            _ => None,
        }
    }

    /// The reference truthiness: everything except `false`, `0`, `-0`, `NaN`,
    /// `""`, `null` and `undefined`.
    pub fn is_truthy(&self) -> bool {
        match self {
            Self::Undefined | Self::Null => false,
            Self::Bool(b) => *b,
            Self::Num(n) => *n != 0.0 && !n.is_nan(),
            Self::Str(s) => !s.is_empty(),
            Self::Arr(_) | Self::Obj(_) => true,
        }
    }
}

impl fmt::Display for DynValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_text())
    }
}

impl From<&str> for DynValue {
    fn from(s: &str) -> Self {
        Self::Str(s.to_owned())
    }
}

impl From<String> for DynValue {
    fn from(s: String) -> Self {
        Self::Str(s)
    }
}

impl From<f64> for DynValue {
    fn from(n: f64) -> Self {
        Self::Num(n)
    }
}

impl From<bool> for DynValue {
    fn from(b: bool) -> Self {
        Self::Bool(b)
    }
}

// --------------------------------------------------------------------------
// Number::toString
// --------------------------------------------------------------------------

/// ECMA-262 `Number::toString(x, 10)`.
///
/// Rust's `{}` and the reference agree on the *digits* — both print the shortest
/// decimal that round-trips — but disagree on when to switch to exponent
/// notation and on the sign of zero. This takes Rust's shortest digits from
/// `{:e}` and re-lays them out by the specification's five cases.
pub fn number_to_string(x: f64) -> String {
    if x.is_nan() {
        return "NaN".to_owned();
    }
    if x == 0.0 {
        // Covers -0.0: the reference's String(-0) is "0". (Only Object.is and
        // 1/x can see the sign; nothing in these classifiers does.)
        return "0".to_owned();
    }
    if x.is_infinite() {
        return if x < 0.0 { "-Infinity" } else { "Infinity" }.to_owned();
    }
    if x < 0.0 {
        return format!("-{}", number_to_string(-x));
    }

    // `{:e}` gives "d[.ddd]e[-]E": shortest round-tripping digits, normalised
    // to one leading digit.
    let sci = format!("{x:e}");
    let (mantissa, exp) = sci
        .split_once('e')
        .expect("Rust LowerExp always emits an exponent");
    let exp: i32 = exp.parse().expect("Rust LowerExp emits a decimal exponent");
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    let digits = digits.trim_end_matches('0');
    let digits = if digits.is_empty() { "0" } else { digits };

    let k = i32::try_from(digits.len()).expect("shortest f64 repr is at most 17 digits");
    // `n` is the position of the decimal point: value == 0.<digits> * 10^n.
    let n = exp + 1;

    if k <= n && n <= 21 {
        // 1e2 -> "100"
        let mut s = String::from(digits);
        s.extend(std::iter::repeat_n('0', (n - k) as usize));
        s
    } else if 0 < n && n <= 21 {
        // 1.5e2 -> "150"; 3.14e0 -> "3.14"
        let split = n as usize;
        format!("{}.{}", &digits[..split], &digits[split..])
    } else if -6 < n && n <= 0 {
        // 1e-3 -> "0.001"
        let mut s = String::from("0.");
        s.extend(std::iter::repeat_n('0', (-n) as usize));
        s.push_str(digits);
        s
    } else if k == 1 {
        // 1e21 -> "1e+21"
        format!("{}e{}{}", digits, sign_of(n - 1), (n - 1).abs())
    } else {
        // 1.5e-7 -> "1.5e-7"
        format!(
            "{}.{}e{}{}",
            &digits[..1],
            &digits[1..],
            sign_of(n - 1),
            (n - 1).abs()
        )
    }
}

const fn sign_of(n: i32) -> char {
    if n < 0 { '-' } else { '+' }
}

// --------------------------------------------------------------------------
// string escaping
// --------------------------------------------------------------------------

/// Appends a JSON string literal, escaping exactly what `JSON.stringify` does.
///
/// Non-ASCII characters are emitted **raw**, not `\u`-escaped — the reference's
/// context keys contain `café` and `😀` verbatim, and escaping them would change
/// every hash key derived from them.
fn write_string(out: &mut String, s: &str) {
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
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

// --------------------------------------------------------------------------
// safe-stable-stringify
// --------------------------------------------------------------------------

/// Compares two property names the way `Array.prototype.sort` does: by UTF-16
/// code unit.
///
/// This is *not* Rust's `str: Ord`, which compares Unicode scalar values. The
/// two disagree for any pair straddling U+E000..U+FFFF and the astral planes:
/// `"😀"` starts with the lead surrogate `0xD83D`, which sorts *before*
/// `U+FFFD`, while by scalar value `U+FFFD` comes first.
pub fn utf16_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    a.encode_utf16().cmp(b.encode_utf16())
}

/// Writes `value` as `safe-stable-stringify` would; returns `false` if the
/// value is `undefined` (which the reference function omits entirely).
fn write_stable(out: &mut String, value: &DynValue) -> bool {
    match value {
        DynValue::Undefined => false,
        DynValue::Null => {
            out.push_str("null");
            true
        }
        DynValue::Bool(b) => {
            out.push_str(if *b { "true" } else { "false" });
            true
        }
        DynValue::Num(n) => {
            // JSON has no NaN or Infinity; the reference emits `null` for both.
            out.push_str(&if n.is_finite() {
                number_to_string(*n)
            } else {
                "null".to_owned()
            });
            true
        }
        DynValue::Str(s) => {
            write_string(out, s);
            true
        }
        DynValue::Arr(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                // An `undefined` element serialises as `null`, not as a hole.
                if !write_stable(out, item) {
                    out.push_str("null");
                }
            }
            out.push(']');
            true
        }
        DynValue::Obj(fields) => {
            let mut keys: Vec<&(String, DynValue)> = fields
                .iter()
                // Properties whose value is `undefined` are dropped outright.
                .filter(|(_, v)| !matches!(v, DynValue::Undefined))
                .collect();
            keys.sort_by(|a, b| utf16_cmp(&a.0, &b.0));
            out.push('{');
            for (i, (k, v)) in keys.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_string(out, k);
                out.push(':');
                write_stable(out, v);
            }
            out.push('}');
            true
        }
    }
}

// --------------------------------------------------------------------------
// JSON.stringify
// --------------------------------------------------------------------------

/// Orders object entries the way the reference enumerates own properties:
/// array-index keys first in ascending numeric order, then the rest in
/// insertion order.
///
/// `JSON.stringify` walks `OwnPropertyKeys`, so this reordering is visible in
/// every saved file. A maxent POS context is the everyday case: its `tagWindow`
/// is built as `{'0':…, '-2':…, '-1':…, '1':…, '2':…}` and serialises as
/// `{"0":…,"1":…,"2":…,"-2":…,"-1":…}`, because `-2` and `-1` are not indices.
fn own_key_order(fields: &[(String, DynValue)]) -> Vec<&(String, DynValue)> {
    let mut indices: Vec<(u32, &(String, DynValue))> = Vec::new();
    let mut rest: Vec<&(String, DynValue)> = Vec::new();
    for field in fields {
        if crate::ordmap::is_array_index(&field.0) {
            indices.push((field.0.parse().expect("checked by is_array_index"), field));
        } else {
            rest.push(field);
        }
    }
    indices.sort_by_key(|(n, _)| *n);
    let mut out: Vec<&(String, DynValue)> = indices.into_iter().map(|(_, f)| f).collect();
    out.extend(rest);
    out
}

/// Writes `value` as `JSON.stringify` would: own-property order, no sorting.
fn write_json(out: &mut String, value: &DynValue) -> bool {
    match value {
        DynValue::Obj(fields) => {
            out.push('{');
            let mut first = true;
            for (k, v) in own_key_order(fields) {
                if matches!(v, DynValue::Undefined) {
                    continue;
                }
                if !first {
                    out.push(',');
                }
                first = false;
                write_string(out, k);
                out.push(':');
                write_json(out, v);
            }
            out.push('}');
            true
        }
        DynValue::Arr(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                if !write_json(out, item) {
                    out.push_str("null");
                }
            }
            out.push(']');
            true
        }
        other => write_stable(out, other),
    }
}

/// Writes `value` as `JSON.stringify(value, null, indent)` would.
///
/// The maxent `save()` uses an indent of 2, and the reference's spec files read
/// those bytes back, so the pretty form is part of the observable contract.
pub fn json_stringify_pretty(value: &DynValue, indent: usize) -> Option<String> {
    let mut out = String::new();
    if write_pretty(&mut out, value, indent, 0) {
        Some(out)
    } else {
        None
    }
}

fn write_pretty(out: &mut String, value: &DynValue, indent: usize, depth: usize) -> bool {
    let pad = |out: &mut String, d: usize| out.extend(std::iter::repeat_n(' ', indent * d));
    match value {
        DynValue::Obj(fields) => {
            let live: Vec<&(String, DynValue)> = own_key_order(fields)
                .into_iter()
                .filter(|(_, v)| !matches!(v, DynValue::Undefined))
                .collect();
            if live.is_empty() {
                out.push_str("{}");
                return true;
            }
            out.push_str("{\n");
            for (i, (k, v)) in live.iter().enumerate() {
                if i > 0 {
                    out.push_str(",\n");
                }
                pad(out, depth + 1);
                write_string(out, k);
                out.push_str(": ");
                write_pretty(out, v, indent, depth + 1);
            }
            out.push('\n');
            pad(out, depth);
            out.push('}');
            true
        }
        DynValue::Arr(items) => {
            if items.is_empty() {
                out.push_str("[]");
                return true;
            }
            out.push_str("[\n");
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push_str(",\n");
                }
                pad(out, depth + 1);
                if !write_pretty(out, item, indent, depth + 1) {
                    out.push_str("null");
                }
            }
            out.push('\n');
            pad(out, depth);
            out.push(']');
            true
        }
        other => write_stable(out, other),
    }
}

// --------------------------------------------------------------------------
// JSON.parse
// --------------------------------------------------------------------------

/// Why a document could not be parsed as JSON.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    /// Byte offset at which parsing stopped.
    pub offset: usize,
    /// What was expected there.
    pub message: &'static str,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} at byte {}", self.message, self.offset)
    }
}

impl std::error::Error for ParseError {}

impl DynValue {
    /// `JSON.parse`, producing a [`DynValue`] with object keys in document order.
    ///
    /// # Why not `serde_json`
    ///
    /// `serde_json`'s float parser is off by one ULP on some 17-significant-digit
    /// literals — the workspace's fixture generator carries fractional numbers as
    /// decimal *strings* for exactly this reason. A restored classifier's `theta`
    /// is full of such literals, so parsing them through `serde_json` would give
    /// a model that scores fractionally differently from the one that was saved.
    /// Rust's own `str::parse::<f64>` is correctly rounded, so the numbers here
    /// go through it directly.
    ///
    /// # Errors
    ///
    /// Returns [`ParseError`] on malformed input.
    pub fn parse(input: &str) -> Result<Self, ParseError> {
        let bytes = input.as_bytes();
        let mut pos = 0usize;
        let value = parse_value(input, bytes, &mut pos)?;
        skip_ws(bytes, &mut pos);
        if pos != bytes.len() {
            return Err(ParseError {
                offset: pos,
                message: "trailing characters",
            });
        }
        Ok(value)
    }
}

fn skip_ws(bytes: &[u8], pos: &mut usize) {
    while *pos < bytes.len() && matches!(bytes[*pos], b' ' | b'\t' | b'\n' | b'\r') {
        *pos += 1;
    }
}

fn expect(
    bytes: &[u8],
    pos: &mut usize,
    want: u8,
    message: &'static str,
) -> Result<(), ParseError> {
    if *pos < bytes.len() && bytes[*pos] == want {
        *pos += 1;
        Ok(())
    } else {
        Err(ParseError {
            offset: *pos,
            message,
        })
    }
}

fn parse_value(input: &str, bytes: &[u8], pos: &mut usize) -> Result<DynValue, ParseError> {
    skip_ws(bytes, pos);
    match bytes.get(*pos) {
        Some(b'{') => {
            *pos += 1;
            let mut fields = Vec::new();
            skip_ws(bytes, pos);
            if bytes.get(*pos) == Some(&b'}') {
                *pos += 1;
                return Ok(DynValue::Obj(fields));
            }
            loop {
                skip_ws(bytes, pos);
                let key = parse_string(input, bytes, pos)?;
                skip_ws(bytes, pos);
                expect(bytes, pos, b':', "expected ':'")?;
                let value = parse_value(input, bytes, pos)?;
                fields.push((key, value));
                skip_ws(bytes, pos);
                match bytes.get(*pos) {
                    Some(b',') => *pos += 1,
                    Some(b'}') => {
                        *pos += 1;
                        return Ok(DynValue::Obj(fields));
                    }
                    _ => {
                        return Err(ParseError {
                            offset: *pos,
                            message: "expected ',' or '}'",
                        });
                    }
                }
            }
        }
        Some(b'[') => {
            *pos += 1;
            let mut items = Vec::new();
            skip_ws(bytes, pos);
            if bytes.get(*pos) == Some(&b']') {
                *pos += 1;
                return Ok(DynValue::Arr(items));
            }
            loop {
                items.push(parse_value(input, bytes, pos)?);
                skip_ws(bytes, pos);
                match bytes.get(*pos) {
                    Some(b',') => *pos += 1,
                    Some(b']') => {
                        *pos += 1;
                        return Ok(DynValue::Arr(items));
                    }
                    _ => {
                        return Err(ParseError {
                            offset: *pos,
                            message: "expected ',' or ']'",
                        });
                    }
                }
            }
        }
        Some(b'"') => Ok(DynValue::Str(parse_string(input, bytes, pos)?)),
        Some(b't') if input[*pos..].starts_with("true") => {
            *pos += 4;
            Ok(DynValue::Bool(true))
        }
        Some(b'f') if input[*pos..].starts_with("false") => {
            *pos += 5;
            Ok(DynValue::Bool(false))
        }
        Some(b'n') if input[*pos..].starts_with("null") => {
            *pos += 4;
            Ok(DynValue::Null)
        }
        Some(_) => parse_number(input, bytes, pos),
        None => Err(ParseError {
            offset: *pos,
            message: "unexpected end of input",
        }),
    }
}

fn parse_number(input: &str, bytes: &[u8], pos: &mut usize) -> Result<DynValue, ParseError> {
    let start = *pos;
    if bytes.get(*pos) == Some(&b'-') {
        *pos += 1;
    }
    while matches!(bytes.get(*pos), Some(c) if c.is_ascii_digit()) {
        *pos += 1;
    }
    if bytes.get(*pos) == Some(&b'.') {
        *pos += 1;
        while matches!(bytes.get(*pos), Some(c) if c.is_ascii_digit()) {
            *pos += 1;
        }
    }
    if matches!(bytes.get(*pos), Some(b'e' | b'E')) {
        *pos += 1;
        if matches!(bytes.get(*pos), Some(b'+' | b'-')) {
            *pos += 1;
        }
        while matches!(bytes.get(*pos), Some(c) if c.is_ascii_digit()) {
            *pos += 1;
        }
    }
    input[start..*pos]
        .parse::<f64>()
        .map(DynValue::Num)
        .map_err(|_| ParseError {
            offset: start,
            message: "invalid number",
        })
}

fn parse_string(input: &str, bytes: &[u8], pos: &mut usize) -> Result<String, ParseError> {
    expect(bytes, pos, b'"', "expected a string")?;
    let mut out = String::new();
    loop {
        match bytes.get(*pos) {
            None => {
                return Err(ParseError {
                    offset: *pos,
                    message: "unterminated string",
                });
            }
            Some(b'"') => {
                *pos += 1;
                return Ok(out);
            }
            Some(b'\\') => {
                *pos += 1;
                let escape = bytes.get(*pos).copied().ok_or(ParseError {
                    offset: *pos,
                    message: "unterminated escape",
                })?;
                *pos += 1;
                match escape {
                    b'"' => out.push('"'),
                    b'\\' => out.push('\\'),
                    b'/' => out.push('/'),
                    b'b' => out.push('\u{8}'),
                    b'f' => out.push('\u{c}'),
                    b'n' => out.push('\n'),
                    b'r' => out.push('\r'),
                    b't' => out.push('\t'),
                    b'u' => {
                        let unit = parse_hex4(input, pos)?;
                        // Surrogate pairs must be recombined; a lone surrogate
                        // has no Rust representation and becomes U+FFFD, the
                        // same substitution the workspace records as divergence
                        // D2 elsewhere.
                        if (0xD800..0xDC00).contains(&unit)
                            && bytes.get(*pos) == Some(&b'\\')
                            && bytes.get(*pos + 1) == Some(&b'u')
                        {
                            let save = *pos;
                            *pos += 2;
                            let low = parse_hex4(input, pos)?;
                            if (0xDC00..0xE000).contains(&low) {
                                let c = 0x1_0000
                                    + ((u32::from(unit) - 0xD800) << 10)
                                    + (u32::from(low) - 0xDC00);
                                out.push(char::from_u32(c).unwrap_or('\u{fffd}'));
                                continue;
                            }
                            *pos = save;
                        }
                        out.push(char::from_u32(u32::from(unit)).unwrap_or('\u{fffd}'));
                    }
                    _ => {
                        return Err(ParseError {
                            offset: *pos,
                            message: "unknown escape",
                        });
                    }
                }
            }
            Some(_) => {
                let c = input[*pos..]
                    .chars()
                    .next()
                    .expect("pos is a char boundary");
                out.push(c);
                *pos += c.len_utf8();
            }
        }
    }
}

fn parse_hex4(input: &str, pos: &mut usize) -> Result<u16, ParseError> {
    let end = *pos + 4;
    let slice = input.get(*pos..end).ok_or(ParseError {
        offset: *pos,
        message: "truncated \\u escape",
    })?;
    let value = u16::from_str_radix(slice, 16).map_err(|_| ParseError {
        offset: *pos,
        message: "invalid \\u escape",
    })?;
    *pos = end;
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obj(pairs: &[(&str, DynValue)]) -> DynValue {
        DynValue::Obj(
            pairs
                .iter()
                .map(|(k, v)| ((*k).to_owned(), v.clone()))
                .collect(),
        )
    }

    // 3.14 is a fixture input, not an approximation of pi.
    #[allow(clippy::approx_constant)]
    #[test]
    fn number_to_string_matches_the_reference() {
        // Values recorded from the reference engine; the exponent-form thresholds are the point.
        assert_eq!(number_to_string(0.0), "0");
        assert_eq!(number_to_string(-0.0), "0");
        assert_eq!(number_to_string(1.0), "1");
        assert_eq!(number_to_string(-1.0), "-1");
        assert_eq!(number_to_string(3.14), "3.14");
        assert_eq!(number_to_string(100.0), "100");
        assert_eq!(number_to_string(1e21), "1e+21");
        assert_eq!(number_to_string(1e20), "100000000000000000000");
        assert_eq!(number_to_string(1e-6), "0.000001");
        assert_eq!(number_to_string(1e-7), "1e-7");
        assert_eq!(number_to_string(1.5e-7), "1.5e-7");
        assert_eq!(number_to_string(1.5e300), "1.5e+300");
        assert_eq!(number_to_string(0.1), "0.1");
        assert_eq!(number_to_string(1.0 / 3.0), "0.3333333333333333");
        assert_eq!(number_to_string(f64::NAN), "NaN");
        assert_eq!(number_to_string(f64::INFINITY), "Infinity");
        assert_eq!(number_to_string(f64::NEG_INFINITY), "-Infinity");
        assert_eq!(number_to_string(5e-324), "5e-324");
        assert_eq!(
            number_to_string(1.7976931348623157e308),
            "1.7976931348623157e+308"
        );
    }

    #[test]
    fn stable_stringify_sorts_keys_by_utf16() {
        let v = obj(&[
            ("b", DynValue::Num(1.0)),
            ("a", DynValue::Num(2.0)),
            ("-1", DynValue::Str("z".into())),
            ("0", DynValue::Str("q".into())),
            ("2", DynValue::Str("w".into())),
        ]);
        assert_eq!(
            v.stable_stringify().unwrap(),
            r#"{"-1":"z","0":"q","2":"w","a":2,"b":1}"#
        );
    }

    #[test]
    fn stable_stringify_puts_astral_before_replacement_char() {
        // 😀 is U+1F600, whose UTF-16 lead unit 0xD83D is below U+FFFD.
        let v = obj(&[
            ("\u{fffd}", DynValue::Num(1.0)),
            ("\u{1f600}", DynValue::Num(2.0)),
        ]);
        let s = v.stable_stringify().unwrap();
        assert!(
            s.find('\u{1f600}').unwrap() < s.find('\u{fffd}').unwrap(),
            "{s}"
        );
    }

    #[test]
    fn stable_stringify_edge_values() {
        assert_eq!(
            DynValue::Str("0".into()).stable_stringify().unwrap(),
            "\"0\""
        );
        assert_eq!(DynValue::Num(0.0).stable_stringify().unwrap(), "0");
        assert_eq!(DynValue::Num(-0.0).stable_stringify().unwrap(), "0");
        assert_eq!(DynValue::Null.stable_stringify().unwrap(), "null");
        assert_eq!(DynValue::Num(f64::NAN).stable_stringify().unwrap(), "null");
        assert_eq!(
            DynValue::Num(f64::INFINITY).stable_stringify().unwrap(),
            "null"
        );
        assert_eq!(DynValue::Undefined.stable_stringify(), None);
        assert_eq!(
            DynValue::Arr(vec![
                DynValue::Num(1.0),
                DynValue::Str("a".into()),
                DynValue::Undefined
            ])
            .stable_stringify()
            .unwrap(),
            r#"[1,"a",null]"#
        );
        assert_eq!(
            DynValue::Str("café😀".into()).stable_stringify().unwrap(),
            "\"café😀\""
        );
        assert_eq!(
            DynValue::Str("a\"b\\c\nd".into())
                .stable_stringify()
                .unwrap(),
            "\"a\\\"b\\\\c\\nd\""
        );
    }

    #[test]
    fn to_reference_string_reproduces_plus_coercion() {
        assert_eq!(DynValue::Num(5.0).to_text(), "5");
        assert_eq!(DynValue::Null.to_text(), "null");
        assert_eq!(DynValue::Undefined.to_text(), "undefined");
        assert_eq!(
            DynValue::Arr(vec![DynValue::Str("p".into()), DynValue::Str("q".into())]).to_text(),
            "p,q"
        );
        assert_eq!(
            obj(&[("z", DynValue::Num(1.0))]).to_text(),
            "[object Object]"
        );
    }

    #[test]
    fn json_stringify_keeps_insertion_order() {
        let v = obj(&[("b", DynValue::Num(1.0)), ("a", DynValue::Num(2.0))]);
        assert_eq!(v.json_stringify().unwrap(), r#"{"b":1,"a":2}"#);
    }

    #[test]
    fn parse_round_trips_and_keeps_key_order() {
        let src = r#"{"b":1,"a":[true,false,null,"x\ny"],"n":-1.5e3}"#;
        let v = DynValue::parse(src).unwrap();
        assert_eq!(
            v.json_stringify().unwrap(),
            r#"{"b":1,"a":[true,false,null,"x\ny"],"n":-1500}"#
        );
    }

    #[test]
    fn parse_uses_a_correctly_rounded_float_reader() {
        // The literal serde_json is documented to misread by one ULP.
        let v = DynValue::parse("0.41798941798941797").unwrap();
        assert_eq!(
            v,
            DynValue::Num("0.41798941798941797".parse::<f64>().unwrap())
        );
    }

    #[test]
    fn parse_handles_escapes_and_surrogate_pairs() {
        let v = DynValue::parse(r#""😀 café""#).unwrap();
        assert_eq!(v.as_str(), Some("😀 café"));
        assert!(DynValue::parse(r#"{"a":}"#).is_err());
        assert!(DynValue::parse("[1,2").is_err());
        assert!(DynValue::parse("1 2").is_err());
    }

    #[test]
    fn pretty_matches_two_space_indent() {
        let v = obj(&[("a", DynValue::Arr(vec![DynValue::Num(1.0)]))]);
        assert_eq!(
            json_stringify_pretty(&v, 2).unwrap(),
            "{\n  \"a\": [\n    1\n  ]\n}"
        );
        assert_eq!(
            json_stringify_pretty(&DynValue::Obj(vec![]), 2).unwrap(),
            "{}"
        );
    }
}
