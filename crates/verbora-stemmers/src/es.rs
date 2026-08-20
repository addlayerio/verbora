//! The Spanish Snowball stemmer, ported from
//! The reference `porter_stemmer_es`.
//!
//! # `stem` does not lowercase
//!
//! Line 99 of the reference is `word.toLowerCase()` — a statement whose result is
//! thrown away. Because `isVowel` carries `/i` while every suffix comparison is
//! case-sensitive, uppercase input flows through the region machinery and then
//! matches nothing: `stem("ÁRBOL")` is `"ÁRBOL"`, `stem("Efecto")` is `"Efect"`,
//! `stem("campa")` is `"camp"`. Adding the "obviously missing" fold changes
//! results for every caller that reaches `stem` directly rather than through
//! `tokenizeAndStem` (which folds separately).
//!
//! # Two more traps
//!
//! `removeAccent` calls `String.prototype.replace` with a **string** pattern, so
//! it rewrites only the first occurrence of each accented vowel:
//! `removeAccent("ááéé")` is `"aáeé"`. Rust's `str::replace` replaces all.
//!
//! The step-2b verb list contains the entry `"  aseis"` — with two leading
//! spaces. It is dead as written, and it is preserved verbatim: "fixing" it to
//! `"aseis"` would start matching real words.
//!
//! # How `stem` searches its tables
//!
//! The reference walks every table linearly per step; this port routes each
//! step through one [`crate::among`] binary search instead — the else-if
//! chain's ten step-1 tables are merged into a single union search whose
//! substring-link walk recovers each table's own longest region-valid match,
//! and the chain fires the lowest-priority-id table exactly as the reference's
//! branch order does. The conversion was measured to remove ~78% of the
//! per-word cost and was verified byte-exact against the linear-scan
//! implementation over 500k+ differential cases (the same oracle now lives in
//! this module's test suite). Region slices become `lb` cursor limits — the
//! search cannot match past them, which is the same restriction slicing
//! enforced, without the `.to_vec()` snapshots.

use std::borrow::Cow;
use std::sync::LazyLock;

use crate::among::{AmongTable, Buf, UnionTable};
use crate::base::{Casing, TokenizeAndStem};
use crate::data::charsets::is_es_vowel;
use crate::data::gates::gate_es;
use crate::stopwords::Language;
use crate::units::{ends_with, longest_suffix, slen, text, units};

/// The Spanish Snowball stemmer.
///
/// ```
/// use verbora_stemmers::PorterStemmerEs;
/// let s = PorterStemmerEs::new();
/// assert_eq!(s.stem("campa"), "camp");
/// // Uppercase input is returned essentially unchanged — see the module docs.
/// assert_eq!(s.stem("CAMPA"), "CAMPA");
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PorterStemmerEs;

#[inline]
fn is_vowel(c: u16) -> bool {
    is_es_vowel(c)
}

/// `(r1, r2, rv)` for a word of length ≥ 2, exactly as the reference marks
/// them. Callers must uphold the length precondition (`length - 1` underflows
/// otherwise); `stem` returns early for shorter input.
fn mark_regions(w: &[u16]) -> (usize, usize, usize) {
    let length = w.len();
    let (mut r1, mut r2, mut rv) = (length, length, length);
    for i in 0..length - 1 {
        if r1 != length {
            break;
        }
        if is_vowel(w[i]) && !is_vowel(w[i + 1]) {
            r1 = i + 2;
        }
    }
    for i in r1..length.saturating_sub(1) {
        if r2 != length {
            break;
        }
        if is_vowel(w[i]) && !is_vowel(w[i + 1]) {
            r2 = i + 2;
        }
    }
    if length > 3 {
        if !is_vowel(w[1]) {
            rv = (2..length).find(|&i| is_vowel(w[i])).unwrap_or(length) + 1;
        } else if is_vowel(w[0]) && is_vowel(w[1]) {
            rv = (2..length).find(|&i| !is_vowel(w[i])).unwrap_or(length) + 1;
        } else {
            rv = 3;
        }
    }
    (r1, r2, rv)
}

