//! `SentimentAnalyzer` — the constructor's lookup rules and the scoring loop.
//!
//! # The scoring loop, exactly
//!
//! ```text
//! score = 0; negator = 1
//! for token of words:
//!     lower = token.toLowerCase()
//!     if negations.indexOf(lower) > -1:  negator = -1          // and never back
//!     else if vocabulary[lower] !== undefined: score += negator * vocabulary[lower]
//!     else if stemmer:
//!         s = stemmer.stem(lower)
//!         if vocabulary[s] !== undefined:  score += negator * vocabulary[s]
//! return score / words.length
//! ```
//!
//! Three details in there are what a careful reading gets wrong:
//!
//! * **Negation is sticky, not windowed.** `negator` is set to -1 by the first
//!   negation word and is never restored — not after one token, not after
//!   punctuation, not at a sentence boundary. `["not","good","good"]` scores
//!   -2, not -1 and not 0. The obvious ports ("negate the next word", "reset on
//!   each hit") both disagree on the second `good`.
//! * **The negation test runs first**, so a word in both lists contributes
//!   nothing at all. `no` is an English AFINN entry worth -1 and an English
//!   negation word; the negation branch wins and it scores zero. Same for
//!   `no`/`nunca` in Spanish and `não`/`nunca` in Portuguese.
//! * **The stemmer is consulted second and only on a miss**, and it stems the
//!   *already lowercased* token, not the original.
//!
//! # The division
//!
//! `score / words.length` uses the array's length, so `getSentiment([])` is
//! `0/0` — `NaN`, not `0` and not an error — and a sparse array divides by a
//! denominator that counts the holes `forEach` skipped. A Rust iterator has no
//! holes, so [`SentimentAnalyzer::get_sentiment_over`] takes the denominator
//! explicitly for callers that need to reproduce that.
//!
//! # Accumulation order
//!
//! `score` is an `f64` summed strictly left to right and divided exactly once at
//! the end. The reference's own golden values — `-0.10144927536231885`,
//! `0.014705882352941176` — are compared for exact equality, and reordering the
//! sum, dividing per term, or summing in `f32` all change the last bits.
//! [`Contributions`] therefore yields one addend per token, in order, including
//! a `0.0` for tokens that score nothing, and [`SentimentAnalyzer::score`] is
//! its plain left fold. Adding `0.0` cannot perturb a sum whose running value
//! started at `+0.0`, so the extra addends are exact.

use std::borrow::Cow;

use crate::data::ordered_object::{
    self, LITERAL_FALLBACK, PROTOTYPE_MEMBER_TABLES, RESTRICTED_MESSAGE, Resolution,
};
use crate::stemmer::{NoStemmer, Stemmer};
use crate::vocabulary::{self, Vocabulary};
use crate::{Language, VocabularyKind};

/// Why `new SentimentAnalyzer(...)` refused to build an analyzer.
///
/// The reference throws bare `Error`s and — for one combination — a `TypeError`,
/// with messages that are part of its observable behaviour. [`Display`] renders
/// each one verbatim and [`Error::error_name`] names the constructor
/// the reference would have used.
///
/// [`Display`]: std::fmt::Display
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// `languageFiles[type]` was falsy: no such vocabulary family.
    ///
    /// The reference's message says "Type Language" — the word order is its
    /// own, and is reproduced.
    UnsupportedType {
        /// The `type` argument, verbatim.
        vocabulary_type: String,
    },
    /// `languageFiles[type][language]` was falsy: the family exists, that
    /// language does not.
    UnsupportedLanguage {
        /// The `type` argument, verbatim.
        vocabulary_type: String,
        /// The `language` argument, verbatim.
        language: String,
    },
    /// The dead `Object.create(languageFiles[type][language][0])` threw.
    ///
    /// Reachable only through the prototype chain: `('name', _, 'constructor')`
    /// resolves to `Object.name`, which is the string `"Object"`, whose `[0]` is
    /// `'O'` — and `Object.create('O')` is a `TypeError`. A `HashMap` lookup has
    /// no way to reach this, which is exactly why it is recorded here.
    ObjectPrototype {
        /// The first character of the string the lookup resolved to.
        value: char,
    },
    /// A `language` of `caller` or `arguments` on a built-in function, which
    /// throws before the analyzer sees it.
    RestrictedProperty,
}

