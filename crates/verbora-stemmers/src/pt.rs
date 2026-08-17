//! The Portuguese Snowball stemmer, ported from
//! The reference `porter_stemmer_pt`.
//!
//! # Regions are marked once and never remarked
//!
//! `stem` marks `all`, `r1`, `r2` and `rv` immediately after the prelude and then
//! mutates the string through five steps without recomputing any of them. A
//! region is therefore an index into a string that no longer exists, and
//! `hasSuffixInRegion` compares `string.length - suffix.length >= regionStart`
//! against the *current* length. Recomputing regions after each step — which
//! reads as an obvious tidy-up — changes results.
//!
//! # Steps chain rather than alternate
//!
//! Step 1 is nine consecutive `replaceSuffixInRegion` calls, not an `else if`
//! ladder: `"abilidades"` can lose `"idades"` and then have the residue matched
//! again by a later call in the same step. Only steps 2/4 and 3/4 are mutually
//! exclusive, and that choice is made by comparing against the string as it was
//! before step 1.
//!
//! # First listed suffix, not longest
//!
//! `replaceSuffixInRegion` walks its array and stops at the first entry that
//! matches, so every table is hand-ordered longest-first and the order is the
//! algorithm. This is the opposite of the Spanish/French/Dutch `endsinArr`
//! convention, and the two must not share a helper.
//!
//! # The nasal detour
//!
//! `ã` and `õ` are rewritten to `a~`/`o~` before anything else so that a nasal
//! vowel counts as *vowel followed by consonant* during region marking, and back
//! again at the end. Half the suffix table is spelled in the detoured form
//! (`"aço~es"`, `"ara~o"`) for that reason.
//!
//! # Why not [`verbora_core::Token`]
//!
//! The reference is written against `Token`, and this crate re-exports it — but
//! `Token` stores `Vec<char>`, which is divergence **D1** in `docs/PARITY.md`.
//! Region marking here compares positions against the literal constants 1, 2 and
//! 3 and against `string.length`, all of which the reference counts in UTF-16 code
//! units, so this port runs on a `Vec<u16>` instead.

use std::borrow::Cow;

use verbora_tokenizers::classes;

use crate::base::{Casing, TokenizeAndStem};
use crate::stopwords::{self, Language};
use crate::units::{ends_with, replace_suffix, slen, text, units};

/// The Portuguese Snowball stemmer.
///
/// ```
/// use verbora_stemmers::PorterStemmerPt;
/// let s = PorterStemmerPt::new();
/// assert_eq!(s.stem("CASA"), "cas");
/// assert_eq!(s.stem("coração"), "coraçã");
/// assert_eq!(s.stem("abilidades"), "abil");
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PorterStemmerPt;

/// `'aeiouáéíóúâêôàãõ'` — the vowel set `usingVowels` installs.
///
/// `ã` and `õ` are unreachable after the prelude has rewritten them, and are kept
/// because the reference keeps them.
#[inline]
fn is_vowel(c: u16) -> bool {
    matches!(
        c,
        0x61 | 0x65 | 0x69 | 0x6F | 0x75          // a e i o u
        | 0xE1 | 0xE9 | 0xED | 0xF3 | 0xFA        // á é í ó ú
        | 0xE2 | 0xEA | 0xF4                      // â ê ô
        | 0xE0 | 0xE3 | 0xF5
    ) // à ã õ
}

/// The mutable token the reference's `Token` models, over UTF-16 code units.
struct PtToken {
    w: Vec<u16>,
    r1: usize,
    r2: usize,
    rv: usize,
}

impl PtToken {
    /// `hasVowelAtIndex`. Out of range is the reference's `undefined`, which is not
    /// in the vowel string, so it is false.
    #[inline]
    fn vowel_at(&self, i: usize) -> bool {
        self.w.get(i).copied().is_some_and(is_vowel)
    }

