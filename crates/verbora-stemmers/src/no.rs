//! The Norwegian stemmer, ported from
//! The reference `porter_stemmer_no`.
//!
//! # Not the Snowball algorithm
//!
//! This is Kristoffer Brabrand's own reading of it, and it differs from Snowball
//! proper in one structural way that dominates the output: [`PorterStemmerNo::step1`]
//! runs 1a, 1b and 1c **independently on the same input** and keeps whichever
//! result is shortest, rather than applying them in sequence. Ties go to the
//! *later* step — `a.len() < b.len()` is strict — so 1c beats 1b beats 1a
//! whenever two agree.
//!
//! # `getR1` returns three different things
//!
//! `/[aeiouyæåø][^aeiouyæåø]([A-Za-z0-9_æøåÆØÅäÄöÖüÜ]+)/` either fails (`null`),
//! matches at index 0 (R1 becomes `token.slice(3)`, which is `""` for a
//! three-letter word), or matches later (R1 is the captured run). Every step then
//! opens with `if (!r1) return token`, and `""` is falsy in the reference — so
//! "matched at index 0 on a short word" and "did not match at all" take the same
//! branch, while `null` and `""` are distinguishable through the exported
//! `getR1`. [`PorterStemmerNo::get_r1`] therefore returns `Option<String>` and
//! the steps test `is_none_or(str::is_empty)`.
//!
//! Note also that the captured run is **not** guaranteed to reach the end of the
//! token: the class excludes `-`, so `getR1("ab-cder")` is `null` and R1 for a
//! hyphenated word stops at the hyphen. The suffix found in R1 is then removed
//! from the *token*, which silently does nothing when the two ends disagree.
//!
//! # Longest suffix, not first alternative
//!
//! Each step matches `/(a|e|ede|…)$/` against R1. The engine takes the earliest
//! start position at which some alternative reaches `$`; because every
//! alternative is a distinct literal and all must end at `$`, that is exactly
//! "the longest suffix of R1 that appears in the list". The order inside the
//! alternation is therefore not load-bearing here, which is worth stating because
//! it *is* load-bearing in the Italian and Portuguese tables.
//!
//! # The unit
//!
//! R1's two bounds, the `slice(3)` above and every cut below count **Unicode
//! scalar values** — the unit [`crate::units`] states for the whole crate. As
//! in Swedish, that matters here more than the rule tables suggest: the cuts
//! come from region arithmetic (`R1Scan`'s `start: 3`, `at_len`'s
//! `index + 2 < len`) rather than from a matched suffix length, so nothing
//! about the tables bounds where one can land. With the scalar unit every cut
//! is a character boundary by construction.

use std::borrow::Cow;
use std::sync::LazyLock;

use crate::among::{AmongTable, Buf, UnionTable};
use crate::base::{Casing, TokenizeAndStem};
use crate::stopwords::Language;
use crate::units::{borrowed_prefix, ends_with, in_set, push_str, set_hi, set_lo, slen, text};

/// The Norwegian stemmer.
///
/// ```
/// use verbora_stemmers::PorterStemmerNo;
/// let s = PorterStemmerNo::new();
/// assert_eq!(s.stem("forebygger"), "forebygg");
/// assert_eq!(s.stem("havnevirksomhetene"), "havnevirksom");
/// assert_eq!(s.stem("hinder"), "hind");
/// ```
///
/// # `æ`, `ø` and `å` are letters, so nothing is folded
///
/// [`TokenizeAndStem::prepare`] is the identity for Norwegian: text reaches
/// the tokenizer, the stop-word list and [`Self::stem`] spelled exactly as it
/// was written. No diacritic fold, no case fold, no rewrite of any kind.
/// Three independent reasons, and each on its own is decisive:
///
/// 1. **They are distinct letters, not accents.** Norwegian has twenty-nine
///    letters; `æ`, `ø` and `å` are the last three and collate *after* `z`,
///    not next to `a` and `o`. Folding `å` merges words that are not the same
///    word: `hår` ("hair") becomes `har` ("has"), and `måte` ("way") becomes
///    `mate` ("to feed"). Only `å` was ever at risk — `æ` and `ø` have no
///    canonical decomposition and so survived any fold by accident, which is
///    exactly the kind of reason that does not survive the next Unicode
///    revision.
/// 2. **Every rule below is stated over the un-folded alphabet.** The vowel
///    class is `[aeiouyæåø]` and R1's character class is
///    `[A-Za-z0-9_æøåÆØÅäÄöÖüÜ]`. A fold that rewrote `å` to `a` but left `æ`
///    and `ø` standing would hand the steps a half-folded alphabet that
///    matches neither the rules as written nor plain ASCII.
/// 3. **It deleted stop words.** Nine of the 129 Norwegian stop words are
///    spelled with `å`, and eight of them fold to strings that are not
///    themselves on the list, so a document-level fold silently removed them:
///    `på`, `så`, `nå`, `når`, `å`, `sånn`, `både` and `også` stopped being
///    stop words while `is_stop_word` still answered `true` for them. Only
///    `vår` survived, and only by the accident that `var` is also on the list.
///
/// The German and Swedish letters `ä ö ü` that appear in Norwegian text
/// (mostly in names) are admitted by R1's class and are likewise left alone,
/// as are foreign accents in loanwords. A caller who wants an
/// accent-insensitive index should fold with
/// `verbora_normalizers::remove_diacritics` *around* this stemmer, where the
/// choice is theirs and visible.
///
/// One stop-word entry is unreachable through this pipeline for an unrelated
/// reason: the list contains `"_"`, and a lone `U+005F` is `ExtendNumLet` with
/// no letter or digit, so [`verbora_tokenizers::WordTokenizer`] never emits it
/// as a token. [`TokenizeAndStem::is_stop_word`] still answers `true` for it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PorterStemmerNo;

