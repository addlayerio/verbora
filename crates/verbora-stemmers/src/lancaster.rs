//! The Lancaster stemmer — Paice/Husk.
//!
//! # The algorithm in one paragraph
//!
//! Look up the rule section for the token's **last scalar value**. Walk that
//! section in order. A rule applies when the token ends with its pattern (and,
//! for `intact` rules, when the token has not been modified yet). Chop `size`
//! units off the end, append the rule's `appendage` if it has one, and test the
//! candidate: a stem starting with a vowel must be longer than one unit,
//! anything else must be longer than two *and* contain a vowel or `y`. If the
//! candidate is acceptable the rule wins — recurse when it says `continuation`,
//! stop otherwise. If it is not acceptable, keep walking. If nothing wins, the
//! token is returned unchanged.
//!
//! # What is easy to get wrong
//!
//! Six rules have `size: '0'`. They compute a candidate *equal to the token* and
//! all six have `continuation: false`, so they act as **stop rules**: they return
//! the word verbatim and prevent every later rule in the same section from
//! firing. Delete them as no-ops and `ear`, `miss`, `seen`, `consist` and
//! `simply` all start stemming.
//!
//! `size` is written as a decimal string in the published rule table; it is
//! parsed to an integer once, at table-generation time, so the length
//! arithmetic below is integer arithmetic.
//!
//! Lengths are Unicode scalar values, as everywhere in this crate.
//! `stem("😀ing")` is `"😀ing"` because the candidate `"😀"` has length 1, and
//! `acceptable` requires at least 2 before any rule may fire.

use std::borrow::Cow;

use crate::base::{Casing, TokenizeAndStem};
use crate::data::lancaster_rules;
use crate::units::slen;

/// One entry of the Paice/Husk rule table.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Rule {
    /// The suffix the token must end with.
    pub(crate) pattern: &'static str,
    /// How many scalar values to remove. Zero makes this a stop rule.
    pub(crate) size: usize,
    /// Text appended after the removal, if any.
    pub(crate) appendage: Option<&'static str>,
    /// Whether stemming continues on the result.
    pub(crate) continuation: bool,
    /// Whether the rule may only fire on an unmodified token.
    pub(crate) intact: bool,
}

/// The Lancaster (Paice/Husk) stemmer.
///
/// ```
/// use verbora_stemmers::LancasterStemmer;
/// let s = LancasterStemmer::new();
/// assert_eq!(s.stem("maximum"), "maxim");
/// assert_eq!(s.stem("presumably"), "presum");
/// // A size-0 stop rule keeps this one intact.
/// assert_eq!(s.stem("ear"), "ear");
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LancasterStemmer;

/// `candidate.match(/^[aeiou]/) ? length > 1 : length > 2 && /[aeiouy]/`
///
/// Both classes are ASCII and lower-case only, which is safe because the input
/// was folded before the first rule ran.
fn acceptable(candidate: &str) -> bool {
    let len = slen(candidate);
    match candidate.as_bytes().first() {
        Some(b'a' | b'e' | b'i' | b'o' | b'u') => len > 1,
        _ => {
            len > 2
                && candidate
                    .bytes()
                    .any(|b| matches!(b, b'a' | b'e' | b'i' | b'o' | b'u' | b'y'))
        }
    }
}

/// One pass of the rule-section walk, flattened from recursion into a loop.
///
/// The algorithm is stated recursively, once per accepted continuation rule, and
/// depth reaches about 300 for `"ing".repeat(300)`. The recursive call is always
/// in tail position, so the loop is observationally identical and costs no stack.
fn apply_rule_sections(mut token: String, mut intact: bool) -> String {
    'outer: loop {
        // Sections are keyed by the token's last scalar value. No section is
        // keyed by an astral character, so a token ending in one matches
        // nothing and is returned whole.
        let Some(last) = token.chars().next_back() else {
            return token;
        };
        for rule in lancaster_rules::section(last) {
            if !(intact || !rule.intact) {
                continue;
            }
            // `token.substr(0 - pattern.length) === pattern`: a negative start
            // clamps, so an over-long pattern compares the whole token and fails.
            if !token.ends_with(rule.pattern) {
                continue;
            }
            // `size` never exceeds the matched pattern's length in the shipped
            // table, so the cut always lands on a character boundary.
            let keep = token.len() - rule.size;
            let mut result = String::with_capacity(keep + 2);
            result.push_str(&token[..keep]);
            if let Some(app) = rule.appendage {
                result.push_str(app);
            }
            if !acceptable(&result) {
                continue;
            }
            if rule.continuation {
                token = result;
                intact = false;
                continue 'outer;
            }
            return result;
        }
        return token;
    }
}

impl LancasterStemmer {
    /// Creates the stemmer. It is stateless and zero-sized.
    #[inline]
    pub const fn new() -> Self {
        Self
    }

