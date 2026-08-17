//! Text-unit dispatch and the reference string primitives.
//!
//! # Why this exists
//!
//! Every phonetic algorithm in the reference indexes text the way the reference does —
//! by UTF-16 code unit. `token[pos]`, `token.substring(a, b)`, `token.length` and
//! every regular-expression character class count code units, so a character
//! outside the Basic Multilingual Plane counts as **two**, and a rule can match
//! one half of a surrogate pair. That is observable:
//!
//! ```text
//! Metaphone.dedup("😀😀")
//!   The reference : "😀😀"   the four code units are D83D DE00 D83D DE00 — no
//!                         two ADJACENT ones are equal, so nothing collapses
//!   Rust chars : "😀"     two identical `char`s collapse to one
//! ```
//!
//! # How it is paid for
//!
//! Promoting everything to `Vec<u16>` would allocate on every call for the sake
//! of input that phonetic algorithms are not even defined for. Instead the
//! algorithms are written once, generically over [`Unit`], and each entry point
//! picks the narrowest representation that is *provably identical* to UTF-16 for
//! the given input:
//!
//! | Input          | Representation | Allocates |
//! |----------------|----------------|-----------|
//! | ASCII          | `&[u8]`        | no        |
//! | anything else  | `Vec<u16>`     | yes       |
//!
//! For ASCII, one byte *is* one UTF-16 code unit, so the fast path is the same
//! computation on a narrower type rather than an approximation. [`str::is_ascii`]
//! is vectorised, so the test itself is close to free.

use std::borrow::Cow;

/// A text unit an algorithm can operate on: a byte, or a UTF-16 code unit.
///
/// The bound is deliberately minimal so the generic scanners monomorphise down
/// to tight loops over primitives.
pub trait Unit: Copy + Eq {
    /// Widens an ASCII byte into this representation.
    fn from_ascii(b: u8) -> Self;

    /// The ASCII byte this unit represents, or `None` if it is not 7-bit.
    fn to_ascii(self) -> Option<u8>;

    /// The UTF-16 code unit this unit represents.
    fn to_utf16(self) -> u16;

    /// Whether this unit is the ASCII byte `b`.
    #[inline]
    fn eq_ascii(self, b: u8) -> bool {
        self == Self::from_ascii(b)
    }
}

impl Unit for u8 {
    #[inline]
    fn from_ascii(b: u8) -> Self {
        b
    }

    #[inline]
    fn to_ascii(self) -> Option<u8> {
        self.is_ascii().then_some(self)
    }

    #[inline]
    fn to_utf16(self) -> u16 {
        u16::from(self)
    }
}

impl Unit for u16 {
    #[inline]
    fn from_ascii(b: u8) -> Self {
        u16::from(b)
    }

    #[inline]
    fn to_ascii(self) -> Option<u8> {
        (self < 0x80).then_some(self as u8)
    }

    #[inline]
    fn to_utf16(self) -> u16 {
        self
    }
}

/// Whether `units` equals the ASCII pattern `pat`, unit for unit.
///
/// Used for the `terms.indexOf(token.substring(a, b)) > -1` idiom, which is a
/// membership test over short ASCII literals — no substring need be allocated.
#[inline]
pub fn eq_ascii_slice<U: Unit>(units: &[U], pat: &[u8]) -> bool {
    units.len() == pat.len() && units.iter().zip(pat).all(|(&u, &b)| u.eq_ascii(b))
}

/// Whether `units` equals any of the ASCII patterns in `terms`.
#[inline]
pub fn any_ascii_slice<U: Unit>(units: &[U], terms: &[&[u8]]) -> bool {
    terms.iter().any(|t| eq_ascii_slice(units, t))
}

/// Appends the ASCII bytes of `pat` to `out`.
#[inline]
pub fn push_ascii<U: Unit>(out: &mut Vec<U>, pat: &[u8]) {
    out.extend(pat.iter().map(|&b| U::from_ascii(b)));
}

/// Re-encodes a unit slice as UTF-16 code units.
#[inline]
pub fn to_utf16<U: Unit>(units: &[U]) -> Vec<u16> {
    units.iter().map(|&u| u.to_utf16()).collect()
}

/// The number of UTF-16 code units in `s` — the reference's `String#length`.
///
/// Astral-plane characters count as two, which is what `padRight0` and every
/// `token.length` comparison in the reference actually measure.
#[inline]
pub fn utf16_len(s: &str) -> usize {
    if s.is_ascii() {
        return s.len();
    }
    s.chars().map(char::len_utf16).sum()
}