    /// `nextVowelIndex`.
    fn next_vowel(&self, start: usize) -> usize {
        let mut i = if start < self.w.len() {
            start
        } else {
            self.w.len()
        };
        while i < self.w.len() && !self.vowel_at(i) {
            i += 1;
        }
        i
    }

    /// `nextConsonantIndex`.
    fn next_consonant(&self, start: usize) -> usize {
        let mut i = if start < self.w.len() {
            start
        } else {
            self.w.len()
        };
        while i < self.w.len() && self.vowel_at(i) {
            i += 1;
        }
        i
    }

    /// `markRegionN`: the first position after a non-vowel that follows a vowel.
    fn mark_region_n(&self, start: usize) -> usize {
        let length = self.w.len();
        let mut index = start;
        let mut region = length;
        while index + 1 < length && region == length {
            if self.vowel_at(index) && !self.vowel_at(index + 1) {
                region = index + 2;
            }
            index += 1;
        }
        region
    }

    /// `markRegionV`.
    fn mark_region_v(&self) -> usize {
        let mut rv = self.w.len();
        if rv > 3 {
            if !self.vowel_at(1) {
                rv = self.next_vowel(2) + 1;
            } else if self.vowel_at(0) && self.vowel_at(1) {
                rv = self.next_consonant(2) + 1;
            } else {
                rv = 3;
            }
        }
        rv
    }

    /// `hasSuffix`. Note `slice(-0) === slice(0)`: an empty suffix compares the
    /// whole string against `""`, so it matches only the empty token.
    fn has_suffix(&self, suffix: &str) -> bool {
        // `units::ends_with` already carries the `slice(-0) === slice(0)` quirk:
        // an empty suffix matches only the empty token.
        ends_with(&self.w, suffix)
    }

    /// `hasSuffixInRegion`.
    fn has_suffix_in_region(&self, suffix: &str, region: usize) -> bool {
        self.has_suffix(suffix) && self.w.len().saturating_sub(slen(suffix)) >= region
    }

    /// `replaceSuffixInRegion`: the **first** listed suffix that matches wins.
    ///
    /// Returns whether a suffix matched and was replaced. Every table this is
    /// called with pairs a non-empty suffix with a distinct replacement (an
    /// empty string, or a different substring), so "a suffix matched" and
    /// "`self.w` actually changed" coincide for every call site in this file.
    /// That lets callers track "did this step change the word" with a `bool`
    /// set right here at the mutation, instead of cloning the whole word
    /// before the step and comparing it after -- the same fact, without the
    /// allocation.
    fn replace_suffix_in_region(
        &mut self,
        suffixes: &[&str],
        replacement: &str,
        region: usize,
    ) -> bool {
        for s in suffixes {
            if self.has_suffix_in_region(s, region) {
                replace_suffix(&mut self.w, s, replacement);
                return true;
            }
        }
        false
    }
}

/// `replaceAll`, which the reference implements as `split(find).join(replace)`:
/// every non-overlapping occurrence, scanning left to right.
fn replace_all(w: &[u16], find: &[u16], replacement: &[u16]) -> Vec<u16> {
    let mut out = Vec::with_capacity(w.len());
    let mut i = 0;
    while i < w.len() {
        if w[i..].starts_with(find) {
            out.extend_from_slice(replacement);
            i += find.len();
        } else {
            out.push(w[i]);
            i += 1;
        }
    }
    out
}