impl Error {
    /// The reference constructor this would have been: `"Error"` or
    /// `"TypeError"`.
    #[must_use]
    pub fn error_name(&self) -> &'static str {
        match self {
            Self::UnsupportedType { .. } | Self::UnsupportedLanguage { .. } => "Error",
            Self::ObjectPrototype { .. } | Self::RestrictedProperty => "TypeError",
        }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedType { vocabulary_type } => {
                write!(f, "Type Language {vocabulary_type} not supported")
            }
            Self::UnsupportedLanguage {
                vocabulary_type,
                language,
            } => write!(
                f,
                "Type {vocabulary_type} for Language {language} not supported"
            ),
            Self::ObjectPrototype { value } => {
                write!(f, "Object prototype may only be an Object or null: {value}")
            }
            Self::RestrictedProperty => f.write_str(RESTRICTED_MESSAGE),
        }
    }
}

impl std::error::Error for Error {}

/// The table a shipped vocabulary lives in, borrowed when nothing rewrote it.
#[derive(Debug, Clone)]
enum Table {
    /// The process-wide decode of one blob, shared by every analyzer that wants
    /// it. The reference re-copies its vocabulary on every construction.
    Shared(&'static Vocabulary),
    /// A stemmed rebuild, or one of the degenerate empty tables the prototype
    /// chain produces.
    Owned(Vocabulary),
}

impl Table {
    fn get(&self) -> &Vocabulary {
        match self {
            Self::Shared(v) => v,
            Self::Owned(v) => v,
        }
    }
}

/// A word-list sentiment analyzer, ported from
/// The reference `SentimentAnalyzer`.
///
/// ```
/// use verbora_sentiment::SentimentAnalyzer;
///
/// let a = SentimentAnalyzer::without_stemmer("English", "afinn").unwrap();
/// assert_eq!(a.get_sentiment(["good"]), 3.0);
/// assert_eq!(a.get_sentiment(["not", "good"]), -1.5);
/// // Sticky: the second `good` is still negated.
/// assert_eq!(a.get_sentiment(["not", "good", "good"]), -2.0);
/// // An empty input divides by zero.
/// assert!(a.get_sentiment(Vec::<&str>::new()).is_nan());
/// ```
#[derive(Debug, Clone)]
pub struct SentimentAnalyzer<S = NoStemmer> {
    language: String,
    vocabulary_type: String,
    vocabulary: Table,
    negations: &'static [&'static str],
    stemmer: Option<S>,
}

impl SentimentAnalyzer<NoStemmer> {
    /// Builds an analyzer with no stemmer — the reference's `undefined` second
    /// argument.
    ///
    /// # Errors
    ///
    /// The same conditions the reference throws on; see [`Error`].
    pub fn without_stemmer(language: &str, vocabulary_type: &str) -> Result<Self, Error> {
        Self::new(language, None, vocabulary_type)
    }
}

impl<S: Stemmer> SentimentAnalyzer<S> {
    /// Builds an analyzer, mirroring `new SentimentAnalyzer(language, stemmer,
    /// type)`.
    ///
    /// `language` and `vocabulary_type` are the reference's own strings, so
    /// unsupported combinations fail with its own messages. [`VocabularyKind`]
    /// and [`Language`] spell the supported ones if you would rather not.
    ///
    /// Supplying a stemmer rebuilds the whole vocabulary through it, which is
    /// what the reference does and what makes the 1234 capitalised German
    /// pattern entries reachable at all. It is the expensive part of
    /// construction — 24,839 stem calls for English senticon — so build one
    /// analyzer and reuse it.
    ///
    /// # Errors
    ///
    /// * [`Error::UnsupportedType`] when no such vocabulary family exists.
    /// * [`Error::UnsupportedLanguage`] when the family has no such language.
    /// * [`Error::ObjectPrototype`] and [`Error::RestrictedProperty`] for the
    ///   prototype-chain corners; see [`Error`].
    pub fn new(language: &str, stemmer: Option<S>, vocabulary_type: &str) -> Result<Self, Error> {
        let vocabulary = resolve(vocabulary_type, language)?;
        let (mut table, negations) = match vocabulary {
            Resolved::Source(index) => {
                let source = crate::data::SOURCES
                    .get(index)
                    .expect("resolve returned an in-range index");
                (Table::Shared(vocabulary::load(index)), source.negations)
            }
            // `Object.assign({}, undefined)` is `{}`, and `undefined[1]` is
            // `undefined`, so these construct with an empty vocabulary and no
            // negations rather than throwing.
            Resolved::Empty(kind) => (Table::Owned(Vocabulary::empty(kind)), &[] as &[&str]),
        };

        if let (Some(stemmer), Table::Shared(base)) = (stemmer.as_ref(), &table) {
            table = Table::Owned(base.stemmed(stemmer));
        }

        Ok(Self {
            language: language.to_owned(),
            vocabulary_type: vocabulary_type.to_owned(),
            vocabulary: table,
            negations,
            stemmer,
        })
    }

