//! The two the reference number conversions `CountInflector` depends on.
//!
//! `nth` is three tokens long — `i.toString() + this.nthForm(i)` — and both
//! halves are the reference-specific:
//!
//! * `Number.prototype.toString` is not Rust's `Display`. It switches to
//!   exponential notation outside `1e-7 … 1e21`, and spells the specials
//!   `NaN`, `Infinity`, `-Infinity` where Rust writes `NaN`, `inf`, `-inf`.
//!   Recorded: `nth(1e21) === "1e+21th"`, `nth(1e-7) === "1e-7th"`,
//!   `nth(Infinity) === "Infinityth"`. Rust's `{}` would print
//!   `1000000000000000000000th` and `infth`.
//! * `i % 100` applies `ToNumber` to whatever it is given, so
//!   `CountInflector.nth("11")` really is `"11th"` and `nth("abc")` is
//!   `"abcth"` (the remainder is `NaN`, every comparison fails, and the switch
//!   falls through to `'th'`).

use std::fmt::Write;

use verbora_core::is_whitespace;

/// `Number::toString(x, 10)`.
///
/// Implemented on top of Rust's shortest-round-trip `{:e}` formatting, which
/// supplies exactly the digit string and exponent the specification's step 5
/// asks for; the remaining work is choosing between the five layouts.
pub(crate) fn number_to_string(x: f64) -> String {
    if x.is_nan() {
        return "NaN".to_owned();
    }
    if x == 0.0 {
        // Covers -0, which the reference also renders as "0".
        return "0".to_owned();
    }
    if x < 0.0 {
        let mut s = String::with_capacity(24);
        s.push('-');
        s.push_str(&number_to_string(-x));
        return s;
    }
    if x.is_infinite() {
        return "Infinity".to_owned();
    }

    // `{:e}` yields `d[.ddd]e±xx` with the shortest digits that round-trip —
    // exactly the digit string and exponent the specification's step 5 asks
    // for. The remaining work is choosing between its five layouts.
    //
    // The `format!` allocation here was measured against a stack-buffered
    // `fmt::Write` sink and made no difference: the cost is the shortest
    // round-trip conversion itself, not the buffer it lands in.
    let sci = format!("{x:e}");
    let (mantissa, exponent) = sci
        .split_once('e')
        .expect("LowerExp for f64 always emits an exponent");
    let exponent: i32 = exponent.parse().expect("LowerExp emits a decimal exponent");

    // The digit string is `head` (always one digit) followed by `tail`; keeping
    // them as two slices avoids materialising a joined copy.
    let (head, tail) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    let k = i32::try_from(head.len() + tail.len()).expect("shortest round-trip is under 20 digits");
    let n = exponent + 1;

    let mut out = String::with_capacity(k as usize + 8);
    if k <= n && n <= 21 {
        // Integral, no exponent needed: digits then n-k zeros.
        out.push_str(head);
        out.push_str(tail);
        out.extend(std::iter::repeat_n('0', (n - k) as usize));
    } else if 0 < n && n <= 21 {
        // A decimal point inside the digits, `n` of them before it.
        let split = (n - 1) as usize;
        out.push_str(head);
        out.push_str(&tail[..split]);
        out.push('.');
        out.push_str(&tail[split..]);
    } else if -6 < n && n <= 0 {
        // Leading zeros after the point; this cut-off is why (1e-6).toString()
        // is "0.000001" but (1e-7).toString() is "1e-7".
        out.push_str("0.");
        out.extend(std::iter::repeat_n('0', (-n) as usize));
        out.push_str(head);
        out.push_str(tail);
    } else {
        // Exponential form.
        out.push_str(head);
        if !tail.is_empty() {
            out.push('.');
            out.push_str(tail);
        }
        out.push('e');
        let e = n - 1;
        out.push(if e >= 0 { '+' } else { '-' });
        let _ = write!(out, "{}", e.unsigned_abs());
    }
    out
}

/// `ToNumber` applied to a string, as the `%` operator does.
///
/// Accepts what the `StringNumericLiteral` grammar accepts — surrounding
/// whitespace, an empty string (`0`), `Infinity`, and `0x`/`0o`/`0b` prefixes —
/// and returns `NaN` for everything else. Validating the grammar before
/// delegating to Rust's parser matters: Rust accepts `"inf"`, `"infinity"` and
/// `"nan"`, none of which the reference does.
pub(crate) fn to_number(s: &str) -> f64 {
    let trimmed = s.trim_matches(is_whitespace);
    if trimmed.is_empty() {
        return 0.0;
    }

    for (prefix, radix) in [
        ("0x", 16u32),
        ("0X", 16),
        ("0o", 8),
        ("0O", 8),
        ("0b", 2),
        ("0B", 2),
    ] {
        if let Some(rest) = trimmed.strip_prefix(prefix) {
            return parse_radix(rest, radix);
        }
    }

    let (sign, body) = match trimmed.as_bytes().first() {
        Some(b'-') => (-1.0, &trimmed[1..]),
        Some(b'+') => (1.0, &trimmed[1..]),
        _ => (1.0, trimmed),
    };
    if body == "Infinity" {
        return sign * f64::INFINITY;
    }
    if !is_decimal_literal(body) {
        return f64::NAN;
    }
    body.parse::<f64>().map_or(f64::NAN, |v| sign * v)
}

