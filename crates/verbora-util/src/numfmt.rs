//! The reference number formatting, reproduced exactly.
//!
//! The graph types depend on two the reference numeric primitives whose Rust
//! equivalents are *not* interchangeable with the obvious `format!` calls:
//!
//! * [`format_s`] / [`number_to_string`] — `util.format('%s', n)`, which is
//!   `Number::toString`. Rust's `{}` never uses exponential notation and Rust's
//!   `{:e}` always does; the reference switches at 1e21 upward and 1e-7 downward.
//!   `DirectedEdge::toString` and therefore `EdgeWeightedDigraph::toString` are
//!   built out of it, so `0.40` must print as `0.4` and `1e21` as `1e+21`.
//!
//! * [`snap_2`] — `parseFloat(n.toFixed(2))`, which `ShortestPathTree` and
//!   `LongestPathTree` apply to **every accepted distance**. That is a numeric
//!   transformation, not a display one: it is why the canonical shortest-path
//!   distance to vertex 2 is `0.62` and not `0.6200000000000001`.
//!
//! # Why `format!("{:.2}", x)` is the wrong primitive
//!
//! `Number.prototype.toFixed` rounds ties **away from zero**, on the *exact*
//! binary value of the double. Rust's formatting rounds ties to even. They
//! disagree on every value that lands exactly on a half:
//!
//! ```text
//! (0.125).toFixed(2)          "0.13"     the reference — larger n wins
//! format!("{:.2}", 0.125)     "0.12"     Rust          — round half to even
//! ```
//!
//! A naive port would have used `(x * 100.0).round() / 100.0`, which is wrong in
//! a third way: the multiplication itself rounds, so `0.145 * 100.0` is
//! `14.499999999999998` and rounds down where the reference's exact-decimal
//! comparison rounds up.

use std::fmt::{self, Write as _};

/// A fixed-capacity, stack-allocated [`fmt::Write`] sink.
///
/// The alternative is two `String` allocations per relaxed edge. Every string
/// this module builds has a hard length bound (see the call sites), so a fixed
/// buffer is sufficient rather than merely convenient.
struct StackBuf<const N: usize> {
    bytes: [u8; N],
    len: usize,
}

impl<const N: usize> StackBuf<N> {
    const fn new() -> Self {
        Self {
            bytes: [0; N],
            len: 0,
        }
    }

    /// The written bytes. Always valid UTF-8: only ASCII is ever written.
    fn as_str(&self) -> &str {
        std::str::from_utf8(&self.bytes[..self.len]).unwrap_or("")
    }
}

impl<const N: usize> fmt::Write for StackBuf<N> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let b = s.as_bytes();
        if self.len + b.len() > N {
            return Err(fmt::Error);
        }
        self.bytes[self.len..self.len + b.len()].copy_from_slice(b);
        self.len += b.len();
        Ok(())
    }
}

/// Formats `x` the way `String(x)` does — ECMA-262 `Number::toString`, radix 10.
///
/// Note that `-0` renders as `"0"` here, matching `String(-0)` and
/// `JSON.stringify(-0)`. Node's `util.format('%s', …)` differs; use [`format_s`]
/// for that, which is what `DirectedEdge::toString` needs.
///
/// # The algorithm
///
/// ECMA-262 asks for the *shortest* digit string `s` (with `k` digits) and
/// exponent `n` such that `s × 10^(n-k)` round-trips to `x`, then picks a layout
/// by comparing `n` against `k`, 0 and 21. Rust's `{:e}` produces exactly that
/// shortest digit string, so the digits are borrowed from it and only the layout
/// is re-derived.
pub fn number_to_string(x: f64) -> String {
    let mut out = String::with_capacity(24);
    write_number(&mut out, x);
    out
}

/// Formats `x` the way Node's `util.format('%s', x)` does.
///
/// Identical to [`number_to_string`] except for negative zero, which Node
/// renders as `"-0"` — the documented exception in its `%s` handling. The reference
/// builds `DirectedEdge::toString` out of `util.format`, so an edge weighted
/// `-0` prints `"5 -> 4, -0"` where `String(-0)` would have said `"0"`.
pub fn format_s(x: f64) -> String {
    if x == 0.0 && x.is_sign_negative() {
        return "-0".to_owned();
    }
    number_to_string(x)
}

