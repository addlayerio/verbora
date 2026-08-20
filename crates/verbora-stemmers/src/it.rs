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
//!
//! # The text unit
//!
//! Every position here is a **Unicode scalar value**: R1, R2 and RV, the
//! `length < 3` gate the prelude runs ahead of, the `len > 3` guard on RV, the
//! literal `rv = 3`, and every cut. See [`crate::units`] for why that is the
//! faithful reading of a Snowball algorithm rather than a preference — the
//! specification is written over *letters*, and the UTF-16 indices this port
//! used to carry were an artefact of the host language it was transcribed
//! through, not of the algorithm. `stem("😀eato")` is `"😀eat"`: five letters,
//! so RV starts at 3 and leaves two of them, which the three-letter step-2
//! entry `-ato` does not fit.

use std::borrow::Cow;
use std::sync::LazyLock;

use crate::among::{AmongTable, Buf, UnionTable, longest_at_most};
use crate::base::{Casing, TokenizeAndStem};
use crate::data::gates::gate_it;
use crate::stopwords::Language;
use crate::units::{ends_with, text_lowercase};

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

/// `a e i o u à è ì ò ù` — lowercase only, so the marked `I`/`U` are consonants,
/// and so is every character outside the set, astral ones included.
#[inline]
fn is_vowel(c: char) -> bool {
    matches!(c, 'a' | 'e' | 'i' | 'o' | 'u' | 'à' | 'è' | 'ì' | 'ò' | 'ù')
}

fn cut(buf: &mut Buf<char>, n: usize, tail: &str) {
    buf.truncate(buf.len().saturating_sub(n));
    buf.push_str(tail);
}

/// The lowercased, acute-to-grave, `qU`/`I`/`U`-marked form of `t` — the
/// reference's prelude, shared by `stem` and the test oracle.
///
/// One character in, one character out at every position, which is why it can
/// run in place on the working buffer.
fn prelude(t: &mut [char]) {
    // Acute accents become grave ones (`/á/gi` etc., so every occurrence).
    for c in t.iter_mut() {
        match *c {
            'á' => *c = 'à',
            'é' => *c = 'è',
            'í' => *c = 'ì',
            'ó' => *c = 'ò',
            'ú' => *c = 'ù',
            _ => {}
        }
    }
    let mut i = 0;
    while i + 1 < t.len() {
        if t[i] == 'q' && t[i + 1] == 'u' {
            t[i + 1] = 'U';
            i += 2;
        } else {
            i += 1;
        }
    }
    // `/([aeiou])(i|u)([aeiou])/g` — non-overlapping, so "aiaia" is "aIaia".
    let mut i = 0;
    while i + 2 < t.len() {
        if is_vowel(t[i]) && (t[i + 1] == 'i' || t[i + 1] == 'u') && is_vowel(t[i + 2]) {
            // The guard has just established that this is `i` or `u`, so the
            // ASCII fold is the reference's `charCodeAt - 32` exactly.
            t[i + 1] = t[i + 1].to_ascii_uppercase();
            i += 3;
        } else {
            i += 1;
        }
    }
}

/// `(r1, r2, rv)` exactly as the reference marks them, in scalar values, for
/// `t.len() >= 3`.
fn mark_regions(t: &[char]) -> (usize, usize, usize) {
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
    /// Every step-1 rule table merged into one search; the `T_*` constants
    /// name each table's slot in the mask array.
    step1: UnionTable<char>,
    pronoun: AmongTable<char>,
    /// `["ando", "endo"]` — the pre-pronoun gerund test.
    ando_endo: AmongTable<char>,
    /// `["ar", "er", "ir"]` — the pre-pronoun infinitive test.
    ar_er_ir: AmongTable<char>,
    step2: AmongTable<char>,
    step3: AmongTable<char>,
}

