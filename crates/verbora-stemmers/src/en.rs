//! The English Porter stemmer, ported from the reference `porter_stemmer`.
//!
//! # Three things a careful reading gets wrong
//!
//! **`measure` is a floating-point number.** It is `categorizeGroups(token)`
//! with a leading `C` and a trailing `V` stripped, divided by two — and that
//! string can have odd length, because the `V` the second pass injects is not
//! itself in `[aeiou]` and so cannot merge with a later vowel run. `measure("sya")`
//! is **0.5**, `measure("syaing")` is 1.5, `measure("fifugyed")` is 3.5. Every
//! guard in the algorithm is `> 0`, `> 1` or `=== 1`, and an integer port changes
//! all of them.
//!
//! **`attemptReplacePatterns` is not "first match wins".** It walks the *whole*
//! table with no early exit, evaluates each rule's measure guard against the
//! **original** token (with the suffix stripped, which is classic Porter), and
//! applies the replacement to an **accumulator**. So `step3("formalizeful")` is
//! `"formalize"`: the `alize` rule is skipped because its guard is tested against
//! `"formalizeful"`, even though the accumulator ends in `alize` by the time the
//! rule is reached.
//!
//! **Empty strings are falsy.** `attemptReplace` skips its callback when the
//! result is `""`, and `|| replacement` discards an empty result, so
//! `step1b("ed")` is `"ed"` rather than `""`.
//!
//! The `\1` backreference in `endsWithDoublCons` has no equivalent in the Rust
//! `regex` crate; it is a two-code-unit comparison here. Note that `y` counts as
//! a consonant for *that* test but as a vowel-former in `categorizeGroups`.

use std::borrow::Cow;

use verbora_tokenizers::classes;

use crate::base::{Casing, TokenizeAndStem};
use crate::units::{at, ends_with, slen, text, truncate_by, u, units};

/// The English Porter stemmer.
///
/// ```
/// use verbora_stemmers::PorterStemmer;
/// let s = PorterStemmer::new();
/// assert_eq!(s.stem("running"), "run");
/// // Tokens shorter than three UTF-16 code units are returned WITHOUT
/// // lowercasing — the fold happens only on the other branch.
/// assert_eq!(s.stem("AB"), "AB");
/// assert_eq!(s.stem("ABC"), "abc");
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PorterStemmer;

#[inline]
const fn is_aeiou(c: u16) -> bool {
    matches!(c, 0x61 | 0x65 | 0x69 | 0x6F | 0x75) // a e i o u
}

#[inline]
const fn is_aeiouy(c: u16) -> bool {
    is_aeiou(c) || c == 0x79 // y
}

/// The characters the reference's `.` refuses to match.
#[inline]
const fn is_line_terminator(c: u16) -> bool {
    matches!(c, 0x0A | 0x0D | 0x2028 | 0x2029)
}

const C: u16 = 0x43;
const V: u16 = 0x56;

/// `token.replace(/[^aeiouy]+y/g,'CV').replace(/[aeiou]+/g,'V').replace(/[^V]+/g,'C')`
///
/// The three passes run in order over each other's output, which is why the
/// result can contain adjacent `VV`: the `V` written by pass two is outside
/// `[aeiou]`, so a following vowel run starts a *new* group.
fn categorize_groups(w: &[u16]) -> Vec<u16> {
    // Pass 1: a run of non-[aeiouy] followed by a literal `y` becomes "CV".
    let mut a: Vec<u16> = Vec::with_capacity(w.len());
    let mut i = 0;
    while i < w.len() {
        if is_aeiouy(w[i]) {
            a.push(w[i]);
            i += 1;
            continue;
        }
        // Greedy run; `y` is excluded from the class, so a shorter run could
        // never be followed by `y` either — no backtracking is possible.
        let start = i;
        while i < w.len() && !is_aeiouy(w[i]) {
            i += 1;
        }
        if at(w, i) == Some(u('y')) {
            a.push(C);
            a.push(V);
            i += 1;
        } else {
            a.extend_from_slice(&w[start..i]);
        }
    }

    // Pass 2: runs of [aeiou] become a single `V`.
    let mut b: Vec<u16> = Vec::with_capacity(a.len());
    let mut i = 0;
    while i < a.len() {
        if is_aeiou(a[i]) {
            while i < a.len() && is_aeiou(a[i]) {
                i += 1;
            }
            b.push(V);
        } else {
            b.push(a[i]);
            i += 1;
        }
    }

    // Pass 3: runs of non-`V` become a single `C`.
    let mut out: Vec<u16> = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == V {
            out.push(V);
            i += 1;
        } else {
            while i < b.len() && b[i] != V {
                i += 1;
            }
            out.push(C);
        }
    }
    out
}