/// The reference's `String#toLowerCase`.
///
/// Borrows when there is nothing to fold, which is the common case for
/// already-lowercased corpora. Non-ASCII text goes through Rust's full Unicode
/// mapping, which implements the same Default Case Conversion algorithm (final
/// sigma included) that the reference language specifies.
pub fn utf16_to_lowercase(s: &str) -> Cow<'_, str> {
    if s.is_ascii() {
        if s.bytes().any(|b| b.is_ascii_uppercase()) {
            Cow::Owned(s.to_ascii_lowercase())
        } else {
            Cow::Borrowed(s)
        }
    } else {
        Cow::Owned(s.to_lowercase())
    }
}

/// The reference's `String#toUpperCase`.
///
/// Case mapping can *change length* — `ß` becomes `SS`, `ﬁ` becomes `FI` — and
/// Metaphone truncates before uppercasing, so callers must not assume the result
/// fits any length budget computed beforehand.
pub fn utf16_to_uppercase(s: &str) -> Cow<'_, str> {
    if s.is_ascii() {
        if s.bytes().any(|b| b.is_ascii_lowercase()) {
            Cow::Owned(s.to_ascii_uppercase())
        } else {
            Cow::Borrowed(s)
        }
    } else {
        Cow::Owned(s.to_uppercase())
    }
}

/// Uppercases a UTF-16 sequence that may contain **unpaired surrogates**.
///
/// Metaphone truncates its output before uppercasing it, and the truncation can
/// fall between the two halves of a surrogate pair. The reference uppercases such a
/// string by leaving the orphaned half alone; `String::from_utf16_lossy` would
/// destroy it, so the conversion is done per decoded scalar instead.
pub fn uppercase_utf16(units: &[u16]) -> Vec<u16> {
    let mut out = Vec::with_capacity(units.len());
    for item in char::decode_utf16(units.iter().copied()) {
        match item {
            Ok(c) => {
                for up in c.to_uppercase() {
                    let mut buf = [0u16; 2];
                    out.extend_from_slice(up.encode_utf16(&mut buf));
                }
            }
            // An unpaired surrogate has no case mapping; the reference keeps it.
            Err(e) => out.push(e.unpaired_surrogate()),
        }
    }
    out
}

/// Whether the code unit `u` is matched by the reference's `String#trim`.
///
/// `trim` removes `WhiteSpace ∪ LineTerminator ∪ U+FEFF`. Rust's [`str::trim`]
/// uses the Unicode `White_Space` property, which **excludes** `U+FEFF` and
/// **includes** `U+0085`; using it inside `Metaphone::c_transform` would leave a
/// byte-order mark in place that the reference strips. Every the reference
/// whitespace character is in the BMP, so a per-code-unit test is exact.
#[inline]
pub fn is_trim_unit<U: Unit>(u: U) -> bool {
    // char::from_u32 of a lone surrogate is None, which correctly reports "not
    // whitespace" without needing the surrogate to be a valid scalar.
    char::from_u32(u32::from(u.to_utf16())).is_some_and(verbora_core::is_whitespace)
}

/// The reference's `String#trim`, over code units.
#[inline]
pub fn trim_units<U: Unit>(units: &[U]) -> &[U] {
    let start = units
        .iter()
        .position(|&u| !is_trim_unit(u))
        .unwrap_or(units.len());
    let end = units
        .iter()
        .rposition(|&u| !is_trim_unit(u))
        .map_or(start, |i| i + 1);
    &units[start..end]
}

/// The effective length for `String#substr(0, n)` / `String#substring(0, n)`
/// applied to a string of `len` code units.
///
/// The reference coerces `n` with `ToIntegerOrInfinity`, which truncates toward
/// zero, then clamps to `0..=len`. `substr(0, 0.5)` is therefore `''`, and
/// `substring(0, -1)` is `''` — both reachable from the reference's public
/// `maxLength` argument.
#[inline]
pub fn clamp_take(n: f64, len: usize) -> usize {
    if n.is_nan() || n <= 0.0 {
        return 0;
    }
    let truncated = n.trunc();
    if truncated >= len as f64 {
        len
    } else {
        truncated as usize
    }
}

/// The effective end index for `String#slice(0, n)` on a string of `len` code
/// units.
///
/// `slice` is **not** `substring`: a negative `n` counts back from the end
/// rather than clamping to zero. `SoundExDM` normalises its output with `slice`,
/// so `codeLength = -1` drops the last digit instead of returning nothing.
#[inline]
pub fn clamp_slice_end(n: f64, len: usize) -> usize {
    let end = if n.is_nan() { 0.0 } else { n.trunc() };
    if end < 0.0 {
        (len as f64 + end).max(0.0) as usize
    } else if end >= len as f64 {
        len
    } else {
        end as usize
    }
}