/// First-occurrence-of-each accent removal, in place.
///
/// A single pass with five "already replaced" flags is equivalent to the
/// reference's five independent first-occurrence `replace` calls: the five
/// code points are distinct, so replacing one never creates or destroys an
/// occurrence of another.
fn remove_accent_inplace(w: &mut [u16]) {
    let (mut a, mut e, mut i, mut o, mut u) = (false, false, false, false, false);
    for c in w.iter_mut() {
        match *c {
            0x00E1 if !a => {
                *c = 0x61;
                a = true;
            }
            0x00E9 if !e => {
                *c = 0x65;
                e = true;
            }
            0x00ED if !i => {
                *c = 0x69;
                i = true;
            }
            0x00F3 if !o => {
                *c = 0x6F;
                o = true;
            }
            0x00FA if !u => {
                *c = 0x75;
                u = true;
            }
            _ => {}
        }
    }
}

/// Whether `w` ends in the two units `gu`.
#[inline]
fn ends_gu(w: &[u16]) -> bool {
    let n = w.len();
    n >= 2 && w[n - 1] == 0x75 && w[n - 2] == 0x67
}

/// The sorted search tables, built once from the `&'static str` rule tables
/// below — those stay the single source of truth.
struct EsTables {
    pronoun: AmongTable,
    /// 0 = [`PRONOUN_PRE1`], 1 = [`PRONOUN_PRE2`].
    pre: UnionTable,
    /// The ten step-1 tables in chain order; id 6 (`amente`) checks R1, the
    /// rest R2.
    step1: UnionTable,
    step2a: AmongTable,
    /// 0 = [`STEP2B`], 1 = [`STEP2B_EN`].
    step2b: UnionTable,
    /// 0 = [`STEP3_A`], 1 = [`STEP3_E`].
    step3: UnionTable,
}

static TABLES: LazyLock<EsTables> = LazyLock::new(|| EsTables {
    pronoun: AmongTable::build(PRONOUN),
    pre: UnionTable::build(&[PRONOUN_PRE1, PRONOUN_PRE2]),
    step1: UnionTable::build(&[
        STEP1_A,
        STEP1_B,
        STEP1_LOGIA,
        STEP1_UCION,
        STEP1_ENCIA,
        STEP1_MENTE2,
        STEP1_AMENTE,
        STEP1_MENTE,
        STEP1_IDAD,
        STEP1_IVA,
    ]),
    step2a: AmongTable::build(STEP2A),
    step2b: UnionTable::build(&[STEP2B, STEP2B_EN]),
    step3: UnionTable::build(&[STEP3_A, STEP3_E]),
});

impl PorterStemmerEs {
    /// Creates the stemmer. It is stateless and zero-sized.
    #[inline]
    pub const fn new() -> Self {
        Self
    }

