//! The Persian stemmer.
//!
//! `stem` is the **identity function**: Verbora ships no Persian suffix rules,
//! so the whole of the exported behaviour is in `tokenize_and_stem` — cut at
//! UAX #29 word boundaries, drop the 26 stop words, return the tokens
//! unchanged. Nothing is lowercased and no gate is applied.
//!
//! The token stream is the shared [`verbora_tokenizers::WordTokenizer`] one, so
//! Persian punctuation and the Arabic comma separate words rather than clinging
//! to them.

use std::borrow::Cow;

use crate::base::{Casing, TokenizeAndStem};
use crate::stopwords::Language;

/// The (disabled) Persian stemmer.
///
/// ```
/// use verbora_stemmers::{PorterStemmerFa, TokenizeAndStem};
/// let s = PorterStemmerFa::new();
/// assert_eq!(s.stem("کتاب"), "کتاب");
/// // Nothing is dropped here: the 26-entry `stopwords_fa` list holds `از با یه
/// // برای و باید شاید` and a run of punctuation, and none of `را در است` is on
/// // it — a reminder that the filter is much narrower than it looks.
/// assert_eq!(
///     s.tokenize_and_stem("کتاب را در است", false),
///     ["کتاب", "را", "در", "است"]
/// );
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PorterStemmerFa;

/// The fourteen literal characters the Persian pre-pass looks for.
///
/// The pre-pass is specified by the regex `/.:\+-=\(\)"'!\?،,؛;/g`, and that
/// pattern is a **sequence**, not a character class: any character except a
/// line terminator, followed by these fourteen literals in this exact order.
/// Real Persian text therefore essentially never matches it. Verbora keeps the
/// pass as specified rather than reinterpreting it as the punctuation class it
/// resembles — "essentially never" is not "never", and rewriting it would strip
/// characters from tokens that currently keep them.
const CLEAR_LITERAL: &str = ":+-=()\"'!?،,؛;";

#[inline]
const fn is_line_terminator(c: char) -> bool {
    matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}')
}

/// Replaces every match of `/.:\+-=\(\)"'!\?،,؛;/g` with a space.
fn clear_text(text: &str) -> Cow<'_, str> {
    if !text.contains(CLEAR_LITERAL) {
        return Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(text.len());
    let mut from = 0;
    let mut copied = 0;
    while let Some(rel) = text[from..].find(CLEAR_LITERAL) {
        let lit = from + rel;
        let end = lit + CLEAR_LITERAL.len();
        match text[..lit].chars().next_back() {
            Some(c) if !is_line_terminator(c) => {
                let start = lit - c.len_utf8();
                out.push_str(&text[copied..start]);
                out.push(' ');
                copied = end;
                from = end;
            }
            _ => from = lit + 1,
        }
    }
    out.push_str(&text[copied..]);
    Cow::Owned(out)
}

impl PorterStemmerFa {
    /// Creates the stemmer. It is stateless and zero-sized.
    #[inline]
    pub const fn new() -> Self {
        Self
    }

    /// Returns `token` unchanged: this crate ships no Persian suffix rules.
    #[allow(
        clippy::unused_self,
        reason = "every stemmer is zero-sized; `stem` is a method so the \
                  sixteen of them share one call shape"
    )]
    pub fn stem<'a>(&self, token: &'a str) -> Cow<'a, str> {
        Cow::Borrowed(token)
    }
}

impl TokenizeAndStem for PorterStemmerFa {
    const FILTER_ON: Casing = Casing::Raw;
    const STEM_ON: Casing = Casing::Raw;

    fn prepare(t: &str) -> Cow<'_, str> {
        clear_text(t)
    }

    fn is_stop_word(word: &str) -> bool {
        Language::Fa.contains(word)
    }

    fn stem_token(&self, token: &str) -> String {
        token.to_owned()
    }
}

impl verbora_core::Stemmer for PorterStemmerFa {
    fn stem<'a>(&self, token: &'a str) -> Cow<'a, str> {
        Self::stem(self, token)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stem_is_the_identity() {
        let s = PorterStemmerFa::new();
        for t in ["", "a", "کتاب", "ABC", "😀", "  ", "کتاب‌ها"] {
            assert_eq!(s.stem(t), t);
        }
    }

    /// Persian cuts at UAX #29 word boundaries like every other language, not
    /// on runs of non-whitespace.
    ///
    /// U+060C ARABIC COMMA and U+0021 EXCLAMATION MARK are `Word_Break=Other`,
    /// so they are their own segments and are dropped by the word filter rather
    /// than clinging to the preceding token — which matters because a token
    /// carrying its own trailing punctuation cannot match a stop-word list.
    #[test]
    fn tokenizing_cuts_at_word_boundaries() {
        let s = PorterStemmerFa::new();
        assert_eq!(s.tokenize_and_stem("سلام، دنیا!", true), ["سلام", "دنیا"]);
        // U+200C ZERO WIDTH NON-JOINER is Format, so WB4 attaches it to the
        // preceding letter and a Persian compound stays one token.
        assert_eq!(
            s.tokenize_and_stem("کتاب\u{200c}ها", true),
            ["کتاب\u{200c}ها"]
        );
        assert_eq!(s.tokenize_and_stem("", false), Vec::<String>::new());
        assert_eq!(s.tokenize_and_stem("   ", false), Vec::<String>::new());
    }

    #[test]
    fn clear_text_only_fires_on_the_literal_sequence() {
        assert!(matches!(clear_text("a:b"), Cow::Borrowed(_)));
        let hit = format!("x{CLEAR_LITERAL}y");
        assert_eq!(clear_text(&hit), " y");
    }
}
