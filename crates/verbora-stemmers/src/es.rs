//! The Spanish Snowball stemmer.
//!
//! # `stem` does not lowercase
//!
//! The vowel test is case-insensitive while every suffix comparison is
//! case-sensitive, so uppercase input flows through the region machinery and
//! then matches nothing: `stem("ÁRBOL")` is `"ÁRBOL"`, `stem("Efecto")` is
//! `"Efect"`, `stem("campa")` is `"camp"`. Verbora keeps it that way rather
//! than folding inside `stem`: the fold would change the stem of every
//! non-lowercase token for callers who reach `stem` directly, and
//! [`TokenizeAndStem`] already lowercases the document before tokenizing.
//!
//! # One more trap
//!
//! [`PorterStemmerEs::remove_accent`] rewrites only the **first** occurrence of
//! each accented vowel, not every one: `remove_accent("ááéé")` is `"aáeé"`.
//!
//! # How `stem` searches its tables
//!
//! Each step goes through one [`crate::among`] binary search rather than a
//! linear walk of its table: the else-if chain's ten step-1 tables are merged
//! into a single union search whose substring-link walk recovers each table's
//! own longest region-valid match, and the chain then fires the
//! lowest-priority-id table, which is the branch order of the chain itself.
//! That is worth ~78% of the per-word cost (`docs/PERFORMANCE_GAPS.md` entry
//! 34), and it is byte-exact against the linear walk it replaced, which lives
//! on in this module's tests as a differential oracle. Region slices become
//! `lb` cursor limits — the search cannot match past them, the same restriction
//! slicing enforced, without the `.to_vec()` snapshots.
//!
//! # The text unit
//!
//! Every position here is a **Unicode scalar value**: R1, R2 and RV, the
//! `length < 2` gate, the `length > 3` guard on RV, the literal `rv = 3`, and
//! every cut. See [`crate::units`] for why a Snowball algorithm is specified
//! over *letters* and why the scalar value is the letter. `stem("😀iamos")` is
//! `"😀iam"`: six letters, so RV starts at 3 and leaves three of them, which
//! the four-letter `-amos` does not fit.

use std::borrow::Cow;
use std::sync::LazyLock;

use crate::among::{AmongTable, Buf, UnionTable};
use crate::base::{Casing, TokenizeAndStem};
use crate::data::charsets::is_es_vowel;
use crate::data::gates::gate_es;
use crate::stopwords::Language;
use crate::units::{ends_with, longest_suffix, slen, text};

/// The working buffer for a `&str`, in this crate's text unit.
#[inline]
fn scalars(s: &str) -> Vec<char> {
    s.chars().collect()
}

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

/// The Spanish vowel class, over a whole character.
///
/// [`is_es_vowel`] is stated over Basic Multilingual Plane code points and
/// nothing in the set reaches `U+00FD`, so anything outside that plane is a
/// consonant for Spanish's purposes — and was under the code-unit reading too,
/// where an astral character was scanned as two surrogates, neither of them in
/// the set either.
#[inline]
fn is_vowel(c: char) -> bool {
    (c as u32) < 0x1_0000 && is_es_vowel(c as u16)
}

