//! The initial-state annotator. The token contract and the tokenizer coupling
//! it makes explicit are documented on [`Lexicon`] itself, since this module is
//! private and its own header would not reach the published documentation.

use std::collections::BTreeMap;
use std::fmt;

use crate::tag::{LiteralError, Tag};
use crate::text;

/// Why a lexicon entry was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum LexiconError {
    /// The key was not a conforming token.
    InvalidKey(LiteralError),
    /// The entry carried no tags. A tagless entry could only ever mean "this
    /// token exists but has no tag", which the initial-state annotator has no
    /// way to act on.
    NoTags {
        /// The key that was rejected.
        key: String,
    },
    /// A tag in the entry was not a conforming literal.
    InvalidTag(LiteralError),
}

impl fmt::Display for LexiconError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidKey(e) => write!(f, "invalid lexicon key: {e}"),
            Self::NoTags { key } => write!(f, "lexicon entry {key:?} has no tags"),
            Self::InvalidTag(e) => write!(f, "invalid tag in lexicon entry: {e}"),
        }
    }
}

impl std::error::Error for LexiconError {}

/// A token → tags dictionary, plus the defaults an unknown token takes.
///
/// # You supply it. This crate ships no dictionary.
///
/// There is no `Lexicon::bundled`, and there is no built-in English. Earlier
/// versions of this crate embedded English and Dutch dictionaries; they were
/// removed because their licences did not permit redistribution under this
/// crate's — `data/NOTICE.md` records exactly which files and why. What is left
/// is the Brill algorithm and two ways to feed it:
///
/// * [`Lexicon::new`] plus [`Lexicon::insert`], when the entries are yours to
///   write down;
/// * [`Corpus::parse_brown`](crate::Corpus::parse_brown) plus
///   [`Corpus::build_lexicon`](crate::Corpus::build_lexicon), when you have an
///   annotated corpus — the tag frequencies come out in the order
///   [`Lexicon::primary_tag`] wants, without you counting anything.
///
/// ```
/// use verbora_tagger::{Lexicon, Tag};
///
/// let mut lexicon = Lexicon::new(Tag::new("NN")?);
/// lexicon.insert("the", vec![Tag::new("DT")?])?;
/// lexicon.insert("book", vec![Tag::new("NN")?, Tag::new("VB")?])?;
///
/// assert_eq!(lexicon.tag_of("book").as_str(), "NN");   // most frequent first
/// assert_eq!(lexicon.tag_of("unheard-of").as_str(), "NN"); // the default
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
///
/// # The token contract, and the tokenizer coupling it makes explicit
///
/// A `Lexicon` key is a **token**: non-empty, and containing no scalar with the
/// Unicode `White_Space` property. [`Lexicon::insert`] rejects anything else.
///
/// That contract is this crate's half of a coupling that would otherwise go
/// unstated: **this crate never tokenizes**, so whatever produced the tokens
/// decides which keys can ever be hit. A dictionary keyed by the
/// whitespace-delimited tokens of a corpus has entries like `well-known`,
/// `A.A.U.` and `Asia/Pacific`, each a single key — and a producer that splits
/// inside them simply never reaches them. [UAX #29] word segmentation, which is
/// what `verbora_tokenizers::WordTokenizer` implements, splits every one of
/// those: U+002D HYPHEN-MINUS is `Word_Break=Other`, so `well-known` segments as
/// `["well", "known"]` and the compound entry is dead weight.
///
/// The rule that follows is simple, and it costs nothing if you follow it from
/// the start: **key the lexicon with the same producer that will tokenize the
/// text.** Building it with
/// [`Corpus::build_lexicon`](crate::Corpus::build_lexicon) from a corpus you
/// tokenized that way makes every key reachable by construction.
/// `tests/tokenization.rs` demonstrates both halves — the matched pair, and the
/// mismatch that silently loses entries.
///
/// # What it guarantees
///
/// * Every key is a conforming token, and every entry has **at least one** tag,
///   so [`Lexicon::primary_tag`] never has to invent one and never returns an
///   empty answer for a present key.
/// * Tags are ordered **most frequent first**, which is what makes
///   [`Lexicon::primary_tag`] the "most likely tag" annotator Brill (1995) §2
///   specifies. [`Lexicon::insert`] stores the order you give it; a lexicon
///   built by [`crate::Corpus::build_lexicon`] is sorted into it.
/// * A `Lexicon` owns its entries. Two lexicons never share state, and
///   [`Lexicon::insert`] on one is invisible to every other.
/// * Nothing here rewrites a token. The lowercase retry described on
///   [`Lexicon::tag_of`] changes what is *looked up*, never what is stored or
///   returned.
///
/// [UAX #29]: https://www.unicode.org/reports/tr29/
#[derive(Debug, Clone)]
pub struct Lexicon {
    /// The entries. Ordered, so iteration and `Debug` are deterministic.
    entries: BTreeMap<Box<str>, Box<[Tag]>>,
    default_tag: Tag,
    capitalized_default_tag: Tag,
    lowercase_retry: bool,
}

