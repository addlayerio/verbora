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
//!
//! # The text unit
//!
//! Every position here is a **Unicode scalar value**: R1, R2 and RV, the
//! `length == 1` gate, the `t[..3]` prefix the `par`/`col`/`tap` test reads,
//! the literal `rv = 3`, and every cut. See [`crate::units`] for why that is
//! the faithful reading of a Snowball algorithm rather than a preference — the
//! specification is written over *letters*, and the UTF-16 indices this port
//! used to carry were an artefact of the host language it was transcribed
//! through, not of the algorithm.
//!
//! Two things make the choice visible, and the second is the one worth stating
//! carefully because it is easy to argue oneself out of.
//!
//! [`PorterStemmerFr::regions`] hands the positions themselves to a caller who
//! is holding a `&str`, so `regions("😀parler")` is `{ r1: 4, r2: 7, rv: 3 }` —
//! offsets into the seven characters the caller can see, where the code-unit
//! reading answered `{ 5, 8, 4 }` and put `r2` past the end of the word.
//!
//! `stem` is affected too. Most of its region checks are *relative* — a match
//! length against `len - region` — and an inserted character shifts `len` and
//! `region` together, so those cancel. RV's literal **`rv = 3`** does not
//! cancel, because it is an absolute position: a word opening with two vowels,
//! or with `par`/`col`/`tap`, gets that 3 whatever follows it, so under the
//! code-unit reading an astral character anywhere later made RV one unit longer
//! than the word had letters. Step 2a's "RV must be strictly longer than the
//! match" test then read differently, and `stem("au😀îmes")` — seven letters,
//! RV of four, against the four-letter `-îmes` — came back `"au😀"` where it is
//! now `"au😀îm"`.

use std::borrow::Cow;
use std::sync::LazyLock;

use crate::among::{AmongTable, Buf, UnionTable, longest_at_most};
use crate::base::{Casing, TokenizeAndStem};
use crate::data::gates::gate_fr;
use crate::stopwords::Language;
use crate::units::{at, ends_with, longest_suffix, text, text_lowercase};

/// The working buffer for a `&str`, in this crate's text unit.
#[inline]
fn scalars(s: &str) -> Vec<char> {
    s.chars().collect()
}

/// The French Snowball stemmer.
///
/// ```
/// use verbora_stemmers::PorterStemmerFr;
/// let s = PorterStemmerFr::new();
/// assert_eq!(s.stem("volerait"), "vol");
/// assert_eq!(s.stem("publicité"), "publiqu");
/// assert_eq!(s.stem(""), "");
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PorterStemmerFr;

/// The R1, R2 and RV offsets a French word is divided into, in **Unicode
/// scalar values**.
///
/// Every field is an index into the word's characters, so a caller holding the
/// `&str` reaches the region with `token.chars().skip(r.r1)`.
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
/// `U`, `I` and `Y` that `prelude` writes stop counting as vowels. Nothing
/// outside the Basic Multilingual Plane is in it either, so an astral character
/// is a consonant to every region scan.
#[inline]
fn is_vowel(c: char) -> bool {
    matches!(
        c,
        'a' | 'e'
            | 'i'
            | 'o'
            | 'u'
            | 'y'
            | 'â'
            | 'à'
            | 'ë'
            | 'é'
            | 'ê'
            | 'è'
            | 'ï'
            | 'î'
            | 'ô'
            | 'û'
            | 'ù'
    )
}

#[inline]
fn vowel_at(w: &[char], i: usize) -> bool {
    at(w, i).is_some_and(is_vowel)
}

/// Removes `n` characters from the end and appends `tail`.
fn cut(buf: &mut Buf<char>, n: usize, tail: &str) {
    buf.truncate(buf.len().saturating_sub(n));
    buf.push_str(tail);
}

/// `prelude` applied to a buffer that already holds the lowercased token.
///
/// # Why the one-character lag is needed
///
/// Every test in `prelude` reads the **original** token, never the partly
/// rewritten result, so an already-marked neighbour still counts as a vowel.
/// Rewriting in place would destroy `t[i - 1]` before `t[i]` is decided, so
/// the original left neighbour is carried in `prev`; the right neighbour is
/// still untouched at that point and can be read from the buffer directly.
/// The transform is one character in, one character out, which is what makes an
/// in-place form possible at all and removes the `prelude` output `Vec`.
fn prelude_in_place(buf: &mut Buf<char>) {
    if buf.len() == 0 {
        // The empty token has nothing to mark. See `PorterStemmerFr::prelude`
        // for what used to happen here instead.
        return;
    }
    let w = buf.as_mut_slice();
    let n = w.len();
    let mut prev = '\0';
    for i in 0..n {
        let c = w[i];
        let next_vowel = i + 1 < n && is_vowel(w[i + 1]);
        let prev_vowel = i > 0 && is_vowel(prev);
        w[i] = if i == 0 {
            if c == 'y' && next_vowel { 'Y' } else { c }
        } else if (c == 'u' || c == 'i') && prev_vowel && next_vowel {
            // The guard has just established that this is `u` or `i`, so the
            // ASCII fold is the reference's `charCodeAt - 32` exactly.
            c.to_ascii_uppercase()
        } else if c == 'y' && (prev_vowel || next_vowel) {
            'Y'
        } else if c == 'u' && prev == 'q' {
            'U'
        } else {
            c
        };
        prev = c;
    }
}