/// `token.replace(/[^aeiouy]y/g,'CV').replace(/[aeiou]/g,'V').replace(/[^V]/g,'C')`
///
/// Length preserving, unlike [`categorize_groups`]: every input code unit maps to
/// exactly one output unit.
fn categorize_chars(w: &[u16]) -> Vec<u16> {
    let mut out: Vec<u16> = Vec::with_capacity(w.len());
    let mut i = 0;
    while i < w.len() {
        if !is_aeiouy(w[i]) && at(w, i + 1) == Some(u('y')) {
            out.push(C);
            out.push(V);
            i += 2;
        } else if is_aeiou(w[i]) {
            out.push(V);
            i += 1;
        } else {
            out.push(C);
            i += 1;
        }
    }
    out
}

/// The Porter measure `m`, as an `f64`.
///
/// `-1` for a falsy token, which in the reference covers `''`, `null` and
/// `undefined` alike — reproduced here as "the empty buffer".
fn measure_units(w: &[u16]) -> f64 {
    if w.is_empty() {
        return -1.0;
    }
    let g = categorize_groups(w);
    let start = usize::from(g.first() == Some(&C));
    let end = g.len() - usize::from(g.last() == Some(&V) && g.len() > start);
    (end - start) as f64 / 2.0
}

/// `token.match(/([^aeiou])\1$/)` — the hand-written backreference.
fn ends_with_double_cons(w: &[u16]) -> bool {
    w.len() >= 2 && w[w.len() - 1] == w[w.len() - 2] && !is_aeiou(w[w.len() - 1])
}

/// `attemptReplace` for a literal-string pattern.
///
/// Returns `None` when the pattern is not a suffix, mirroring the reference
/// `null`. An empty *result* is returned as `Some(vec![])`; callers apply the
/// falsy check themselves, because the two call sites treat it differently.
fn attempt_replace(w: &[u16], pattern: &str, replacement: &str) -> Option<Vec<u16>> {
    if !ends_with(w, pattern) {
        return None;
    }
    let mut out = w[..w.len() - slen(pattern)].to_vec();
    out.extend(replacement.encode_utf16());
    Some(out)
}

/// `attemptReplacePatterns`: walk the entire table, guard against the original,
/// apply to the accumulator, never break.
fn attempt_replace_patterns(
    token: &[u16],
    rules: &[(&str, &str, &str)],
    threshold: Option<f64>,
) -> Vec<u16> {
    let mut replacement = token.to_vec();
    for &(pattern, guard_replacement, real_replacement) in rules {
        let passes = match threshold {
            None => true,
            Some(t) => {
                let guarded = attempt_replace(token, pattern, guard_replacement);
                // `measure(null)` is -1, and so is `measure('')`.
                guarded.as_deref().map_or(-1.0, measure_units) > t
            }
        };
        if passes
            && let Some(next) = attempt_replace(&replacement, pattern, real_replacement)
            && !next.is_empty()
        {
            replacement = next;
        }
    }
    replacement
}