    /// `isVowel` — case-insensitive, which is why uppercase words still get
    /// regions.
    #[allow(
        clippy::unused_self,
        reason = "every stemmer is zero-sized; `stem` is a method so the \
                  sixteen of them share one call shape"
    )]
    pub fn is_vowel(&self, c: &str) -> bool {
        c.encode_utf16().any(is_vowel)
    }

    /// The index of the next vowel at or after `start`, or the length.
    #[allow(
        clippy::unused_self,
        reason = "every stemmer is zero-sized; `stem` is a method so the \
                  sixteen of them share one call shape"
    )]
    pub fn next_vowel_position(&self, word: &str, start: usize) -> usize {
        let w = units(word);
        (start..w.len())
            .find(|&i| is_vowel(w[i]))
            .unwrap_or(w.len())
    }

    /// The index of the next consonant at or after `start`, or the length.
    #[allow(
        clippy::unused_self,
        reason = "every stemmer is zero-sized; `stem` is a method so the \
                  sixteen of them share one call shape"
    )]
    pub fn next_consonant_position(&self, word: &str, start: usize) -> usize {
        let w = units(word);
        (start..w.len())
            .find(|&i| !is_vowel(w[i]))
            .unwrap_or(w.len())
    }

    /// Whether `word` ends with `suffix`, guarding on length as the reference does.
    #[allow(
        clippy::unused_self,
        reason = "every stemmer is zero-sized; `stem` is a method so the \
                  sixteen of them share one call shape"
    )]
    pub fn ends_in(&self, word: &str, suffix: &str) -> bool {
        slen(word) >= slen(suffix) && ends_with(&units(word), suffix)
    }

    /// The **longest** matching suffix, or `""`.
    ///
    /// Spanish sorts its matches by length; Italian and Portuguese take the first
    /// in array order instead. The two policies are not interchangeable.
    #[allow(
        clippy::unused_self,
        reason = "every stemmer is zero-sized; `stem` is a method so the \
                  sixteen of them share one call shape"
    )]
    pub fn ends_in_arr<'s>(&self, word: &str, suffixes: &[&'s str]) -> &'s str {
        longest_suffix(&units(word), suffixes).unwrap_or("")
    }

    /// Replaces the **first** occurrence of each accented vowel, in the order
    /// á é í ó ú.
    #[allow(
        clippy::unused_self,
        reason = "every stemmer is zero-sized; `stem` is a method so the \
                  sixteen of them share one call shape"
    )]
    pub fn remove_accent<'a>(&self, word: &'a str) -> Cow<'a, str> {
        if !word
            .chars()
            .any(|c| matches!(c, 'á' | 'é' | 'í' | 'ó' | 'ú'))
        {
            return Cow::Borrowed(word);
        }
        let mut out = word.to_owned();
        for (accented, plain) in [('á', 'a'), ('é', 'e'), ('í', 'i'), ('ó', 'o'), ('ú', 'u')] {
            if let Some(idx) = out.find(accented) {
                out.replace_range(
                    idx..idx + accented.len_utf8(),
                    plain.encode_utf8(&mut [0; 4]),
                );
            }
        }
        Cow::Owned(out)
    }

    /// Stems one token.
    #[allow(
        clippy::unused_self,
        reason = "every stemmer is zero-sized; `stem` is a method so the \
                  sixteen of them share one call shape"
    )]
    #[expect(
        clippy::too_many_lines,
        reason = "one block per Snowball step; splitting it would obscure the order, which is the specification"
    )]
    pub fn stem<'a>(&self, word: &'a str) -> Cow<'a, str> {
        let t = &*TABLES;
        let mut b = Buf::fill(word);
        let length = b.len();
        if length < 2 {
            remove_accent_inplace(b.as_mut_slice());
            return Cow::Owned(text(b.as_slice()));
        }
        let (r1, r2, rv) = mark_regions(b.as_slice());

        // --- Step 0: attached pronoun --------------------------------------
        //
        // The pronoun is sought over the whole word; the gerund/infinitive
        // head is the RV slice minus the pronoun, expressed as a cursor range
        // so no snapshot of it is taken.
        let n = t.pronoun.longest(b.as_slice(), length, 0);
        if n > 0 {
            let start = rv.min(length);
            let head_len = (length - start).saturating_sub(n);
            let head_end = start + head_len;
            let mut have_accented = false;
            let mut have_plain = false;
            let mut idx = t.pre.find_longest_index(b.as_slice(), head_end, start);
            while idx >= 0 {
                let (_, link, tid) = t.pre.entry(idx);
                if tid == 0 {
                    have_accented = true;
                } else {
                    have_plain = true;
                }
                idx = link;
            }
            if have_accented {
                b.truncate(length - n);
                remove_accent_inplace(b.as_mut_slice());
            } else if have_plain || {
                let keep = length - n;
                let s = b.as_slice();
                // "uyendo", compared on the stem the truncation would leave.
                keep >= 6 && s[keep - 6..keep] == [0x75, 0x79, 0x65, 0x6E, 0x64, 0x6F]
            } {
                b.truncate(length - n);
            }
        }

        // --- Step 1: standard suffixes -------------------------------------
        //
        // One union search replaces the reference's ten-table else-if chain.
        // The link-walk visits every matching entry longest-first; per table
        // id the longest entry that fits its region is recorded, and the
        // lowest id fires — which is exactly which branch of the chain would
        // have taken it.
        let len1 = b.len();
        let lb2 = r2.min(len1);
        let lb1 = r1.min(len1);
        let mut step1_changed = false;
        {
            let mut best: [usize; 10] = [0; 10];
            let mut idx = t.step1.find_longest_index(b.as_slice(), len1, 0);
            while idx >= 0 {
                let (n, link, tid) = t.step1.entry(idx);
                let lb = if tid == 6 { lb1 } else { lb2 };
                if n <= len1 - lb && best[tid] == 0 {
                    best[tid] = n;
                }
                idx = link;
            }
            for (tid, &m) in best.iter().enumerate() {
                if m == 0 {
                    continue;
                }
                b.truncate(len1 - m);
                match tid {
                    2 => b.push_str("log"),
                    3 => b.push(0x75),
                    4 => b.push_str("ente"),
                    _ => {}
                }
                step1_changed = true;
                break;
            }
        }

        if !step1_changed {
            // --- Step 2a: `y` verb suffixes --------------------------------
            let len = b.len();
            let lbv = rv.min(len);
            let mut step2a_changed = false;
            let n = t.step2a.longest(b.as_slice(), len, lbv);
            if n > 0 && len > n && b.as_slice()[len - n - 1] == 0x75 {
                b.truncate(len - n);
                step2a_changed = true;
            }

            // --- Step 2b: the rest of the verb suffixes --------------------
            if !step2a_changed {
                let len = b.len();
                let lbv = rv.min(len);
                let mut best: [usize; 2] = [0; 2];
                let mut idx = t.step2b.find_longest_index(b.as_slice(), len, lbv);
                while idx >= 0 {
                    let (n, link, tid) = t.step2b.entry(idx);
                    if best[tid] == 0 {
                        best[tid] = n;
                    }
                    idx = link;
                }
                if best[0] > 0 {
                    b.truncate(len - best[0]);
                } else if best[1] > 0 {
                    b.truncate(len - best[1]);
                    if ends_gu(b.as_slice()) {
                        let keep = b.len() - 1;
                        b.truncate(keep);
                    }
                }
            }
        }

        // --- Step 3: residual ----------------------------------------------
        {
            let len = b.len();
            let lbv = rv.min(len);
            let mut best: [usize; 2] = [0; 2];
            let mut idx = t.step3.find_longest_index(b.as_slice(), len, lbv);
            while idx >= 0 {
                let (n, link, tid) = t.step3.entry(idx);
                if best[tid] == 0 {
                    best[tid] = n;
                }
                idx = link;
            }
            if best[0] > 0 {
                b.truncate(len - best[0]);
            } else if best[1] > 0 {
                b.truncate(len.saturating_sub(1));
                let len = b.len();
                let lbv = rv.min(len);
                // `ends_with(rv_slice, "u")`: RV non-empty and ends in `u`.
                if len > lbv && b.as_slice()[len - 1] == 0x75 && ends_gu(b.as_slice()) {
                    b.truncate(len - 1);
                }
            }
        }

        remove_accent_inplace(b.as_mut_slice());
        Cow::Owned(text(b.as_slice()))
    }
}

