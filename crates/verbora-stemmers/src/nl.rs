//! The Dutch Snowball stemmer, ported from
//! The reference `porter_stemmer_nl`.
//!
//! # The sticky flag
//!
//! `step2` sets `this.suffixeRemoved = true` when it deletes an `e`. **Nothing
//! ever resets it**, and `step3b`'s `bar` rule reads it — on a module-level
//! singleton. Stemming is therefore order dependent:
//!
//! ```
//! use verbora_stemmers::PorterStemmerNl;
//! let s = PorterStemmerNl::new();
//! assert_eq!(s.stem("onaantastbar"), "onaantastbar"); // cold
//! assert_eq!(s.stem("lichte"), "licht");              // trips the flag
//! assert_eq!(s.stem("onaantastbar"), "onaantast");    // hot
//! ```
//!
//! Over the reference's own 45,669-word corpus this is the difference between
//! **237** mismatches — the number the reference spec asserts — and 235 with a
//! fresh instance per word. The two order-dependent words are `jongensgebaren`
//! and `molenwiekgebaren`. A pure-function port silently re-baselines the
//! reference test suite, so the flag is reproduced: [`PorterStemmerNl`] owns a
//! `Cell<bool>` initialised to `false` at construction and never cleared.
//!
//! Because the state lives in the value rather than in a process global, a caller
//! who *wants* determinism can construct a fresh stemmer per word — which is a
//! choice the reference does not offer.
//!
//! # Case sensitivity
//!
//! Only the postlude lowercases. Everything before it — the accent folding, the
//! `y`/`i` marking, the region scan, every suffix literal — is lowercase-only and
//! case-sensitive, so `stem("aachen")` is `"aach"` while `stem("AACHEN")` is just
//! `"aachen"`.
//!
//! # The unit
//!
//! R1, R2, every suffix position and the `r1 < 3` clamp count **Unicode scalar
//! values** — the unit [`crate::units`] states for the whole crate, and the one
//! `markRegions` is written in: R1 is *"the region after the first non-vowel
//! following a vowel"*, a statement about letters. So a cut here can only ever
//! land on a character boundary.

use std::borrow::Cow;
use std::cell::Cell;

use crate::among::Buf;
use crate::base::{Casing, TokenizeAndStem};
use crate::data::gates::gate_nl;
use crate::stopwords::Language;
use crate::units::{ends_with, longest_suffix, slen, text_lowercase};

/// The Dutch Snowball stemmer.
///
/// **Not `Sync`**: it carries a sticky `suffix_e_removed` flag that step 2
/// sets, nothing resets and step 3b reads, so its output depends on which
/// words it has already stemmed. The flag lives in this value rather than in a
/// process global, so a caller who wants determinism constructs a fresh
/// stemmer per word; see [`Self::suffix_e_removed`].
#[derive(Debug, Clone, Default)]
pub struct PorterStemmerNl {
    /// `this.suffixeRemoved` — set by step 2, read by step 3b, never reset.
    suffix_e_removed: Cell<bool>,
}

/// `vowels = 'aeiouèy'`.
#[inline]
fn is_vowel(c: char) -> bool {
    matches!(c, 'a' | 'e' | 'i' | 'o' | 'u' | 'è' | 'y')
}

#[inline]
fn is_vowel_at(w: &[char], i: usize) -> bool {
    w.get(i).copied().is_some_and(is_vowel)
}

/// Whether `c` is one of the letters [`gate_nl`] accepts.
///
/// The gate is stated over Basic Multilingual Plane code points and its
/// highest member is `ü` (`U+00FC`), so scanning characters and scanning
/// UTF-16 code units admit exactly the same tokens: a BMP character *is* its
/// own code unit, and an astral character is neither in the set itself nor are
/// the two surrogates it used to be scanned as.
#[inline]
fn is_dutch_letter(c: char) -> bool {
    (c as u32) < 0x1_0000 && gate_nl(c as u16)
}

/// `undoubleEnding`: drop the last letter when the word ends `kk`, `tt` or `dd`.
fn undouble_ending(w: &mut Buf<char>) {
    let s = w.as_slice();
    if UNDOUBLE.iter().any(|d| ends_with(s, d)) {
        w.truncate(w.len() - 1);
    }
}