/// `[aeiouyæåø]` — lowercase only; the regexes carry no `/i` flag.
///
/// A list of **code points**, feeding the compile-time bitmasks below; the
/// buffer this is tested against holds characters.
static VOWELS: &[u16] = &[0x61, 0x65, 0x69, 0x6F, 0x75, 0x79, 0xE6, 0xE5, 0xF8];
const VOWEL_LO: u128 = set_lo(VOWELS);
const VOWEL_HI: u128 = set_hi(VOWELS);

#[inline]
fn is_vowel(c: char) -> bool {
    in_set(c, VOWEL_LO, VOWEL_HI)
}

/// `[A-Za-z0-9_æøåÆØÅäÄöÖüÜ]`, the class R1's captured run is drawn from.
///
/// Region marking tests this once per character, and as a `matches!` it was
/// sixteen comparisons deep; as a mask it is a shift and an and. See
/// [`crate::units::in_set`], and `character_classes_match_the_literal_sets`
/// for the exhaustive equivalence check.
static R1_CHARS: &[u16] = &[
    0x30, 0x31, 0x32, 0x33, 0x34, 0x35, 0x36, 0x37, 0x38, 0x39, // 0-9
    0x41, 0x42, 0x43, 0x44, 0x45, 0x46, 0x47, 0x48, 0x49, 0x4A, 0x4B, 0x4C, 0x4D, 0x4E, 0x4F, 0x50,
    0x51, 0x52, 0x53, 0x54, 0x55, 0x56, 0x57, 0x58, 0x59, 0x5A, // A-Z
    0x5F, // _
    0x61, 0x62, 0x63, 0x64, 0x65, 0x66, 0x67, 0x68, 0x69, 0x6A, 0x6B, 0x6C, 0x6D, 0x6E, 0x6F, 0x70,
    0x71, 0x72, 0x73, 0x74, 0x75, 0x76, 0x77, 0x78, 0x79, 0x7A, // a-z
    0xE6, 0xF8, 0xE5, 0xC6, 0xD8, 0xC5, 0xE4, 0xC4, 0xF6, 0xD6, 0xFC, 0xDC,
];
const R1_LO: u128 = set_lo(R1_CHARS);
const R1_HI: u128 = set_hi(R1_CHARS);

#[inline]
fn is_r1_char(c: char) -> bool {
    in_set(c, R1_LO, R1_HI)
}

/// The longest suffix of `w` that appears in `alternatives`, or `None`.
///
/// This is what `/(x|y|z)$/` computes: the earliest start position at which some
/// alternative reaches the end. See the module note.
fn longest_listed_suffix<'s>(w: &[char], alternatives: &[&'s str]) -> Option<&'s str> {
    let mut best: Option<&'s str> = None;
    for a in alternatives {
        if ends_with(w, a) && best.is_none_or(|b| slen(a) > slen(b)) {
            best = Some(a);
        }
    }
    best
}

/// Removes `suffix` from the end of `w`, if it is there.
///
/// `token.replace(new RegExp(m + '$'), '')` in the reference — which does
/// nothing when R1's end and the token's end disagree, as they can.
fn strip(w: &[char], suffix: &str) -> Vec<char> {
    if ends_with(w, suffix) {
        w[..w.len() - slen(suffix)].to_vec()
    } else {
        w.to_vec()
    }
}

/// `getR1` as a character range into `t`, or `None` for the reference's `null`.
///
/// The range form is what `stem`'s `find_among` searches consume: R1 is an
/// *interior* slice of the token (the capture class excludes `-`, so it can
/// stop before the end), and the search takes it as `(lb, cursor)` limits
/// instead of a slice, which is the same characters without an intermediate
/// borrow.
fn r1_range(t: &[char]) -> Option<(usize, usize)> {
    R1Scan::of(t).at_len(t.len())
}

/// A scan of `getR1`'s match, kept so the later steps can re-derive R1 after
/// a truncation instead of rescanning the word.
///
/// # Why this is exact
///
/// The reference calls `getR1(token)` afresh in every step, and steps 2 and 3
/// only ever *truncate* — so the question is what a rescan of a prefix would
/// return. The match position is the first `i` with
/// `vowel(t[i]) && !vowel(t[i+1]) && r1char(t[i+2])`; truncating to `len'`
/// changes none of those three characters for any `i` with `i + 2 < len'`, and
/// removes exactly the positions with `i + 2 >= len'` from consideration. So
/// the same `i` is still the first match when `i + 2 < len'`, and there is no
/// match at all otherwise. The captured run's end is the first non-R1
/// character at or after `i + 2`, or the length; over a prefix that is the
/// same index when it lies inside the prefix and the prefix's own length when
/// it does not — which is `min(end, len')` in both cases, including the
/// `index == 0` branch whose end is the length by construction.
///
/// Any step that *rewrites* rather than truncates (step 1c appends `er`)
/// invalidates this and rescans; `stem` marks those sites.
#[derive(Clone, Copy)]
struct R1Scan {
    /// The match position, or `None` for the reference's `null`.
    index: Option<usize>,
    /// R1's start, already carrying the `index == 0` special case.
    start: usize,
    /// The captured run's end in the *scanned* word.
    end: usize,
}

impl R1Scan {
    fn of(t: &[char]) -> R1Scan {
        let Some(index) = (0..t.len().saturating_sub(2))
            .find(|&i| is_vowel(t[i]) && !is_vowel(t[i + 1]) && is_r1_char(t[i + 2]))
        else {
            return R1Scan {
                index: None,
                start: 0,
                end: 0,
            };
        };
        if index == 0 {
            // `preR1Length = index + 2` is 2, which is `< 3`, so the reference
            // substitutes `token.slice(3)` — the empty string for a
            // three-character token.
            return R1Scan {
                index: Some(0),
                start: 3,
                end: t.len(),
            };
        }
        let end = (index + 2..t.len())
            .find(|&i| !is_r1_char(t[i]))
            .unwrap_or(t.len());
        R1Scan {
            index: Some(index),
            start: index + 2,
            end,
        }
    }

