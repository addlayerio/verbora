//! Sentences and tagged words — the data every other module operates on.
//!
//! The reference's `Sentence` is `{ taggedWords: [{token, tag}] }`, and three
//! details of that shape are load-bearing:
//!
//! 1. **`tag` is an own property that may hold `undefined`.** A word the lexicon
//!    could not tag is `{token: 'x', tag: undefined}`, not `{token: 'x'}`. This
//!    matters because rules compare `tag === oldCategory` with `===`: a rule
//!    whose old category is itself `undefined` (the trainer builds those) matches
//!    an untagged word. `addTaggedWord` always creates the property, but a
//!    `Sentence` wrapped around caller-supplied data need not have it — and
//!    `clone()` then *adds* it, so `new Sentence([{token: 'a'}]).clone()` and its
//!    source disagree on `Object.keys`. [`Prop`] keeps the two apart.
//! 2. **`testTag` and `newTag` are genuinely absent** until the trainer writes
//!    them, and `JSON.stringify` shows the difference. [`Prop`] models all three
//!    states rather than flattening absent and undefined together.
//! 3. **`clone()` copies only `token` and `tag`.** Dropping `testTag`/`newTag` is
//!    exactly what makes `TransformationRule.applyAt` overwrite the gold tag; a
//!    "helpful" full clone changes every training run.

use std::borrow::Cow;

/// A tag string.
///
/// Borrowed from the bundled tables in the common case — every tag the shipped
/// English and Dutch lexicons and rule sets produce is a `&'static str` embedded
/// in the binary, so tagging a document allocates nothing for its tags. Only
/// tags that came from user-supplied data are owned.
pub type Tag = Cow<'static, str>;

/// A reference object property with three distinguishable states.
///
/// `Option<Option<T>>` would do the job, but reads terribly at every call site;
/// this spells out which state is which.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Prop {
    /// The property does not exist on the object. `JSON.stringify` omits it, and
    /// `Object.keys` does not list it.
    #[default]
    Absent,
    /// The property exists and holds `undefined`.
    Undefined,
    /// The property exists and holds a string.
    Set(Tag),
}

impl Prop {
    /// The stored string, if any. Absent and `undefined` are both `None` —
    /// which is what `===` against a string sees.
    #[inline]
    #[must_use]
    pub fn value(&self) -> Option<&str> {
        match self {
            Self::Set(s) => Some(s),
            _ => None,
        }
    }

    /// Whether the property exists at all, regardless of its value.
    #[inline]
    #[must_use]
    pub const fn is_present(&self) -> bool {
        !matches!(self, Self::Absent)
    }

    /// Builds a property from an optional value, treating `None` as *present and
    /// `undefined`* — the state an assignment `obj.p = maybeUndefined` produces.
    #[inline]
    #[must_use]
    pub fn assigned(v: Option<Tag>) -> Self {
        match v {
            Some(s) => Self::Set(s),
            None => Self::Undefined,
        }
    }

    /// The stored string as an owned [`Tag`], if any.
    ///
    /// Cheap: [`Tag`] is a `Cow`, and every tag the bundled tables produce is a
    /// `&'static str`, so cloning one copies a pointer pair.
    #[inline]
    #[must_use]
    pub fn owned(&self) -> Option<Tag> {
        match self {
            Self::Set(s) => Some(s.clone()),
            _ => None,
        }
    }
}

/// One token and the tags attached to it.
///
/// The token borrows from the caller's input for the lifetime `'a`, so tagging a
/// slice of `&str` copies no text at all. Use [`TaggedWord::into_owned`] to
/// detach.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaggedWord<'a> {
    /// The word itself. Never modified by any operation in this crate.
    pub token: Cow<'a, str>,
    /// Its part-of-speech tag.
    ///
    /// [`Prop::Undefined`] and [`Prop::Absent`] behave identically under every
    /// `===` the reference performs; they differ only in `Object.keys`, which
    /// is why [`TaggedWord::untagged`] exists alongside
    /// [`TaggedWord::new`]`(_, None)`.
    pub tag: Prop,
    /// The trainer's *hypothesis* tag, written by `Corpus::tag`.
    pub test_tag: Prop,
    /// The tag a rule *would* assign, written by
    /// [`crate::TransformationRule::is_applicable_at`].
    pub new_tag: Prop,
}

impl<'a> TaggedWord<'a> {
    /// A word with a token and a tag, matching `addTaggedWord(token, tag)` —
    /// which assigns `tag` unconditionally, so the property always exists.
    #[inline]
    pub fn new(token: impl Into<Cow<'a, str>>, tag: Option<Tag>) -> Self {
        Self {
            token: token.into(),
            tag: Prop::assigned(tag),
            test_tag: Prop::Absent,
            new_tag: Prop::Absent,
        }
    }

    /// A word with **no `tag` property at all**, as in `{token: 'a'}`.
    ///
    /// Only reachable by handing raw data to [`Sentence::from_words`]; nothing
    /// in the reference produces one. It survives here because
    /// `Sentence::clone` turns it into an explicit `tag: undefined`, and the
    /// recorded fixture asserts on the difference.
    #[inline]
    pub fn untagged(token: impl Into<Cow<'a, str>>) -> Self {
        Self {
            token: token.into(),
            tag: Prop::Absent,
            test_tag: Prop::Absent,
            new_tag: Prop::Absent,
        }
    }

