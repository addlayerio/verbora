//! The Swedish stemmer, ported from
//! The reference `porter_stemmer_sv`.
//!
//! # Rebuilt from `rest`, not truncated
//!
//! Steps 1a and 3 do not cut the token; they return `regions.rest +
//! r1.slice(0, match.index)`. `rest` is `str.slice(0, str.length - r1.length)`,
//! which is only the same thing as "the token minus R1" when R1 really is a
//! suffix — and it need not be, because the capture class `[a-zåäö]` excludes
//! digits, `-`, `ü` and every uppercase letter, so R1 stops early on
//! `"björk-1"`. The reference's arithmetic is reproduced literally rather than
//! simplified into a truncation.
//!
//! # `getRegions` has a comment admitting it is unexplained
//!
//! `if (match.index + 2 < 3) r1 = str.slice(3)` carries the note *"Not clear why
//! we need this! Algorithm does not describe this part!"*. It fires exactly when
//! the match starts at index 0, and it is kept.
//!
//! # Step 1 keeps the shorter of 1a and 1b
//!
//! Like Norwegian, and with the same strict `<`: on a tie step 1b wins. Unlike
//! Norwegian, both branches share one `getRegions` call, so the regions are those
//! of the *input*; steps 2 and 3 recompute them from their own argument through
//! The reference's default-parameter evaluation.

use std::borrow::Cow;

use verbora_normalizers::normalize_sv;
use verbora_tokenizers::classes;

use crate::base::{Casing, TokenizeAndStem};
use crate::stopwords::{self, Language};
use crate::units::{ends_with, slen, text, units};

/// The Swedish stemmer.
///
/// ```
/// use verbora_stemmers::PorterStemmerSv;
/// let s = PorterStemmerSv::new();
/// assert_eq!(s.stem("björks"), "björk");
/// assert_eq!(s.stem("jaktbössa"), "jaktböss");
/// assert_eq!(s.stem("BJÖRKS"), "björk");
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PorterStemmerSv;

/// `[aeiouyäåö]` — lowercase only.
#[inline]
fn is_vowel(c: u16) -> bool {
    matches!(
        c,
        0x61 | 0x65 | 0x69 | 0x6F | 0x75 | 0x79 | 0xE4 | 0xE5 | 0xF6
    )
}

/// `[a-zåäö]`, the class R1's captured run is drawn from.
#[inline]
fn is_r1_char(c: u16) -> bool {
    matches!(c, 0x61..=0x7A | 0xE5 | 0xE4 | 0xF6)
}

/// R1 and the prefix that precedes it *by length*.
///
/// `r1` borrows from the same `t` its owning `step*` function was called
/// with: `regions(t)` is always the first thing each `step*` function does,
/// and `t` is a `&[u16]` parameter that function never reassigns, so the
/// borrow trivially outlives every use of `r.r1` below it. Unlike `w` in the
/// other Snowball ports, there is no later `t = ...` in these functions for
/// the borrow to conflict with.
struct Regions<'t> {
    r1: &'t [u16],
    rest_len: usize,
}

/// `getRegions`. R1 is empty when the pattern does not match at all.
fn regions(t: &[u16]) -> Regions<'_> {
    let mut r1: &[u16] = &[];
    if let Some(index) = (0..t.len().saturating_sub(2))
        .find(|&i| is_vowel(t[i]) && !is_vowel(t[i + 1]) && is_r1_char(t[i + 2]))
    {
        let end = (index + 2..t.len())
            .find(|&i| !is_r1_char(t[i]))
            .unwrap_or(t.len());
        r1 = &t[index + 2..end];
        if index == 0 {
            r1 = t.get(3..).unwrap_or_default();
        }
    }
    Regions {
        rest_len: t.len().saturating_sub(r1.len()),
        r1,
    }
}

/// The longest suffix of `w` present in `alternatives`, and where it starts.
///
/// `/(x|y|z)$/` takes the earliest start position at which an alternative
/// reaches `$`; since every alternative is a distinct literal that must end at
/// `$`, that is the longest listed suffix. The index is returned because steps 1a
/// and 3 slice with it rather than truncating.
fn listed_suffix(w: &[u16], alternatives: &[&str]) -> Option<usize> {
    let mut best: Option<usize> = None;
    for a in alternatives {
        if ends_with(w, a) {
            let start = w.len() - slen(a);
            if best.is_none_or(|b| start < b) {
                best = Some(start);
            }
        }
    }
    best
}

fn step1a(t: &[u16], r: &Regions<'_>) -> Vec<u16> {
    if r.r1.is_empty() {
        return t.to_vec();
    }
    match listed_suffix(r.r1, STEP1A) {
        Some(idx) => {
            let mut out = t[..r.rest_len.min(t.len())].to_vec();
            out.extend_from_slice(&r.r1[..idx]);
            out
        }
        None => t.to_vec(),
    }
}

fn step1b(t: &[u16], r: &Regions<'_>) -> Vec<u16> {
    // `/(b|c|d|f|g|h|j|k|l|m|n|o|p|r|t|v|y)s$/` — this list has `k` where the
    // Norwegian one does not, and again includes the vowel `o`.
    if !r.r1.is_empty()
        && t.len() >= 2
        && t[t.len() - 1] == 0x73
        && matches!(
            t[t.len() - 2],
            0x62 | 0x63
                | 0x64
                | 0x66
                | 0x67
                | 0x68
                | 0x6A
                | 0x6B
                | 0x6C
                | 0x6D
                | 0x6E
                | 0x6F
                | 0x70
                | 0x72
                | 0x74
                | 0x76
                | 0x79
        )
    {
        return t[..t.len() - 1].to_vec();
    }
    t.to_vec()
}

