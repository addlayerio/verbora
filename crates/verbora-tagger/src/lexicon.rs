//! The initial-state annotator. The token contract and the tokenizer coupling
//! it makes explicit are documented on [`Lexicon`] itself, since this module is
//! private and its own header would not reach the published documentation.

use std::collections::BTreeMap;
use std::fmt;

use crate::data::{StaticLexicon, StaticTags};
use crate::language::Language;
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
    /// way to act on; the bundled English `""` entry was exactly that.
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
/// # The token contract, and the tokenizer coupling it makes explicit
///
/// A `Lexicon` key is a **token**: non-empty, and containing no scalar with the
/// Unicode `White_Space` property. [`Lexicon::insert`] rejects anything else,
/// and the crate's build script holds the bundled JSON to exactly the same rule.
///
/// That contract is this crate's half of a coupling that would otherwise go
/// unstated: **this crate never tokenizes**, so whatever produced the tokens
/// decides which keys can ever be hit. Verbora's bundled dictionaries are keyed
/// by the whitespace-delimited tokens of the corpora they were derived from —
/// `well-known`, `A.A.U.`, `%CHG` and `Asia/Pacific` are each a single key — so a
/// producer that splits inside those keys simply never reaches them.
///
/// This is measured, not asserted. Feeding every bundled key through [UAX #29]
/// word segmentation, which is what `verbora_tokenizers::WordTokenizer`
/// implements:
///
/// | Lexicon | keys | never emitted whole by a UAX #29 word tokenizer | share |
/// |---|---|---|---|
/// | English | 92,538 | 15,543 | 16.8% |
/// | Dutch | 11,699 | 313 | 2.7% |
///
/// The English figure breaks down as: 14,417 keys containing U+002D
/// HYPHEN-MINUS, which is `Word_Break=Other`, so `well-known` segments as
/// `["well", "known"]`; 864 containing a full stop (`A.A.U.`); 151 containing
/// `/` (`Asia/Pacific`); 49 containing `&`; 9 containing `$`; 34 that are pure
/// punctuation and are dropped entirely; and 19 others. `tests/tokenization.rs`
/// walks **every** bundled key, pins those counts and pins the breakdown, so a
/// change on either side breaks a test instead of quietly costing one key in six.
///
/// The guidance follows from the numbers:
///
/// * with the **bundled** dictionaries, split on whitespace —
///   `str::split_whitespace` yields exactly conforming tokens and reaches every
///   key;
/// * with a **UAX #29** tokenizer, build the lexicon from a corpus tokenized the
///   same way, with [`Corpus::build_lexicon`](crate::Corpus::build_lexicon).
///   Every key it produces is then reachable by construction.
///
/// # What it guarantees
///
/// * Every key is a conforming token, and every entry has **at least one** tag,
///   so [`Lexicon::primary_tag`] never has to invent one and never returns an
///   empty answer for a present key.
/// * Tags are ordered **most frequent first**, which is what makes
///   [`Lexicon::primary_tag`] the "most likely tag" annotator Brill (1995) §2
///   specifies. The bundled data is stored in that order; a lexicon built by
///   [`crate::Corpus::build_lexicon`] is sorted into it.
/// * A `Lexicon` owns its entries. Two lexicons never share state, and
///   [`Lexicon::insert`] on one is invisible to every other.
/// * Nothing here rewrites a token. The lowercase retry described on
///   [`Lexicon::tag_of`] changes what is *looked up*, never what is stored or
///   returned.
///
/// [UAX #29]: https://www.unicode.org/reports/tr29/
#[derive(Debug, Clone)]
pub struct Lexicon {
    /// The packed dictionary this lexicon started from, if any.
    base: Option<StaticLexicon>,
    /// Entries added or overridden at runtime. Ordered, so iteration and
    /// `Debug` are deterministic.
    overlay: BTreeMap<Box<str>, Box<[Tag]>>,
    /// How many overlay keys are not also base keys.
    overlay_new: usize,
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
            base: None,
            overlay: BTreeMap::new(),
            overlay_new: 0,
            capitalized_default_tag: default_tag.clone(),
            default_tag,
            lowercase_retry: true,
        }
    }

    /// The bundled dictionary for `language`, with that language's documented
    /// defaults (see [`Language::default_tag`]).
    ///
    /// Costs nothing at construction: the entries are read in place from bytes
    /// embedded in the executable, so there is no parse step and no lazily
    /// initialised table.
    #[must_use]
    pub fn bundled(language: Language) -> Self {
        Self {
            base: Some(language.lexicon()),
            overlay: BTreeMap::new(),
            overlay_new: 0,
            default_tag: language.default_tag(),
            capitalized_default_tag: language.capitalized_default_tag(),
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
    /// Returns the tags the key previously carried, if it had an overlay entry.
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
        let shadows_base = self.base.is_some_and(|b| b.find(key).is_some());
        let previous = self.overlay.insert(key.into(), tags.into_boxed_slice());
        if previous.is_none() && !shadows_base {
            self.overlay_new += 1;
        }
        Ok(previous)
    }

    /// Whether `key` is present, exactly as spelled.
    #[must_use]
    pub fn contains(&self, key: &str) -> bool {
        self.overlay.contains_key(key) || self.base.is_some_and(|b| b.find(key).is_some())
    }

    /// The tags of `key`, most frequent first, or `None` when it is absent.
    ///
    /// The iterator is never empty when it is `Some`.
    #[must_use]
    pub fn tags(&self, key: &str) -> Option<Tags<'_>> {
        if let Some(t) = self.overlay.get(key) {
            return Some(Tags(TagsInner::Owned(t.iter())));
        }
        let base = self.base?;
        let i = base.find(key)?;
        Some(Tags(TagsInner::Static(base.tags(i))))
    }

    /// The most frequent tag of `key`, or `None` when it is absent.
    ///
    /// This is the lookup [`Lexicon::tag_of`] is built on. It performs one
    /// ordered-map probe and, for a bundled lexicon, one binary search over
    /// bytes embedded in the executable; it allocates nothing.
    #[inline]
    #[must_use]
    pub fn primary_tag(&self, key: &str) -> Option<Tag> {
        if let Some(t) = self.overlay.get(key) {
            return t.first().cloned();
        }
        let base = self.base?;
        base.primary_tag(key).map(Tag::from_static)
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
        self.base.map_or(0, StaticLexicon::len) + self.overlay_new
    }

    /// Whether the lexicon has no entries.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Every entry, in ascending key order, most frequent tag first.
    ///
    /// The order is by Unicode scalar value (equivalently, by UTF-8 bytes) and
    /// is stable across runs and platforms, so the output of a
    /// [`Corpus::build_lexicon`](crate::Corpus::build_lexicon) can be written to
    /// a file and diffed.
    pub fn entries(&self) -> Entries<'_> {
        Entries {
            lexicon: self,
            base_at: 0,
            base_len: self.base.map_or(0, StaticLexicon::len),
            overlay: self.overlay.iter().peekable(),
        }
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
pub struct Tags<'a>(TagsInner<'a>);

