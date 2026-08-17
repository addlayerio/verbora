//! The Italian Snowball stemmer, ported from
//! The reference `porter_stemmer_it`.
//!
//! # First match, not longest match
//!
//! Italian's `endsinArr` returns the **first** suffix in array order that
//! matches, unlike the Spanish, French and Dutch helpers of the same name, which
//! return the longest. Every Italian table is therefore hand-ordered longest
//! first, and that order is the algorithm. Sorting the tables, or reusing a
//! shared longest-match helper, changes results.
//!
//! # Off-by-one on purpose
//!
//! `getNextVowelPos` starts scanning at `start + 1`, where the Spanish and
//! Portuguese equivalents start at `start`. The Italian caller compensates by
//! passing 1 where they pass 2. Sharing one helper across the three languages
//! without preserving the call-site arguments shifts every Italian RV by one.
//!
//! # `Yamo`
//!
//! The step-2 verb list contains `"Yamo"`, which `vowelMarking` can never
//! produce — it only uppercases `i` and `u`. It is dead, and it is kept.

use std::borrow::Cow;

use verbora_tokenizers::classes;

use crate::base::{Casing, TokenizeAndStem};
use crate::data::gates::gate_it;
use crate::stopwords::{self, Language};
use crate::units::{ends_with, first_suffix, slen, text, u, units};

/// The Italian Snowball stemmer.
///
/// ```
/// use verbora_stemmers::PorterStemmerIt;
/// let s = PorterStemmerIt::new();
/// assert_eq!(s.stem("CASA"), "cas");
/// assert_eq!(s.stem("acqua"), "acqua");
/// assert_eq!(s.stem("QU"), "qU"); // shorter than 3, but the prelude already ran
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PorterStemmerIt;

/// `a e i o u à è ì ò ù` — lowercase only, so the marked `I`/`U` are consonants.
#[inline]
fn is_vowel(c: u16) -> bool {
    matches!(
        c,
        0x61 | 0x65 | 0x69 | 0x6F | 0x75 | 0xE0 | 0xE8 | 0xEC | 0xF2 | 0xF9
    )
}

#[inline]
fn from(w: &[u16], at: usize) -> &[u16] {
    &w[at.min(w.len())..]
}

fn cut(w: &mut Vec<u16>, n: usize, tail: &str) {
    w.truncate(w.len().saturating_sub(n));
    w.extend(tail.encode_utf16());
}

impl PorterStemmerIt {
    /// Creates the stemmer. It is stateless and zero-sized.
    #[inline]
    pub const fn new() -> Self {
        Self
    }