    /// What `getR1` would return for the same word truncated to `len`
    /// characters.
    #[inline]
    fn at_len(self, len: usize) -> Option<(usize, usize)> {
        let index = self.index?;
        if index + 2 >= len {
            return None;
        }
        Some((self.start, self.end.min(len)))
    }
}

/// `getR1` over characters. `None` is the reference's `null`.
///
/// Returns a slice borrowed from `t` rather than an owned snapshot: every
/// call site either only checks [`falsy`] or feeds the result straight into
/// [`longest_listed_suffix`], which (like `es.rs`'s `longest_suffix`) returns
/// a value borrowed from the *alternatives* list, never from `t` — so the
/// borrow only needs to live for one lookup, and `t` itself is never
/// reassigned inside any of these pure, non-mutating functions.
fn r1_units(t: &[char]) -> Option<&[char]> {
    r1_range(t).map(|(start, end)| &t[start..end])
}

/// Whether a step should bail out: `if (!r1) return token`, where `""` is falsy.
#[inline]
fn falsy(r1: Option<&[char]>) -> bool {
    r1.is_none_or(<[char]>::is_empty)
}

/// The sorted search tables `stem` uses, built once from the alternations
/// below. The per-step public methods keep the linear scans — they are the
/// reference-shaped API and the test oracle — while `stem` routes the same
/// tables through the `find_among` binary search
/// (`docs/PERFORMANCE_GAPS.md` entry 34).
///
/// # Why only two tables for four alternations
///
/// Steps 1a and 1c interrogate the *same* region of the *same* word — the
/// reference recomputes R1 for each, but both run on step 1's untouched
/// input — so their tables are merged and one search answers both, the link
/// walk recovering each alternation's own longest match (see
/// [`crate::among::UnionTable`]). Steps 2 and 3 run after step 1 has already
/// truncated, so they cannot join that search; step 2's `(dt|vt)` is instead
/// two character comparisons written out in [`ends_dt_vt`], which beats any
/// table lookup at that size.
struct NoTables {
    /// 0 = [`STEP1A`], 1 = [`ERT`] — step 1a's and step 1c's alternations,
    /// searched together over step 1's R1.
    step1: UnionTable<char>,
    step3: AmongTable<char>,
}

static TABLES: LazyLock<NoTables> = LazyLock::new(|| NoTables {
    step1: UnionTable::build(&[STEP1A, ERT]),
    step3: AmongTable::build(STEP3),
});

/// Whether `w[lb..cursor]` ends in `dt` or `vt` — step 2's alternation.
///
/// Both alternatives are two characters ending in `t`, so the whole
/// `find_among` apparatus collapses to the comparisons below.
#[inline]
fn ends_dt_vt(w: &[char], cursor: usize, lb: usize) -> bool {
    cursor >= lb + 2 && w[cursor - 1] == 't' && matches!(w[cursor - 2], 'd' | 'v')
}

/// The length of the longest of `erte`/`ert` that is a suffix of `w`, or 0.
///
/// Step 1c matches its alternation against the **token**, not the region it
/// was gated on, so this takes no limit.
#[inline]
fn ert_suffix_len(w: &[char]) -> usize {
    let n = w.len();
    // The two alternatives end in different characters (`e` and `t`), so at
    // most one of them can match any word and "longest match" has nothing to
    // choose between: the order of these two arms is not load-bearing.
    if n >= 4 && w[n - 4..] == ['e', 'r', 't', 'e'] {
        4
    } else if n >= 3 && w[n - 3..] == ['e', 'r', 't'] {
        3
    } else {
        0
    }
}