/// The step-1 rule tables, in chain order; their positions are the `T_*`
/// constants and the ids [`UnionTable::length_masks`] writes.
static STEP1_TABLE_LIST: &[&[&str]] = &[
    STEP1_AMENTE,
    STEP1_AZIONE,
    LOGIA,
    UZIONE,
    ENZA,
    AMENTO,
    AMENTE_LIT,
    STEP1_ATRICE,
    ITA,
    STEP1_ICATIVA,
];
/// The number of step-1 tables, and so the size of the mask array.
const STEP1_TABLES: usize = 10;
const T_AMENTE: usize = 0;
const T_AZIONE: usize = 1;
const T_LOGIA: usize = 2;
const T_UZIONE: usize = 3;
const T_ENZA: usize = 4;
const T_AMENTO: usize = 5;
const T_AMENTE_LIT: usize = 6;
const T_ATRICE: usize = 7;
const T_ITA: usize = 8;
const T_ICATIVA: usize = 9;

/// `["ando", "endo"]` — the pre-pronoun gerund test.
static ANDO_ENDO: &[&str] = &["ando", "endo"];
/// `["ar", "er", "ir"]` — the pre-pronoun infinitive test.
static AR_ER_IR: &[&str] = &["ar", "er", "ir"];
/// The two digraphs the final rule undoes.
static CH_GH: &[&str] = &["ch", "gh"];
static LOGIA: &[&str] = &["logia", "logie"];
static UZIONE: &[&str] = &["uzione", "uzioni", "usione", "usioni"];
static ENZA: &[&str] = &["enza", "enze"];
static AMENTO: &[&str] = &["amento", "amenti", "imento", "imenti"];
/// `['amente']` in R1 — one literal, but a chain step of its own.
static AMENTE_LIT: &[&str] = &["amente"];
static ITA: &[&str] = &["abilità", "icità", "ività", "ità"];