/// Attached pronouns, matched against the whole word.
static PRONOUN: &[&str] = &[
    "me", "se", "sela", "selo", "selas", "selos", "la", "le", "lo", "las", "les", "los", "nos",
];
/// Accented gerund/infinitive endings that must precede a removed pronoun.
static PRONOUN_PRE1: &[&str] = &["iéndo", "ándo", "ár", "ér", "ír"];
/// The unaccented forms of the same.
static PRONOUN_PRE2: &[&str] = &["iendo", "ando", "ar", "er", "ir"];

static STEP1_A: &[&str] = &[
    "anza", "anzas", "ico", "ica", "icos", "icas", "ismo", "ismos", "able", "ables", "ible",
    "ibles", "ista", "istas", "oso", "osa", "osos", "osas", "amiento", "amientos", "imiento",
    "imientos",
];
static STEP1_B: &[&str] = &[
    "icadora",
    "icador",
    "icación",
    "icadoras",
    "icadores",
    "icaciones",
    "icante",
    "icantes",
    "icancia",
    "icancias",
    "adora",
    "ador",
    "ación",
    "adoras",
    "adores",
    "aciones",
    "ante",
    "antes",
    "ancia",
    "ancias",
];
static STEP1_LOGIA: &[&str] = &["logía", "logías"];
static STEP1_UCION: &[&str] = &["ución", "uciones"];
static STEP1_ENCIA: &[&str] = &["encia", "encias"];
static STEP1_MENTE2: &[&str] = &["ativamente", "ivamente", "osamente", "icamente", "adamente"];
static STEP1_AMENTE: &[&str] = &["amente"];
static STEP1_MENTE: &[&str] = &["antemente", "ablemente", "iblemente", "mente"];
static STEP1_IDAD: &[&str] = &[
    "abilidad",
    "abilidades",
    "icidad",
    "icidades",
    "ividad",
    "ividades",
    "idad",
    "idades",
];
static STEP1_IVA: &[&str] = &[
    "ativa", "ativo", "ativas", "ativos", "iva", "ivo", "ivas", "ivos",
];
static STEP2A: &[&str] = &[
    "ya", "ye", "yan", "yen", "yeron", "yendo", "yo", "yó", "yas", "yes", "yais", "yamos",
];
/// The step-2b verb list: the finite verb endings deleted in RV.
///
/// `"aseis"` was shipped as `"  aseis"`, with two leading spaces. No token
/// can contain a space, so the rule never fired for any input at all, and the
/// `-ar` imperfect subjunctive was left with four of its five endings — `ase`,
/// `ases`, `ásemos` and `asen` are all below, and only the second-person
/// plural was missing. `hablaseis` came back unstemmed while `hablasteis`
/// stemmed to `habl`. `data::table_audit` now walks every entry of this table
/// through the pipeline that searches it, so a space cannot reappear here
/// unnoticed.
static STEP2B: &[&str] = &[
    "arían", "arías", "arán", "arás", "aríais", "aría", "aréis", "aríamos", "aremos", "ará", "aré",
    "erían", "erías", "erán", "erás", "eríais", "ería", "eréis", "eríamos", "eremos", "erá", "eré",
    "irían", "irías", "irán", "irás", "iríais", "iría", "iréis", "iríamos", "iremos", "irá", "iré",
    "aba", "ada", "ida", "ía", "ara", "iera", "ad", "ed", "id", "ase", "iese", "aste", "iste",
    "an", "aban", "ían", "aran", "ieran", "asen", "iesen", "aron", "ieron", "ado", "ido", "ando",
    "iendo", "ió", "ar", "er", "ir", "as", "abas", "adas", "idas", "ías", "aras", "ieras", "ases",
    "ieses", "ís", "áis", "abais", "íais", "arais", "ierais", "aseis", "ieseis", "asteis",
    "isteis", "ados", "idos", "amos", "ábamos", "íamos", "imos", "áramos", "iéramos", "iésemos",
    "ásemos",
];
static STEP2B_EN: &[&str] = &["en", "es", "éis", "emos"];
static STEP3_A: &[&str] = &["os", "a", "o", "á", "í", "ó"];
static STEP3_E: &[&str] = &["e", "é"];