impl PorterStemmerNo {
    /// Creates the stemmer. It is stateless and zero-sized.
    #[inline]
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// `getR1`: the token after the first non-vowel that follows a vowel.
    ///
    /// `None` is the reference's `null`; `Some("")` is the distinct outcome of
    /// matching at index 0 on a token shorter than four characters.
    #[allow(
        clippy::unused_self,
        reason = "every stemmer is zero-sized; `stem` is a method so the \
                  sixteen of them share one call shape"
    )]
    #[must_use]
    pub fn get_r1(&self, token: &str) -> Option<String> {
        let t: Vec<char> = token.chars().collect();
        r1_units(&t).map(text)
    }

    /// `step1a`: remove the longest listed nominal suffix found in R1.
    #[must_use]
    pub fn step1a<'a>(&self, token: &'a str) -> Cow<'a, str> {
        self.run(token, step1a)
    }

    /// `step1b`: drop a plural `s` after a listed consonant, or after `Ck`.
    #[must_use]
    pub fn step1b<'a>(&self, token: &'a str) -> Cow<'a, str> {
        self.run(token, step1b)
    }

    /// `step1c`: rewrite a trailing `erte`/`ert` as `er`.
    #[must_use]
    pub fn step1c<'a>(&self, token: &'a str) -> Cow<'a, str> {
        self.run(token, step1c)
    }

    /// `step1`: the shortest of [`Self::step1a`], [`Self::step1b`] and
    /// [`Self::step1c`], each computed from the **same** input.
    #[must_use]
    pub fn step1<'a>(&self, token: &'a str) -> Cow<'a, str> {
        self.run(token, step1)
    }

    /// `step2`: drop a final `t` after `d` or `v` in R1.
    #[must_use]
    pub fn step2<'a>(&self, token: &'a str) -> Cow<'a, str> {
        self.run(token, step2)
    }

    /// `step3`: remove a listed derivational suffix found in R1.
    #[must_use]
    pub fn step3<'a>(&self, token: &'a str) -> Cow<'a, str> {
        self.run(token, step3)
    }

    /// Runs one step over the characters of `token`, borrowing when unchanged.
    #[allow(
        clippy::unused_self,
        reason = "every stemmer is zero-sized; `stem` is a method so the \
                  sixteen of them share one call shape"
    )]
    fn run<'a>(&self, token: &'a str, step: fn(&[char]) -> Vec<char>) -> Cow<'a, str> {
        let t: Vec<char> = token.chars().collect();
        let out = step(&t);
        if out == t {
            Cow::Borrowed(token)
        } else {
            Cow::Owned(text(&out))
        }
    }

    /// Stems one token: lowercase, then step 1, step 2, step 3.
    ///
    /// Byte-identical to running the three public step methods in sequence
    /// (pinned by this module's differential test); the table lookups go
    /// through one `find_among` binary search each instead of the per-step
    /// linear scans, and the three steps share one working buffer instead of
    /// allocating five intermediate `Vec`s.
    #[allow(
        clippy::unused_self,
        reason = "every stemmer is zero-sized; `stem` is a method so the \
                  sixteen of them share one call shape"
    )]
    #[must_use]
    pub fn stem<'a>(&self, token: &'a str) -> Cow<'a, str> {
        let tb = &*TABLES;
        // The lowered characters land straight in a stack buffer: no `String`
        // for `toLowerCase`'s result, no `Vec<char>` for the working copy.
        let (mut b, ascii_lower) = Buf::<char>::fill_lowercase_tracked(token);
        let t = b.as_slice();
        // One scan of `getR1`'s match serves all three steps; see `R1Scan`
        // for why re-deriving it after a truncation is exact.
        let mut scan = R1Scan::of(t);

        // --- Step 1: 1a, 1b and 1c from the SAME input; the shortest result
        // wins and a tie goes to the later step (strict `<`, see module docs).
        // Each candidate is a truncation of `t` (1c also appends "er"), so the
        // three are decided as lengths first and applied to the buffer once.
        let len = t.len();
        let mut len_a = len;
        let mut len_b = len;
        let mut cut_c: Option<usize> = None; // truncate here, then append "er"
        if let Some((r1s, r1e)) = scan.at_len(len)
            && r1e > r1s
        {
            // One search over R1 answers both 1a and 1c: the link walk visits
            // every matching entry longest-first, so the first hit per table
            // id is that table's own longest match.
            let mut best_a = 0usize;
            let mut best_ert = 0usize;
            let mut i = tb.step1.find_longest_index(t, r1e, r1s);
            while i >= 0 {
                let (n, link, tid) = tb.step1.entry(i);
                if tid == 0 {
                    if best_a == 0 {
                        best_a = n;
                    }
                } else if best_ert == 0 {
                    best_ert = n;
                }
                i = link;
            }
            // 1a: remove the longest listed suffix of R1 — from the TOKEN,
            // which does nothing when R1's end and the token's end disagree.
            // A matched entry *is* `t[r1e - n..r1e]`, so the token can be
            // tested against that slice of itself rather than against the
            // table's copy of it.
            if best_a > 0 && t[len - best_a..] == t[r1e - best_a..r1e] {
                len_a = len - best_a;
            }
            // 1b: `/(b|c|d|f|g|h|j|l|m|n|o|p|r|t|v|y|z)s$/`, then `/([^V]k)s$/`
            // — two rules with the same one-unit removal, so one `||`.
            let ends_s = t.last() == Some(&'s');
            let listed = ends_s
                && len >= 2
                && matches!(
                    t[len - 2],
                    'b' | 'c'
                        | 'd'
                        | 'f'
                        | 'g'
                        | 'h'
                        | 'j'
                        | 'l'
                        | 'm'
                        | 'n'
                        | 'o'
                        | 'p'
                        | 'r'
                        | 't'
                        | 'v'
                        | 'y'
                        | 'z'
                );
            let cons_k = ends_s && len >= 3 && t[len - 2] == 'k' && !is_vowel(t[len - 3]);
            if listed || cons_k {
                len_b = len - 1;
            }
            // 1c: gated on R1 containing `erte`/`ert`, applied by re-matching
            // the TOKEN (the two can disagree; see `erte_becomes_er` below).
            if best_ert > 0 {
                let n = ert_suffix_len(t);
                if n > 0 {
                    cut_c = Some(len - n);
                }
            }
        }
        let mut rewrote = false;
        let len_c = cut_c.map_or(len, |k| k + 2);
        if len_a < len_b && len_a < len_c {
            b.truncate(len_a);
        } else if len_a >= len_b && len_b < len_c {
            b.truncate(len_b);
        } else if let Some(k) = cut_c {
            b.truncate(k);
            b.push_str("er");
            // 1c rewrites the tail rather than only shortening it, so the
            // cached scan no longer describes the word — and the result is
            // no longer a prefix of the input, so it cannot be borrowed.
            scan = R1Scan::of(b.as_slice());
            rewrote = true;
        }

        // --- Step 2: drop a final unit when R1 ends in `dt`/`vt` -----------
        let t = b.as_slice();
        if let Some((r1s, r1e)) = scan.at_len(t.len())
            && r1e > r1s
            && ends_dt_vt(t, r1e, r1s)
        {
            let keep = t.len().saturating_sub(1);
            b.truncate(keep);
        }

        // --- Step 3: remove a listed derivational suffix found in R1 -------
        let t = b.as_slice();
        if let Some((r1s, r1e)) = scan.at_len(t.len())
            && r1e > r1s
        {
            let n = tb.step3.longest(t, r1e, r1s);
            // As in 1a, the matched entry is `t[r1e - n..r1e]`, so the
            // "is it also a suffix of the token" test compares the word
            // against itself.
            if n > 0 && t[t.len() - n..] == t[r1e - n..r1e] {
                let keep = t.len() - n;
                b.truncate(keep);
            }
        }

        // Steps 1a/1b, 2 and 3 only truncate, so unless 1c rewrote the tail
        // the answer is a prefix of the caller's own string. See
        // `crate::units::borrowed_prefix`.
        if let Some(prefix) = borrowed_prefix(token, b.len(), ascii_lower, rewrote) {
            return Cow::Borrowed(prefix);
        }
        Cow::Owned(b.into_text())
    }
}

fn step1a(t: &[char]) -> Vec<char> {
    let r1 = r1_units(t);
    if falsy(r1) {
        return t.to_vec();
    }
    longest_listed_suffix(r1.unwrap_or(&[]), STEP1A).map_or_else(|| t.to_vec(), |s| strip(t, s))
}

