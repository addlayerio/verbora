//! `NGramsZH` — the Chinese variant, which splits characters instead of
//! tokenizing.
//!
//! The reference `ngrams_zh` is `ngrams` with two changes: there is no
//! statistics code, and string input goes through `sequence.split('')` instead of
//! a tokenizer. It exposes only `ngrams`, `bigrams` and `trigrams` — no
//! `setTokenizer`, no `multrigrams`, no `stats` argument, and no module-level
//! state — and none of those are added here.
//!
//! The padding and windowing are byte-for-byte the same algorithm, so this module
//! is a thin front end over [`crate::engine`]. **Array input needs nothing from
//! this module at all**: `NGramsZH.ngrams(array, …)` and `NGrams.ngrams(array, …)`
//! are the same function, so use [`crate::ngrams`] for it.
//!
//! # `split('')` splits UTF-16 code units
//!
//! This is the one real difference, and it is observable:
//!
//! ```text
//! NGramsZH.ngrams('a👍b', 2)
//!   The reference : [['a','\ud83d'], ['\ud83d','\udc4d'], ['\udc4d','b']]   (length 4)
//!   Rust chars : [['a','👍'], ['👍','b']]                                (length 3)
//! ```
//!
//! `'👍'.length === 2`, so the emoji is torn into its surrogate halves and each
//! half becomes its own element. Combining marks are separated too: `'éx'` in NFD
//! yields `['e','◌́']`, `['◌́','x']`.
//!
//! Rust's `String` cannot hold an unpaired surrogate, so this module offers two
//! entry points:
//!
//! | Function | Representation | Astral input |
//! |---|---|---|
//! | [`ngrams_zh_utf16`] | `&[u16]` code units | **exact**, surrogates preserved |
//! | [`ngrams_zh`] | `Cow<str>` | correct shape and positions; each unpaired surrogate renders as `U+FFFD` |
//!
//! [`ngrams_zh`] is the ergonomic one and is exact for the entire Basic
//! Multilingual Plane — which includes every CJK character this module exists to
//! serve. Reach for [`ngrams_zh_utf16`] when the input may contain astral
//! characters and the elements must round-trip.

use std::borrow::Cow;

use crate::engine::ngrams;

/// The UTF-16 code units of `text` — the reference's `String#split('')` before it
/// wraps each unit back into a string.
///
/// ```
/// use verbora_ngrams::zh::code_units;
///
/// assert_eq!(code_units("中"), vec![0x4E2D]);
/// assert_eq!(code_units("👍"), vec![0xD83D, 0xDC4D]); // one char, two units
/// ```
pub fn code_units(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

/// Splits `text` into one element per UTF-16 code unit, rendering unpaired
/// surrogates as `U+FFFD`.
///
/// The element **count** and the positions are exactly the reference's, so every
/// n-gram lands on the same boundaries; only the rendering of a torn surrogate
/// pair differs, because Rust has no way to spell one. BMP characters — all of
/// CJK — are borrowed slices of `text` and cost nothing.
pub fn split_lossy(text: &str) -> Vec<Cow<'_, str>> {
    let mut out = Vec::with_capacity(text.len());
    for (at, ch) in text.char_indices() {
        if ch.len_utf16() == 1 {
            out.push(Cow::Borrowed(&text[at..at + ch.len_utf8()]));
        } else {
            // The reference would yield the high surrogate and then the low one.
            out.push(Cow::Borrowed("\u{FFFD}"));
            out.push(Cow::Borrowed("\u{FFFD}"));
        }
    }
    out
}

/// The n-grams of `text`, split by UTF-16 code unit — `NGramsZH.ngrams`.
///
/// Exact for BMP input; see the module documentation for the astral case.
///
/// ```
/// use verbora_ngrams::zh::ngrams_zh;
///
/// assert_eq!(
///     ngrams_zh("中文测试", 2, None, None),
///     vec![vec!["中", "文"], vec!["文", "测"], vec!["测", "试"]]
/// );
/// ```
///
/// # The cheap path
///
/// This returns the reference's shape, so every tuple is its own `Vec`. Splitting
/// once and windowing over the result keeps the tuples borrowed and allocates
/// nothing per n-gram — measurably faster on document-scale input:
///
/// ```
/// use verbora_ngrams::ngrams;
/// use verbora_ngrams::zh::split_lossy;
///
/// let units = split_lossy("中文测试");
/// let grams = ngrams(&units, 2, None, None); // Cow::Borrowed windows
/// assert_eq!(grams.len(), 3);
/// ```
pub fn ngrams_zh<'a>(
    text: &'a str,
    n: usize,
    start_symbol: Option<&'a str>,
    end_symbol: Option<&'a str>,
) -> Vec<Vec<Cow<'a, str>>> {
    let units = split_lossy(text);
    ngrams(
        &units,
        n,
        start_symbol.map(Cow::Borrowed),
        end_symbol.map(Cow::Borrowed),
    )
    .into_iter()
    .map(Cow::into_owned)
    .collect()
}

