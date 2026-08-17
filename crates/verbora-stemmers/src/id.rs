//! The Indonesian (Sastrawi/Nazief–Adriani) stemmer, ported from
//! The reference `indonesian` module.
//!
//! # It writes to `globalThis`
//!
//! `suffix_rules` and `prefix_rules` are **non-strict** modules whose rule
//! bodies call `createResultObject` and `runDisambiguator` as bare functions. In
//! sloppy mode a bare call binds `this` to the global object, so every rule
//! stores its `removal` and `currentWord` on `globalThis` and then *returns*
//! `globalThis`. `StemmerId.a('hancurlah') === globalThis` is literally
//! true, and the driver reads the result back off the object it was handed.
//!
//! This port models the returned object as [`RuleResult`] — the two fields the
//! driver actually reads. Nothing else about `globalThis` is observable, because
//! both writers always set both fields before returning, so no stale value can
//! ever be read. **Divergence:** identity with a global object is not
//! reproducible in Rust and is not reproduced.
//!
//! # Per-call state is reset once, not per sub-word
//!
//! `stem()` clears the module-level `removals` array, and then `stemPluralWord`
//! calls `stemSingularWord` two or three times **without** clearing it again. The
//! second sub-word therefore inherits the first's removals, and
//! `loopRestorePrefixes` will happily restore a prefix that was stripped from a
//! different half of the word. [`StemmerId::stem`] threads exactly that: one list per
//! `stem` call, shared by every sub-word inside it.
//!
//! # Regexes that are not anchored
//!
//! Most prefix rules are `^…$`, but not all, and the exceptions change the
//! output rather than merely the match:
//!
//! * Rule 3 (`/ber([C])([a-z])er([aiueo])(.*)/`) has **neither** anchor, so
//!   `"xxberkaerat"` matches at offset 2 and everything before it is discarded:
//!   the rule returns `"kaerat"`.
//! * Rule 5 (`/be([C])(er[C])(.*)$/`) has no `^`.
//! * Rules 2 and 32 have no `$`, and `.` does not match a line terminator, so
//!   `(.*)` stops at the first `\n` and the tail is silently dropped.
//!
//! # `[g|h|q|k]`
//!
//! Rule 16's character class contains a literal `|` — it was written as if `[…]`
//! were an alternation. `"meng|foo"` therefore matches it and reduces to
//! `"|foo"`. This is preserved.
//!
//! # `runDisambiguator` keeps the LAST attempt, not the first that matched
//!
//! The loop assigns `result` on every iteration and only *breaks* when the value
//! is in the dictionary. When nothing hits, what survives is whatever the final
//! sub-rule returned — including `undefined`, which turns the whole rule into a
//! no-op. That makes rules 17 and 30 behave differently on the same shape:
//!
//! ```text
//! rule 17 ("mengasahxyz")  17a,17b miss; 17c (^menge) undefined; 17d -> "ngasahxyz"
//! rule 30 ("pengasahxyz")  30a,30b miss; 30c (^penge) undefined  -> no-op
//! ```
//!
//! Both were verified against the reference. A port that treats "no sub-rule
//! matched" as the only `undefined` case, or that returns the first sub-rule that
//! matched, loses this.

use std::borrow::Cow;

use verbora_tokenizers::classes;

use crate::base::{Casing, TokenizeAndStem};
use crate::data::indonesian_dict;
use crate::stopwords::{self, Language};
use crate::units::{eq_str, starts_with, text, units};

/// The Indonesian stemmer.
///
/// ```
/// use verbora_stemmers::StemmerId;
/// let s = StemmerId::new();
/// assert_eq!(s.stem("hancurlah"), "hancur");
/// assert_eq!(s.stem("mempengaruhi"), "pengaruh");
/// assert_eq!(s.stem("buku-buku"), "buku");
/// assert_eq!(s.stem("malaikat-malaikat-Nya"), "malaikat");
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StemmerId;

/// Which kind of affix a [`Removal`] recorded — the reference's `affixType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RemovalKind {
    /// `DP` — a derivational **prefix**, produced by the prefix rules.
    DerivationalPrefix,
    /// `DS` — a derivational suffix (`is`, `isme`, `isasi`, `i`, `kan`, `an`).
    DerivationalSuffix,
    /// `PP` — an inflectional possessive pronoun (`ku`, `mu`, `nya`).
    PossessivePronoun,
    /// `P` — an inflectional particle (`lah`, `kah`, `tah`, `pun`).
    Particle,
}

impl RemovalKind {
    /// The reference's two-letter code.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::DerivationalPrefix => "DP",
            Self::DerivationalSuffix => "DS",
            Self::PossessivePronoun => "PP",
            Self::Particle => "P",
        }
    }

    /// `isSuffixRemovals`: everything except a derivational prefix.
    #[must_use]
    pub const fn is_suffix(self) -> bool {
        !matches!(self, Self::DerivationalPrefix)
    }
}

/// One recorded affix removal — `indonesian/removal`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Removal {
    original: Vec<u16>,
    result: Vec<u16>,
    removed: Vec<u16>,
    kind: RemovalKind,
}

impl Removal {
    /// `getOriginalWord()`.
    #[must_use]
    pub fn original_word(&self) -> String {
        text(&self.original)
    }
    /// `getResult()`.
    #[must_use]
    pub fn result(&self) -> String {
        text(&self.result)
    }
    /// `getRemovedPart()`.
    ///
    /// Computed as `word.replace(result, '')` — the **first** occurrence of the
    /// result *anywhere* in the word, deleted. For a prefix rule that rewrites
    /// rather than truncates this is rarely the affix: `"mengeboran" ->
    /// "ngeboran"` records a removed part of `"me"` only because `"ngeboran"`
    /// does not occur in `"mengeboran"` at all, so nothing is deleted and the
    /// whole word is reported.
    #[must_use]
    pub fn removed_part(&self) -> String {
        text(&self.removed)
    }
    /// `getAffixType()`.
    #[must_use]
    pub const fn affix_type(&self) -> RemovalKind {
        self.kind
    }
}