/// The reference's `x || default` for a numeric argument.
///
/// `0`, `NaN`, `null`, `undefined` and `false` are all falsy and select the
/// default; every other number — negatives included — is passed through, which
/// is how `maxLength = -1` reaches `substring` and yields an empty code.
#[inline]
pub fn coerce_or_default(value: Option<f64>, default: f64) -> f64 {
    match value {
        Some(v) if v != 0.0 && !v.is_nan() => v,
        _ => default,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf16_len_counts_utf16_code_units() {
        for s in ["", "abc", "café", "Москва", "😀", "a😀b", "𝕳𝖊𝖑𝖑𝖔"] {
            assert_eq!(utf16_len(s), s.encode_utf16().count(), "for {s:?}");
        }
    }

    #[test]
    fn ascii_case_folding_borrows_when_it_can() {
        assert!(matches!(utf16_to_lowercase("already"), Cow::Borrowed(_)));
        assert!(matches!(utf16_to_uppercase("ALREADY"), Cow::Borrowed(_)));
        assert_eq!(utf16_to_lowercase("MiXeD"), "mixed");
        assert_eq!(utf16_to_uppercase("MiXeD"), "MIXED");
    }

    #[test]
    fn case_mapping_can_change_length() {
        // The reason Metaphone's output can exceed its own maxLength.
        assert_eq!(utf16_to_uppercase("ß"), "SS");
        assert_eq!(utf16_to_uppercase("ﬁ"), "FI");
    }

    #[test]
    fn uppercase_utf16_matches_str_uppercase_for_well_formed_input() {
        for s in ["", "abc", "straße", "ﬁt", "Ελλάδα", "😀", "ας"] {
            let got = uppercase_utf16(&s.encode_utf16().collect::<Vec<_>>());
            let want: Vec<u16> = s.to_uppercase().encode_utf16().collect();
            assert_eq!(got, want, "for {s:?}");
        }
    }

    #[test]
    fn uppercase_utf16_preserves_unpaired_surrogates() {
        // The high half of 😀, orphaned by truncation.
        let got = uppercase_utf16(&[0xD83D]);
        assert_eq!(got, vec![0xD83D]);
        let got = uppercase_utf16(&[0x61, 0xDE00]);
        assert_eq!(got, vec![0x41, 0xDE00]);
    }

    #[test]
    fn trim_follows_the_reference_not_rust() {
        let bom: Vec<u16> = "\u{FEFF}abc\u{FEFF}".encode_utf16().collect();
        assert_eq!(
            trim_units(&bom),
            &"abc".encode_utf16().collect::<Vec<_>>()[..]
        );

        // U+0085 is whitespace to Rust but not to the reference, so it stays.
        let nel: Vec<u16> = "\u{0085}a".encode_utf16().collect();
        assert_eq!(trim_units(&nel), &nel[..]);

        let all: Vec<u16> = "   ".encode_utf16().collect();
        assert!(trim_units(&all).is_empty());
    }

    #[test]
    fn take_reproduces_tointegerorinfinity() {
        assert_eq!(clamp_take(3.0, 10), 3);
        assert_eq!(clamp_take(0.5, 10), 0);
        assert_eq!(clamp_take(-1.0, 10), 0);
        assert_eq!(clamp_take(f64::NAN, 10), 0);
        assert_eq!(clamp_take(99.0, 4), 4);
        assert_eq!(clamp_take(1.9, 10), 1);
    }

    #[test]
    fn slice_counts_negatives_from_the_end() {
        assert_eq!(clamp_slice_end(6.0, 3), 3);
        assert_eq!(clamp_slice_end(-1.0, 6), 5);
        assert_eq!(clamp_slice_end(-9.0, 6), 0);
        assert_eq!(clamp_slice_end(6.5, 7), 6);
        assert_eq!(clamp_slice_end(f64::NAN, 7), 0);
    }

    #[test]
    fn falsy_arguments_select_the_default() {
        assert_eq!(coerce_or_default(None, 32.0), 32.0);
        assert_eq!(coerce_or_default(Some(0.0), 32.0), 32.0);
        assert_eq!(coerce_or_default(Some(f64::NAN), 32.0), 32.0);
        assert_eq!(coerce_or_default(Some(-1.0), 32.0), -1.0);
        assert_eq!(coerce_or_default(Some(4.0), 32.0), 4.0);
    }

    #[test]
    fn ascii_slice_comparison() {
        let units: Vec<u8> = b"ABC".to_vec();
        assert!(eq_ascii_slice(&units, b"ABC"));
        assert!(!eq_ascii_slice(&units, b"AB"));
        assert!(any_ascii_slice(&units, &[b"XY", b"ABC"]));
    }
}
