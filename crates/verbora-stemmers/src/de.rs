//! The German Snowball stemmer, ported from
//! The reference `porter_stemmer_de`.
//!
//! # Case sensitivity is the whole story
//!
//! Nothing here lowercases. Every vowel class and every suffix literal is
//! lowercase, so an uppercase word flows through the region machinery matching
//! nothing: `stem("Häuser")` is `"Haus"` — the capital `H` survives — while
//! `stem("STRASSE")` and `stem("FRÖHLICH")` come back untouched. A `to_lowercase`
//! at the front would look like an obvious improvement and would change results
//! for every capitalised noun, which in German is most of them.
//!
//! # R2 is computed from the *unadjusted* R1
//!
//! The Snowball specification, quoted in the reference's own comment, says R1 is
//! adjusted so that at least three letters precede it. The code computes R1, then
//! R2 from the un-adjusted R1's substring, and only afterwards clamps R1 to 3.
//! Dutch, which is otherwise the same shape, adjusts first. Sharing one region
//! routine between the two breaks German — verified on `äckern`, where R1 becomes
//! 3 but R2 stays 5.
//!
//! # Suffix selection
//!
//! Each `word.search(/(a|b|c)$/)` returns the UTF-16 index where the match
//! *starts*. Because the alternatives are distinct literals anchored at `$`, the
//! earliest start is the longest suffix. The German code then does arithmetic on
//! those indices (`c1Index++`, `b2Index += 4`, `b3Index++`) and picks the smallest
//! with a **strict** `<`, so a tie keeps the earlier option letter — and the
//! option letter decides which follow-up rule runs.

use std::borrow::Cow;

use verbora_tokenizers::classes;

use crate::base::{Casing, TokenizeAndStem};
use crate::data::gates::gate_de;
use crate::stopwords::{self, Language};
use crate::units::{ends_with, slen, text, u, units};

/// Options for [`PorterStemmerDe::stem_with`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PorterStemmerDeOptions {
    /// Keep `ä ö ü` instead of folding them to `a o u` in the postlude.
    pub preserve_umlauts: bool,
}

/// The German Snowball stemmer.
///
/// ```
/// use verbora_stemmers::PorterStemmerDe;
/// let s = PorterStemmerDe::new();
/// assert_eq!(s.stem("bedürfnissen"), "bedurfnis");
/// assert_eq!(s.stem("Häuser"), "Haus");
/// assert_eq!(s.stem("STRASSE"), "STRASSE");
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PorterStemmerDe;

/// `[aeiouyäöü]` — lowercase only, as written.
#[inline]
fn is_vowel(c: u16) -> bool {
    matches!(
        c,
        0x61 | 0x65 | 0x69 | 0x6F | 0x75 | 0x79 | 0xE4 | 0xF6 | 0xFC
    )
}

/// `[bdfghklmnrt]`, the valid s-ending.
#[inline]
fn is_s_ending(c: u16) -> bool {
    matches!(
        c,
        0x62 | 0x64 | 0x66 | 0x67 | 0x68 | 0x6B | 0x6C | 0x6D | 0x6E | 0x72 | 0x74
    )
}

/// `[bdfghklmnt]`, the valid st-ending (the s-ending without `r`).
#[inline]
fn is_st_ending(c: u16) -> bool {
    matches!(
        c,
        0x62 | 0x64 | 0x66 | 0x67 | 0x68 | 0x6B | 0x6C | 0x6D | 0x6E | 0x74
    )
}

#[inline]
const fn is_line_terminator(c: u16) -> bool {
    matches!(c, 0x0A | 0x0D | 0x2028 | 0x2029)
}

/// `word.search(/(alt|…)$/)`: the start index of the longest matching suffix.
fn search_suffix(w: &[u16], alts: &[&str]) -> Option<usize> {
    let mut best: Option<usize> = None;
    for a in alts {
        let n = slen(a);
        if ends_with(w, a) && best.is_none_or(|b| n > b) {
            best = Some(n);
        }
    }
    best.map(|n| w.len() - n)
}

/// `word.replace(/([V])x([V])/g, '$1X$2')` for one marked letter.
///
/// The `/g` replace is non-overlapping and scans left to right, so `"auau"`
/// becomes `"aUau"`: the second `u` is inside the region the first match already
/// consumed.
fn mark_between_vowels(w: &mut [u16], letter: u16, marked: u16) {
    let mut i = 0;
    while i + 2 < w.len() {
        if is_vowel(w[i]) && w[i + 1] == letter && is_vowel(w[i + 2]) {
            w[i + 1] = marked;
            i += 3;
        } else {
            i += 1;
        }
    }
}