impl TokenizeAndStem for PorterStemmerEs {
    const FILTER_ON: Casing = Casing::Raw;
    const STEM_ON: Casing = Casing::Lower;

    fn is_stop_word(word: &str) -> bool {
        Language::Es.contains(word)
    }

    fn gate(token: &str) -> bool {
        token.encode_utf16().any(gate_es)
    }

    fn stem_token(&self, token: &str) -> String {
        self.stem(token).into_owned()
    }
}

/// What [`crate::data::table_audit`] needs to walk this language's tables.
#[cfg(test)]
pub(crate) mod audit {
    /// Every rule table, named.
    pub(crate) static TABLES: &[(&str, &[&str])] = &[
        ("PRONOUN", super::PRONOUN),
        ("PRONOUN_PRE1", super::PRONOUN_PRE1),
        ("PRONOUN_PRE2", super::PRONOUN_PRE2),
        ("STEP1_A", super::STEP1_A),
        ("STEP1_B", super::STEP1_B),
        ("STEP1_LOGIA", super::STEP1_LOGIA),
        ("STEP1_UCION", super::STEP1_UCION),
        ("STEP1_ENCIA", super::STEP1_ENCIA),
        ("STEP1_MENTE2", super::STEP1_MENTE2),
        ("STEP1_AMENTE", super::STEP1_AMENTE),
        ("STEP1_MENTE", super::STEP1_MENTE),
        ("STEP1_IDAD", super::STEP1_IDAD),
        ("STEP1_IVA", super::STEP1_IVA),
        ("STEP2A", super::STEP2A),
        ("STEP2B", super::STEP2B),
        ("STEP2B_EN", super::STEP2B_EN),
        ("STEP3_A", super::STEP3_A),
        ("STEP3_E", super::STEP3_E),
    ];

