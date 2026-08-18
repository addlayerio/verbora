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
//! `stem` still routes every table through one [`crate::among`] longest-match
//! binary search (`docs/PERFORMANCE_GAPS.md` entry 34): in every shipped
//! table, whenever one entry is a proper suffix of another the longer one is
//! listed first, so first-listed and longest coincide — the region limit
//! excludes an entry exactly when the sliced `endsinArr` would fail to see it.
//! That table property is pinned by
//! `tables_are_ordered_longest_first_within_nests` below, and the
//! pre-conversion implementation is kept in the tests as the byte-exactness
//! oracle.
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
use std::sync::LazyLock;

use verbora_tokenizers::classes;

use crate::among::AmongTable;
use crate::base::{Casing, TokenizeAndStem};
use crate::data::gates::gate_it;
use crate::stopwords::{self, Language};
use crate::units::{ends_with, text, text_lowercase, u, units};

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

fn cut(w: &mut Vec<u16>, n: usize, tail: &str) {
    w.truncate(w.len().saturating_sub(n));
    w.extend(tail.encode_utf16());
}

/// The lowercased, acute-to-grave, `qU`/`I`/`U`-marked form of `t` — the
/// reference's prelude, shared by `stem` and the test oracle.
fn prelude(t: &mut [u16]) {
    // Acute accents become grave ones (`/á/gi` etc., so every occurrence).
    for c in t.iter_mut() {
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
}

/// `(r1, r2, rv)` exactly as the reference marks them, for `t.len() >= 3`.
fn mark_regions(t: &[u16]) -> (usize, usize, usize) {
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
    (r1, r2, rv)
}

/// The sorted search tables, built once from the ordered rule tables below.
struct ItTables {
    pronoun: AmongTable,
    /// `["ando", "endo"]` — the pre-pronoun gerund test.
    ando_endo: AmongTable,
    /// `["ar", "er", "ir"]` — the pre-pronoun infinitive test.
    ar_er_ir: AmongTable,
    amente: AmongTable,
    azione: AmongTable,
    logia: AmongTable,
    uzione: AmongTable,
    enza: AmongTable,
    amento: AmongTable,
    atrice: AmongTable,
    ita: AmongTable,
    icativa: AmongTable,
    step2: AmongTable,
    step3: AmongTable,
}

static TABLES: LazyLock<ItTables> = LazyLock::new(|| ItTables {
    pronoun: AmongTable::build(PRONOUN),
    ando_endo: AmongTable::build(&["ando", "endo"]),
    ar_er_ir: AmongTable::build(&["ar", "er", "ir"]),
    amente: AmongTable::build(STEP1_AMENTE),
    azione: AmongTable::build(STEP1_AZIONE),
    logia: AmongTable::build(&["logia", "logie"]),
    uzione: AmongTable::build(&["uzione", "uzioni", "usione", "usioni"]),
    enza: AmongTable::build(&["enza", "enze"]),
    amento: AmongTable::build(&["amento", "amenti", "imento", "imenti"]),
    atrice: AmongTable::build(STEP1_ATRICE),
    ita: AmongTable::build(&["abilità", "icità", "ività", "ità"]),
    icativa: AmongTable::build(STEP1_ICATIVA),
    step2: AmongTable::build(STEP2),
    step3: AmongTable::build(STEP3),
});

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
    pub fn stem<'a>(&self, token: &'a str) -> Cow<'a, str> {
        let tb = &*TABLES;
        let mut t = units(&token.to_lowercase());

        // --- Prelude -------------------------------------------------------
        prelude(&mut t);
        if t.len() < 3 {
            // Already lowercased, acute-replaced and qU-marked.
            return Cow::Owned(text(&t));
        }

        // --- Regions -------------------------------------------------------
        let (r1, r2, rv) = mark_regions(&t);

        // --- Step 0: attached pronoun --------------------------------------
        let len = t.len();
        let i = tb.pronoun.find(&t, len, 0);
        if i >= 0 {
            let n = tb.pronoun.entries[i as usize].0.len();
            let start = rv.min(len);
            let head_end = start + (len - start).saturating_sub(n);
            // Two consecutive `if`s in the reference, not an if/else. The two
            // lists are disjoint, so a double truncation cannot actually happen.
            let pre1 = tb.ando_endo.longest(&t, head_end, start) > 0;
            let pre2 = tb.ar_er_ir.longest(&t, head_end, start) > 0;
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
        // One `find_among` search per table of the reference's else-if chain,
        // each limited to its region via `lb`.
        let len = t.len();
        let lb2 = r2.min(len);
        let lb1 = r1.min(len);
        let lbv = rv.min(len);
        // Expressed as a labeled block with one early exit per rule: the
        // first rule whose (region-limited) search hits fires and ends the
        // step, which is exactly the reference's else-if ladder.
        let mut step1_changed = true;
        'step1: {
            let m = tb.amente.longest(&t, len, lb2);
            if m > 0 {
                cut(&mut t, m, "");
                break 'step1;
            }
            let m = tb.azione.longest(&t, len, lb2);
            if m > 0 {
                cut(&mut t, m, "");
                break 'step1;
            }
            let m = tb.logia.longest(&t, len, lb2);
            if m > 0 {
                cut(&mut t, m, "log");
                break 'step1;
            }
            let m = tb.uzione.longest(&t, len, lb2);
            if m > 0 {
                cut(&mut t, m, "u");
                break 'step1;
            }
            let m = tb.enza.longest(&t, len, lb2);
            if m > 0 {
                cut(&mut t, m, "ente");
                break 'step1;
            }
            let m = tb.amento.longest(&t, len, lbv);
            if m > 0 {
                cut(&mut t, m, "");
                break 'step1;
            }
            // `['amente']` in R1 — a single literal, checked directly.
            if ends_with(&t, "amente") && len - 6 >= lb1 {
                cut(&mut t, 6, "");
                break 'step1;
            }
            let m = tb.atrice.longest(&t, len, lb2);
            if m > 0 {
                cut(&mut t, m, "");
                break 'step1;
            }
            let m = tb.ita.longest(&t, len, lb2);
            if m > 0 {
                cut(&mut t, m, "");
                break 'step1;
            }
            let m = tb.icativa.longest(&t, len, lb2);
            if m > 0 {
                cut(&mut t, m, "");
                break 'step1;
            }
            step1_changed = false;
        }

        // --- Step 2: verb suffixes, only if step 1 changed nothing ---------
        if !step1_changed {
            let len = t.len();
            let m = tb.step2.longest(&t, len, rv.min(len));
            if m > 0 {
                cut(&mut t, m, "");
            }
        }

        // --- Step 3: vowel suffix, always ----------------------------------
        {
            let len = t.len();
            let m = tb.step3.longest(&t, len, rv.min(len));
            if m > 0 {
                cut(&mut t, m, "");
            }
        }

        let len = t.len();
        let lbv = rv.min(len);
        if ends_with(&t[lbv..], "ch") {
            cut(&mut t, 2, "c");
        } else if ends_with(&t[lbv..], "gh") {
            cut(&mut t, 2, "g");
        }

        Cow::Owned(text_lowercase(&mut t))
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

    /// Italian is a **first-match** language; routing its tables through the
    /// longest-match search is only sound while every nested pair is ordered
    /// longest-first. See the module docs.
    #[test]
    fn tables_are_ordered_longest_first_within_nests() {
        for (name, table) in [
            ("PRONOUN", PRONOUN),
            ("STEP1_AMENTE", STEP1_AMENTE),
            ("STEP1_AZIONE", STEP1_AZIONE),
            ("logia", &["logia", "logie"] as &[&str]),
            ("uzione", &["uzione", "uzioni", "usione", "usioni"]),
            ("enza", &["enza", "enze"]),
            ("amento", &["amento", "amenti", "imento", "imenti"]),
            ("STEP1_ATRICE", STEP1_ATRICE),
            ("ita", &["abilità", "icità", "ività", "ità"]),
            ("STEP1_ICATIVA", STEP1_ICATIVA),
            ("STEP2", STEP2),
            ("STEP3", STEP3),
        ] {
            crate::among::nested_pairs_are_longest_first(name, table);
        }
    }

    // -----------------------------------------------------------------------
    // Differential oracle: the pre-find_among implementation, verbatim
    // (the linear first-match scans over region slices).
    // -----------------------------------------------------------------------
    mod oracle {
        use super::super::*;
        use crate::units::{first_suffix, slen};

        fn from(w: &[u16], at: usize) -> &[u16] {
            &w[at.min(w.len())..]
        }

        pub(super) fn stem(token: &str) -> String {
            let mut t = units(&token.to_lowercase());

            prelude(&mut t);
            if t.len() < 3 {
                return text(&t);
            }

            let (r1, r2, rv) = mark_regions(&t);

            if let Some(suf) = first_suffix(&t, PRONOUN) {
                let n = slen(suf);
                let rv_slice = from(&t, rv);
                let head = &rv_slice[..rv_slice.len().saturating_sub(n)];
                let pre1 = first_suffix(head, &["ando", "endo"]).is_some();
                let pre2 = first_suffix(head, &["ar", "er", "ir"]).is_some();
                if pre1 {
                    cut(&mut t, n, "");
                }
                if pre2 {
                    cut(&mut t, n, "e");
                }
            }

            let mut step1_changed = false;
            if let Some(sfx) = first_suffix(from(&t, r2), STEP1_AMENTE) {
                cut(&mut t, slen(sfx), "");
                step1_changed = true;
            } else if let Some(sfx) = first_suffix(from(&t, r2), STEP1_AZIONE) {
                cut(&mut t, slen(sfx), "");
                step1_changed = true;
            } else if let Some(sfx) = first_suffix(from(&t, r2), &["logia", "logie"]) {
                cut(&mut t, slen(sfx), "log");
                step1_changed = true;
            } else if let Some(sfx) =
                first_suffix(from(&t, r2), &["uzione", "uzioni", "usione", "usioni"])
            {
                cut(&mut t, slen(sfx), "u");
                step1_changed = true;
            } else if let Some(sfx) = first_suffix(from(&t, r2), &["enza", "enze"]) {
                cut(&mut t, slen(sfx), "ente");
                step1_changed = true;
            } else if let Some(sfx) =
                first_suffix(from(&t, rv), &["amento", "amenti", "imento", "imenti"])
            {
                cut(&mut t, slen(sfx), "");
                step1_changed = true;
            } else if let Some(sfx) = first_suffix(from(&t, r1), &["amente"]) {
                cut(&mut t, slen(sfx), "");
                step1_changed = true;
            } else if let Some(sfx) = first_suffix(from(&t, r2), STEP1_ATRICE) {
                cut(&mut t, slen(sfx), "");
                step1_changed = true;
            } else if let Some(sfx) =
                first_suffix(from(&t, r2), &["abilità", "icità", "ività", "ità"])
            {
                cut(&mut t, slen(sfx), "");
                step1_changed = true;
            } else if let Some(sfx) = first_suffix(from(&t, r2), STEP1_ICATIVA) {
                cut(&mut t, slen(sfx), "");
                step1_changed = true;
            }

            if !step1_changed && let Some(sfx) = first_suffix(from(&t, rv), STEP2) {
                cut(&mut t, slen(sfx), "");
            }

            if let Some(sfx) = first_suffix(from(&t, rv), STEP3) {
                cut(&mut t, slen(sfx), "");
            }

            if ends_with(from(&t, rv), "ch") {
                cut(&mut t, 2, "c");
            } else if ends_with(from(&t, rv), "gh") {
                cut(&mut t, 2, "g");
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

    /// Italian stems crossed with real table suffixes (stacked up to two
    /// deep, pronouns included), acute accents for the prelude, `qu`, and
    /// case/astral/CJK noise.
    fn random_word(rng: &mut Rng) -> String {
        const ALPHA: &[char] = &[
            'a', 'b', 'c', 'd', 'e', 'f', 'g', 'i', 'l', 'm', 'n', 'o', 'p', 'q', 'r', 's', 't',
            'u', 'v', 'z', 'à', 'è', 'ì', 'ò', 'ù', 'á', 'é', 'í', 'ó', 'ú',
        ];
        const SUFFIXES: &[&str] = &[
            "glieli",
            "gliene",
            "sene",
            "cela",
            "vene",
            "mene",
            "tela",
            "gli",
            "ci",
            "la",
            "ne",
            "ando",
            "endo",
            "ar",
            "er",
            "ir",
            "ativamente",
            "icamente",
            "icazione",
            "azioni",
            "atore",
            "logia",
            "uzione",
            "usioni",
            "enza",
            "amento",
            "imenti",
            "amente",
            "atrice",
            "abile",
            "mente",
            "anza",
            "ismo",
            "ista",
            "abilità",
            "icità",
            "ità",
            "icativa",
            "ativo",
            "iva",
            "erebbero",
            "assero",
            "iscono",
            "avamo",
            "iamo",
            "Yamo",
            "ita",
            "ono",
            "are",
            "erà",
            "ia",
            "ie",
            "iò",
            "a",
            "e",
            "i",
            "o",
            "à",
            "ch",
            "gh",
            "chi",
            "ghi",
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
            _ => {}
        }
        s
    }

    #[test]
    fn differential_against_the_linear_scan_oracle() {
        let stemmer = PorterStemmerIt::new();
        let check = |input: &str| {
            assert_eq!(
                stemmer.stem(input).as_ref(),
                oracle::stem(input),
                "stem({input:?})"
            );
        };
        for w in crate::test_support::bench_words("it") {
            check(&w);
        }
        for w in [
            "",
            "a",
            "qu",
            "QU",
            "acqua",
            "perché",
            "città",
            "gli",
            "abbandonandoglieli",
            "mangiandola",
            "aiaia",
            "poiché",
            "amiche",
            "laghi",
        ] {
            check(w);
        }
        let mut rng = Rng(0x17A1_1A57_EA11_AB1E);
        for _ in 0..60_000 {
            let w = random_word(&mut rng);
            check(&w);
        }
    }
}