    /// The tag as a `&str`, or `None` for `undefined` — and for absent, which
    /// `===` cannot tell apart from `undefined`.
    #[inline]
    #[must_use]
    pub fn tag(&self) -> Option<&str> {
        self.tag.value()
    }

    /// Detaches the token from the input it borrows.
    #[must_use]
    pub fn into_owned(self) -> TaggedWord<'static> {
        TaggedWord {
            token: Cow::Owned(self.token.into_owned()),
            tag: self.tag,
            test_tag: self.test_tag,
            new_tag: self.new_tag,
        }
    }
}

/// A sentence: an ordered list of tagged words.
///
/// Constructed empty and grown with [`Sentence::add_tagged_word`], or wrapped
/// around existing words with [`Sentence::from_words`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Sentence<'a> {
    /// The words, in order.
    pub tagged_words: Vec<TaggedWord<'a>>,
}

impl<'a> Sentence<'a> {
    /// An empty sentence — `new Sentence()`.
    #[inline]
    #[must_use]
    pub const fn new() -> Self {
        Self {
            tagged_words: Vec::new(),
        }
    }

    /// Wraps an existing word list — `new Sentence(data)`.
    ///
    /// The reference stores the array *by reference*, so mutating the corpus
    /// mutates the caller's JSON. Rust's ownership rules make that impossible;
    /// see the aliasing note in the crate documentation.
    #[inline]
    #[must_use]
    pub const fn from_words(tagged_words: Vec<TaggedWord<'a>>) -> Self {
        Self { tagged_words }
    }

    /// Appends `{token, tag}`.
    #[inline]
    pub fn add_tagged_word(&mut self, token: impl Into<Cow<'a, str>>, tag: Option<Tag>) {
        self.tagged_words.push(TaggedWord::new(token, tag));
    }

    /// Number of words.
    #[inline]
    #[must_use]
    pub fn len(&self) -> usize {
        self.tagged_words.len()
    }

    /// Whether the sentence has no words.
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tagged_words.is_empty()
    }

    /// Copies **only** `token` and `tag`, dropping `testTag` and `newTag`.
    ///
    /// The dropping is deliberate and required: `applyAt` clones a sentence,
    /// mutates the original, then reads the clone's `tag` back into the
    /// original's `testTag`. A clone that preserved every field would make that
    /// sequence a no-op and change the trainer's output.
    ///
    /// The copy goes through `addTaggedWord`, so a word whose `tag` property was
    /// *absent* comes back with an explicit `tag: undefined`.
    #[must_use]
    pub fn clone_tags_only(&self) -> Sentence<'a> {
        Sentence {
            tagged_words: self
                .tagged_words
                .iter()
                .map(|w| TaggedWord::new(w.token.clone(), w.tag.owned()))
                .collect(),
        }
    }

    /// Detaches every token from the input it borrows.
    #[must_use]
    pub fn into_owned(self) -> Sentence<'static> {
        Sentence {
            tagged_words: self
                .tagged_words
                .into_iter()
                .map(TaggedWord::into_owned)
                .collect(),
        }
    }

    /// The `(token, tag)` pairs, for callers who want the flat shape.
    pub fn pairs(&self) -> impl Iterator<Item = (&str, Option<&str>)> {
        self.tagged_words.iter().map(|w| (&*w.token, w.tag()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clone_drops_training_fields() {
        let mut s = Sentence::new();
        s.add_tagged_word("book", Some(Cow::Borrowed("NN")));
        s.tagged_words[0].test_tag = Prop::Set(Cow::Borrowed("VB"));
        s.tagged_words[0].new_tag = Prop::Set(Cow::Borrowed("JJ"));

        let c = s.clone_tags_only();
        assert_eq!(c.tagged_words[0].tag(), Some("NN"));
        assert_eq!(c.tagged_words[0].test_tag, Prop::Absent);
        assert_eq!(c.tagged_words[0].new_tag, Prop::Absent);
    }

    #[test]
    fn prop_distinguishes_absent_from_undefined() {
        assert!(!Prop::Absent.is_present());
        assert!(Prop::Undefined.is_present());
        assert_eq!(Prop::Undefined.value(), None);
        assert_eq!(Prop::Absent.value(), None);
        assert_eq!(Prop::assigned(None), Prop::Undefined);
        assert_eq!(
            Prop::Set(Cow::Borrowed("NN")).owned().as_deref(),
            Some("NN")
        );
        assert_eq!(Prop::Absent.owned(), None);
    }

    #[test]
    fn cloning_materialises_an_absent_tag() {
        // `new Sentence([{token: 'a'}]).clone()` yields `{token:'a', tag:undefined}`:
        // Object.keys differs between the two, and the fixture records it.
        let s = Sentence::from_words(vec![TaggedWord::untagged("a")]);
        assert_eq!(s.tagged_words[0].tag, Prop::Absent);
        assert_eq!(s.tagged_words[0].tag(), None);
        let c = s.clone_tags_only();
        assert_eq!(c.tagged_words[0].tag, Prop::Undefined);
    }

    #[test]
    fn empty_sentence() {
        let s = Sentence::new();
        assert!(s.is_empty());
        assert_eq!(s.len(), 0);
        assert_eq!(s.pairs().count(), 0);
    }
}