/// What a rule function returns — the reference hands back `globalThis`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleResult {
    /// `globalThis.removal`; `None` is the reference's `undefined`.
    pub removal: Option<Removal>,
    /// `globalThis.currentWord`.
    pub current_word: String,
}

// ---------------------------------------------------------------------------
// Dictionary
// ---------------------------------------------------------------------------

/// `dictionary.has(word)`.
///
/// Every root is ASCII, so a word carrying any non-ASCII code unit cannot be in
/// the set and never reaches the binary search. This runs on every rule
/// application — `stemming_process` calls it after each suffix and prefix rule —
/// so it is one of the hottest paths in the stemmer.
///
/// The longest dictionary entry is 20 bytes, so a stack buffer covers every word
/// that could possibly be found; longer inputs fall back to a heap buffer purely
/// for correctness (an unfindable word must still be looked up, not just assumed
/// absent) rather than for speed.
fn find(w: &[u16]) -> bool {
    if w.iter().any(|&c| c >= 0x80) {
        return false;
    }
    const STACK_CAP: usize = 64;
    let mut stack = [0u8; STACK_CAP];
    let mut heap;
    let bytes: &[u8] = if w.len() <= STACK_CAP {
        for (dst, &c) in stack.iter_mut().zip(w) {
            *dst = c as u8;
        }
        &stack[..w.len()]
    } else {
        heap = vec![0u8; w.len()];
        for (dst, &c) in heap.iter_mut().zip(w) {
            *dst = c as u8;
        }
        &heap
    };
    let s = core::str::from_utf8(bytes).expect("ASCII code units are valid UTF-8");
    indonesian_dict::SORTED.binary_search(&s).is_ok()
}

// ---------------------------------------------------------------------------
// Small scanners
// ---------------------------------------------------------------------------

/// `[aiueo]`.
#[inline]
fn v(c: u16) -> bool {
    matches!(c, 0x61 | 0x69 | 0x75 | 0x65 | 0x6F)
}
/// `[a-z]`.
#[inline]
fn az(c: u16) -> bool {
    (0x61..=0x7A).contains(&c)
}
/// `[bcdfghjklmnpqrstvwxyz]` — every lowercase consonant.
#[inline]
fn cons(c: u16) -> bool {
    az(c) && !v(c)
}
/// `[bcdfghjklmnpqstvwxyz]` — rule 5's class, which omits `r`.
#[inline]
fn cons_no_r(c: u16) -> bool {
    cons(c) && c != 0x72
}
/// `[bcdfghjkpqstvxz]` — rules 35 and 36.
#[inline]
fn cons_35(c: u16) -> bool {
    matches!(
        c,
        0x62 | 0x63
            | 0x64
            | 0x66
            | 0x67
            | 0x68
            | 0x6A
            | 0x6B
            | 0x70
            | 0x71
            | 0x73
            | 0x74
            | 0x76
            | 0x78
            | 0x7A
    )
}
/// `[abcdfghijklmopqrstuvwxyz]` — rule 19's class, which omits `e` and `n`.
#[inline]
fn cls19(c: u16) -> bool {
    az(c) && c != 0x65 && c != 0x6E
}
/// `[g|h|q|k]` — the literal `|` is part of the class, not an alternation.
#[inline]
fn cls16(c: u16) -> bool {
    matches!(c, 0x67 | 0x7C | 0x68 | 0x71 | 0x6B)
}

/// How far `.` can run from `at`: to the first the reference line terminator.
fn dot_end(w: &[u16], at: usize) -> usize {
    (at..w.len())
        .find(|&i| matches!(w[i], 0x000A | 0x000D | 0x2028 | 0x2029))
        .unwrap_or(w.len())
}

/// Whether `(.*)$` can consume `w[at..]` in one piece.
fn dot_to_end(w: &[u16], at: usize) -> bool {
    dot_end(w, at) == w.len()
}

/// Whether `w[at..]` begins with the ASCII literal `lit`.
fn lit_at(w: &[u16], at: usize, lit: &str) -> bool {
    at <= w.len() && starts_with(&w[at..], lit)
}

/// Concatenates ASCII literals and slices into one buffer.
macro_rules! cat {
    ($($part:expr),+ $(,)?) => {{
        let mut out: Vec<u16> = Vec::new();
        $( $part.append_to(&mut out); )+
        out
    }};
}

/// Lets `cat!` take both `&str` and `&[u16]`.
trait Append {
    fn append_to(&self, out: &mut Vec<u16>);
}
impl Append for &str {
    fn append_to(&self, out: &mut Vec<u16>) {
        out.extend(self.encode_utf16());
    }
}
impl Append for &[u16] {
    fn append_to(&self, out: &mut Vec<u16>) {
        out.extend_from_slice(self);
    }
}
impl Append for u16 {
    fn append_to(&self, out: &mut Vec<u16>) {
        out.push(*self);
    }
}

/// `word.replace(needle, '')`: delete the **first** occurrence of a literal.
///
/// An empty needle is found at index 0 and deletes nothing, so the word comes
/// back unchanged — which is how a rule that reduces a word to `""` still
/// records the whole word as its removed part.
fn delete_first(w: &[u16], needle: &[u16]) -> Vec<u16> {
    if needle.is_empty() || needle.len() > w.len() {
        return w.to_vec();
    }
    for i in 0..=w.len() - needle.len() {
        if &w[i..i + needle.len()] == needle {
            let mut out = w[..i].to_vec();
            out.extend_from_slice(&w[i + needle.len()..]);
            return out;
        }
    }
    w.to_vec()
}

