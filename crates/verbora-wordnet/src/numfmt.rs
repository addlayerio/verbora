//! The reference number semantics that the WordNet parsers depend on.
//!
//! Every numeric field in a WordNet record goes through `parseInt`, and
//! `parseInt` is not `str::parse`:
//!
//! * it **stops at the first character it cannot use** rather than failing —
//!   `parseInt('12abc', 10)` is `12`;
//! * it returns **`NaN`**, not an error, when there is nothing to parse — and
//!   `NaN` then flows into the record as a value the caller can observe;
//! * `for (i = 0; i < NaN; i++)` runs **zero** times, because every comparison
//!   with `NaN` is false. A naive port that treats a parse failure as an error
//!   turns a record the reference happily returns into a hard failure.
//!
//! One field is parsed in a different radix from all the others:
//! `data_file` reads `wCnt` with `parseInt(tokens[3], 16)`. `'0b'` is 11, not
//! 0 and not an error. Getting that wrong silently truncates the synonyms list
//! *and* shifts the pointer offset for every synset with ten or more words.

/// The largest count this crate will materialise from a parsed field.
///
/// The reference would loop `parseInt(tok, 16)` times no matter how large that is;
/// a mid-line offset can put an eight-digit hex string in `tokens[3]`, which in
/// Node means pushing 300 million `undefined`s (in practice: an out-of-memory
/// crash). Refusing beyond this bound turns an unbounded allocation into a
/// diagnosable error. See the crate docs, divergence **W4**.
pub const MAX_COUNT: usize = 1 << 20;

/// The reference's `parseInt(s, radix)`.
///
/// `s` is `None` for the `undefined` that the reference's out-of-range array
/// indexing produces; `parseInt(undefined, 10)` is `NaN`, so that is what this
/// returns. Pass `radix = None` for a bare `parseInt(s)`, which defaults to 10
/// but still honours an `0x` prefix.
#[must_use]
pub fn parse_int(s: Option<&str>, radix: Option<u32>) -> f64 {
    let Some(s) = s else { return f64::NAN };

    // `parseInt` skips leading StrWhiteSpace, which is the reference's `\s` set.
    let s = s.trim_start_matches(verbora_core::is_whitespace);
    let bytes = s.as_bytes();
    let mut i = 0usize;

    let negative = match bytes.first() {
        Some(b'-') => {
            i += 1;
            true
        }
        Some(b'+') => {
            i += 1;
            false
        }
        _ => false,
    };

    let mut radix = radix.unwrap_or(0);
    // `0x`/`0X` selects 16 when the radix is absent, 0 or already 16.
    if (radix == 0 || radix == 16)
        && bytes.len() >= i + 2
        && bytes[i] == b'0'
        && (bytes[i + 1] | 0x20) == b'x'
    {
        i += 2;
        radix = 16;
    }
    if radix == 0 {
        radix = 10;
    }

    let start = i;
    // Accumulate in f64 throughout: the reference has no integers, and a long digit
    // run must saturate the same way (into an inexact float), not wrap or error.
    let mut value = 0.0f64;
    while i < bytes.len() {
        let Some(d) = (bytes[i] as char).to_digit(radix) else {
            break;
        };
        value = value * f64::from(radix) + f64::from(d);
        i += 1;
    }

    if i == start {
        return f64::NAN;
    }
    if negative { -value } else { value }
}

/// `parseInt(s, 10)`.
#[inline]
#[must_use]
pub fn parse_int_10(s: Option<&str>) -> f64 {
    parse_int(s, Some(10))
}

/// `parseInt(s, 16)` — used for, and only for, a synset's word count.
#[inline]
#[must_use]
pub fn parse_int_16(s: Option<&str>) -> f64 {
    parse_int(s, Some(16))
}

