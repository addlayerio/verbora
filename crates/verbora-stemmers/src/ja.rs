//! The Japanese katakana stemmer, ported from
//! The reference `stemmer_ja`.
//!
//! The whole algorithm is one rule: drop a trailing U+30FC PROLONGED SOUND MARK
//! from a katakana-only string of at least four code units. `コーヒー` becomes
//! `コーヒ`; `コピー` is too short and is left alone; `ﾀｸｼｰ` is halfwidth katakana,
//! which the range `゠..ヿ` (U+30A0..U+30FF) excludes, so it is left alone too.
//! Only one mark is ever removed, never a run.
//!
//! `tokenizeAndStem` is the one place this crate cannot use the shared
//! [`TokenizeAndStem`](crate::TokenizeAndStem) machinery: `TokenizerJa` is a
//! TinySegmenter, not a character-class splitter. The inherent methods below have
//! the same signatures, and reproduce the same quirk the base classes have — the
//! stop-word test reads the **raw** token while the emitted token is the
//! lowercased, stemmed one.

use std::borrow::Cow;

use verbora_tokenizers::{Tokenize, TokenizerJa, Utf16Token};

use crate::stopwords::{self, Language};
use crate::units::slen;

/// U+30FC HIRAGANA-KATAKANA PROLONGED SOUND MARK.
const MARK: char = '\u{30FC}';

/// The Japanese stemmer.
///
/// ```
/// use verbora_stemmers::StemmerJa;
/// let s = StemmerJa::new();
/// assert_eq!(s.stem("コーヒー"), "コーヒ");
/// assert_eq!(s.stem("コピー"), "コピー");   // three characters: too short
/// assert_eq!(s.stem("ﾀｸｼｰ"), "ﾀｸｼｰ");     // halfwidth: not katakana
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StemmerJa;

impl StemmerJa {
    /// Creates the stemmer. It is stateless and zero-sized.
    #[inline]
    pub const fn new() -> Self {
        Self
    }

    /// Whether every character of `str` lies in U+30A0..U+30FF.
    ///
    /// The range is inclusive at both ends, so the middle dot `・`, the iteration
    /// marks `ヽ ヾ` and `ヿ` all count as katakana — and the prolonged sound mark
    /// itself does too, which is what lets `ーーーー` be stemmed to `ーーー`.
    /// An empty string is **not** katakana: The reference's `+` needs one match.
    #[allow(
        clippy::unused_self,
        reason = "mirrors the reference's method-shaped API"
    )]
    pub fn is_katakana(&self, str: &str) -> bool {
        !str.is_empty() && str.chars().all(|c| ('\u{30A0}'..='\u{30FF}').contains(&c))
    }

    /// Removes one trailing prolonged sound mark from a long katakana token.
    pub fn stem_katakana<'a>(&self, token: &'a str) -> Cow<'a, str> {
        // The reference tests `token.length >= 4` in UTF-16 code units, then
        // `isKatakana`. Katakana are all BMP, so for any string that passes the
        // second test the two length notions agree; the order of the tests makes
        // the difference unobservable rather than merely unlikely.
        if slen(token) >= 4 && token.ends_with(MARK) && self.is_katakana(token) {
            return Cow::Borrowed(&token[..token.len() - MARK.len_utf8()]);
        }
        Cow::Borrowed(token)
    }

    /// Stems one token. Delegates to [`Self::stem_katakana`]; nothing else
    /// happens, and the token is **not** lowercased.
    pub fn stem<'a>(&self, token: &'a str) -> Cow<'a, str> {
        self.stem_katakana(token)
    }

    /// Lazily yields the stemmed tokens of `text`.
    ///
    /// The stop-word test uses the **raw** token while the value emitted is
    /// `stem(token.toLowerCase())`, so the predicate and the output are computed
    /// from different strings. Japanese stop words are caseless, which makes the
    /// difference latent rather than absent — a fullwidth-Latin token whose
    /// lowercase form is a stop word survives the filter and is emitted folded.
    pub fn stems<'a>(
        &'a self,
        text: &'a str,
        keep_stops: bool,
    ) -> impl Iterator<Item = String> + 'a {
        // `Tokenize::tokens` returns `impl Iterator`, which captures the borrow
        // of the tokenizer it was called on. Writing `TokenizerJa::new().tokens()`
        // would therefore borrow a temporary that dies at the end of this
        // expression. The tokenizer is zero-sized and stateless, so a `static`
        // gives the returned iterator a `'static` borrow at no cost — the
        // alternative, storing an owned tokenizer in a hand-written iterator
        // struct, would add a type for nothing.
        static TOKENIZER: TokenizerJa = TokenizerJa::new();

        TOKENIZER.tokens(text).filter_map(move |tok| {
            let raw = match &tok {
                Utf16Token::Text(s) => s.as_ref(),
                // A token that is half of a surrogate pair cannot be a stop word
                // and cannot be katakana, so it passes straight through.
                Utf16Token::Raw(_) => return Some(tok.to_string()),
            };
            if !keep_stops && stopwords::contains(Language::Ja, raw) {
                return None;
            }
            Some(self.stem(&raw.to_lowercase()).into_owned())
        })
    }

    /// Tokenizes `text` and stems each token, dropping stop words unless
    /// `keep_stops`.
    pub fn tokenize_and_stem(&self, text: &str, keep_stops: bool) -> Vec<String> {
        self.stems(text, keep_stops).collect()
    }
}

impl verbora_core::Stemmer for StemmerJa {
    fn stem<'a>(&self, token: &'a str) -> Cow<'a, str> {
        Self::stem(self, token)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn documented_vectors() {
        let s = StemmerJa::new();
        for (input, want) in [
            ("コピー", "コピー"),
            ("コーヒー", "コーヒ"),
            ("タクシー", "タクシ"),
            ("パーティー", "パーティ"),
            ("パーティ", "パーティ"),
            ("ヘルプ・センター", "ヘルプ・センタ"),
            ("ﾀｸｼｰ", "ﾀｸｼｰ"),
            ("ーーーー", "ーーー"),
            ("ーーー", "ーーー"),
            ("アイウー", "アイウ"),
            ("", ""),
        ] {
            assert_eq!(s.stem(input), want, "stem({input})");
        }
    }

    #[test]
    fn only_one_mark_is_removed() {
        assert_eq!(StemmerJa::new().stem("アイウエーー"), "アイウエー");
    }

    #[test]
    fn is_katakana_boundaries() {
        let s = StemmerJa::new();
        assert!(!s.is_katakana(""));
        assert!(s.is_katakana("゠"));
        assert!(s.is_katakana("ヿ"));
        assert!(s.is_katakana("・"));
        assert!(!s.is_katakana("あ"));
        assert!(!s.is_katakana("ｱ"));
        assert!(!s.is_katakana("アa"));
        assert!(!s.is_katakana("ア\nア"));
        assert!(!s.is_katakana("😀"));
    }
}