fn step1b(t: &[char]) -> Vec<char> {
    let r1 = r1_units(t);
    if falsy(r1) {
        return t.to_vec();
    }
    // `/(b|c|d|f|g|h|j|l|m|n|o|p|r|t|v|y|z)s$/` — note `o` is in the list and
    // `k`, `q`, `s`, `u`, `w`, `x` and the other vowels are not.
    let ends_s = t.last() == Some(&'s');
    if ends_s
        && t.len() >= 2
        && matches!(
            t[t.len() - 2],
            'b' | 'c'
                | 'd'
                | 'f'
                | 'g'
                | 'h'
                | 'j'
                | 'l'
                | 'm'
                | 'n'
                | 'o'
                | 'p'
                | 'r'
                | 't'
                | 'v'
                | 'y'
                | 'z'
        )
    {
        return t[..t.len() - 1].to_vec();
    }
    // `/([^aeiouyæåø]k)s$/` — a character before the `k` is required.
    if ends_s && t.len() >= 3 && t[t.len() - 2] == 'k' && !is_vowel(t[t.len() - 3]) {
        return t[..t.len() - 1].to_vec();
    }
    t.to_vec()
}

fn step1c(t: &[char]) -> Vec<char> {
    let r1 = r1_units(t);
    if falsy(r1) {
        return t.to_vec();
    }
    if longest_listed_suffix(r1.unwrap_or(&[]), ERT).is_none() {
        return t.to_vec();
    }
    // The replacement runs against the TOKEN, and re-matches: R1 ending in
    // `erte` does not guarantee the token does.
    match longest_listed_suffix(t, ERT) {
        Some(s) => {
            let mut out = strip(t, s);
            push_str(&mut out, "er");
            out
        }
        None => t.to_vec(),
    }
}

fn step1(t: &[char]) -> Vec<char> {
    let a = step1a(t);
    let b = step1b(t);
    let c = step1c(t);
    // Strict `<` throughout, so a tie always resolves to the later step.
    if a.len() < b.len() {
        if a.len() < c.len() { a } else { c }
    } else if b.len() < c.len() {
        b
    } else {
        c
    }
}

fn step2(t: &[char]) -> Vec<char> {
    let r1 = r1_units(t);
    if falsy(r1) {
        return t.to_vec();
    }
    if longest_listed_suffix(r1.unwrap_or(&[]), STEP2).is_some() {
        return t[..t.len().saturating_sub(1)].to_vec();
    }
    t.to_vec()
}

fn step3(t: &[char]) -> Vec<char> {
    let r1 = r1_units(t);
    if falsy(r1) {
        return t.to_vec();
    }
    longest_listed_suffix(r1.unwrap_or(&[]), STEP3).map_or_else(|| t.to_vec(), |s| strip(t, s))
}

/// The step-1a alternation, in source order.
static STEP1A: &[&str] = &[
    "a", "e", "ede", "ande", "ende", "ane", "ene", "hetene", "en", "heten", "ar", "er", "heter",
    "as", "es", "edes", "endes", "enes", "hetenes", "ens", "hetens", "ers", "ets", "et", "het",
    "ast",
];

/// Step 2's alternation, in source order.
static STEP2: &[&str] = &["dt", "vt"];

/// Step 1c's alternation, in source order.
static ERT: &[&str] = &["erte", "ert"];

/// The step-3 alternation, in source order.
static STEP3: &[&str] = &[
    "leg", "eleg", "ig", "eig", "lig", "elig", "els", "lov", "elov", "slov", "hetslov",
];

impl TokenizeAndStem for PorterStemmerNo {
    const FILTER_ON: Casing = Casing::Lower;
    const STEM_ON: Casing = Casing::Raw;

    // `prepare` is deliberately *not* overridden: the trait's identity default
    // is the specified behaviour. See `PorterStemmerNo`'s own documentation,
    // "`æ`, `ø` and `å` are letters, so nothing is folded", for the reasoning
    // and for the 8 stop words a fold here used to delete.

    fn is_stop_word(word: &str) -> bool {
        Language::No.contains(word)
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
        ("STEP1A", super::STEP1A),
        ("ERT", super::ERT),
        ("STEP2", super::STEP2),
        ("STEP3", super::STEP3),
    ];

    /// The prelude `stem` runs before any table is consulted: lowercasing,
    /// and nothing else. `æ`, `ø` and `å` are letters and are not folded.
    pub(crate) fn prelude(token: &str) -> String {
        token.to_lowercase()
    }

    /// The prelude writes no marker unit.
    pub(crate) static MARKERS: &[(&str, &str)] = &[];
}

impl verbora_core::Stemmer for PorterStemmerNo {
    fn stem<'a>(&self, token: &'a str) -> Cow<'a, str> {
        Self::stem(self, token)
    }
}

impl PorterStemmerNo {
    /// Appends a stop word to the **process-global Norwegian list**.
    ///
    /// `stemmer_no` exposes `addStopWord` and `addStopWords` but no remover;
    /// the missing methods are missing here too.
    pub fn add_stop_word(&self, word: impl Into<String>) {
        Language::No.add(word);
    }

    /// Appends several stop words to the process-global Norwegian list.
    pub fn add_stop_words<I, S>(&self, words: I)
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Language::No.add_all(words);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(t: &str) -> String {
        PorterStemmerNo::new().stem(t).into_owned()
    }

    /// A working buffer, the way `stem` builds one.
    fn scalars(t: &str) -> Vec<char> {
        t.chars().collect()
    }