/// Appends `number_to_string(x)` to `out`, without an intermediate allocation.
fn write_number(out: &mut String, x: f64) {
    if x.is_nan() {
        out.push_str("NaN");
        return;
    }
    if x == 0.0 {
        // Covers -0 as well: ECMA-262 renders both as "0".
        out.push('0');
        return;
    }
    if x < 0.0 {
        out.push('-');
        write_number(out, -x);
        return;
    }
    if x.is_infinite() {
        out.push_str("Infinity");
        return;
    }

    // `{:e}` yields the shortest round-tripping digits as "d[.ddd]e<exp>".
    // 17 significant digits plus punctuation and a 4-digit exponent fits in 32.
    let mut buf = StackBuf::<32>::new();
    let _ = write!(buf, "{x:e}");
    let s = buf.as_str();

    let (mantissa, exp) = s.split_once('e').unwrap_or((s, "0"));
    let exp: i32 = exp.parse().unwrap_or(0);

    // Digits with the point removed; `k` in the specification.
    let mut digits = StackBuf::<24>::new();
    for c in mantissa.chars().filter(|c| *c != '.') {
        let _ = digits.write_char(c);
    }
    let digits = digits.as_str();
    let k = digits.len() as i32;
    // `n` is the position of the decimal point relative to the digit string.
    let n = exp + 1;

    if k <= n && n <= 21 {
        out.push_str(digits);
        for _ in 0..(n - k) {
            out.push('0');
        }
    } else if 0 < n && n <= 21 {
        let split = n as usize;
        out.push_str(&digits[..split]);
        out.push('.');
        out.push_str(&digits[split..]);
    } else if -6 < n && n <= 0 {
        out.push_str("0.");
        for _ in 0..(-n) {
            out.push('0');
        }
        out.push_str(digits);
    } else {
        // Exponential form. The exponent printed is `n - 1`, always signed.
        if k == 1 {
            out.push_str(digits);
        } else {
            out.push_str(&digits[..1]);
            out.push('.');
            out.push_str(&digits[1..]);
        }
        out.push('e');
        let e = n - 1;
        if e >= 0 {
            out.push('+');
        } else {
            out.push('-');
        }
        let _ = write!(out, "{}", e.unsigned_abs());
    }
}

/// Formats `x` the way `x.toFixed(2)` does.
///
/// Follows ECMA-262 `Number.prototype.toFixed` step for step: `NaN` short
/// circuits, the sign is split off first (so `-0` yields `"0.00"` and not
/// `"-0.00"`), magnitudes at or above 1e21 fall back to `ToString`, and
/// everything else picks the integer `n` minimising `|n / 100 - x|`, **choosing
/// the larger `n` on a tie**.
pub fn to_fixed_2(x: f64) -> String {
    let mut out = String::with_capacity(26);
    write_to_fixed_2(&mut out, x);
    out
}

