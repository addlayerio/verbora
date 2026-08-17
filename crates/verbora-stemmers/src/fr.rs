//! The French Snowball stemmer, ported from
//! The reference `porter_stemmer_fr`.
//!
//! # `stem("")` is the nine-character string `"undefined"`
//!
//! `prelude` handles index 0 specially and unconditionally does `result +=
//! token[0]`. For an empty token that is `undefined`, and the reference stringifies
//! it during concatenation. The literal `"undefined"` then flows through the
//! whole algorithm — nine characters, regions and all — and comes back out. Every
//! other stemmer in the family returns `""` for `""`.
//!
//! # RV's two independent `if`s
//!
//! ```text
//! if (isVowel(t[0]) && isVowel(t[1])) rv = 3
//! if (three === 'par' || 'col' || 'tap') rv = 3
//! else { for (i = 1; i < len - 1 && rv === len; i++) if (isVowel(t[i])) rv = i + 1 }
//! ```
//!
//! The second `if` is **not** an `else if`. When the word starts with two vowels
//! the first assignment normally wins, because the fallback loop is guarded by
//! `rv === len`. But for a three-letter word `rv = 3` *is* `len`, so the guard
//! still holds and the loop overwrites it — which is why `regions("tue").rv` is 2
//! while `regions("aimer").rv` is 3.
//!
//! # `endsinArr` accepts a string where an array is expected
//!
//! Step 4 calls `endsinArr(rvtxt, 'e')` and `endsinArr(rvtxt, 'ë')`. The reference
//! indexes the string, so `'le'` would be tried as `['l', 'e']`, not as the
//! two-character suffix. Both shipped call sites pass one character, so the
//! behaviour coincides — but a port that reads them as whole suffixes has
//! changed the algorithm.

use std::borrow::Cow;

use verbora_tokenizers::classes;

use crate::base::{Casing, TokenizeAndStem};
use crate::data::gates::gate_fr;
use crate::stopwords::{self, Language};
use crate::units::{at, ends_with, longest_suffix, slen, text, u, units};

/// The French Snowball stemmer.
///
/// ```
/// use verbora_stemmers::PorterStemmerFr;
/// let s = PorterStemmerFr::new();
/// assert_eq!(s.stem("volerait"), "vol");
/// assert_eq!(s.stem("publicité"), "publiqu");
/// assert_eq!(s.stem(""), "undefined"); // yes, really — see the module docs
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PorterStemmerFr;

/// The R1, R2 and RV offsets a French word is divided into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Regions {
    /// Start of R1.
    pub r1: usize,
    /// Start of R2.
    pub r2: usize,
    /// Start of RV.
    pub rv: usize,
}

/// The seventeen-way vowel test, transcribed exactly.
///
/// There is no `á í ó ú ü œ`, and no uppercase form — which is precisely how the
/// `U`, `I` and `Y` that `prelude` writes stop counting as vowels.
#[inline]
fn is_vowel(c: u16) -> bool {
    matches!(
        c,
        0x61 | 0x65 | 0x69 | 0x6F | 0x75 | 0x79 // a e i o u y
        | 0xE2 | 0xE0 | 0xEB | 0xE9 | 0xEA | 0xE8 // â à ë é ê è
        | 0xEF | 0xEE | 0xF4 | 0xFB | 0xF9 // ï î ô û ù
    )
}

#[inline]
fn vowel_at(w: &[u16], i: usize) -> bool {
    at(w, i).is_some_and(is_vowel)
}

#[inline]
fn from(w: &[u16], at: usize) -> &[u16] {
    &w[at.min(w.len())..]
}

/// Removes `n` code units from the end and appends `tail`.
fn cut(w: &mut Vec<u16>, n: usize, tail: &str) {
    w.truncate(w.len().saturating_sub(n));
    w.extend(tail.encode_utf16());
}

/// `endsin(token, letterBefore + suffix)` without building a temporary string.
fn ends_with_prefixed(w: &[u16], before: u16, suffix: &str) -> bool {
    let n = slen(suffix);
    w.len() > n && w[w.len() - n - 1] == before && ends_with(w, suffix)
}

impl PorterStemmerFr {
    /// Creates the stemmer. It is stateless and zero-sized.
    #[inline]
    pub const fn new() -> Self {
        Self
    }