/// `0x…`, `0o…`, `0b…`. Exact while the digits fit in a `u128`; beyond that the
/// accumulation rounds per digit rather than once, which no realistic ordinal
/// reaches.
fn parse_radix(digits: &str, radix: u32) -> f64 {
    if digits.is_empty() {
        return f64::NAN;
    }
    let mut exact: Option<u128> = Some(0);
    let mut approx = 0.0f64;
    for c in digits.chars() {
        let Some(d) = c.to_digit(radix) else {
            return f64::NAN;
        };
        exact = exact
            .and_then(|v| v.checked_mul(u128::from(radix)))
            .and_then(|v| v.checked_add(u128::from(d)));
        approx = approx.mul_add(f64::from(radix), f64::from(d));
    }
    exact.map_or(approx, |v| v as f64)
}

/// Whether `s` is a `StrUnsignedDecimalLiteral` without `Infinity`.
fn is_decimal_literal(s: &str) -> bool {
    let b = s.as_bytes();
    let mut i = 0;
    let mut integer_digits = 0;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
        integer_digits += 1;
    }
    let mut fraction_digits = 0;
    if i < b.len() && b[i] == b'.' {
        i += 1;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
            fraction_digits += 1;
        }
    }
    if integer_digits == 0 && fraction_digits == 0 {
        return false;
    }
    if i < b.len() && b[i] | 0x20 == b'e' {
        i += 1;
        if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
            i += 1;
        }
        let mut exponent_digits = 0;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
            exponent_digits += 1;
        }
        if exponent_digits == 0 {
            return false;
        }
    }
    i == b.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integral_values_have_no_exponent_below_1e21() {
        assert_eq!(number_to_string(0.0), "0");
        assert_eq!(number_to_string(-0.0), "0");
        assert_eq!(number_to_string(1.0), "1");
        assert_eq!(number_to_string(-1.0), "-1");
        assert_eq!(number_to_string(100.0), "100");
        assert_eq!(
            number_to_string(9_007_199_254_740_991.0),
            "9007199254740991"
        );
        assert_eq!(number_to_string(1e20), "100000000000000000000");
    }

    #[test]
    fn the_exponential_thresholds_match_the_reference() {
        assert_eq!(number_to_string(1e21), "1e+21");
        assert_eq!(number_to_string(1e22), "1e+22");
        assert_eq!(number_to_string(1e-6), "0.000001");
        assert_eq!(number_to_string(1e-7), "1e-7");
        assert_eq!(number_to_string(1.5e-7), "1.5e-7");
        assert_eq!(number_to_string(f64::MAX), "1.7976931348623157e+308");
        assert_eq!(number_to_string(5e-324), "5e-324");
    }

    #[test]
    fn fractions_and_specials() {
        assert_eq!(number_to_string(1.5), "1.5");
        assert_eq!(number_to_string(0.1), "0.1");
        assert_eq!(number_to_string(-0.5), "-0.5");
        assert_eq!(number_to_string(123_456_789.123), "123456789.123");
        assert_eq!(number_to_string(f64::NAN), "NaN");
        assert_eq!(number_to_string(f64::INFINITY), "Infinity");
        assert_eq!(number_to_string(f64::NEG_INFINITY), "-Infinity");
    }

    #[test]
    fn string_coercion_follows_the_reference_grammar() {
        assert_eq!(to_number("1"), 1.0);
        assert_eq!(to_number(""), 0.0);
        assert_eq!(to_number("   "), 0.0);
        assert_eq!(to_number("  12  "), 12.0);
        assert_eq!(to_number("\t7\n"), 7.0);
        assert_eq!(to_number("1.5"), 1.5);
        assert_eq!(to_number("-11"), -11.0);
        assert_eq!(to_number("+3"), 3.0);
        assert_eq!(to_number(".5"), 0.5);
        assert_eq!(to_number("1e3"), 1000.0);
        assert_eq!(to_number("0x1f"), 31.0);
        assert_eq!(to_number("0b101"), 5.0);
        assert_eq!(to_number("0o17"), 15.0);
        assert_eq!(to_number("Infinity"), f64::INFINITY);
        assert_eq!(to_number("-Infinity"), f64::NEG_INFINITY);
    }

    #[test]
    fn rust_only_spellings_are_not_numbers_in_the_reference() {
        for s in [
            "abc",
            "12abc",
            "1,000",
            "NaN",
            "inf",
            "infinity",
            "nan",
            "1e",
            "0x",
            "--1",
            "infinity ",
            "1_000",
        ] {
            assert!(to_number(s).is_nan(), "{s:?} should coerce to NaN");
        }
    }
}