/// How many times `for (let i = 0; i < n; i++)` runs for a reference `n`.
///
/// `NaN`, negatives and zero all yield zero iterations. Returns `None` when the
/// count exceeds [`MAX_COUNT`], which the caller turns into an error rather than
/// attempting an allocation the reference would only survive by crashing.
#[inline]
#[must_use]
pub fn loop_count(n: f64) -> Option<usize> {
    // `NaN` must land here: every comparison with it is false in the reference, so
    // `for (i = 0; i < NaN; i++)` runs zero times. `matches!` on `partial_cmp`
    // states that explicitly rather than relying on a negated `>`.
    if !matches!(n.partial_cmp(&0.0), Some(std::cmp::Ordering::Greater)) {
        return Some(0);
    }
    if n > MAX_COUNT as f64 {
        return None;
    }
    // `i < n` for a fractional n runs ceil(n) times: i = 0..=floor(n).
    Some(n.ceil() as usize)
}

/// The reference's `array[index]` where `index` is a number that may be `NaN`,
/// negative or out of range — all of which yield `undefined`, never a panic.
#[inline]
#[must_use]
pub fn index_at<'a>(tokens: &[&'a str], index: f64) -> Option<&'a str> {
    if !index.is_finite() || index < 0.0 || index.fract() != 0.0 {
        return None;
    }
    tokens.get(index as usize).copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_int_matches_the_reference() {
        assert_eq!(parse_int_10(Some("42")), 42.0);
        assert_eq!(parse_int_10(Some("08515608")), 8_515_608.0);
        assert_eq!(parse_int_10(Some("12abc")), 12.0);
        assert_eq!(parse_int_10(Some("-7")), -7.0);
        assert_eq!(parse_int_10(Some("+7")), 7.0);
        assert_eq!(parse_int_10(Some("  9  ")), 9.0);
        assert!(parse_int_10(Some("")).is_nan());
        assert!(parse_int_10(Some("abc")).is_nan());
        assert!(parse_int_10(None).is_nan());
        // Radix 10 stops at 'x'; radix 16 (or none) consumes the prefix.
        assert_eq!(parse_int(Some("0x10"), Some(10)), 0.0);
        assert_eq!(parse_int(Some("0x10"), None), 16.0);
        assert_eq!(parse_int(Some("0x10"), Some(16)), 16.0);
    }

    #[test]
    fn word_count_is_hexadecimal() {
        // The single field in the whole format that is not base 10.
        assert_eq!(parse_int_16(Some("0b")), 11.0);
        assert_eq!(parse_int_16(Some("0d")), 13.0);
        assert_eq!(parse_int_16(Some("0a")), 10.0);
        assert_eq!(parse_int_16(Some("03")), 3.0);
        // What a naive `str::parse` would have produced instead:
        assert!("0b".parse::<u32>().is_err());
    }

    #[test]
    fn nan_bounded_loops_run_zero_times() {
        assert_eq!(loop_count(f64::NAN), Some(0));
        assert_eq!(loop_count(-1.0), Some(0));
        assert_eq!(loop_count(0.0), Some(0));
        assert_eq!(loop_count(3.0), Some(3));
        assert_eq!(loop_count(2.5), Some(3));
        assert_eq!(loop_count(f64::INFINITY), None);
        assert_eq!(loop_count(1e9), None);
    }

    #[test]
    fn indexing_out_of_range_is_undefined() {
        let toks = ["a", "b", "c"];
        assert_eq!(index_at(&toks, 0.0), Some("a"));
        assert_eq!(index_at(&toks, 2.0), Some("c"));
        assert_eq!(index_at(&toks, 3.0), None);
        assert_eq!(index_at(&toks, -1.0), None);
        assert_eq!(index_at(&toks, f64::NAN), None);
        assert_eq!(index_at(&toks, 1.5), None);
    }

    #[test]
    fn very_long_digit_runs_saturate_like_the_reference() {
        // 25 nines: beyond f64's exact integer range, and the reference's own
        // parseInt is equally inexact — the point is that neither errors.
        let long = "9".repeat(25);
        let got = parse_int_10(Some(&long));
        assert!(got.is_finite() && got > 1e24);
    }
}