    /// Stems one token.
    ///
    /// The token is lower-cased with full Unicode semantics first — `"CAFÉ"`
    /// becomes `"café"`, and `"İs"` becomes `"i\u{307}s"` before any rule runs.
    pub fn stem<'a>(&self, token: &'a str) -> Cow<'a, str> {
        let lower = token.to_lowercase();
        let stemmed = apply_rule_sections(lower, true);
        if stemmed == token {
            Cow::Borrowed(token)
        } else {
            Cow::Owned(stemmed)
        }
    }

    /// Appends a stop word to the **process-global English list**, shared with
    /// [`crate::PorterStemmer`].
    pub fn add_stop_word(&self, word: impl Into<String>) {
        verbora_core::add_global_stopword(word);
    }

    /// Appends several stop words to the process-global English list.
    pub fn add_stop_words<I, S>(&self, words: I)
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        verbora_core::add_global_stopwords(words);
    }

    /// Removes the first occurrence of `word` from the process-global list.
    pub fn remove_stop_word(&self, word: &str) {
        verbora_core::remove_global_stopword(word);
    }

    /// Removes the first occurrence of each of `words`.
    pub fn remove_stop_words<'a, I>(&self, words: I)
    where
        I: IntoIterator<Item = &'a str>,
    {
        verbora_core::remove_global_stopwords(words);
    }
}

impl TokenizeAndStem for LancasterStemmer {
    const FILTER_ON: Casing = Casing::Raw;
    const STEM_ON: Casing = Casing::Raw;

    fn prepare(t: &str) -> Cow<'_, str> {
        Cow::Owned(t.to_lowercase())
    }

    fn is_stop_word(word: &str) -> bool {
        verbora_core::is_global_stopword(word)
    }

    fn stem_token(&self, token: &str) -> String {
        self.stem(token).into_owned()
    }
}

impl verbora_core::Stemmer for LancasterStemmer {
    fn stem<'a>(&self, token: &'a str) -> Cow<'a, str> {
        Self::stem(self, token)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(t: &str) -> String {
        LancasterStemmer::new().stem(t).into_owned()
    }

    #[test]
    fn documented_vectors() {
        for (input, want) in [
            ("marks", "mark"),
            ("MARKs", "mark"),
            ("living", "liv"),
            ("thing", "thing"),
            ("ear", "ear"),
            ("string", "string"),
            ("triplicate", "triply"),
            ("triPlicAte", "triply"),
            ("classified", "class"),
            ("maximum", "maxim"),
            ("presumably", "presum"),
            ("exceed", "excess"),
            ("anguish", "anct"),
            ("affluxion", "affluct"),
            ("discept", "disceiv"),
            ("this", "thy"),
            ("exist", "ex"),
            ("ancy", "ant"),
            ("media", "med"),
            ("running", "run"),
            ("runninging", "run"),
            ("eeeeeeee", "ee"),
            ("sionsionsion", "sionsiond"),
            ("butt", "but"),
        ] {
            assert_eq!(s(input), want, "stem({input})");
        }
    }

    #[test]
    fn size_zero_rules_are_stop_rules() {
        for w in ["ear", "seen", "miss", "consist", "simply", "ss", "gas"] {
            assert_eq!(s(w), w, "{w} should be left alone by its size-0 rule");
        }
    }

    /// One astral character is one scalar value, and `acceptable` needs a
    /// candidate of at least two before any rule may fire.
    ///
    /// `"\u{1F600}es"` is the case that separates the two readings. The
    /// candidate after chopping `s` is `"\u{1F600}e"`, whose first character is
    /// not an ASCII vowel, so `acceptable` takes its `len > 2 && contains a
    /// vowel` arm. Counting scalar values that length is 2, the arm rejects,
    /// and the token is returned whole. Measuring the same text in UTF-16 code
    /// units it was 3 — two surrogates plus `e` — the arm accepted, and the
    /// stem came back `"\u{1F600}e"`. Every other language in this crate has a
    /// test pinning this; without one here the unit could revert unnoticed.
    #[test]
    fn an_astral_character_counts_as_one_position() {
        assert_eq!(s("\u{1F600}es"), "\u{1F600}es");
        assert_eq!(s("\u{1D7CE}aal"), "\u{1D7CE}aal");
    }

    #[test]
    fn edges_and_unicode() {
        assert_eq!(s(""), "");
        assert_eq!(s("a"), "a");
        assert_eq!(s("I"), "i");
        assert_eq!(s("CAFÉ"), "café");
        assert_eq!(s("ÅÄÖs"), "åäös");
        assert_eq!(s("naïve"), "naïv");
        assert_eq!(s("ünïcödé"), "ünïcödé");
        assert_eq!(s("İs"), "i\u{307}");
        assert_eq!(s("😀ing"), "😀ing");
        assert_eq!(s("日本語"), "日本語");
        assert_eq!(s("123"), "123");
    }

    #[test]
    fn deep_recursion_terminates() {
        assert_eq!(s(&"ing".repeat(300)), "ing");
        // `ing` is NOT stripped here: the `ing` rules are `intact`-only or
        // require an acceptable stem, and 1000 `x`s never becomes one.
        let long = "x".repeat(1000) + "ing";
        assert_eq!(s(&long), long);
    }
}
