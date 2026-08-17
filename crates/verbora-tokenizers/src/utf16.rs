//! A token type that can represent what the reference actually produces.
//!
//! # Why this exists
//!
//! Four of these tokenizers cut text at UTF-16 **code unit** boundaries:
//! [`WordPunctTokenizer`](crate::WordPunctTokenizer) and
//! [`TreebankWordTokenizer`](crate::TreebankWordTokenizer) because their regexes
//! use a bare `.` (which matches one code unit), [`TokenizerJa`](crate::TokenizerJa)
//! because it starts with `text.split('')`, and [`CaseTokenizer`](crate::CaseTokenizer)
//! because it indexes `text[i]` with `i` counting code units. For an astral-plane
//! character such as `😀` that means the two halves of the surrogate pair land in
//! *separate tokens*:
//!
//! ```text
//! WordPunctTokenizer().tokenize("a😀b")  →  ["a", "\ud83d", "\ude00", "b"]
//! ```
//!
//! An unpaired surrogate is not a Unicode scalar value, so it cannot be held by
//! `char`, `String` or `&str`. A port that yields `String` therefore has three
//! options: replace the halves with U+FFFD (wrong content), merge them back into
//! one token (wrong token *count*), or represent them honestly. This type does
//! the third.
//!
//! The well-formed case — everything except astral input — stays a plain
//! `Cow<'a, str>`, and borrows the input wherever the algorithm permits, so the
//! representation costs nothing on ordinary text.

use std::borrow::Cow;
use std::fmt;

/// One token from a tokenizer that operates on UTF-16 code units.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Utf16Token<'a> {
    /// Well-formed text. Borrowed from the input when the tokenizer only slices
    /// it, owned when the tokenizer had to rewrite it.
    Text(Cow<'a, str>),
    /// Code units that are **not** well-formed UTF-16 — in practice a token that
    /// is half of a surrogate pair. Kept verbatim so no information is lost.
    Raw(Box<[u16]>),
}

impl<'a> Utf16Token<'a> {
    /// A token borrowed from the input.
    #[inline]
    pub const fn borrowed(s: &'a str) -> Self {
        Self::Text(Cow::Borrowed(s))
    }

    /// A token that owns its text.
    #[inline]
    pub const fn owned(s: String) -> Self {
        Self::Text(Cow::Owned(s))
    }

    /// Builds a token from raw code units, preferring the well-formed
    /// representation.
    ///
    /// This is the only constructor that can produce [`Utf16Token::Raw`], and it
    /// does so only when the units genuinely cannot be decoded.
    pub fn from_units(units: &[u16]) -> Self {
        match String::from_utf16(units) {
            Ok(s) => Self::Text(Cow::Owned(s)),
            Err(_) => Self::Raw(units.into()),
        }
    }

    /// The token as a string slice, or `None` if it contains unpaired surrogates.
    #[inline]
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::Text(s) => Some(s),
            Self::Raw(_) => None,
        }
    }

    /// Whether the token is well-formed Unicode.
    #[inline]
    pub const fn is_well_formed(&self) -> bool {
        matches!(self, Self::Text(_))
    }

    /// Appends the token's UTF-16 code units to `out`.
    ///
    /// This is the lossless view, and the one parity checks compare on: it is
    /// exactly the sequence of units the reference string would contain.
    pub fn extend_utf16(&self, out: &mut Vec<u16>) {
        match self {
            Self::Text(s) => out.extend(s.encode_utf16()),
            Self::Raw(u) => out.extend_from_slice(u),
        }
    }

    /// The token's UTF-16 code units.
    pub fn to_utf16(&self) -> Vec<u16> {
        let mut out = Vec::new();
        self.extend_utf16(&mut out);
        out
    }

    /// The token as text, substituting U+FFFD for unpaired surrogates.
    ///
    /// Convenience for callers that must have a `String`; prefer [`Self::as_str`]
    /// plus explicit handling when exactness matters.
    pub fn to_string_lossy(&self) -> Cow<'_, str> {
        match self {
            Self::Text(s) => Cow::Borrowed(s),
            Self::Raw(u) => Cow::Owned(String::from_utf16_lossy(u)),
        }
    }

    /// Converts to an owned token with a `'static` lifetime.
    pub fn into_owned(self) -> Utf16Token<'static> {
        match self {
            Self::Text(s) => Utf16Token::Text(Cow::Owned(s.into_owned())),
            Self::Raw(u) => Utf16Token::Raw(u),
        }
    }
}

impl fmt::Display for Utf16Token<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_string_lossy())
    }
}

impl PartialEq<str> for Utf16Token<'_> {
    fn eq(&self, other: &str) -> bool {
        self.as_str() == Some(other)
    }
}

impl PartialEq<&str> for Utf16Token<'_> {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == Some(*other)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn well_formed_units_decode() {
        let t = Utf16Token::from_units(&[0x61, 0x62]);
        assert_eq!(t.as_str(), Some("ab"));
        assert!(t.is_well_formed());
    }

    #[test]
    fn lone_surrogate_survives_intact() {
        let t = Utf16Token::from_units(&[0xd83d]);
        assert_eq!(t.as_str(), None);
        assert_eq!(t.to_utf16(), vec![0xd83d]);
        // Lossy rendering is available but must not be what parity compares on.
        assert_eq!(t.to_string_lossy(), "\u{fffd}");
    }

    #[test]
    fn paired_surrogates_are_well_formed() {
        let t = Utf16Token::from_units(&[0xd83d, 0xde00]);
        assert_eq!(t.as_str(), Some("😀"));
    }

    #[test]
    fn utf16_roundtrips_for_every_script() {
        for s in ["", "a", "café", "Привет", "日本語", "😀", "a😀b"] {
            let t = Utf16Token::borrowed(s);
            assert_eq!(t.to_utf16(), s.encode_utf16().collect::<Vec<_>>());
        }
    }
}
