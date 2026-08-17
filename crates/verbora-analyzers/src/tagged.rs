//! The POS-tagged input `SentenceAnalyzer` consumes, and its `punct()` accessor.
//!
//! The reference passes a plain object literal:
//!
//! ```text
//! { tags: [{ token: 'The', pos: 'DT' }, …], punct: () => [] }
//! ```
//!
//! [`TaggedWord`] and [`TaggedSentence`] model that shape closely enough that
//! the analyzer's *destructive* behaviour — it annotates the caller's tags in
//! place, appends to the array, and pops from it — stays visible rather than
//! being tidied away behind a pure function.

use std::borrow::Cow;
use std::fmt;

/// One of the five keys a tag object can carry.
///
/// The reference objects remember key-insertion order and `Object.keys` exposes
/// it, so a faithful port has to as well: the tag the analyzer appends is
/// written `{ token, spos, pos, added }`, with `pos` *third*, while every tag a
/// caller writes starts `{ token, pos, … }`. See [`TaggedWord::keys`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Field {
    /// `token` — the surface word.
    Token,
    /// `pos` — the part-of-speech tag.
    Pos,
    /// `pp` — set by `prepositionPhrases`, marking a prepositional phrase.
    Pp,
    /// `spos` — set by `part`, marking subject (`SP`) or predicate (`PP`).
    Spos,
    /// `added` — set only on the implicit-'You' tag the analyzer appends.
    Added,
}

impl Field {
    /// The reference property name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Token => "token",
            Self::Pos => "pos",
            Self::Pp => "pp",
            Self::Spos => "spos",
            Self::Added => "added",
        }
    }
}

impl fmt::Display for Field {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The key-insertion order of one tag object.
///
/// Five slots is the exact capacity needed, so this is a `Copy` six-byte value
/// with no allocation — the whole point of tracking key order is that it must
/// not cost anything on the hot path through `part()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct KeyOrder {
    slots: [Field; 5],
    len: u8,
}

impl KeyOrder {
    /// The order of a tag written as `{ token, pos }`.
    const fn token_pos() -> Self {
        Self {
            slots: [Field::Token, Field::Pos, Field::Pos, Field::Pos, Field::Pos],
            len: 2,
        }
    }

    /// The order of a tag written as `{ token }`, before the rest is filled in.
    const fn token_only() -> Self {
        Self {
            slots: [
                Field::Token,
                Field::Token,
                Field::Token,
                Field::Token,
                Field::Token,
            ],
            len: 1,
        }
    }

    fn as_slice(&self) -> &[Field] {
        &self.slots[..self.len as usize]
    }

    /// Records an assignment. Re-assigning an existing key does not move it,
    /// which is exactly what the reference does.
    fn insert(&mut self, field: Field) {
        if self.as_slice().contains(&field) {
            return;
        }
        let i = self.len as usize;
        debug_assert!(i < self.slots.len(), "a tag has at most five keys");
        self.slots[i] = field;
        self.len += 1;
    }
}

/// One POS-tagged word: the reference's `TaggedWord`.
///
/// ```
/// use verbora_analyzers::TaggedWord;
///
/// let w = TaggedWord::new("bear", "NN");
/// assert_eq!(w.token(), "bear");
/// assert_eq!(w.pos(), "NN");
/// assert_eq!(w.spos(), None);
/// ```
///
/// # Borrowed by default
///
/// `token`, `pos` and `spos` are [`Cow`]s, so tags built from a tagger's output
/// borrow it and the analyzer's own writes (`"SP"`, `"PP"`, the `"You"` tag)
/// are `&'static str` literals. Annotating a whole sentence therefore allocates
/// nothing beyond the tag vector itself.
///
/// # What is *not* modelled
///
/// The shipped `index.d.ts` declares `[key: string]: any`, so a reference tag
/// may carry arbitrary extra properties. Nothing in the reference reads or writes
/// any key other than the five above, so they are not represented here; see the
/// crate-level documentation for the full list of deliberate divergences.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaggedWord<'a> {
    token: Cow<'a, str>,
    pos: Cow<'a, str>,
    pp: Option<bool>,
    spos: Option<Cow<'a, str>>,
    added: Option<bool>,
    order: KeyOrder,
}

impl<'a> TaggedWord<'a> {
    /// A tag written `{ token, pos }`, the shape every the reference caller uses.
    pub fn new(token: impl Into<Cow<'a, str>>, pos: impl Into<Cow<'a, str>>) -> Self {
        Self {
            token: token.into(),
            pos: pos.into(),
            pp: None,
            spos: None,
            added: None,
            order: KeyOrder::token_pos(),
        }
    }