    /// `PorterStemmerFr.prelude`, exported by the reference for tests.
    pub fn prelude(token: &str) -> String {
        text(&Self::prelude_units(&units(&token.to_lowercase())))
    }

    fn prelude_units(t: &[u16]) -> Vec<u16> {
        if t.is_empty() {
            // `result += token[0]` with `token[0] === undefined`.
            return units("undefined");
        }
        let mut out = Vec::with_capacity(t.len());
        out.push(if t[0] == u('y') && vowel_at(t, 1) {
            u('Y')
        } else {
            t[0]
        });
        for i in 1..t.len() {
            // Every test reads the ORIGINAL token, never the partly-rewritten
            // result, so an already-marked neighbour still counts as a vowel.
            let c = t[i];
            if (c == u('u') || c == u('i')) && vowel_at(t, i - 1) && vowel_at(t, i + 1) {
                out.push(c - 32); // ASCII uppercase
            } else if c == u('y') && (vowel_at(t, i - 1) || vowel_at(t, i + 1)) {
                out.push(u('Y'));
            } else if c == u('u') && t[i - 1] == u('q') {
                out.push(u('U'));
            } else {
                out.push(c);
            }
        }
        out
    }

    /// `PorterStemmerFr.regions`, exported by the reference for tests.
    pub fn regions(token: &str) -> Regions {
        Self::regions_units(&units(token))
    }

    fn regions_units(t: &[u16]) -> Regions {
        let len = t.len();
        let (mut r1, mut r2, mut rv) = (len, len, len);
        for i in 0..len.saturating_sub(1) {
            if r1 != len {
                break;
            }
            if is_vowel(t[i]) && !is_vowel(t[i + 1]) {
                r1 = i + 2;
            }
        }
        for i in r1..len.saturating_sub(1) {
            if r2 != len {
                break;
            }
            if is_vowel(t[i]) && !is_vowel(t[i + 1]) {
                r2 = i + 2;
            }
        }
        if vowel_at(t, 0) && vowel_at(t, 1) {
            rv = 3;
        }
        let three = &t[..3.min(len)];
        if ends_with(three, "par") || ends_with(three, "col") || ends_with(three, "tap") {
            rv = 3;
        } else {
            for (i, &c) in t.iter().enumerate().take(len.saturating_sub(1)).skip(1) {
                if rv != len {
                    break;
                }
                if is_vowel(c) {
                    rv = i + 1;
                }
            }
        }
        Regions { r1, r2, rv }
    }