/// Appends `to_fixed_2(x)` to `out`.
fn write_to_fixed_2(out: &mut String, x: f64) {
    if x.is_nan() {
        out.push_str("NaN");
        return;
    }
    let neg = x < 0.0;
    // `-0` is not negative by the specification's `x < 0` test, and normalising
    // it here keeps the `{:.30}` rendering below from growing a stray sign.
    let mag = if x == 0.0 {
        0.0
    } else if neg {
        -x
    } else {
        x
    };
    if neg {
        out.push('-');
    }
    if mag >= 1e21 {
        write_number(out, mag);
        return;
    }

    // The exact decimal expansion, correctly rounded to 30 fractional digits.
    //
    // 30 is far more than a tie test needs. A double whose expansion sits
    // strictly between two 2-decimal values but agrees with a tie to 30 digits
    // would have to be within 1e-30 of that tie; for any magnitude at which the
    // third fractional digit can even be a 5 (>= 0.005) the spacing between
    // adjacent doubles is above 4e-19, so no such double exists. Rounding at
    // digit 30 can only carry *upward*, and a carry that reaches digit 3 means
    // the exact digit was a 9 — which rounds up to the same answer anyway.
    //
    // 21 integer digits + '.' + 30 fractional digits = 52.
    let mut buf = StackBuf::<64>::new();
    let _ = write!(buf, "{mag:.30}");
    let s = buf.as_str();
    let dot = s.find('.').unwrap_or(s.len());
    let int_part = &s[..dot];
    let frac = s.get(dot + 1..).unwrap_or("");

    // Digits of `n`: the integer part followed by the first two fractional
    // digits. 21 + 2 + 1 for a carry.
    let mut digits = [b'0'; 24];
    let mut len = 0;
    for &b in int_part.as_bytes() {
        digits[len] = b;
        len += 1;
    }
    for i in 0..2 {
        digits[len] = frac.as_bytes().get(i).copied().unwrap_or(b'0');
        len += 1;
    }

    // Round half away from zero: the specification's "pick the larger n" makes
    // an exact tie behave identically to anything above it, so a single
    // `>= '5'` test on the third fractional digit covers both.
    if frac.as_bytes().get(2).copied().unwrap_or(b'0') >= b'5' {
        let mut i = len;
        loop {
            if i == 0 {
                // Carried past the most significant digit: shift right and
                // prepend a 1 (0.999… -> 1.00).
                digits.copy_within(0..len, 1);
                digits[0] = b'1';
                len += 1;
                break;
            }
            i -= 1;
            if digits[i] == b'9' {
                digits[i] = b'0';
            } else {
                digits[i] += 1;
                break;
            }
        }
    }

    // Place the decimal point two digits from the right, padding so there is
    // always at least one integer digit.
    let text = std::str::from_utf8(&digits[..len]).unwrap_or("0");
    let (int_out, frac_out) = text.split_at(len - 2);
    if int_out.is_empty() {
        out.push('0');
    } else {
        out.push_str(int_out);
    }
    out.push('.');
    out.push_str(frac_out);
}

/// Computes `parseFloat(x.toFixed(2))` — the snap the path trees apply to every
/// accepted distance.
///
/// Two shortcuts avoid formatting where the answer is provably `x` itself:
/// non-finite values render as `"NaN"`/`"Infinity"`/`"-Infinity"` and parse
/// straight back, and magnitudes at or above 1e21 render via `ToString`, whose
/// output is by construction the shortest string that round-trips to `x`.
pub fn snap_2(x: f64) -> f64 {
    if !x.is_finite() || x.abs() >= 1e21 {
        return x;
    }
    let mut out = String::with_capacity(26);
    write_to_fixed_2(&mut out, x);
    // The text is always a plain decimal literal here, and Rust's parser is
    // correctly rounded, as `parseFloat` is.
    out.parse().unwrap_or(f64::NAN)
}

