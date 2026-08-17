//! `normalizeJa` — the preprocessing `TokenizerJa` runs on every input.
//!
//! Ported from the reference `normalizer_ja`. It is reproduced here
//! rather than taken from `verbora-normalizers` so that this crate's parity does
//! not depend on another crate's in-flight port; integration can collapse the two
//! once both are settled.
//!
//! # Why this works on `[u16]`
//!
//! Every step is a `String.prototype.replace` over UTF-16 code units, and two of
//! them can split a surrogate pair: `/(..)々々/g` captures **two code units**,
//! and `/(.)々/g` captures one. Given `"😀々"` the reference duplicates the low
//! surrogate alone, producing a string Rust's `String` cannot hold. Working in
//! `[u16]` throughout keeps that representable — and keeps the code-unit indexing
//! the segmenter itself depends on.
//!
//! # Why the substitution tables are ordered
//!
//! `replacer()` joins every table key into one alternation and the reference
//! alternation is *ordered*: the first listed alternative that matches at a
//! position wins, regardless of length. The tables list two-unit keys before the
//! one-unit keys that prefix them (`ｳﾞ` before `ｳ`), so "try two units, then one"
//! reproduces the ordering exactly — a property the generator asserts rather than
//! assumes.

use std::borrow::Cow;

use super::norm_tables::{
    COMPOSITE_1, COMPOSITE_2, FIX_KANA_1, FIX_KANA_2, NORMALIZE_1, NORMALIZE_2,
};
use crate::whitespace::is_line_terminator_unit;

/// U+3005 IDEOGRAPHIC ITERATION MARK, the "repeat the previous character" sign.
const REPEAT_MARK: u16 = 0x3005;

/// Applies an ordered substitution table to `src`.
fn substitute<'a>(
    src: &'a [u16],
    two: &[([u16; 2], &'static [u16])],
    one: &[(u16, &'static [u16])],
) -> Cow<'a, [u16]> {
    let hit = |i: usize| -> Option<(usize, &'static [u16])> {
        if let Some(pair) = src.get(i..i + 2) {
            let key = [pair[0], pair[1]];
            if let Ok(k) = two.binary_search_by(|probe| probe.0.cmp(&key)) {
                return Some((2, two[k].1));
            }
        }
        let u = *src.get(i)?;
        one.binary_search_by(|probe| probe.0.cmp(&u))
            .ok()
            .map(|k| (1, one[k].1))
    };

    // Nothing to do is the common case for Latin input; leave it borrowed.
    let Some(first) = (0..src.len()).find(|&i| hit(i).is_some()) else {
        return Cow::Borrowed(src);
    };

    let mut out = Vec::with_capacity(src.len());
    out.extend_from_slice(&src[..first]);
    let mut i = first;
    while i < src.len() {
        match hit(i) {
            Some((len, rep)) => {
                out.extend_from_slice(rep);
                i += len;
            }
            None => {
                out.push(src[i]);
                i += 1;
            }
        }
    }
    Cow::Owned(out)
}

/// `str.replace(/(..)々々/g, '$1$1')` — a two-unit sequence repeated by a double
/// iteration mark.
fn expand_double_repeat(src: &[u16]) -> Cow<'_, [u16]> {
    expand_repeat(src, 2)
}

/// `str.replace(/(.)々/g, '$1$1')` — a one-unit sequence repeated by a single
/// iteration mark.
fn expand_single_repeat(src: &[u16]) -> Cow<'_, [u16]> {
    expand_repeat(src, 1)
}

/// Shared body of the two repeat-mark rules.
///
/// `width` code units are captured, followed by `width` iteration marks; the
/// match is replaced by the capture, twice. The captured units must not be line
/// terminators, which is what `.` excludes in the reference — Rust's `.` excludes
/// only `\n`, so the check is explicit.
fn expand_repeat(src: &[u16], width: usize) -> Cow<'_, [u16]> {
    let span = width * 2;
    let matches_at = |i: usize| -> bool {
        let Some(window) = src.get(i..i + span) else {
            return false;
        };
        window[..width].iter().all(|&u| !is_line_terminator_unit(u))
            && window[width..].iter().all(|&u| u == REPEAT_MARK)
    };

    let Some(first) = (0..src.len()).find(|&i| matches_at(i)) else {
        return Cow::Borrowed(src);
    };

    let mut out = Vec::with_capacity(src.len() + span);
    out.extend_from_slice(&src[..first]);
    let mut i = first;
    while i < src.len() {
        if matches_at(i) {
            out.extend_from_slice(&src[i..i + width]);
            out.extend_from_slice(&src[i..i + width]);
            i += span;
        } else {
            out.push(src[i]);
            i += 1;
        }
    }
    Cow::Owned(out)
}

/// Normalises Japanese text, returning UTF-16 code units.
///
/// Repeat marks are expanded, fullwidth alphanumerics and symbols become
/// halfwidth, halfwidth punctuation and katakana become fullwidth, combining
/// dakuten are folded into their precomposed forms, and composite symbols such as
/// `㍿` expand to `株式会社`.
pub(crate) fn normalize_ja(text: &str) -> Vec<u16> {
    let units: Vec<u16> = text.encode_utf16().collect();
    let a = expand_double_repeat(&units);
    let b = expand_single_repeat(&a);
    let c = substitute(&b, &NORMALIZE_2, &NORMALIZE_1);
    let d = substitute(&c, &FIX_KANA_2, &FIX_KANA_1);
    substitute(&d, &COMPOSITE_2, &COMPOSITE_1).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn norm(s: &str) -> String {
        String::from_utf16(&normalize_ja(s)).expect("well-formed for BMP input")
    }

    #[test]
    fn widths_are_normalised() {
        assert_eq!(norm("ﾊﾝｶｸ"), "ハンカク");
        assert_eq!(norm("１２３"), "123");
        assert_eq!(norm("ＡＢＣ"), "ABC");
        // Fullwidth space becomes halfwidth; halfwidth punctuation goes the other way.
        assert_eq!(norm("あ　い"), "あ い");
        assert_eq!(norm("a｡b"), "a。b");
    }

    #[test]
    fn dakuten_are_recombined() {
        assert_eq!(norm("ｶﾞ"), "ガ");
        assert_eq!(norm("ｳﾞ"), "ヴ");
        // The two-unit key must win over the one-unit prefix.
        assert_eq!(norm("ｳ"), "ウ");
    }

    #[test]
    fn repeat_marks_expand() {
        assert_eq!(norm("人々"), "人人");
        assert_eq!(norm("時々"), "時時");
        assert_eq!(norm("平成々々"), "平成平成");
    }

    #[test]
    fn composite_symbols_expand() {
        assert_eq!(norm("㍿"), "株式会社");
        assert_eq!(norm("㋀"), "1月");
    }

    #[test]
    fn plain_text_is_untouched_and_unallocated() {
        let units: Vec<u16> = "hello".encode_utf16().collect();
        assert!(matches!(
            substitute(&units, &NORMALIZE_2, &NORMALIZE_1),
            Cow::Borrowed(_)
        ));
        assert_eq!(norm(""), "");
        assert_eq!(norm("日本語"), "日本語");
    }

    #[test]
    fn astral_input_survives_as_code_units() {
        // No panic and no lost data: the emoji is two units that match nothing.
        assert_eq!(normalize_ja("😀"), "😀".encode_utf16().collect::<Vec<_>>());
    }
}