    /// Stems one token.
    #[allow(
        clippy::unused_self,
        reason = "mirrors the reference's method-shaped API"
    )]
    #[expect(
        clippy::too_many_lines,
        reason = "the else-if chain IS the specification; splitting it hides the order"
    )]
    pub fn stem<'a>(&self, token: &'a str) -> Cow<'a, str> {
        let mut t = units(&token.to_lowercase());

        // --- Prelude -------------------------------------------------------
        // Acute accents become grave ones (`/á/gi` etc., so every occurrence),
        // `qu` is marked, then `i`/`u` between vowels are marked.
        for c in &mut t {
            match *c {
                x if x == u('á') => *c = u('à'),
                x if x == u('é') => *c = u('è'),
                x if x == u('í') => *c = u('ì'),
                x if x == u('ó') => *c = u('ò'),
                x if x == u('ú') => *c = u('ù'),
                _ => {}
            }
        }
        let mut i = 0;
        while i + 1 < t.len() {
            if t[i] == u('q') && t[i + 1] == u('u') {
                t[i + 1] = u('U');
                i += 2;
            } else {
                i += 1;
            }
        }
        // `/([aeiou])(i|u)([aeiou])/g` — non-overlapping, so "aiaia" is "aIaia".
        let mut i = 0;
        while i + 2 < t.len() {
            if is_vowel(t[i]) && (t[i + 1] == u('i') || t[i + 1] == u('u')) && is_vowel(t[i + 2]) {
                t[i + 1] -= 32; // ASCII uppercase
                i += 3;
            } else {
                i += 1;
            }
        }

        if t.len() < 3 {
            // Already lowercased, acute-replaced and qU-marked.
            return Cow::Owned(text(&t));
        }

        // --- Regions -------------------------------------------------------
        let len = t.len();
        let (mut r1, mut r2, mut rv) = (len, len, len);
        for i in 0..len - 1 {
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
        if len > 3 {
            if !is_vowel(t[1]) {
                // getNextVowelPos(token, 1) starts its scan at index 2.
                rv = (2..len).find(|&i| is_vowel(t[i])).unwrap_or(len) + 1;
            } else if is_vowel(t[0]) && is_vowel(t[1]) {
                rv = (2..len).find(|&i| !is_vowel(t[i])).unwrap_or(len) + 1;
            } else {
                rv = 3;
            }
        }

        // `r1txt`/`r2txt`/`rvtxt` are borrowed slices of `t`, recomputed
        // fresh at each use point below, rather than owned `Vec<u16>`
        // snapshots taken once (and re-snapshotted after every mutation):
        // `first_suffix` returns `Option<&'s str>` borrowed from the
        // *suffix list* argument, never from `w`, so the borrow of `t`
        // needed to call it ends the moment the call returns -- well before
        // the `cut(&mut t, ..)` that runs in the same branch, if any. Only
        // one branch of each if-else-if chain below ever executes, and `t`
        // is not mutated until that branch is entered, so a fresh
        // `from(&t, r)` at each check point observes exactly the bytes the
        // old `.to_vec()` snapshot (refreshed by hand after every mutating
        // step) would have. Protected by the recorded-fixture parity suite
        // (`tests/parity.rs`), which replays real the reference output through
        // this function -- not just the hand-written vectors below.

        // --- Step 0: attached pronoun --------------------------------------
        if let Some(suf) = first_suffix(&t, PRONOUN) {
            let n = slen(suf);
            let rv_slice = from(&t, rv);
            let head = &rv_slice[..rv_slice.len().saturating_sub(n)];
            // Two consecutive `if`s in the reference, not an if/else. The two
            // lists are disjoint, so a double truncation cannot actually happen.
            let pre1 = first_suffix(head, &["ando", "endo"]).is_some();
            let pre2 = first_suffix(head, &["ar", "er", "ir"]).is_some();
            if pre1 {
                cut(&mut t, n, "");
            }
            if pre2 {
                // Applied to the ALREADY-truncated token, as written. Using
                // `else if` here would be a behavioural narrowing, not a tidy-up.
                cut(&mut t, n, "e");
            }
        }

        // --- Step 1: standard suffixes -------------------------------------
        // `after0 == after1` in the reference's own shape is "did step 1
        // change anything" -- a `bool` set by every mutating branch below is
        // the same fact without cloning the whole word (twice) to compare it.
        let mut step1_changed = false;
        if let Some(s) = first_suffix(from(&t, r2), STEP1_AMENTE) {
            cut(&mut t, slen(s), "");
            step1_changed = true;
        } else if let Some(s) = first_suffix(from(&t, r2), STEP1_AZIONE) {
            cut(&mut t, slen(s), "");
            step1_changed = true;
        } else if let Some(s) = first_suffix(from(&t, r2), &["logia", "logie"]) {
            cut(&mut t, slen(s), "log");
            step1_changed = true;
        } else if let Some(s) =
            first_suffix(from(&t, r2), &["uzione", "uzioni", "usione", "usioni"])
        {
            cut(&mut t, slen(s), "u");
            step1_changed = true;
        } else if let Some(s) = first_suffix(from(&t, r2), &["enza", "enze"]) {
            cut(&mut t, slen(s), "ente");
            step1_changed = true;
        } else if let Some(s) =
            first_suffix(from(&t, rv), &["amento", "amenti", "imento", "imenti"])
        {
            cut(&mut t, slen(s), "");
            step1_changed = true;
        } else if let Some(s) = first_suffix(from(&t, r1), &["amente"]) {
            cut(&mut t, slen(s), "");
            step1_changed = true;
        } else if let Some(s) = first_suffix(from(&t, r2), STEP1_ATRICE) {
            cut(&mut t, slen(s), "");
            step1_changed = true;
        } else if let Some(s) = first_suffix(from(&t, r2), &["abilità", "icità", "ività", "ità"])
        {
            cut(&mut t, slen(s), "");
            step1_changed = true;
        } else if let Some(s) = first_suffix(from(&t, r2), STEP1_ICATIVA) {
            cut(&mut t, slen(s), "");
            step1_changed = true;
        }

        // --- Step 2: verb suffixes, only if step 1 changed nothing ---------
        if !step1_changed && let Some(s) = first_suffix(from(&t, rv), STEP2) {
            cut(&mut t, slen(s), "");
        }

        // --- Step 3: vowel suffix, always ----------------------------------
        if let Some(s) = first_suffix(from(&t, rv), STEP3) {
            cut(&mut t, slen(s), "");
        }

        if ends_with(from(&t, rv), "ch") {
            cut(&mut t, 2, "c");
        } else if ends_with(from(&t, rv), "gh") {
            cut(&mut t, 2, "g");
        }

        Cow::Owned(text(&t).to_lowercase())
    }
}