    /// The `language` argument, stored verbatim and never validated — as the
    /// reference stores it.
    #[must_use]
    pub fn language(&self) -> &str {
        &self.language
    }

    /// The `type` argument, stored verbatim.
    #[must_use]
    pub fn vocabulary_type(&self) -> &str {
        &self.vocabulary_type
    }

    /// The word → polarity table this analyzer scores with.
    #[must_use]
    pub fn vocabulary(&self) -> &Vocabulary {
        self.vocabulary.get()
    }

    /// The negation words for this language, in file order.
    ///
    /// The reference hands out the require-cache's own array, so pushing to one
    /// analyzer's list changes every other analyzer of that language and
    /// outlives them both. This is an immutable slice instead — see the crate
    /// documentation's divergence note.
    #[must_use]
    pub fn negations(&self) -> &'static [&'static str] {
        self.negations
    }

    /// The stemmer, if one was supplied.
    #[must_use]
    pub fn stemmer(&self) -> Option<&S> {
        self.stemmer.as_ref()
    }

    /// The per-token addends, lazily, in order.
    ///
    /// This is the primitive: it borrows the analyzer, consumes any iterator of
    /// string-like tokens, materialises nothing, and carries the sticky negation
    /// state across the whole run. Everything else here is built on it, so a
    /// tokenizer can be piped straight in:
    ///
    /// ```
    /// use verbora_sentiment::SentimentAnalyzer;
    ///
    /// let a = SentimentAnalyzer::without_stemmer("English", "afinn").unwrap();
    /// let deltas: Vec<f64> = a.contributions(["it", "is", "not", "good"]).collect();
    /// assert_eq!(deltas, [0.0, 0.0, 0.0, -3.0]);
    /// ```
    pub fn contributions<I>(&self, words: I) -> Contributions<'_, S, I::IntoIter>
    where
        I: IntoIterator,
        I::Item: AsRef<str>,
    {
        Contributions {
            analyzer: self,
            words: words.into_iter(),
            negator: 1.0,
        }
    }

    /// The running total and the token count, without dividing.
    ///
    /// Useful when the denominator is not the token count — see
    /// [`Self::get_sentiment_over`] — or when several segments are scored and
    /// combined.
    pub fn score<I>(&self, words: I) -> Score
    where
        I: IntoIterator,
        I::Item: AsRef<str>,
    {
        let mut sum = 0.0;
        let mut count = 0usize;
        for delta in self.contributions(words) {
            sum += delta;
            count += 1;
        }
        Score { sum, count }
    }

    /// `getSentiment(words)`: the summed polarity divided by the token count.
    ///
    /// Returns `NaN` for an empty input, because the reference computes `0 / 0`.
    pub fn get_sentiment<I>(&self, words: I) -> f64
    where
        I: IntoIterator,
        I::Item: AsRef<str>,
    {
        self.score(words).value()
    }

    /// `getSentiment` with the denominator supplied separately.
    ///
    /// The reference divides by `words.length`, which for a *sparse* array counts
    /// the holes that `forEach` skipped: `['good', <2 holes>, 'bad']` has length
    /// 4 and scores `(3 + -3) / 4`. Rust iterators have no holes, so that case
    /// is expressed by passing the length explicitly.
    ///
    /// ```
    /// use verbora_sentiment::SentimentAnalyzer;
    ///
    /// let a = SentimentAnalyzer::without_stemmer("English", "afinn").unwrap();
    /// assert_eq!(a.get_sentiment_over(["good"], 2), 1.5);
    /// ```
    pub fn get_sentiment_over<I>(&self, words: I, len: usize) -> f64
    where
        I: IntoIterator,
        I::Item: AsRef<str>,
    {
        self.score(words).over(len)
    }

    /// Whether `word` — already lowercased — is a negation word.
    fn is_negation(&self, word: &str) -> bool {
        self.negations.contains(&word)
    }
}