    /// The implicit-'You' subject `part()` appends to a verb-initial sentence.
    ///
    /// The reference writes it as `{ token: 'You', spos: 'SP', pos: 'PRP', added:
    /// true }` — note that `pos` is the *third* key, unlike every other tag.
    #[must_use]
    pub fn implicit_you() -> TaggedWord<'static> {
        let mut w = TaggedWord {
            token: Cow::Borrowed("You"),
            pos: Cow::Borrowed(""),
            pp: None,
            spos: None,
            added: None,
            order: KeyOrder::token_only(),
        };
        // Assigned in the reference's own order so `keys()` reports it faithfully.
        w.set_spos("SP");
        w.set_pos("PRP");
        w.set_added(true);
        w
    }

    /// Builder form of [`Self::set_pp`].
    #[must_use]
    pub fn with_pp(mut self, pp: bool) -> Self {
        self.set_pp(pp);
        self
    }

    /// Builder form of [`Self::set_spos`].
    #[must_use]
    pub fn with_spos(mut self, spos: impl Into<Cow<'a, str>>) -> Self {
        self.set_spos(spos);
        self
    }

    /// Builder form of [`Self::set_added`].
    #[must_use]
    pub fn with_added(mut self, added: bool) -> Self {
        self.set_added(added);
        self
    }

    /// The surface word.
    #[must_use]
    pub fn token(&self) -> &str {
        &self.token
    }

    /// The part-of-speech tag.
    #[must_use]
    pub fn pos(&self) -> &str {
        &self.pos
    }

    /// The `pp` flag: `None` when the key is absent.
    #[must_use]
    pub fn pp(&self) -> Option<bool> {
        self.pp
    }

    /// The `spos` annotation: `None` when the key is absent.
    #[must_use]
    pub fn spos(&self) -> Option<&str> {
        self.spos.as_deref()
    }

    /// The `added` flag: `None` when the key is absent.
    #[must_use]
    pub fn added(&self) -> Option<bool> {
        self.added
    }

    /// Sets `token`.
    pub fn set_token(&mut self, token: impl Into<Cow<'a, str>>) -> &mut Self {
        self.token = token.into();
        self.order.insert(Field::Token);
        self
    }

    /// Sets `pos`.
    pub fn set_pos(&mut self, pos: impl Into<Cow<'a, str>>) -> &mut Self {
        self.pos = pos.into();
        self.order.insert(Field::Pos);
        self
    }

    /// Sets `pp`.
    pub fn set_pp(&mut self, pp: bool) -> &mut Self {
        self.pp = Some(pp);
        self.order.insert(Field::Pp);
        self
    }

    /// Sets `spos`.
    pub fn set_spos(&mut self, spos: impl Into<Cow<'a, str>>) -> &mut Self {
        self.spos = Some(spos.into());
        self.order.insert(Field::Spos);
        self
    }

    /// Sets `added`.
    pub fn set_added(&mut self, added: bool) -> &mut Self {
        self.added = Some(added);
        self.order.insert(Field::Added);
        self
    }

    /// Whether this tag is inside a prepositional phrase.
    ///
    /// The reference test is `tags[i].pp !== true`, a *strict* comparison: a
    /// tag carrying `pp: false` is still annotated with `spos`. Writing this as
    /// "is `pp` truthy" would get that case wrong.
    #[must_use]
    pub fn is_pp(&self) -> bool {
        self.pp == Some(true)
    }

    /// Whether this tag is the analyzer's implicit-'You'.
    ///
    /// `implicitYou()` tests `if (tags[i].added)` — truthiness, not equality —
    /// which for the `boolean | undefined` this type models is the same thing.
    #[must_use]
    pub fn is_added(&self) -> bool {
        self.added == Some(true)
    }

    /// The property names present on this tag, in the reference key-insertion
    /// order.
    ///
    /// ```
    /// use verbora_analyzers::{Field, TaggedWord};
    ///
    /// let plain = TaggedWord::new("bear", "NN");
    /// assert_eq!(plain.keys().collect::<Vec<_>>(), [Field::Token, Field::Pos]);
    ///
    /// // The appended subject puts `pos` third — the reference's literal order.
    /// let you = TaggedWord::implicit_you();
    /// assert_eq!(
    ///     you.keys().map(Field::as_str).collect::<Vec<_>>(),
    ///     ["token", "spos", "pos", "added"]
    /// );
    /// ```
    pub fn keys(&self) -> impl ExactSizeIterator<Item = Field> + '_ {
        self.order.as_slice().iter().copied()
    }

    /// Detaches the tag from the text it borrows.
    #[must_use]
    pub fn into_owned(self) -> TaggedWord<'static> {
        TaggedWord {
            token: Cow::Owned(self.token.into_owned()),
            pos: Cow::Owned(self.pos.into_owned()),
            pp: self.pp,
            spos: self.spos.map(|s| Cow::Owned(s.into_owned())),
            added: self.added,
            order: self.order,
        }
    }
}