/// `replaceRegex(token, /^(.+?)(alt|…)$/, [1], min)` — the step-4 shape.
///
/// Lazy `.+?` plus a `$`-anchored alternation means "the shortest non-empty
/// prefix whose remainder is one of the alternatives", i.e. the **longest**
/// matching suffix. `.` cannot cross a line terminator, so a prefix containing
/// one blocks the match entirely.
fn strip_longest_alternative(
    w: &[u16],
    alternatives: &[&str],
    keep_extra: usize,
) -> Option<Vec<u16>> {
    let mut best: Option<usize> = None;
    for alt in alternatives {
        let n = slen(alt);
        if n < w.len() && ends_with(w, alt) && best.is_none_or(|b| n > b) {
            best = Some(n);
        }
    }
    let n = best?;
    let cut = w.len() - n;
    if w[..cut].iter().copied().any(is_line_terminator) {
        return None;
    }
    Some(w[..cut + keep_extra].to_vec())
}

impl PorterStemmer {
    /// Creates the stemmer. It is stateless and zero-sized.
    #[inline]
    pub const fn new() -> Self {
        Self
    }

    /// `PorterStemmer.categorizeGroups`, exported by the reference for tests.
    pub fn categorize_groups(token: &str) -> String {
        text(&categorize_groups(&units(token)))
    }

    /// `PorterStemmer.measure` — the Porter measure `m`, which is **not** an
    /// integer (see the module documentation).
    pub fn measure(token: &str) -> f64 {
        measure_units(&units(token))
    }

    /// Step 1a: plural `s` removal.
    pub fn step1a(token: &str) -> String {
        let mut w = units(token);
        Self::step1a_units(&mut w);
        text(&w)
    }

    fn step1a_units(w: &mut Vec<u16>) {
        // `/(ss|i)es$/ -> '$1'` reduces to "drop the trailing `es`" for both
        // alternatives, because the captured group is what precedes it.
        if ends_with(w, "sses") || ends_with(w, "ies") {
            truncate_by(w, 2);
            return;
        }
        if w.len() > 2 && w[w.len() - 1] == u('s') && w[w.len() - 2] != u('s') {
            truncate_by(w, 1);
        }
    }

    /// Step 1b: `eed`, `ed` and `ing`.
    pub fn step1b(token: &str) -> String {
        let mut w = units(token);
        Self::step1b_units(&mut w);
        text(&w)
    }

    fn step1b_units(w: &mut Vec<u16>) {
        // `token.substr(-3) === 'eed'` needs the token to be at least three units
        // long — a shorter `substr(-3)` returns the whole token, which can never
        // equal a three-character literal.
        if ends_with(w, "eed") {
            if measure_units(&w[..w.len() - 3]) > 0.0 {
                truncate_by(w, 1); // eed -> ee
            }
            return;
        }
        let stripped = if ends_with(w, "ing") {
            Some(w[..w.len() - 3].to_vec())
        } else if ends_with(w, "ed") {
            Some(w[..w.len() - 2].to_vec())
        } else {
            None
        };
        let Some(t) = stripped else { return };
        // The callback returns null when the stripped stem has no vowel group.
        let Some(result) = Self::step1b_callback(&t) else {
            return;
        };
        // `if (result)` — an empty result is falsy and leaves the token alone.
        if !result.is_empty() {
            *w = result;
        }
    }

    fn step1b_callback(t: &[u16]) -> Option<Vec<u16>> {
        if !categorize_groups(t).contains(&V) {
            return None;
        }
        // No measure threshold here: every rule fires unconditionally.
        let r = attempt_replace_patterns(
            t,
            &[("at", "", "ate"), ("bl", "", "ble"), ("iz", "", "ize")],
            None,
        );
        if r.as_slice() != t {
            return Some(r);
        }
        if ends_with_double_cons(t)
            && t.last()
                .is_some_and(|&c| c != u('l') && c != u('s') && c != u('z'))
        {
            return Some(t[..t.len() - 1].to_vec());
        }
        let cc = categorize_chars(t);
        let tail3 = &cc[cc.len().saturating_sub(3)..];
        if measure_units(t) == 1.0
            && tail3 == [C, V, C]
            && t.last()
                .is_some_and(|&c| c != u('w') && c != u('x') && c != u('y'))
        {
            let mut out = t.to_vec();
            out.push(u('e'));
            return Some(out);
        }
        Some(t.to_vec())
    }