/// Formats `x` the way `JSON.stringify(x)` does.
///
/// Differs from [`number_to_string`] only for non-finite values, which JSON has
/// no syntax for and renders as `null`. `Topological`'s cycle message is built
/// with `JSON.stringify`, so this is what a numeric vertex contributes to it.
pub fn json_number(x: f64) -> String {
    if x.is_finite() {
        number_to_string(x)
    } else {
        "null".to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integers_and_zero() {
        assert_eq!(number_to_string(0.0), "0");
        assert_eq!(number_to_string(-0.0), "0");
        assert_eq!(format_s(-0.0), "-0");
        assert_eq!(number_to_string(1.0), "1");
        assert_eq!(number_to_string(-1.0), "-1");
        assert_eq!(number_to_string(100.0), "100");
        assert_eq!(number_to_string(1e6), "1000000");
    }

    #[test]
    fn exponent_thresholds() {
        assert_eq!(number_to_string(1e20), "100000000000000000000");
        assert_eq!(number_to_string(1e21), "1e+21");
        assert_eq!(number_to_string(1e-6), "0.000001");
        assert_eq!(number_to_string(1e-7), "1e-7");
        assert_eq!(number_to_string(5e-324), "5e-324");
        assert_eq!(number_to_string(f64::MAX), "1.7976931348623157e+308");
    }

    #[test]
    fn shortest_round_tripping_digits() {
        assert_eq!(number_to_string(0.4), "0.4");
        assert_eq!(number_to_string(0.1 + 0.2), "0.30000000000000004");
        assert_eq!(number_to_string(1.0 / 3.0), "0.3333333333333333");
        // The reference prints the shortest form, not the exact binary value.
        assert_eq!(
            number_to_string(123456789012345678901.0),
            "123456789012345680000"
        );
    }

    #[test]
    fn non_finite() {
        assert_eq!(number_to_string(f64::NAN), "NaN");
        assert_eq!(number_to_string(f64::INFINITY), "Infinity");
        assert_eq!(number_to_string(f64::NEG_INFINITY), "-Infinity");
        assert_eq!(json_number(f64::NAN), "null");
        assert_eq!(json_number(f64::INFINITY), "null");
        assert_eq!(json_number(-0.0), "0");
    }

    #[test]
    fn to_fixed_rounds_ties_away_from_zero() {
        // The whole reason this module exists: `format!("{:.2}", 0.125)` is
        // "0.12" because Rust rounds half to even.
        assert_eq!(to_fixed_2(0.125), "0.13");
        assert_eq!(to_fixed_2(-0.125), "-0.13");
        // ...while values that merely *look* like ties are not ties at all: the
        // double nearest 1.005 is 1.00499999999999989, so it rounds down.
        assert_eq!(to_fixed_2(1.005), "1.00");
        assert_eq!(to_fixed_2(2.675), "2.67");
        assert_eq!(to_fixed_2(9.995), "9.99");
        assert_eq!(to_fixed_2(0.005), "0.01");
        assert_eq!(to_fixed_2(0.015), "0.01");
    }

    #[test]
    fn to_fixed_edges() {
        assert_eq!(to_fixed_2(0.0), "0.00");
        assert_eq!(to_fixed_2(-0.0), "0.00");
        assert_eq!(to_fixed_2(-1.0), "-1.00");
        assert_eq!(to_fixed_2(0.4), "0.40");
        assert_eq!(to_fixed_2(1e20), "100000000000000000000.00");
        assert_eq!(to_fixed_2(1e21), "1e+21");
        assert_eq!(to_fixed_2(f64::MAX), "1.7976931348623157e+308");
        assert_eq!(to_fixed_2(f64::NAN), "NaN");
        assert_eq!(to_fixed_2(f64::INFINITY), "Infinity");
        assert_eq!(to_fixed_2(f64::NEG_INFINITY), "-Infinity");
        assert_eq!(to_fixed_2(1e-7), "0.00");
        assert_eq!(to_fixed_2(0.999), "1.00");
        assert_eq!(to_fixed_2(0.9999), "1.00");
        assert_eq!(
            to_fixed_2(123456789012345678901.0),
            "123456789012345683968.00"
        );
    }

    #[test]
    fn snap_matches_the_spec_distances() {
        // 0.28 + 0.34 is 0.6200000000000001 in binary; the snap is what makes
        // the canonical shortest-path distance 0.62.
        assert_eq!(snap_2(0.28 + 0.34), 0.62);
        assert_eq!(snap_2(0.32 + 0.29), 0.61);
        assert_eq!(snap_2(0.61 + 0.52), 1.13);
        assert_eq!(snap_2(0.0), 0.0);
        assert!(snap_2(f64::NAN).is_nan());
        assert_eq!(snap_2(f64::INFINITY), f64::INFINITY);
        assert_eq!(snap_2(f64::MAX), f64::MAX);
        assert_eq!(snap_2(0.125), 0.13);
    }

    #[test]
    fn snap_of_a_small_negative_keeps_the_sign_bit() {
        // parseFloat("-0.00") is -0, distinguishable via `is_sign_negative`.
        let v = snap_2(-0.001);
        assert_eq!(v, 0.0);
        assert!(v.is_sign_negative());
    }
}
