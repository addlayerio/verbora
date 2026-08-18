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

use std::borrow::Cow;
use std::sync::LazyLock;

use verbora_normalizers::normalize_no;
use verbora_tokenizers::classes;

use crate::among::AmongTable;
use crate::base::{Casing, TokenizeAndStem};
use crate::stopwords::{self, Language};
use crate::units::{ends_with, push_str, slen, text, units};

/// The Norwegian stemmer.
///
/// ```
/// use verbora_stemmers::PorterStemmerNo;
/// let s = PorterStemmerNo::new();
/// assert_eq!(s.stem("forebygger"), "forebygg");
/// assert_eq!(s.stem("havnevirksomhetene"), "havnevirksom");
/// assert_eq!(s.stem("hinder"), "hind");
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PorterStemmerNo;

/// `[aeiouyæåø]` — lowercase only; the regexes carry no `/i` flag.
#[inline]
fn is_vowel(c: u16) -> bool {
    matches!(
        c,
        0x61 | 0x65 | 0x69 | 0x6F | 0x75 | 0x79 | 0xE6 | 0xE5 | 0xF8
    )
}

/// `[A-Za-z0-9_æøåÆØÅäÄöÖüÜ]`, the class R1's captured run is drawn from.
#[inline]
fn is_r1_char(c: u16) -> bool {
    matches!(c,
        0x30..=0x39 | 0x41..=0x5A | 0x5F | 0x61..=0x7A
        | 0xE6 | 0xF8 | 0xE5 | 0xC6 | 0xD8 | 0xC5
        | 0xE4 | 0xC4 | 0xF6 | 0xD6 | 0xFC | 0xDC)
}

/// The longest suffix of `w` that appears in `alternatives`, or `None`.
///
/// This is what `/(x|y|z)$/` computes: the earliest start position at which some
/// alternative reaches the end. See the module note.
fn longest_listed_suffix<'s>(w: &[u16], alternatives: &[&'s str]) -> Option<&'s str> {
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
fn strip(w: &[u16], suffix: &str) -> Vec<u16> {
    if ends_with(w, suffix) {
        w[..w.len() - slen(suffix)].to_vec()
    } else {
        w.to_vec()
    }
}

/// `getR1` as a code-unit range into `t`, or `None` for the reference's `null`.
///
/// The range form is what `stem`'s `find_among` searches consume: R1 is an
/// *interior* slice of the token (the capture class excludes `-`, so it can
/// stop before the end), and the search takes it as `(lb, cursor)` limits
/// instead of a slice, which is the same bytes without an intermediate borrow.
fn r1_range(t: &[u16]) -> Option<(usize, usize)> {
    let index = (0..t.len().saturating_sub(2))
        .find(|&i| is_vowel(t[i]) && !is_vowel(t[i + 1]) && is_r1_char(t[i + 2]))?;
    if index == 0 {
        // `preR1Length = index + 2` is 2, which is `< 3`, so the reference
        // substitutes `token.slice(3)` — the empty string for a 3-unit token.
        return Some((3, t.len()));
    }
    let end = (index + 2..t.len())
        .find(|&i| !is_r1_char(t[i]))
        .unwrap_or(t.len());
    Some((index + 2, end))
}

/// `getR1` over code units. `None` is the reference's `null`.
///
/// Returns a slice borrowed from `t` rather than an owned snapshot: every
/// call site either only checks [`falsy`] or feeds the result straight into
/// [`longest_listed_suffix`], which (like `es.rs`'s `longest_suffix`) returns
/// a value borrowed from the *alternatives* list, never from `t` — so the
/// borrow only needs to live for one lookup, and `t` itself is never
/// reassigned inside any of these pure, non-mutating functions.
fn r1_units(t: &[u16]) -> Option<&[u16]> {
    r1_range(t).map(|(start, end)| &t[start..end])
}

/// Whether a step should bail out: `if (!r1) return token`, where `""` is falsy.
#[inline]
fn falsy(r1: Option<&[u16]>) -> bool {
    r1.is_none_or(<[u16]>::is_empty)
}

/// The sorted search tables `stem` uses, built once from the alternations
/// below. The per-step public methods keep the linear scans — they are the
/// reference-shaped API and the test oracle — while `stem` routes the same
/// tables through the `find_among` binary search
/// (`docs/PERFORMANCE_GAPS.md` entry 34).
struct NoTables {
    step1a: AmongTable,
    /// `(erte|ert)` — step 1c's alternation, searched in R1 and re-matched
    /// against the whole token.
    ert: AmongTable,
    /// `(dt|vt)` — step 2's alternation.
    dt_vt: AmongTable,
    step3: AmongTable,
}