/// What `posObj.punct()` handed back.
///
/// The declared the reference return type is `Array<Record<string, string>> | ''`,
/// and `type()` branches on `.length !== 0` — which both an empty array and the
/// empty string satisfy. The two are therefore *indistinguishable* to the
/// analyzer; both variants exist so that porting a `punct` written either way
/// is a direct translation rather than a judgement call.
///
/// The other things a reference `punct` can return — `null`, `undefined`, or a
/// non-empty string — all throw before any classification happens, so they have
/// no representation here. See [`SentenceAnalyzer::type_of`] for the list.
///
/// [`SentenceAnalyzer::type_of`]: crate::SentenceAnalyzer::type_of
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Punct<'a> {
    /// The array form. May be empty.
    List(Vec<TaggedWord<'a>>),
    /// The `''` form. Behaves exactly like an empty [`Self::List`].
    EmptyString,
}

impl Punct<'_> {
    /// Whether `.length === 0`, the test `type()` actually performs.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        match self {
            Self::List(marks) => marks.is_empty(),
            Self::EmptyString => true,
        }
    }
}

impl Default for Punct<'_> {
    /// An empty list — the `function () { return [] }` used throughout
    /// The reference's own specs.
    fn default() -> Self {
        Self::List(Vec::new())
    }
}

/// The type of the `punct` accessor an analyzer built with
/// [`SentenceAnalyzer::new`] carries.
///
/// [`SentenceAnalyzer::new`]: crate::SentenceAnalyzer::new
pub type NoPunct<'a> = fn() -> Punct<'a>;

/// A `punct()` that always reports no punctuation.
#[must_use]
pub fn no_punct<'a>() -> Punct<'a> {
    Punct::List(Vec::new())
}

/// The reference's `posObj`: the tag array plus its `punct()` accessor.
///
/// `punct` is a *function* rather than a value because that is what the
/// reference calls, once per `type()`. Modelling it as `FnMut` keeps callers
/// able to reproduce a `punct` with state — including the aliasing case where
/// the reference's `pop()` shrinks an array the closure shares.
pub struct TaggedSentence<'a, P> {
    /// The tags. Public because every analyzer method mutates them in place and
    /// callers are expected to inspect the result.
    pub tags: Vec<TaggedWord<'a>>,
    punct: P,
}

impl<'a, P> TaggedSentence<'a, P> {
    /// Builds a sentence from tags and a `punct` accessor.
    pub fn new(tags: Vec<TaggedWord<'a>>, punct: P) -> Self {
        Self { tags, punct }
    }

    /// Consumes the sentence, returning its tags.
    #[must_use]
    pub fn into_tags(self) -> Vec<TaggedWord<'a>> {
        self.tags
    }
}

impl<'a, P: FnMut() -> Punct<'a>> TaggedSentence<'a, P> {
    /// Calls the `punct` accessor.
    pub fn punct(&mut self) -> Punct<'a> {
        (self.punct)()
    }
}

impl<P> fmt::Debug for TaggedSentence<'_, P> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // `P` is a closure and cannot be printed; naming it keeps the output
        // honest about the field existing.
        f.debug_struct("TaggedSentence")
            .field("tags", &self.tags)
            .field("punct", &"<fn>")
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_order_follows_assignment() {
        let mut w = TaggedWord::new("a", "NN");
        assert_eq!(w.keys().collect::<Vec<_>>(), [Field::Token, Field::Pos]);
        w.set_spos("SP");
        w.set_pp(true);
        assert_eq!(
            w.keys().collect::<Vec<_>>(),
            [Field::Token, Field::Pos, Field::Spos, Field::Pp]
        );
        // Re-assignment does not move a key.
        w.set_spos("PP");
        assert_eq!(
            w.keys().collect::<Vec<_>>(),
            [Field::Token, Field::Pos, Field::Spos, Field::Pp]
        );
    }

    #[test]
    fn implicit_you_matches_the_reference_literal() {
        let you = TaggedWord::implicit_you();
        assert_eq!(you.token(), "You");
        assert_eq!(you.pos(), "PRP");
        assert_eq!(you.spos(), Some("SP"));
        assert_eq!(you.added(), Some(true));
        assert!(you.is_added());
        assert_eq!(
            you.keys().map(Field::as_str).collect::<Vec<_>>(),
            ["token", "spos", "pos", "added"]
        );
    }

    #[test]
    fn pp_false_is_not_pp() {
        // `pp !== true`: a tag carrying `pp: false` still gets annotated.
        assert!(!TaggedWord::new("a", "NN").with_pp(false).is_pp());
        assert!(TaggedWord::new("a", "NN").with_pp(true).is_pp());
        assert!(!TaggedWord::new("a", "NN").is_pp());
    }

    #[test]
    fn empty_string_punct_is_empty() {
        assert!(Punct::EmptyString.is_empty());
        assert!(Punct::List(Vec::new()).is_empty());
        assert!(!Punct::List(vec![TaggedWord::new(".", ".")]).is_empty());
    }

    #[test]
    fn into_owned_detaches() {
        let text = String::from("borrowed");
        let w = TaggedWord::new(text.as_str(), "NN").with_spos("SP");
        let owned: TaggedWord<'static> = w.into_owned();
        drop(text);
        assert_eq!(owned.token(), "borrowed");
        assert_eq!(owned.spos(), Some("SP"));
    }
}