static TABLES: LazyLock<ItTables> = LazyLock::new(|| ItTables {
    step1: UnionTable::build(STEP1_TABLE_LIST),
    pronoun: AmongTable::build(PRONOUN),
    ando_endo: AmongTable::build(ANDO_ENDO),
    ar_er_ir: AmongTable::build(AR_ER_IR),
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
        reason = "every stemmer is zero-sized; `stem` is a method so the \
                  sixteen of them share one call shape"
    )]
    pub fn stem<'a>(&self, token: &'a str) -> Cow<'a, str> {
        let tb = &*TABLES;
        let mut t: Buf<char> = Buf::fill_lowercase(token);

        // --- Prelude -------------------------------------------------------
        prelude(t.as_mut_slice());
        if t.len() < 3 {
            // Already lowercased, acute-replaced and qU-marked.
            return Cow::Owned(t.into_text());
        }

        // --- Regions -------------------------------------------------------
        let (r1, r2, rv) = mark_regions(t.as_slice());

        // --- Step 0: attached pronoun --------------------------------------
        let len = t.len();
        let i = tb.pronoun.find(t.as_slice(), len, 0);
        if i >= 0 {
            let n = tb.pronoun.len_at(i);
            let start = rv.min(len);
            let head_end = start + (len - start).saturating_sub(n);
            // Two consecutive `if`s in the reference, not an if/else. The two
            // lists are disjoint, so a double truncation cannot actually happen.
            let pre1 = tb.ando_endo.longest(t.as_slice(), head_end, start) > 0;
            let pre2 = tb.ar_er_ir.longest(t.as_slice(), head_end, start) > 0;
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
        // One union search answers every step-1 table at once: the walk of
        // its substring links records, per rule table, the lengths of that
        // table's entries that are suffixes of the word, and each rule reads
        // its own region-restricted longest match out of that mask. The
        // ladder below is otherwise the reference's, unchanged and in order.
        let mut lm = [0u32; STEP1_TABLES];
        tb.step1.length_masks(t.as_slice(), len, &mut lm);
        let (av1, av2, avv) = (len - lb1, len - lb2, len - lbv);
        let mut step1_changed = true;
        'step1: {
            let m = longest_at_most(lm[T_AMENTE], av2);
            if m > 0 {
                cut(&mut t, m, "");
                break 'step1;
            }
            let m = longest_at_most(lm[T_AZIONE], av2);
            if m > 0 {
                cut(&mut t, m, "");
                break 'step1;
            }
            let m = longest_at_most(lm[T_LOGIA], av2);
            if m > 0 {
                cut(&mut t, m, "log");
                break 'step1;
            }
            let m = longest_at_most(lm[T_UZIONE], av2);
            if m > 0 {
                cut(&mut t, m, "u");
                break 'step1;
            }
            let m = longest_at_most(lm[T_ENZA], av2);
            if m > 0 {
                cut(&mut t, m, "ente");
                break 'step1;
            }
            let m = longest_at_most(lm[T_AMENTO], avv);
            if m > 0 {
                cut(&mut t, m, "");
                break 'step1;
            }
            // `['amente']` in R1 — a single literal, but its own chain step.
            if longest_at_most(lm[T_AMENTE_LIT], av1) > 0 {
                cut(&mut t, 6, "");
                break 'step1;
            }
            let m = longest_at_most(lm[T_ATRICE], av2);
            if m > 0 {
                cut(&mut t, m, "");
                break 'step1;
            }
            let m = longest_at_most(lm[T_ITA], av2);
            if m > 0 {
                cut(&mut t, m, "");
                break 'step1;
            }
            let m = longest_at_most(lm[T_ICATIVA], av2);
            if m > 0 {
                cut(&mut t, m, "");
                break 'step1;
            }
            step1_changed = false;
        }

        // --- Step 2: verb suffixes, only if step 1 changed nothing ---------
        if !step1_changed {
            let len = t.len();
            let m = tb.step2.longest(t.as_slice(), len, rv.min(len));
            if m > 0 {
                cut(&mut t, m, "");
            }
        }

        // --- Step 3: vowel suffix, always ----------------------------------
        {
            let len = t.len();
            let m = tb.step3.longest(t.as_slice(), len, rv.min(len));
            if m > 0 {
                cut(&mut t, m, "");
            }
        }

        let len = t.len();
        let lbv = rv.min(len);
        if ends_with(&t.as_slice()[lbv..], CH_GH[0]) {
            cut(&mut t, 2, "c");
        } else if ends_with(&t.as_slice()[lbv..], CH_GH[1]) {
            cut(&mut t, 2, "g");
        }

        Cow::Owned(text_lowercase(t.as_mut_slice()))
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
/// The verb list.
///
/// It used to carry `"Yamo"`, which nothing could ever match: `stem`
/// lowercases the token before [`prelude`] runs, and `prelude` writes only
/// `I` and `U`, so no `Y` can be in the buffer when this table is searched.
/// Removing a suffix that cannot occur changes no stem, and
/// `no_capital_but_i_or_u_survives_the_prelude` pins the reason rather than
/// the removal.
static STEP2: &[&str] = &[
    "erebbero", "irebbero", "assero", "assimo", "eranno", "erebbe", "eremmo", "ereste", "eresti",
    "essero", "iranno", "irebbe", "iremmo", "ireste", "iresti", "iscano", "iscono", "issero",
    "arono", "avamo", "avano", "avate", "eremo", "erete", "erono", "evamo", "evano", "evate",
    "iremo", "irete", "irono", "ivamo", "ivano", "ivate", "ammo", "ando", "asse", "assi", "emmo",
    "enda", "ende", "endi", "endo", "erai", "iamo", "immo", "irai", "irei", "isca", "isce", "isci",
    "isco", "erei", "uti", "uto", "ita", "ite", "iti", "ito", "iva", "ivi", "ivo", "ono", "uta",
    "ute", "ano", "are", "ata", "ate", "ati", "ato", "ava", "avi", "avo", "erà", "ere", "erò",
    "ete", "eva", "evi", "evo", "irà", "ire", "irò", "ar", "ir",
];
static STEP3: &[&str] = &[
    "ia", "ie", "ii", "io", "ià", "iè", "iì", "iò", "a", "e", "i", "o", "à", "è", "ì", "ò",
];

impl TokenizeAndStem for PorterStemmerIt {
    const FILTER_ON: Casing = Casing::Raw;
    const STEM_ON: Casing = Casing::Lower;

    fn is_stop_word(word: &str) -> bool {
        Language::It.contains(word)
    }

    fn gate(token: &str) -> bool {
        token.chars().any(is_italian_letter)
    }

    fn stem_token(&self, token: &str) -> String {
        self.stem(token).into_owned()
    }
}