impl PorterStemmerNl {
    /// Creates a stemmer with the sticky flag cleared.
    #[inline]
    #[must_use]
    pub const fn new() -> Self {
        Self {
            suffix_e_removed: Cell::new(false),
        }
    }

    /// Whether step 2 has ever removed an `e` on this stemmer.
    ///
    /// Exposed because it is the one piece of cross-call state in the crate, and
    /// a caller reasoning about reproducibility needs to be able to see it.
    #[must_use]
    pub fn suffix_e_removed(&self) -> bool {
        self.suffix_e_removed.get()
    }

    /// `replaceAccentedCharacters`: ten fixed mappings, applied in one pass.
    fn replace_accented(w: &mut [char]) {
        for c in w {
            match *c {
                'ä' | 'á' => *c = 'a',
                'ë' | 'é' => *c = 'e',
                'ï' | 'í' => *c = 'i',
                'ö' | 'ó' => *c = 'o',
                'ü' | 'ú' => *c = 'u',
                _ => {}
            }
        }
    }

    /// `handleYI`: initial `y`, `y` after a vowel, and `i` between vowels.
    ///
    /// The `é` in the reference's character classes is dead — `replaceAccented`
    /// has already turned every `é` into `e` — and is left out for that reason,
    /// not overlooked.
    fn handle_yi(w: &mut [char]) {
        if w.first() == Some(&'y') {
            w[0] = 'Y';
        }
        // `/([aeioue])y/g` — non-overlapping, left to right.
        let mut i = 0;
        while i + 1 < w.len() {
            if matches!(w[i], 'a' | 'e' | 'i' | 'o' | 'u') && w[i + 1] == 'y' {
                w[i + 1] = 'Y';
                i += 2;
            } else {
                i += 1;
            }
        }
        // `/([aeioue])i([aeioue])/g` — "aiaia" becomes "aIaia", not "aIaIa".
        let mut i = 0;
        while i + 2 < w.len() {
            if matches!(w[i], 'a' | 'e' | 'i' | 'o' | 'u')
                && w[i + 1] == 'i'
                && matches!(w[i + 2], 'a' | 'e' | 'i' | 'o' | 'u')
            {
                w[i + 1] = 'I';
                i += 3;
            } else {
                i += 1;
            }
        }
    }

    /// `markRegions`. Unlike German, R1 is adjusted **before** R2 is scanned.
    fn mark_regions(w: &[char]) -> (usize, usize) {
        let len = w.len();
        let mut r1 = len;
        for i in 0..len.saturating_sub(1) {
            if r1 != len {
                break;
            }
            if is_vowel(w[i]) && !is_vowel(w[i + 1]) {
                r1 = i + 2;
            }
        }
        if r1 != len && r1 < 3 {
            r1 = if len > 3 { 3 } else { len };
        }
        let mut r2 = len;
        for i in r1..len.saturating_sub(1) {
            if r2 != len {
                break;
            }
            if is_vowel(w[i]) && !is_vowel(w[i + 1]) {
                r2 = i + 2;
            }
        }
        (r1, r2)
    }

    /// `step1b`: delete a suffix in R1 preceded by a valid en-ending, then
    /// undouble.
    fn step1b(w: &mut Buf<char>, r1: usize, suffixes: &[&str]) {
        let s = w.as_slice();
        let Some(m) = longest_suffix(s, suffixes) else {
            return;
        };
        let pos = s.len() - slen(m);
        if pos < r1 {
            return;
        }
        // `pos >= 3` is guaranteed: R1 is either at least 3 or equal to the
        // length, and a matched suffix is non-empty, so the substr start below
        // is never negative. The literal is compared as characters rather than
        // collected into a fresh `Vec` — this runs on every Dutch word.
        let gem = pos >= 3 && s[pos - 3..pos] == ['g', 'e', 'm'];
        if !is_vowel_at(s, pos.wrapping_sub(1)) && !gem {
            w.truncate(pos);
            undouble_ending(w);
        }
    }