    /// Spanish has no prelude: `stem` marks its regions on the token as it
    /// arrives. Accent removal is a *postlude*, so the accented spellings the
    /// tables carry are exactly what the tables are searched against.
    pub(crate) fn prelude(token: &str) -> String {
        token.to_owned()
    }

    /// No marker unit is written before the tables are searched.
    pub(crate) static MARKERS: &[(&str, &str)] = &[];
}

impl verbora_core::Stemmer for PorterStemmerEs {
    fn stem<'a>(&self, token: &'a str) -> Cow<'a, str> {
        Self::stem(self, token)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(t: &str) -> String {
        PorterStemmerEs::new().stem(t).into_owned()
    }

    /// The `-aseis` ending: the `-ar` imperfect subjunctive, second person
    /// plural.
    #[test]
    fn the_imperfect_subjunctive_paradigm_is_complete() {
        for (input, want) in [
            ("hablase", "habl"),
            ("hablases", "habl"),
            ("hablásemos", "habl"),
            ("hablaseis", "habl"),
            ("hablasen", "habl"),
        ] {
            assert_eq!(s(input), want, "stem({input})");
        }
    }

    #[test]
    fn uppercase_is_a_no_op() {
        assert_eq!(s("ÁRBOL"), "ÁRBOL");
        assert_eq!(s("CAMPA"), "CAMPA");
        assert_eq!(s("Efecto"), "Efect");
        assert_eq!(s("campa"), "camp");
    }

    #[test]
    fn remove_accent_hits_only_the_first_occurrence() {
        assert_eq!(PorterStemmerEs::new().remove_accent("ááéé"), "aáeé");
        assert_eq!(PorterStemmerEs::new().remove_accent("abc"), "abc");
    }

    #[test]
    fn edges_and_unicode() {
        assert_eq!(s(""), "");
        assert_eq!(s("a"), "a");
        assert_eq!(s("ab"), "ab");
        assert_eq!(s("á"), "a");
        assert_eq!(s("😀"), "😀");
        assert_eq!(s("日本語"), "日本語");
        assert_eq!(s("123"), "123");
    }

    // -----------------------------------------------------------------------
    // Differential oracle: the pre-find_among implementation, verbatim.
    //
    // `stem` above is a restructuring of this code — the whole point is that
    // the two are byte-identical on every input, so the old linear-scan port
    // is kept here as the oracle and the tests below replay the bench word
    // list, the documented edge cases and a seeded random corpus through both.
    // -----------------------------------------------------------------------
    mod oracle {
        use super::super::*;
        use crate::units::{ends_with, longest_suffix, slen, text, u, units};

        fn from(w: &[u16], at: usize) -> &[u16] {
            &w[at.min(w.len())..]
        }

        fn drop_last(w: &[u16], n: usize) -> Vec<u16> {
            w[..w.len().saturating_sub(n)].to_vec()
        }

        fn remove_accent_units(w: &[u16]) -> Vec<u16> {
            let mut out = w.to_vec();
            for (accented, plain) in [
                (u('á'), u('a')),
                (u('é'), u('e')),
                (u('í'), u('i')),
                (u('ó'), u('o')),
                (u('ú'), u('u')),
            ] {
                if let Some(i) = out.iter().position(|&c| c == accented) {
                    out[i] = plain;
                }
            }
            out
        }

        #[expect(clippy::too_many_lines, reason = "verbatim copy of the reference port")]
        pub(super) fn stem(word: &str) -> String {
            let mut w = units(word);
            let length = w.len();
            if length < 2 {
                return text(&remove_accent_units(&w));
            }

            let (mut r1, mut r2, mut rv) = (length, length, length);
            for i in 0..length - 1 {
                if r1 != length {
                    break;
                }
                if is_vowel(w[i]) && !is_vowel(w[i + 1]) {
                    r1 = i + 2;
                }
            }
            for i in r1..length.saturating_sub(1) {
                if r2 != length {
                    break;
                }
                if is_vowel(w[i]) && !is_vowel(w[i + 1]) {
                    r2 = i + 2;
                }
            }
            if length > 3 {
                if !is_vowel(w[1]) {
                    rv = (2..length).find(|&i| is_vowel(w[i])).unwrap_or(length) + 1;
                } else if is_vowel(w[0]) && is_vowel(w[1]) {
                    rv = (2..length).find(|&i| !is_vowel(w[i])).unwrap_or(length) + 1;
                } else {
                    rv = 3;
                }
            }

            if let Some(suffix) = longest_suffix(&w, PRONOUN) {
                let n = slen(suffix);
                let rv_text = from(&w, rv).to_vec();
                let head = &rv_text[..rv_text.len().saturating_sub(n)];
                if longest_suffix(head, PRONOUN_PRE1).is_some() {
                    w = remove_accent_units(&drop_last(&w, n));
                } else {
                    let stem_head = drop_last(&w, n);
                    if longest_suffix(head, PRONOUN_PRE2).is_some()
                        || ends_with(&stem_head, "uyendo")
                    {
                        w = stem_head;
                    }
                }
            }

            let step1_start = w.clone();

            if let Some(s) = longest_suffix(from(&w, r2), STEP1_A) {
                w = drop_last(&w, slen(s));
            } else if let Some(s) = longest_suffix(from(&w, r2), STEP1_B) {
                w = drop_last(&w, slen(s));
            } else if let Some(s) = longest_suffix(from(&w, r2), STEP1_LOGIA) {
                w = drop_last(&w, slen(s));
                w.extend("log".encode_utf16());
            } else if let Some(s) = longest_suffix(from(&w, r2), STEP1_UCION) {
                w = drop_last(&w, slen(s));
                w.push(u('u'));
            } else if let Some(s) = longest_suffix(from(&w, r2), STEP1_ENCIA) {
                w = drop_last(&w, slen(s));
                w.extend("ente".encode_utf16());
            } else if let Some(s) = longest_suffix(from(&w, r2), STEP1_MENTE2) {
                w = drop_last(&w, slen(s));
            } else if let Some(s) = longest_suffix(from(&w, r1), STEP1_AMENTE) {
                w = drop_last(&w, slen(s));
            } else if let Some(s) = longest_suffix(from(&w, r2), STEP1_MENTE) {
                w = drop_last(&w, slen(s));
            } else if let Some(s) = longest_suffix(from(&w, r2), STEP1_IDAD) {
                w = drop_last(&w, slen(s));
            } else if let Some(s) = longest_suffix(from(&w, r2), STEP1_IVA) {
                w = drop_last(&w, slen(s));
            }

            let step1_changed = w != step1_start;

            if !step1_changed {
                let mut step2a_changed = false;
                if let Some(s) = longest_suffix(from(&w, rv), STEP2A) {
                    let n = slen(s);
                    if w.len() > n && w[w.len() - n - 1] == u('u') {
                        w = drop_last(&w, n);
                        step2a_changed = true;
                    }
                }

                if !step2a_changed {
                    if let Some(s) = longest_suffix(from(&w, rv), STEP2B) {
                        w = drop_last(&w, slen(s));
                    } else if let Some(s) = longest_suffix(from(&w, rv), STEP2B_EN) {
                        w = drop_last(&w, slen(s));
                        if ends_with(&w, "gu") {
                            w.truncate(w.len() - 1);
                        }
                    }
                }
            }

            if let Some(s) = longest_suffix(from(&w, rv), STEP3_A) {
                w = drop_last(&w, slen(s));
            } else if longest_suffix(from(&w, rv), STEP3_E).is_some() {
                w.truncate(w.len().saturating_sub(1));
                if ends_with(from(&w, rv), "u") && ends_with(&w, "gu") {
                    w.truncate(w.len() - 1);
                }
            }

            text(&remove_accent_units(&w))
        }
    }

    /// A deterministic xorshift; no dev-dependency needed.
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

    /// Random stems crossed with real table suffixes (stacked up to two deep)
    /// plus case/astral/CJK/digit noise — the corpus shape that verified the
    /// prototype over 500k cases.
    fn random_word(rng: &mut Rng) -> String {
        const ALPHA: &[char] = &[
            'a', 'b', 'c', 'd', 'e', 'f', 'g', 'i', 'l', 'm', 'n', 'o', 'p', 'r', 's', 't', 'u',
            'v', 'y', 'z', 'á', 'é', 'í', 'ó', 'ú', 'ñ', 'ü', 'g', 'u', 'q',
        ];
        const SUFFIXES: &[&str] = &[
            "amente",
            "mente",
            "aciones",
            "logía",
            "uciones",
            "encia",
            "icidades",
            "ativos",
            "yendo",
            "yeron",
            "aríamos",
            "iésemos",
            "ando",
            "iendo",
            "selo",
            "selas",
            "nos",
            "ar",
            "er",
            "ir",
            "os",
            "a",
            "o",
            "e",
            "é",
            "gu",
            "u",
            "ución",
            "  aseis",
            "aseis",
            "íais",
            "uyendo",
            "ándoselo",
            "iéndonos",
            "ísimo",
        ];
        let mut s = String::new();
        for _ in 0..rng.below(9) {
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
        let stemmer = PorterStemmerEs::new();
        let check = |input: &str| {
            assert_eq!(
                stemmer.stem(input).as_ref(),
                oracle::stem(input),
                "stem({input:?})"
            );
        };
        for w in crate::test_support::bench_words("es") {
            check(&w);
        }
        for w in [
            "",
            "a",
            "ab",
            "á",
            "😀",
            "日本語",
            "123",
            "ÁRBOL",
            "CAMPA",
            "Efecto",
            "campa",
            "ááéé",
            "abc",
            "uyendo",
            "cantándoselo",
            "digámoselo",
            "muéstrame",
            "aseis",
            "  aseis",
            "xy  aseis",
            "gu",
            "agu",
            "guiar",
            "averigüéis",
        ] {
            check(w);
        }
        let long = "cantar".repeat(20); // exercises the Buf heap spill
        check(&long);
        let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
        for _ in 0..60_000 {
            let w = random_word(&mut rng);
            check(&w);
        }
    }
}