// ---------------------------------------------------------------------------
// Suffix rules
// ---------------------------------------------------------------------------

/// `/-*(alt)$/`: the start of the leftmost match, dashes included.
fn dashed_suffix(w: &[u16], alts: &[&str]) -> Option<usize> {
    let mut best: Option<usize> = None;
    for a in alts {
        let n = a.len();
        if n <= w.len() && lit_at(w, w.len() - n, a) {
            let mut start = w.len() - n;
            while start > 0 && w[start - 1] == 0x2D {
                start -= 1;
            }
            if best.is_none_or(|b| start < b) {
                best = Some(start);
            }
        }
    }
    best
}

/// `/(alt)$/` with no dashes: the longest listed suffix.
fn plain_suffix(w: &[u16], alts: &[&str]) -> Option<usize> {
    let mut best: Option<usize> = None;
    for a in alts {
        let n = a.len();
        if n <= w.len() && lit_at(w, w.len() - n, a) {
            let start = w.len() - n;
            if best.is_none_or(|b| start < b) {
                best = Some(start);
            }
        }
    }
    best
}

/// `suffix_rules`'s `createResultObject`: no removal when nothing changed.
fn suffix_result(result: Vec<u16>, word: &[u16], kind: RemovalKind) -> (Option<Removal>, Vec<u16>) {
    if result == word {
        return (None, result);
    }
    let removed = delete_first(word, &result);
    (
        Some(Removal {
            original: word.to_vec(),
            result: result.clone(),
            removed,
            kind,
        }),
        result,
    )
}

fn remove_particle(w: &[u16]) -> (Option<Removal>, Vec<u16>) {
    let cut = dashed_suffix(w, &["lah", "kah", "tah", "pun"]).unwrap_or(w.len());
    suffix_result(w[..cut].to_vec(), w, RemovalKind::Particle)
}

fn remove_possessive(w: &[u16]) -> (Option<Removal>, Vec<u16>) {
    let cut = dashed_suffix(w, &["ku", "mu", "nya"]).unwrap_or(w.len());
    suffix_result(w[..cut].to_vec(), w, RemovalKind::PossessivePronoun)
}

fn remove_derivational_suffix(w: &[u16]) -> (Option<Removal>, Vec<u16>) {
    let cut = plain_suffix(w, &["is", "isme", "isasi", "i", "kan", "an"]).unwrap_or(w.len());
    suffix_result(w[..cut].to_vec(), w, RemovalKind::DerivationalSuffix)
}

/// The three suffix rules, in `SuffixRules.rules` order.
type SuffixRule = fn(&[u16]) -> (Option<Removal>, Vec<u16>);
static SUFFIX_RULES: &[SuffixRule] = &[
    remove_particle,
    remove_possessive,
    remove_derivational_suffix,
];

// ---------------------------------------------------------------------------
// Prefix rules
// ---------------------------------------------------------------------------

/// One disambiguation attempt: `undefined` when its pattern does not match.
type SubRule = fn(&[u16]) -> Option<Vec<u16>>;