    /// **Every** entry of the Norwegian stop-word list must still be recognised
    /// through the documented pipeline, not a spot check of a few.
    ///
    /// A whole-document diacritic fold in `prepare` rewrote `på` to `pa`
    /// before `is_stop_word` ever saw it, so 8 of the 129 entries — every one
    /// spelled with `å` whose folded form is not itself on the list — silently
    /// stopped being stop words. A handful of ASCII spot checks passes that
    /// unchanged, which is why this enumerates the list.
    ///
    /// The single exception is spelled out rather than skipped quietly: `"_"`
    /// is on the list and a lone `U+005F` is `ExtendNumLet` carrying no letter
    /// or digit, so [`verbora_tokenizers::WordTokenizer`] never emits it as a
    /// token and no `prepare` could change that. `is_stop_word("_")` is still
    /// `true`; only the tokenizing pipeline cannot reach it.
    #[test]
    fn every_stop_word_is_filtered_by_the_pipeline() {
        let st = PorterStemmerNo::new();
        let defaults = Language::No.defaults();
        let mut unreachable = Vec::new();
        let mut not_a_token = Vec::new();
        for word in defaults {
            // The entry must be one word to the tokenizer, or no `prepare`
            // could ever present it to `is_stop_word` in the first place.
            if st.tokenize_and_stem(word, true).len() != 1 {
                not_a_token.push(*word);
                assert!(
                    PorterStemmerNo::is_stop_word(word),
                    "{word:?} is not even on the list it came from"
                );
                continue;
            }
            if !st.tokenize_and_stem(word, false).is_empty() {
                unreachable.push(*word);
            }
        }
        assert!(
            unreachable.is_empty(),
            "{} of {} Norwegian stop words are unreachable through the pipeline: {unreachable:?}",
            unreachable.len(),
            defaults.len()
        );
        assert_eq!(
            not_a_token,
            ["_"],
            "the set of entries UAX #29 never produces as a token changed"
        );
    }

    #[test]
    fn documented_vectors() {
        for (input, want) in [
            ("forebygger", "forebygg"),
            ("forenkla", "forenkl"),
            ("havnevirksomhetene", "havnevirksom"),
            ("hinder", "hind"),
            ("alltids", "alltid"),
            ("lovleg", "lov"),
            ("", ""),
        ] {
            assert_eq!(s(input), want, "stem({input})");
        }
    }

    #[test]
    fn uppercase_is_folded_first() {
        assert_eq!(s("FOREBYGGER"), "forebygg");
    }