    /// Step 1c: terminal `y` becomes `i` when the stem has a vowel group.
    pub fn step1c(token: &str) -> String {
        let mut w = units(token);
        Self::step1c_units(&mut w);
        text(&w)
    }

    fn step1c_units(w: &mut [u16]) {
        let g = categorize_groups(w);
        // `g.substr(0, g.length - 1)` drops one character of the COMPRESSED
        // categorisation, whose length has nothing to do with the token's.
        let head = &g[..g.len().saturating_sub(1)];
        if w.last() == Some(&u('y')) && head.contains(&V) {
            let n = w.len();
            w[n - 1] = u('i');
        }
    }

    /// Step 2: the twenty-two long-suffix rewrites.
    pub fn step2(token: &str) -> String {
        text(&Self::step2_units(&units(token)))
    }

    fn step2_units(w: &[u16]) -> Vec<u16> {
        attempt_replace_patterns(w, STEP2, Some(0.0))
    }

    /// Step 3: the seven `-icate`/`-ful`/`-ness` rewrites.
    pub fn step3(token: &str) -> String {
        text(&Self::step3_units(&units(token)))
    }

    fn step3_units(w: &[u16]) -> Vec<u16> {
        attempt_replace_patterns(w, STEP3, Some(0.0))
    }

    /// Step 4: suffix removal for stems of measure greater than one.
    pub fn step4(token: &str) -> String {
        text(&Self::step4_units(&units(token)))
    }

    fn step4_units(w: &[u16]) -> Vec<u16> {
        if let Some(r) = strip_longest_alternative(w, STEP4, 0)
            && measure_units(&r) > 1.0
        {
            return r;
        }
        // `/^(.+?)(s|t)(ion)$/` keeps groups 1 AND 2, i.e. everything but `ion`.
        if let Some(r) = strip_longest_alternative(w, &["sion", "tion"], 1)
            && measure_units(&r) > 1.0
        {
            return r;
        }
        w.to_vec()
    }

    /// Step 5a: terminal `e` removal.
    pub fn step5a(token: &str) -> String {
        let mut w = units(token);
        Self::step5a_units(&mut w);
        text(&w)
    }

    fn step5a_units(w: &mut Vec<u16>) {
        // `token.replace(/e$/, '')` — a no-op when the token does not end in `e`,
        // which is why this step can fire on tokens that have no `e` to remove.
        let keep = w.len() - usize::from(w.last() == Some(&u('e')));
        let m = measure_units(&w[..keep]);
        // `categorizeChars(token).substr(-4, 3)`: a negative start clamps to 0,
        // so a three-unit token yields its whole categorisation.
        let cc = categorize_chars(w);
        let start = cc.len().saturating_sub(4);
        let end = (start + 3).min(cc.len());
        let cvc = cc[start..end] == [C, V, C];
        // `/[^wxy].$/` needs two units, and `.` refuses a line terminator.
        let short_e = w.len() >= 2
            && !matches!(w[w.len() - 2], c if c == u('w') || c == u('x') || c == u('y'))
            && !is_line_terminator(w[w.len() - 1]);
        if m > 1.0 || (m == 1.0 && !(cvc && short_e)) {
            w.truncate(keep);
        }
    }

    /// Step 5b: `ll` becomes `l` for stems of measure greater than one.
    pub fn step5b(token: &str) -> String {
        let mut w = units(token);
        Self::step5b_units(&mut w);
        text(&w)
    }

    fn step5b_units(w: &mut Vec<u16>) {
        if measure_units(w) > 1.0 && ends_with(w, "ll") {
            truncate_by(w, 1);
        }
    }

