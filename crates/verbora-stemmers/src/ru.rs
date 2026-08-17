//! The Russian stemmer, ported from
//! The reference `porter_stemmer_ru`.
//!
//! # Regex shapes, and why they are hand-written
//!
//! Every rule is `pattern.test(token)` followed by `token.replace(pattern, repl)`
//! on an anchored alternation. Three shapes appear, and each reduces to a scan:
//!
//! | the reference | What it computes |
//! |---|---|
//! | `/(a\|bb\|ccc)$/ → ''` | delete the **longest** listed suffix |
//! | `/([ая])(ла\|на\|…)$/ → '$1'` | delete the longest listed suffix that is preceded by `а` or `я`, keeping that letter |
//! | `/(н)н/g → '$1'` | collapse every `нн` to `н`, left to right, non-overlapping |
//!
//! The first shape is *longest*, not first-listed, even though the alternation is
//! ordered: the engine takes the earliest start position at which some
//! alternative reaches `$`, and since every alternative is a distinct literal
//! anchored at the end, the earliest start is the longest suffix. Porting the
//! order as if it were first-match — the Italian convention — would give
//! `"важностию"` a different stem.
//!
//! # `||` is falsy, not null-ish
//!
//! `reflexive(RV) || RV` and `superlative(x) || x` fall back when the rule
//! returns the **empty string** as well as when it returns `null`. Stemming
//! `"ся"` therefore returns `"ся"` rather than `""`, and a naive `??` port
//! changes it. Both sites are marked in the code.
//!
//! # `.` does not match a line terminator
//!
//! The region split is `/^(.*?[аеиоюяуыиэ])(.*)$/`. A token containing `\n`,
//! `\r`, U+2028 or U+2029 cannot match it at any offset, so the whole algorithm
//! is skipped and the lowercased, `ё`-folded token is returned unchanged.
//!
//! # One unreachable `TypeError`
//!
//! When R2's tail ends in `ост`/`ость` but the working string does not, the
//! reference assigns `null` to `derivationalResult` and then calls `.replace` on
//! it, which throws. No word in the reference's corpora, and none of 200,000
//! randomised Cyrillic probes, reaches it — the `ь` that `noun` always strips
//! from `ость` leaves `ост` behind, which matches. Rather than panic on input no
//! caller can construct, this port keeps the un-derivationalised string. See
//! `deviations` in the crate's parity report.

use std::borrow::Cow;

use verbora_tokenizers::classes;

use crate::base::{Casing, TokenizeAndStem};
use crate::data::gates::gate_ru;
use crate::stopwords::{self, Language};
use crate::units::{ends_with, slen, text, units};

/// The Russian stemmer.
///
/// ```
/// use verbora_stemmers::PorterStemmerRu;
/// let s = PorterStemmerRu::new();
/// assert_eq!(s.stem("важнейшими"), "важн");
/// assert_eq!(s.stem("ёлка"), "елк");
/// assert_eq!(s.stem("ВАЖНАЯ"), "важн");
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PorterStemmerRu;

/// `[аеиоюяуыиэ]` — the class the region split uses (`и` appears twice in it).
#[inline]
fn is_vowel(c: u16) -> bool {
    matches!(
        c,
        0x0430 | 0x0435 | 0x0438 | 0x043E | 0x044E | 0x044F | 0x0443 | 0x044B | 0x044D
    )
}

// ---------------------------------------------------------------------------
// Shared scanners, also used by the Ukrainian stemmer
// ---------------------------------------------------------------------------

/// The reference's line terminators, the four code points `.` refuses to match.
#[inline]
pub(crate) fn is_line_terminator(c: u16) -> bool {
    matches!(c, 0x000A | 0x000D | 0x2028 | 0x2029)
}

/// `/(a|bb|ccc)$/`: the start index of the longest listed suffix of `w`.
pub(crate) fn alt_suffix(w: &[u16], alts: &[&str]) -> Option<usize> {
    let mut best: Option<usize> = None;
    for a in alts {
        if ends_with(w, a) {
            let start = w.len() - slen(a);
            if best.is_none_or(|b| start < b) {
                best = Some(start);
            }
        }
    }
    best
}

/// `/([ая])(ла|на|…)$/`: the index just after the kept `[ая]`.
///
/// Returns where to truncate so that the captured letter survives and the listed
/// suffix does not — which is exactly what replacing with `'$1'` does.
fn alt_suffix_after(w: &[u16], keep: &[u16], alts: &[&str]) -> Option<usize> {
    let mut best: Option<usize> = None;
    for a in alts {
        let n = slen(a);
        if n < w.len() && ends_with(w, a) && keep.contains(&w[w.len() - n - 1]) {
            let start = w.len() - n;
            if best.is_none_or(|b| start < b) {
                best = Some(start);
            }
        }
    }
    best
}

