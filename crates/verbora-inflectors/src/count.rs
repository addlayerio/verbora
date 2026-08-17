//! Ordinal suffixes: `CountInflector` and `CountInflectorFr`.
//!
//! Both are stateless in the reference (`class CountInflector` with no
//! constructor and no fields), so both are associated functions here.
//!
//! # They do not agree on coercion
//!
//! ```text
//! // count_inflector — arithmetic, so anything is coerced
//! const teenth = (i % 100)
//! if (teenth > 10 && teenth < 14) return 'th'
//! switch (i % 10) { case 1: return 'st'; case 2: return 'nd'; case 3: return 'rd'; default: return 'th' }
//!
//! // fr/count_inflector — strict equality, so nothing is coerced
//! if (i === 1 || i === 'I') return 'er'
//! return 'e'
//! ```
//!
//! `CountInflector.nth("1")` is `"1st"` — the string is coerced by `%` — while
//! `CountInflectorFr.nth("1")` is `"1e"`, because `"1" === 1` is false. And
//! `CountInflectorFr.nth("I")` is `"Ier"` while `nth("i")` is `"ie"`. One
//! generic Rust signature cannot express both, so each argument kind gets its
//! own entry point.
//!
//! # `%` is a remainder, not a modulo
//!
//! The reference's `%` keeps the sign of the dividend, so `-21 % 10` is `-1` and
//! matches no `case`. Every negative ordinal is therefore `th`: `nth(-1)` is
//! `"-1th"`, `nth(-21)` is `"-21th"`. Rust's `%` agrees; `rem_euclid` would not.

use std::fmt::Write;

use crate::numfmt::{number_to_string, to_number};

/// English ordinal suffixes — the reference `count_inflector`.
///
/// ```
/// use verbora_inflectors::CountInflector;
///
/// assert_eq!(CountInflector::nth(1), "1st");
/// assert_eq!(CountInflector::nth(22), "22nd");
/// // The teens are the exception the `% 100` guard exists for.
/// assert_eq!(CountInflector::nth(112), "112th");
/// // the reference's remainder is signed, so negatives never take st/nd/rd.
/// assert_eq!(CountInflector::nth(-1), "-1th");
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CountInflector;

impl CountInflector {
    /// The ordinal suffix for an integer.
    #[must_use]
    pub fn nth_form(i: i64) -> &'static str {
        let teenth = i % 100;
        if teenth > 10 && teenth < 14 {
            return "th";
        }
        match i % 10 {
            1 => "st",
            2 => "nd",
            3 => "rd",
            _ => "th",
        }
    }

    /// The ordinal for an integer: its decimal form plus the suffix.
    ///
    /// Exact for every `i64`. The reference stores numbers as `f64` and so cannot
    /// represent integers beyond 2^53 − 1 faithfully; use [`Self::nth_f64`] to
    /// reproduce that rounding deliberately.
    #[must_use]
    pub fn nth(i: i64) -> String {
        let suffix = Self::nth_form(i);
        // 20 digits plus a sign is the widest `i64`, and the suffix is two
        // bytes: one allocation, never resized. Formatting *into* the buffer
        // rather than through `i.to_string()` avoids a second one.
        let mut out = String::with_capacity(24);
        let _ = write!(out, "{i}");
        out.push_str(suffix);
        out
    }

    /// The ordinal suffix for a reference number, including non-integers and
    /// the specials.
    ///
    /// `1.5 % 10` is `1.5`, which is not `=== 1`, so fractions take `th`; `NaN`
    /// and the infinities produce `NaN` remainders and take `th` as well.
    #[must_use]
    pub fn nth_form_f64(x: f64) -> &'static str {
        let teenth = x % 100.0;
        if teenth > 10.0 && teenth < 14.0 {
            return "th";
        }
        let ones = x % 10.0;
        if ones == 1.0 {
            "st"
        } else if ones == 2.0 {
            "nd"
        } else if ones == 3.0 {
            "rd"
        } else {
            "th"
        }
    }

    /// The ordinal for a reference number, formatted with
    /// `Number.prototype.toString`.
    ///
    /// ```
    /// use verbora_inflectors::CountInflector;
    ///
    /// assert_eq!(CountInflector::nth_f64(1.5), "1.5th");
    /// assert_eq!(CountInflector::nth_f64(1e21), "1e+21th");
    /// assert_eq!(CountInflector::nth_f64(f64::NAN), "NaNth");
    /// assert_eq!(CountInflector::nth_f64(f64::INFINITY), "Infinityth");
    /// ```
    #[must_use]
    pub fn nth_f64(x: f64) -> String {
        let mut out = number_to_string(x);
        out.push_str(Self::nth_form_f64(x));
        out
    }

    /// The ordinal suffix for a string argument, which `%` coerces to a number.
    #[must_use]
    pub fn nth_form_str(s: &str) -> &'static str {
        Self::nth_form_f64(to_number(s))
    }

    /// The ordinal for a string argument.
    ///
    /// `i.toString()` on a string is the string itself, so the input is
    /// reproduced verbatim and only the suffix is derived from its numeric
    /// value.
    ///
    /// ```
    /// use verbora_inflectors::CountInflector;
    ///
    /// assert_eq!(CountInflector::nth_str("1"), "1st");
    /// assert_eq!(CountInflector::nth_str("11"), "11th");
    /// assert_eq!(CountInflector::nth_str("abc"), "abcth");
    /// ```
    #[must_use]
    pub fn nth_str(s: &str) -> String {
        let mut out = String::with_capacity(s.len() + 2);
        out.push_str(s);
        out.push_str(Self::nth_form_str(s));
        out
    }
}