/// A prefix rule, as `PrefixRules.rules` holds them.
enum PrefixRule {
    /// `RemovePlainPrefix` — the only one that can record *no* removal.
    Plain,
    /// A `runDisambiguator` rule over an ordered list of attempts.
    Dis(&'static [SubRule]),
}

fn r1a(w: &[u16]) -> Option<Vec<u16>> {
    (lit_at(w, 0, "ber") && w.len() > 3 && v(w[3]) && dot_to_end(w, 4)).then(|| w[3..].to_vec())
}
fn r1b(w: &[u16]) -> Option<Vec<u16>> {
    (lit_at(w, 0, "ber") && w.len() > 3 && v(w[3]) && dot_to_end(w, 4)).then(|| cat!("r", &w[3..]))
}
/// Rule 2. No `$`, so `(.*)` stops at the first line terminator.
fn r2(w: &[u16]) -> Option<Vec<u16>> {
    if !(lit_at(w, 0, "ber") && w.len() > 4 && cons(w[3]) && az(w[4])) {
        return None;
    }
    let g3 = &w[5..dot_end(w, 5)];
    // `P != 'er'`
    if lit_at(g3, 0, "er") {
        return None;
    }
    Some(cat!(w[3], w[4], g3))
}
/// Rule 3. Neither anchor, so anything before the match is discarded.
fn r3(w: &[u16]) -> Option<Vec<u16>> {
    let p = (0..w.len()).find(|&p| {
        lit_at(w, p, "ber")
            && p + 7 < w.len()
            && cons(w[p + 3])
            && az(w[p + 4])
            && lit_at(w, p + 5, "er")
            && v(w[p + 7])
    })?;
    // `C != 'r'` is checked *after* the match, so a `berr…` word yields nothing
    // rather than retrying at a later offset.
    if w[p + 3] == 0x72 {
        return None;
    }
    let g4 = &w[p + 8..dot_end(w, p + 8)];
    Some(cat!(w[p + 3], w[p + 4], "er", w[p + 7], g4))
}
fn r4(w: &[u16]) -> Option<Vec<u16>> {
    eq_str(w, "belajar").then(|| units("ajar"))
}
/// Rule 5. No `^`, so it may start anywhere; `$` forces it to reach the end.
fn r5(w: &[u16]) -> Option<Vec<u16>> {
    let p = (0..w.len()).find(|&p| {
        lit_at(w, p, "be")
            && p + 5 < w.len()
            && cons_no_r(w[p + 2])
            && lit_at(w, p + 3, "er")
            && cons(w[p + 5])
            && dot_to_end(w, p + 6)
    })?;
    Some(w[p + 2..].to_vec())
}
fn r6a(w: &[u16]) -> Option<Vec<u16>> {
    (lit_at(w, 0, "ter") && w.len() > 3 && v(w[3]) && dot_to_end(w, 4)).then(|| w[3..].to_vec())
}
fn r6b(w: &[u16]) -> Option<Vec<u16>> {
    (lit_at(w, 0, "ter") && w.len() > 3 && v(w[3]) && dot_to_end(w, 4)).then(|| cat!("r", &w[3..]))
}
fn r7(w: &[u16]) -> Option<Vec<u16>> {
    if !(lit_at(w, 0, "ter")
        && w.len() > 6
        && cons(w[3])
        && lit_at(w, 4, "er")
        && v(w[6])
        && dot_to_end(w, 7))
    {
        return None;
    }
    if w[3] == 0x72 {
        return None;
    }
    Some(cat!(w[3], "er", &w[6..]))
}
fn r8(w: &[u16]) -> Option<Vec<u16>> {
    if !(lit_at(w, 0, "ter") && w.len() > 3 && cons(w[3]) && dot_to_end(w, 4)) {
        return None;
    }
    if w[3] == 0x72 || lit_at(w, 4, "er") {
        return None;
    }
    Some(w[3..].to_vec())
}
fn r9(w: &[u16]) -> Option<Vec<u16>> {
    if !(lit_at(w, 0, "te")
        && w.len() > 5
        && cons(w[2])
        && lit_at(w, 3, "er")
        && cons(w[5])
        && dot_to_end(w, 6))
    {
        return None;
    }
    if w[2] == 0x72 {
        return None;
    }
    Some(w[2..].to_vec())
}
fn r10(w: &[u16]) -> Option<Vec<u16>> {
    (lit_at(w, 0, "me")
        && w.len() > 3
        && matches!(w[2], 0x6C | 0x72 | 0x77 | 0x79)
        && v(w[3])
        && dot_to_end(w, 4))
    .then(|| w[2..].to_vec())
}
fn r11(w: &[u16]) -> Option<Vec<u16>> {
    (lit_at(w, 0, "mem") && w.len() > 3 && matches!(w[3], 0x62 | 0x66 | 0x76) && dot_to_end(w, 4))
        .then(|| w[3..].to_vec())
}
fn r12(w: &[u16]) -> Option<Vec<u16>> {
    (lit_at(w, 0, "mempe") && dot_to_end(w, 5)).then(|| cat!("pe", &w[5..]))
}
fn r13a(w: &[u16]) -> Option<Vec<u16>> {
    (lit_at(w, 0, "mem") && w.len() > 3 && v(w[3]) && dot_to_end(w, 4)).then(|| cat!("m", &w[3..]))
}
fn r13b(w: &[u16]) -> Option<Vec<u16>> {
    (lit_at(w, 0, "mem") && w.len() > 3 && v(w[3]) && dot_to_end(w, 4)).then(|| cat!("p", &w[3..]))
}
fn r14(w: &[u16]) -> Option<Vec<u16>> {
    (lit_at(w, 0, "men")
        && w.len() > 3
        && matches!(w[3], 0x63 | 0x64 | 0x6A | 0x73 | 0x74 | 0x7A)
        && dot_to_end(w, 4))
    .then(|| w[3..].to_vec())
}
fn r15a(w: &[u16]) -> Option<Vec<u16>> {
    (lit_at(w, 0, "men") && w.len() > 3 && v(w[3]) && dot_to_end(w, 4)).then(|| cat!("n", &w[3..]))
}
fn r15b(w: &[u16]) -> Option<Vec<u16>> {
    (lit_at(w, 0, "men") && w.len() > 3 && v(w[3]) && dot_to_end(w, 4)).then(|| cat!("t", &w[3..]))
}
fn r16(w: &[u16]) -> Option<Vec<u16>> {
    (lit_at(w, 0, "meng") && w.len() > 4 && cls16(w[4]) && dot_to_end(w, 5))
        .then(|| w[4..].to_vec())
}
fn r17a(w: &[u16]) -> Option<Vec<u16>> {
    (lit_at(w, 0, "meng") && w.len() > 4 && v(w[4]) && dot_to_end(w, 5)).then(|| w[4..].to_vec())
}
fn r17b(w: &[u16]) -> Option<Vec<u16>> {
    (lit_at(w, 0, "meng") && w.len() > 4 && v(w[4]) && dot_to_end(w, 5)).then(|| cat!("k", &w[4..]))
}
fn r17c(w: &[u16]) -> Option<Vec<u16>> {
    (lit_at(w, 0, "menge") && dot_to_end(w, 5)).then(|| w[5..].to_vec())
}
fn r17d(w: &[u16]) -> Option<Vec<u16>> {
    (lit_at(w, 0, "meng") && w.len() > 4 && v(w[4]) && dot_to_end(w, 5))
        .then(|| cat!("ng", &w[4..]))
}
fn r18a(w: &[u16]) -> Option<Vec<u16>> {
    (lit_at(w, 0, "meny") && w.len() > 4 && v(w[4]) && dot_to_end(w, 5))
        .then(|| cat!("ny", &w[4..]))
}
fn r18b(w: &[u16]) -> Option<Vec<u16>> {
    (lit_at(w, 0, "meny") && w.len() > 4 && v(w[4]) && dot_to_end(w, 5)).then(|| cat!("s", &w[4..]))
}
fn r19(w: &[u16]) -> Option<Vec<u16>> {
    (lit_at(w, 0, "memp") && w.len() > 4 && cls19(w[4]) && dot_to_end(w, 5))
        .then(|| cat!("p", &w[4..]))
}
fn r20(w: &[u16]) -> Option<Vec<u16>> {
    (lit_at(w, 0, "pe")
        && w.len() > 3
        && matches!(w[2], 0x77 | 0x79)
        && v(w[3])
        && dot_to_end(w, 4))
    .then(|| w[2..].to_vec())
}
fn r21a(w: &[u16]) -> Option<Vec<u16>> {
    (lit_at(w, 0, "per") && w.len() > 3 && v(w[3]) && dot_to_end(w, 4)).then(|| w[3..].to_vec())
}
fn r21b(w: &[u16]) -> Option<Vec<u16>> {
    (lit_at(w, 0, "pe") && w.len() > 3 && w[2] == 0x72 && v(w[3]) && dot_to_end(w, 4))
        .then(|| w[2..].to_vec())
}
fn r23(w: &[u16]) -> Option<Vec<u16>> {
    if !(lit_at(w, 0, "per") && w.len() > 4 && cons(w[3]) && az(w[4]) && dot_to_end(w, 5)) {
        return None;
    }
    if lit_at(w, 5, "er") {
        return None;
    }
    Some(w[3..].to_vec())
}
fn r24(w: &[u16]) -> Option<Vec<u16>> {
    if !(lit_at(w, 0, "per")
        && w.len() > 7
        && cons(w[3])
        && az(w[4])
        && lit_at(w, 5, "er")
        && v(w[7])
        && dot_to_end(w, 8))
    {
        return None;
    }
    if w[3] == 0x72 {
        return None;
    }
    Some(w[3..].to_vec())
}
fn r25(w: &[u16]) -> Option<Vec<u16>> {
    (lit_at(w, 0, "pem") && w.len() > 3 && matches!(w[3], 0x62 | 0x66 | 0x76) && dot_to_end(w, 4))
        .then(|| w[3..].to_vec())
}
fn r26a(w: &[u16]) -> Option<Vec<u16>> {
    (lit_at(w, 0, "pem") && w.len() > 3 && v(w[3]) && dot_to_end(w, 4)).then(|| cat!("m", &w[3..]))
}
fn r26b(w: &[u16]) -> Option<Vec<u16>> {
    (lit_at(w, 0, "pem") && w.len() > 3 && v(w[3]) && dot_to_end(w, 4)).then(|| cat!("p", &w[3..]))
}
fn r27(w: &[u16]) -> Option<Vec<u16>> {
    (lit_at(w, 0, "pen")
        && w.len() > 3
        && matches!(w[3], 0x63 | 0x64 | 0x6A | 0x73 | 0x74 | 0x7A)
        && dot_to_end(w, 4))
    .then(|| w[3..].to_vec())
}
fn r28a(w: &[u16]) -> Option<Vec<u16>> {
    (lit_at(w, 0, "pen") && w.len() > 3 && v(w[3]) && dot_to_end(w, 4)).then(|| cat!("n", &w[3..]))
}
fn r28b(w: &[u16]) -> Option<Vec<u16>> {
    (lit_at(w, 0, "pen") && w.len() > 3 && v(w[3]) && dot_to_end(w, 4)).then(|| cat!("t", &w[3..]))
}
fn r29(w: &[u16]) -> Option<Vec<u16>> {
    (lit_at(w, 0, "peng") && w.len() > 4 && cons(w[4]) && dot_to_end(w, 5)).then(|| w[4..].to_vec())
}
fn r30a(w: &[u16]) -> Option<Vec<u16>> {
    (lit_at(w, 0, "peng") && w.len() > 4 && v(w[4]) && dot_to_end(w, 5)).then(|| w[4..].to_vec())
}
fn r30b(w: &[u16]) -> Option<Vec<u16>> {
    (lit_at(w, 0, "peng") && w.len() > 4 && v(w[4]) && dot_to_end(w, 5)).then(|| cat!("k", &w[4..]))
}
fn r30c(w: &[u16]) -> Option<Vec<u16>> {
    (lit_at(w, 0, "penge") && dot_to_end(w, 5)).then(|| w[5..].to_vec())
}
fn r31a(w: &[u16]) -> Option<Vec<u16>> {
    (lit_at(w, 0, "peny") && w.len() > 4 && v(w[4]) && dot_to_end(w, 5))
        .then(|| cat!("ny", &w[4..]))
}
fn r31b(w: &[u16]) -> Option<Vec<u16>> {
    (lit_at(w, 0, "peny") && w.len() > 4 && v(w[4]) && dot_to_end(w, 5)).then(|| cat!("s", &w[4..]))
}
/// Rule 32. The `pelajar` special case, then `^pe(l[aiueo])(.*)` — no `$`.
fn r32(w: &[u16]) -> Option<Vec<u16>> {
    if eq_str(w, "pelajar") {
        return Some(units("ajar"));
    }
    (lit_at(w, 0, "pe") && w.len() > 3 && w[2] == 0x6C && v(w[3]))
        .then(|| w[2..dot_end(w, 4)].to_vec())
}
fn r34(w: &[u16]) -> Option<Vec<u16>> {
    if !(lit_at(w, 0, "pe") && w.len() > 2 && cons(w[2]) && dot_to_end(w, 3)) {
        return None;
    }
    if lit_at(w, 3, "er") {
        return None;
    }
    Some(w[2..].to_vec())
}
fn r35(w: &[u16]) -> Option<Vec<u16>> {
    (lit_at(w, 0, "ter")
        && w.len() > 6
        && cons_35(w[3])
        && lit_at(w, 4, "er")
        && cons(w[6])
        && dot_to_end(w, 7))
    .then(|| w[3..].to_vec())
}
fn r36(w: &[u16]) -> Option<Vec<u16>> {
    (lit_at(w, 0, "pe")
        && w.len() > 5
        && cons_35(w[2])
        && lit_at(w, 3, "er")
        && cons(w[5])
        && dot_to_end(w, 6))
    .then(|| w[2..].to_vec())
}

/// `^(C)(XY[aiueo])(.*)$` for the four `CerV`/`CelV`/`CemV`/`CinV` pairs.
///
/// The `a` variant re-emits the word unchanged; the `b` variant drops the
/// infix. Returning the input verbatim still creates a `Removal` — the prefix
/// `createResultObject` is unconditional — whose removed part is `""`, and that
/// non-empty `removals` list is what stops `checkPrefixRules`.
fn infix(w: &[u16], infix: &str, keep: bool) -> Option<Vec<u16>> {
    if !(w.len() > 3 && cons(w[0]) && lit_at(w, 1, infix) && v(w[3]) && dot_to_end(w, 4)) {
        return None;
    }
    Some(if keep {
        w.to_vec()
    } else {
        cat!(w[0], &w[3..])
    })
}
fn r37a(w: &[u16]) -> Option<Vec<u16>> {
    infix(w, "er", true)
}
fn r37b(w: &[u16]) -> Option<Vec<u16>> {
    infix(w, "er", false)
}
fn r38a(w: &[u16]) -> Option<Vec<u16>> {
    infix(w, "el", true)
}
fn r38b(w: &[u16]) -> Option<Vec<u16>> {
    infix(w, "el", false)
}
fn r39a(w: &[u16]) -> Option<Vec<u16>> {
    infix(w, "em", true)
}
fn r39b(w: &[u16]) -> Option<Vec<u16>> {
    infix(w, "em", false)
}
fn r40a(w: &[u16]) -> Option<Vec<u16>> {
    infix(w, "in", true)
}
fn r40b(w: &[u16]) -> Option<Vec<u16>> {
    infix(w, "in", false)
}
fn r41(w: &[u16]) -> Option<Vec<u16>> {
    (lit_at(w, 0, "ku") && dot_to_end(w, 2)).then(|| w[2..].to_vec())
}
fn r42(w: &[u16]) -> Option<Vec<u16>> {
    (lit_at(w, 0, "kau") && dot_to_end(w, 3)).then(|| w[3..].to_vec())
}

/// `PrefixRules.rules`, in push order. Rule 22, 33 and 43 do not exist in the
/// reference either — the numbering has gaps.
static PREFIX_RULES: &[PrefixRule] = &[
    PrefixRule::Plain,
    PrefixRule::Dis(&[r1a, r1b]),
    PrefixRule::Dis(&[r2]),
    PrefixRule::Dis(&[r3]),
    PrefixRule::Dis(&[r4]),
    PrefixRule::Dis(&[r5]),
    PrefixRule::Dis(&[r6a, r6b]),
    PrefixRule::Dis(&[r7]),
    PrefixRule::Dis(&[r8]),
    PrefixRule::Dis(&[r9]),
    PrefixRule::Dis(&[r10]),
    PrefixRule::Dis(&[r11]),
    PrefixRule::Dis(&[r12]),
    PrefixRule::Dis(&[r13a, r13b]),
    PrefixRule::Dis(&[r14]),
    PrefixRule::Dis(&[r15a, r15b]),
    PrefixRule::Dis(&[r16]),
    PrefixRule::Dis(&[r17a, r17b, r17c, r17d]),
    PrefixRule::Dis(&[r18a, r18b]),
    PrefixRule::Dis(&[r19]),
    PrefixRule::Dis(&[r20]),
    PrefixRule::Dis(&[r21a, r21b]),
    PrefixRule::Dis(&[r23]),
    PrefixRule::Dis(&[r24]),
    PrefixRule::Dis(&[r25]),
    PrefixRule::Dis(&[r26a, r26b]),
    PrefixRule::Dis(&[r27]),
    PrefixRule::Dis(&[r28a, r28b]),
    PrefixRule::Dis(&[r29]),
    PrefixRule::Dis(&[r30a, r30b, r30c]),
    PrefixRule::Dis(&[r31a, r31b]),
    PrefixRule::Dis(&[r32]),
    PrefixRule::Dis(&[r34]),
    PrefixRule::Dis(&[r35]),
    PrefixRule::Dis(&[r36]),
    PrefixRule::Dis(&[r37a, r37b]),
    PrefixRule::Dis(&[r38a, r38b]),
    PrefixRule::Dis(&[r39a, r39b]),
    PrefixRule::Dis(&[r40a, r40b]),
    PrefixRule::Dis(&[r41]),
    PrefixRule::Dis(&[r42]),
];

impl PrefixRule {
    fn apply(&self, w: &[u16]) -> (Option<Removal>, Vec<u16>) {
        match self {
            Self::Plain => {
                // `word.replace(/^(di|ke|se)/, '')`
                let result = ["di", "ke", "se"]
                    .into_iter()
                    .find(|p| lit_at(w, 0, p))
                    .map_or_else(|| w.to_vec(), |_| w[2..].to_vec());
                if result == w {
                    return (None, result);
                }
                let removed = delete_first(w, &result);
                (
                    Some(Removal {
                        original: w.to_vec(),
                        result: result.clone(),
                        removed,
                        kind: RemovalKind::DerivationalPrefix,
                    }),
                    result,
                )
            }
            Self::Dis(rules) => {
                // `result` keeps whatever the LAST attempt returned unless one
                // hit the dictionary first — including `undefined`.
                let mut result: Option<Vec<u16>> = None;
                for r in *rules {
                    result = r(w);
                    if result.as_deref().is_some_and(find) {
                        break;
                    }
                }
                let Some(result) = result else {
                    return (None, w.to_vec());
                };
                // The prefix `createResultObject` is unconditional: it records a
                // removal even when the result equals the input.
                let removed = delete_first(w, &result);
                (
                    Some(Removal {
                        original: w.to_vec(),
                        result: result.clone(),
                        removed,
                        kind: RemovalKind::DerivationalPrefix,
                    }),
                    result,
                )
            }
        }
    }
}

// ---------------------------------------------------------------------------
// The driver
// ---------------------------------------------------------------------------

/// The module-level `removals`, `originalWord` and `currentWord`, per call.
struct State {
    removals: Vec<Removal>,
    original: Vec<u16>,
    current: Vec<u16>,
}

impl State {
    fn found(&self) -> bool {
        find(&self.current)
    }

