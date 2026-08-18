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
//!
//! # How `stem` reads its regions
//!
//! The reference keeps `r1txt`/`r2txt`/`rvtxt` snapshots and refreshes them by
//! hand after every cut. This port computes each suffix test directly on the
//! current buffer, with the region expressed as the `lb` limit of one
//! [`crate::among`] binary search (`docs/PERFORMANCE_GAPS.md` entry 34) —
//! equivalent everywhere the reference's snapshot was fresh, which is every
//! read site except one: step 2b's `âmes` branch tests the **pre-deletion**
//! RV for the `e` that precedes the suffix, and that one pre-cut fact is
//! captured before the cut (marked at the call site). French is a
//! longest-match language throughout, which is `find_among`'s native
//! semantics — no table-ordering caveat applies. The pre-conversion
//! implementation, snapshots and all, is kept in this module's tests as the
//! byte-exactness oracle.

use std::borrow::Cow;
use std::sync::LazyLock;

use verbora_tokenizers::classes;

use crate::among::AmongTable;
use crate::base::{Casing, TokenizeAndStem};
use crate::data::gates::gate_fr;
use crate::stopwords::{self, Language};
use crate::units::{at, ends_with, longest_suffix, text, text_lowercase, u, units};

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

/// Removes `n` code units from the end and appends `tail`.
fn cut(w: &mut Vec<u16>, n: usize, tail: &str) {
    w.truncate(w.len().saturating_sub(n));
    w.extend(tail.encode_utf16());
}

/// The sorted search tables, built once from the rule tables below.
struct FrTables {
    ance: AmongTable,
    icatrice: AmongTable,
    atrice: AmongTable,
    logie: AmongTable,
    usion: AmongTable,
    ence: AmongTable,
    issement: AmongTable,
    ativement: AmongTable,
    ivement: AmongTable,
    eusement: AmongTable,
    ablement: AmongTable,
    ierement: AmongTable,
    ement: AmongTable,
    icite: AmongTable,
    abilite: AmongTable,
    ite: AmongTable,
    icatif: AmongTable,
    atif: AmongTable,
    if_ive: AmongTable,
    euse: AmongTable,
    ment: AmongTable,
    step2a: AmongTable,
    step2b_e: AmongTable,
    step2b_a: AmongTable,
    ier: AmongTable,
    undouble: AmongTable,
}