/// Whether `c` is one of the letters [`gate_it`] accepts.
///
/// The gate is stated over Basic Multilingual Plane code points and nothing in
/// it reaches `U+00FA`, so scanning characters and scanning UTF-16 code units
/// accept exactly the same tokens: a BMP character *is* its own code unit, and
/// an astral character is neither in the set itself nor are the two surrogates
/// it used to be scanned as. The scan is per character because that is the
/// crate's unit, not because the answer moved.
#[inline]
fn is_italian_letter(c: char) -> bool {
    (c as u32) < 0x1_0000 && gate_it(c as u16)
}

/// What [`crate::data::table_audit`] needs to walk this language's tables.
#[cfg(test)]
pub(crate) mod audit {
    use crate::among::Buf;

    /// Every rule table, named.
    pub(crate) static TABLES: &[(&str, &[&str])] = &[
        ("PRONOUN", super::PRONOUN),
        ("ANDO_ENDO", super::ANDO_ENDO),
        ("AR_ER_IR", super::AR_ER_IR),
        ("STEP1_AMENTE", super::STEP1_AMENTE),
        ("STEP1_AZIONE", super::STEP1_AZIONE),
        ("LOGIA", super::LOGIA),
        ("UZIONE", super::UZIONE),
        ("ENZA", super::ENZA),
        ("AMENTO", super::AMENTO),
        ("AMENTE_LIT", super::AMENTE_LIT),
        ("STEP1_ATRICE", super::STEP1_ATRICE),
        ("ITA", super::ITA),
        ("STEP1_ICATIVA", super::STEP1_ICATIVA),
        ("STEP2", super::STEP2),
        ("STEP3", super::STEP3),
        ("CH_GH", super::CH_GH),
    ];

    /// The prelude `stem` runs before any table is consulted, in isolation.
    pub(crate) fn prelude(token: &str) -> String {
        let mut t: Buf<char> = Buf::fill_lowercase(token);
        super::prelude(t.as_mut_slice());
        t.into_text()
    }

    /// The units the prelude writes, paired with what it writes them for.
    /// It writes `I` and `U` and nothing else — which is why an entry
    /// spelled with any other capital can never match.
    pub(crate) static MARKERS: &[(&str, &str)] = &[("I", "i"), ("U", "u")];
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

    /// The text unit is the **Unicode scalar value**, and Italian's region
    /// arithmetic makes the choice observable.
    ///
    /// Derived from the algorithm rather than recorded from it. `"😀eato"` is
    /// five letters — `😀 e a t o` — and the prelude leaves every one of them
    /// alone (no acute accent, no `qu`, and the `(vowel)(i|u)(vowel)` pattern
    /// needs an `i` or a `u` in the middle). `t[1]` is `e`, a vowel, so RV
    /// skips the first arm; `t[0]` is not a vowel, so it skips the second and
    /// takes the literal **`rv = 3`**. RV therefore leaves `5 - 3 = 2`
    /// letters, which the three-letter step-2 entry `-ato` does not fit, so
    /// step 2 declines and step 3's one-letter `-o` is what cuts: `"😀eat"`.
    ///
    /// `"ñeato"` is the control, and it is a control rather than a decoration:
    /// `ñ` is outside `isVowel` exactly as `😀` is, so the two words have the
    /// same letter classes in the same positions and the algorithm cannot tell
    /// them apart. Anything that makes their stems differ is the encoding
    /// leaking through.
    #[test]
    fn one_astral_character_is_one_letter() {
        assert_eq!(s("😀eato"), "😀eat");
        assert_eq!(s("ñeato"), "ñeat");
    }

    /// The two predicates that used to be stated over UTF-16 code units answer
    /// identically over characters, for every scalar value there is.
    ///
    /// This is what makes converting [`is_vowel`] and [`is_italian_letter`] a no-op
    /// rather than a change to be argued about: the vowel set tops out at
    /// `U+00F9` and `gate_it` at `U+00FA`, so neither an astral character
    /// nor either half of the surrogate pair it encodes to is ever admitted.
    /// Enumerated over the whole scalar range rather than sampled, because a
    /// set that reached into the surrogate range would fail on exactly the
    /// characters a spot check does not name.
    #[test]
    fn the_character_scans_agree_with_the_code_unit_scans() {
        let mut buf = [0u16; 2];
        for cp in 0..=0x10_FFFFu32 {
            let Some(c) = char::from_u32(cp) else {
                continue;
            };
            let units = c.encode_utf16(&mut buf);
            assert!(
                !is_vowel(c) || cp < 0x1_0000,
                "U+{cp:04X} vowel outside the plane the set covers"
            );
            assert_eq!(
                is_italian_letter(c),
                units.iter().any(|u| gate_it(*u)),
                "U+{cp:04X} gate"
            );
        }
    }