impl Lexicon {
    /// An empty lexicon in which every token takes `default_tag`.
    ///
    /// The capitalised default starts equal to `default_tag`; set it with
    /// [`Lexicon::with_capitalized_default_tag`].
    #[must_use]
    pub fn new(default_tag: Tag) -> Self {
        Self {
            entries: BTreeMap::new(),
            capitalized_default_tag: default_tag.clone(),
            default_tag,
            lowercase_retry: true,
        }
    }

    /// Replaces the tag unknown, uncapitalised tokens take.
    #[must_use]
    pub fn with_default_tag(mut self, tag: Tag) -> Self {
        self.default_tag = tag;
        self
    }

    /// Replaces the tag unknown, capitalised tokens take.
    ///
    /// "Capitalised" is the Unicode `Uppercase` property on the token's first
    /// scalar, so `Ålesund` and `Москва` count and `5`, `.` and `日本` do not.
    #[must_use]
    pub fn with_capitalized_default_tag(mut self, tag: Tag) -> Self {
        self.capitalized_default_tag = tag;
        self
    }

    /// Turns the lowercase retry described on [`Lexicon::tag_of`] on or off.
    ///
    /// On by default, because a sentence-initial `The` should find `the`.
    #[must_use]
    pub const fn with_lowercase_retry(mut self, on: bool) -> Self {
        self.lowercase_retry = on;
        self
    }

    /// The tag unknown, uncapitalised tokens take.
    #[inline]
    #[must_use]
    pub const fn default_tag(&self) -> &Tag {
        &self.default_tag
    }

    /// The tag unknown, capitalised tokens take.
    #[inline]
    #[must_use]
    pub const fn capitalized_default_tag(&self) -> &Tag {
        &self.capitalized_default_tag
    }

    /// Whether the lowercase retry is enabled.
    #[inline]
    #[must_use]
    pub const fn lowercase_retry(&self) -> bool {
        self.lowercase_retry
    }

    /// Adds or replaces one entry.
    ///
    /// Returns the tags the key previously carried, if it had an entry.
    ///
    /// # Errors
    ///
    /// [`LexiconError::InvalidKey`] when `key` is empty or contains whitespace,
    /// [`LexiconError::NoTags`] when `tags` is empty, and
    /// [`LexiconError::InvalidTag`] never in practice — a [`Tag`] is already
    /// checked — but the variant exists so the contract is stated in one place.
    pub fn insert(
        &mut self,
        key: &str,
        tags: Vec<Tag>,
    ) -> Result<Option<Box<[Tag]>>, LexiconError> {
        if key.is_empty() {
            return Err(LexiconError::InvalidKey(LiteralError::Empty));
        }
        if let Some(found) = key.chars().find(|c| c.is_whitespace()) {
            return Err(LexiconError::InvalidKey(LiteralError::Whitespace { found }));
        }
        if tags.is_empty() {
            return Err(LexiconError::NoTags {
                key: key.to_owned(),
            });
        }
        Ok(self.entries.insert(key.into(), tags.into_boxed_slice()))
    }

    /// Whether `key` is present, exactly as spelled.
    #[must_use]
    pub fn contains(&self, key: &str) -> bool {
        self.entries.contains_key(key)
    }

