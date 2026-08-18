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
//! `stem` nevertheless runs each table through one [`crate::among`]
//! longest-match binary search (`docs/PERFORMANCE_GAPS.md` entry 34): in every
//! shipped table, whenever one entry is a proper suffix of another the longer
//! one is listed first, so first-listed and longest coincide — including under
//! the region check, because an entry too long for the region is excluded from
//! the search by the same `lb` limit `hasSuffixInRegion` applies. That table
//! property is pinned by `tables_are_ordered_longest_first_within_nests`
//! below, so a future table edit cannot silently break the equivalence, and
//! the pre-conversion implementation is kept in the tests as the
//! byte-exactness oracle.
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
use std::sync::LazyLock;

use verbora_tokenizers::classes;

use crate::among::AmongTable;
use crate::base::{Casing, TokenizeAndStem};
use crate::stopwords::{self, Language};
use crate::units::{ends_with, push_str, text, units};

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

/// `hasVowelAtIndex`. Out of range is the reference's `undefined`, which is not
/// in the vowel string, so it is false.
#[inline]
fn vowel_at(w: &[u16], i: usize) -> bool {
    w.get(i).copied().is_some_and(is_vowel)
}

/// `markRegionN`: the first position after a non-vowel that follows a vowel.
fn mark_region_n(w: &[u16], start: usize) -> usize {
    let length = w.len();
    let mut index = start;
    let mut region = length;
    while index + 1 < length && region == length {
        if vowel_at(w, index) && !vowel_at(w, index + 1) {
            region = index + 2;
        }
        index += 1;
    }
    region
}

/// `markRegionV`.
fn mark_region_v(w: &[u16]) -> usize {
    let mut rv = w.len();
    if rv > 3 {
        if !vowel_at(w, 1) {
            rv = (2..w.len()).find(|&i| vowel_at(w, i)).unwrap_or(w.len()) + 1;
        } else if vowel_at(w, 0) && vowel_at(w, 1) {
            rv = (2..w.len()).find(|&i| !vowel_at(w, i)).unwrap_or(w.len()) + 1;
        } else {
            rv = 3;
        }
    }
    rv
}

/// `replaceAll`, which the reference implements as `split(find).join(replace)`:
/// every non-overlapping occurrence, scanning left to right.
///
/// Rebuilds in place only when `find` actually occurs — the four detour calls
/// in `stem` are no-ops for the typical nasal-free word, and skipping the
/// rebuild there removes four allocations per word without changing a byte
/// of output (a zero-occurrence rebuild is the identity).
fn replace_all(w: &mut Vec<u16>, find: &[u16], replacement: &[u16]) {
    let first = match w.windows(find.len()).position(|win| win == find) {
        Some(i) => i,
        None => return,
    };
    let mut out = Vec::with_capacity(w.len());
    out.extend_from_slice(&w[..first]);
    let mut i = first;
    while i < w.len() {
        if w[i..].starts_with(find) {
            out.extend_from_slice(replacement);
            i += find.len();
        } else {
            out.push(w[i]);
            i += 1;
        }
    }
    *w = out;
}

/// `replaceSuffixInRegion` through one `find_among` search: the longest entry
/// fitting the region fires, which is the first listed one — see module docs.
///
/// Returns whether a suffix matched and was replaced; every shipped table
/// pairs a non-empty suffix with a distinct replacement, so "matched" and
/// "the word changed" coincide at every call site.
fn replace_in_region(
    w: &mut Vec<u16>,
    table: &AmongTable,
    replacement: &str,
    region: usize,
) -> bool {
    let len = w.len();
    let n = table.longest(w, len, region.min(len));
    if n > 0 {
        w.truncate(len - n);
        push_str(w, replacement);
        true
    } else {
        false
    }
}

/// The sorted search tables, built once from the ordered rule tables below.
struct PtTables {
    noun: AmongTable,
    logia: AmongTable,
    encia: AmongTable,
    mente2: AmongTable,
    mente: AmongTable,
    idade: AmongTable,
    ivo: AmongTable,
    /// `iras|ira → "ir"`, gated on the word ending `eiras`/`eira`.
    ira: AmongTable,
    verb: AmongTable,
    residual: AmongTable,
    /// Step 5's `ue|ué|uê` group (after a `g`).
    ue: AmongTable,
    /// Step 5's `ie|ié|iê` group (after a `c`).
    ie: AmongTable,
    /// Step 5's unconditional `e|é|ê`.
    e: AmongTable,
}

