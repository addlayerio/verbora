//! The three values every other module is built from: [`Tag`], [`Word`] and
//! [`TaggedToken`]. The literal contract both [`Tag`] and [`Word`] enforce is
//! documented on [`Tag`], which is the public home for it.

use std::borrow::{Borrow, Cow};
use std::fmt;
use std::str::FromStr;

/// Why a [`Tag`] or a [`Word`] was rejected.
///
/// Both literal types accept exactly the same strings — see [`Tag`] for the
/// contract — so both produce this error.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum LiteralError {
    /// The string was empty. A rule string separates its fields with spaces, so
    /// an empty field is not writable and not parseable.
    Empty,
    /// The string contained a Unicode `White_Space` scalar, which would split
    /// the field in two when the rule is written out.
    Whitespace {
        /// The first offending scalar.
        found: char,
    },
}

impl fmt::Display for LiteralError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("a tag or word literal may not be empty"),
            Self::Whitespace { found } => write!(
                f,
                "a tag or word literal may not contain whitespace (found U+{:04X})",
                *found as u32
            ),
        }
    }
}

impl std::error::Error for LiteralError {}

/// Checks the shared literal contract.
fn check(s: &str) -> Result<(), LiteralError> {
    if s.is_empty() {
        return Err(LiteralError::Empty);
    }
    match s.chars().find(|c| c.is_whitespace()) {
        Some(found) => Err(LiteralError::Whitespace { found }),
        None => Ok(()),
    }
}

macro_rules! literal_type {
    ($name:ident, $what:literal) => {
        impl $name {
            /// Builds the literal, checking the contract.
            ///
            /// Passing a `&'static str` borrows it; every tag the bundled data
            /// produces is a `&'static str` embedded in the binary, so tagging a
            /// document allocates nothing for its tags.
            ///
            /// # Errors
            ///
            /// [`LiteralError::Empty`] for the empty string, and
            /// [`LiteralError::Whitespace`] when any scalar has the Unicode
            /// `White_Space` property.
            pub fn new(value: impl Into<Cow<'static, str>>) -> Result<Self, LiteralError> {
                let value = value.into();
                check(&value)?;
                Ok(Self(value))
            }

            /// The literal as a string slice.
            #[inline]
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl std::ops::Deref for $name {
            type Target = str;
            #[inline]
            fn deref(&self) -> &str {
                &self.0
            }
        }

        impl AsRef<str> for $name {
            #[inline]
            fn as_ref(&self) -> &str {
                &self.0
            }
        }

        impl Borrow<str> for $name {
            #[inline]
            fn borrow(&self) -> &str {
                &self.0
            }
        }

        impl PartialEq<str> for $name {
            #[inline]
            fn eq(&self, other: &str) -> bool {
                &*self.0 == other
            }
        }

        impl PartialEq<&str> for $name {
            #[inline]
            fn eq(&self, other: &&str) -> bool {
                &*self.0 == *other
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, concat!($what, "({:?})"), &*self.0)
            }
        }

        impl FromStr for $name {
            type Err = LiteralError;
            fn from_str(s: &str) -> Result<Self, LiteralError> {
                Self::new(s.to_owned())
            }
        }

        impl TryFrom<&'static str> for $name {
            type Error = LiteralError;
            fn try_from(s: &'static str) -> Result<Self, LiteralError> {
                Self::new(s)
            }
        }
    };
}