/// `(r1, r2, rv)` for a word of length ≥ 2, in scalar values. Callers must
/// uphold the length precondition
/// (`length - 1` underflows otherwise); `stem` returns early for shorter input.
fn mark_regions(w: &[char]) -> (usize, usize, usize) {
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
/// A single pass with five "already replaced" flags is equivalent to five
/// independent first-occurrence replacements: the five code points are
/// distinct, so replacing one never creates or destroys an occurrence of
/// another.
fn remove_accent_inplace(w: &mut [char]) {
    let (mut a, mut e, mut i, mut o, mut u) = (false, false, false, false, false);
    for c in w.iter_mut() {
        match *c {
            'á' if !a => {
                *c = 'a';
                a = true;
            }
            'é' if !e => {
                *c = 'e';
                e = true;
            }
            'í' if !i => {
                *c = 'i';
                i = true;
            }
            'ó' if !o => {
                *c = 'o';
                o = true;
            }
            'ú' if !u => {
                *c = 'u';
                u = true;
            }
            _ => {}
        }
    }
}

/// Whether `w` ends in the two characters `gu`.
#[inline]
fn ends_gu(w: &[char]) -> bool {
    let n = w.len();
    n >= 2 && w[n - 1] == 'u' && w[n - 2] == 'g'
}

/// The sorted search tables, built once from the `&'static str` rule tables
/// below — those stay the single source of truth.
struct EsTables {
    pronoun: AmongTable<char>,
    /// 0 = [`PRONOUN_PRE1`], 1 = [`PRONOUN_PRE2`].
    pre: UnionTable<char>,
    /// The ten step-1 tables in chain order; id 6 (`amente`) checks R1, the
    /// rest R2.
    step1: UnionTable<char>,
    step2a: AmongTable<char>,
    /// 0 = [`STEP2B`], 1 = [`STEP2B_EN`].
    step2b: UnionTable<char>,
    /// 0 = [`STEP3_A`], 1 = [`STEP3_E`].
    step3: UnionTable<char>,
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

    /// The vowel test — case-insensitive, which is why uppercase words still get
    /// regions.
    #[allow(
        clippy::unused_self,
        reason = "every stemmer is zero-sized; `stem` is a method so the \
                  sixteen of them share one call shape"
    )]
    pub fn is_vowel(&self, c: &str) -> bool {
        c.chars().any(is_vowel)
    }

    /// The index of the next vowel at or after `start`, or the length.
    ///
    /// `start`, the answer and the length are all counts of **scalar values**,
    /// so `next_vowel_position("😀casa", 0)` is 2: the characters are `😀`,
    /// `c`, `a`, … and the first vowel is the third of them.
    #[allow(
        clippy::unused_self,
        reason = "every stemmer is zero-sized; `stem` is a method so the \
                  sixteen of them share one call shape"
    )]
    pub fn next_vowel_position(&self, word: &str, start: usize) -> usize {
        let w = scalars(word);
        (start..w.len())
            .find(|&i| is_vowel(w[i]))
            .unwrap_or(w.len())
    }

    /// The index of the next consonant at or after `start`, or the length.
    ///
    /// Indexed in **scalar values**, exactly as [`Self::next_vowel_position`]
    /// is, so the two round-trip.
    #[allow(
        clippy::unused_self,
        reason = "every stemmer is zero-sized; `stem` is a method so the \
                  sixteen of them share one call shape"
    )]
    pub fn next_consonant_position(&self, word: &str, start: usize) -> usize {
        let w = scalars(word);
        (start..w.len())
            .find(|&i| !is_vowel(w[i]))
            .unwrap_or(w.len())
    }

    /// Whether `word` ends with `suffix`, and is at least as long as it.
    #[allow(
        clippy::unused_self,
        reason = "every stemmer is zero-sized; `stem` is a method so the \
                  sixteen of them share one call shape"
    )]
    pub fn ends_in(&self, word: &str, suffix: &str) -> bool {
        slen(word) >= slen(suffix) && ends_with(&scalars(word), suffix)
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
        longest_suffix(&scalars(word), suffixes).unwrap_or("")
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
        let mut b: Buf<char> = Buf::fill(word);
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
                keep >= 6 && s[keep - 6..keep] == ['u', 'y', 'e', 'n', 'd', 'o']
            } {
                b.truncate(length - n);
            }
        }

        // --- Step 1: standard suffixes -------------------------------------
        //
        // One union search answers the step's ten-table else-if chain at once.
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
                    3 => b.push('u'),
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
            if n > 0 && len > n && b.as_slice()[len - n - 1] == 'u' {
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
                if len > lbv && b.as_slice()[len - 1] == 'u' && ends_gu(b.as_slice()) {
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
        token.chars().any(is_spanish_letter)
    }

    fn stem_token(&self, token: &str) -> String {
        self.stem(token).into_owned()
    }
}