/// The first index where a vowel is followed by a non-vowel.
fn region_scan(w: &[u16], from: usize) -> Option<usize> {
    (from..w.len().saturating_sub(1)).find(|&i| is_vowel(w[i]) && !is_vowel(w[i + 1]))
}

/// The chosen option among several `(index, letter)` candidates.
///
/// `if (x !== -1 && x < index)` — a strict `<`, so the earlier letter wins ties.
fn choose(candidates: &[(Option<usize>, char)]) -> Option<(usize, char)> {
    let mut best: Option<(usize, char)> = None;
    for &(idx, letter) in candidates {
        if let Some(i) = idx
            && best.is_none_or(|(b, _)| i < b)
        {
            best = Some((i, letter));
        }
    }
    best
}

impl PorterStemmerDe {
    /// Creates the stemmer. It is stateless and zero-sized.
    #[inline]
    pub const fn new() -> Self {
        Self
    }

    /// Stems one token with default options.
    pub fn stem<'a>(&self, word: &'a str) -> Cow<'a, str> {
        self.stem_with(word, PorterStemmerDeOptions::default())
    }

    /// Stems one token, optionally keeping the umlauts.
    #[allow(
        clippy::unused_self,
        reason = "mirrors the reference's method-shaped API"
    )]
    pub fn stem_with<'a>(&self, word: &'a str, options: PorterStemmerDeOptions) -> Cow<'a, str> {
        let mut w = units(word);

        // --- Prelude -------------------------------------------------------
        // `u` and `y` between vowels are marked so they stop counting as vowels.
        // The commented-out ae/oe/ue mappings in the reference stay omitted:
        // they cause trouble with diphthongs, as its comment says.
        mark_between_vowels(&mut w, u('u'), u('U'));
        mark_between_vowels(&mut w, u('y'), u('Y'));
        if w.contains(&u('ß')) {
            let mut expanded = Vec::with_capacity(w.len() + 2);
            for c in &w {
                if *c == u('ß') {
                    expanded.extend("ss".encode_utf16());
                } else {
                    expanded.push(*c);
                }
            }
            w = expanded;
        }

        // --- Regions -------------------------------------------------------
        let mut r1_index = region_scan(&w, 0).map(|i| i + 2);
        // R2 comes from the UNADJUSTED R1. Reordering these two blocks is the
        // single most tempting "cleanup" in this file, and it changes results.
        let r2_index = r1_index.and_then(|r1| region_scan(&w[r1..], 0).map(|i| i + 2 + r1));
        if let Some(r1) = r1_index
            && r1 < 3
        {
            r1_index = Some(3);
        }

        // --- Step 1 --------------------------------------------------------
        let a1 = search_suffix(&w, &["em", "ern", "er"]);
        let b1 = search_suffix(&w, &["e", "en", "es"]);
        let c1 = (w.len() >= 2 && w[w.len() - 1] == u('s') && is_s_ending(w[w.len() - 2]))
            .then(|| w.len() - 1);
        if let Some((index1, option1)) = choose(&[(a1, 'a'), (b1, 'b'), (c1, 'c')])
            && let Some(r1) = r1_index
            && index1 >= r1
        {
            w.truncate(index1);
            if option1 == 'b' && ends_with(&w, "niss") {
                w.truncate(w.len() - 1);
            }
        }

        // --- Step 2 --------------------------------------------------------
        let a2 = search_suffix(&w, &["en", "er", "est"]);
        // `/(.{3}[bdfghklmnt]st)$/` then `+= 4`: three characters `.` can match
        // (so no line terminators), a valid st-ending, then `st`.
        let b2 = (w.len() >= 6
            && ends_with(&w, "st")
            && is_st_ending(w[w.len() - 3])
            && !w[w.len() - 6..w.len() - 3]
                .iter()
                .copied()
                .any(is_line_terminator))
        .then(|| w.len() - 2);
        if let Some((index2, _)) = choose(&[(a2, 'a'), (b2, 'b')])
            && let Some(r1) = r1_index
            && index2 >= r1
        {
            w.truncate(index2);
        }

        // --- Step 3 --------------------------------------------------------
        let a3 = search_suffix(&w, &["end", "ung"]);
        let b3 = Self::search_non_e_prefixed(&w, &["isch", "ig", "ik"]).map(|i| i + 1);
        let c3 = search_suffix(&w, &["lich", "heit"]);
        let d3 = search_suffix(&w, &["keit"]);
        if let Some((index3, option3)) = choose(&[(a3, 'a'), (b3, 'b'), (c3, 'c'), (d3, 'd')])
            && let Some(r2) = r2_index
            && index3 >= r2
        {
            w.truncate(index3);
            match option3 {
                'a' => {
                    if let Some(o) = Self::search_non_e_prefixed(&w, &["ig"]).map(|i| i + 1)
                        && o >= r2
                    {
                        w.truncate(o);
                    }
                }
                'c' => {
                    if let Some(o) = search_suffix(&w, &["er", "en"])
                        && r1_index.is_some_and(|r1| o >= r1)
                    {
                        w.truncate(o);
                    }
                }
                'd' => {
                    if let Some(o) = search_suffix(&w, &["lich", "ig"])
                        && o >= r2
                    {
                        w.truncate(o);
                    }
                }
                _ => {}
            }
        }

        // --- Postlude ------------------------------------------------------
        for c in &mut w {
            match *c {
                x if x == u('U') => *c = u('u'),
                x if x == u('Y') => *c = u('y'),
                _ => {}
            }
        }
        if !options.preserve_umlauts {
            for c in &mut w {
                match *c {
                    x if x == u('ä') => *c = u('a'),
                    x if x == u('ö') => *c = u('o'),
                    x if x == u('ü') => *c = u('u'),
                    _ => {}
                }
            }
        }
        Cow::Owned(text(&w))
    }

    /// `word.search(/[^e](alt|…)$/)`: the index of the guard character.
    ///
    /// The guard is "any character that is not `e`" — a negated class, so it
    /// matches line terminators too, unlike `.`.
    fn search_non_e_prefixed(w: &[u16], alts: &[&str]) -> Option<usize> {
        let mut best: Option<usize> = None;
        for a in alts {
            let n = slen(a);
            if w.len() > n && ends_with(w, a) && w[w.len() - n - 1] != u('e') {
                let start = w.len() - n - 1;
                if best.is_none_or(|b| start < b) {
                    best = Some(start);
                }
            }
        }
        best
    }
}