/// The sorted search tables, built once from the rule tables below.
struct FrTables {
    /// Every step-1 rule table merged into one search; the `T_*` constants
    /// name each table's slot in the mask array.
    step1: UnionTable<char>,
    /// Also a step-1 table, kept standalone because one rule searches it
    /// again after a cut, when the merged mask no longer describes the word.
    atif: AmongTable<char>,
    step2a: AmongTable<char>,
    step2b_e: AmongTable<char>,
    step2b_a: AmongTable<char>,
    ier: AmongTable<char>,
    undouble: AmongTable<char>,
}

/// The step-1 rule tables, in chain order. Their positions are the `T_*`
/// constants and the ids [`UnionTable::length_masks`] writes.
static STEP1_TABLE_LIST: &[&[&str]] = &[
    STEP1_ANCE, ICATRICE, ATRICE, LOGIE, USION, ENCE, ISSEMENT, ATIVEMENT, IVEMENT, EUSEMENT,
    ABLEMENT, IEREMENT, EMENT, ICITE, ABILITE, ITE, ICATIF, ATIF, IF_IVE, EUSE, MENT,
];
/// The number of step-1 tables, and so the size of the mask array.
const STEP1_TABLES: usize = 21;
const T_ANCE: usize = 0;
const T_ICATRICE: usize = 1;
const T_ATRICE: usize = 2;
const T_LOGIE: usize = 3;
const T_USION: usize = 4;
const T_ENCE: usize = 5;
const T_ISSEMENT: usize = 6;
const T_ATIVEMENT: usize = 7;
const T_IVEMENT: usize = 8;
const T_EUSEMENT: usize = 9;
const T_ABLEMENT: usize = 10;
const T_IEREMENT: usize = 11;
const T_EMENT: usize = 12;
const T_ICITE: usize = 13;
const T_ABILITE: usize = 14;
const T_ITE: usize = 15;
const T_ICATIF: usize = 16;
const T_ATIF: usize = 17;
const T_IF_IVE: usize = 18;
const T_EUSE: usize = 19;
const T_MENT: usize = 20;

static TABLES: LazyLock<FrTables> = LazyLock::new(|| FrTables {
    step1: UnionTable::build(STEP1_TABLE_LIST),
    atif: AmongTable::build(ATIF),
    step2a: AmongTable::build(STEP2A),
    step2b_e: AmongTable::build(STEP2B_E),
    step2b_a: AmongTable::build(STEP2B_A),
    ier: AmongTable::build(IER),
    undouble: AmongTable::build(UNDOUBLE),
});

impl PorterStemmerFr {
    /// Creates the stemmer. It is stateless and zero-sized.
    #[inline]
    pub const fn new() -> Self {
        Self
    }

    /// The prelude: lower case, then mark the semivowels.
    ///
    /// `u` and `i` between two vowels become `U` and `I`, `y` next to a vowel
    /// becomes `Y`, and the `u` of `qu` becomes `U`. The marked forms are
    /// outside the vowel class, which is how the following region scan stops
    /// counting them as vowels — and why several rule tables are spelled with
    /// them (`iqUe`, `aIent`, `Ièrement`).
    ///
    /// Exposed because the marking is observable in the stem: `stem("voyiez")`
    /// is `"voi"`, and the `Y` is where that comes from.
    ///
    /// # Empty input
    ///
    /// `prelude("")` is `""`, and `stem("")` is `""`. Both used to be the
    /// nine-character string `"undefined"`: the marking loop was written to
    /// handle index 0 outside the loop and appended `token[0]` unconditionally,
    /// which for an empty token appended the *name* of the absent value. The
    /// literal then flowed through region marking and every step and came back
    /// out as the stem. It is not a word, not a stem of anything, and had no
    /// basis in the algorithm; every other stemmer in this crate returns `""`
    /// for `""` and this one now does too.
    pub fn prelude(token: &str) -> String {
        text(&Self::prelude_units(&scalars(&token.to_lowercase())))
    }