/// A summed score and the number of tokens it came from.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Score {
    /// The polarity total, accumulated left to right in `f64`.
    pub sum: f64,
    /// How many tokens the iterator yielded.
    pub count: usize,
}

impl Score {
    /// `sum / count`, which is `NaN` when nothing was scored.
    #[must_use]
    pub fn value(self) -> f64 {
        #[expect(
            clippy::cast_precision_loss,
            reason = "matches the reference, where the length is already an f64"
        )]
        {
            self.sum / self.count as f64
        }
    }

    /// `sum / len`, for a denominator that is not the token count.
    #[must_use]
    pub fn over(self, len: usize) -> f64 {
        #[expect(
            clippy::cast_precision_loss,
            reason = "matches the reference, where the length is already an f64"
        )]
        {
            self.sum / len as f64
        }
    }
}

/// The lazy per-token addends of [`SentimentAnalyzer::contributions`].
///
/// Yields exactly one `f64` per input token — `0.0` for a token that scored
/// nothing, including the negation words themselves — so summing it left to
/// right reproduces the reference's `score += …` sequence addend for addend.
#[derive(Debug)]
pub struct Contributions<'a, S, I> {
    analyzer: &'a SentimentAnalyzer<S>,
    words: I,
    negator: f64,
}

impl<S: Stemmer, I> Iterator for Contributions<'_, S, I>
where
    I: Iterator,
    I::Item: AsRef<str>,
{
    type Item = f64;

    fn next(&mut self) -> Option<f64> {
        let token = self.words.next()?;
        let lower = lowercase(token.as_ref());
        if self.analyzer.is_negation(&lower) {
            self.negator = -1.0;
            return Some(0.0);
        }
        let vocabulary = self.analyzer.vocabulary.get();
        if let Some(value) = vocabulary.polarity(&lower) {
            return Some(self.negator * value);
        }
        if let Some(stemmer) = self.analyzer.stemmer.as_ref() {
            let stem = stemmer.stem(&lower);
            if let Some(value) = vocabulary.polarity(&stem) {
                return Some(self.negator * value);
            }
        }
        Some(0.0)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.words.size_hint()
    }
}

/// `String.prototype.toLowerCase`, without allocating for text that is already
/// lowercase.
///
/// The rewriting path must be `str::to_lowercase` and nothing else: it is the
/// only one of Rust's four lowercasing routines that implements the same
/// full-string algorithm. `to_ascii_lowercase` leaves `İ` and `Σ` alone; a
/// `char`-by-`char` loop expands `İ` correctly but has no context for
/// Final_Sigma, so `ΑΣ` comes out as `ασ` where the reference gives `ας`.
///
/// Two fast paths avoid it without weakening it. All-ASCII input can be tested
/// with one byte scan, because neither special rule can apply. Non-ASCII input
/// is tested by [`needs_lowering`], which matters far more here than it looks:
/// nine of the fourteen vocabularies are non-English, so most of this crate's
/// tokens are accented Latin, Greek or Cyrillic that is *already* lowercase and
/// whose `to_lowercase` is the identity — and paying an allocation and a copy to
/// discover that is the difference between beating the reference engine on this path and losing to
/// it.
fn lowercase(s: &str) -> Cow<'_, str> {
    if s.is_ascii() {
        if s.bytes().any(|b| b.is_ascii_uppercase()) {
            Cow::Owned(s.to_ascii_lowercase())
        } else {
            Cow::Borrowed(s)
        }
    } else if needs_lowering(s) {
        Cow::Owned(s.to_lowercase())
    } else {
        Cow::Borrowed(s)
    }
}

