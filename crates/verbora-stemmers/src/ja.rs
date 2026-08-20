//! The Japanese katakana stemmer.
//!
//! The whole algorithm is one rule: drop a trailing U+30FC PROLONGED SOUND MARK
//! from a katakana-only string of at least four scalar values. `コーヒー` becomes
//! `コーヒ`; `コピー` is too short and is left alone; `ﾀｸｼｰ` is halfwidth katakana,
//! which the range `゠..ヿ` (U+30A0..U+30FF) excludes, so it is left alone too.
//! Only one mark is ever removed, never a run.
//!
//! # No tokenization here
//!
//! This stemmer stems one token; it does not cut text into tokens. The
//! `stems`/`tokenize_and_stem` pair that used to live here was built on a
//! TinySegmenter whose 1,480-line weight table had no version, no checksum, no
//! upstream URL and no generator anywhere in the repository, so its output was
//! unauditable and could not be defended as a specification. UAX #29 §4 states
//! outright that its default rules do not segment Japanese, so
//! [`verbora_tokenizers::WordTokenizer`] is not a substitute and none is
//! offered: **the caller supplies the segmentation** and calls
//! [`StemmerJa::stem`] on each token.
//!
//! An attributable Japanese segmenter — a cited model, a checked-in generator,
//! a licence review — is a future crate, and adding it will not be a breaking
//! change.

use std::borrow::Cow;

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
        reason = "every stemmer is zero-sized; `stem` is a method so the \
                  sixteen of them share one call shape"
    )]
    pub fn is_katakana(&self, str: &str) -> bool {
        !str.is_empty() && str.chars().all(|c| ('\u{30A0}'..='\u{30FF}').contains(&c))
    }

    /// Removes one trailing prolonged sound mark from a long katakana token.
    pub fn stem_katakana<'a>(&self, token: &'a str) -> Cow<'a, str> {
        // The length gate counts scalar values, like the rest of this crate;
        // every katakana scalar is inside the BMP, so the two readings of
        // "four" coincide for any input this rule can fire on. Then
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