static TABLES: LazyLock<FrTables> = LazyLock::new(|| FrTables {
    ance: AmongTable::build(STEP1_ANCE),
    icatrice: AmongTable::build(ICATRICE),
    atrice: AmongTable::build(ATRICE),
    logie: AmongTable::build(&["logie", "logies"]),
    usion: AmongTable::build(&["usion", "ution", "usions", "utions"]),
    ence: AmongTable::build(&["ence", "ences"]),
    issement: AmongTable::build(&["issement", "issements"]),
    ativement: AmongTable::build(&["ativement", "ativements"]),
    ivement: AmongTable::build(&["ivement", "ivements"]),
    eusement: AmongTable::build(&["eusement", "eusements"]),
    ablement: AmongTable::build(&["ablement", "ablements", "iqUement", "iqUements"]),
    ierement: AmongTable::build(&["ièrement", "ièrements", "Ièrement", "Ièrements"]),
    ement: AmongTable::build(&["ement", "ements"]),
    icite: AmongTable::build(&["icité", "icités"]),
    abilite: AmongTable::build(&["abilité", "abilités"]),
    ite: AmongTable::build(&["ité", "ités"]),
    icatif: AmongTable::build(ICATIF),
    atif: AmongTable::build(ATIF),
    if_ive: AmongTable::build(&["if", "ive", "ifs", "ives"]),
    euse: AmongTable::build(&["euse", "euses"]),
    ment: AmongTable::build(&["ment", "ments"]),
    step2a: AmongTable::build(STEP2A),
    step2b_e: AmongTable::build(STEP2B_E),
    step2b_a: AmongTable::build(STEP2B_A),
    ier: AmongTable::build(&["ier", "ière", "Ier", "Ière"]),
    undouble: AmongTable::build(&["enn", "onn", "ett", "ell", "eill"]),
});

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
    pub fn stem<'a>(&self, token: &'a str) -> Cow<'a, str> {
        let tb = &*TABLES;
        let mut t = Self::prelude_units(&units(&token.to_lowercase()));
        if t.len() == 1 {
            return Cow::Owned(text(&t));
        }
        let regs = Self::regions_units(&t);
        let before_step1 = t.clone();
        let mut do_step2a = false;

        // --- Step 1 --------------------------------------------------------
        //
        // A labeled block with one early exit per rule reproduces the
        // reference's else-if ladder: the first rule whose search hits ends
        // the step, even when its inner guard then declines to cut. `t` is
        // only cut as a rule's last act, so the region limits computed here
        // from the un-cut `t` are the reference's fresh snapshots at every
        // read below; the two rules that cut twice recompute their limits in
        // between, where the reference refreshed its snapshots.
        let len = t.len();
        let lb1 = regs.r1.min(len);
        let lb2 = regs.r2.min(len);
        let lbv = regs.rv.min(len);
        'step1: {
            let m = tb.ance.longest(&t, len, lb2);
            if m > 0 {
                cut(&mut t, m, "");
                break 'step1;
            }
            let m = tb.icatrice.longest(&t, len, 0);
            if m > 0 {
                if tb.icatrice.longest(&t, len, lb2) > 0 {
                    cut(&mut t, m, "");
                } else {
                    cut(&mut t, m, "iqU");
                }
                break 'step1;
            }
            let m = tb.atrice.longest(&t, len, lb2);
            if m > 0 {
                cut(&mut t, m, "");
                break 'step1;
            }
            let m = tb.logie.longest(&t, len, lb2);
            if m > 0 {
                cut(&mut t, m, "log");
                break 'step1;
            }
            let m = tb.usion.longest(&t, len, lb2);
            if m > 0 {
                cut(&mut t, m, "u");
                break 'step1;
            }
            let m = tb.ence.longest(&t, len, lb2);
            if m > 0 {
                cut(&mut t, m, "ent");
                break 'step1;
            }
            let m = tb.issement.longest(&t, len, lb1);
            if m > 0 {
                // The rule consumes the chain even when the vowel test
                // declines the cut — a plain `if` inside the branch.
                if !(len > m && is_vowel(t[len - m - 1])) {
                    cut(&mut t, m, "");
                }
                break 'step1;
            }
            let m = tb.ativement.longest(&t, len, lb2);
            if m > 0 {
                cut(&mut t, m, "");
                break 'step1;
            }
            let m = tb.ivement.longest(&t, len, lb2);
            if m > 0 {
                cut(&mut t, m, "");
                break 'step1;
            }
            if tb.eusement.longest(&t, len, 0) > 0 {
                let m = tb.eusement.longest(&t, len, lb2);
                if m > 0 {
                    cut(&mut t, m, "");
                } else {
                    let m = tb.eusement.longest(&t, len, lb1);
                    if m > 0 {
                        cut(&mut t, m, "eux");
                    } else {
                        let m = tb.ement.longest(&t, len, lbv);
                        if m > 0 {
                            cut(&mut t, m, "");
                        }
                    }
                }
                break 'step1;
            }
            let m = tb.ablement.longest(&t, len, lb2);
            if m > 0 {
                cut(&mut t, m, "");
                break 'step1;
            }
            let m = tb.ierement.longest(&t, len, lbv);
            if m > 0 {
                cut(&mut t, m, "i");
                break 'step1;
            }
            let m = tb.ement.longest(&t, len, lbv);
            if m > 0 {
                cut(&mut t, m, "");
                break 'step1;
            }
            let m = tb.icite.longest(&t, len, 0);
            if m > 0 {
                if tb.icite.longest(&t, len, lb2) > 0 {
                    cut(&mut t, m, "");
                } else {
                    cut(&mut t, m, "iqU");
                }
                break 'step1;
            }
            let m = tb.abilite.longest(&t, len, 0);
            if m > 0 {
                if tb.abilite.longest(&t, len, lb2) > 0 {
                    cut(&mut t, m, "");
                } else {
                    cut(&mut t, m, "abl");
                }
                break 'step1;
            }
            let m = tb.ite.longest(&t, len, lb2);
            if m > 0 {
                cut(&mut t, m, "");
                break 'step1;
            }
            if tb.icatif.longest(&t, len, 0) > 0 {
                // Two consecutive `if`s, not an if/else — both can fire.
                let m = tb.icatif.longest(&t, len, lb2);
                if m > 0 {
                    cut(&mut t, m, "");
                }
                let len2 = t.len();
                let m = tb.atif.longest(&t, len2, regs.r2.min(len2));
                if m > 0 {
                    cut(&mut t, m + 2, "iqU");
                }
                break 'step1;
            }
            let m = tb.atif.longest(&t, len, lb2);
            if m > 0 {
                cut(&mut t, m, "");
                break 'step1;
            }
            let m = tb.if_ive.longest(&t, len, lb2);
            if m > 0 {
                cut(&mut t, m, "");
                break 'step1;
            }
            if ends_with(&t, "eaux") {
                cut(&mut t, 4, "eau");
                break 'step1;
            }
            if ends_with(&t, "aux") && len - lb1 >= 3 {
                cut(&mut t, 3, "al");
                break 'step1;
            }
            let m = tb.euse.longest(&t, len, lb2);
            if m > 0 {
                cut(&mut t, m, "");
                break 'step1;
            }
            let m = tb.euse.longest(&t, len, lb1);
            if m > 0 {
                cut(&mut t, m, "eux");
                break 'step1;
            }
            if ends_with(&t, "amment") && len - lbv >= 6 {
                cut(&mut t, 6, "ant");
                do_step2a = true;
                break 'step1;
            }
            if ends_with(&t, "emment") && len - lbv >= 6 {
                cut(&mut t, 6, "ent");
                do_step2a = true;
                break 'step1;
            }
            let m = tb.ment.longest(&t, len, lbv);
            if m > 0 {
                // A vowel must precede, and that vowel must be inside RV.
                if len > m && is_vowel(t[len - m - 1]) && len - lbv > m {
                    cut(&mut t, m, "");
                    do_step2a = true;
                }
                break 'step1;
            }
        }

        // --- Step 2a -------------------------------------------------------
        let mut step2a_done = false;
        let mut step2a_changed = false;
        if before_step1 == t || do_step2a {
            step2a_done = true;
            let len = t.len();
            let lbv = regs.rv.min(len);
            let m = tb.step2a.longest(&t, len, lbv);
            if m > 0 && len > m {
                let b = t[len - m - 1];
                if !is_vowel(b) && len - lbv > m {
                    cut(&mut t, m, "");
                    step2a_changed = true;
                }
            }
        }

        // --- Step 2b -------------------------------------------------------
        if step2a_done && !step2a_changed {
            let len = t.len();
            let lbv = regs.rv.min(len);
            let m = tb.step2b_e.longest(&t, len, lbv);
            if m > 0 {
                cut(&mut t, m, "");
            } else if ends_with(&t, "ions") && len - lbv >= 4 && len - regs.r2.min(len) >= 4 {
                cut(&mut t, 4, "");
            } else {
                let m = tb.step2b_a.longest(&t, len, lbv);
                if m > 0 {
                    // The `e` test below reads the PRE-deletion RV,
                    // deliberately — capture its length before the cut.
                    let rv_len_before = len - lbv;
                    cut(&mut t, m, "");
                    if t.last() == Some(&u('e')) && rv_len_before > m {
                        t.truncate(t.len() - 1);
                    }
                }
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
            }
            {
                let len = t.len();
                if ends_with(&t, "ion") && len - regs.r2.min(len) >= 3 {
                    let before = if len >= 4 { Some(t[len - 4]) } else { None };
                    if before.is_some_and(|c| c == u('s') || c == u('t')) {
                        cut(&mut t, 3, "");
                    }
                }
            }
            {
                let len = t.len();
                let lbv = regs.rv.min(len);
                let m = tb.ier.longest(&t, len, lbv);
                if m > 0 {
                    cut(&mut t, m, "i");
                }
            }
            // `endsinArr(rvtxt, 'e')`: a STRING, iterated as its characters.
            {
                let len = t.len();
                let lbv = regs.rv.min(len);
                if len > lbv && t[len - 1] == u('e') {
                    t.truncate(len - 1);
                }
            }
            {
                let len = t.len();
                let lbv = regs.rv.min(len);
                if len > lbv && t[len - 1] == u('ë') {
                    // `token.slice(token.length - 3, -1)`: a start argument
                    // below zero is treated as an offset from the end, not
                    // clamped, so for a token shorter than three units this
                    // can never be "gu".
                    if len >= 3 && t[len - 3] == u('g') && t[len - 2] == u('u') {
                        t.truncate(len - 1);
                    }
                }
            }
        }

        // --- Step 5: undouble ----------------------------------------------
        if tb.undouble.longest(&t, t.len(), 0) > 0 {
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

        Cow::Owned(text_lowercase(&mut t))
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

    // -----------------------------------------------------------------------
    // Differential oracle: the pre-find_among implementation, verbatim —
    // owned `r2txt`/`rvtxt` snapshots, hand refreshes and all.
    // -----------------------------------------------------------------------
    mod oracle {
        use super::super::*;
        use crate::units::{longest_suffix, slen};

        fn from(w: &[u16], at: usize) -> &[u16] {
            &w[at.min(w.len())..]
        }

        /// `endsin(token, letterBefore + suffix)` without building a string.
        fn ends_with_prefixed(w: &[u16], before: u16, suffix: &str) -> bool {
            let n = slen(suffix);
            w.len() > n && w[w.len() - n - 1] == before && ends_with(w, suffix)
        }

        #[expect(clippy::too_many_lines, reason = "verbatim copy of the reference port")]
        #[expect(
            unused_assignments,
            reason = "the reference refreshes r1/r2/rv after every cut and this copy \
                      mirrors that uniformly, dead refreshes included"
        )]
        pub(super) fn stem(token: &str) -> String {
            let mut t = PorterStemmerFr::prelude_units(&units(&token.to_lowercase()));
            if t.len() == 1 {
                return text(&t);
            }
            let regs = PorterStemmerFr::regions_units(&t);
            let mut r1txt = from(&t, regs.r1);
            let mut r2txt = from(&t, regs.r2).to_vec();
            let mut rvtxt = from(&t, regs.rv).to_vec();
            let before_step1 = t.clone();
            let mut do_step2a = false;

            // --- Step 1 ----------------------------------------------------
            if let Some(sfx) = longest_suffix(&r2txt, STEP1_ANCE) {
                cut(&mut t, slen(sfx), "");
            } else if let Some(sfx) = longest_suffix(&t, ICATRICE) {
                if longest_suffix(&r2txt, ICATRICE).is_some() {
                    cut(&mut t, slen(sfx), "");
                } else {
                    cut(&mut t, slen(sfx), "iqU");
                }
            } else if let Some(sfx) = longest_suffix(&r2txt, ATRICE) {
                cut(&mut t, slen(sfx), "");
            } else if let Some(sfx) = longest_suffix(&r2txt, &["logie", "logies"]) {
                cut(&mut t, slen(sfx), "log");
            } else if let Some(sfx) =
                longest_suffix(&r2txt, &["usion", "ution", "usions", "utions"])
            {
                cut(&mut t, slen(sfx), "u");
            } else if let Some(sfx) = longest_suffix(&r2txt, &["ence", "ences"]) {
                cut(&mut t, slen(sfx), "ent");
            } else if let Some(sfx) = longest_suffix(r1txt, &["issement", "issements"]) {
                let n = slen(sfx);
                if !(t.len() > n && is_vowel(t[t.len() - n - 1])) {
                    cut(&mut t, n, "");
                    r1txt = from(&t, regs.r1);
                    r2txt = from(&t, regs.r2).to_vec();
                    rvtxt = from(&t, regs.rv).to_vec();
                }
            } else if let Some(sfx) = longest_suffix(&r2txt, &["ativement", "ativements"]) {
                cut(&mut t, slen(sfx), "");
            } else if let Some(sfx) = longest_suffix(&r2txt, &["ivement", "ivements"]) {
                cut(&mut t, slen(sfx), "");
            } else if longest_suffix(&t, &["eusement", "eusements"]).is_some() {
                if let Some(sfx) = longest_suffix(&r2txt, &["eusement", "eusements"]) {
                    cut(&mut t, slen(sfx), "");
                } else if let Some(sfx) = longest_suffix(r1txt, &["eusement", "eusements"]) {
                    cut(&mut t, slen(sfx), "eux");
                } else if let Some(sfx) = longest_suffix(&rvtxt, &["ement", "ements"]) {
                    cut(&mut t, slen(sfx), "");
                }
            } else if let Some(sfx) =
                longest_suffix(&r2txt, &["ablement", "ablements", "iqUement", "iqUements"])
            {
                cut(&mut t, slen(sfx), "");
            } else if let Some(sfx) =
                longest_suffix(&rvtxt, &["ièrement", "ièrements", "Ièrement", "Ièrements"])
            {
                cut(&mut t, slen(sfx), "i");
            } else if let Some(sfx) = longest_suffix(&rvtxt, &["ement", "ements"]) {
                cut(&mut t, slen(sfx), "");
            } else if let Some(sfx) = longest_suffix(&t, &["icité", "icités"]) {
                if longest_suffix(&r2txt, &["icité", "icités"]).is_some() {
                    cut(&mut t, slen(sfx), "");
                } else {
                    cut(&mut t, slen(sfx), "iqU");
                }
            } else if let Some(sfx) = longest_suffix(&t, &["abilité", "abilités"]) {
                if longest_suffix(&r2txt, &["abilité", "abilités"]).is_some() {
                    cut(&mut t, slen(sfx), "");
                } else {
                    cut(&mut t, slen(sfx), "abl");
                }
            } else if let Some(sfx) = longest_suffix(&r2txt, &["ité", "ités"]) {
                cut(&mut t, slen(sfx), "");
            } else if longest_suffix(&t, ICATIF).is_some() {
                if let Some(sfx) = longest_suffix(&r2txt, ICATIF) {
                    cut(&mut t, slen(sfx), "");
                    r2txt = from(&t, regs.r2).to_vec();
                    rvtxt = from(&t, regs.rv).to_vec();
                }
                if let Some(sfx) = longest_suffix(&r2txt, ATIF) {
                    cut(&mut t, slen(sfx) + 2, "iqU");
                    r2txt = from(&t, regs.r2).to_vec();
                    rvtxt = from(&t, regs.rv).to_vec();
                }
            } else if let Some(sfx) = longest_suffix(&r2txt, ATIF) {
                cut(&mut t, slen(sfx), "");
            } else if let Some(sfx) = longest_suffix(&r2txt, &["if", "ive", "ifs", "ives"]) {
                cut(&mut t, slen(sfx), "");
            } else if let Some(sfx) = longest_suffix(&t, &["eaux"]) {
                cut(&mut t, slen(sfx), "eau");
            } else if let Some(sfx) = longest_suffix(r1txt, &["aux"]) {
                cut(&mut t, slen(sfx), "al");
            } else if let Some(sfx) = longest_suffix(&r2txt, &["euse", "euses"]) {
                cut(&mut t, slen(sfx), "");
            } else if let Some(sfx) = longest_suffix(r1txt, &["euse", "euses"]) {
                cut(&mut t, slen(sfx), "eux");
            } else if let Some(sfx) = longest_suffix(&rvtxt, &["amment"]) {
                cut(&mut t, slen(sfx), "ant");
                do_step2a = true;
            } else if let Some(sfx) = longest_suffix(&rvtxt, &["emment"]) {
                cut(&mut t, slen(sfx), "ent");
                do_step2a = true;
            } else if let Some(sfx) = longest_suffix(&rvtxt, &["ment", "ments"]) {
                let n = slen(sfx);
                let before = if t.len() > n {
                    Some(t[t.len() - n - 1])
                } else {
                    None
                };
                if let Some(b) = before
                    && is_vowel(b)
                    && ends_with_prefixed(&rvtxt, b, sfx)
                {
                    cut(&mut t, n, "");
                    do_step2a = true;
                }
            }

            r2txt = from(&t, regs.r2).to_vec();
            rvtxt = from(&t, regs.rv).to_vec();

            // --- Step 2a ---------------------------------------------------
            let mut step2a_done = false;
            let mut step2a_changed = false;
            if before_step1 == t || do_step2a {
                step2a_done = true;
                if let Some(sfx) = longest_suffix(&rvtxt, STEP2A) {
                    let n = slen(sfx);
                    if t.len() > n {
                        let b = t[t.len() - n - 1];
                        if !is_vowel(b) && ends_with_prefixed(&rvtxt, b, sfx) {
                            cut(&mut t, n, "");
                            step2a_changed = true;
                        }
                    }
                }
            }

            // --- Step 2b ---------------------------------------------------
            if step2a_done && !step2a_changed {
                if let Some(sfx) = longest_suffix(&rvtxt, STEP2B_E) {
                    cut(&mut t, slen(sfx), "");
                    r2txt = from(&t, regs.r2).to_vec();
                    rvtxt = from(&t, regs.rv).to_vec();
                } else if longest_suffix(&rvtxt, &["ions"]).is_some()
                    && longest_suffix(&r2txt, &["ions"]).is_some()
                {
                    cut(&mut t, 4, "");
                    r2txt = from(&t, regs.r2).to_vec();
                    rvtxt = from(&t, regs.rv).to_vec();
                } else if let Some(sfx) = longest_suffix(&rvtxt, STEP2B_A) {
                    cut(&mut t, slen(sfx), "");
                    // `rvtxt` here is still the pre-deletion slice, deliberately.
                    if t.last() == Some(&u('e')) && ends_with_prefixed(&rvtxt, u('e'), sfx) {
                        t.truncate(t.len() - 1);
                    }
                    r2txt = from(&t, regs.r2).to_vec();
                    rvtxt = from(&t, regs.rv).to_vec();
                }
            }

            if t != before_step1 {
                // --- Step 3 ------------------------------------------------
                if t.last() == Some(&u('Y')) {
                    let n = t.len();
                    t[n - 1] = u('i');
                }
                if t.last() == Some(&u('ç')) {
                    let n = t.len();
                    t[n - 1] = u('c');
                }
            } else {
                // --- Step 4: residual --------------------------------------
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
                if let Some(sfx) = longest_suffix(&rvtxt, &["ier", "ière", "Ier", "Ière"]) {
                    cut(&mut t, slen(sfx), "i");
                    r2txt = from(&t, regs.r2).to_vec();
                    rvtxt = from(&t, regs.rv).to_vec();
                }
                if ends_with(&rvtxt, "e") {
                    t.truncate(t.len().saturating_sub(1));
                    rvtxt = from(&t, regs.rv).to_vec();
                }
                if ends_with(&rvtxt, "ë")
                    && t.len() >= 3
                    && ends_with(&t[t.len() - 3..t.len() - 1], "gu")
                {
                    t.truncate(t.len() - 1);
                }
            }

            // --- Step 5: undouble ------------------------------------------
            if longest_suffix(&t, &["enn", "onn", "ett", "ell", "eill"]).is_some() {
                t.truncate(t.len() - 1);
            }

            // --- Step 6: un-accent the final é/è ---------------------------
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

            text(&t).to_lowercase()
        }
    }

    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            let mut x = self.0;
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            self.0 = x;
            x
        }
        fn below(&mut self, n: usize) -> usize {
            (self.next() % n as u64) as usize
        }
    }

    /// French stems crossed with real table suffixes (stacked up to two
    /// deep), the prelude's semivowel triggers (`y`, `qu`, vowel runs), the
    /// `par`/`col`/`tap` RV prefixes, and case/astral/CJK noise.
    fn random_word(rng: &mut Rng) -> String {
        const ALPHA: &[char] = &[
            'a', 'b', 'c', 'd', 'e', 'f', 'g', 'i', 'l', 'm', 'n', 'o', 'p', 'q', 'r', 's', 't',
            'u', 'v', 'y', 'z', 'é', 'è', 'ê', 'ë', 'â', 'î', 'ï', 'û', 'ç',
        ];
        const PREFIXES: &[&str] = &["", "", "", "par", "col", "tap", "y", "qu"];
        const SUFFIXES: &[&str] = &[
            "ance",
            "iqUe",
            "ique",
            "isme",
            "able",
            "iste",
            "eux",
            "icatrice",
            "ications",
            "atrice",
            "ateurs",
            "logie",
            "usion",
            "utions",
            "ence",
            "issement",
            "ativement",
            "ivement",
            "eusement",
            "ablement",
            "ièrement",
            "ement",
            "icité",
            "abilité",
            "ité",
            "icatif",
            "atives",
            "if",
            "ives",
            "eaux",
            "aux",
            "euse",
            "euses",
            "amment",
            "emment",
            "ment",
            "ments",
            "îmes",
            "issaIent",
            "irions",
            "is",
            "it",
            "ie",
            "ir",
            "èrent",
            "er",
            "eraIent",
            "erions",
            "ez",
            "iez",
            "âmes",
            "aIent",
            "assions",
            "ant",
            "as",
            "a",
            "ai",
            "ions",
            "ion",
            "ier",
            "ière",
            "e",
            "ë",
            "guë",
            "enne",
            "onne",
            "ette",
            "elle",
            "eille",
            "s",
            "és",
            "ée",
        ];
        let mut s = String::from(PREFIXES[rng.below(PREFIXES.len())]);
        for _ in 0..rng.below(7) {
            s.push(ALPHA[rng.below(ALPHA.len())]);
        }
        if rng.below(10) < 7 {
            s.push_str(SUFFIXES[rng.below(SUFFIXES.len())]);
            if rng.below(4) == 0 {
                s.push_str(SUFFIXES[rng.below(SUFFIXES.len())]);
            }
        }
        match rng.below(40) {
            0 => s = s.to_uppercase(),
            1 => s.push('😀'),
            2 => s.insert(0, '日'),
            3 => s.push_str("123"),
            _ => {}
        }
        s
    }

    #[test]
    fn differential_against_the_linear_scan_oracle() {
        let stemmer = PorterStemmerFr::new();
        let check = |input: &str| {
            assert_eq!(
                stemmer.stem(input).as_ref(),
                oracle::stem(input),
                "stem({input:?})"
            );
        };
        for w in crate::test_support::bench_words("fr") {
            check(&w);
        }
        for w in [
            "",
            "a",
            "é",
            "yeux",
            "quand",
            "ennuie",
            "ÊTRE",
            "volerait",
            "publicité",
            "pitoyable",
            "saisissement",
            "premièrement",
            "trouverions",
            "voyiez",
            "aiguë",
            "guë",
            "assions",
            "instruments",
            "tempérament",
            "eusement",
            "fameusement",
        ] {
            check(w);
        }
        let mut rng = Rng(0xF12A_9C0B_55AA_33CC);
        for _ in 0..60_000 {
            let w = random_word(&mut rng);
            check(&w);
        }
    }
}