fn step1(t: &[u16]) -> Vec<u16> {
    let r = regions(t);
    let a = step1a(t, &r);
    let b = step1b(t, &r);
    if a.len() < b.len() { a } else { b }
}

fn step2(t: &[u16]) -> Vec<u16> {
    let r = regions(t);
    if !r.r1.is_empty()
        && listed_suffix(r.r1, &["dd", "gd", "nn", "dt", "gt", "kt", "tt"]).is_some()
    {
        return t[..t.len().saturating_sub(1)].to_vec();
    }
    t.to_vec()
}

fn step3(t: &[u16]) -> Vec<u16> {
    let r = regions(t);
    if r.r1.is_empty() {
        return t.to_vec();
    }
    // `/(lös|full)t$/` — the trailing `t` is outside the group.
    if listed_suffix(r.r1, &["löst", "fullt"]).is_some() {
        return t[..t.len().saturating_sub(1)].to_vec();
    }
    match listed_suffix(r.r1, &["lig", "ig", "els"]) {
        Some(idx) => {
            let mut out = t[..r.rest_len.min(t.len())].to_vec();
            out.extend_from_slice(&r.r1[..idx]);
            out
        }
        None => t.to_vec(),
    }
}

/// The step-1a alternation, in source order.
static STEP1A: &[&str] = &[
    "heterna", "hetens", "anden", "andes", "andet", "arens", "arnas", "ernas", "heten", "heter",
    "ornas", "ande", "ades", "aren", "arna", "arne", "aste", "erna", "erns", "orna", "ade", "are",
    "ast", "ens", "ern", "het", "ad", "ar", "as", "at", "en", "er", "es", "or", "a", "e",
];

impl PorterStemmerSv {
    /// Creates the stemmer. It is stateless and zero-sized.
    #[inline]
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Stems one token: lowercase, then step 1, step 2, step 3.
    #[allow(
        clippy::unused_self,
        reason = "mirrors the reference's method-shaped API"
    )]
    #[must_use]
    pub fn stem<'a>(&self, token: &'a str) -> Cow<'a, str> {
        let lower = token.to_lowercase();
        Cow::Owned(text(&step3(&step2(&step1(&units(&lower))))))
    }

    /// Appends a stop word to the **process-global Swedish list**.
    pub fn add_stop_word(&self, word: impl Into<String>) {
        stopwords::add(Language::Sv, word);
    }

    /// Appends several stop words to the process-global Swedish list.
    pub fn add_stop_words<I, S>(&self, words: I)
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        stopwords::add_all(Language::Sv, words);
    }
}

impl TokenizeAndStem for PorterStemmerSv {
    const FILTER_ON: Casing = Casing::Lower;
    const STEM_ON: Casing = Casing::Raw;

    fn is_word_char(c: char) -> bool {
        classes::is_word_sv(c)
    }

    /// `AggressiveTokenizerSv` folds `à á è é` — and only their first occurrence
    /// each — before splitting.
    fn prepare(text: &str) -> Cow<'_, str> {
        normalize_sv(text)
    }

    fn is_stop_word(word: &str) -> bool {
        stopwords::contains(Language::Sv, word)
    }

    fn stem_token(&self, token: &str) -> String {
        self.stem(token).into_owned()
    }
}

impl verbora_core::Stemmer for PorterStemmerSv {
    fn stem<'a>(&self, token: &'a str) -> Cow<'a, str> {
        Self::stem(self, token)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(t: &str) -> String {
        PorterStemmerSv::new().stem(t).into_owned()
    }

    #[test]
    fn documented_vectors() {
        for (input, want) in [
            ("björks", "björk"),
            ("BJÖRKS", "björk"),
            ("jaktbössa", "jaktböss"),
            ("klockorna", "klock"),
            ("flickornas", "flick"),
            ("stiftelsen", "stift"),
            ("frihetens", "frihet"),
            ("härligt", "här"),
            ("körsbärsträdgårdarna", "körsbärsträdgård"),
            // R1 of "fullt" is "llt", which does not end in "fullt", so the
            // `(lös|full)t` rule cannot fire on the word it was written for.
            ("fullt", "fullt"),
            ("löst", "löst"),
            ("", ""),
        ] {
            assert_eq!(s(input), want, "stem({input})");
        }
    }

    /// The cross-cutting battery from `docs/PARITY.md`: empty, one character,
    /// uppercase, accented Latin, Greek, Cyrillic, CJK, an astral pair,
    /// punctuation, digits, a line terminator, and a very long word. Every
    /// expectation below was read off the reference with `node`.
    #[test]
    fn cross_script_battery() {
        for (input, want) in [
            ("", ""),
            ("a", "a"),
            ("A", "a"),
            ("Ä", "ä"),
            ("café", "café"),
            ("ΟΔΟΣ", "οδος"),
            ("Ω", "ω"),
            ("мама", "мама"),
            ("日本語", "日本語"),
            ("😀", "😀"),
            ("😀ab", "😀ab"),
            ("!?,.", "!?,."),
            ("123", "123"),
            ("\n", "\n"),
        ] {
            assert_eq!(s(input), want, "stem({input:?})");
        }
        assert_eq!(s(&"x".repeat(1000)).len(), 1000);
    }
}
