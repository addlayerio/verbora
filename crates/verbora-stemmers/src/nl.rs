//! The Dutch Snowball stemmer, ported from
//! The reference `porter_stemmer_nl`.
//!
//! # The sticky flag
//!
//! `step2` sets `this.suffixeRemoved = true` when it deletes an `e`. **Nothing
//! ever resets it**, and `step3b`'s `bar` rule reads it — on a module-level
//! singleton. Stemming is therefore order dependent:
//!
//! ```
//! use verbora_stemmers::PorterStemmerNl;
//! let s = PorterStemmerNl::new();
//! assert_eq!(s.stem("onaantastbar"), "onaantastbar"); // cold
//! assert_eq!(s.stem("lichte"), "licht");              // trips the flag
//! assert_eq!(s.stem("onaantastbar"), "onaantast");    // hot
//! ```
//!
//! Over the reference's own 45,669-word corpus this is the difference between
//! **237** mismatches — the number the reference spec asserts — and 235 with a
//! fresh instance per word. The two order-dependent words are `jongensgebaren`
//! and `molenwiekgebaren`. A pure-function port silently re-baselines the
//! reference test suite, so the flag is reproduced: [`PorterStemmerNl`] owns a
//! `Cell<bool>` initialised to `false` at construction and never cleared.
//!
//! Because the state lives in the value rather than in a process global, a caller
//! who *wants* determinism can construct a fresh stemmer per word — which is a
//! choice the reference does not offer.
//!
//! # Case sensitivity
//!
//! Only the postlude lowercases. Everything before it — the accent folding, the
//! `y`/`i` marking, the region scan, every suffix literal — is lowercase-only and
//! case-sensitive, so `stem("aachen")` is `"aach"` while `stem("AACHEN")` is just
//! `"aachen"`.

use std::borrow::Cow;
use std::cell::Cell;

use verbora_tokenizers::classes;

use crate::base::{Casing, TokenizeAndStem};
use crate::data::gates::gate_nl;
use crate::stopwords::{self, Language};
use crate::units::{ends_with, longest_suffix, slen, text, u, units};

/// The Dutch Snowball stemmer.
///
/// **Not `Sync`**: it carries the sticky `suffixeRemoved` flag described in the
/// module documentation, exactly as the reference's exported singleton does.
#[derive(Debug, Clone, Default)]
pub struct PorterStemmerNl {
    /// `this.suffixeRemoved` — set by step 2, read by step 3b, never reset.
    suffix_e_removed: Cell<bool>,
}

/// `vowels = 'aeiouèy'`.
#[inline]
fn is_vowel(c: u16) -> bool {
    matches!(c, 0x61 | 0x65 | 0x69 | 0x6F | 0x75 | 0xE8 | 0x79)
}

#[inline]
fn is_vowel_at(w: &[u16], i: usize) -> bool {
    w.get(i).copied().is_some_and(is_vowel)
}

/// `undoubleEnding`: drop the last letter when the word ends `kk`, `tt` or `dd`.
fn undouble_ending(w: &mut Vec<u16>) {
    if ends_with(w, "kk") || ends_with(w, "tt") || ends_with(w, "dd") {
        w.truncate(w.len() - 1);
    }
}

impl PorterStemmerNl {
    /// Creates a stemmer with the sticky flag cleared.
    #[inline]
    #[must_use]
    pub const fn new() -> Self {
        Self {
            suffix_e_removed: Cell::new(false),
        }
    }

    /// Whether step 2 has ever removed an `e` on this stemmer.
    ///
    /// Exposed because it is the one piece of cross-call state in the crate, and
    /// a caller reasoning about reproducibility needs to be able to see it.
    #[must_use]
    pub fn suffix_e_removed(&self) -> bool {
        self.suffix_e_removed.get()
    }

    /// `replaceAccentedCharacters`: ten fixed mappings, applied in one pass.
    fn replace_accented(w: &mut [u16]) {
        for c in w {
            match *c {
                x if x == u('ä') || x == u('á') => *c = u('a'),
                x if x == u('ë') || x == u('é') => *c = u('e'),
                x if x == u('ï') || x == u('í') => *c = u('i'),
                x if x == u('ö') || x == u('ó') => *c = u('o'),
                x if x == u('ü') || x == u('ú') => *c = u('u'),
                _ => {}
            }
        }
    }