/// The 36 attached pronouns, longest first.
static PRONOUN: &[&str] = &[
    "glieli", "glielo", "gliene", "gliela", "gliele", "sene", "tene", "cela", "cele", "celi",
    "celo", "cene", "vela", "vele", "veli", "velo", "vene", "mela", "mele", "meli", "melo", "mene",
    "tela", "tele", "teli", "telo", "gli", "ci", "la", "le", "li", "lo", "mi", "ne", "si", "ti",
    "vi",
];
static STEP1_AMENTE: &[&str] = &[
    "ativamente",
    "abilamente",
    "ivamente",
    "osamente",
    "icamente",
];
static STEP1_AZIONE: &[&str] = &[
    "icazione", "icazioni", "icatore", "icatori", "azione", "azioni", "atore", "atori",
];
static STEP1_ATRICE: &[&str] = &[
    "atrice", "atrici", "abile", "abili", "ibile", "ibili", "mente", "ante", "anti", "anza",
    "anze", "iche", "ichi", "ismo", "ismi", "ista", "iste", "isti", "istà", "istè", "istì", "ico",
    "ici", "ica", "ice", "oso", "osi", "osa", "ose",
];
static STEP1_ICATIVA: &[&str] = &[
    "icativa", "icativo", "icativi", "icative", "ativa", "ativo", "ativi", "ative", "iva", "ivo",
    "ivi", "ive",
];
/// The verb list. `"Yamo"` is dead — `vowelMarking` only ever writes `I` and `U`
/// — and is preserved for byte parity with the reference table.
static STEP2: &[&str] = &[
    "erebbero", "irebbero", "assero", "assimo", "eranno", "erebbe", "eremmo", "ereste", "eresti",
    "essero", "iranno", "irebbe", "iremmo", "ireste", "iresti", "iscano", "iscono", "issero",
    "arono", "avamo", "avano", "avate", "eremo", "erete", "erono", "evamo", "evano", "evate",
    "iremo", "irete", "irono", "ivamo", "ivano", "ivate", "ammo", "ando", "asse", "assi", "emmo",
    "enda", "ende", "endi", "endo", "erai", "Yamo", "iamo", "immo", "irai", "irei", "isca", "isce",
    "isci", "isco", "erei", "uti", "uto", "ita", "ite", "iti", "ito", "iva", "ivi", "ivo", "ono",
    "uta", "ute", "ano", "are", "ata", "ate", "ati", "ato", "ava", "avi", "avo", "erà", "ere",
    "erò", "ete", "eva", "evi", "evo", "irà", "ire", "irò", "ar", "ir",
];
static STEP3: &[&str] = &[
    "ia", "ie", "ii", "io", "ià", "iè", "iì", "iò", "a", "e", "i", "o", "à", "è", "ì", "ò",
];

impl TokenizeAndStem for PorterStemmerIt {
    const FILTER_ON: Casing = Casing::Raw;
    const STEM_ON: Casing = Casing::Lower;

    fn is_word_char(c: char) -> bool {
        classes::is_word_it(c)
    }

    fn is_stop_word(word: &str) -> bool {
        stopwords::contains(Language::It, word)
    }

    fn gate(token: &str) -> bool {
        token.encode_utf16().any(gate_it)
    }

    fn stem_token(&self, token: &str) -> String {
        self.stem(token).into_owned()
    }
}

impl verbora_core::Stemmer for PorterStemmerIt {
    fn stem<'a>(&self, token: &'a str) -> Cow<'a, str> {
        Self::stem(self, token)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(t: &str) -> String {
        PorterStemmerIt::new().stem(t).into_owned()
    }

    #[test]
    fn documented_vectors() {
        for (input, want) in [
            ("CASA", "cas"),
            ("QUELLO", "quell"),
            ("acqua", "acqua"),
            ("perché", "perc"),
            ("città", "citt"),
            ("gli", "gli"),
            ("", ""),
        ] {
            assert_eq!(s(input), want, "stem({input})");
        }
    }

    #[test]
    fn the_prelude_runs_before_the_length_gate() {
        assert_eq!(s("QU"), "qU");
        assert_eq!(s("qu"), "qU");
    }

    #[test]
    fn unicode_and_edges() {
        assert_eq!(s("a"), "a");
        assert_eq!(s("😀"), "😀");
        assert_eq!(s("日本語"), "日本語");
        assert_eq!(s("123"), "123");
    }
}