    fn step1(w: &mut Buf<char>, r1: usize) {
        // (1a) heden -> heid in R1
        if ends_with(w.as_slice(), HEDEN[0]) && w.len() >= 5 && w.len() - 5 >= r1 {
            w.truncate(w.len() - 5);
            w.push_str(HEID[0]);
        }
        // (1b) en / ene
        Self::step1b(w, r1, STEP1B_EN);
        // (1c) s / se in R1, preceded by a valid s-ending
        let s = w.as_slice();
        if let Some(m) = longest_suffix(s, STEP1C_S) {
            let pos = s.len() - slen(m);
            if pos >= r1 && !is_vowel_at(s, pos.wrapping_sub(1)) && !Self::se_guard(s) {
                w.truncate(pos);
            }
        }
    }

    /// `!result.match(/[js]se?$/)` — the reference's own note says a preceding
    /// `s` means the suffix should stay.
    fn se_guard(w: &[char]) -> bool {
        // `[js]se?$` is either `[js]s` or `[js]se` at the end.
        let tail = |n: usize| -> Option<&[char]> { w.len().checked_sub(n).map(|i| &w[i..]) };
        if let Some(t) = tail(2)
            && matches!(t[0], 'j' | 's')
            && t[1] == 's'
        {
            return true;
        }
        if let Some(t) = tail(3)
            && matches!(t[0], 'j' | 's')
            && t[1] == 's'
            && t[2] == 'e'
        {
            return true;
        }
        false
    }

    /// `step2`: delete `e` in R1 after a non-vowel — and set the sticky flag.
    fn step2(&self, w: &mut Buf<char>, r1: usize) {
        let s = w.as_slice();
        if ends_with(s, "e") && r1 < s.len() && s.len() > 1 && !is_vowel_at(s, s.len() - 2) {
            w.truncate(w.len() - 1);
            self.suffix_e_removed.set(true);
            undouble_ending(w);
        }
    }

    fn step3a(w: &mut Buf<char>, r1: usize, r2: usize) {
        let s = w.as_slice();
        if ends_with(s, HEID[0])
            && s.len() >= 4
            && s.len() - 4 >= r2
            && s.len() >= 5
            && s[s.len() - 5] != 'c'
        {
            w.truncate(w.len() - 4);
            Self::step1b(w, r1, STEP3A_EN);
        }
    }

    fn step3b(&self, w: &mut Buf<char>, r1: usize, r2: usize) {
        let s = w.as_slice();
        if longest_suffix(s, STEP3B_END_ING).is_some() && s.len() >= 3 && s.len() - 3 >= r2 {
            w.truncate(w.len() - 3);
            let s = w.as_slice();
            if ends_with(s, IG[0])
                && s.len() >= 2
                && s.len() - 2 >= r2
                && s.len() >= 3
                && s[s.len() - 3] != 'e'
            {
                w.truncate(w.len() - 2);
            } else {
                undouble_ending(w);
            }
        }
        // Independent of the block above, not an `else`.
        let s = w.as_slice();
        if ends_with(s, IG[0])
            && s.len() >= 2
            && r2 <= s.len() - 2
            && s.len() >= 3
            && s[s.len() - 3] != 'e'
        {
            w.truncate(w.len() - 2);
        }
        if ends_with(w.as_slice(), LIJK[0]) && w.len() >= 4 && r2 <= w.len() - 4 {
            w.truncate(w.len() - 4);
            self.step2(w, r1);
        }
        if ends_with(w.as_slice(), BAAR[0]) && w.len() >= 4 && r2 <= w.len() - 4 {
            w.truncate(w.len() - 4);
        }
        // The sticky flag decides this one.
        if ends_with(w.as_slice(), BAR[0])
            && w.len() >= 3
            && r2 <= w.len() - 3
            && self.suffix_e_removed.get()
        {
            w.truncate(w.len() - 3);
        }
    }

    /// `step4`: undouble a long vowel between two consonants.
    fn step4(w: &mut Buf<char>) {
        const CONS: &[char] = &[
            'b', 'c', 'd', 'f', 'g', 'h', 'j', 'k', 'l', 'm', 'n', 'p', 'q', 'r', 's', 't', 'v',
            'w', 'x', 'z',
        ];
        if w.len() < 4 {
            return;
        }
        let s = w.as_slice();
        let n = s.len();
        let doubled = matches!(
            (s[n - 3], s[n - 2]),
            ('a', 'a') | ('e', 'e') | ('o', 'o') | ('u', 'u')
        );
        if doubled && CONS.contains(&s[n - 4]) && CONS.contains(&s[n - 1]) {
            let last = s[n - 1];
            w.truncate(n - 2);
            w.push(last);
        }
    }