    /// `PorterStemmerFr.endsinArr` — the **longest** matching suffix, or `""`.
    pub fn endsin_arr<'s>(token: &str, suffixes: &[&'s str]) -> &'s str {
        longest_suffix(&units(token), suffixes).unwrap_or("")
    }

    /// Stems one token.
    #[allow(
        clippy::unused_self,
        reason = "mirrors the reference's method-shaped API"
    )]
    #[expect(
        clippy::too_many_lines,
        reason = "the else-if chains ARE the specification; splitting them hides the order"
    )]
    #[expect(
        unused_assignments,
        reason = "the reference refreshes r1/r2/rv after every cut, and this port \
                  mirrors that uniformly. Several of those refreshes are dead — the \
                  branch they sit in is the last one that can fire — but dropping \
                  only the dead ones would make the surviving refreshes look \
                  deliberate rather than mechanical, and a future rule reordering \
                  would then silently read a stale region."
    )]
    pub fn stem<'a>(&self, token: &'a str) -> Cow<'a, str> {
        let mut t = Self::prelude_units(&units(&token.to_lowercase()));
        if t.len() == 1 {
            return Cow::Owned(text(&t));
        }
        let regs = Self::regions_units(&t);
        // `r1txt` is a borrowed slice of `t`, not an owned snapshot:
        // `longest_suffix` returns `Option<&'s str>` borrowed from the
        // *suffix list* argument, never from `word`, so the borrow of `t`
        // needed to call it ends the moment the call returns -- well before
        // any branch below mutates `t` via `cut`. r1txt is read only inside
        // Step 1's single if-else-if chain (mutually exclusive branches) and
        // is never read again afterwards, so no read of it needs to survive
        // past a `cut` call on a reachable path.
        //
        // `r2txt` and `rvtxt` stay owned `Vec<u16>` snapshots, unlike
        // r1txt, because both are read again *after* Step 1's chain ends --
        // in Step 2a, Step 2b and Step 4, which are separate, sequential `if`
        // statements rather than mutually-exclusive branches of one chain.
        // The borrow checker must assume every one of those blocks can run
        // in sequence (even though at the value level at most one actually
        // mutates `t` before the next read), so a borrow taken once and
        // reused across that sequence does not typecheck: `cut(&mut t, ...)`
        // in an earlier block conflicts with a later block's read of the
        // same borrow. (Verified by attempting exactly this conversion for
        // r2txt: it fails to compile with E0502 at the Step 2b "ions" read,
        // because Step 2a's `cut` sits between the borrow's creation after
        // Step 1 and that read.) `rvtxt` additionally has a same-branch case
        // that rules it out on its own: step 2b's `ment` (STEP2B_A) branch
        // reads the PRE-mutation `rvtxt` *after* calling `cut(&mut t, ...)`
        // in that same branch -- see the comment at that call site.
        let mut r1txt = from(&t, regs.r1);
        let mut r2txt = from(&t, regs.r2).to_vec();
        let mut rvtxt = from(&t, regs.rv).to_vec();
        let before_step1 = t.clone();
        let mut do_step2a = false;

        // --- Step 1 --------------------------------------------------------
        if let Some(s) = longest_suffix(&r2txt, STEP1_ANCE) {
            cut(&mut t, slen(s), "");
        } else if let Some(s) = longest_suffix(&t, ICATRICE) {
            if longest_suffix(&r2txt, ICATRICE).is_some() {
                cut(&mut t, slen(s), "");
            } else {
                cut(&mut t, slen(s), "iqU");
            }
        } else if let Some(s) = longest_suffix(&r2txt, ATRICE) {
            cut(&mut t, slen(s), "");
        } else if let Some(s) = longest_suffix(&r2txt, &["logie", "logies"]) {
            cut(&mut t, slen(s), "log");
        } else if let Some(s) = longest_suffix(&r2txt, &["usion", "ution", "usions", "utions"]) {
            cut(&mut t, slen(s), "u");
        } else if let Some(s) = longest_suffix(&r2txt, &["ence", "ences"]) {
            cut(&mut t, slen(s), "ent");
        } else if let Some(s) = longest_suffix(r1txt, &["issement", "issements"]) {
            let n = slen(s);
            // The branch is taken even when the vowel test fails, so no later
            // rule can fire — a plain `if` inside an `else if` chain.
            if !(t.len() > n && is_vowel(t[t.len() - n - 1])) {
                cut(&mut t, n, "");
                r1txt = from(&t, regs.r1);
                r2txt = from(&t, regs.r2).to_vec();
                rvtxt = from(&t, regs.rv).to_vec();
            }
        } else if let Some(s) = longest_suffix(&r2txt, &["ativement", "ativements"]) {
            cut(&mut t, slen(s), "");
        } else if let Some(s) = longest_suffix(&r2txt, &["ivement", "ivements"]) {
            cut(&mut t, slen(s), "");
        } else if longest_suffix(&t, &["eusement", "eusements"]).is_some() {
            if let Some(s) = longest_suffix(&r2txt, &["eusement", "eusements"]) {
                cut(&mut t, slen(s), "");
            } else if let Some(s) = longest_suffix(r1txt, &["eusement", "eusements"]) {
                cut(&mut t, slen(s), "eux");
            } else if let Some(s) = longest_suffix(&rvtxt, &["ement", "ements"]) {
                cut(&mut t, slen(s), "");
            }
        } else if let Some(s) =
            longest_suffix(&r2txt, &["ablement", "ablements", "iqUement", "iqUements"])
        {
            cut(&mut t, slen(s), "");
        } else if let Some(s) =
            longest_suffix(&rvtxt, &["ièrement", "ièrements", "Ièrement", "Ièrements"])
        {
            cut(&mut t, slen(s), "i");
        } else if let Some(s) = longest_suffix(&rvtxt, &["ement", "ements"]) {
            cut(&mut t, slen(s), "");
        } else if let Some(s) = longest_suffix(&t, &["icité", "icités"]) {
            if longest_suffix(&r2txt, &["icité", "icités"]).is_some() {
                cut(&mut t, slen(s), "");
            } else {
                cut(&mut t, slen(s), "iqU");
            }
        } else if let Some(s) = longest_suffix(&t, &["abilité", "abilités"]) {
            if longest_suffix(&r2txt, &["abilité", "abilités"]).is_some() {
                cut(&mut t, slen(s), "");
            } else {
                cut(&mut t, slen(s), "abl");
            }
        } else if let Some(s) = longest_suffix(&r2txt, &["ité", "ités"]) {
            cut(&mut t, slen(s), "");
        } else if longest_suffix(&t, ICATIF).is_some() {
            // Two consecutive `if`s, not an if/else — both can fire.
            if let Some(s) = longest_suffix(&r2txt, ICATIF) {
                cut(&mut t, slen(s), "");
                r2txt = from(&t, regs.r2).to_vec();
                rvtxt = from(&t, regs.rv).to_vec();
            }
            if let Some(s) = longest_suffix(&r2txt, ATIF) {
                cut(&mut t, slen(s) + 2, "iqU");
                r2txt = from(&t, regs.r2).to_vec();
                rvtxt = from(&t, regs.rv).to_vec();
            }
        } else if let Some(s) = longest_suffix(&r2txt, ATIF) {
            cut(&mut t, slen(s), "");
        } else if let Some(s) = longest_suffix(&r2txt, &["if", "ive", "ifs", "ives"]) {
            cut(&mut t, slen(s), "");
        } else if let Some(s) = longest_suffix(&t, &["eaux"]) {
            cut(&mut t, slen(s), "eau");
        } else if let Some(s) = longest_suffix(r1txt, &["aux"]) {
            cut(&mut t, slen(s), "al");
        } else if let Some(s) = longest_suffix(&r2txt, &["euse", "euses"]) {
            cut(&mut t, slen(s), "");
        } else if let Some(s) = longest_suffix(r1txt, &["euse", "euses"]) {
            cut(&mut t, slen(s), "eux");
        } else if let Some(s) = longest_suffix(&rvtxt, &["amment"]) {
            cut(&mut t, slen(s), "ant");
            do_step2a = true;
        } else if let Some(s) = longest_suffix(&rvtxt, &["emment"]) {
            cut(&mut t, slen(s), "ent");
            do_step2a = true;
        } else if let Some(s) = longest_suffix(&rvtxt, &["ment", "ments"]) {
            let n = slen(s);
            let before = if t.len() > n {
                Some(t[t.len() - n - 1])
            } else {
                None
            };
            if let Some(b) = before
                && is_vowel(b)
                && ends_with_prefixed(&rvtxt, b, s)
            {
                cut(&mut t, n, "");
                do_step2a = true;
            }
        }

        // R1 is refreshed here in the reference too, but nothing reads it again.
        r2txt = from(&t, regs.r2).to_vec();
        rvtxt = from(&t, regs.rv).to_vec();

        // --- Step 2a -------------------------------------------------------
        // `before_step2a == t` in the reference's own shape is "did step 2a
        // change anything" -- step 2a has exactly one mutating call site
        // (the `cut` below), so a `bool` flag set right there is the same
        // fact without cloning the whole word to compare it.
        let mut step2a_done = false;
        let mut step2a_changed = false;
        if before_step1 == t || do_step2a {
            step2a_done = true;
            if let Some(s) = longest_suffix(&rvtxt, STEP2A) {
                let n = slen(s);
                if t.len() > n {
                    let b = t[t.len() - n - 1];
                    if !is_vowel(b) && ends_with_prefixed(&rvtxt, b, s) {
                        cut(&mut t, n, "");
                        step2a_changed = true;
                    }
                }
            }
        }

        // --- Step 2b -------------------------------------------------------
        if step2a_done && !step2a_changed {
            if let Some(s) = longest_suffix(&rvtxt, STEP2B_E) {
                cut(&mut t, slen(s), "");
                r2txt = from(&t, regs.r2).to_vec();
                rvtxt = from(&t, regs.rv).to_vec();
            } else if longest_suffix(&rvtxt, &["ions"]).is_some()
                && longest_suffix(&r2txt, &["ions"]).is_some()
            {
                cut(&mut t, 4, "");
                r2txt = from(&t, regs.r2).to_vec();
                rvtxt = from(&t, regs.rv).to_vec();
            } else if let Some(s) = longest_suffix(&rvtxt, STEP2B_A) {
                cut(&mut t, slen(s), "");
                // `rvtxt` here is still the pre-deletion slice, deliberately.
                if t.last() == Some(&u('e')) && ends_with_prefixed(&rvtxt, u('e'), s) {
                    t.truncate(t.len() - 1);
                }
                r2txt = from(&t, regs.r2).to_vec();
                rvtxt = from(&t, regs.rv).to_vec();
            }
        }

        if t != before_step1 {
            // --- Step 3 ----------------------------------------------------
            if t.last() == Some(&u('Y')) {
                let n = t.len();
                t[n - 1] = u('i');
            }
            if t.last() == Some(&u('ç')) {
                let n = t.len();
                t[n - 1] = u('c');
            }
        } else {
            // --- Step 4: residual ------------------------------------------
            let last = t.last().copied();
            let second_last = if t.len() >= 2 {
                Some(t[t.len() - 2])
            } else {
                None
            };
            if last == Some(u('s'))
                && !second_last.is_some_and(|c| {
                    matches!(c, x if x == u('a') || x == u('i') || x == u('o') || x == u('u') || x == u('è') || x == u('s'))
                })
            {
                t.truncate(t.len() - 1);
                r2txt = from(&t, regs.r2).to_vec();
                rvtxt = from(&t, regs.rv).to_vec();
            }
            if longest_suffix(&r2txt, &["ion"]).is_some() {
                let before = if t.len() >= 4 {
                    Some(t[t.len() - 4])
                } else {
                    None
                };
                if before.is_some_and(|c| c == u('s') || c == u('t')) {
                    cut(&mut t, 3, "");
                    r2txt = from(&t, regs.r2).to_vec();
                    rvtxt = from(&t, regs.rv).to_vec();
                }
            }
            if let Some(s) = longest_suffix(&rvtxt, &["ier", "ière", "Ier", "Ière"]) {
                cut(&mut t, slen(s), "i");
                r2txt = from(&t, regs.r2).to_vec();
                rvtxt = from(&t, regs.rv).to_vec();
            }
            // `endsinArr(rvtxt, 'e')`: a STRING, iterated as its characters.
            if ends_with(&rvtxt, "e") {
                t.truncate(t.len().saturating_sub(1));
                rvtxt = from(&t, regs.rv).to_vec();
            }
            if ends_with(&rvtxt, "ë") {
                // `token.slice(token.length - 3, -1)`: a start argument below
                // zero is treated as an offset from the end, not clamped, so for
                // a token shorter than three units this can never be "gu".
                if t.len() >= 3 && ends_with(&t[t.len() - 3..t.len() - 1], "gu") {
                    t.truncate(t.len() - 1);
                }
            }
        }

        // --- Step 5: undouble ----------------------------------------------
        if longest_suffix(&t, &["enn", "onn", "ett", "ell", "eill"]).is_some() {
            t.truncate(t.len() - 1);
        }

        // --- Step 6: un-accent the final é/è -------------------------------
        let mut i = t.len().saturating_sub(1);
        while i > 0 {
            if !is_vowel(t[i]) {
                i -= 1;
            } else if i != t.len() - 1 && (t[i] == u('é') || t[i] == u('è')) {
                t[i] = u('e');
                break;
            } else {
                break;
            }
        }

        Cow::Owned(text(&t).to_lowercase())
    }
}