/// The 2-grams of `text` — `NGramsZH.bigrams`.
pub fn bigrams_zh<'a>(
    text: &'a str,
    start_symbol: Option<&'a str>,
    end_symbol: Option<&'a str>,
) -> Vec<Vec<Cow<'a, str>>> {
    ngrams_zh(text, 2, start_symbol, end_symbol)
}

/// The 3-grams of `text` — `NGramsZH.trigrams`.
pub fn trigrams_zh<'a>(
    text: &'a str,
    start_symbol: Option<&'a str>,
    end_symbol: Option<&'a str>,
) -> Vec<Vec<Cow<'a, str>>> {
    ngrams_zh(text, 3, start_symbol, end_symbol)
}

/// The n-grams of a UTF-16 buffer, one code unit per element — the exact form of
/// `NGramsZH.ngrams`.
///
/// Every element of the result is a slice of `units` (one code unit long) or the
/// whole of `start_symbol`/`end_symbol`, so unpaired surrogates survive
/// untouched. Encode the input with [`code_units`] and the pad symbols with
/// `str::encode_utf16`.
///
/// ```
/// use verbora_ngrams::zh::{code_units, ngrams_zh_utf16};
///
/// let units = code_units("a👍b");
/// let grams = ngrams_zh_utf16(&units, 2, None, None);
/// // Three bigrams, not two: the emoji contributes two elements.
/// assert_eq!(grams.len(), 3);
/// assert_eq!(grams[1], vec![&[0xD83Du16][..], &[0xDC4Du16][..]]);
/// ```
pub fn ngrams_zh_utf16<'a>(
    units: &'a [u16],
    n: usize,
    start_symbol: Option<&'a [u16]>,
    end_symbol: Option<&'a [u16]>,
) -> Vec<Vec<&'a [u16]>> {
    // One element per code unit. `chunks(1)` yields the same slices the reference
    // `split('')` yields strings for, without copying any of them.
    let elements: Vec<&[u16]> = units.chunks(1).collect();
    ngrams(&elements, n, start_symbol, end_symbol)
        .into_iter()
        .map(Cow::into_owned)
        .collect()
}

/// The 2-grams of a UTF-16 buffer — the exact form of `NGramsZH.bigrams`.
pub fn bigrams_zh_utf16<'a>(
    units: &'a [u16],
    start_symbol: Option<&'a [u16]>,
    end_symbol: Option<&'a [u16]>,
) -> Vec<Vec<&'a [u16]>> {
    ngrams_zh_utf16(units, 2, start_symbol, end_symbol)
}