    /// The tags of `key`, most frequent first, or `None` when it is absent.
    ///
    /// The iterator is never empty when it is `Some`.
    #[must_use]
    pub fn tags(&self, key: &str) -> Option<Tags<'_>> {
        self.entries.get(key).map(|t| Tags(t.iter()))
    }

    /// The most frequent tag of `key`, or `None` when it is absent.
    ///
    /// This is the lookup [`Lexicon::tag_of`] is built on: one ordered-map probe
    /// and a clone of the tag, which for a tag built from a `&'static str`
    /// borrows rather than allocates.
    #[inline]
    #[must_use]
    pub fn primary_tag(&self, key: &str) -> Option<Tag> {
        self.entries.get(key)?.first().cloned()
    }

    /// The tag the initial-state annotator gives `token`.
    ///
    /// Total: every token gets a tag. The chain is, in order:
    ///
    /// 1. the most frequent tag of `token` exactly as spelled;
    /// 2. if [`Lexicon::lowercase_retry`] is on and step 1 missed, the most
    ///    frequent tag of `token.to_lowercase()` — the Unicode default full
    ///    lowercase mapping, which is why `İstanbul` is retried as
    ///    `i̇stanbul` and not as `istanbul`;
    /// 3. [`Lexicon::capitalized_default_tag`] when the token is capitalised;
    /// 4. [`Lexicon::default_tag`].
    ///
    /// Step 2 changes only what is *looked up*. `token` itself is returned to
    /// the caller unchanged by every API in this crate.
    ///
    /// A token that violates the key contract — empty, or containing
    /// whitespace — cannot match any entry, because no such entry can exist, so
    /// it takes a default. That is deliberate: rejecting it would make the whole
    /// tagging path fallible for an input shape a conforming tokenizer never
    /// produces.
    #[must_use]
    pub fn tag_of(&self, token: &str) -> Tag {
        if let Some(t) = self.primary_tag(token) {
            return t;
        }
        if self.lowercase_retry && !is_already_lowercase_ascii(token) {
            let lowered = token.to_lowercase();
            if let Some(t) = self.primary_tag(&lowered) {
                return t;
            }
        }
        if text::is_capitalized(token) {
            self.capitalized_default_tag.clone()
        } else {
            self.default_tag.clone()
        }
    }

    /// Number of entries.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the lexicon has no entries.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Every entry, in ascending key order, most frequent tag first.
    ///
    /// The order is by Unicode scalar value (equivalently, by UTF-8 bytes) and
    /// is stable across runs and platforms, so the output of a
    /// [`Corpus::build_lexicon`](crate::Corpus::build_lexicon) can be written to
    /// a file and diffed.
    pub fn entries(&self) -> Entries<'_> {
        Entries(self.entries.iter())
    }
}

/// Whether the lowercase retry can be skipped because it would be a no-op.
///
/// An all-ASCII token with no uppercase byte lowercases to itself, so the retry
/// would repeat the probe that just missed. Restricting the fast path to ASCII
/// is deliberate: `İ`, `ẞ` and the Greek final sigma all lowercase to something
/// other than themselves and must still be retried.
#[inline]
fn is_already_lowercase_ascii(token: &str) -> bool {
    token.is_ascii() && !token.bytes().any(|b| b.is_ascii_uppercase())
}

/// The tags of one entry, most frequent first. See [`Lexicon::tags`].
#[derive(Debug, Clone)]
pub struct Tags<'a>(std::slice::Iter<'a, Tag>);

impl Iterator for Tags<'_> {
    type Item = Tag;

    #[inline]
    fn next(&mut self) -> Option<Tag> {
        self.0.next().cloned()
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.0.size_hint()
    }
}

impl ExactSizeIterator for Tags<'_> {}

/// Every entry of a [`Lexicon`], in ascending key order. See
/// [`Lexicon::entries`].
#[derive(Debug)]
pub struct Entries<'a>(std::collections::btree_map::Iter<'a, Box<str>, Box<[Tag]>>);

impl<'a> Iterator for Entries<'a> {
    type Item = (&'a str, Tags<'a>);

    #[inline]
    fn next(&mut self) -> Option<(&'a str, Tags<'a>)> {
        self.0.next().map(|(k, v)| (&**k, Tags(v.iter())))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.0.size_hint()
    }
}

impl ExactSizeIterator for Entries<'_> {}