    /// `removeSuffixes`.
    fn remove_suffixes(&mut self) {
        for rule in SUFFIX_RULES {
            let (removal, next) = rule(&self.current);
            if let Some(r) = removal {
                self.removals.push(r);
            }
            self.current = next;
            if self.found() {
                return;
            }
        }
    }

    /// `removePrefixes`: three passes of `checkPrefixRules`.
    fn remove_prefixes(&mut self) {
        for _ in 0..3 {
            self.check_prefix_rules();
            if self.found() {
                return;
            }
        }
    }

    /// `checkPrefixRules`: stop at the first rule that records a removal.
    fn check_prefix_rules(&mut self) {
        let before = self.removals.len();
        for rule in PREFIX_RULES {
            let (removal, next) = rule.apply(&self.current);
            if let Some(r) = removal {
                self.removals.push(r);
            }
            self.current = next;
            if self.found() || self.removals.len() > before {
                return;
            }
        }
    }

    /// `restorePrefix`: rewind to the last removal's original word, then forget
    /// every prefix removal.
    fn restore_prefix(&mut self) {
        // The reference loops over every removal assigning `currentWord`, with a
        // commented-out `break`, so the LAST one wins.
        if let Some(last) = self.removals.last() {
            self.current = last.original.clone();
        }
        self.removals
            .retain(|r| r.kind != RemovalKind::DerivationalPrefix);
    }