/// The 3-grams of a UTF-16 buffer — the exact form of `NGramsZH.trigrams`.
pub fn trigrams_zh_utf16<'a>(
    units: &'a [u16],
    start_symbol: Option<&'a [u16]>,
    end_symbol: Option<&'a [u16]>,
) -> Vec<Vec<&'a [u16]>> {
    ngrams_zh_utf16(units, 3, start_symbol, end_symbol)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cjk_is_split_per_character() {
        assert_eq!(
            ngrams_zh("中文文本测试", 2, None, None),
            vec![
                vec!["中", "文"],
                vec!["文", "文"],
                vec!["文", "本"],
                vec!["本", "测"],
                vec!["测", "试"],
            ]
        );
    }

    #[test]
    fn empty_and_single_character_input() {
        assert!(ngrams_zh("", 2, None, None).is_empty());
        assert_eq!(
            ngrams_zh("", 2, Some("<s>"), Some("</s>")),
            vec![vec!["<s>"], vec!["</s>"]]
        );
        assert_eq!(ngrams_zh("中", 1, None, None), vec![vec!["中"]]);
        assert!(ngrams_zh("中", 2, None, None).is_empty());
    }

    #[test]
    fn zero_and_oversized_n() {
        assert_eq!(
            ngrams_zh("abc", 0, None, None),
            vec![Vec::<Cow<'_, str>>::new(); 4]
        );
        assert!(ngrams_zh("abc", 9, None, None).is_empty());
    }

    #[test]
    fn latin_greek_cyrillic_and_digits_are_per_character() {
        assert_eq!(
            ngrams_zh("ABC", 2, None, None),
            vec![vec!["A", "B"], vec!["B", "C"]]
        );
        assert_eq!(ngrams_zh("café", 2, None, None).len(), 3);
        assert_eq!(ngrams_zh("Ελλάδα", 2, None, None).len(), 5);
        assert_eq!(ngrams_zh("Москва", 3, None, None).len(), 4);
        assert_eq!(
            ngrams_zh("123", 2, None, None),
            vec![vec!["1", "2"], vec!["2", "3"]]
        );
        assert_eq!(
            ngrams_zh("!?.", 2, None, None),
            vec![vec!["!", "?"], vec!["?", "."]]
        );
    }

    #[test]
    fn combining_marks_are_separate_elements() {
        // "e" + U+0301, i.e. NFD: two code units, so one bigram.
        let nfd = "e\u{301}x";
        assert_eq!(
            ngrams_zh(nfd, 2, None, None),
            vec![vec!["e", "\u{301}"], vec!["\u{301}", "x"]]
        );
    }

    #[test]
    fn astral_characters_contribute_two_elements() {
        // The shape is the reference's: three bigrams over a 4-code-unit string.
        let got = ngrams_zh("a👍b", 2, None, None);
        assert_eq!(got.len(), 3);
        assert_eq!(got[0][0], "a");
        assert_eq!(got[0][1], "\u{FFFD}"); // torn high surrogate
        assert_eq!(got[2][1], "b");
    }

    #[test]
    fn utf16_form_preserves_surrogates_exactly() {
        let units = code_units("a👍b");
        assert_eq!(units.len(), 4);
        let got = ngrams_zh_utf16(&units, 2, None, None);
        assert_eq!(
            got,
            vec![
                vec![&[0x0061u16][..], &[0xD83D][..]],
                vec![&[0xD83Du16][..], &[0xDC4D][..]],
                vec![&[0xDC4Du16][..], &[0x0062][..]],
            ]
        );
    }

    #[test]
    fn utf16_form_takes_multi_unit_pad_symbols() {
        let units = code_units("中文");
        let start: Vec<u16> = "<s>".encode_utf16().collect();
        let got = ngrams_zh_utf16(&units, 2, Some(&start), None);
        assert_eq!(got[0], vec![&start[..], &units[..1]]);
    }

    #[test]
    fn padding_matches_the_shared_engine() {
        // Recorded from the reference: `NGramsZH.ngrams('ab', 3, 'S', 'E')`.
        // With len == n - 1 the window loop produces nothing at all, and both
        // padding halves are clamped rather than symmetric.
        assert_eq!(
            ngrams_zh("ab", 3, Some("S"), Some("E")),
            vec![
                vec!["S", "S", "a"],
                vec!["S", "a", "b"],
                vec!["a", "b", "E"],
                vec!["b", "E", "E"],
            ]
        );
    }

    #[test]
    fn aliases_agree() {
        assert_eq!(
            bigrams_zh("中文测试", None, None),
            ngrams_zh("中文测试", 2, None, None)
        );
        assert_eq!(
            trigrams_zh("中文测试", None, None),
            ngrams_zh("中文测试", 3, None, None)
        );
        let units = code_units("中文测试");
        assert_eq!(
            bigrams_zh_utf16(&units, None, None),
            ngrams_zh_utf16(&units, 2, None, None)
        );
        assert_eq!(
            trigrams_zh_utf16(&units, None, None),
            ngrams_zh_utf16(&units, 3, None, None)
        );
    }

    #[test]
    fn very_long_input() {
        let text = "中文".repeat(5_000);
        assert_eq!(ngrams_zh(&text, 2, None, None).len(), 9_999);
    }
}