impl PorterStemmerPt {
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
    pub fn stem<'a>(&self, word: &'a str) -> Cow<'a, str> {
        let mut w = units(&word.to_lowercase());

        // --- Prelude: nasal vowels become vowel + '~' ----------------------
        w = replace_all(&w, &[0x00E3], &[0x0061, 0x007E]); // ã -> a~
        w = replace_all(&w, &[0x00F5], &[0x006F, 0x007E]); // õ -> o~

        let mut t = PtToken {
            w,
            r1: 0,
            r2: 0,
            rv: 0,
        };
        t.r1 = t.mark_region_n(0);
        t.r2 = t.mark_region_n(t.r1);
        t.rv = t.mark_region_v();

        // `original`/`before` are `bool` flags here, not owned snapshots of
        // `t.w`: `replace_suffix_in_region` now reports whether it actually
        // replaced a suffix, and (per its doc comment) every table it is
        // called with here only ever fires on a real change. So "did this
        // step change the word" is exactly the OR of the calls it ran,
        // without cloning the whole word to compare it before and after.
        // Protected by the full recorded-fixture parity suite
        // (`tests/parity.rs`), which replays real the reference output through
        // this function -- not just the hand-written vectors below.

        // --- Step 1: standard suffixes (nine chained calls) ----------------
        let step1_changed = standard_suffix(&mut t);

        // --- Step 2: verb suffixes, only if step 1 changed nothing ---------
        let mut changed = step1_changed;
        if !step1_changed {
            changed = t.replace_suffix_in_region(VERB, "", t.rv);
        }

        // --- Step 3 or 4 ---------------------------------------------------
        if !changed {
            t.replace_suffix_in_region(RESIDUAL, "", t.rv);
        } else if t.has_suffix("ci") {
            t.replace_suffix_in_region(&["i"], "", t.rv);
        }

        // --- Step 5: residual form ----------------------------------------
        let mut step5_changed = false;
        if t.has_suffix("gue") || t.has_suffix("gué") || t.has_suffix("guê") {
            step5_changed |= t.replace_suffix_in_region(&["ue", "ué", "uê"], "", t.rv);
        }
        if t.has_suffix("cie") || t.has_suffix("cié") || t.has_suffix("ciê") {
            step5_changed |= t.replace_suffix_in_region(&["ie", "ié", "iê"], "", t.rv);
        }
        if !step5_changed {
            t.replace_suffix_in_region(&["e", "é", "ê"], "", t.rv);
        }
        // Region `all` is 0, so this one is unconditional.
        t.replace_suffix_in_region(&["ç"], "c", 0);

        // --- Postlude ------------------------------------------------------
        let mut w = replace_all(&t.w, &[0x0061, 0x007E], &[0x00E3]);
        w = replace_all(&w, &[0x006F, 0x007E], &[0x00F5]);
        Cow::Owned(text(&w))
    }
}

/// Step 1. Nine `replaceSuffixInRegion` calls in sequence — each can fire.
///
/// Returns whether *any* of the nine changed the word, so the caller can
/// track that fact as a `bool` instead of cloning `t.w` before this runs and
/// comparing it after.
fn standard_suffix(t: &mut PtToken) -> bool {
    let mut changed = t.replace_suffix_in_region(STEP1_NOUN, "", t.r2);
    changed |= t.replace_suffix_in_region(&["logias", "logia"], "log", t.r2);
    // The reference has a commented-out `['uço~es', 'uça~o'] -> 'u'` call here;
    // it is commented out there and absent here.
    changed |= t.replace_suffix_in_region(&["ências", "ência"], "ente", t.r2);
    changed |= t.replace_suffix_in_region(
        &["ativamente", "icamente", "ivamente", "osamente", "adamente"],
        "",
        t.r2,
    );
    changed |= t.replace_suffix_in_region(&["amente"], "", t.r1);
    changed |=
        t.replace_suffix_in_region(&["antemente", "avelmente", "ivelmente", "mente"], "", t.r2);
    changed |= t.replace_suffix_in_region(STEP1_IDADE, "", t.r2);
    changed |= t.replace_suffix_in_region(STEP1_IVO, "", t.r2);
    if t.has_suffix("eiras") || t.has_suffix("eira") {
        changed |= t.replace_suffix_in_region(&["iras", "ira"], "ir", t.rv);
    }
    changed
}