static STEP1_ANCE: &[&str] = &[
    "ance", "iqUe", "isme", "able", "iste", "eux", "ances", "iqUes", "ismes", "ables", "istes",
];
static ICATRICE: &[&str] = &[
    "icatrice",
    "icateur",
    "ication",
    "icatrices",
    "icateurs",
    "ications",
];
static ATRICE: &[&str] = &["atrice", "ateur", "ation", "atrices", "ateurs", "ations"];
static ICATIF: &[&str] = &["icatif", "icative", "icatifs", "icatives"];
static ATIF: &[&str] = &["atif", "ative", "atifs", "atives"];
static STEP2A: &[&str] = &[
    "îmes", "ît", "îtes", "i", "ie", "Ie", "ies", "ir", "ira", "irai", "iraIent", "irais", "irait",
    "iras", "irent", "irez", "iriez", "irions", "irons", "iront", "is", "issaIent", "issais",
    "issait", "issant", "issante", "issantes", "issants", "isse", "issent", "isses", "issez",
    "issiez", "issions", "issons", "it",
];
static STEP2B_E: &[&str] = &[
    "é", "ée", "ées", "és", "èrent", "er", "era", "erai", "eraIent", "erais", "erait", "eras",
    "erez", "eriez", "erions", "erons", "eront", "ez", "iez", "Iez",
];
static STEP2B_A: &[&str] = &[
    "âmes", "ât", "âtes", "a", "ai", "aIent", "ais", "ait", "ant", "ante", "antes", "ants", "as",
    "asse", "assent", "asses", "assiez", "assions",
];

