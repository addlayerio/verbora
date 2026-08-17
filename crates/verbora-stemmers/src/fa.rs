//! The Persian stemmer, ported from the reference `porter_stemmer_fa`.
//!
//! `stem` is the **identity function**. The reference says so in a comment —
//! "disabled stemming for Farsi / Farsi stemming will be supported soon" — and
//! the whole of the exported behaviour is therefore in `tokenizeAndStem`: split
//! on whitespace, drop the 26 stop words, return the tokens verbatim. Nothing is
//! lowercased and no gate is applied.

use std::borrow::Cow;

use verbora_core::whitespace::is_whitespace;

use crate::base::{Casing, TokenizeAndStem};
use crate::stopwords::{self, Language};

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

/// The fourteen characters `AggressiveTokenizerFa#clearText` looks for.
///
/// The reference regex is `/.:\+-=\(\)"'!\?،,؛;/g`. It looks like a punctuation
/// class with the brackets forgotten; what it compiles to is a *sequence*: any
/// character except a line terminator, followed by these fourteen literals in
/// this exact order. It therefore matches essentially nothing — but "essentially"
/// is not "provably", so it is reproduced rather than dropped.
const CLEAR_LITERAL: &str = ":+-=()\"'!?،,؛;";

#[inline]
const fn is_line_terminator(c: char) -> bool {
    matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}')
}

/// `text.replace(/.:\+-=\(\)"'!\?،,؛;/g, ' ')`.
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

    /// Returns `token` unchanged. Persian stemming is disabled upstream.
    #[allow(
        clippy::unused_self,
        reason = "mirrors the reference's method-shaped API"
    )]
    pub fn stem<'a>(&self, token: &'a str) -> Cow<'a, str> {
        Cow::Borrowed(token)
    }
}

impl TokenizeAndStem for PorterStemmerFa {
    const FILTER_ON: Casing = Casing::Raw;
    const STEM_ON: Casing = Casing::Raw;

    fn is_word_char(c: char) -> bool {
        !is_whitespace(c)
    }

    fn prepare(t: &str) -> Cow<'_, str> {
        clear_text(t)
    }

    fn is_stop_word(word: &str) -> bool {
        stopwords::contains(Language::Fa, word)
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

    #[test]
    fn tokenizing_splits_on_whitespace_only() {
        let s = PorterStemmerFa::new();
        // Punctuation stays attached: the class the author meant never compiled.
        assert_eq!(s.tokenize_and_stem("سلام، دنیا!", true), ["سلام،", "دنیا!"]);
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
