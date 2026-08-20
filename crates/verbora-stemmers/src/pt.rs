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
//! # Why the buffer is `Vec<u16>`
//!
//! Region marking here compares positions against the literal constants 1, 2
//! and 3 and against the word's length, all of which are stated in UTF-16 code
//! units, so this stemmer runs on its own `Vec<u16>` rather than on a scalar
//! buffer. A `Vec<char>` would count one per Unicode scalar value and move
//! every region boundary in a word containing an astral scalar. See the crate
//! root on the text unit.

use std::borrow::Cow;
use std::sync::LazyLock;

use crate::among::{AmongTable, Buf, UnionTable, longest_at_most};
use crate::base::{Casing, TokenizeAndStem};
use crate::stopwords::Language;
use crate::units::ends_with;

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

/// The Portuguese vowels, as the region scans see them.
///
/// `ã` and `õ` are deliberately **absent**. The prelude rewrites them to `a~`
/// and `o~` before any region is marked — that is the whole point of the nasal
/// detour, so that a nasal vowel counts as *vowel followed by consonant* — so
/// neither can be in the buffer when this predicate runs. They were listed
/// here and could never match; `the_nasal_detour_leaves_no_nasal_behind` pins
/// the reason rather than the removal.
#[inline]
fn is_vowel(c: u16) -> bool {
    matches!(
        c,
        0x61 | 0x65 | 0x69 | 0x6F | 0x75          // a e i o u
        | 0xE1 | 0xE9 | 0xED | 0xF3 | 0xFA        // á é í ó ú
        | 0xE2 | 0xEA | 0xF4                      // â ê ô
        | 0xE0 // à
    )
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
#[cfg(test)]
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
fn replace_in_region(buf: &mut Buf, table: &AmongTable, replacement: &str, region: usize) -> bool {
    let len = buf.len();
    let n = table.longest(buf.as_slice(), len, region.min(len));
    if n > 0 {
        buf.truncate(len - n);
        buf.push_str(replacement);
        true
    } else {
        false
    }
}

/// The prelude's nasal detour: `ã` becomes `a~` and `õ` becomes `o~`.
///
/// # Why one pass for both, and why backwards
///
/// This is `replaceAll` for one-unit needles and two-unit replacements, so
/// the word *grows*. Rebuilding into a fresh `Vec` — what the general
/// `replaceAll` does — cost an allocation on every word even though the
/// overwhelming majority of Portuguese words contain no nasal at all, and
/// running the two rewrites separately cost two full scans instead of one.
/// The two are safe to fuse because their needles and their replacements
/// share no character, so neither can create or destroy an occurrence of the
/// other. Counting first and then copying backwards means the buffer is only
/// touched when there is something to move, and no unit is overwritten
/// before it has been read.
fn expand_nasals(buf: &mut Buf) {
    let extra = buf
        .as_slice()
        .iter()
        .filter(|&&c| c == 0x00E3 || c == 0x00F5)
        .count();
    if extra == 0 {
        return;
    }
    let old_len = buf.len();
    for _ in 0..extra {
        buf.push(0);
    }
    let w = buf.as_mut_slice();
    let mut write = old_len + extra;
    for read in (0..old_len).rev() {
        let base = match w[read] {
            0x00E3 => Some(0x0061),
            0x00F5 => Some(0x006F),
            _ => None,
        };
        if let Some(base) = base {
            write -= 2;
            w[write] = base;
            w[write + 1] = 0x7E; // '~'
        } else {
            write -= 1;
            w[write] = w[read];
        }
    }
}

/// The postlude's inverse: `a~` becomes `ã` and `o~` becomes `õ`.
///
/// Neither needle can overlap itself or the other (they differ in their
/// first unit and a `~` consumed by one could only be consumed by the other
/// if the same position held both `a` and `o`), so one left-to-right
/// compaction visits exactly the occurrences the reference's two successive
/// `split(find).join(replace)` calls would. A word with no `~` at all — every
/// word that had no nasal, and that is nearly all of them — skips the pass
/// entirely.
fn collapse_nasals(buf: &mut Buf) {
    if !buf.as_slice().contains(&0x7E) {
        return;
    }
    let len = buf.len();
    let w = buf.as_mut_slice();
    let mut write = 0usize;
    let mut read = 0usize;
    while read < len {
        let nasal = if read + 1 < len && w[read + 1] == 0x7E {
            match w[read] {
                0x0061 => Some(0x00E3),
                0x006F => Some(0x00F5),
                _ => None,
            }
        } else {
            None
        };
        if let Some(nasal) = nasal {
            w[write] = nasal;
            read += 2;
        } else {
            w[write] = w[read];
            read += 1;
        }
        write += 1;
    }
    buf.truncate(write);
}

/// The nine step-1 rules, in chain order: the suffix table, the replacement
/// it writes, and which region the match must fit inside.
///
/// Step 1 is a *chain*, not a ladder — every rule runs, and a later one sees
/// what an earlier one left. That is why the whole step cannot collapse to a
/// single decision the way French's and Italian's step 1 can. What it can
/// collapse to is a single *search*: none of these rules touches the word
/// unless it matches, so one merged search over the un-mutated word answers
/// every rule that is going to decline, and only an actual mutation forces a
/// re-search. Real words fire at most one or two of the nine.
struct Step1Rule {
    /// Index into [`STEP1_TABLE_LIST`], and so into the mask array.
    table: usize,
    replacement: &'static str,
    region: Region,
}

/// Which of the three marked regions a step-1 rule checks.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Region {
    R1,
    R2,
    Rv,
}

