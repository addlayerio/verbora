//! The reference's two string→number conversions, reproduced exactly.
//!
//! `CURRENT-WORD-IS-NUMBER` is one line of the reference:
//!
//! ```text
//! let isNumber = !isNaN(token)
//! if (!isNumber) isNumber = parseFloat(token)
//! return (parameter === 'YES') ? isNumber : !isNumber
//! ```
//!
//! and every part of it is a trap for a port that reaches for
//! `str::parse::<f64>()`:
//!
//! | Token | `Number()` | `parseFloat()` | Rule fires on `YES`? |
//! |---|---|---|---|
//! | `""` | `0` | — | **yes** — the empty string is a number |
//! | `"  "`, `"\n"`, `"\u{feff}"` | `0` | — | **yes** |
//! | `"0x10"`, `"0b101"`, `"0o17"` | 16 / 5 / 15 | — | yes |
//! | `"1e400"`, `"Infinity"` | `Infinity` | — | yes — infinite, not NaN |
//! | `"+5"`, `".5"`, `"5."`, `"  12  "` | 5 / 0.5 / 5 / 12 | — | yes |
//! | `"5abc"` | `NaN` | `5` | yes — a *truthy number* is returned |
//! | `"0abc"` | `NaN` | `0` | **no** — `0` is falsy, so `NO` fires instead |
//! | `"1_000"`, `"1,000"` | `NaN` | `1` | yes |
//! | `"abc"`, `"NaN"`, `"٣"` | `NaN` | `NaN` | no |
//!
//! `Number("0abc")` and `Number("abc")` are both `NaN`, but the *predicate*
//! distinguishes them, because `parseFloat` returns `0` for one and `NaN` for
//! the other and only one of those is falsy. Collapsing the two — the obvious
//! simplification — flips the rule for every token that starts with a zero.
//!
//! Both functions here follow the reference language grammar rather than delegating to
//! Rust's parser, because Rust accepts `"inf"`, `"infinity"` and `"NaN"`
//! (case-insensitively) which the reference does not, and rejects nothing that
//! The reference accepts. Validation happens first; only a syntactically valid
//! decimal literal is handed to `str::parse`, whose correctly-rounded conversion
//! is then exactly the reference engine's.

use verbora_core::whitespace::is_whitespace;

/// `Number(string)` — the reference language `StringNumericLiteral`.
///
/// Returns `NaN` for input that is not a complete numeric literal. Leading and
/// trailing whitespace uses the reference's `\s` set (which includes `U+FEFF` and
/// excludes `U+0085`), not `char::is_whitespace`.
///
/// # Precision
///
/// Decimal literals are converted with `str::parse::<f64>`, which is correctly
/// rounded and therefore bit-identical to the reference engine. Hex/binary/octal literals longer
/// than 128 bits accumulate in `f64` and can round differently from the
/// specification's exact-then-round rule; no such token occurs in any corpus
/// this crate is tested against, and the predicate only observes NaN-ness.
#[must_use]
pub fn to_number(s: &str) -> f64 {
    let t = s.trim_matches(is_whitespace);
    if t.is_empty() {
        // StrNumericLiteral is optional: the empty string converts to +0.
        return 0.0;
    }
    if let Some(rest) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        return radix_literal(rest, 16);
    }
    if let Some(rest) = t.strip_prefix("0b").or_else(|| t.strip_prefix("0B")) {
        return radix_literal(rest, 2);
    }
    if let Some(rest) = t.strip_prefix("0o").or_else(|| t.strip_prefix("0O")) {
        return radix_literal(rest, 8);
    }
    match decimal_literal_len(t) {
        Some(n) if n == t.len() => t.parse::<f64>().unwrap_or(f64::NAN),
        _ => f64::NAN,
    }
}

/// Whether `Number(string)` is `NaN` — i.e. The reference's global `isNaN`.
#[must_use]
pub fn is_nan(s: &str) -> bool {
    to_number(s).is_nan()
}

/// `parseFloat(string)`: converts the longest leading decimal literal.
///
/// Unlike [`to_number`] this ignores trailing garbage (`"5abc"` → `5`), does not
/// understand `0x`/`0b`/`0o` prefixes (`"0x10"` → `0`), and yields `NaN` when no
/// literal starts the string.
#[must_use]
pub fn parse_float(s: &str) -> f64 {
    let t = s.trim_start_matches(is_whitespace);
    match decimal_literal_len(t) {
        // `t[..n]` is a validated literal, so `parse` cannot fail.
        Some(n) => t[..n].parse::<f64>().unwrap_or(f64::NAN),
        None => f64::NAN,
    }
}