impl TokenizeAndStem for PorterStemmerFr {
    // French is the only language that lowercases BEFORE the stop-word test.
    const FILTER_ON: Casing = Casing::Lower;
    const STEM_ON: Casing = Casing::Lower;

    fn is_word_char(c: char) -> bool {
        classes::is_word_fr(c)
    }

    fn is_stop_word(word: &str) -> bool {
        stopwords::contains(Language::Fr, word)
    }

    fn gate(token: &str) -> bool {
        token.encode_utf16().any(gate_fr)
    }

    fn stem_token(&self, token: &str) -> String {
        self.stem(token).into_owned()
    }
}

impl verbora_core::Stemmer for PorterStemmerFr {
    fn stem<'a>(&self, token: &'a str) -> Cow<'a, str> {
        Self::stem(self, token)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(t: &str) -> String {
        PorterStemmerFr::new().stem(t).into_owned()
    }

    #[test]
    fn documented_vectors() {
        for (input, want) in [
            ("volera", "vol"),
            ("volerait", "vol"),
            ("subitement", "subit"),
            ("tempérament", "temper"),
            ("voudriez", "voudr"),
            ("vengeait", "veng"),
            ("saisissement", "sais"),
            ("transatlantique", "transatlant"),
            ("premièrement", "premi"),
            ("instruments", "instrument"),
            ("trouverions", "trouv"),
            ("voyiez", "voi"),
            ("publicité", "publiqu"),
            ("pitoyable", "pitoi"),
            ("ÊTRE", "être"),
        ] {
            assert_eq!(s(input), want, "stem({input})");
        }
    }

    #[test]
    fn empty_input_stems_to_the_word_undefined() {
        assert_eq!(s(""), "undefined");
        assert_eq!(PorterStemmerFr::prelude(""), "undefined");
    }

    #[test]
    fn prelude_marks_semivowels() {
        assert_eq!(PorterStemmerFr::prelude("JOUER"), "joUer");
        assert_eq!(PorterStemmerFr::prelude("ennuie"), "ennuIe");
        assert_eq!(PorterStemmerFr::prelude("yeux"), "Yeux");
        assert_eq!(PorterStemmerFr::prelude("quand"), "qUand");
        assert_eq!(PorterStemmerFr::prelude("a"), "a");
    }

    #[test]
    fn regions_rv_has_two_independent_ifs() {
        assert_eq!(PorterStemmerFr::regions("fameusement").r1, 3);
        assert_eq!(PorterStemmerFr::regions("fameusement").r2, 6);
        assert_eq!(PorterStemmerFr::regions("taii").r1, 4);
        assert_eq!(PorterStemmerFr::regions("taii").r2, 4);
        for (word, rv) in [
            ("parade", 3),
            ("colet", 3),
            ("tapis", 3),
            ("aimer", 3),
            ("adorer", 3),
            ("voler", 2),
            ("tue", 2),
        ] {
            assert_eq!(PorterStemmerFr::regions(word).rv, rv, "rv({word})");
        }
        assert_eq!(
            PorterStemmerFr::regions(""),
            Regions {
                r1: 0,
                r2: 0,
                rv: 0
            }
        );
        assert_eq!(
            PorterStemmerFr::regions("a"),
            Regions {
                r1: 1,
                r2: 1,
                rv: 1
            }
        );
    }

    #[test]
    fn endsin_arr_takes_the_longest() {
        assert_eq!(
            PorterStemmerFr::endsin_arr("table", &["le", "able", "e"]),
            "able"
        );
        assert_eq!(PorterStemmerFr::endsin_arr("table", &["e"]), "e");
        assert_eq!(PorterStemmerFr::endsin_arr("abc", &[]), "");
    }

    #[test]
    fn unicode_and_edges() {
        assert_eq!(s("a"), "a");
        assert_eq!(s("😀"), "😀");
        assert_eq!(s("日本語"), "日本語");
        assert_eq!(s("123"), "123");
    }
}