/// `/[ая]в(ши|шись)$/`: the start index of the whole match.
pub(crate) fn av_shi(w: &[u16]) -> Option<usize> {
    let mut best: Option<usize> = None;
    for a in ["ши", "шись"] {
        let n = slen(a);
        if n + 2 <= w.len()
            && ends_with(w, a)
            && w[w.len() - n - 1] == 0x0432 // в
            && matches!(w[w.len() - n - 2], 0x0430 | 0x044F)
        {
            let start = w.len() - n - 2;
            if best.is_none_or(|b| start < b) {
                best = Some(start);
            }
        }
    }
    best
}

/// `/(н)н/g → '$1'`: collapse doubled `н`, non-overlapping, left to right.
pub(crate) fn collapse_double(w: &[u16], letter: u16) -> Vec<u16> {
    let mut out = Vec::with_capacity(w.len());
    let mut i = 0;
    while i < w.len() {
        if w[i] == letter && w.get(i + 1) == Some(&letter) {
            out.push(letter);
            i += 2;
        } else {
            out.push(w[i]);
            i += 1;
        }
    }
    out
}

/// `/^(.*?[V])(.*)$/`: `(head, tail)`, or `None` when the pattern cannot match.
pub(crate) fn split_at_first_vowel(w: &[u16], vowel: fn(u16) -> bool) -> Option<(usize, usize)> {
    if w.iter().copied().any(is_line_terminator) {
        // `.` cannot cross a line terminator, and neither `.*?` nor `.*` may skip
        // one, so no offset can produce a match.
        return None;
    }
    let i = w.iter().position(|&c| vowel(c))?;
    Some((i + 1, i + 1))
}

/// Removes the last code unit when it is `c`; `/c$/` with an empty replacement.
pub(crate) fn strip_final(w: &mut Vec<u16>, c: u16) {
    if w.last() == Some(&c) {
        w.pop();
    }
}

/// The reference's `x || fallback` for a rule result: the empty string is falsy.
pub(crate) fn or_falsy(value: Option<Vec<u16>>, fallback: &[u16]) -> Vec<u16> {
    match value {
        Some(v) if !v.is_empty() => v,
        _ => fallback.to_vec(),
    }
}

// ---------------------------------------------------------------------------
// The rule groups
// ---------------------------------------------------------------------------

fn perfective_gerund(w: &[u16]) -> Option<Vec<u16>> {
    if let Some(at) = av_shi(w) {
        return Some(w[..at].to_vec());
    }
    alt_suffix(w, &["ив", "ивши", "ившись", "ывши", "ывшись", "ыв"]).map(|at| w[..at].to_vec())
}

fn adjective(w: &[u16]) -> Option<Vec<u16>> {
    alt_suffix(w, ADJECTIVE).map(|at| w[..at].to_vec())
}

fn participle(w: &[u16]) -> Option<Vec<u16>> {
    if let Some(at) = alt_suffix_after(w, &[0x0430, 0x044F], &["ем", "нн", "вш", "ющ", "щ"])
    {
        return Some(w[..at].to_vec());
    }
    alt_suffix(w, &["ивш", "ывш", "ующ"]).map(|at| w[..at].to_vec())
}

fn adjectival(w: &[u16]) -> Option<Vec<u16>> {
    let result = adjective(w)?;
    // `result = pariticipleResult || result` — falsy fallback again.
    Some(or_falsy(participle(&result), &result))
}

fn reflexive(w: &[u16]) -> Option<Vec<u16>> {
    alt_suffix(w, &["ся", "сь"]).map(|at| w[..at].to_vec())
}

fn verb(w: &[u16]) -> Option<Vec<u16>> {
    if let Some(at) = alt_suffix_after(w, &[0x0430, 0x044F], VERB1) {
        return Some(w[..at].to_vec());
    }
    alt_suffix(w, VERB2).map(|at| w[..at].to_vec())
}

fn noun(w: &[u16]) -> Option<Vec<u16>> {
    alt_suffix(w, NOUN).map(|at| w[..at].to_vec())
}

fn superlative(w: &[u16]) -> Option<Vec<u16>> {
    alt_suffix(w, &["ейш", "ейше"]).map(|at| w[..at].to_vec())
}

fn derivational(w: &[u16]) -> Option<Vec<u16>> {
    alt_suffix(w, &["ост", "ость"]).map(|at| w[..at].to_vec())
}

static ADJECTIVE: &[&str] = &[
    "ее", "ие", "ые", "ое", "ими", "ыми", "ей", "ий", "ый", "ой", "ем", "им", "ым", "ом", "его",
    "ого", "ему", "ому", "их", "ых", "ую", "юю", "ая", "яя", "ою", "ею",
];
static VERB1: &[&str] = &[
    "ла", "на", "ете", "йте", "ли", "й", "л", "ем", "н", "ло", "но", "ет", "ют", "ны", "ть", "ешь",
    "нно",
];
static VERB2: &[&str] = &[
    "ила", "ыла", "ена", "ейте", "уйте", "ите", "или", "ыли", "ей", "уй", "ил", "ыл", "им", "ым",
    "ен", "ило", "ыло", "ено", "ят", "ует", "ит", "ыт", "ены", "ить", "ыть", "ишь", "ую", "ю",
];
static NOUN: &[&str] = &[
    "а", "ев", "ов", "ие", "ье", "е", "иями", "ями", "ами", "еи", "ии", "и", "ией", "ей", "ой",
    "ий", "й", "иям", "ям", "ием", "ем", "ам", "ом", "о", "у", "ах", "иях", "ях", "ы", "ь", "ию",
    "ью", "ю", "ия", "ья", "я",
];