    fn prelude_units(t: &[char]) -> Vec<char> {
        if t.is_empty() {
            return Vec::new();
        }
        let mut out = Vec::with_capacity(t.len());
        out.push(if t[0] == 'y' && vowel_at(t, 1) {
            'Y'
        } else {
            t[0]
        });
        for i in 1..t.len() {
            // Every test reads the ORIGINAL token, never the partly-rewritten
            // result, so an already-marked neighbour still counts as a vowel.
            let c = t[i];
            if (c == 'u' || c == 'i') && vowel_at(t, i - 1) && vowel_at(t, i + 1) {
                out.push(c.to_ascii_uppercase());
            } else if c == 'y' && (vowel_at(t, i - 1) || vowel_at(t, i + 1)) {
                out.push('Y');
            } else if c == 'u' && t[i - 1] == 'q' {
                out.push('U');
            } else {
                out.push(c);
            }
        }
        out
    }

    /// The R1, R2 and RV offsets `token` is divided into, in **Unicode scalar
    /// values**.
    ///
    /// R1 is the region after the first non-vowel following a vowel, R2 the
    /// same taken inside R1, and RV the French-specific region every verb rule
    /// is checked against. Exposed because which region a suffix falls in is
    /// the whole reason a rule fires or does not, and a caller debugging an
    /// unexpected stem needs to see them.
    ///
    /// Each offset counts characters, so it indexes the same thing the caller's
    /// own `token.chars()` does — `regions("😀parler").r2` is 7, the length of
    /// a seven-character word, not the 8 a UTF-16 count would put past its end.
    pub fn regions(token: &str) -> Regions {
        Self::regions_units(&scalars(token))
    }

    fn regions_units(t: &[char]) -> Regions {
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
        longest_suffix(&scalars(token), suffixes).unwrap_or("")
    }