/// Whether `c` is one of the letters [`gate_es`] accepts.
///
/// The gate is stated over Basic Multilingual Plane code points and nothing in
/// it reaches `U+00FD`, so an astral character is never a Spanish letter:
/// neither the character itself nor either half of the surrogate pair encoding
/// it is in the set.
#[inline]
fn is_spanish_letter(c: char) -> bool {
    (c as u32) < 0x1_0000 && gate_es(c as u16)
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

    /// The text unit is the **Unicode scalar value**, and Spanish's region
    /// arithmetic makes the choice observable.
    ///
    /// Derived from the algorithm rather than recorded from it. `"😀iamos"` is
    /// six letters — `😀 i a m o s`. `w[1]` is `i`, a vowel, so RV skips the
    /// first arm; `w[0]` is not a vowel, so it skips the second too and takes
    /// the literal **`rv = 3`**. RV therefore leaves `6 - 3 = 3` letters, which
    /// the four-letter step-2b entry `-amos` does not fit, so step 2b declines
    /// and step 3's two-letter `-os` is what cuts: `"😀iam"`.
    ///
    /// `"ñiamos"` is the control, and it is a control rather than a decoration:
    /// `ñ` is a Spanish letter that [`is_vowel`] rejects, exactly as `😀` is
    /// rejected, so the two words have the same letter classes in the same
    /// positions and the algorithm cannot tell them apart. Anything that makes
    /// their stems differ is the encoding leaking through, which is what this
    /// pins.
    #[test]
    fn one_astral_character_is_one_letter() {
        assert_eq!(s("😀iamos"), "😀iam");
        assert_eq!(s("ñiamos"), "ñiam");
    }

    /// The two public position helpers index scalar values, and so index the
    /// same thing the caller's own `chars()` does.
    ///
    /// `"😀casa"` is `😀 c a s a`. The first vowel at or after 0 is the `a` at
    /// index 2; the first consonant at or after 2 is the `s` at index 3. The
    /// two are documented as round-tripping, which they can only do while both
    /// count in the same unit as each other *and* as the string the caller
    /// passed.
    #[test]
    fn the_public_position_helpers_index_scalar_values() {
        let es = PorterStemmerEs::new();
        assert_eq!(es.next_vowel_position("😀casa", 0), 2);
        assert_eq!(es.next_consonant_position("😀casa", 2), 3);
        // The control, same letter classes in the same positions.
        assert_eq!(es.next_vowel_position("ñcasa", 0), 2);
        assert_eq!(es.next_consonant_position("ñcasa", 2), 3);
    }

    /// [`is_vowel`] and [`is_spanish_letter`] are unit-independent: they answer
    /// identically over characters and over code units, for every scalar value
    /// there is.
    ///
    /// `is_es_vowel` tops out at `U+00FA` and `gate_es` at `U+00FC`, so neither
    /// an astral character nor either half of the surrogate pair it encodes to
    /// is ever admitted by either one.
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
            assert_eq!(
                is_vowel(c),
                units.iter().any(|u| is_es_vowel(*u)),
                "U+{cp:04X} vowel"
            );
            assert_eq!(
                is_spanish_letter(c),
                units.iter().any(|u| gate_es(*u)),
                "U+{cp:04X} gate"
            );
        }
    }

    /// A character that is inert for this algorithm and inside the Basic
    /// Multilingual Plane: not in [`is_vowel`], not spelled in any rule table,
    /// its own lower case, and untouched by accent removal.
    const INERT_TWIN: char = 'ж';

    /// Every entry of the Spanish stop-word list and of every Spanish rule
    /// table, walked through `stem` with one astral character inserted at
    /// **every** position, against the same word carrying an inert
    /// Basic-Multilingual-Plane character instead.
    ///
    /// # What the twin proves, and why it needs no second implementation
    ///
    /// [`INERT_TWIN`] is inert for this algorithm in exactly the way an astral
    /// character is: neither is a vowel, neither is spelled in any rule table,
    /// neither is rewritten by the lower-casing or by accent removal. So the
    /// only thing that can possibly distinguish the two words is **how long
    /// each of them is** — one character each under the contract, one and two
    /// under a code-unit reading. One build run over both therefore measures
    /// the unit directly, and a divergence here is a position being counted in
    /// code units.
    ///
    /// # Why every entry and every position, rather than a sample
    ///
    /// This is the shape of defect that has already cost this crate 116 Swedish
    /// stop words and two dead rules: a stage transforms text before a later
    /// stage measures or looks it up, and the entries that die are exactly the
    /// ones a spot check does not name. So the walk is over every entry of
    /// every table and of the stop-word list, behind every alphabetic entry of
    /// that same list — the probe construction `crate::data::table_audit` uses
    /// — and the astral character goes in at every position of each, including
    /// the two ends. The counts are pinned by equality so that a walk which
    /// quietly stops enumerating cannot report a clean sweep of nothing.
    ///
    /// # What it catches
    ///
    /// Run against a code-unit reading, this walk reports **233 of 138 335**
    /// probes measuring an astral character as more than one letter. It reports
    /// **0** here. Every one of the 233 has the same shape:
    /// a word that opens with a non-vowel and whose second letter is a vowel,
    /// so that RV takes its third arm and is the literal `rv = 3`. That 3 is an
    /// absolute position rather than a relative one, so the extra code unit
    /// lengthened the region past it, and a suffix that does not fit the
    /// region by one letter fitted it by one code unit.
    /// [`one_astral_character_is_one_letter`] states one such word with its
    /// arithmetic written out; this walk is what shows the shape is the only
    /// one, and that no other shipped entry can see the unit at all.
    #[test]
    fn every_shipped_entry_measures_the_same_under_either_unit() {
        let stemmer = PorterStemmerEs::new();
        let stops = Language::Es.defaults();
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
            213,
            "the Spanish rule-table entry count moved"
        );
        entries.extend(stops.iter().copied());
        assert_eq!(entries.len(), 283, "the Spanish entry count moved");
        assert_eq!(fillers.len(), 60, "the Spanish filler count moved");

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
            probes, 138_335,
            "the number of probes this walk builds moved"
        );
    }

    // -----------------------------------------------------------------------
    // Differential oracle: the same steps written as plain linear table scans.
    //
    // `stem` above is a restructuring of this code — the whole point is that
    // the two are byte-identical on every input — so the linear-scan form is
    // kept here as the oracle and the tests below replay the bench word list,
    // the documented edge cases and a seeded random corpus through both.
    // -----------------------------------------------------------------------
    mod oracle {
        use super::super::*;
        use crate::units::{ends_with, longest_suffix, slen, text};

        fn from(w: &[char], at: usize) -> &[char] {
            &w[at.min(w.len())..]
        }

        fn drop_last(w: &[char], n: usize) -> Vec<char> {
            w[..w.len().saturating_sub(n)].to_vec()
        }

        fn remove_accent_units(w: &[char]) -> Vec<char> {
            let mut out = w.to_vec();
            for (accented, plain) in [('á', 'a'), ('é', 'e'), ('í', 'i'), ('ó', 'o'), ('ú', 'u')]
            {
                if let Some(i) = out.iter().position(|&c| c == accented) {
                    out[i] = plain;
                }
            }
            out
        }

        #[expect(
            clippy::too_many_lines,
            reason = "the oracle is one straight-line transcription of the steps"
        )]
        pub(super) fn stem(word: &str) -> String {
            let mut w = scalars(word);
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
                w.extend("log".chars());
            } else if let Some(s) = longest_suffix(from(&w, r2), STEP1_UCION) {
                w = drop_last(&w, slen(s));
                w.push('u');
            } else if let Some(s) = longest_suffix(from(&w, r2), STEP1_ENCIA) {
                w = drop_last(&w, slen(s));
                w.extend("ente".chars());
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
                    if w.len() > n && w[w.len() - n - 1] == 'u' {
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