/// Whether `str::to_lowercase` would change `s`.
///
/// `str::to_lowercase` is the per-character full lowercase mapping plus exactly
/// one context-sensitive rule, Final_Sigma, which fires only on `Σ`. If no
/// character maps to anything but itself then none of them is `Σ` — `Σ` already
/// maps to `σ` — so the whole-string result is the input, and the answer is
/// exact rather than conservative. `lowercase_agrees_with_the_standard_library`
/// checks it over every code point below U+3000 plus the astral cased blocks.
fn needs_lowering(s: &str) -> bool {
    s.chars().any(|c| {
        let mut lower = c.to_lowercase();
        lower.next() != Some(c) || lower.next().is_some()
    })
}

/// What the constructor's two lookups resolved to.
enum Resolved {
    /// A row of `languageFiles`, by index.
    Source(usize),
    /// A truthy non-table value: construction succeeds with an empty vocabulary.
    Empty(Option<VocabularyKind>),
}

/// `languageFiles[type][language]`, prototype chain and all.
fn resolve(vocabulary_type: &str, language: &str) -> Result<Resolved, Error> {
    let unsupported_language = || Error::UnsupportedLanguage {
        vocabulary_type: vocabulary_type.to_owned(),
        language: language.to_owned(),
    };

    if let Some(kind) = VocabularyKind::from_pattern(vocabulary_type) {
        if let Some(index) = vocabulary::source_index(kind, language) {
            return Ok(Resolved::Source(index));
        }
        // `languageFiles[realType]` is an object literal too, so a `language`
        // named after an Object.prototype member finds a truthy function and
        // constructs — with an empty vocabulary — instead of throwing.
        return match lookup(LITERAL_FALLBACK, language) {
            Some(Resolution::Truthy) => Ok(Resolved::Empty(Some(kind))),
            Some(Resolution::FirstChar(value)) => Err(Error::ObjectPrototype { value }),
            Some(Resolution::Restricted) => Err(Error::RestrictedProperty),
            Some(Resolution::Falsy) | None => Err(unsupported_language()),
        };
    }

    // Not one of the four families. `languageFiles[type]` still resolves for
    // every Object.prototype member — `Object`, `toString`, `valueOf`, … — and
    // all of them are truthy, so the constructor moves on to the second lookup
    // and the error it eventually throws names the *language*, not the type.
    let Some(table) = lookup_table(vocabulary_type) else {
        return Err(Error::UnsupportedType {
            vocabulary_type: vocabulary_type.to_owned(),
        });
    };
    match lookup(table, language) {
        Some(Resolution::Truthy) => Ok(Resolved::Empty(None)),
        Some(Resolution::FirstChar(value)) => Err(Error::ObjectPrototype { value }),
        Some(Resolution::Restricted) => Err(Error::RestrictedProperty),
        Some(Resolution::Falsy) | None => Err(unsupported_language()),
    }
}

fn lookup(table: &[(&str, Resolution)], name: &str) -> Option<Resolution> {
    table.iter().find(|(n, _)| *n == name).map(|(_, r)| *r)
}

/// The chain table for `languageFiles[name]`, or `None` if `name` is not an
/// `Object.prototype` member and the lookup really is `undefined`.
fn lookup_table(name: &str) -> Option<&'static [(&'static str, Resolution)]> {
    let slot = ordered_object::OBJECT_PROTOTYPE_NAMES
        .iter()
        .position(|n| *n == name)?;
    PROTOTYPE_MEMBER_TABLES.get(slot).copied()
}

impl VocabularyKind {
    /// Parses the reference's `type` argument.
    #[must_use]
    pub fn from_pattern(s: &str) -> Option<Self> {
        match s {
            "afinn" => Some(Self::Afinn),
            "afinnFinancialMarketNews" => Some(Self::AfinnFinancialMarketNews),
            "senticon" => Some(Self::Senticon),
            "pattern" => Some(Self::Pattern),
            _ => None,
        }
    }
}