static STEP1_TABLE_LIST: &[&[&str]] = &[
    STEP1_NOUN,
    LOGIA,
    ENCIA,
    MENTE2,
    AMENTE,
    MENTE,
    STEP1_IDADE,
    STEP1_IVO,
    IRA,
];
static LOGIA: &[&str] = &["logias", "logia"];
static ENCIA: &[&str] = &["ências", "ência"];
static MENTE2: &[&str] = &["ativamente", "icamente", "ivamente", "osamente", "adamente"];
/// `['amente'] -> '' in R1` — one literal, but a chain step of its own.
static AMENTE: &[&str] = &["amente"];
static MENTE: &[&str] = &["antemente", "avelmente", "ivelmente", "mente"];
/// `iras|ira → "ir"`, gated on the word ending `eiras`/`eira`.
static IRA: &[&str] = &["iras", "ira"];

/// The chain order. The reference has a commented-out
/// `['uço~es', 'uça~o'] -> 'u'` rule between `logia` and `ência`; it is
/// commented out there and absent here.
static STEP1_RULES: &[Step1Rule] = &[
    Step1Rule {
        table: 0,
        replacement: "",
        region: Region::R2,
    },
    Step1Rule {
        table: 1,
        replacement: "log",
        region: Region::R2,
    },
    Step1Rule {
        table: 2,
        replacement: "ente",
        region: Region::R2,
    },
    Step1Rule {
        table: 3,
        replacement: "",
        region: Region::R2,
    },
    Step1Rule {
        table: 4,
        replacement: "",
        region: Region::R1,
    },
    Step1Rule {
        table: 5,
        replacement: "",
        region: Region::R2,
    },
    Step1Rule {
        table: 6,
        replacement: "",
        region: Region::R2,
    },
    Step1Rule {
        table: 7,
        replacement: "",
        region: Region::R2,
    },
    Step1Rule {
        table: 8,
        replacement: "ir",
        region: Region::Rv,
    },
];
/// The one rule with a gate outside its own table.
const RULE_IRA: usize = 8;