    /// Stems one token.
    ///
    /// Returns the input unchanged — and **uncased** — when it is shorter than
    /// three UTF-16 code units, which is what the reference's `token.length < 3`
    /// guard does before the `toLowerCase()` on the other branch.
    pub fn stem<'a>(&self, token: &'a str) -> Cow<'a, str> {
        if slen(token) < 3 {
            return Cow::Borrowed(token);
        }
        let mut w = units(&token.to_lowercase());
        Self::step1a_units(&mut w);
        Self::step1b_units(&mut w);
        Self::step1c_units(&mut w);
        w = Self::step2_units(&w);
        w = Self::step3_units(&w);
        w = Self::step4_units(&w);
        Self::step5a_units(&mut w);
        Self::step5b_units(&mut w);
        Cow::Owned(text(&w))
    }
}

/// Step 2's rewrite table, in the reference's exact order.
///
/// Each entry is `(suffix, guard replacement, real replacement)`. The guard
/// replacement is always `''`: the measure test is `m(stem without the suffix) > 0`.
static STEP2: &[(&str, &str, &str)] = &[
    ("ational", "", "ate"),
    ("tional", "", "tion"),
    ("enci", "", "ence"),
    ("anci", "", "ance"),
    ("izer", "", "ize"),
    ("abli", "", "able"),
    ("bli", "", "ble"),
    ("alli", "", "al"),
    ("entli", "", "ent"),
    ("eli", "", "e"),
    ("ousli", "", "ous"),
    ("ization", "", "ize"),
    ("ation", "", "ate"),
    ("ator", "", "ate"),
    ("alism", "", "al"),
    ("iveness", "", "ive"),
    ("fulness", "", "ful"),
    ("ousness", "", "ous"),
    ("aliti", "", "al"),
    ("iviti", "", "ive"),
    ("biliti", "", "ble"),
    ("logi", "", "log"),
];

/// Step 3's rewrite table, in the reference's exact order.
static STEP3: &[(&str, &str, &str)] = &[
    ("icate", "", "ic"),
    ("ative", "", ""),
    ("alize", "", "al"),
    ("iciti", "", "ic"),
    ("ical", "", "ic"),
    ("ful", "", ""),
    ("ness", "", ""),
];

/// Step 4's alternation, in the reference's exact order (which does not matter:
/// the `$` anchor makes the longest suffix win regardless).
static STEP4: &[&str] = &[
    "al", "ance", "ence", "er", "ic", "able", "ible", "ant", "ement", "ment", "ent", "ou", "ism",
    "ate", "iti", "ous", "ive", "ize",
];

impl TokenizeAndStem for PorterStemmer {
    const FILTER_ON: Casing = Casing::Raw;
    const STEM_ON: Casing = Casing::Raw;

    fn is_word_char(c: char) -> bool {
        classes::is_word_en(c)
    }

    /// The whole document is lowercased before tokenizing, so the stop-word test
    /// and `stem` both see an already-folded token.
    fn prepare(t: &str) -> Cow<'_, str> {
        Cow::Owned(t.to_lowercase())
    }

    fn is_stop_word(word: &str) -> bool {
        verbora_core::stopwords::is_default_stopword(word)
    }

    fn stem_token(&self, token: &str) -> String {
        self.stem(token).into_owned()
    }
}

impl verbora_core::Stemmer for PorterStemmer {
    fn stem<'a>(&self, token: &'a str) -> Cow<'a, str> {
        Self::stem(self, token)
    }
}

impl PorterStemmer {
    /// Appends a stop word to the **process-global English list**, which
    /// `LancasterStemmer` and the phonetics helpers also read.
    pub fn add_stop_word(&self, word: impl Into<String>) {
        verbora_core::stopwords::add_global_stopword(word);
    }