/// French ordinal suffixes — the reference `count_inflector`.
///
/// Only `1er`; everything else is `e`. The reference also accepts the Roman
/// numeral `'I'`, by exact string comparison.
///
/// ```
/// use verbora_inflectors::CountInflectorFr;
///
/// assert_eq!(CountInflectorFr::nth(1), "1er");
/// assert_eq!(CountInflectorFr::nth(2), "2e");
/// assert_eq!(CountInflectorFr::nth_str("I"), "Ier");
/// // Strict equality: the string "1" is not the number 1, and "i" is not "I".
/// assert_eq!(CountInflectorFr::nth_str("1"), "1e");
/// assert_eq!(CountInflectorFr::nth_str("i"), "ie");
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CountInflectorFr;

impl CountInflectorFr {
    /// The ordinal suffix for an integer.
    #[must_use]
    pub fn nth_form(i: i64) -> &'static str {
        if i == 1 { "er" } else { "e" }
    }

    /// The ordinal for an integer.
    #[must_use]
    pub fn nth(i: i64) -> String {
        let mut out = String::with_capacity(24);
        let _ = write!(out, "{i}");
        out.push_str(Self::nth_form(i));
        out
    }

    /// The ordinal suffix for a reference number.
    ///
    /// `i === 1` is true for the `f64` value `1.0` and false for everything
    /// else, including `NaN` and `-0`.
    #[must_use]
    pub fn nth_form_f64(x: f64) -> &'static str {
        if x == 1.0 { "er" } else { "e" }
    }

    /// The ordinal for a reference number.
    #[must_use]
    pub fn nth_f64(x: f64) -> String {
        let mut out = number_to_string(x);
        out.push_str(Self::nth_form_f64(x));
        out
    }

    /// The ordinal suffix for a string argument.
    ///
    /// No coercion happens: `i === 1` is false for every string, so only the
    /// exact string `"I"` earns `er`.
    #[must_use]
    pub fn nth_form_str(s: &str) -> &'static str {
        if s == "I" { "er" } else { "e" }
    }

    /// The ordinal for a string argument.
    #[must_use]
    pub fn nth_str(s: &str) -> String {
        let mut out = String::with_capacity(s.len() + 2);
        out.push_str(s);
        out.push_str(Self::nth_form_str(s));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn english_suffixes_over_the_first_hundred() {
        assert_eq!(CountInflector::nth(0), "0th");
        assert_eq!(CountInflector::nth(1), "1st");
        assert_eq!(CountInflector::nth(2), "2nd");
        assert_eq!(CountInflector::nth(3), "3rd");
        assert_eq!(CountInflector::nth(4), "4th");
        // 11, 12 and 13 are the whole reason for the `% 100` guard.
        assert_eq!(CountInflector::nth(11), "11th");
        assert_eq!(CountInflector::nth(12), "12th");
        assert_eq!(CountInflector::nth(13), "13th");
        assert_eq!(CountInflector::nth(14), "14th");
        assert_eq!(CountInflector::nth(21), "21st");
        assert_eq!(CountInflector::nth(102), "102nd");
        assert_eq!(CountInflector::nth(111), "111th");
        assert_eq!(CountInflector::nth(1113), "1113th");
    }

    #[test]
    fn negative_ordinals_never_take_st_nd_rd() {
        // The remainder keeps the dividend's sign, so nothing matches.
        assert_eq!(CountInflector::nth(-1), "-1th");
        assert_eq!(CountInflector::nth(-2), "-2th");
        assert_eq!(CountInflector::nth(-11), "-11th");
        assert_eq!(CountInflector::nth(-21), "-21th");
    }

    #[test]
    fn floats_and_specials() {
        assert_eq!(CountInflector::nth_f64(1.5), "1.5th");
        assert_eq!(CountInflector::nth_f64(0.1), "0.1th");
        assert_eq!(CountInflector::nth_f64(1.0), "1st");
        assert_eq!(CountInflector::nth_f64(-0.0), "0th");
        assert_eq!(CountInflector::nth_f64(f64::NAN), "NaNth");
        assert_eq!(CountInflector::nth_f64(f64::NEG_INFINITY), "-Infinityth");
    }

    #[test]
    fn strings_are_coerced_for_the_suffix_but_reproduced_verbatim() {
        assert_eq!(CountInflector::nth_str("1"), "1st");
        assert_eq!(CountInflector::nth_str("2"), "2nd");
        assert_eq!(CountInflector::nth_str("11"), "11th");
        assert_eq!(CountInflector::nth_str(""), "th");
        assert_eq!(CountInflector::nth_str("abc"), "abcth");
        assert_eq!(CountInflector::nth_str("0x1f"), "0x1fst");
    }

    #[test]
    fn french_uses_strict_equality() {
        assert_eq!(CountInflectorFr::nth(1), "1er");
        assert_eq!(CountInflectorFr::nth(0), "0e");
        assert_eq!(CountInflectorFr::nth(11), "11e");
        assert_eq!(CountInflectorFr::nth(-1), "-1e");
        assert_eq!(CountInflectorFr::nth_f64(1.0), "1er");
        assert_eq!(CountInflectorFr::nth_f64(1.5), "1.5e");
        assert_eq!(CountInflectorFr::nth_f64(f64::NAN), "NaNe");
        assert_eq!(CountInflectorFr::nth_str("I"), "Ier");
        assert_eq!(CountInflectorFr::nth_str("i"), "ie");
        assert_eq!(CountInflectorFr::nth_str("II"), "IIe");
        assert_eq!(CountInflectorFr::nth_str("XX"), "XXe");
        assert_eq!(CountInflectorFr::nth_str("1"), "1e");
    }
}