/// The sorted search tables, built once from the ordered rule tables below.
struct PtTables {
    /// The nine step-1 tables merged into one search; see [`Step1Rule`].
    step1: UnionTable,
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
    step1: UnionTable::build(STEP1_TABLE_LIST),
    verb: AmongTable::build(VERB),
    residual: AmongTable::build(RESIDUAL),
    ue: AmongTable::build(STEP5_UE),
    ie: AmongTable::build(STEP5_IE),
    e: AmongTable::build(STEP5_E),
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
        reason = "every stemmer is zero-sized; `stem` is a method so the \
                  sixteen of them share one call shape"
    )]
    #[must_use]
    pub fn stem<'a>(&self, word: &'a str) -> Cow<'a, str> {
        let tb = &*TABLES;
        let mut w = Buf::fill_lowercase(word);

        // --- Prelude: nasal vowels become vowel + '~' ----------------------
        expand_nasals(&mut w);

        let r1 = mark_region_n(w.as_slice(), 0);
        let r2 = mark_region_n(w.as_slice(), r1);
        let rv = mark_region_v(w.as_slice());

        // --- Step 1: standard suffixes (nine chained calls) ----------------
        //
        // One merged search per *mutation*, not per rule: the mask describes
        // the word every remaining rule is about to be tested against, and
        // stays valid until some rule actually rewrites it.
        let mut changed = false;
        let mut rule = 0usize;
        'chain: while rule < STEP1_RULES.len() {
            let len = w.len();
            let mut lm = [0u32; 9];
            tb.step1.length_masks(w.as_slice(), len, &mut lm);
            while rule < STEP1_RULES.len() {
                let r = &STEP1_RULES[rule];
                let start = match r.region {
                    Region::R1 => r1,
                    Region::R2 => r2,
                    Region::Rv => rv,
                };
                let gated =
                    rule == RULE_IRA && !IRA_GUARD.iter().any(|g| ends_with(w.as_slice(), g));
                let m = if gated {
                    0
                } else {
                    longest_at_most(lm[r.table], len - start.min(len))
                };
                rule += 1;
                if m > 0 {
                    w.truncate(len - m);
                    w.push_str(r.replacement);
                    changed = true;
                    continue 'chain;
                }
            }
            break;
        }
        let step1_changed = changed;

        // --- Step 2: verb suffixes, only if step 1 changed nothing ---------
        if !step1_changed {
            changed = replace_in_region(&mut w, &tb.verb, "", rv);
        }

        // --- Step 3 or 4 ---------------------------------------------------
        if !changed {
            replace_in_region(&mut w, &tb.residual, "", rv);
        } else if ends_with(w.as_slice(), STEP4_CI[0]) && w.len().saturating_sub(1) >= rv {
            // `['i'] -> '' in RV`, gated on a preceding `c`.
            let keep = w.len() - 1;
            w.truncate(keep);
        }

        // --- Step 5: residual form ----------------------------------------
        let mut step5_changed = false;
        if STEP5_GUE.iter().any(|g| ends_with(w.as_slice(), g)) {
            step5_changed |= replace_in_region(&mut w, &tb.ue, "", rv);
        }
        if STEP5_CIE.iter().any(|g| ends_with(w.as_slice(), g)) {
            step5_changed |= replace_in_region(&mut w, &tb.ie, "", rv);
        }
        if !step5_changed {
            replace_in_region(&mut w, &tb.e, "", rv);
        }
        // `['ç'] -> 'c'` in region `all` (0), so this one is unconditional.
        if w.as_slice().last() == Some(&0x00E7) {
            let keep = w.len() - 1;
            w.truncate(keep);
            w.push(0x63);
        }

        // --- Postlude ------------------------------------------------------
        collapse_nasals(&mut w);
        Cow::Owned(w.into_text())
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
static RESIDUAL: &[&str] = &["os", "a", "i", "o", "\u{e1}", "\u{ed}", "\u{f3}"];
/// Step 5's `ue|ué|uê` group, removable only after a `g`.
static STEP5_UE: &[&str] = &["ue", "u\u{e9}", "u\u{ea}"];
/// Step 5's `ie|ié|iê` group, removable only after a `c`.
static STEP5_IE: &[&str] = &["ie", "i\u{e9}", "i\u{ea}"];
/// Step 5's unconditional `e|é|ê`.
static STEP5_E: &[&str] = &["e", "\u{e9}", "\u{ea}"];
/// The guards on [`STEP5_UE`]: the same three endings with their `g`.
static STEP5_GUE: &[&str] = &["gue", "gu\u{e9}", "gu\u{ea}"];
/// The guards on [`STEP5_IE`]: the same three endings with their `c`.
static STEP5_CIE: &[&str] = &["cie", "ci\u{e9}", "ci\u{ea}"];
/// Step 4's `ci`, whose `i` is deleted in RV.
static STEP4_CI: &[&str] = &["ci"];
/// The endings [`IRA`] is gated on.
static IRA_GUARD: &[&str] = &["eiras", "eira"];

impl TokenizeAndStem for PorterStemmerPt {
    const FILTER_ON: Casing = Casing::Lower;
    const STEM_ON: Casing = Casing::Raw;

