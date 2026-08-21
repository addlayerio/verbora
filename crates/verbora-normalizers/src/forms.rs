//! The four Unicode normalization forms of [UAX #15].
//!
//! [UAX #15]: https://www.unicode.org/reports/tr15/

use std::borrow::Cow;

use unicode_normalization::{UnicodeNormalization, is_nfc, is_nfd, is_nfkc, is_nfkd};

/// Canonical Decomposition — **NFD**, [UAX #15] §1.2.
///
/// Replaces every scalar by its full canonical decomposition and puts the
/// result in canonical order (`Canonical_Combining_Class`, ascending, stable
/// within a class). Nothing is composed afterwards, so `"é"` U+00E9 becomes
/// `"e"` + U+0301.
///
/// Returns [`Cow::Borrowed`] if and only if `s` is already in NFD, so
/// `matches!(r, Cow::Borrowed(_))` is a sound test for "this input needed no
/// work" and not merely a fast-path artefact. See the crate documentation for
/// the guarantee in full.
///
/// # Examples
///
/// ```
/// use std::borrow::Cow;
/// use verbora_normalizers::nfd;
///
/// assert_eq!(nfd("é"), "e\u{0301}");
/// // Already decomposed: borrowed, byte for byte.
/// assert!(matches!(nfd("e\u{0301}"), Cow::Borrowed(_)));
/// // Hangul decomposes algorithmically into conjoining jamo.
/// assert_eq!(nfd("한"), "\u{1112}\u{1161}\u{11AB}");
/// ```
///
/// [UAX #15]: https://www.unicode.org/reports/tr15/#Norm_Forms
#[must_use]
pub fn nfd(s: &str) -> Cow<'_, str> {
    if is_nfd(s) {
        Cow::Borrowed(s)
    } else {
        Cow::Owned(s.nfd().collect())
    }
}

/// Canonical Decomposition followed by Canonical Composition — **NFC**,
/// [UAX #15] §1.2.
///
/// The form to prefer when two strings that a reader would call "the same"
/// must compare equal: NFC is the identity on text that is already composed,
/// and it recombines base-plus-mark sequences into the precomposed scalars
/// that exist for them.
///
/// Returns [`Cow::Borrowed`] if and only if `s` is already in NFC.
///
/// # Examples
///
/// ```
/// use std::borrow::Cow;
/// use verbora_normalizers::nfc;
///
/// assert_eq!(nfc("e\u{0301}"), "é");
/// assert!(matches!(nfc("é"), Cow::Borrowed(_)));
/// // Composition is not the inverse of *compatibility* decomposition:
/// // the fullwidth letter has no canonical decomposition, so it survives.
/// assert_eq!(nfc("Ａ"), "Ａ");
/// ```
///
/// [UAX #15]: https://www.unicode.org/reports/tr15/#Norm_Forms
#[must_use]
pub fn nfc(s: &str) -> Cow<'_, str> {
    if is_nfc(s) {
        Cow::Borrowed(s)
    } else {
        Cow::Owned(s.nfc().collect())
    }
}

/// Compatibility Decomposition — **NFKD**, [UAX #15] §1.2.
///
/// Like [`nfd`], but also applies compatibility mappings: fullwidth and
/// halfwidth forms, circled and superscript letters, ligatures, and the CJK
/// squared-unit symbols in U+3300..=U+33FF all collapse onto their nominal
/// spellings.
///
/// This is **lossy in the formatting sense**: `"Ⅻ"` U+216B ROMAN NUMERAL
/// TWELVE becomes `"XII"` and cannot be recovered. Use it to build a search or
/// index key, not to store text you intend to display back to the author.
///
/// Returns [`Cow::Borrowed`] if and only if `s` is already in NFKD.
///
/// # Examples
///
/// ```
/// use verbora_normalizers::nfkd;
///
/// assert_eq!(nfkd("Ａ"), "A");
/// assert_eq!(nfkd("ﬁ"), "fi");
/// assert_eq!(nfkd("é"), "e\u{0301}");
/// ```
///
/// [UAX #15]: https://www.unicode.org/reports/tr15/#Norm_Forms
#[must_use]
pub fn nfkd(s: &str) -> Cow<'_, str> {
    if is_nfkd(s) {
        Cow::Borrowed(s)
    } else {
        Cow::Owned(s.nfkd().collect())
    }
}