impl PorterStemmerRu {
    /// Creates the stemmer. It is stateless and zero-sized.
    #[inline]
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Stems one token.
    #[allow(
        clippy::unused_self,
        reason = "mirrors the reference's method-shaped API"
    )]
    #[must_use]
    pub fn stem<'a>(&self, token: &'a str) -> Cow<'a, str> {
        // `.toLowerCase().replace(/ё/g, 'е')` — global, so every ё folds.
        let mut t = units(&token.to_lowercase());
        for c in &mut t {
            if *c == 0x0451 {
                *c = 0x0435;
            }
        }

        let Some((head_end, rv_start)) = split_at_first_vowel(&t, is_vowel) else {
            return Cow::Owned(text(&t));
        };
        let head = &t[..head_end];
        let rv = &t[rv_start..];

        // R2 is the same split applied to RV; only its tail is ever read.
        let r2_tail = split_at_first_vowel(rv, is_vowel).map(|(_, s)| &rv[s..]);

        let mut result = match perfective_gerund(rv) {
            Some(r) => r,
            None => {
                let reflexed = or_falsy(reflexive(rv), rv);
                adjectival(&reflexed)
                    .or_else(|| verb(&reflexed))
                    .or_else(|| noun(&reflexed))
                    .unwrap_or(reflexed)
            }
        };
        strip_final(&mut result, 0x0438); // /и$/

        // `result` is not read again after this, so it can move into whichever
        // branch is taken instead of being cloned up front.
        //
        // This guard only needs the fact of whether `derivational` would match
        // `tail`, not the truncated word it would build — `derivational(tail)`
        // discarded its `Vec<u16>` on every call here, purely for `.is_some()`.
        // `alt_suffix(w, alts).is_some() == alt_suffix(w, alts).map(f).is_some()`
        // for any `f`, so calling the non-allocating `alt_suffix` scan directly
        // (the same one `derivational` itself calls) is provably equivalent and
        // skips that allocation.
        let derived = if r2_tail
            .is_some_and(|tail| !tail.is_empty() && alt_suffix(tail, &["ост", "ость"]).is_some())
        {
            // The reference throws here when this second call returns null; see the
            // module note. Falling back to `result` is the documented divergence.
            derivational(&result).unwrap_or(result)
        } else {
            result
        };

        let mut out = or_falsy(superlative(&derived), &derived);
        out = collapse_double(&out, 0x043D); // /(н)н/g
        strip_final(&mut out, 0x044C); // /ь$/

        let mut full = head.to_vec();
        full.extend_from_slice(&out);
        Cow::Owned(text(&full))
    }
}

impl TokenizeAndStem for PorterStemmerRu {
    const FILTER_ON: Casing = Casing::Raw;
    const STEM_ON: Casing = Casing::Lower;

    fn is_word_char(c: char) -> bool {
        classes::is_word_ru(c)
    }

    fn is_stop_word(word: &str) -> bool {
        stopwords::contains(Language::Ru, word)
    }

    fn gate(token: &str) -> bool {
        token.encode_utf16().any(gate_ru)
    }

    fn stem_token(&self, token: &str) -> String {
        self.stem(token).into_owned()
    }
}

impl verbora_core::Stemmer for PorterStemmerRu {
    fn stem<'a>(&self, token: &'a str) -> Cow<'a, str> {
        Self::stem(self, token)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(t: &str) -> String {
        PorterStemmerRu::new().stem(t).into_owned()
    }

    #[test]
    fn documented_vectors() {
        for (input, want) in [
            ("важнейшими", "важн"),
            ("важностию", "важност"),
            ("валандался", "валанда"),
            ("вагоном", "вагон"),
            ("ёлка", "елк"),
            ("ВАЖНАЯ", "важн"),
            ("", ""),
        ] {
            assert_eq!(s(input), want, "stem({input})");
        }
    }

    #[test]
    fn a_line_terminator_disables_the_whole_algorithm() {
        assert_eq!(s("аб\nв"), "аб\nв");
    }

    #[test]
    fn doubled_n_collapses_left_to_right() {
        assert_eq!(collapse_double(&units("нннн"), 0x043D), units("нн"));
        // The word itself is vowel-free, so the split fails and it is returned
        // untouched — the collapse never runs.
        assert_eq!(s("нннн"), "нннн");
        assert_eq!(collapse_double(&units("ннн"), 0x043D), units("нн"));
        assert_eq!(collapse_double(&units("н"), 0x043D), units("н"));
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
            ("мама", "мам"),
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

    #[test]
    fn a_vowelless_word_is_returned_untouched() {
        assert_eq!(s("бвг"), "бвг");
    }
}