static STEP1_NOUN: &[&str] = &[
    "amentos", "imentos", "aço~es", "adoras", "adores", "amento", "imento", "aça~o", "adora",
    "ância", "antes", "ismos", "istas", "ador", "ante", "ável", "ezas", "icas", "icos", "ismo",
    "ista", "ível", "osas", "osos", "eza", "ica", "ico", "osa", "oso",
];
static STEP1_IDADE: &[&str] = &[
    "abilidades",
    "abilidade",
    "icidades",
    "icidade",
    "ividades",
    "ividade",
    "idades",
    "idade",
];
static STEP1_IVO: &[&str] = &[
    "ativas", "ativos", "ativa", "ativo", "ivas", "ivos", "iva", "ivo",
];
/// The step-2 verb table: 120 suffixes, hand-ordered longest first.
static VERB: &[&str] = &[
    "aríamos", "ássemos", "eríamos", "êssemos", "iríamos", "íssemos", "áramos", "aremos", "aríeis",
    "ásseis", "ávamos", "éramos", "eremos", "eríeis", "ésseis", "íramos", "iremos", "iríeis",
    "ísseis", "ara~o", "ardes", "areis", "áreis", "ariam", "arias", "armos", "assem", "asses",
    "astes", "áveis", "era~o", "erdes", "ereis", "éreis", "eriam", "erias", "ermos", "essem",
    "esses", "estes", "íamos", "ira~o", "irdes", "ireis", "íreis", "iriam", "irias", "irmos",
    "issem", "isses", "istes", "adas", "ados", "amos", "ámos", "ando", "aram", "aras", "arás",
    "arei", "arem", "ares", "aria", "asse", "aste", "avam", "avas", "emos", "endo", "eram", "eras",
    "erás", "erei", "erem", "eres", "eria", "esse", "este", "idas", "idos", "íeis", "imos", "indo",
    "iram", "iras", "irás", "irei", "irem", "ires", "iria", "isse", "iste", "ada", "ado", "ais",
    "ara", "ará", "ava", "eis", "era", "erá", "iam", "ias", "ida", "ido", "ira", "irá", "am", "ar",
    "as", "ei", "em", "er", "es", "eu", "ia", "ir", "is", "iu", "ou",
];
static RESIDUAL: &[&str] = &["os", "a", "i", "o", "á", "í", "ó"];

impl TokenizeAndStem for PorterStemmerPt {
    const FILTER_ON: Casing = Casing::Lower;
    const STEM_ON: Casing = Casing::Raw;

    fn is_word_char(c: char) -> bool {
        classes::is_word_pt(c)
    }

    fn is_stop_word(word: &str) -> bool {
        stopwords::contains(Language::Pt, word)
    }

    fn stem_token(&self, token: &str) -> String {
        self.stem(token).into_owned()
    }
}

impl verbora_core::Stemmer for PorterStemmerPt {
    fn stem<'a>(&self, token: &'a str) -> Cow<'a, str> {
        Self::stem(self, token)
    }
}

impl PorterStemmerPt {
    /// Appends several stop words to the **process-global Portuguese list**.
    ///
    /// `stemmer_pt` declares `addStopWords` twice, and the second declaration
    /// — the concatenating one — wins, so the singular `addStopWord` the first
    /// declaration was meant to provide does not exist. Neither does it here.
    pub fn add_stop_words<I, S>(&self, words: I)
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        stopwords::add_all(Language::Pt, words);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(t: &str) -> String {
        PorterStemmerPt::new().stem(t).into_owned()
    }

    #[test]
    fn documented_vectors() {
        for (input, want) in [
            ("CASA", "cas"),
            ("coração", "coraçã"),
            ("ações", "açõ"),
            ("você", "voc"),
            ("eiras", "eir"),
            ("abilidades", "abil"),
            ("", ""),
        ] {
            assert_eq!(s(input), want, "stem({input})");
        }
    }

    #[test]
    fn the_nasal_detour_round_trips() {
        // `ã` survives when no rule reaches it.
        assert_eq!(s("ã"), "ã");
        assert_eq!(s("õ"), "õ");
        assert_eq!(s("ãõãõ"), "ãõãõ");
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
            ("café", "caf"),
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

    #[test]
    fn cedilla_becomes_c_in_region_all() {
        assert_eq!(s("çç"), "çc");
    }
}