    /// Stems one token.
    #[allow(
        clippy::unused_self,
        reason = "every stemmer is zero-sized; `stem` is a method so the \
                  sixteen of them share one call shape"
    )]
    #[expect(
        clippy::too_many_lines,
        reason = "the else-if chains ARE the specification; splitting them hides the order"
    )]
    pub fn stem<'a>(&self, token: &'a str) -> Cow<'a, str> {
        let tb = &*TABLES;
        // Lowercase, prelude and working copy all land in one stack buffer:
        // the straightforward port allocated three `Vec`/`String`s before the
        // first rule ran, plus a fourth for the step-1 snapshot below.
        let mut buf: Buf<char> = Buf::fill_lowercase(token);
        prelude_in_place(&mut buf);
        if buf.len() == 1 {
            return Cow::Owned(buf.into_text());
        }
        let regs = Self::regions_units(buf.as_slice());
        // A `Buf` clone is a stack copy, so the reference's "has step 1
        // changed the word?" comparison stays a content comparison rather
        // than becoming a fragile change-flag.
        let before_step1 = buf.clone();
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
        let len = buf.len();
        let lb1 = regs.r1.min(len);
        let lb2 = regs.r2.min(len);
        let lbv = regs.rv.min(len);
        // One union search answers every step-1 table at once: the walk of
        // its substring links records, per rule table, the lengths of that
        // table's entries that are suffixes of the word, and each rule then
        // reads its own region-restricted longest match out of that mask.
        // The chain below is otherwise the reference's else-if ladder,
        // unchanged and in order.
        let mut lm = [0u32; STEP1_TABLES];
        tb.step1.length_masks(buf.as_slice(), len, &mut lm);
        let (av1, av2, avv) = (len - lb1, len - lb2, len - lbv);
        'step1: {
            let m = longest_at_most(lm[T_ANCE], av2);
            if m > 0 {
                cut(&mut buf, m, "");
                break 'step1;
            }
            let m = longest_at_most(lm[T_ICATRICE], len);
            if m > 0 {
                if longest_at_most(lm[T_ICATRICE], av2) > 0 {
                    cut(&mut buf, m, "");
                } else {
                    cut(&mut buf, m, "iqU");
                }
                break 'step1;
            }
            let m = longest_at_most(lm[T_ATRICE], av2);
            if m > 0 {
                cut(&mut buf, m, "");
                break 'step1;
            }
            let m = longest_at_most(lm[T_LOGIE], av2);
            if m > 0 {
                cut(&mut buf, m, "log");
                break 'step1;
            }
            let m = longest_at_most(lm[T_USION], av2);
            if m > 0 {
                cut(&mut buf, m, "u");
                break 'step1;
            }
            let m = longest_at_most(lm[T_ENCE], av2);
            if m > 0 {
                cut(&mut buf, m, "ent");
                break 'step1;
            }
            let m = longest_at_most(lm[T_ISSEMENT], av1);
            if m > 0 {
                // The rule consumes the chain even when the vowel test
                // declines the cut — a plain `if` inside the branch.
                if !(len > m && is_vowel(buf.as_slice()[len - m - 1])) {
                    cut(&mut buf, m, "");
                }
                break 'step1;
            }
            let m = longest_at_most(lm[T_ATIVEMENT], av2);
            if m > 0 {
                cut(&mut buf, m, "");
                break 'step1;
            }
            let m = longest_at_most(lm[T_IVEMENT], av2);
            if m > 0 {
                cut(&mut buf, m, "");
                break 'step1;
            }
            if longest_at_most(lm[T_EUSEMENT], len) > 0 {
                let m = longest_at_most(lm[T_EUSEMENT], av2);
                if m > 0 {
                    cut(&mut buf, m, "");
                } else {
                    let m = longest_at_most(lm[T_EUSEMENT], av1);
                    if m > 0 {
                        cut(&mut buf, m, "eux");
                    } else {
                        let m = longest_at_most(lm[T_EMENT], avv);
                        if m > 0 {
                            cut(&mut buf, m, "");
                        }
                    }
                }
                break 'step1;
            }
            let m = longest_at_most(lm[T_ABLEMENT], av2);
            if m > 0 {
                cut(&mut buf, m, "");
                break 'step1;
            }
            let m = longest_at_most(lm[T_IEREMENT], avv);
            if m > 0 {
                cut(&mut buf, m, "i");
                break 'step1;
            }
            let m = longest_at_most(lm[T_EMENT], avv);
            if m > 0 {
                cut(&mut buf, m, "");
                break 'step1;
            }
            let m = longest_at_most(lm[T_ICITE], len);
            if m > 0 {
                if longest_at_most(lm[T_ICITE], av2) > 0 {
                    cut(&mut buf, m, "");
                } else {
                    cut(&mut buf, m, "iqU");
                }
                break 'step1;
            }
            let m = longest_at_most(lm[T_ABILITE], len);
            if m > 0 {
                if longest_at_most(lm[T_ABILITE], av2) > 0 {
                    cut(&mut buf, m, "");
                } else {
                    cut(&mut buf, m, "abl");
                }
                break 'step1;
            }
            let m = longest_at_most(lm[T_ITE], av2);
            if m > 0 {
                cut(&mut buf, m, "");
                break 'step1;
            }
            if longest_at_most(lm[T_ICATIF], len) > 0 {
                // Two consecutive `if`s, not an if/else — both can fire.
                let m = longest_at_most(lm[T_ICATIF], av2);
                if m > 0 {
                    cut(&mut buf, m, "");
                }
                // This one reads the *cut* buffer, so it is a real search
                // rather than a mask lookup — the mask describes the word as
                // it was before the branch.
                let len2 = buf.len();
                let m = tb.atif.longest(buf.as_slice(), len2, regs.r2.min(len2));
                if m > 0 {
                    cut(&mut buf, m + 2, "iqU");
                }
                break 'step1;
            }
            let m = longest_at_most(lm[T_ATIF], av2);
            if m > 0 {
                cut(&mut buf, m, "");
                break 'step1;
            }
            let m = longest_at_most(lm[T_IF_IVE], av2);
            if m > 0 {
                cut(&mut buf, m, "");
                break 'step1;
            }
            if ends_with(buf.as_slice(), "eaux") {
                cut(&mut buf, 4, "eau");
                break 'step1;
            }
            if ends_with(buf.as_slice(), "aux") && av1 >= 3 {
                cut(&mut buf, 3, "al");
                break 'step1;
            }
            let m = longest_at_most(lm[T_EUSE], av2);
            if m > 0 {
                cut(&mut buf, m, "");
                break 'step1;
            }
            let m = longest_at_most(lm[T_EUSE], av1);
            if m > 0 {
                cut(&mut buf, m, "eux");
                break 'step1;
            }
            if ends_with(buf.as_slice(), "amment") && avv >= 6 {
                cut(&mut buf, 6, "ant");
                do_step2a = true;
                break 'step1;
            }
            if ends_with(buf.as_slice(), "emment") && avv >= 6 {
                cut(&mut buf, 6, "ent");
                do_step2a = true;
                break 'step1;
            }
            let m = longest_at_most(lm[T_MENT], avv);
            if m > 0 {
                // A vowel must precede, and that vowel must be inside RV.
                if len > m && is_vowel(buf.as_slice()[len - m - 1]) && avv > m {
                    cut(&mut buf, m, "");
                    do_step2a = true;
                }
                break 'step1;
            }
        }

        // --- Step 2a -------------------------------------------------------
        let mut step2a_done = false;
        let mut step2a_changed = false;
        if before_step1.as_slice() == buf.as_slice() || do_step2a {
            step2a_done = true;
            let len = buf.len();
            let lbv = regs.rv.min(len);
            let m = tb.step2a.longest(buf.as_slice(), len, lbv);
            if m > 0 && len > m {
                let prev = buf.as_slice()[len - m - 1];
                if !is_vowel(prev) && len - lbv > m {
                    cut(&mut buf, m, "");
                    step2a_changed = true;
                }
            }
        }

        // --- Step 2b -------------------------------------------------------
        if step2a_done && !step2a_changed {
            let len = buf.len();
            let lbv = regs.rv.min(len);
            let m = tb.step2b_e.longest(buf.as_slice(), len, lbv);
            if m > 0 {
                cut(&mut buf, m, "");
            } else if ends_with(buf.as_slice(), "ions")
                && len - lbv >= 4
                && len - regs.r2.min(len) >= 4
            {
                cut(&mut buf, 4, "");
            } else {
                let m = tb.step2b_a.longest(buf.as_slice(), len, lbv);
                if m > 0 {
                    // The `e` test below reads the PRE-deletion RV,
                    // deliberately — capture its length before the cut.
                    let rv_len_before = len - lbv;
                    cut(&mut buf, m, "");
                    if buf.as_slice().last() == Some(&'e') && rv_len_before > m {
                        buf.truncate(buf.len() - 1);
                    }
                }
            }
        }

        if buf.as_slice() != before_step1.as_slice() {
            // --- Step 3 ----------------------------------------------------
            if buf.as_slice().last() == Some(&'Y') {
                let n = buf.len();
                buf.as_mut_slice()[n - 1] = 'i';
            }
            if buf.as_slice().last() == Some(&'ç') {
                let n = buf.len();
                buf.as_mut_slice()[n - 1] = 'c';
            }
        } else {
            // --- Step 4: residual ------------------------------------------
            let last = buf.as_slice().last().copied();
            let second_last = if buf.len() >= 2 {
                Some(buf.as_slice()[buf.len() - 2])
            } else {
                None
            };
            if last == Some('s')
                && !second_last.is_some_and(|c| matches!(c, 'a' | 'i' | 'o' | 'u' | 'è' | 's'))
            {
                buf.truncate(buf.len() - 1);
            }
            {
                let len = buf.len();
                if ends_with(buf.as_slice(), "ion") && len - regs.r2.min(len) >= 3 {
                    let before = if len >= 4 {
                        Some(buf.as_slice()[len - 4])
                    } else {
                        None
                    };
                    if before.is_some_and(|c| c == 's' || c == 't') {
                        cut(&mut buf, 3, "");
                    }
                }
            }
            {
                let len = buf.len();
                let lbv = regs.rv.min(len);
                let m = tb.ier.longest(buf.as_slice(), len, lbv);
                if m > 0 {
                    cut(&mut buf, m, "i");
                }
            }
            // `endsinArr(rvtxt, 'e')`: a STRING, iterated as its characters.
            {
                let len = buf.len();
                let lbv = regs.rv.min(len);
                if len > lbv && buf.as_slice()[len - 1] == 'e' {
                    buf.truncate(len - 1);
                }
            }
            {
                let len = buf.len();
                let lbv = regs.rv.min(len);
                if len > lbv && buf.as_slice()[len - 1] == 'ë' {
                    // `token.slice(token.length - 3, -1)`: a start argument
                    // below zero is treated as an offset from the end, not
                    // clamped, so for a token shorter than three units this
                    // can never be "gu".
                    if len >= 3 && buf.as_slice()[len - 3] == 'g' && buf.as_slice()[len - 2] == 'u'
                    {
                        buf.truncate(len - 1);
                    }
                }
            }
        }

        // --- Step 5: undouble ----------------------------------------------
        if tb.undouble.longest(buf.as_slice(), buf.len(), 0) > 0 {
            buf.truncate(buf.len() - 1);
        }

        // --- Step 6: un-accent the final é/è -------------------------------
        let w = buf.as_mut_slice();
        let mut i = w.len().saturating_sub(1);
        while i > 0 {
            if !is_vowel(w[i]) {
                i -= 1;
            } else if i != w.len() - 1 && (w[i] == 'é' || w[i] == 'è') {
                w[i] = 'e';
                break;
            } else {
                break;
            }
        }

        Cow::Owned(text_lowercase(buf.as_mut_slice()))
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
static LOGIE: &[&str] = &["logie", "logies"];
static USION: &[&str] = &["usion", "ution", "usions", "utions"];
static ENCE: &[&str] = &["ence", "ences"];
static ISSEMENT: &[&str] = &["issement", "issements"];
static ATIVEMENT: &[&str] = &["ativement", "ativements"];
static IVEMENT: &[&str] = &["ivement", "ivements"];
static EUSEMENT: &[&str] = &["eusement", "eusements"];
static ABLEMENT: &[&str] = &["ablement", "ablements", "iqUement", "iqUements"];
static IEREMENT: &[&str] = &["ièrement", "ièrements", "Ièrement", "Ièrements"];
static EMENT: &[&str] = &["ement", "ements"];
static ICITE: &[&str] = &["icité", "icités"];
static ABILITE: &[&str] = &["abilité", "abilités"];
static ITE: &[&str] = &["ité", "ités"];
static IF_IVE: &[&str] = &["if", "ive", "ifs", "ives"];
static EUSE: &[&str] = &["euse", "euses"];
static MENT: &[&str] = &["ment", "ments"];
static ICATIF: &[&str] = &["icatif", "icative", "icatifs", "icatives"];
static ATIF: &[&str] = &["atif", "ative", "atifs", "atives"];
/// Step 3's `ier` group, in both the plain and the `I`-marked spelling.
static IER: &[&str] = &["ier", "i\u{e8}re", "Ier", "I\u{e8}re"];
/// Step 4's undoubling group.
static UNDOUBLE: &[&str] = &["enn", "onn", "ett", "ell", "eill"];
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

    fn is_stop_word(word: &str) -> bool {
        Language::Fr.contains(word)
    }

    fn gate(token: &str) -> bool {
        token.chars().any(is_french_letter)
    }

    fn stem_token(&self, token: &str) -> String {
        self.stem(token).into_owned()
    }
}