    /// Stems one token, updating the sticky flag.
    pub fn stem<'a>(&self, word: &'a str) -> Cow<'a, str> {
        let mut w: Buf<char> = Buf::fill(word);
        Self::replace_accented(w.as_mut_slice());
        Self::handle_yi(w.as_mut_slice());
        let (r1, r2) = Self::mark_regions(w.as_slice());
        Self::step1(&mut w, r1);
        self.step2(&mut w, r1);
        Self::step3a(&mut w, r1, r2);
        self.step3b(&mut w, r1, r2);
        Self::step4(&mut w);
        // `text(&w).to_lowercase()` without the intermediate `String`: the
        // postlude is the only place Dutch folds case.
        Cow::Owned(text_lowercase(w.as_mut_slice()))
    }
}

// ---------------------------------------------------------------------------
// Rule tables
// ---------------------------------------------------------------------------
//
// Every suffix literal the algorithm compares against is named here rather than
// written inline at its call site, so `data::table_audit` can walk all of them
// through the prelude that guards them. An inline literal is a table no audit
// can enumerate.

/// `undoubleEnding`'s doubled consonants.
static UNDOUBLE: &[&str] = &["kk", "tt", "dd"];
/// Step 1a's suffix, rewritten to [`HEID`].
static HEDEN: &[&str] = &["heden"];
/// The replacement step 1a writes, and the suffix step 3a deletes.
static HEID: &[&str] = &["heid"];
/// Step 1b.
static STEP1B_EN: &[&str] = &["en", "ene"];
/// Step 1c.
static STEP1C_S: &[&str] = &["se", "s"];
/// The step-1b table step 3a re-runs after deleting `heid`.
static STEP3A_EN: &[&str] = &["en"];
/// Step 3b's first rule.
static STEP3B_END_ING: &[&str] = &["end", "ing"];
/// Step 3b's `ig`, both as the follow-up to `end`/`ing` and on its own.
static IG: &[&str] = &["ig"];
/// Step 3b's `lijk`.
static LIJK: &[&str] = &["lijk"];
/// Step 3b's `baar`.
static BAAR: &[&str] = &["baar"];
/// Step 3b's `bar`, the one rule the sticky flag gates.
static BAR: &[&str] = &["bar"];

impl TokenizeAndStem for PorterStemmerNl {
    const FILTER_ON: Casing = Casing::Raw;
    const STEM_ON: Casing = Casing::Lower;
    /// `stem` both reads and writes the sticky `suffix_e_removed` flag, so a
    /// cached stem could be wrong (the same word stems differently once the
    /// flag is set) *and* serving it would skip the state change a fresh call
    /// makes. `tokenize_and_stem_cached` therefore ignores the cache here.
    const PURE_STEM: bool = false;

    fn is_stop_word(word: &str) -> bool {
        Language::Nl.contains(word)
    }

    fn gate(token: &str) -> bool {
        token.chars().any(is_dutch_letter)
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
        ("UNDOUBLE", super::UNDOUBLE),
        ("HEDEN", super::HEDEN),
        ("HEID", super::HEID),
        ("STEP1B_EN", super::STEP1B_EN),
        ("STEP1C_S", super::STEP1C_S),
        ("STEP3A_EN", super::STEP3A_EN),
        ("STEP3B_END_ING", super::STEP3B_END_ING),
        ("IG", super::IG),
        ("LIJK", super::LIJK),
        ("BAAR", super::BAAR),
        ("BAR", super::BAR),
    ];

    /// The prelude `stem` runs before any table is consulted, in isolation.
    pub(crate) fn prelude(token: &str) -> String {
        let mut w: Buf<char> = Buf::fill(token);
        super::PorterStemmerNl::replace_accented(w.as_mut_slice());
        super::PorterStemmerNl::handle_yi(w.as_mut_slice());
        w.into_text()
    }