    /// `loopRestorePrefixes`: the ECS restore loop.
    fn loop_restore_prefixes(&mut self) {
        self.restore_prefix();
        self.removals.reverse();
        let temp = self.current.clone();

        // `for (const i in reversedRemovals)` snapshots the index keys, so the
        // removals that `removePrefixes` appends below are never visited.
        let n = self.removals.len();
        for i in 0..n {
            let removal = self.removals[i].clone();
            if !removal.kind.is_suffix() {
                continue;
            }
            if eq_str(&removal.removed, "kan") {
                self.current = cat!(removal.result.as_slice(), "k");
                self.remove_prefixes();
                if self.found() {
                    return;
                }
                self.current = cat!(removal.result.as_slice(), "kan");
            } else {
                self.current = removal.original.clone();
            }
            self.remove_prefixes();
            if self.found() {
                return;
            }
            self.current.clone_from(&temp);
        }
    }

    /// `stemmingProcess` — steps 2 through 5.
    fn stemming_process(&mut self) {
        if self.found() {
            return;
        }
        if precedence_adjustment(&self.original) {
            self.remove_prefixes();
            if self.found() {
                return;
            }
            self.remove_suffixes();
            if self.found() {
                return;
            }
            self.current.clone_from(&self.original);
            self.removals.clear();
        }
        self.remove_suffixes();
        if self.found() {
            return;
        }
        self.remove_prefixes();
        if self.found() {
            return;
        }
        self.loop_restore_prefixes();
    }