/// Whether `c` is one of the letters [`gate_fr`] accepts.
///
/// The gate is stated over Basic Multilingual Plane code points and nothing in
/// it reaches `U+00FD`, so scanning characters and scanning UTF-16 code units
/// accept exactly the same tokens: a BMP character *is* its own code unit, and
/// an astral character is neither in the set itself nor are the two surrogates
/// it used to be scanned as. The scan is per character because that is the
/// crate's unit, not because the answer moved.
#[inline]
fn is_french_letter(c: char) -> bool {
    (c as u32) < 0x1_0000 && gate_fr(c as u16)
}

/// What [`crate::data::table_audit`] needs to walk this language's tables.
#[cfg(test)]
pub(crate) mod audit {
    /// Every rule table, named.
    pub(crate) static TABLES: &[(&str, &[&str])] = &[
        ("STEP1_ANCE", super::STEP1_ANCE),
        ("ICATRICE", super::ICATRICE),
        ("ATRICE", super::ATRICE),
        ("LOGIE", super::LOGIE),
        ("USION", super::USION),
        ("ENCE", super::ENCE),
        ("ISSEMENT", super::ISSEMENT),
        ("ATIVEMENT", super::ATIVEMENT),
        ("IVEMENT", super::IVEMENT),
        ("EUSEMENT", super::EUSEMENT),
        ("ABLEMENT", super::ABLEMENT),
        ("IEREMENT", super::IEREMENT),
        ("EMENT", super::EMENT),
        ("ICITE", super::ICITE),
        ("ABILITE", super::ABILITE),
        ("ITE", super::ITE),
        ("ICATIF", super::ICATIF),
        ("ATIF", super::ATIF),
        ("IF_IVE", super::IF_IVE),
        ("EUSE", super::EUSE),
        ("MENT", super::MENT),
        ("STEP2A", super::STEP2A),
        ("STEP2B_E", super::STEP2B_E),
        ("STEP2B_A", super::STEP2B_A),
        ("IER", super::IER),
        ("UNDOUBLE", super::UNDOUBLE),
    ];