    #[test]
    fn get_r1_distinguishes_null_from_empty() {
        let st = PorterStemmerNo::new();
        assert_eq!(st.get_r1("aa"), None);
        assert_eq!(st.get_r1("xyz"), None);
        assert_eq!(st.get_r1("ab-cder"), None);
        assert_eq!(st.get_r1("abc"), Some(String::new()));
        assert_eq!(st.get_r1("ert"), Some(String::new()));
        assert_eq!(st.get_r1("hinder"), Some("der".to_owned()));
        assert_eq!(st.get_r1("forebygger"), Some("ebygger".to_owned()));
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
            ("café", "café"),
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

    /// The two characters this file cannot tell apart.
    ///
    /// `U+1F600` is one Unicode scalar value and two UTF-16 code units;
    /// `U+4E2D` is one of each. Nothing in this file distinguishes them: the
    /// vowel class is `[aeiouyæåø]`, R1's capture class is
    /// `[A-Za-z0-9_æøåÆØÅäÄöÖüÜ]`, step 1b's consonant list and step 2's pairs
    /// are ASCII, every table entry is ASCII, and `str::to_lowercase` leaves
    /// both alone. Neither is in any of those sets, so the only thing about
    /// either that can influence the result is a position or a length — which
    /// is the unit under test.
    const ASTRAL: char = '😀';
    /// The Basic Multilingual Plane twin of [`ASTRAL`]; see there.
    const BMP_TWIN: char = '中';

    /// Every entry of the Norwegian stop-word list and of every rule table,
    /// with one inert character inserted at every character position, paired
    /// with the same insertion of its BMP twin.
    fn inert_placements() -> Vec<(String, String)> {
        let mut corpus: Vec<&str> = Language::No.defaults().to_vec();
        for (_, table) in audit::TABLES {
            corpus.extend_from_slice(table);
        }
        let mut out = Vec::new();
        for w in corpus {
            for at in w.char_indices().map(|(i, _)| i).chain([w.len()]) {
                let (mut astral, mut bmp) = (w.to_owned(), w.to_owned());
                astral.insert(at, ASTRAL);
                bmp.insert(at, BMP_TWIN);
                out.push((astral, bmp));
            }
        }
        out
    }

    /// An inert character occupies **one** position, whichever plane it lives
    /// on — enumerated over the whole stop-word list and every rule table
    /// rather than sampled.
    ///
    /// Norwegian's cuts come from region arithmetic — `R1Scan`'s `start: 3`
    /// and `at_len`'s `index + 2 < len` — rather than from a matched suffix
    /// length, so nothing about the rule tables bounds where a cut can land.
    /// Only the buffer's own unit does.
    #[test]
    fn an_astral_character_occupies_one_position() {
        let st = PorterStemmerNo::new();
        let cases = inert_placements();
        let twin = BMP_TWIN.to_string();
        // Pinned so an enumeration that quietly walked nothing cannot pass:
        // 129 stop words and 41 rule-table entries, each probed at every one of its
        // `len + 1` character positions.
        assert_eq!(cases.len(), 744, "the enumerated corpus changed size");
        let mut diverged: Vec<(String, String, String)> = Vec::new();
        let mut invented: Vec<String> = Vec::new();
        for (astral, bmp) in &cases {
            let got = st.stem(astral).into_owned();
            if got.contains('\u{FFFD}') {
                invented.push(astral.clone());
            }
            let want = st.stem(bmp).into_owned();
            if got.replace(ASTRAL, &twin) != want {
                diverged.push((astral.clone(), got, want));
            }
        }
        // One assertion for both defects, so a failing run reports both counts
        // rather than stopping at the first.
        assert!(
            invented.is_empty() && diverged.is_empty(),
            "of {} placements, {} come back carrying a replacement character \
             the caller never supplied ({:?}) and {} measure an astral \
             character as more than one position ({:?})",
            cases.len(),
            invented.len(),
            &invented[..invented.len().min(3)],
            diverged.len(),
            &diverged[..diverged.len().min(3)]
        );
    }

    /// No stem carries a character the input did not.
    ///
    /// Steps 1a, 1b, 2 and 3 truncate and step 1c appends `er` after a token
    /// that ends in `ert`/`erte`, so every character of the result comes from
    /// the input — a claim a buffer that can be cut between the halves of a
    /// surrogate pair cannot make. The corpus reaches R1's interior case:
    /// the capture class excludes `-`, so a hyphen makes R1 stop before the
    /// token's end and the suffix found in R1 is then not a suffix of the
    /// token.
    #[test]
    fn no_cut_can_split_a_character() {
        let st = PorterStemmerNo::new();
        let mut corpus: Vec<String> = Vec::new();
        for stem in ["hind", "forebygg", "havn", "lov", "akkumul", "bok"] {
            for tail in [
                "a", "e", "en", "er", "hetene", "hetens", "ert", "erte", "ig", "s",
            ] {
                for filler in ["", "-", "1"] {
                    corpus.push(format!("{stem}{filler}{tail}"));
                }
            }
        }
        let mut probes = 0usize;
        for word in &corpus {
            for at in word.char_indices().map(|(i, _)| i).chain([word.len()]) {
                let mut probe = word.clone();
                probe.insert(at, ASTRAL);
                probes += 1;
                let got = st.stem(&probe).into_owned();
                assert!(
                    !got.contains('\u{FFFD}'),
                    "stem({probe:?}) = {got:?} — a character the caller never supplied"
                );
                assert!(
                    got.chars().all(|c| probe.contains(c)),
                    "stem({probe:?}) = {got:?} holds a character the input does not"
                );
            }
        }
        // 6 stems x 10 tails x 3 fillers = 180 words, each probed at every one
        // of its `len + 1` positions: 30*29 + 18*28 + 60*2 + 180 = 1674.
        assert_eq!(corpus.len(), 6 * 10 * 3);
        assert_eq!(probes, 1_674, "the probe corpus changed size");
    }

    /// `R1Scan::at_len` must agree with a fresh `getR1` of the truncated
    /// word at every truncation point, which is what lets `stem` scan once.
    #[test]
    fn a_cached_scan_matches_a_rescan_of_every_prefix() {
        let mut rng = Rng(0x5EED_0DDB_1A5E_5BAD);
        for _ in 0..20_000 {
            let word = random_word(&mut rng).to_lowercase();
            let t = scalars(&word);
            let scan = R1Scan::of(&t);
            for len in 0..=t.len() {
                assert_eq!(
                    scan.at_len(len),
                    r1_range(&t[..len]),
                    "{word:?} truncated to {len}"
                );
            }
        }
    }

    /// The bitmask forms must agree with the literal classes the reference
    /// writes, for **every** Unicode scalar value — not just the Basic
    /// Multilingual Plane, since the buffer now holds whole characters and an
    /// astral one has to answer `false` to both on its own account rather than
    /// through a surrogate half.
    #[test]
    fn character_classes_match_the_literal_sets() {
        for cp in 0..=0x10_FFFFu32 {
            let Some(c) = char::from_u32(cp) else {
                continue;
            };
            // `[aeiouyæåø]`
            let vowel = matches!(c, 'a' | 'e' | 'i' | 'o' | 'u' | 'y' | 'æ' | 'å' | 'ø');
            assert_eq!(is_vowel(c), vowel, "vowel U+{cp:04X}");
            // `[A-Za-z0-9_æøåÆØÅäÄöÖüÜ]`
            let r1 = matches!(c,
                '0'..='9' | 'A'..='Z' | '_' | 'a'..='z'
                | 'æ' | 'ø' | 'å' | 'Æ' | 'Ø' | 'Å'
                | 'ä' | 'Ä' | 'ö' | 'Ö' | 'ü' | 'Ü');
            assert_eq!(is_r1_char(c), r1, "r1 class U+{cp:04X}");
        }
    }

    /// The two hand-written matchers `stem` uses in place of a table search
    /// must answer exactly what searching that table would, over every
    /// region and every pair of Latin-1 characters — otherwise the tables
    /// they replaced would still be the specification.
    #[test]
    fn the_hand_written_matchers_agree_with_their_tables() {
        let dt_vt: AmongTable<char> = AmongTable::build(&["dt", "vt"]);
        for a in 0..=0xFFu32 {
            for b in 0..=0xFFu32 {
                let (ca, cb) = (
                    char::from_u32(a).expect("U+0000-U+00FF are all scalar values"),
                    char::from_u32(b).expect("U+0000-U+00FF are all scalar values"),
                );
                let w = ['x', ca, cb];
                for lb in 0..=3usize {
                    for cursor in lb..=3usize {
                        assert_eq!(
                            ends_dt_vt(&w, cursor, lb),
                            dt_vt.longest(&w, cursor, lb) > 0,
                            "dt/vt: characters {a:#06X},{b:#06X} region {lb}..{cursor}"
                        );
                    }
                }
            }
        }
        // An astral character is one position, so it cannot half-match a pair.
        assert!(!ends_dt_vt(&scalars("a😀t"), 3, 0));
        assert!(ends_dt_vt(&scalars("😀dt"), 3, 0));
        // `ert_suffix_len` takes no limit — step 1c matches the whole token —
        // so it is checked against the unrestricted longest-suffix helper.
        let mut rng = Rng(0xA5A5_1234_DEAD_BEEF);
        for _ in 0..20_000 {
            let word = random_word(&mut rng).to_lowercase();
            let t = scalars(&word);
            let want = longest_listed_suffix(&t, ERT).map_or(0, slen);
            assert_eq!(ert_suffix_len(&t), want, "ert/erte: {word:?}");
        }
        for word in [
            "ert", "erte", "aerte", "xert", "er", "erter", "", "t", "😀ert",
        ] {
            let t = scalars(word);
            assert_eq!(
                ert_suffix_len(&t),
                longest_listed_suffix(&t, ERT).map_or(0, slen),
                "ert/erte: {word:?}"
            );
        }
    }

    /// The result is a slice of the caller's own string whenever lowercasing
    /// was a no-op and only truncation happened — the allocation-free path
    /// `crate::units::borrowed_prefix` exists for. Uppercase input, non-ASCII
    /// input and step 1c's rewrite must all still return owned data.
    #[test]
    fn an_unrewritten_ascii_word_is_returned_borrowed() {
        let st = PorterStemmerNo::new();
        assert!(matches!(st.stem("forebygger"), Cow::Borrowed("forebygg")));
        assert!(matches!(st.stem("hinder"), Cow::Borrowed("hind")));
        // Unchanged words borrow too.
        assert!(matches!(st.stem("xyz"), Cow::Borrowed("xyz")));
        // Lowercasing changed the word, so there is nothing to borrow from.
        assert!(matches!(st.stem("FOREBYGGER"), Cow::Owned(_)));
        // Step 1c rewrites the tail rather than truncating.
        assert!(matches!(st.stem("akkumulerte"), Cow::Owned(_)));
        // Non-ASCII: a character count is not a byte index.
        assert!(matches!(st.stem("æøå"), Cow::Owned(_)));
        // Whatever the variant, the content is what it always was.
        assert_eq!(st.stem("FOREBYGGER"), "forebygg");
        assert_eq!(st.stem("akkumulerte"), "akkumuler");
    }

    #[test]
    fn norwegian_specifics() {
        assert_eq!(s("Æ Ø Å"), "æ ø å");
    }

    /// `prepare` rewrites nothing and borrows everything.
    ///
    /// This is the decision recorded on [`PorterStemmerNo`]: `æ`, `ø` and `å`
    /// are letters of the Norwegian alphabet, so a diacritic fold here merges
    /// distinct words, half-folds the alphabet the rules are written over
    /// (`å` decomposes, `æ` and `ø` do not), and deletes 8 stop words.
    #[test]
    fn prepare_is_the_identity_and_never_allocates() {
        for text in [
            "blåbærsyltetøy",
            "à la façon",
            "ààà",
            "forebygger",
            "Æ Ø Å",
            "",
        ] {
            assert!(
                matches!(PorterStemmerNo::prepare(text), Cow::Borrowed(t) if t == text),
                "prepare rewrote {text:?}"
            );
        }
    }

    /// The alphabet argument, at the level a caller sees it: folding `å` would
    /// collapse pairs that Norwegian spells differently because they *are*
    /// different words.
    #[test]
    fn folding_would_merge_distinct_words() {
        let st = PorterStemmerNo::new();
        // `hår` ("hair") vs `har` ("has"); `måte` ("way") vs `mate` ("feed").
        assert_ne!(
            st.tokenize_and_stem("hår", true),
            st.tokenize_and_stem("har", true)
        );
        assert_ne!(
            st.tokenize_and_stem("måte", true),
            st.tokenize_and_stem("mate", true)
        );
    }

    #[test]
    fn erte_becomes_er() {
        assert_eq!(PorterStemmerNo::new().step1c("akkumulerte"), "akkumuler");
        // R1 of "erte" is "e", which contains neither alternative, so 1c is a
        // no-op even though the token itself ends in "erte".
        assert_eq!(PorterStemmerNo::new().step1c("erte"), "erte");
    }

    /// The pre-`find_among` pipeline, which still exists verbatim as the
    /// public per-step methods: `stem` must equal running them in sequence.
    fn oracle(token: &str) -> String {
        let lower = token.to_lowercase();
        let t: Vec<char> = lower.chars().collect();
        text(&step3(&step2(&step1(&t))))
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

    /// Norwegian stems crossed with real table suffixes (stacked up to two
    /// deep) plus the characters that make R1 interior (`-`, digits) and
    /// case/astral/CJK noise.
    fn random_word(rng: &mut Rng) -> String {
        const ALPHA: &[char] = &[
            'a', 'b', 'd', 'e', 'f', 'g', 'h', 'i', 'k', 'l', 'm', 'n', 'o', 'p', 'r', 's', 't',
            'u', 'v', 'y', 'æ', 'ø', 'å', 'ä', 'ö', 'ü', '_',
        ];
        const SUFFIXES: &[&str] = &[
            "hetenes", "hetene", "hetens", "heten", "heter", "endes", "ande", "ende", "edes",
            "ene", "ens", "ers", "ets", "het", "ast", "erte", "ert", "dt", "vt", "ks", "ts", "s",
            "a", "e", "en", "ar", "er", "as", "es", "et", "hetslov", "slov", "elov", "eleg",
            "elig", "eig", "lig", "leg", "els", "lov", "ig",
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
        match rng.below(30) {
            0 => s = s.to_uppercase(),
            1 => s.push('😀'),
            2 => s.insert(0, '日'),
            3 => s.push_str("123"),
            4 => {
                // A hyphen makes R1 stop before the token's end — the case
                // where a suffix found in R1 is not a suffix of the token.
                let at = rng.below(s.len().max(1)).min(s.len());
                if s.is_char_boundary(at) {
                    s.insert(at, '-');
                }
            }
            _ => {}
        }
        s
    }

    #[test]
    fn differential_against_the_per_step_oracle() {
        let stemmer = PorterStemmerNo::new();
        let check = |input: &str| {
            assert_eq!(
                stemmer.stem(input).as_ref(),
                oracle(input),
                "stem({input:?})"
            );
        };
        for w in crate::test_support::bench_words("no") {
            check(&w);
        }
        for w in [
            "",
            "a",
            "ab-cder",
            "abc",
            "ert",
            "erte",
            "hinder",
            "forebygger",
            "alltids",
            "lovleg",
            "havnevirksomhetene",
            "akkumulerte",
            "ks",
            "boks",
            "æøå",
        ] {
            check(w);
        }
        let mut rng = Rng(0x0DDB_1A5E_5BAD_5EED);
        for _ in 0..60_000 {
            let w = random_word(&mut rng);
            check(&w);
        }
    }
}