    /// `handleYI`: initial `y`, `y` after a vowel, and `i` between vowels.
    ///
    /// The `é` in the reference's character classes is dead — `replaceAccented`
    /// has already turned every `é` into `e` — and is left out for that reason,
    /// not overlooked.
    fn handle_yi(w: &mut [u16]) {
        if w.first() == Some(&u('y')) {
            w[0] = u('Y');
        }
        // `/([aeioue])y/g` — non-overlapping, left to right.
        let mut i = 0;
        while i + 1 < w.len() {
            if matches!(w[i], 0x61 | 0x65 | 0x69 | 0x6F | 0x75) && w[i + 1] == u('y') {
                w[i + 1] = u('Y');
                i += 2;
            } else {
                i += 1;
            }
        }
        // `/([aeioue])i([aeioue])/g` — "aiaia" becomes "aIaia", not "aIaIa".
        let mut i = 0;
        while i + 2 < w.len() {
            if matches!(w[i], 0x61 | 0x65 | 0x69 | 0x6F | 0x75)
                && w[i + 1] == u('i')
                && matches!(w[i + 2], 0x61 | 0x65 | 0x69 | 0x6F | 0x75)
            {
                w[i + 1] = u('I');
                i += 3;
            } else {
                i += 1;
            }
        }
    }

    /// `markRegions`. Unlike German, R1 is adjusted **before** R2 is scanned.
    fn mark_regions(w: &[u16]) -> (usize, usize) {
        let len = w.len();
        let mut r1 = len;
        for i in 0..len.saturating_sub(1) {
            if r1 != len {
                break;
            }
            if is_vowel(w[i]) && !is_vowel(w[i + 1]) {
                r1 = i + 2;
            }
        }
        if r1 != len && r1 < 3 {
            r1 = if len > 3 { 3 } else { len };
        }
        let mut r2 = len;
        for i in r1..len.saturating_sub(1) {
            if r2 != len {
                break;
            }
            if is_vowel(w[i]) && !is_vowel(w[i + 1]) {
                r2 = i + 2;
            }
        }
        (r1, r2)
    }

    /// `step1b`: delete a suffix in R1 preceded by a valid en-ending, then
    /// undouble.
    fn step1b(w: &mut Vec<u16>, r1: usize, suffixes: &[&str]) {
        let Some(m) = longest_suffix(w, suffixes) else {
            return;
        };
        let pos = w.len() - slen(m);
        if pos < r1 {
            return;
        }
        // `pos >= 3` is guaranteed: R1 is either at least 3 or equal to the
        // length, and a matched suffix is non-empty, so the substr start below
        // is never negative.
        let gem = pos >= 3 && &w[pos - 3..pos] == units("gem").as_slice();
        if !is_vowel_at(w, pos.wrapping_sub(1)) && !gem {
            w.truncate(pos);
            undouble_ending(w);
        }
    }

    fn step1(w: &mut Vec<u16>, r1: usize) {
        // (1a) heden -> heid in R1
        if ends_with(w, "heden") && w.len() >= 5 && w.len() - 5 >= r1 {
            w.truncate(w.len() - 5);
            w.extend("heid".encode_utf16());
        }
        // (1b) en / ene
        Self::step1b(w, r1, &["en", "ene"]);
        // (1c) s / se in R1, preceded by a valid s-ending
        if let Some(m) = longest_suffix(w, &["se", "s"]) {
            let pos = w.len() - slen(m);
            if pos >= r1 && !is_vowel_at(w, pos.wrapping_sub(1)) && !Self::se_guard(w) {
                w.truncate(pos);
            }
        }
    }

    /// `!result.match(/[js]se?$/)` — the reference's own note says a preceding
    /// `s` means the suffix should stay.
    fn se_guard(w: &[u16]) -> bool {
        // `[js]se?$` is either `[js]s` or `[js]se` at the end.
        let tail = |n: usize| -> Option<&[u16]> { w.len().checked_sub(n).map(|i| &w[i..]) };
        if let Some(t) = tail(2)
            && matches!(t[0], 0x6A | 0x73)
            && t[1] == u('s')
        {
            return true;
        }
        if let Some(t) = tail(3)
            && matches!(t[0], 0x6A | 0x73)
            && t[1] == u('s')
            && t[2] == u('e')
        {
            return true;
        }
        false
    }

    /// `step2`: delete `e` in R1 after a non-vowel — and set the sticky flag.
    fn step2(&self, w: &mut Vec<u16>, r1: usize) {
        if ends_with(w, "e") && r1 < w.len() && w.len() > 1 && !is_vowel_at(w, w.len() - 2) {
            w.truncate(w.len() - 1);
            self.suffix_e_removed.set(true);
            undouble_ending(w);
        }
    }

    fn step3a(w: &mut Vec<u16>, r1: usize, r2: usize) {
        if ends_with(w, "heid")
            && w.len() >= 4
            && w.len() - 4 >= r2
            && w.len() >= 5
            && w[w.len() - 5] != u('c')
        {
            w.truncate(w.len() - 4);
            Self::step1b(w, r1, &["en"]);
        }
    }