    /// A character that is inert for this algorithm and inside the Basic
    /// Multilingual Plane: not in `isVowel`, not spelled in any rule table,
    /// its own lower case, and untouched by the acute-to-grave and `qU`/`I`/`U`
    /// marking of the prelude.
    const INERT_TWIN: char = 'ж';

    /// Every entry of the Italian stop-word list and of every Italian rule
    /// table, walked through `stem` with one astral character inserted at
    /// **every** position, against the same word carrying an inert
    /// Basic-Multilingual-Plane character instead.
    ///
    /// # What the twin proves, and why it needs no second implementation
    ///
    /// [`INERT_TWIN`] is inert for this algorithm in exactly the way an astral
    /// character is: neither is a vowel, neither is spelled in any rule table,
    /// neither is rewritten by the lower-casing or by the prelude's markings.
    /// So the only thing that can possibly distinguish the two words is **how
    /// long each of them is** — one character each under the contract, one and
    /// two under the code-unit reading this port used to carry. One build run
    /// over both therefore measures the unit directly, and a divergence here is
    /// a position that is still being counted in code units.
    ///
    /// # Why every entry and every position, rather than a sample
    ///
    /// This is the shape of defect that has already cost this crate 116 Swedish
    /// stop words and this module's own dead `"Yamo"` rule: a stage transforms
    /// text before a later stage measures or looks it up, and the entries that
    /// die are exactly the ones a spot check does not name. So the walk is over
    /// every entry of every table and of the stop-word list, behind every
    /// alphabetic entry of that same list — the probe construction
    /// [`crate::data::table_audit`] uses — and the astral character goes in at
    /// every position of each, including the two ends. The counts are pinned by
    /// equality so that a walk which quietly stops enumerating cannot report a
    /// clean sweep of nothing.
    ///
    /// # Red, then green
    ///
    /// Against the code-unit reading this port used to carry, this walk reports
    /// **677 of 1 473 028** probes measuring an astral character as more than one
    /// letter. It reports **0** here. Every one of the 677 has the same shape:
    /// a word that opens with a non-vowel and whose second letter is a vowel,
    /// so that RV takes its third arm and is the literal `rv = 3`. That 3 is an
    /// absolute position rather than a relative one, so the extra code unit
    /// lengthened the region past it, and a suffix that does not fit the
    /// region by one letter fitted it by one code unit.
    /// [`one_astral_character_is_one_letter`] states one such word
    /// with its arithmetic written out; this walk is what shows the shape is the
    /// only one, and that nothing else in the shipped data moved.
    #[test]
    fn every_shipped_entry_measures_the_same_under_either_unit() {
        let stemmer = PorterStemmerIt::new();
        let stops = Language::It.defaults();
        let fillers: Vec<&str> = std::iter::once("")
            .chain(
                stops
                    .iter()
                    .copied()
                    .filter(|w| !w.is_empty() && w.chars().all(char::is_alphabetic)),
            )
            .collect();
        let mut entries: Vec<&str> = audit::TABLES
            .iter()
            .flat_map(|(_, table)| table.iter().copied())
            .collect();
        assert_eq!(
            entries.len(),
            217,
            "the Italian rule-table entry count moved"
        );
        entries.extend(stops.iter().copied());
        assert_eq!(entries.len(), 507, "the Italian entry count moved");
        assert_eq!(fillers.len(), 280, "the Italian filler count moved");

        let twin = INERT_TWIN.to_string();
        let mut probes = 0usize;
        let mut diverging: Vec<String> = Vec::new();
        for entry in &entries {
            for filler in &fillers {
                let word: Vec<char> = format!("{filler}{entry}").chars().collect();
                for pos in 0..=word.len() {
                    let mut astral: String = word[..pos].iter().collect();
                    astral.push('\u{1F600}');
                    astral.extend(&word[pos..]);
                    let bmp = astral.replace('\u{1F600}', &twin);
                    probes += 1;
                    let from_astral = stemmer.stem(&astral).replace('\u{1F600}', &twin);
                    let from_bmp = stemmer.stem(&bmp).into_owned();
                    if from_astral != from_bmp {
                        diverging.push(format!("{astral:?}: {from_astral:?} vs {from_bmp:?}"));
                    }
                }
            }
        }
        assert!(
            diverging.is_empty(),
            "{} of {probes} probes measure an astral character as more than one \
             letter: {:#?}",
            diverging.len(),
            &diverging[..diverging.len().min(10)]
        );
        assert_eq!(
            probes, 1_473_028,
            "the number of probes this walk builds moved"
        );
    }