/// Compatibility Decomposition followed by Canonical Composition — **NFKC**,
/// [UAX #15] §1.2.
///
/// The width- and shape-insensitive form, and the right choice for Japanese
/// text that mixes halfwidth katakana with fullwidth Latin: the halfwidth
/// voiced sound mark U+FF9E decomposes to the combining U+3099, which
/// canonical composition then folds into the preceding kana, so `"ｶ"` + `"ﾞ"`
/// becomes `"ガ"` in one step.
///
/// Compatibility decomposition is lossy in the same sense as [`nfkd`]; the
/// composition step does not undo it.
///
/// Returns [`Cow::Borrowed`] if and only if `s` is already in NFKC.
///
/// # Examples
///
/// ```
/// use verbora_normalizers::nfkc;
///
/// assert_eq!(nfkc("ＡＢＣ１２３"), "ABC123");
/// assert_eq!(nfkc("ｶﾞ"), "ガ");
/// assert_eq!(nfkc("㌔"), "キロ");
/// // Astral scalars survive intact — there is no UTF-16 buffer to split them in.
/// assert_eq!(nfkc("😀々"), "😀々");
/// ```
///
/// [UAX #15]: https://www.unicode.org/reports/tr15/#Norm_Forms
#[must_use]
pub fn nfkc(s: &str) -> Cow<'_, str> {
    if is_nfkc(s) {
        Cow::Borrowed(s)
    } else {
        Cow::Owned(s.nfkc().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The `Cow` contract, as an iff, for all four forms at once.
    fn assert_cow_iff(s: &str) {
        for (name, got) in [
            ("nfd", nfd(s)),
            ("nfc", nfc(s)),
            ("nfkd", nfkd(s)),
            ("nfkc", nfkc(s)),
        ] {
            assert_eq!(
                matches!(got, Cow::Borrowed(_)),
                got.as_ref() == s,
                "{name}({s:?}) borrowed-ness disagrees with equality"
            );
        }
    }

    #[test]
    fn empty_and_ascii_are_borrowed() {
        for s in ["", "a", "the quick brown fox", "3.14 1,000 don't"] {
            assert!(matches!(nfd(s), Cow::Borrowed(_)));
            assert!(matches!(nfc(s), Cow::Borrowed(_)));
            assert!(matches!(nfkd(s), Cow::Borrowed(_)));
            assert!(matches!(nfkc(s), Cow::Borrowed(_)));
            assert_cow_iff(s);
        }
    }

    /// Values from `UnicodeData.txt`: U+00E9 has canonical decomposition
    /// `0065 0301`, so NFD/NFKD expand it and NFC/NFKC leave it alone.
    #[test]
    fn precomposed_latin() {
        assert_eq!(nfd("é"), "e\u{0301}");
        assert_eq!(nfkd("é"), "e\u{0301}");
        assert_eq!(nfc("é"), "é");
        assert_eq!(nfkc("é"), "é");
        assert_eq!(nfc("e\u{0301}"), "é");
        assert_eq!(nfkc("e\u{0301}"), "é");
    }

    /// U+FF21 has decomposition `<wide> 0041` — compatibility, not canonical —
    /// so only the K forms touch it.
    #[test]
    fn compatibility_is_not_canonical() {
        assert_eq!(nfd("Ａ"), "Ａ");
        assert_eq!(nfc("Ａ"), "Ａ");
        assert_eq!(nfkd("Ａ"), "A");
        assert_eq!(nfkc("Ａ"), "A");
    }

    /// Hangul syllables decompose algorithmically (The Unicode Standard §3.12)
    /// rather than by table, which is the case a table-driven implementation
    /// with a missing entry gets wrong.
    #[test]
    fn hangul_decomposes_and_recomposes() {
        assert_eq!(
            nfd("한국"),
            "\u{1112}\u{1161}\u{11AB}\u{1100}\u{116E}\u{11A8}"
        );
        assert_eq!(nfc("\u{1112}\u{1161}\u{11AB}"), "한");
        assert_eq!(nfc("한국"), "한국");
    }

    /// Canonical ordering is part of every form: U+0323 (ccc 220) must sort
    /// before U+0307 (ccc 230) whichever order the input used. Values from
    /// `NormalizationTest.txt` Part 0, line 3.
    #[test]
    fn canonical_ordering_is_applied() {
        assert_eq!(nfd("\u{1E0A}\u{0323}"), "D\u{0323}\u{0307}");
        assert_eq!(nfc("\u{1E0A}\u{0323}"), "\u{1E0C}\u{0307}");
    }

    /// The `Cow` iff-guarantee over the cases that separate its two arms. A
    /// precomposed `"café"` is the one an ASCII-only test never reaches: it is
    /// already NFC (borrow) while its NFD contains a mark (allocate).
    #[test]
    fn cow_iff_over_the_cases_that_separate_the_arms() {
        for s in [
            "",
            "café",                     // NFC yes, NFD no
            "cafe\u{0301}",             // NFD yes, NFC no
            "Ａ",                       // NFC/NFD yes, NFKC/NFKD no
            "한국",                     // NFC yes, NFD no
            "\u{1112}\u{1161}\u{11AB}", // NFD yes, NFC no
            "😀々",                     // all four yes
            "\u{1E0A}\u{0323}",         // out of canonical order: all four no
            "Москва",
            "日本語",
            "ｶﾞ",
            "\u{1D167}",
        ] {
            assert_cow_iff(s);
        }
    }
}