static TABLES: LazyLock<NoTables> = LazyLock::new(|| NoTables {
    step1a: AmongTable::build(STEP1A),
    ert: AmongTable::build(&["erte", "ert"]),
    dt_vt: AmongTable::build(&["dt", "vt"]),
    step3: AmongTable::build(STEP3),
});

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
    /// matching at index 0 on a token shorter than four units.
    #[allow(
        clippy::unused_self,
        reason = "mirrors the reference's method-shaped API"
    )]
    #[must_use]
    pub fn get_r1(&self, token: &str) -> Option<String> {
        let t = units(token);
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

    /// Runs one step over the code units of `token`, borrowing when unchanged.
    #[allow(
        clippy::unused_self,
        reason = "mirrors the reference's method-shaped API"
    )]
    fn run<'a>(&self, token: &'a str, step: fn(&[u16]) -> Vec<u16>) -> Cow<'a, str> {
        let t = units(token);
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
        reason = "mirrors the reference's method-shaped API"
    )]
    #[must_use]
    pub fn stem<'a>(&self, token: &'a str) -> Cow<'a, str> {
        let tb = &*TABLES;
        let lower = token.to_lowercase();
        let mut t = units(&lower);

        // --- Step 1: 1a, 1b and 1c from the SAME input; the shortest result
        // wins and a tie goes to the later step (strict `<`, see module docs).
        // Each candidate is a truncation of `t` (1c also appends "er"), so the
        // three are decided as lengths first and applied to the buffer once.
        let len = t.len();
        let mut len_a = len;
        let mut len_b = len;
        let mut cut_c: Option<usize> = None; // truncate here, then append "er"
        if let Some((r1s, r1e)) = r1_range(&t)
            && r1e > r1s
        {
            // 1a: remove the longest listed suffix of R1 — from the TOKEN,
            // which does nothing when R1's end and the token's end disagree.
            let i = tb.step1a.find(&t, r1e, r1s);
            if i >= 0 {
                let entry = &tb.step1a.entries[i as usize].0;
                if t.ends_with(entry) {
                    len_a = len - entry.len();
                }
            }
            // 1b: `/(b|c|d|f|g|h|j|l|m|n|o|p|r|t|v|y|z)s$/`, then `/([^V]k)s$/`
            // — two rules with the same one-unit removal, so one `||`.
            let ends_s = t.last() == Some(&0x73);
            let listed = ends_s
                && len >= 2
                && matches!(
                    t[len - 2],
                    0x62 | 0x63
                        | 0x64
                        | 0x66
                        | 0x67
                        | 0x68
                        | 0x6A
                        | 0x6C
                        | 0x6D
                        | 0x6E
                        | 0x6F
                        | 0x70
                        | 0x72
                        | 0x74
                        | 0x76
                        | 0x79
                        | 0x7A
                );
            let cons_k = ends_s && len >= 3 && t[len - 2] == 0x6B && !is_vowel(t[len - 3]);
            if listed || cons_k {
                len_b = len - 1;
            }
            // 1c: gated on R1 containing `erte`/`ert`, applied by re-matching
            // the TOKEN (the two can disagree; see `erte_becomes_er` below).
            if tb.ert.find(&t, r1e, r1s) >= 0 {
                let j = tb.ert.find(&t, len, 0);
                if j >= 0 {
                    cut_c = Some(len - tb.ert.entries[j as usize].0.len());
                }
            }
        }
        let len_c = cut_c.map_or(len, |k| k + 2);
        let apply_c = |t: &mut Vec<u16>| {
            if let Some(k) = cut_c {
                t.truncate(k);
                push_str(t, "er");
            }
        };
        if len_a < len_b {
            if len_a < len_c {
                t.truncate(len_a);
            } else {
                apply_c(&mut t);
            }
        } else if len_b < len_c {
            t.truncate(len_b);
        } else {
            apply_c(&mut t);
        }

        // --- Step 2: drop a final unit when R1 ends in `dt`/`vt` -----------
        if let Some((r1s, r1e)) = r1_range(&t)
            && r1e > r1s
            && tb.dt_vt.find(&t, r1e, r1s) >= 0
        {
            let keep = t.len().saturating_sub(1);
            t.truncate(keep);
        }

        // --- Step 3: remove a listed derivational suffix found in R1 -------
        if let Some((r1s, r1e)) = r1_range(&t)
            && r1e > r1s
        {
            let i = tb.step3.find(&t, r1e, r1s);
            if i >= 0 {
                let entry = &tb.step3.entries[i as usize].0;
                if t.ends_with(entry) {
                    let keep = t.len() - entry.len();
                    t.truncate(keep);
                }
            }
        }

        Cow::Owned(text(&t))
    }
}