    /// The units the prelude writes, paired with what it writes them for.
    pub(crate) static MARKERS: &[(&str, &str)] = &[("Y", "y"), ("I", "i")];
}

impl verbora_core::Stemmer for PorterStemmerNl {
    fn stem<'a>(&self, token: &'a str) -> Cow<'a, str> {
        Self::stem(self, token)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(t: &str) -> String {
        PorterStemmerNl::new().stem(t).into_owned()
    }

    #[test]
    fn the_flag_is_sticky() {
        let st = PorterStemmerNl::new();
        assert_eq!(st.stem("onaantastbar"), "onaantastbar");
        assert!(!st.suffix_e_removed());
        assert_eq!(st.stem("lichte"), "licht");
        assert!(st.suffix_e_removed());
        assert_eq!(st.stem("onaantastbar"), "onaantast");
    }

    #[test]
    fn a_fresh_stemmer_starts_cold() {
        assert_eq!(s("onaantastbar"), "onaantastbar");
        assert_eq!(s("jongensgebaren"), "jongensgebar");
        assert_eq!(s("molenwiekgebaren"), "molenwiekgebar");
    }

    #[test]
    fn case_sensitivity() {
        assert_eq!(s("aachen"), "aach");
        assert_eq!(s("AACHEN"), "aachen");
    }

    #[test]
    fn documented_vectors() {
        for (input, want) in [
            ("", ""),
            ("a", "a"),
            ("ab", "ab"),
            ("abc", "abc"),
            ("kleurbaar", "kleurbar"),
        ] {
            assert_eq!(s(input), want, "stem({input})");
        }
    }

    #[test]
    fn unicode_and_edges() {
        assert_eq!(s("😀"), "😀");
        assert_eq!(s("日本語"), "日本語");
        assert_eq!(s("123"), "123");
    }

    /// The two characters this file cannot tell apart.
    ///
    /// `U+1F600` is one Unicode scalar value and two UTF-16 code units;
    /// `U+4E2D` is one of each. Nothing in this file distinguishes them: the
    /// vowel class is `[aeiouèy]`, `replaceAccentedCharacters` maps only ten
    /// Latin-1 letters, `handleYI` looks only for `y` and `i` beside
    /// `[aeiou]`, step 4's consonant list is `b`–`z`, `se_guard` looks for
    /// `j`, `s` and `e`, every rule table is ASCII, and `str::to_lowercase`
    /// leaves both alone. Neither is in any of those sets, so the only thing
    /// about either that can influence the result is a position or a length —
    /// which is the unit under test.
    const ASTRAL: char = '😀';
    /// The Basic Multilingual Plane twin of [`ASTRAL`]; see there.
    const BMP_TWIN: char = '中';

    /// Every entry of the Dutch stop-word list and of every rule table, with
    /// one inert character inserted at every character position, paired with
    /// the same insertion of its BMP twin.
    fn inert_placements() -> Vec<(String, String)> {
        let mut corpus: Vec<&str> = Language::Nl.defaults().to_vec();
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
    /// A fresh stemmer per call, because the sticky `suffix_e_removed` flag
    /// would otherwise let the first stem of a pair change the second.
    #[test]
    fn an_astral_character_occupies_one_position() {
        let cases = inert_placements();
        let twin = BMP_TWIN.to_string();
        // Pinned so an enumeration that quietly walked nothing cannot pass:
        // 143 stop words and 16 rule-table entries, each probed at every one of its
        // `len + 1` character positions.
        assert_eq!(cases.len(), 597, "the enumerated corpus changed size");
        let mut diverged: Vec<(String, String, String)> = Vec::new();
        let mut invented: Vec<String> = Vec::new();
        for (astral, bmp) in &cases {
            let got = PorterStemmerNl::new().stem(astral).into_owned();
            if got.contains('\u{FFFD}') {
                invented.push(astral.clone());
            }
            let want = PorterStemmerNl::new().stem(bmp).into_owned();
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

    /// The Dutch gate keeps its verdict on every entry of the list once the
    /// scan is per character rather than per code unit.
    #[test]
    fn the_gate_survives_the_character_scan() {
        // The three entries `data::gates` records as the ones this gate
        // exempts: they hold no letter in any script.
        let rejected: Vec<&str> = Language::Nl
            .defaults()
            .iter()
            .copied()
            .filter(|w| !PorterStemmerNl::gate(&w.to_lowercase()))
            .collect();
        assert_eq!(
            rejected,
            ["$", "_", "-"],
            "the set of Dutch stop words its own gate exempts from stemming changed"
        );
        // Nothing outside the Basic Multilingual Plane is a Dutch letter, and
        // scanning characters must not admit one through a surrogate half.
        assert!(!PorterStemmerNl::gate("😀"));
        assert!(!PorterStemmerNl::gate("日本語"));
        assert!(PorterStemmerNl::gate("😀a"));
        for c in "áäéëíïóöúüè".chars() {
            assert!(
                PorterStemmerNl::gate(&c.to_string()),
                "the Dutch gate rejects {c:?}"
            );
        }
    }

    // -----------------------------------------------------------------------
    // Differential oracle: the pre-`Buf` implementation, verbatim.
    //
    // The conversion above moved the whole algorithm from an owned
    // `Vec<char>` onto the stack buffer and replaced the closing
    // `text(&w).to_lowercase()` with the in-place fold. Neither is meant to
    // change a byte, and the sticky flag makes that claim stronger than
    // usual: the oracle carries its own flag and the test below drives both
    // implementations through the *same word sequence*, so a divergence in
    // when the flag is set shows up as a divergence in a later stem.
    // -----------------------------------------------------------------------
    mod oracle {
        use super::super::*;
        use crate::units::text;

        fn undouble_ending(w: &mut Vec<char>) {
            if ends_with(w, "kk") || ends_with(w, "tt") || ends_with(w, "dd") {
                w.truncate(w.len() - 1);
            }
        }

        fn step1b(w: &mut Vec<char>, r1: usize, suffixes: &[&str]) {
            let Some(m) = longest_suffix(w, suffixes) else {
                return;
            };
            let pos = w.len() - slen(m);
            if pos < r1 {
                return;
            }
            let gem = pos >= 3 && w[pos - 3..pos] == ['g', 'e', 'm'];
            if !is_vowel_at(w, pos.wrapping_sub(1)) && !gem {
                w.truncate(pos);
                undouble_ending(w);
            }
        }

        fn step1(w: &mut Vec<char>, r1: usize) {
            if ends_with(w, "heden") && w.len() >= 5 && w.len() - 5 >= r1 {
                w.truncate(w.len() - 5);
                w.extend("heid".chars());
            }
            step1b(w, r1, &["en", "ene"]);
            if let Some(m) = longest_suffix(w, &["se", "s"]) {
                let pos = w.len() - slen(m);
                if pos >= r1
                    && !is_vowel_at(w, pos.wrapping_sub(1))
                    && !PorterStemmerNl::se_guard(w)
                {
                    w.truncate(pos);
                }
            }
        }

        fn step2(flag: &mut bool, w: &mut Vec<char>, r1: usize) {
            if ends_with(w, "e") && r1 < w.len() && w.len() > 1 && !is_vowel_at(w, w.len() - 2) {
                w.truncate(w.len() - 1);
                *flag = true;
                undouble_ending(w);
            }
        }

        fn step3a(w: &mut Vec<char>, r1: usize, r2: usize) {
            if ends_with(w, "heid")
                && w.len() >= 4
                && w.len() - 4 >= r2
                && w.len() >= 5
                && w[w.len() - 5] != 'c'
            {
                w.truncate(w.len() - 4);
                step1b(w, r1, &["en"]);
            }
        }

        fn step3b(flag: &mut bool, w: &mut Vec<char>, r1: usize, r2: usize) {
            if longest_suffix(w, &["end", "ing"]).is_some() && w.len() >= 3 && w.len() - 3 >= r2 {
                w.truncate(w.len() - 3);
                if ends_with(w, "ig")
                    && w.len() >= 2
                    && w.len() - 2 >= r2
                    && w.len() >= 3
                    && w[w.len() - 3] != 'e'
                {
                    w.truncate(w.len() - 2);
                } else {
                    undouble_ending(w);
                }
            }
            if ends_with(w, "ig")
                && w.len() >= 2
                && r2 <= w.len() - 2
                && w.len() >= 3
                && w[w.len() - 3] != 'e'
            {
                w.truncate(w.len() - 2);
            }
            if ends_with(w, "lijk") && w.len() >= 4 && r2 <= w.len() - 4 {
                w.truncate(w.len() - 4);
                step2(flag, w, r1);
            }
            if ends_with(w, "baar") && w.len() >= 4 && r2 <= w.len() - 4 {
                w.truncate(w.len() - 4);
            }
            if ends_with(w, "bar") && w.len() >= 3 && r2 <= w.len() - 3 && *flag {
                w.truncate(w.len() - 3);
            }
        }

        fn step4(w: &mut Vec<char>) {
            const CONS: &[char] = &[
                'b', 'c', 'd', 'f', 'g', 'h', 'j', 'k', 'l', 'm', 'n', 'p', 'q', 'r', 's', 't',
                'v', 'w', 'x', 'z',
            ];
            if w.len() < 4 {
                return;
            }
            let n = w.len();
            let doubled = matches!(
                (w[n - 3], w[n - 2]),
                ('a', 'a') | ('e', 'e') | ('o', 'o') | ('u', 'u')
            );
            if doubled && CONS.contains(&w[n - 4]) && CONS.contains(&w[n - 1]) {
                let last = w[n - 1];
                w.truncate(n - 2);
                w.push(last);
            }
        }

        pub(super) fn stem(flag: &mut bool, word: &str) -> String {
            let mut w: Vec<char> = word.chars().collect();
            PorterStemmerNl::replace_accented(&mut w);
            PorterStemmerNl::handle_yi(&mut w);
            let (r1, r2) = PorterStemmerNl::mark_regions(&w);
            step1(&mut w, r1);
            step2(flag, &mut w, r1);
            step3a(&mut w, r1, r2);
            step3b(flag, &mut w, r1, r2);
            step4(&mut w);
            text(&w).to_lowercase()
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

    /// Dutch stems crossed with the real rule suffixes, plus the accent,
    /// case, astral and digit noise the module documents.
    fn random_word(rng: &mut Rng) -> String {
        const ALPHA: &[char] = &[
            'a', 'b', 'd', 'e', 'g', 'h', 'i', 'j', 'k', 'l', 'm', 'n', 'o', 'p', 'r', 's', 't',
            'u', 'v', 'w', 'y', 'z', 'ä', 'á', 'ë', 'é', 'ï', 'í', 'ö', 'ó', 'ü', 'ú', 'è',
        ];
        const SUFFIXES: &[&str] = &[
            "heden", "heid", "ene", "en", "se", "s", "e", "end", "ing", "ig", "lijk", "baar",
            "bar", "kk", "tt", "dd", "aa", "ee", "oo", "uu", "gem", "jse", "sse", "cheid",
            "gebaren", "tbar",
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
    fn differential_against_the_pre_buf_oracle() {
        // One stemmer and one oracle flag for the whole run, in lockstep:
        // the sticky flag is part of what is being compared.
        let stemmer = PorterStemmerNl::new();
        let mut flag = false;
        let mut check = |input: &str| {
            let got = stemmer.stem(input).into_owned();
            let want = oracle::stem(&mut flag, input);
            assert_eq!(got, want, "stem({input:?})");
            assert_eq!(
                stemmer.suffix_e_removed(),
                flag,
                "sticky flag after stem({input:?})"
            );
        };
        for w in crate::test_support::bench_words("nl") {
            check(&w);
        }
        for w in [
            "",
            "a",
            "ab",
            "abc",
            "aachen",
            "AACHEN",
            "kleurbaar",
            "onaantastbar",
            "lichte",
            "jongensgebaren",
            "molenwiekgebaren",
            "😀",
            "日本語",
            "123",
            "gemenen",
            "jsse",
            "heden",
            "cheid",
        ] {
            check(w);
        }
        let long = "gebaren".repeat(12); // exercises the `Buf` heap spill
        check(&long);
        let mut rng = Rng(0xD1B5_4A32_D192_ED03);
        for _ in 0..60_000 {
            let w = random_word(&mut rng);
            check(&w);
        }
    }
}