    /// The prelude `stem` runs before any table is consulted, in isolation.
    pub(crate) fn prelude(token: &str) -> String {
        super::PorterStemmerFr::prelude(token)
    }

    /// The units the prelude writes, paired with what it writes them for.
    pub(crate) static MARKERS: &[(&str, &str)] = &[("I", "i"), ("U", "u"), ("Y", "y")];
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

    /// The empty token stems to the empty token.
    ///
    /// This asserted `"undefined"` for as long as the crate has existed — a
    /// nine-character string fabricated by the prelude out of an absent value
    /// and then stemmed as if it were a French word. See
    /// [`PorterStemmerFr::prelude`].
    #[test]
    fn empty_input_stems_to_the_empty_string() {
        assert_eq!(s(""), "");
        assert_eq!(PorterStemmerFr::prelude(""), "");
        assert_eq!(PorterStemmerFr::new().stem(""), "");
        // And the pipeline agrees: no token, no stem.
        assert_eq!(
            PorterStemmerFr::new().tokenize_and_stem("", true),
            Vec::<String>::new()
        );
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

    /// [`Regions`] are positions in **Unicode scalar values**, so they index
    /// the same thing the caller's own `chars()` does.
    ///
    /// Derived from the algorithm rather than recorded from it. `"😀parler"` is
    /// seven letters — `😀 p a r l e r`. R1 is the position after the first
    /// non-vowel following a vowel: `a` at 2 is a vowel and `r` at 3 is not, so
    /// R1 is 4. R2 repeats that inside R1: `e` at 5 is a vowel and `r` at 6 is
    /// not, so R2 is 7, the length. RV takes neither special arm — `😀` is not
    /// a vowel, and the first three letters are `😀pa`, not `par` — so the
    /// fallback loop runs and stops at the first vowel after index 0, the `a`
    /// at 2, giving `rv = 3`.
    ///
    /// The code-unit reading answered `{ r1: 5, r2: 8, rv: 4 }` for this word:
    /// three offsets into a seven-character string, one of them past its end.
    ///
    /// `"ñparler"` is the control, and it is a control rather than a
    /// decoration: `ñ` is outside `isVowel` exactly as `😀` is, and is not `p`,
    /// so the two words divide identically. `"parler"` is the third row because
    /// it is the same word without the extra letter, and shows each offset
    /// moving by exactly one.
    ///
    /// See [`one_astral_character_is_one_letter`] for the same choice observed
    /// through `stem`.
    #[test]
    fn regions_are_positions_in_scalar_values() {
        assert_eq!(
            PorterStemmerFr::regions("😀parler"),
            Regions {
                r1: 4,
                r2: 7,
                rv: 3
            }
        );
        assert_eq!(
            PorterStemmerFr::regions("ñparler"),
            Regions {
                r1: 4,
                r2: 7,
                rv: 3
            }
        );
        assert_eq!(
            PorterStemmerFr::regions("parler"),
            Regions {
                r1: 3,
                r2: 6,
                rv: 3
            }
        );
    }

    /// The same choice, observed through `stem` rather than through
    /// [`PorterStemmerFr::regions`].
    ///
    /// Derived from the algorithm rather than recorded from it. `"au😀îmes"` is
    /// seven letters — `a u 😀 î m e s` — and the prelude leaves them alone
    /// (the `u` at 1 needs vowels on *both* sides, and `😀` is not one). RV
    /// takes its first arm, because `a` and `u` are both vowels: the literal
    /// **`rv = 3`**. The `par`/`col`/`tap` test then fails on `au😀`, and its
    /// fallback loop is guarded by `rv === len`, which no longer holds, so
    /// `rv` stays 3 and RV is `7 - 3 = 4` letters long.
    ///
    /// Step 2a finds the four-letter `-îmes` inside RV, preceded by the
    /// non-vowel `😀` — but its last condition is that RV be **strictly
    /// longer** than the match, and `4 > 4` is false. So step 2a declines,
    /// step 2b finds nothing, the word is unchanged from before step 1, and
    /// the residual step 4 runs instead: it drops the final `s` (the letter
    /// before it is `e`, which is not one of `a i o u è s`), then drops the
    /// exposed `e` inside RV. That leaves `"au😀îm"`.
    ///
    /// Under the code-unit reading RV measured five against the same
    /// four-letter match, `5 > 4` held, step 2a cut `-îmes`, and the stem was
    /// `"au😀"`.
    ///
    /// `"auжîmes"` is the control: `ж` is outside `isVowel` exactly as `😀` is,
    /// so the two words divide identically and the algorithm cannot tell them
    /// apart.
    #[test]
    fn one_astral_character_is_one_letter() {
        assert_eq!(s("au😀îmes"), "au😀îm");
        assert_eq!(s("auжîmes"), "auжîm");
    }

    /// The two predicates that used to be stated over UTF-16 code units answer
    /// identically over characters, for every scalar value there is.
    ///
    /// This is what makes converting [`is_vowel`] and [`is_french_letter`] a no-op
    /// rather than a change to be argued about: the vowel set tops out at
    /// `U+00F9` and `gate_fr` at `U+00FC`, so neither an astral character
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
                is_french_letter(c),
                units.iter().any(|u| gate_fr(*u)),
                "U+{cp:04X} gate"
            );
        }
    }

    /// A character that is inert for this algorithm and inside the Basic
    /// Multilingual Plane: not in `isVowel`, not spelled in any rule table,
    /// its own lower case, and neither a trigger nor a target of the prelude's
    /// `I`/`U`/`Y` marking.
    const INERT_TWIN: char = 'ж';

    /// Every entry of the French stop-word list and of every French rule table,
    /// walked through `stem` with one astral character inserted at **every**
    /// position, against the same word carrying an inert
    /// Basic-Multilingual-Plane character instead.
    ///
    /// # What the twin proves, and why it needs no second implementation
    ///
    /// [`INERT_TWIN`] is inert for this algorithm in exactly the way an astral
    /// character is: neither is a vowel, neither is spelled in any rule table,
    /// neither is rewritten by the lower-casing or by the prelude. So the only
    /// thing that can possibly distinguish the two words is **how long each of
    /// them is** — one character each under the contract, one and two under the
    /// code-unit reading this port used to carry. One build run over both
    /// therefore measures the unit directly, and a divergence here is a
    /// position that is still being counted in code units.
    ///
    /// # Red, then green
    ///
    /// Against the code-unit reading this port used to carry, this walk reports
    /// **194 of 510 435** probes measuring an astral character as more than one
    /// letter. It reports **0** here. Every one of the 194 has the same shape —
    /// a word opening with two vowels, where RV's literal `rv = 3` is an
    /// absolute position, followed by `-îmes` or `-ît` at the boundary where
    /// step 2a's strict "RV longer than the match" test turns over. That shape
    /// is what [`one_astral_character_is_one_letter`] states as one derived
    /// case; this walk is what found it, after the module documentation had
    /// argued — wrongly — that French's rules were all relative and could not
    /// see the unit at all.
    ///
    /// # Why every entry and every position, rather than a sample
    ///
    /// This is the shape of defect that has already cost this crate 116 Swedish
    /// stop words and two dead rules: a stage transforms text before a later
    /// stage measures or looks it up, and the entries that die are exactly the
    /// ones a spot check does not name. So the walk is over every entry of
    /// every table and of the stop-word list, behind every alphabetic entry of
    /// that same list — the probe construction [`crate::data::table_audit`]
    /// uses — and the astral character goes in at every position of each,
    /// including the two ends. The counts are pinned by equality so that a walk
    /// which quietly stops enumerating cannot report a clean sweep of nothing.
    #[test]
    fn every_shipped_entry_measures_the_same_under_either_unit() {
        let stemmer = PorterStemmerFr::new();
        let stops = Language::Fr.defaults();
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
            154,
            "the French rule-table entry count moved"
        );
        entries.extend(stops.iter().copied());
        assert_eq!(entries.len(), 322, "the French entry count moved");
        assert_eq!(fillers.len(), 169, "the French filler count moved");

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
            probes, 510_435,
            "the number of probes this walk builds moved"
        );
    }

    // -----------------------------------------------------------------------
    // Differential oracle: the pre-find_among implementation, verbatim —
    // owned `r2txt`/`rvtxt` snapshots, hand refreshes and all.
    // -----------------------------------------------------------------------
    mod oracle {
        use super::super::*;
        use crate::units::{longest_suffix, slen};

        /// The pre-`Buf` `cut`, over the owned `Vec` the oracle still uses.
        fn cut(w: &mut Vec<char>, n: usize, tail: &str) {
            w.truncate(w.len().saturating_sub(n));
            w.extend(tail.chars());
        }

        fn from(w: &[char], at: usize) -> &[char] {
            &w[at.min(w.len())..]
        }

        /// `endsin(token, letterBefore + suffix)` without building a string.
        fn ends_with_prefixed(w: &[char], before: char, suffix: &str) -> bool {
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
            let mut t = PorterStemmerFr::prelude_units(&scalars(&token.to_lowercase()));
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
                    if t.last() == Some(&'e') && ends_with_prefixed(&rvtxt, 'e', sfx) {
                        t.truncate(t.len() - 1);
                    }
                    r2txt = from(&t, regs.r2).to_vec();
                    rvtxt = from(&t, regs.rv).to_vec();
                }
            }

            if t != before_step1 {
                // --- Step 3 ------------------------------------------------
                if t.last() == Some(&'Y') {
                    let n = t.len();
                    t[n - 1] = 'i';
                }
                if t.last() == Some(&'ç') {
                    let n = t.len();
                    t[n - 1] = 'c';
                }
            } else {
                // --- Step 4: residual --------------------------------------
                let last = t.last().copied();
                let second_last = if t.len() >= 2 {
                    Some(t[t.len() - 2])
                } else {
                    None
                };
                if last == Some('s')
                    && !second_last.is_some_and(|c| matches!(c, 'a' | 'i' | 'o' | 'u' | 'è' | 's'))
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
                    if before.is_some_and(|c| c == 's' || c == 't') {
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
                } else if i != t.len() - 1 && (t[i] == 'é' || t[i] == 'è') {
                    t[i] = 'e';
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