    fn is_stop_word(word: &str) -> bool {
        Language::Pt.contains(word)
    }

    fn stem_token(&self, token: &str) -> String {
        self.stem(token).into_owned()
    }
}

/// What [`crate::data::table_audit`] needs to walk this language's tables.
#[cfg(test)]
pub(crate) mod audit {
    use crate::among::Buf;

    /// Every rule table, named.
    pub(crate) static TABLES: &[(&str, &[&str])] = &[
        ("STEP1_NOUN", super::STEP1_NOUN),
        ("LOGIA", super::LOGIA),
        ("ENCIA", super::ENCIA),
        ("MENTE2", super::MENTE2),
        ("AMENTE", super::AMENTE),
        ("MENTE", super::MENTE),
        ("STEP1_IDADE", super::STEP1_IDADE),
        ("STEP1_IVO", super::STEP1_IVO),
        ("IRA", super::IRA),
        ("IRA_GUARD", super::IRA_GUARD),
        ("VERB", super::VERB),
        ("RESIDUAL", super::RESIDUAL),
        ("STEP5_UE", super::STEP5_UE),
        ("STEP5_IE", super::STEP5_IE),
        ("STEP5_E", super::STEP5_E),
        ("STEP5_GUE", super::STEP5_GUE),
        ("STEP5_CIE", super::STEP5_CIE),
        ("STEP4_CI", super::STEP4_CI),
    ];

    /// The prelude `stem` runs before any table is consulted, in isolation:
    /// lowercase, then the nasal detour.
    pub(crate) fn prelude(token: &str) -> String {
        let mut w = Buf::fill_lowercase(token);
        super::expand_nasals(&mut w);
        w.into_text()
    }

    /// The units the prelude writes, paired with what it writes them for.
    /// Half the suffix table is spelled in the detoured form for this reason.
    pub(crate) static MARKERS: &[(&str, &str)] = &[("a~", "\u{e3}"), ("o~", "\u{f5}")];
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
        Language::Pt.add_all(words);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(t: &str) -> String {
        PorterStemmerPt::new().stem(t).into_owned()
    }

    /// No `ã` or `õ` can reach a region scan or a rule table.
    ///
    /// The proof that removing them from [`is_vowel`] changed nothing, and the
    /// proof that every table entry spelled `a~`/`o~` is spelled that way for
    /// a reason. Enumerated over every scalar value in four positions rather
    /// than sampled: the detour is a `replace_all`, so a character it missed
    /// would be missed everywhere and in every word at once.
    #[test]
    fn the_nasal_detour_leaves_no_nasal_behind() {
        for cp in 0..=0x10_FFFFu32 {
            let Some(c) = char::from_u32(cp) else {
                continue;
            };
            for probe in [
                c.to_string(),
                format!("a{c}o"),
                format!("{c}{c}"),
                format!("c{c}rac{c}o"),
            ] {
                let mut w = Buf::fill_lowercase(&probe);
                expand_nasals(&mut w);
                let out = w.into_text();
                assert!(
                    !out.contains('\u{e3}') && !out.contains('\u{f5}'),
                    "the nasal detour left a nasal in {probe:?}: {out:?}"
                );
            }
        }
        // ...and the postlude puts them back, so the stem a caller sees keeps
        // its spelling: no `~` ever escapes, and a surviving nasal is spelled
        // as a nasal.
        let pt = PorterStemmerPt::new();
        assert_eq!(pt.stem("coração"), "coraçã");
        for w in ["coração", "ações", "irmã", "limões", "põe", "ara~o"] {
            assert!(
                !pt.stem(w).contains('~'),
                "the detour leaked into the stem of {w:?}: {:?}",
                pt.stem(w)
            );
        }
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

    /// The cross-cutting battery every stemmer in this crate answers: empty,
    /// one character, uppercase, accented Latin, Greek, Cyrillic, CJK, an
    /// astral pair, punctuation, digits, a line terminator, and a very long
    /// word.
    ///
    /// The expectations are the *identity* in every row but the case fold,
    /// which is the whole point: none of these is a word of this language, so
    /// a stemmer that changes one is reaching outside its own alphabet.
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
        use crate::units::{ends_with, replace_suffix, slen, text, units};

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