static TABLES: LazyLock<PtTables> = LazyLock::new(|| PtTables {
    noun: AmongTable::build(STEP1_NOUN),
    logia: AmongTable::build(&["logias", "logia"]),
    encia: AmongTable::build(&["ências", "ência"]),
    mente2: AmongTable::build(&["ativamente", "icamente", "ivamente", "osamente", "adamente"]),
    mente: AmongTable::build(&["antemente", "avelmente", "ivelmente", "mente"]),
    idade: AmongTable::build(STEP1_IDADE),
    ivo: AmongTable::build(STEP1_IVO),
    ira: AmongTable::build(&["iras", "ira"]),
    verb: AmongTable::build(VERB),
    residual: AmongTable::build(RESIDUAL),
    ue: AmongTable::build(&["ue", "ué", "uê"]),
    ie: AmongTable::build(&["ie", "ié", "iê"]),
    e: AmongTable::build(&["e", "é", "ê"]),
});

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
        let tb = &*TABLES;
        let mut w = units(&word.to_lowercase());

        // --- Prelude: nasal vowels become vowel + '~' ----------------------
        replace_all(&mut w, &[0x00E3], &[0x0061, 0x007E]); // ã -> a~
        replace_all(&mut w, &[0x00F5], &[0x006F, 0x007E]); // õ -> o~

        let r1 = mark_region_n(&w, 0);
        let r2 = mark_region_n(&w, r1);
        let rv = mark_region_v(&w);

        // --- Step 1: standard suffixes (nine chained calls) ----------------
        let mut changed = replace_in_region(&mut w, &tb.noun, "", r2);
        changed |= replace_in_region(&mut w, &tb.logia, "log", r2);
        // The reference has a commented-out `['uço~es', 'uça~o'] -> 'u'` call
        // here; it is commented out there and absent here.
        changed |= replace_in_region(&mut w, &tb.encia, "ente", r2);
        changed |= replace_in_region(&mut w, &tb.mente2, "", r2);
        // `['amente'] -> '' in R1` — a single literal, checked directly.
        if ends_with(&w, "amente") && w.len().saturating_sub(6) >= r1 {
            let keep = w.len() - 6;
            w.truncate(keep);
            changed = true;
        }
        changed |= replace_in_region(&mut w, &tb.mente, "", r2);
        changed |= replace_in_region(&mut w, &tb.idade, "", r2);
        changed |= replace_in_region(&mut w, &tb.ivo, "", r2);
        if ends_with(&w, "eiras") || ends_with(&w, "eira") {
            changed |= replace_in_region(&mut w, &tb.ira, "ir", rv);
        }
        let step1_changed = changed;

        // --- Step 2: verb suffixes, only if step 1 changed nothing ---------
        if !step1_changed {
            changed = replace_in_region(&mut w, &tb.verb, "", rv);
        }

        // --- Step 3 or 4 ---------------------------------------------------
        if !changed {
            replace_in_region(&mut w, &tb.residual, "", rv);
        } else if ends_with(&w, "ci") && w.len().saturating_sub(1) >= rv {
            // `['i'] -> '' in RV`, gated on a preceding `c`.
            let keep = w.len() - 1;
            w.truncate(keep);
        }

        // --- Step 5: residual form ----------------------------------------
        let mut step5_changed = false;
        if ends_with(&w, "gue") || ends_with(&w, "gué") || ends_with(&w, "guê") {
            step5_changed |= replace_in_region(&mut w, &tb.ue, "", rv);
        }
        if ends_with(&w, "cie") || ends_with(&w, "cié") || ends_with(&w, "ciê") {
            step5_changed |= replace_in_region(&mut w, &tb.ie, "", rv);
        }
        if !step5_changed {
            replace_in_region(&mut w, &tb.e, "", rv);
        }
        // `['ç'] -> 'c'` in region `all` (0), so this one is unconditional.
        if w.last() == Some(&0x00E7) {
            let keep = w.len() - 1;
            w.truncate(keep);
            w.push(0x63);
        }

        // --- Postlude ------------------------------------------------------
        replace_all(&mut w, &[0x0061, 0x007E], &[0x00E3]);
        replace_all(&mut w, &[0x006F, 0x007E], &[0x00F5]);
        Cow::Owned(text(&w))
    }
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

    /// Portuguese is a **first-match** language; routing its tables through
    /// the longest-match search is only sound while every nested pair is
    /// ordered longest-first. See the module docs.
    #[test]
    fn tables_are_ordered_longest_first_within_nests() {
        for (name, table) in [
            ("STEP1_NOUN", STEP1_NOUN),
            ("logia", &["logias", "logia"] as &[&str]),
            ("encia", &["ências", "ência"]),
            (
                "mente2",
                &["ativamente", "icamente", "ivamente", "osamente", "adamente"],
            ),
            ("mente", &["antemente", "avelmente", "ivelmente", "mente"]),
            ("STEP1_IDADE", STEP1_IDADE),
            ("STEP1_IVO", STEP1_IVO),
            ("ira", &["iras", "ira"]),
            ("VERB", VERB),
            ("RESIDUAL", RESIDUAL),
            ("ue", &["ue", "ué", "uê"]),
            ("ie", &["ie", "ié", "iê"]),
            ("e", &["e", "é", "ê"]),
        ] {
            crate::among::nested_pairs_are_longest_first(name, table);
        }
    }

    // -----------------------------------------------------------------------
    // Differential oracle: the pre-find_among implementation, verbatim
    // (the `Token`-shaped port with linear first-match scans).
    // -----------------------------------------------------------------------
    mod oracle {
        use super::super::*;
        use crate::units::{ends_with, replace_suffix, slen};

        struct PtToken {
            w: Vec<u16>,
            r1: usize,
            r2: usize,
            rv: usize,
        }

        impl PtToken {
            fn has_suffix(&self, suffix: &str) -> bool {
                ends_with(&self.w, suffix)
            }

            fn has_suffix_in_region(&self, suffix: &str, region: usize) -> bool {
                self.has_suffix(suffix) && self.w.len().saturating_sub(slen(suffix)) >= region
            }

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

        fn standard_suffix(t: &mut PtToken) -> bool {
            let mut changed = t.replace_suffix_in_region(STEP1_NOUN, "", t.r2);
            changed |= t.replace_suffix_in_region(&["logias", "logia"], "log", t.r2);
            changed |= t.replace_suffix_in_region(&["ências", "ência"], "ente", t.r2);
            changed |= t.replace_suffix_in_region(
                &["ativamente", "icamente", "ivamente", "osamente", "adamente"],
                "",
                t.r2,
            );
            changed |= t.replace_suffix_in_region(&["amente"], "", t.r1);
            changed |= t.replace_suffix_in_region(
                &["antemente", "avelmente", "ivelmente", "mente"],
                "",
                t.r2,
            );
            changed |= t.replace_suffix_in_region(STEP1_IDADE, "", t.r2);
            changed |= t.replace_suffix_in_region(STEP1_IVO, "", t.r2);
            if t.has_suffix("eiras") || t.has_suffix("eira") {
                changed |= t.replace_suffix_in_region(&["iras", "ira"], "ir", t.rv);
            }
            changed
        }

        pub(super) fn stem(word: &str) -> String {
            let mut w = units(&word.to_lowercase());

            replace_all(&mut w, &[0x00E3], &[0x0061, 0x007E]);
            replace_all(&mut w, &[0x00F5], &[0x006F, 0x007E]);

            let mut t = PtToken {
                w,
                r1: 0,
                r2: 0,
                rv: 0,
            };
            t.r1 = mark_region_n(&t.w, 0);
            t.r2 = mark_region_n(&t.w, t.r1);
            t.rv = mark_region_v(&t.w);

            let step1_changed = standard_suffix(&mut t);

            let mut changed = step1_changed;
            if !step1_changed {
                changed = t.replace_suffix_in_region(VERB, "", t.rv);
            }

            if !changed {
                t.replace_suffix_in_region(RESIDUAL, "", t.rv);
            } else if t.has_suffix("ci") {
                t.replace_suffix_in_region(&["i"], "", t.rv);
            }

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
            t.replace_suffix_in_region(&["ç"], "c", 0);

            let mut w = t.w;
            replace_all(&mut w, &[0x0061, 0x007E], &[0x00E3]);
            replace_all(&mut w, &[0x006F, 0x007E], &[0x00F5]);
            text(&w)
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

    /// Portuguese stems crossed with real table suffixes (stacked up to two
    /// deep — step 1 chains, so stacking matters here more than anywhere),
    /// nasal vowels for the detour, and case/astral/CJK noise.
    fn random_word(rng: &mut Rng) -> String {
        const ALPHA: &[char] = &[
            'a', 'b', 'c', 'd', 'e', 'f', 'g', 'i', 'l', 'm', 'n', 'o', 'p', 'r', 's', 't', 'u',
            'v', 'z', 'ã', 'õ', 'ç', 'á', 'é', 'ê', 'ô',
        ];
        const SUFFIXES: &[&str] = &[
            "amentos",
            "ações",
            "adoras",
            "ância",
            "ável",
            "ezas",
            "logias",
            "ências",
            "ativamente",
            "amente",
            "mente",
            "abilidades",
            "icidade",
            "ividades",
            "idades",
            "ativas",
            "ivos",
            "eiras",
            "eira",
            "iras",
            "ira",
            "aríamos",
            "ássemos",
            "ara~o",
            "ão",
            "ãos",
            "ando",
            "endo",
            "indo",
            "ou",
            "iu",
            "ia",
            "ei",
            "em",
            "er",
            "ir",
            "os",
            "a",
            "i",
            "o",
            "á",
            "í",
            "ó",
            "gue",
            "gué",
            "cie",
            "ciê",
            "e",
            "é",
            "ê",
            "ç",
            "ci",
        ];
        let mut s = String::new();
        for _ in 0..rng.below(8) {
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
            4 => s.push('~'),
            _ => {}
        }
        s
    }

    #[test]
    fn differential_against_the_linear_scan_oracle() {
        let stemmer = PorterStemmerPt::new();
        let check = |input: &str| {
            assert_eq!(
                stemmer.stem(input).as_ref(),
                oracle::stem(input),
                "stem({input:?})"
            );
        };
        for w in crate::test_support::bench_words("pt") {
            check(&w);
        }
        for w in [
            "",
            "a",
            "ã",
            "õ",
            "ãõãõ",
            "çç",
            "coração",
            "ações",
            "abilidades",
            "eiras",
            "logias",
            "guê",
            "ciê",
            "amente",
            "camente",
            "ci",
            "aci",
        ] {
            check(w);
        }
        let mut rng = Rng(0xBEEF_5A5A_1357_9BDF);
        for _ in 0..60_000 {
            let w = random_word(&mut rng);
            check(&w);
        }
    }
}