    /// `stemSingularWord`.
    fn stem_singular(&mut self, word: &[u16]) -> Vec<u16> {
        self.original = word.to_vec();
        self.current = word.to_vec();
        if self.current.len() > 3 {
            self.stemming_process();
        }
        if self.found() {
            self.current.clone()
        } else {
            self.original.clone()
        }
    }
}

/// `precedenceAdjustmentSpecification`: six `^X(.*)Y$` probes.
fn precedence_adjustment(w: &[u16]) -> bool {
    [
        ("be", "lah"),
        ("be", "an"),
        ("me", "i"),
        ("di", "i"),
        ("pe", "i"),
        ("ter", "i"),
    ]
    .into_iter()
    .any(|(head, tail)| {
        let (h, t) = (head.len(), tail.len());
        w.len() >= h + t
            && lit_at(w, 0, head)
            && lit_at(w, w.len() - t, tail)
            // The `(.*)` between them cannot cross a line terminator.
            && dot_end(w, h) >= w.len() - t
    })
}

/// `/^(.*)-(.*)$/`: the index of the LAST hyphen, or `None`.
///
/// `(.*)` is greedy, so the split lands on the last hyphen; and because both
/// groups together must cover the whole string, a line terminator anywhere makes
/// the pattern unmatchable.
fn split_last_hyphen(w: &[u16]) -> Option<usize> {
    if dot_end(w, 0) != w.len() {
        return None;
    }
    (0..w.len()).rev().find(|&i| w[i] == 0x2D)
}

impl StemmerId {
    /// Creates the stemmer. It is stateless and zero-sized.
    #[inline]
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// The 29,932 root words, in file order — `StemmerId.dictionary`.
    #[allow(
        clippy::unused_self,
        reason = "mirrors the reference's property-shaped API"
    )]
    #[must_use]
    pub const fn dictionary(&self) -> &'static [&'static str] {
        indonesian_dict::WORDS
    }

    /// `isPlural`.
    #[allow(
        clippy::unused_self,
        reason = "mirrors the reference's method-shaped API"
    )]
    #[must_use]
    pub fn is_plural(&self, token: &str) -> bool {
        let w = units(token);
        // `/^(.*)-(ku|mu|nya|lah|kah|tah|pun)$/` — `(.*)` is greedy, so the
        // shortest possessive wins the tie and the hyphen sits as late as it can.
        let head = if dot_end(&w, 0) == w.len() {
            [
                ["ku", "mu"].as_slice(),
                ["nya", "lah", "kah", "tah", "pun"].as_slice(),
            ]
            .into_iter()
            .find_map(|group| {
                group.iter().find_map(|a| {
                    let n = a.len();
                    (w.len() > n && lit_at(&w, w.len() - n, a) && w[w.len() - n - 1] == 0x2D)
                        .then(|| w.len() - n - 1)
                })
            })
        } else {
            None
        };
        match head {
            Some(cut) => w[..cut].contains(&0x2D),
            None => w.contains(&0x2D),
        }
    }

    /// Stems one token.
    #[allow(
        clippy::unused_self,
        reason = "mirrors the reference's method-shaped API"
    )]
    #[must_use]
    pub fn stem<'a>(&self, token: &'a str) -> Cow<'a, str> {
        let lower = token.to_lowercase();
        let w = units(&lower);
        let mut st = State {
            // Reset here, and — deliberately — nowhere else.
            removals: Vec::new(),
            original: Vec::new(),
            current: Vec::new(),
        };
        let out = if self.is_plural(&lower) {
            stem_plural(&w, &mut st)
        } else {
            st.stem_singular(&w)
        };
        Cow::Owned(text(&out))
    }

    /// `RemoveInflectionalParticle`, the rule the reference exports as
    /// `StemmerId.a`.
    ///
    /// In the reference this returns `globalThis`; here it returns the two fields
    /// the driver reads off it. See the module documentation.
    #[allow(
        clippy::unused_self,
        reason = "mirrors the reference's method-shaped API"
    )]
    #[must_use]
    pub fn remove_inflectional_particle(&self, word: &str) -> RuleResult {
        let (removal, current) = remove_particle(&units(word));
        RuleResult {
            removal,
            current_word: text(&current),
        }
    }

    /// Appends a stop word to the **process-global Indonesian list**.
    pub fn add_stop_word(&self, word: impl Into<String>) {
        stopwords::add(Language::Id, word);
    }

    /// Appends several stop words to the process-global Indonesian list.
    pub fn add_stop_words<I, S>(&self, words: I)
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        stopwords::add_all(Language::Id, words);
    }

    /// Removes the first occurrence of `word` from the Indonesian list.
    pub fn remove_stop_word(&self, word: &str) {
        stopwords::remove(Language::Id, word);
    }

    /// Removes the first occurrence of each of `words`.
    pub fn remove_stop_words<'a, I>(&self, words: I)
    where
        I: IntoIterator<Item = &'a str>,
    {
        stopwords::remove_all(Language::Id, words);
    }
}