#[cfg(test)]
mod tests {
    use super::*;

    fn tag(s: &'static str) -> Tag {
        Tag::new(s).unwrap()
    }

    /// A small dictionary standing in for a real one.
    ///
    /// It is deliberately mixed: an entry with two tags so the frequency order
    /// is observable, a lowercase/capitalised pair so an exact hit can be shown
    /// to beat the lowercase retry, a key whose lowercase mapping is not its
    /// ASCII lowercasing, and a punctuation key.
    fn fixture() -> Lexicon {
        let mut l = Lexicon::new(tag("NN")).with_capitalized_default_tag(tag("NNP"));
        for (key, tags) in [
            ("the", vec!["DT"]),
            ("dog", vec!["NN"]),
            ("Dog", vec!["NNP"]),
            ("jumps", vec!["NNS"]),
            ("book", vec!["NN", "VB"]),
            ("i\u{0307}stanbul", vec!["NNP"]),
            (".", vec!["."]),
        ] {
            l.insert(key, tags.into_iter().map(tag).collect()).unwrap();
        }
        l
    }

    #[test]
    fn a_fresh_lexicon_is_empty_and_defaults_everything() {
        let l = Lexicon::new(tag("NN"));
        assert!(l.is_empty());
        assert_eq!(l.len(), 0);
        assert_eq!(l.entries().count(), 0);
        assert_eq!(l.default_tag(), &tag("NN"));
        assert_eq!(
            l.capitalized_default_tag(),
            &tag("NN"),
            "the capitalised default starts equal to the plain one"
        );
        assert_eq!(l.tag_of("anything"), tag("NN"));
        assert_eq!(l.tag_of("Anything"), tag("NN"));
    }

    #[test]
    fn the_entry_contract_is_enforced() {
        let mut l = Lexicon::new(tag("NN"));
        assert_eq!(
            l.insert("", vec![tag("NN")]),
            Err(LexiconError::InvalidKey(LiteralError::Empty))
        );
        assert_eq!(
            l.insert("a b", vec![tag("NN")]),
            Err(LexiconError::InvalidKey(LiteralError::Whitespace {
                found: ' '
            }))
        );
        assert_eq!(
            l.insert("dog", vec![]),
            Err(LexiconError::NoTags {
                key: "dog".to_owned()
            })
        );
        assert!(l.insert("dog", vec![tag("NN")]).unwrap().is_none());
        assert_eq!(
            l.insert("dog", vec![tag("VB")]).unwrap().as_deref(),
            Some(&[tag("NN")][..]),
            "the previous tags come back"
        );
        assert_eq!(l.len(), 1, "replacing a key is not a new key");
        assert_eq!(l.primary_tag("dog"), Some(tag("VB")));
    }

    /// The empty token cannot be an entry, so it takes a default rather than
    /// finding a tagless entry and coming back untagged.
    #[test]
    fn the_empty_token_takes_the_default() {
        let l = fixture();
        assert!(!l.contains(""));
        assert!(l.tags("").is_none());
        assert_eq!(l.tag_of(""), tag("NN"));
    }

    #[test]
    fn lexicons_are_independent() {
        let mut a = fixture();
        let b = fixture();
        a.insert("zzzprivate", vec![tag("XX")]).unwrap();
        assert_eq!(a.tag_of("zzzprivate"), tag("XX"));
        assert_eq!(b.tag_of("zzzprivate"), tag("NN"));
        assert_eq!(a.len(), b.len() + 1);
    }

    /// The four steps of [`Lexicon::tag_of`], each reached in turn.
    #[test]
    fn the_lookup_chain_runs_in_order() {
        let l = fixture();
        // 1. exact — and an exact hit always wins, even when a differently
        //    cased spelling of the same word exists.
        assert_eq!(l.tag_of("dog"), tag("NN"));
        assert_eq!(l.tag_of("Dog"), tag("NNP"));
        // 2. lowercase retry, for a capitalised form with no entry of its own.
        assert!(!l.contains("Jumps"));
        assert_eq!(l.tag_of("Jumps"), l.tag_of("jumps"));
        assert_eq!(l.tag_of("Jumps"), tag("NNS"));
        // 3. capitalised default
        assert_eq!(l.tag_of("Zzzzznotaword"), tag("NNP"));
        // 4. plain default
        assert_eq!(l.tag_of("zzzzznotaword"), tag("NN"));
        // The retry can be switched off, and then step 3 takes over.
        let strict = fixture().with_lowercase_retry(false);
        assert!(!strict.lowercase_retry());
        assert_eq!(strict.tag_of("Jumps"), tag("NNP"));
    }