#[derive(Debug, Clone)]
enum TagsInner<'a> {
    Static(StaticTags),
    Owned(std::slice::Iter<'a, Tag>),
}

impl Iterator for Tags<'_> {
    type Item = Tag;

    #[inline]
    fn next(&mut self) -> Option<Tag> {
        match &mut self.0 {
            TagsInner::Static(it) => it.next().map(Tag::from_static),
            TagsInner::Owned(it) => it.next().cloned(),
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        match &self.0 {
            TagsInner::Static(it) => it.size_hint(),
            TagsInner::Owned(it) => it.size_hint(),
        }
    }
}

impl ExactSizeIterator for Tags<'_> {}

/// Every entry of a [`Lexicon`], in ascending key order. See
/// [`Lexicon::entries`].
#[derive(Debug)]
pub struct Entries<'a> {
    lexicon: &'a Lexicon,
    base_at: usize,
    base_len: usize,
    overlay: std::iter::Peekable<std::collections::btree_map::Iter<'a, Box<str>, Box<[Tag]>>>,
}

impl<'a> Iterator for Entries<'a> {
    type Item = (&'a str, Tags<'a>);

    fn next(&mut self) -> Option<(&'a str, Tags<'a>)> {
        // A two-way merge of two ascending sequences. One step per call: an
        // overlay entry that shadows a base key advances both cursors at once,
        // so the shadowed base entry is skipped rather than emitted twice.
        //
        // Three `expect`s below rest on one invariant, which is worth stating
        // once because reordering this function is what would break it:
        // `base_len` is `base.map_or(0, StaticLexicon::len)` fixed at
        // construction, and `Entries` borrows the `Lexicon` immutably for its
        // whole lifetime, so `base_len > 0` implies `base.is_some()` and stays
        // implying it. Every read of `base` here is therefore guarded by
        // `base_at < base_len`, directly or through `base_key` being `Some`,
        // and the same guard is what keeps `key(base_at)`/`tags(base_at)` in
        // bounds. The last `expect` is different in kind: it is `Peekable`'s
        // own contract, that a `peek()` returning `Some` is followed by a
        // `next()` returning `Some` — nothing between the two touches
        // `self.overlay`.
        let base_key = (self.base_at < self.base_len).then(|| {
            self.lexicon
                .base
                .expect("base_len > 0 implies a base")
                .key(self.base_at)
        });
        let overlay_key = self.overlay.peek().map(|(k, _)| &***k);
        let take_base = match (base_key, overlay_key) {
            (None, None) => return None,
            (Some(_), None) => true,
            (None, Some(_)) => false,
            (Some(b), Some(o)) => b < o,
        };
        if take_base {
            let lex = self.lexicon.base.expect("base key exists");
            let tags = lex.tags(self.base_at);
            let key = lex.key(self.base_at);
            self.base_at += 1;
            return Some((key, Tags(TagsInner::Static(tags))));
        }
        if base_key == overlay_key {
            self.base_at += 1;
        }
        let (k, v) = self.overlay.next().expect("peeked a Some");
        Some((k, Tags(TagsInner::Owned(v.iter()))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tag(s: &'static str) -> Tag {
        Tag::new(s).unwrap()
    }

    #[test]
    fn bundled_sizes_and_defaults() {
        let en = Lexicon::bundled(Language::English);
        assert_eq!(en.len(), 92_538);
        assert_eq!(en.default_tag(), &tag("NN"));
        assert_eq!(en.capitalized_default_tag(), &tag("NNP"));
        let nl = Lexicon::bundled(Language::Dutch);
        assert_eq!(nl.len(), 11_699);
        assert_eq!(nl.default_tag(), &tag("N(soort,ev,neut)"));
    }

    /// A bundled token whose own text contains `/` or `*` is reachable by that
    /// text. The source corpora escape both characters, because a bare `/`
    /// separates a token from its tag; the packed keys carry the token, not the
    /// escape.
    #[test]
    fn tokens_containing_a_slash_or_a_star_are_reachable_by_their_text() {
        let en = Lexicon::bundled(Language::English);
        assert_eq!(en.tag_of("Asia/Pacific"), tag("JJ"));
        assert_eq!(en.tag_of("producer/director"), tag("NN"));
        assert_eq!(en.tag_of("M*A*S*H"), tag("NNP"));
        // ...and the markup spellings are gone.
        for gone in ["Asia\\/Pacific", "Asia\\", "me/PRP", "W/NNP.R.G."] {
            assert!(!en.contains(gone), "{gone:?} is still a key");
        }
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
        assert!(l.insert("dog", vec![tag("VB")]).unwrap().is_some());
        assert_eq!(l.len(), 1);
    }

    /// The empty token cannot be an entry, so it takes a default rather than
    /// finding a tagless entry and coming back untagged.
    #[test]
    fn the_empty_token_takes_the_default() {
        let en = Lexicon::bundled(Language::English);
        assert!(!en.contains(""));
        assert!(en.tags("").is_none());
        assert_eq!(en.tag_of(""), tag("NN"));
    }

    #[test]
    fn lexicons_are_independent() {
        let mut a = Lexicon::bundled(Language::English);
        let b = Lexicon::bundled(Language::English);
        a.insert("zzzprivate", vec![tag("XX")]).unwrap();
        assert_eq!(a.tag_of("zzzprivate"), tag("XX"));
        assert_eq!(b.tag_of("zzzprivate"), tag("NN"));
        assert_eq!(a.len(), b.len() + 1);
    }

    #[test]
    fn overriding_a_bundled_key_is_not_a_new_key() {
        let mut l = Lexicon::bundled(Language::English);
        let before = l.len();
        l.insert("dog", vec![tag("ZZ")]).unwrap();
        assert_eq!(l.len(), before);
        assert_eq!(l.primary_tag("dog"), Some(tag("ZZ")));
    }

    /// The four steps, each reached in turn. The bundled data behind them:
    /// `dog` is present (`NN` first); `Dog` is *also* present as `NNP`, so it
    /// never reaches the retry, while `Jumps` is absent and `jumps` is present
    /// as `NNS`; and no key spelled `Zzzzznotaword` exists in either case.
    #[test]
    fn the_lookup_chain_runs_in_order() {
        let en = Lexicon::bundled(Language::English);
        // 1. exact — and an exact hit always wins, even when a differently
        //    cased spelling of the same word exists.
        assert_eq!(en.tag_of("dog"), tag("NN"));
        assert_eq!(en.tag_of("Dog"), tag("NNP"));
        // 2. lowercase retry, for a capitalised form with no entry of its own.
        assert!(!en.contains("Jumps"));
        assert_eq!(en.tag_of("Jumps"), en.tag_of("jumps"));
        assert_eq!(en.tag_of("Jumps"), tag("NNS"));
        // 3. capitalised default
        assert_eq!(en.tag_of("Zzzzznotaword"), tag("NNP"));
        // 4. plain default
        assert_eq!(en.tag_of("zzzzznotaword"), tag("NN"));
        // The retry can be switched off, and then step 3 takes over.
        let strict = Lexicon::bundled(Language::English).with_lowercase_retry(false);
        assert!(!strict.lowercase_retry());
        assert_eq!(strict.tag_of("Jumps"), tag("NNP"));
    }

    /// The capitalised default is the Unicode `Uppercase` property, not `A`–`Z`.
    #[test]
    fn capitalisation_default_is_unicode() {
        let en = Lexicon::bundled(Language::English);
        for capitalised in ["Zzzzznotaword", "Ålesundzzz", "Москвазззз", "Ελλάςζζζ"]
        {
            assert_eq!(en.tag_of(capitalised), tag("NNP"), "{capitalised}");
        }
        for not in [
            "zzzzznotaword",
            "5zzzzz",
            ".zzzzz",
            "日本語ずずず",
            "😀zzzzz",
        ] {
            assert_eq!(en.tag_of(not), tag("NN"), "{not}");
        }
    }

    #[test]
    fn tags_are_most_frequent_first_and_never_empty() {
        let en = Lexicon::bundled(Language::English);
        let tags: Vec<Tag> = en.tags("null").expect("a real English word").collect();
        assert_eq!(tags, [tag("JJ"), tag("NN")]);
        assert_eq!(en.primary_tag("null"), Some(tag("JJ")));
        assert!(en.tags("no-such-word-at-all").is_none());
    }

    #[test]
    fn entries_merge_base_and_overlay_in_key_order() {
        let mut l = Lexicon::new(tag("NN"));
        for (k, t) in [("b", "NN"), ("a", "DT"), ("c", "VB")] {
            l.insert(k, vec![tag(t)]).unwrap();
        }
        let keys: Vec<&str> = l.entries().map(|(k, _)| k).collect();
        assert_eq!(keys, ["a", "b", "c"]);

        let mut l = Lexicon::bundled(Language::English);
        l.insert("dog", vec![tag("ZZ")]).unwrap();
        l.insert("zzzzznew", vec![tag("QQ")]).unwrap();
        let collected: Vec<(String, Vec<Tag>)> = l
            .entries()
            .map(|(k, t)| (k.to_owned(), t.collect()))
            .collect();
        assert_eq!(collected.len(), l.len(), "no entry emitted twice or lost");
        for pair in collected.windows(2) {
            assert!(pair[0].0 < pair[1].0, "entries are not sorted");
        }
        let dog = collected.iter().find(|(k, _)| k == "dog").unwrap();
        assert_eq!(dog.1, vec![tag("ZZ")], "the overlay shadows the base entry");
    }

    /// Every bundled entry is reachable by its own key and carries at least one
    /// tag. Enumerated over all 104,360 entries of both languages, through the
    /// public API rather than the packed reader.
    #[test]
    fn every_bundled_key_is_reachable_through_the_public_api() {
        for language in [Language::English, Language::Dutch] {
            let lex = Lexicon::bundled(language);
            let mut seen = 0usize;
            for (key, tags) in lex.entries() {
                seen += 1;
                let tags: Vec<Tag> = tags.collect();
                assert!(!tags.is_empty(), "{key:?} has no tags");
                assert!(lex.contains(key), "{key:?} not found by contains");
                assert_eq!(lex.primary_tag(key).as_ref(), tags.first(), "{key:?}");
                assert_eq!(lex.tag_of(key), tags[0], "{key:?}");
            }
            assert_eq!(seen, lex.len());
        }
    }

    /// The lowercase retry must not make any key unreachable: a key is always
    /// found by its own spelling, whatever case it is in. Enumerated over every
    /// bundled key that is not already all-lowercase — the population the retry
    /// can affect at all.
    #[test]
    fn the_lowercase_retry_never_shadows_an_exact_key() {
        for language in [Language::English, Language::Dutch] {
            let lex = Lexicon::bundled(language);
            let mut mixed_case = 0usize;
            for (key, tags) in lex.entries() {
                if key.to_lowercase() == key {
                    continue;
                }
                mixed_case += 1;
                let first = tags.into_iter().next().expect("non-empty");
                assert_eq!(
                    lex.tag_of(key),
                    first,
                    "{key:?} lost its exact match to the lowercase retry"
                );
            }
            assert!(
                mixed_case > 0,
                "{language:?} has no mixed-case keys to check"
            );
        }
    }
}