    /// `I` and `U` are the only characters the tables may be spelled with that
    /// are not already lower case — the invariant removing `"Yamo"` rests on.
    ///
    /// Checked rather than asserted, and enumerated rather than sampled.
    /// `Buf::fill_lowercase` runs `str::to_lowercase`, after which every
    /// character equals its own lower case; [`prelude`] then writes `I` and
    /// `U` and nothing else. So the set of characters in the buffer that
    /// *differ* from their own lower case must be exactly `{I, U}`, whatever
    /// the input. Every scalar value is fed through in four positions — alone,
    /// between two vowels (where `i` and `u` are marked), after a `q` (where
    /// `u` is) and interleaved with vowels — and the union is compared by
    /// equality.
    ///
    /// Stating it as "no capital survives" would be wrong rather than merely
    /// strict: `ℂ`, `𝐀` and `🄰` are upper case with no lower-case mapping at
    /// all, so they pass through unchanged and always have. None of them is a
    /// letter any Italian suffix is spelled with, and none is `Y`.
    #[test]
    fn only_i_and_u_reach_the_tables_in_upper_case() {
        use std::collections::BTreeSet;

        let mut unfolded: BTreeSet<char> = BTreeSet::new();
        for cp in 0..=0x10_FFFFu32 {
            let Some(c) = char::from_u32(cp) else {
                continue;
            };
            for probe in [
                c.to_string(),
                format!("a{c}a"),
                format!("q{c}"),
                format!("{c}a{c}a{c}"),
            ] {
                let mut buf: Buf<char> = Buf::fill_lowercase(&probe);
                prelude(buf.as_mut_slice());
                unfolded.extend(
                    buf.into_text()
                        .chars()
                        .filter(|c| !c.to_lowercase().eq(std::iter::once(*c))),
                );
            }
        }
        assert_eq!(
            unfolded.into_iter().collect::<Vec<_>>(),
            ['I', 'U'],
            "the Italian prelude emits a character the rule tables do not expect"
        );
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
        use crate::units::{first_suffix, slen, text};

        /// The pre-`Buf` `cut`, over the owned `Vec` the oracle still uses.
        fn cut(w: &mut Vec<char>, n: usize, tail: &str) {
            w.truncate(w.len().saturating_sub(n));
            w.extend(tail.chars());
        }

        fn from(w: &[char], at: usize) -> &[char] {
            &w[at.min(w.len())..]
        }

        pub(super) fn stem(token: &str) -> String {
            let mut t: Vec<char> = token.to_lowercase().chars().collect();

            prelude(&mut t);
            if t.len() < 3 {
                return text(&t);
            }

            let (r1, r2, rv) = mark_regions(&t);

            if let Some(suf) = first_suffix(&t, PRONOUN) {
                let n = slen(suf);
                let rv_slice = from(&t, rv);
                let head = &rv_slice[..rv_slice.len().saturating_sub(n)];
                let pre1 = first_suffix(head, ANDO_ENDO).is_some();
                let pre2 = first_suffix(head, AR_ER_IR).is_some();
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

            if ends_with(from(&t, rv), CH_GH[0]) {
                cut(&mut t, 2, "c");
            } else if ends_with(from(&t, rv), CH_GH[1]) {
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