impl TokenizeAndStem for PorterStemmerDe {
    const FILTER_ON: Casing = Casing::Raw;
    const STEM_ON: Casing = Casing::Lower;

    fn is_word_char(c: char) -> bool {
        classes::is_word_de(c)
    }

    fn is_stop_word(word: &str) -> bool {
        stopwords::contains(Language::De, word)
    }

    fn gate(token: &str) -> bool {
        token.encode_utf16().any(gate_de)
    }

    fn stem_token(&self, token: &str) -> String {
        self.stem(token).into_owned()
    }
}

impl verbora_core::Stemmer for PorterStemmerDe {
    fn stem<'a>(&self, token: &'a str) -> Cow<'a, str> {
        Self::stem(self, token)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(t: &str) -> String {
        PorterStemmerDe::new().stem(t).into_owned()
    }

    #[test]
    fn documented_vectors() {
        for (input, want) in [
            ("bedürfnissen", "bedurfnis"),
            ("äckern", "ack"),
            ("ackers", "ack"),
            ("armes", "arm"),
            ("derbsten", "derb"),
            ("straße", "strass"),
            ("", ""),
            ("123", "123"),
        ] {
            assert_eq!(s(input), want, "stem({input})");
        }
    }

    #[test]
    fn case_is_preserved_because_nothing_folds() {
        assert_eq!(s("Häuser"), "Haus");
        assert_eq!(s("STRASSE"), "STRASSE");
        assert_eq!(s("FRÖHLICH"), "FRÖHLICH");
        assert_eq!(s("fröhlich"), "frohlich");
    }

    #[test]
    fn umlauts_can_be_kept() {
        let opts = PorterStemmerDeOptions {
            preserve_umlauts: true,
        };
        assert_eq!(
            PorterStemmerDe::new().stem_with("fröhlich", opts),
            "fröhlich"
        );
        assert_eq!(PorterStemmerDe::new().stem_with("äckern", opts), "äck");
    }

    #[test]
    fn unicode_and_edges() {
        assert_eq!(s("a"), "a");
        assert_eq!(s("ab"), "ab");
        assert_eq!(s("😀"), "😀");
        assert_eq!(s("日本語"), "日本語");
    }
}