    /// The retry uses the Unicode default full lowercase mapping, not an ASCII
    /// fold: `İ` lowercases to `i` + U+0307, which is how the entry is keyed.
    #[test]
    fn the_lowercase_retry_is_the_unicode_mapping() {
        let l = fixture();
        assert!(!l.contains("\u{130}stanbul"));
        assert_eq!(l.tag_of("\u{130}stanbul"), tag("NNP"));
        assert!(
            !l.contains("istanbul"),
            "the ASCII lowercasing is not a key, so a hit could only come from \
             the Unicode mapping"
        );
    }

    /// The capitalised default is the Unicode `Uppercase` property, not `A`–`Z`.
    #[test]
    fn capitalisation_default_is_unicode() {
        let l = fixture();
        for capitalised in ["Zzzzznotaword", "Ålesundzzz", "Москвазззз", "Ελλάςζζζ"]
        {
            assert_eq!(l.tag_of(capitalised), tag("NNP"), "{capitalised}");
        }
        for not in [
            "zzzzznotaword",
            "5zzzzz",
            ".zzzzz",
            "日本語ずずず",
            "😀zzzzz",
        ] {
            assert_eq!(l.tag_of(not), tag("NN"), "{not}");
        }
    }

    #[test]
    fn tags_are_most_frequent_first_and_never_empty() {
        let l = fixture();
        let tags: Vec<Tag> = l.tags("book").expect("an entry").collect();
        assert_eq!(tags, [tag("NN"), tag("VB")]);
        assert_eq!(l.primary_tag("book"), Some(tag("NN")));
        assert!(l.tags("no-such-word-at-all").is_none());
        assert_eq!(l.primary_tag("no-such-word-at-all"), None);
    }

    #[test]
    fn entries_are_in_ascending_key_order() {
        let mut l = Lexicon::new(tag("NN"));
        for (k, t) in [("b", "NN"), ("a", "DT"), ("c", "VB"), ("A", "NNP")] {
            l.insert(k, vec![tag(t)]).unwrap();
        }
        let keys: Vec<&str> = l.entries().map(|(k, _)| k).collect();
        assert_eq!(
            keys,
            ["A", "a", "b", "c"],
            "by Unicode scalar value, so uppercase sorts before lowercase"
        );
    }

    /// Every entry is reachable by its own key, through every accessor, and
    /// `entries()` emits each exactly once.
    #[test]
    fn every_entry_is_reachable_through_the_public_api() {
        let l = fixture();
        let mut seen = 0usize;
        for (key, tags) in l.entries() {
            seen += 1;
            let tags: Vec<Tag> = tags.collect();
            assert!(!tags.is_empty(), "{key:?} has no tags");
            assert!(l.contains(key), "{key:?} not found by contains");
            assert_eq!(l.primary_tag(key).as_ref(), tags.first(), "{key:?}");
            assert_eq!(l.tag_of(key), tags[0], "{key:?}");
        }
        assert_eq!(seen, l.len());
        assert_eq!(l.entries().len(), l.len(), "the size hint agrees");
    }

    /// The lowercase retry must not make any key unreachable: a key is always
    /// found by its own spelling, whatever case it is in. Enumerated over every
    /// key of the fixture that is not already all-lowercase — the population the
    /// retry can affect at all.
    #[test]
    fn the_lowercase_retry_never_shadows_an_exact_key() {
        let l = fixture();
        let mut mixed_case = 0usize;
        for (key, tags) in l.entries() {
            if key.to_lowercase() == key {
                continue;
            }
            mixed_case += 1;
            let first = tags.into_iter().next().expect("non-empty");
            assert_eq!(
                l.tag_of(key),
                first,
                "{key:?} lost its exact match to the lowercase retry"
            );
        }
        assert!(mixed_case > 0, "no mixed-case keys to check");
    }
}