fn step1a(t: &[u16]) -> Vec<u16> {
    let r1 = r1_units(t);
    if falsy(r1) {
        return t.to_vec();
    }
    longest_listed_suffix(r1.unwrap_or(&[]), STEP1A).map_or_else(|| t.to_vec(), |s| strip(t, s))
}

fn step1b(t: &[u16]) -> Vec<u16> {
    let r1 = r1_units(t);
    if falsy(r1) {
        return t.to_vec();
    }
    // `/(b|c|d|f|g|h|j|l|m|n|o|p|r|t|v|y|z)s$/` — note `o` is in the list and
    // `k`, `q`, `s`, `u`, `w`, `x` and the other vowels are not.
    let ends_s = t.last() == Some(&0x73);
    if ends_s
        && t.len() >= 2
        && matches!(
            t[t.len() - 2],
            0x62 | 0x63
                | 0x64
                | 0x66
                | 0x67
                | 0x68
                | 0x6A
                | 0x6C
                | 0x6D
                | 0x6E
                | 0x6F
                | 0x70
                | 0x72
                | 0x74
                | 0x76
                | 0x79
                | 0x7A
        )
    {
        return t[..t.len() - 1].to_vec();
    }
    // `/([^aeiouyæåø]k)s$/` — a character before the `k` is required.
    if ends_s && t.len() >= 3 && t[t.len() - 2] == 0x6B && !is_vowel(t[t.len() - 3]) {
        return t[..t.len() - 1].to_vec();
    }
    t.to_vec()
}

fn step1c(t: &[u16]) -> Vec<u16> {
    let r1 = r1_units(t);
    if falsy(r1) {
        return t.to_vec();
    }
    if longest_listed_suffix(r1.unwrap_or(&[]), &["erte", "ert"]).is_none() {
        return t.to_vec();
    }
    // The replacement runs against the TOKEN, and re-matches: R1 ending in
    // `erte` does not guarantee the token does.
    match longest_listed_suffix(t, &["erte", "ert"]) {
        Some(s) => {
            let mut out = strip(t, s);
            push_str(&mut out, "er");
            out
        }
        None => t.to_vec(),
    }
}

fn step1(t: &[u16]) -> Vec<u16> {
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

fn step2(t: &[u16]) -> Vec<u16> {
    let r1 = r1_units(t);
    if falsy(r1) {
        return t.to_vec();
    }
    if longest_listed_suffix(r1.unwrap_or(&[]), &["dt", "vt"]).is_some() {
        return t[..t.len().saturating_sub(1)].to_vec();
    }
    t.to_vec()
}

fn step3(t: &[u16]) -> Vec<u16> {
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

/// The step-3 alternation, in source order.
static STEP3: &[&str] = &[
    "leg", "eleg", "ig", "eig", "lig", "elig", "els", "lov", "elov", "slov", "hetslov",
];

impl TokenizeAndStem for PorterStemmerNo {
    const FILTER_ON: Casing = Casing::Lower;
    const STEM_ON: Casing = Casing::Raw;

    fn is_word_char(c: char) -> bool {
        classes::is_word_no(c)
    }

    /// `AggressiveTokenizerNo` folds diacritics before splitting, and the folder
    /// rewrites only the **first** occurrence of each accented letter.
    fn prepare(text: &str) -> Cow<'_, str> {
        normalize_no(text)
    }

    fn is_stop_word(word: &str) -> bool {
        stopwords::contains(Language::No, word)
    }

    fn stem_token(&self, token: &str) -> String {
        self.stem(token).into_owned()
    }
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
        stopwords::add(Language::No, word);
    }

    /// Appends several stop words to the process-global Norwegian list.
    pub fn add_stop_words<I, S>(&self, words: I)
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        stopwords::add_all(Language::No, words);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(t: &str) -> String {
        PorterStemmerNo::new().stem(t).into_owned()
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
    fn norwegian_specifics() {
        assert_eq!(s("Æ Ø Å"), "æ ø å");
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
        text(&step3(&step2(&step1(&units(&lower)))))
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