/// A part-of-speech label, such as `NN` or `Adj(attr,stell,onverv)`.
///
/// Verbora attaches no meaning to a tag beyond string identity: the tag set is
/// whatever the lexicon and the rules agree on. The bundled English data uses
/// the Penn Treebank tag set plus the Verbora-defined `URL`; the bundled Dutch
/// data uses the tag set of its own source lexicon.
///
/// # The literal contract
///
/// A `Tag` and a [`Word`] are both *literals*: short strings that appear
/// verbatim in a rule string, where fields are separated by whitespace. Both are
/// therefore constrained the same way — **non-empty, and containing no scalar
/// with the Unicode `White_Space` property** ([UAX #44]).
///
/// The constraint is not decoration. It is exactly what makes
/// `rule.to_string().parse::<Rule>() == Ok(rule)` hold for every rule that can
/// be built, and [`RuleSet`](crate::RuleSet)'s round-trip test asserts that over
/// all 313 bundled rules. Both constructors return [`Result`], so a value that
/// would break the round trip cannot be constructed at all.
///
/// ```
/// use verbora_tagger::{LiteralError, Tag, Word};
///
/// assert!(Tag::new("Adj(attr,stell,onverv)").is_ok());
/// assert!(Word::new(",").is_ok());
/// assert_eq!(Tag::new(""), Err(LiteralError::Empty));
/// assert_eq!(Word::new("a b"), Err(LiteralError::Whitespace { found: ' ' }));
/// // The property, not an ad-hoc set: U+00A0 has White_Space and is rejected,
/// // U+FEFF does not and is accepted.
/// assert!(Tag::new("a\u{00a0}b").is_err());
/// assert!(Tag::new("a\u{feff}b").is_ok());
/// ```
///
/// Nothing is folded, trimmed or normalised on the way in: `Tag::new("İ")` is
/// the tag `İ`.
///
/// [UAX #44]: https://www.unicode.org/reports/tr44/#White_Space
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Tag(Cow<'static, str>);

literal_type!(Tag, "Tag");

impl Tag {
    /// Borrows a `&'static str` **without checking the literal contract**.
    ///
    /// Crate-private, and used only for two families of value whose conformance
    /// is established elsewhere: the tags packed by `build.rs`, which rejects
    /// any that would violate the contract and whose output `data.rs` re-checks
    /// entry by entry, and the four default tags named in `language.rs`, checked
    /// by `language::tests::bundled_defaults_satisfy_the_literal_contract`.
    pub(crate) const fn from_static(value: &'static str) -> Self {
        Self(Cow::Borrowed(value))
    }
}

/// A literal token fragment a [`Condition`](crate::Condition) compares against.
///
/// Most conditions compare a `Word` against a whole token; the one exception is
/// [`Condition::CurrentWordEndsWith`](crate::Condition::CurrentWordEndsWith),
/// where it is a suffix. Both are literal text that must survive being written
/// into a rule string, hence the same contract — documented in full on [`Tag`].
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Word(Cow<'static, str>);

literal_type!(Word, "Word");

/// One token with the tag currently assigned to it.
///
/// The token borrows from the caller's input for the lifetime `'a`, so tagging a
/// slice of `&str` copies no text at all. Use [`TaggedToken::into_owned`] to
/// detach it.
///
/// **Nothing in this crate ever rewrites `token`.** Case folding, trimming and
/// normalisation are the caller's explicit choice; a token that went into the
/// tagger comes out byte-identical.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TaggedToken<'a> {
    /// The token, exactly as the caller supplied it.
    pub token: Cow<'a, str>,
    /// Its part-of-speech tag. Always present: the initial-state annotator
    /// assigns a lexicon tag or the lexicon's default, never nothing.
    pub tag: Tag,
}

impl<'a> TaggedToken<'a> {
    /// Pairs a token with a tag.
    #[inline]
    pub fn new(token: impl Into<Cow<'a, str>>, tag: Tag) -> Self {
        Self {
            token: token.into(),
            tag,
        }
    }

    /// The token as a string slice.
    #[inline]
    #[must_use]
    pub fn token(&self) -> &str {
        &self.token
    }

    /// The tag.
    #[inline]
    #[must_use]
    pub const fn tag(&self) -> &Tag {
        &self.tag
    }

    /// Detaches the token from the input it borrows.
    #[must_use]
    pub fn into_owned(self) -> TaggedToken<'static> {
        TaggedToken {
            token: Cow::Owned(self.token.into_owned()),
            tag: self.tag,
        }
    }
}

impl fmt::Display for TaggedToken<'_> {
    /// `token/tag`, the notation used throughout this crate's documentation.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.token, self.tag)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_literal_contract_is_enforced_in_both_directions() {
        assert!(Tag::new("NN").is_ok());
        assert!(Tag::new("Adj(attr,stell,onverv)").is_ok());
        assert!(Word::new(",").is_ok());
        assert!(Word::new("www.example.com").is_ok());
        assert_eq!(Tag::new(""), Err(LiteralError::Empty));
        assert_eq!(Word::new(""), Err(LiteralError::Empty));
        assert_eq!(
            Tag::new("N N"),
            Err(LiteralError::Whitespace { found: ' ' })
        );
        assert_eq!(
            Word::new("a\tb"),
            Err(LiteralError::Whitespace { found: '\t' })
        );
        // Unicode White_Space beyond ASCII is rejected too: U+00A0 NO-BREAK
        // SPACE and U+3000 IDEOGRAPHIC SPACE both have the property.
        assert_eq!(
            Tag::new("a\u{00a0}b"),
            Err(LiteralError::Whitespace { found: '\u{00a0}' })
        );
        assert_eq!(
            Tag::new("a\u{3000}b"),
            Err(LiteralError::Whitespace { found: '\u{3000}' })
        );
        // U+FEFF ZERO WIDTH NO-BREAK SPACE does *not* have White_Space, so it is
        // accepted — the contract is the Unicode property, not an ad-hoc set.
        assert!(Tag::new("a\u{feff}b").is_ok());
    }

    #[test]
    fn literals_are_not_folded_trimmed_or_normalised() {
        assert_eq!(Tag::new("İstanbul").unwrap().as_str(), "İstanbul");
        assert_eq!(Word::new("Straße").unwrap().as_str(), "Straße");
        assert_eq!(Word::new("😀").unwrap().as_str(), "😀");
    }

    #[test]
    fn display_and_debug() {
        assert_eq!(Tag::new("NN").unwrap().to_string(), "NN");
        assert_eq!(format!("{:?}", Tag::new("NN").unwrap()), "Tag(\"NN\")");
        assert_eq!(format!("{:?}", Word::new("x").unwrap()), "Word(\"x\")");
        let t = TaggedToken::new("dog", Tag::new("NN").unwrap());
        assert_eq!(t.to_string(), "dog/NN");
    }

    #[test]
    fn tokens_detach_without_being_rewritten() {
        let owned = String::from("Ålesund");
        let t = TaggedToken::new(owned.as_str(), Tag::new("NNP").unwrap());
        let t = t.into_owned();
        assert_eq!(t.token(), "Ålesund");
        assert_eq!(t.tag().as_str(), "NNP");
    }

    #[test]
    fn from_str_and_try_from_agree_with_new() {
        assert_eq!("NN".parse::<Tag>().unwrap(), Tag::new("NN").unwrap());
        assert_eq!(Tag::try_from("NN").unwrap(), Tag::new("NN").unwrap());
        assert_eq!("".parse::<Word>(), Err(LiteralError::Empty));
    }
}