/// `stemPluralWord`.
fn stem_plural(w: &[u16], st: &mut State) -> Vec<u16> {
    let Some(at) = split_last_hyphen(w) else {
        return w.to_vec();
    };
    let mut first = w[..at].to_vec();
    let mut second = w[at + 1..].to_vec();

    // `malaikat-malaikat-nya` -> `malaikat` + `malaikat-nya`
    let is_pronoun = ["ku", "mu", "nya", "lah", "kah", "tah", "pun"]
        .iter()
        .any(|s| second.len() == s.len() && lit_at(&second, 0, s));
    if is_pronoun && let Some(inner) = split_last_hyphen(&first) {
        let head = first[..inner].to_vec();
        let tail = first[inner + 1..].to_vec();
        second = cat!(tail.as_slice(), "-", second.as_slice());
        first = head;
    }

    let root1 = st.stem_singular(&first);
    let mut root2 = st.stem_singular(&second);

    // `meniru-nirukan` -> `tiru`
    if !find(&second) && root2 == second {
        root2 = st.stem_singular(&cat!("me", second.as_slice()));
    }

    if root1 == root2 { root1 } else { w.to_vec() }
}

impl TokenizeAndStem for StemmerId {
    const FILTER_ON: Casing = Casing::Raw;
    const STEM_ON: Casing = Casing::Raw;

    fn is_word_char(c: char) -> bool {
        classes::is_word_id(c)
    }

    /// The whole document is lowercased before tokenizing, as in English.
    fn prepare(text: &str) -> Cow<'_, str> {
        Cow::Owned(text.to_lowercase())
    }

    fn is_stop_word(word: &str) -> bool {
        stopwords::contains(Language::Id, word)
    }

    fn stem_token(&self, token: &str) -> String {
        self.stem(token).into_owned()
    }
}

impl verbora_core::Stemmer for StemmerId {
    fn stem<'a>(&self, token: &'a str) -> Cow<'a, str> {
        Self::stem(self, token)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(t: &str) -> String {
        StemmerId::new().stem(t).into_owned()
    }

    #[test]
    fn documented_vectors() {
        for (input, want) in [
            ("hancurlah", "hancur"),
            ("bukumukah", "buku"),
            ("berikanku", "beri"),
            ("dibuang", "buang"),
            ("belajar", "ajar"),
            ("pelajar", "ajar"),
            ("mempengaruhi", "pengaruh"),
            ("mengkritik", "kritik"),
            ("buku-buku", "buku"),
            ("malaikat-malaikat-Nya", "malaikat"),
            ("", ""),
        ] {
            assert_eq!(s(input), want, "stem({input})");
        }
    }

    #[test]
    fn is_plural_reads_the_hyphen_before_the_pronoun() {
        let st = StemmerId::new();
        assert!(st.is_plural("buku-buku"));
        assert!(st.is_plural("malaikat-malaikat-nya"));
        assert!(!st.is_plural("buku-nya"));
        assert!(!st.is_plural("buku"));
        assert!(!st.is_plural(""));
    }

    #[test]
    fn the_dictionary_is_the_reference_size() {
        assert_eq!(StemmerId::new().dictionary().len(), 29932);
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

    #[test]
    fn a_bare_hyphen_run_is_not_a_plural() {
        assert_eq!(s("---"), "---");
        assert_eq!(s("МАМА"), "мама");
    }
}