/// Length in bytes of the longest `StrDecimalLiteral` prefix of `t`, if any.
///
/// Grammar: `[+-]? ( "Infinity" | digits ("." digits?)? exp? | "." digits exp? )`
/// with `exp = [eE] [+-]? digits`. An `e` not followed by digits is *not* part of
/// the literal, so `"1e"` parses as `1` and `"1e+"` likewise.
fn decimal_literal_len(t: &str) -> Option<usize> {
    let b = t.as_bytes();
    let mut i = 0;
    if matches!(b.first(), Some(b'+' | b'-')) {
        i += 1;
    }
    if t[i..].starts_with("Infinity") {
        return Some(i + "Infinity".len());
    }
    let int_start = i;
    while matches!(b.get(i), Some(d) if d.is_ascii_digit()) {
        i += 1;
    }
    let int_digits = i - int_start;
    let mut frac_digits = 0;
    if b.get(i) == Some(&b'.') {
        i += 1;
        let frac_start = i;
        while matches!(b.get(i), Some(d) if d.is_ascii_digit()) {
            i += 1;
        }
        frac_digits = i - frac_start;
    }
    if int_digits == 0 && frac_digits == 0 {
        // Neither "123" nor ".5": no literal here (a bare "." or "+" included).
        return None;
    }
    // The exponent is only consumed when it is complete; otherwise the mantissa
    // alone is the longest valid prefix.
    if matches!(b.get(i), Some(b'e' | b'E')) {
        let mut j = i + 1;
        if matches!(b.get(j), Some(b'+' | b'-')) {
            j += 1;
        }
        let digits_start = j;
        while matches!(b.get(j), Some(d) if d.is_ascii_digit()) {
            j += 1;
        }
        if j > digits_start {
            i = j;
        }
    }
    Some(i)
}

/// `NonDecimalIntegerLiteral` after its `0x`/`0b`/`0o` prefix has been stripped.
fn radix_literal(digits: &str, radix: u32) -> f64 {
    if digits.is_empty() {
        return f64::NAN;
    }
    let mut acc_exact: u128 = 0;
    let mut acc_f = 0.0f64;
    let mut overflowed = false;
    for c in digits.chars() {
        let Some(d) = c.to_digit(radix) else {
            return f64::NAN;
        };
        if !overflowed {
            match acc_exact
                .checked_mul(u128::from(radix))
                .and_then(|v| v.checked_add(u128::from(d)))
            {
                Some(v) => acc_exact = v,
                None => {
                    overflowed = true;
                    acc_f = acc_exact as f64;
                }
            }
        }
        if overflowed {
            acc_f = acc_f.mul_add(f64::from(radix), f64::from(d));
        }
    }
    if overflowed { acc_f } else { acc_exact as f64 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[track_caller]
    fn num(s: &str, expected: f64) {
        let got = to_number(s);
        assert!(
            (got.is_nan() && expected.is_nan()) || got == expected,
            "Number({s:?}) = {got}, want {expected}"
        );
    }

    #[test]
    fn empty_and_whitespace_are_zero() {
        // The single most surprising row: the empty string IS a number in the reference.
        num("", 0.0);
        num(" ", 0.0);
        num("\t\n\r", 0.0);
        num("\u{00a0}", 0.0);
        num("\u{2028}", 0.0);
        num("\u{feff}", 0.0);
        // U+0085 is whitespace to Rust but NOT to the reference, so it stays NaN.
        num("\u{0085}", f64::NAN);
    }

    #[test]
    fn radix_prefixes() {
        num("0x10", 16.0);
        num("0X1f", 31.0);
        num("0b101", 5.0);
        num("0o17", 15.0);
        num("0x", f64::NAN);
        num("0xg", f64::NAN);
        num("-0x10", f64::NAN); // signs are not allowed on non-decimal literals
    }

    #[test]
    fn decimal_forms() {
        num("+5", 5.0);
        num("-5", -5.0);
        num(".5", 0.5);
        num("5.", 5.0);
        num("1e5", 100_000.0);
        num("1e400", f64::INFINITY);
        num("Infinity", f64::INFINITY);
        num("-Infinity", f64::NEG_INFINITY);
        num("  12  ", 12.0);
        num("1_000", f64::NAN);
        num("1,000", f64::NAN);
        num("NaN", f64::NAN);
        num("abc", f64::NAN);
        num("٣", f64::NAN); // Arabic-Indic digit: not an ASCII digit
        num("infinity", f64::NAN); // Rust's parser would accept this; the reference does not
        num("inf", f64::NAN);
        num("1e", f64::NAN); // incomplete exponent fails the *whole-string* test
    }

    #[test]
    fn negative_zero_is_preserved() {
        assert!(to_number("-0").is_sign_negative());
        assert!(parse_float("-0").is_sign_negative());
    }

    #[test]
    fn parse_float_takes_the_longest_prefix() {
        assert_eq!(parse_float("5abc"), 5.0);
        assert_eq!(parse_float("0abc"), 0.0);
        assert_eq!(parse_float("1_000"), 1.0);
        assert_eq!(parse_float("1,000"), 1.0);
        assert_eq!(parse_float("0x10"), 0.0); // "0" then "x10" is garbage
        assert_eq!(parse_float("  12  "), 12.0);
        assert_eq!(parse_float(".5x"), 0.5);
        assert_eq!(parse_float("1e"), 1.0); // mantissa only
        assert_eq!(parse_float("1e+"), 1.0);
        assert_eq!(parse_float("Infinityx"), f64::INFINITY);
        assert!(parse_float("abc").is_nan());
        assert!(parse_float("").is_nan());
        assert!(parse_float(".").is_nan());
        assert!(parse_float("+").is_nan());
        assert!(parse_float("e5").is_nan());
    }

    #[test]
    fn zero_and_nan_differ_in_truthiness() {
        // This distinction is what makes '0abc' and 'abc' tag differently.
        assert_eq!(parse_float("0abc"), 0.0);
        assert!(parse_float("abc").is_nan());
        assert!(parse_float("0abc") == 0.0);
    }
}