    fn step3b(&self, w: &mut Vec<u16>, r1: usize, r2: usize) {
        if longest_suffix(w, &["end", "ing"]).is_some() && w.len() >= 3 && w.len() - 3 >= r2 {
            w.truncate(w.len() - 3);
            if ends_with(w, "ig")
                && w.len() >= 2
                && w.len() - 2 >= r2
                && w.len() >= 3
                && w[w.len() - 3] != u('e')
            {
                w.truncate(w.len() - 2);
            } else {
                undouble_ending(w);
            }
        }
        // Independent of the block above, not an `else`.
        if ends_with(w, "ig")
            && w.len() >= 2
            && r2 <= w.len() - 2
            && w.len() >= 3
            && w[w.len() - 3] != u('e')
        {
            w.truncate(w.len() - 2);
        }
        if ends_with(w, "lijk") && w.len() >= 4 && r2 <= w.len() - 4 {
            w.truncate(w.len() - 4);
            self.step2(w, r1);
        }
        if ends_with(w, "baar") && w.len() >= 4 && r2 <= w.len() - 4 {
            w.truncate(w.len() - 4);
        }
        // The sticky flag decides this one.
        if ends_with(w, "bar") && w.len() >= 3 && r2 <= w.len() - 3 && self.suffix_e_removed.get() {
            w.truncate(w.len() - 3);
        }
    }

    /// `step4`: undouble a long vowel between two consonants.
    fn step4(w: &mut Vec<u16>) {
        const CONS: &[u16] = &[
            0x62, 0x63, 0x64, 0x66, 0x67, 0x68, 0x6A, 0x6B, 0x6C, 0x6D, 0x6E, 0x70, 0x71, 0x72,
            0x73, 0x74, 0x76, 0x77, 0x78, 0x7A,
        ];
        if w.len() < 4 {
            return;
        }
        let n = w.len();
        let doubled = matches!(
            (w[n - 3], w[n - 2]),
            (0x61, 0x61) | (0x65, 0x65) | (0x6F, 0x6F) | (0x75, 0x75)
        );
        if doubled && CONS.contains(&w[n - 4]) && CONS.contains(&w[n - 1]) {
            let last = w[n - 1];
            w.truncate(n - 2);
            w.push(last);
        }
    }

    /// Stems one token, updating the sticky flag.
    pub fn stem<'a>(&self, word: &'a str) -> Cow<'a, str> {
        let mut w = units(word);
        Self::replace_accented(&mut w);
        Self::handle_yi(&mut w);
        let (r1, r2) = Self::mark_regions(&w);
        Self::step1(&mut w, r1);
        self.step2(&mut w, r1);
        Self::step3a(&mut w, r1, r2);
        self.step3b(&mut w, r1, r2);
        Self::step4(&mut w);
        Cow::Owned(text(&w).to_lowercase())
    }
}

impl TokenizeAndStem for PorterStemmerNl {
    const FILTER_ON: Casing = Casing::Raw;
    const STEM_ON: Casing = Casing::Lower;

    fn is_word_char(c: char) -> bool {
        classes::is_word_nl(c)
    }

    fn is_stop_word(word: &str) -> bool {
        stopwords::contains(Language::Nl, word)
    }

    fn gate(token: &str) -> bool {
        token.encode_utf16().any(gate_nl)
    }

    fn stem_token(&self, token: &str) -> String {
        self.stem(token).into_owned()
    }
}

impl verbora_core::Stemmer for PorterStemmerNl {
    fn stem<'a>(&self, token: &'a str) -> Cow<'a, str> {
        Self::stem(self, token)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(t: &str) -> String {
        PorterStemmerNl::new().stem(t).into_owned()
    }

    #[test]
    fn the_flag_is_sticky() {
        let st = PorterStemmerNl::new();
        assert_eq!(st.stem("onaantastbar"), "onaantastbar");
        assert!(!st.suffix_e_removed());
        assert_eq!(st.stem("lichte"), "licht");
        assert!(st.suffix_e_removed());
        assert_eq!(st.stem("onaantastbar"), "onaantast");
    }

    #[test]
    fn a_fresh_stemmer_starts_cold() {
        assert_eq!(s("onaantastbar"), "onaantastbar");
        assert_eq!(s("jongensgebaren"), "jongensgebar");
        assert_eq!(s("molenwiekgebaren"), "molenwiekgebar");
    }

    #[test]
    fn case_sensitivity() {
        assert_eq!(s("aachen"), "aach");
        assert_eq!(s("AACHEN"), "aachen");
    }

    #[test]
    fn documented_vectors() {
        for (input, want) in [
            ("", ""),
            ("a", "a"),
            ("ab", "ab"),
            ("abc", "abc"),
            ("kleurbaar", "kleurbar"),
        ] {
            assert_eq!(s(input), want, "stem({input})");
        }
    }

    #[test]
    fn unicode_and_edges() {
        assert_eq!(s("😀"), "😀");
        assert_eq!(s("日本語"), "日本語");
        assert_eq!(s("123"), "123");
    }
}
