use std::borrow::Cow;
use std::fmt;

use crate::tag::TagClass;

/// One part-of-speech-tagged word: a token and the tag a tagger assigned it.
///
/// This is the analyzer's input unit, and it is **opaque**. Nothing in this
/// crate splits a token into characters, folds its case, trims it, normalises
/// it, or reads inside it — with exactly one exception, spelled out under
/// *Text units* in the [crate documentation](crate). A token may therefore be
/// anything: a word, a punctuation mark, an emoji, the empty string.
///
/// ```
/// use verbora_analyzers::{TagClass, TaggedWord};
///
/// let word = TaggedWord::new("bear", "NN");
/// assert_eq!(word.token(), "bear");
/// assert_eq!(word.tag(), "NN");
/// assert_eq!(word.tag_class(), TagClass::Noun);
/// ```
///
/// # Borrowed by default
///
/// Both fields are [`Cow`], so a sentence built from a tagger's output borrows
/// it and building the input costs one `Vec` and no string copies. Use
/// [`TaggedWord::into_owned`] to detach a word from the text it borrows.
///
/// ```
/// use verbora_analyzers::TaggedWord;
///
/// let text = String::from("squirrel");
/// let borrowed = TaggedWord::new(text.as_str(), "NN");
/// let owned: TaggedWord<'static> = borrowed.into_owned();
/// drop(text);
/// assert_eq!(owned.token(), "squirrel");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TaggedWord<'a> {
    token: Cow<'a, str>,
    tag: Cow<'a, str>,
}

impl<'a> TaggedWord<'a> {
    /// Builds a tagged word.
    ///
    /// Neither argument is validated or rewritten: an unrecognised tag is
    /// [`TagClass::Other`] and takes part in no rule, rather than being an
    /// error. See [`TagClass`] for the tag set this crate specifies.
    pub fn new(token: impl Into<Cow<'a, str>>, tag: impl Into<Cow<'a, str>>) -> Self {
        Self {
            token: token.into(),
            tag: tag.into(),
        }
    }

    /// The surface token, byte for byte as it was supplied.
    #[must_use]
    pub fn token(&self) -> &str {
        &self.token
    }

    /// The part-of-speech tag, byte for byte as it was supplied.
    #[must_use]
    pub fn tag(&self) -> &str {
        &self.tag
    }

    /// The class [`Self::tag`] falls into. Shorthand for
    /// `TagClass::of(word.tag())`.
    #[must_use]
    pub fn tag_class(&self) -> TagClass {
        TagClass::of(&self.tag)
    }

    /// Detaches the word from the text it borrows, cloning both fields.
    #[must_use]
    pub fn into_owned(self) -> TaggedWord<'static> {
        TaggedWord {
            token: Cow::Owned(self.token.into_owned()),
            tag: Cow::Owned(self.tag.into_owned()),
        }
    }
}

/// Renders `token/tag`, the conventional inline notation for a tagged word.
///
/// The separator is a literal `/`, and neither field is escaped, so a token
/// containing `/` renders ambiguously. This is a display form, not a
/// serialization format.
impl fmt::Display for TaggedWord<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.token, self.tag)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fields_are_returned_byte_for_byte() {
        // Astral scalars, combining marks, a zero-width no-break space, the
        // empty string: a token is never re-indexed, so every one must survive.
        for token in [
            "",
            " ",
            "café",
            "cafe\u{0301}",
            "Москва",
            "日本語",
            "😀",
            "a😀b",
            "\u{feff}",
            "a/b",
        ] {
            let word = TaggedWord::new(token, "NN");
            assert_eq!(word.token(), token, "{token:?}");
            assert_eq!(word.token().as_bytes(), token.as_bytes(), "{token:?}");
            assert_eq!(word.clone().into_owned().token(), token, "{token:?}");
        }
    }

    #[test]
    fn a_tag_is_never_rewritten() {
        for tag in ["", " NN", "nn", "NN ", "NN|IN", "😀"] {
            assert_eq!(TaggedWord::new("x", tag).tag(), tag, "{tag:?}");
        }
    }

    #[test]
    fn display_is_token_slash_tag() {
        assert_eq!(TaggedWord::new("bear", "NN").to_string(), "bear/NN");
        assert_eq!(TaggedWord::new("", "").to_string(), "/");
    }

    #[test]
    fn ordering_is_by_token_then_tag() {
        let mut words = [
            TaggedWord::new("b", "NN"),
            TaggedWord::new("a", "VB"),
            TaggedWord::new("a", "NN"),
        ];
        words.sort();
        let rendered: Vec<String> = words.iter().map(ToString::to_string).collect();
        assert_eq!(rendered, ["a/NN", "a/VB", "b/NN"]);
    }
}