    /// Appends several stop words to the process-global English list.
    pub fn add_stop_words<I, S>(&self, words: I)
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        verbora_core::stopwords::add_global_stopwords(words);
    }

    /// Removes the first occurrence of `word` from the process-global list.
    pub fn remove_stop_word(&self, word: &str) {
        verbora_core::stopwords::remove_global_stopword(word);
    }

    /// Removes the first occurrence of each of `words`.
    pub fn remove_stop_words<'a, I>(&self, words: I)
    where
        I: IntoIterator<Item = &'a str>,
    {
        verbora_core::stopwords::remove_global_stopwords(words);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measure_is_fractional() {
        assert_eq!(PorterStemmer::measure(""), -1.0);
        assert_eq!(PorterStemmer::measure("a"), 0.0);
        assert_eq!(PorterStemmer::measure("syllog"), 2.0);
        assert_eq!(PorterStemmer::measure("sya"), 0.5);
        assert_eq!(PorterStemmer::measure("syaing"), 1.5);
        assert_eq!(PorterStemmer::measure("oypvyegg"), 2.5);
        assert_eq!(PorterStemmer::measure("fifugyed"), 3.5);
    }

    #[test]
    fn categorize_groups_can_emit_adjacent_vv() {
        assert_eq!(PorterStemmer::categorize_groups("syllog"), "CVCVC");
        assert_eq!(PorterStemmer::categorize_groups("gypsy"), "CVCV");
        assert_eq!(PorterStemmer::categorize_groups(""), "");
        assert_eq!(PorterStemmer::categorize_groups("sya"), "CVV");
        assert_eq!(PorterStemmer::categorize_groups("xyz"), "CVC");
    }

    #[test]
    fn short_tokens_keep_their_case() {
        let s = PorterStemmer::new();
        assert_eq!(s.stem(""), "");
        assert_eq!(s.stem("a"), "a");
        assert_eq!(s.stem("AB"), "AB");
        assert_eq!(s.stem("IS"), "IS");
        assert_eq!(s.stem("ABC"), "abc");
    }

    #[test]
    fn astral_input_counts_as_two_code_units() {
        // "😀s".length is 3 in the reference, so the algorithm runs and step 1a
        // strips the `s`. A char-counting port would see 2 and bail out.
        assert_eq!(PorterStemmer::new().stem("😀s"), "😀");
    }

    #[test]
    fn the_accumulator_quirk_is_reproduced() {
        assert_eq!(PorterStemmer::step3("formalizeful"), "formalize");
        assert_eq!(PorterStemmer::step2("rationalization"), "rationalize");
    }

    #[test]
    fn empty_replacements_are_discarded() {
        assert_eq!(PorterStemmer::step1b("ed"), "ed");
        assert_eq!(PorterStemmer::step1b("ing"), "ing");
    }

    #[test]
    fn documented_step_vectors() {
        for (input, want) in [
            ("caresses", "caress"),
            ("ponies", "poni"),
            ("ties", "ti"),
            ("caress", "caress"),
            ("cats", "cat"),
            ("us", "us"),
        ] {
            assert_eq!(PorterStemmer::step1a(input), want, "step1a({input})");
        }
        for (input, want) in [
            ("feed", "feed"),
            ("agreed", "agree"),
            ("plastered", "plaster"),
            ("bled", "bled"),
            ("motoring", "motor"),
            ("sing", "sing"),
            ("hopping", "hop"),
            ("filing", "file"),
            ("falling", "fall"),
            ("hissing", "hiss"),
            ("fizzed", "fizz"),
        ] {
            assert_eq!(PorterStemmer::step1b(input), want, "step1b({input})");
        }
        assert_eq!(PorterStemmer::step1c("happy"), "happi");
        assert_eq!(PorterStemmer::step1c("sky"), "sky");
        assert_eq!(PorterStemmer::step5a("probate"), "probat");
        assert_eq!(PorterStemmer::step5a("rate"), "rate");
        assert_eq!(PorterStemmer::step5a("ace"), "ac");
        assert_eq!(PorterStemmer::step5b("controll"), "control");
        assert_eq!(PorterStemmer::step5b("roll"), "roll");
    }

    #[test]
    fn unicode_and_pathological_inputs_survive() {
        let s = PorterStemmer::new();
        assert_eq!(s.stem("café"), "café");
        assert_eq!(s.stem("Ångström"), "ångström");
        assert_eq!(s.stem("Москва"), "москва");
        assert_eq!(s.stem("日本語"), "日本語");
        assert_eq!(s.stem("123"), "123");
        assert_eq!(s.stem("..."), "...");
        let long = "a".repeat(500);
        assert_eq!(s.stem(&long), long);
    }
}