impl Language {
    /// Parses one of the reference's `language` arguments.
    #[must_use]
    pub fn from_pattern(s: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|l| l.as_str() == s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Polarity;

    fn afinn_en() -> SentimentAnalyzer<NoStemmer> {
        SentimentAnalyzer::without_stemmer("English", "afinn").expect("English AFINN exists")
    }

    #[test]
    fn negation_is_sticky_and_unscored() {
        let a = afinn_en();
        assert_eq!(a.get_sentiment(["good"]), 3.0);
        assert_eq!(a.get_sentiment(["not", "good"]), -1.5);
        assert_eq!(a.get_sentiment(["not", "good", "good"]), -2.0);
        assert_eq!(a.get_sentiment(["good", "not", "good"]), 0.0);
        assert_eq!(a.get_sentiment(["not", "not", "good"]), -1.0);
        assert_eq!(a.get_sentiment(["NOT", "GOOD"]), -1.5);
    }

    #[test]
    fn a_word_in_both_lists_scores_nothing() {
        // `no` is worth -1 in English AFINN and is also a negation word.
        let a = afinn_en();
        assert_eq!(a.vocabulary().polarity("no"), Some(-1.0));
        assert_eq!(a.get_sentiment(["no"]), 0.0);
        assert_eq!(a.get_sentiment(["no", "good"]), -1.5);
    }

    #[test]
    fn empty_input_is_nan() {
        assert!(afinn_en().get_sentiment(Vec::<&str>::new()).is_nan());
        assert!(afinn_en().score(Vec::<&str>::new()).value().is_nan());
    }

    #[test]
    fn holes_are_counted_by_the_denominator() {
        let a = afinn_en();
        assert_eq!(a.get_sentiment_over(["good", "bad"], 4), 0.0);
        assert_eq!(a.get_sentiment_over(["good"], 2), 1.5);
        assert_eq!(a.get_sentiment_over(Vec::<&str>::new(), 5), 0.0);
    }

    #[test]
    fn a_null_prototype_answers_undefined_for_object_members() {
        let a = afinn_en();
        assert_eq!(
            a.get_sentiment([
                "constructor",
                "toString",
                "hasOwnProperty",
                "setPrototypeOf"
            ]),
            0.0
        );
    }

    #[test]
    fn error_messages_match_the_reference() {
        let e = SentimentAnalyzer::without_stemmer("English", "bogus").unwrap_err();
        assert_eq!(e.to_string(), "Type Language bogus not supported");
        assert_eq!(e.error_name(), "Error");

        let e = SentimentAnalyzer::without_stemmer("Klingon", "afinn").unwrap_err();
        assert_eq!(
            e.to_string(),
            "Type afinn for Language Klingon not supported"
        );

        let e = SentimentAnalyzer::without_stemmer("English", "constructor").unwrap_err();
        assert_eq!(
            e.to_string(),
            "Type constructor for Language English not supported"
        );

        let e = SentimentAnalyzer::without_stemmer("name", "constructor").unwrap_err();
        assert_eq!(
            e.to_string(),
            "Object prototype may only be an Object or null: O"
        );
        assert_eq!(e.error_name(), "TypeError");
    }

    #[test]
    fn prototype_names_construct_an_empty_analyzer() {
        for language in ["__proto__", "constructor", "toString", "valueOf"] {
            let a = SentimentAnalyzer::without_stemmer(language, "afinn")
                .unwrap_or_else(|e| panic!("{language}: {e}"));
            assert_eq!(a.vocabulary().len(), 0);
            assert_eq!(a.negations(), &[] as &[&str]);
            assert_eq!(a.language(), language);
        }
    }

    #[test]
    fn financial_market_news_scores_zero() {
        let a = SentimentAnalyzer::without_stemmer("English", "afinnFinancialMarketNews").unwrap();
        assert_eq!(a.vocabulary().len(), 0);
        assert_eq!(a.negations(), &["not", "no", "never", "neither"]);
        assert_eq!(a.get_sentiment(["bankruptcy", "raises"]), 0.0);
    }

    #[test]
    fn lowercase_matches_the_full_string_algorithm() {
        assert_eq!(lowercase("ABC"), "abc");
        assert!(matches!(lowercase("abc"), Cow::Borrowed("abc")));
        assert_eq!(lowercase("İ"), "i\u{307}");
        assert_eq!(lowercase("ΑΣ"), "ας");
        assert_eq!(lowercase("Σ"), "σ");
        assert_eq!(lowercase(""), "");

        // The point of the non-ASCII fast path: already-lowercase accented text
        // is borrowed, not copied.
        assert!(matches!(lowercase("película"), Cow::Borrowed(_)));
        assert!(matches!(lowercase("ελλάδα"), Cow::Borrowed(_)));
        assert!(matches!(lowercase("москва"), Cow::Borrowed(_)));
        assert!(matches!(lowercase("Película"), Cow::Owned(_)));
    }

    #[test]
    fn lowercase_agrees_with_the_standard_library() {
        // Every code point where the fast path could wrongly claim "no change",
        // plus the two-character contexts Final_Sigma needs.
        for cp in 0u32..0x3000 {
            if let Some(c) = char::from_u32(cp) {
                let s = c.to_string();
                assert_eq!(lowercase(&s), s.to_lowercase(), "U+{cp:04X}");
            }
        }
        // Deseret and Warang Citi: astral blocks that really do have case.
        for cp in (0x1_0400u32..0x1_0450).chain(0x1_18A0..0x1_18E0) {
            if let Some(c) = char::from_u32(cp) {
                let s = c.to_string();
                assert_eq!(lowercase(&s), s.to_lowercase(), "U+{cp:04X}");
            }
        }
        for a in ['Σ', 'σ', 'ς', 'Α', 'α', 'İ', 'ß', 'ẞ', 'ǅ', 'a'] {
            for b in ['Σ', 'α', ' ', '.', 'Α', '😀'] {
                let s = format!("{a}{b}");
                assert_eq!(lowercase(&s), s.to_lowercase(), "{s:?}");
                let s = format!("{a}{b}{a}");
                assert_eq!(lowercase(&s), s.to_lowercase(), "{s:?}");
            }
        }
    }

    #[test]
    fn the_prototype_tables_cover_object_prototype_exactly() {
        // The generator dumps one table per Object.prototype member, parallel to
        // the name list `lookup_table` indexes with. If they ever drift apart,
        // `resolve` would answer "type not supported" for a name the reference
        // resolves — so pin the lengths, and pin one representative lookup.
        assert_eq!(
            PROTOTYPE_MEMBER_TABLES.len(),
            ordered_object::OBJECT_PROTOTYPE_NAMES.len()
        );
        assert_eq!(
            LITERAL_FALLBACK.len(),
            ordered_object::OBJECT_PROTOTYPE_NAMES.len()
        );
        let ctor = lookup_table("constructor").expect("Object.prototype.constructor");
        assert_eq!(lookup(ctor, "name"), Some(Resolution::FirstChar('O')));
        assert_eq!(lookup(ctor, "arguments"), Some(Resolution::Restricted));
        assert_eq!(lookup(ctor, "English"), None);
        assert!(lookup_table("nonesuch").is_none());
    }

    #[test]
    fn contributions_are_lazy_and_ordered() {
        let a = afinn_en();
        let first: Vec<f64> = a.contributions(["good", "bad", "good"]).take(2).collect();
        assert_eq!(first, [3.0, -3.0]);
    }

    // ---- input classes ---------------------------------------------------
    //
    // Every one of these must produce a finite, defined answer rather than a
    // panic or a slice-boundary error: the analyzer's input is arbitrary text
    // and the lookup key is whatever the caller's tokenizer produced.

    #[test]
    fn empty_and_single_character_inputs() {
        let a = afinn_en();
        assert!(a.get_sentiment(Vec::<&str>::new()).is_nan());
        assert_eq!(a.get_sentiment([""]), 0.0);
        assert_eq!(a.get_sentiment(["", "", ""]), 0.0);
        assert_eq!(a.get_sentiment(["a"]), 0.0);
        assert_eq!(a.get_sentiment(["A"]), 0.0);
        assert_eq!(a.get_sentiment(["0"]), 0.0);
        assert_eq!(a.get_sentiment(["9"]), 0.0);
        assert_eq!(a.score([""]).count, 1);
    }

    #[test]
    fn case_folding_reaches_the_same_entry() {
        let a = afinn_en();
        for token in ["good", "Good", "GOOD", "gOOd", "GooD"] {
            assert_eq!(a.get_sentiment([token]), 3.0, "{token}");
        }
    }

    #[test]
    fn accented_latin_cyrillic_greek_and_cjk() {
        // Spanish AFINN has accented entries; the lookup must be on the
        // lowercased *string*, not on ASCII-folded bytes.
        let es = SentimentAnalyzer::without_stemmer("Spanish", "afinn").unwrap();
        assert!(es.vocabulary().polarity("sueño").is_some());
        assert_eq!(es.get_sentiment(["SUEÑO"]), es.get_sentiment(["sueño"]));
        assert!(es.vocabulary().polarity("éxito").is_some());
        assert_eq!(es.get_sentiment(["ÉXITO"]), es.get_sentiment(["éxito"]));

        let a = afinn_en();
        // `naïve` really is an AFINN-165 key, accent and all — which is why the
        // lookup must not fold accents away.
        assert_eq!(a.vocabulary().get("naïve"), Some(Polarity::Number(-2.0)));
        assert_eq!(a.get_sentiment(["Naïve"]), -2.0);
        assert_eq!(a.get_sentiment(["café", "Ångström", "crème"]), 0.0);
        assert_eq!(a.get_sentiment(["Москва", "Ελλάδα", "ΑΣΤΕΙΟ"]), 0.0);
        assert_eq!(a.get_sentiment(["日本語", "中文测试", "한국어"]), 0.0);
        // Greek Final_Sigma: the lowercase of "ΑΣ" is "ας", not "ασ".
        assert_eq!(lowercase("ΑΣΤΕΙΟ"), "αστειο");
        assert_eq!(lowercase("ΟΔΟΣ"), "οδος");
    }

    #[test]
    fn astral_characters_and_emoji() {
        let a = afinn_en();
        assert_eq!(a.get_sentiment(["😀", "a😀b", "𝕳𝖊𝖑𝖑𝖔"]), 0.0);
        // Spanish and Portuguese AFINN really do key on emoji.
        let es = SentimentAnalyzer::without_stemmer("Spanish", "afinn").unwrap();
        assert_eq!(es.vocabulary().get("😂"), Some(Polarity::Number(1.0)));
        assert_eq!(es.get_sentiment(["😂"]), 1.0);
        assert_eq!(es.get_sentiment(["no", "😂"]), -0.5);
    }

    #[test]
    fn punctuation_and_digits_are_inert() {
        let a = afinn_en();
        assert_eq!(
            a.get_sentiment(["!", "...", "--", "'", "?!", "«»", "—"]),
            0.0
        );
        assert_eq!(a.get_sentiment(["123", "3.14", "1,000", "-0"]), 0.0);
        // Punctuation does not reset the negation, which is the whole point.
        assert_eq!(a.get_sentiment(["not", ".", "good"]), -1.0);
    }

    #[test]
    fn very_long_inputs() {
        let a = afinn_en();
        let long_token = "a".repeat(100_000);
        assert_eq!(a.get_sentiment([long_token.as_str()]), 0.0);

        let many: Vec<&str> = std::iter::repeat_n("good", 10_000).collect();
        assert_eq!(a.get_sentiment(many.iter().copied()), 3.0);

        // One negation at the front flips all 10,000 that follow.
        let negated: Vec<&str> = std::iter::once("not")
            .chain(std::iter::repeat_n("good", 10_000))
            .collect();
        let expected = -3.0 * 10_000.0 / 10_001.0;
        assert_eq!(a.get_sentiment(negated.iter().copied()), expected);
    }

    #[test]
    fn the_sum_is_left_to_right() {
        // Three values whose f64 sum differs by an ULP if reassociated.
        let a = SentimentAnalyzer::without_stemmer("English", "senticon").unwrap();
        let tokens = ["good", "bad", "excellent", "awful", "fine"];
        let by_fold: f64 = a.contributions(tokens).fold(0.0, |acc, d| acc + d);
        assert_eq!(a.score(tokens).sum.to_bits(), by_fold.to_bits());
    }

    #[test]
    fn iterators_of_owned_strings_work_too() {
        let a = afinn_en();
        let owned: Vec<String> = vec!["not".into(), "good".into()];
        assert_eq!(a.get_sentiment(&owned), -1.5);
        assert_eq!(a.get_sentiment(owned.iter().map(String::as_str)), -1.5);
        assert_eq!(a.get_sentiment(owned), -1.5);
    }
}
