//! Word-list sentiment analysis with the reference parity.
//!
//! A port of the reference `SentimentAnalyzer`: 166 lines of code
//! over fourteen vocabularies in ten languages, drawn from three lexicon
//! projects — AFINN, ML-SentiCon and the CLiPS Pattern project.
//!
//! ```
//! use verbora_sentiment::SentimentAnalyzer;
//!
//! let analyzer = SentimentAnalyzer::without_stemmer("English", "afinn")?;
//! assert_eq!(analyzer.get_sentiment(["good"]), 3.0);
//! assert_eq!(analyzer.get_sentiment(["not", "good"]), -1.5);
//! # Ok::<(), verbora_sentiment::Error>(())
//! ```
//!
//! # Iterator first
//!
//! [`SentimentAnalyzer::contributions`] is the primitive: it takes any iterator
//! of string-like tokens, yields one addend per token, and materialises nothing.
//! [`score`](SentimentAnalyzer::score) and
//! [`get_sentiment`](SentimentAnalyzer::get_sentiment) are folds over it, so a
//! tokenizer pipes straight in and a document is never collected into a `Vec`:
//!
//! ```
//! use verbora_sentiment::SentimentAnalyzer;
//! use verbora_tokenizers::WordTokenizer;
//!
//! let analyzer = SentimentAnalyzer::without_stemmer("English", "afinn").unwrap();
//! let tokenizer = WordTokenizer::new();
//! let tokens = tokenizer.tokens("This is not a good day.").expect("splitting mode");
//! // Six tokens; `not` flips `good`, so the total is -3 over 6.
//! assert_eq!(analyzer.get_sentiment(tokens), -0.5);
//! ```
//!
//! # Scoring many documents at once
//!
//! One `SentimentAnalyzer` is read-only once built, so scoring an independent
//! batch of documents against it — one review, one ticket, one comment at a
//! time — parallelises without changing a single rule of the scoring loop.
//! The optional `parallel` feature (off by default) adds
//! `SentimentAnalyzer::par_get_sentiment_batch`, a Rayon-backed fan-out over
//! the exact same [`get_sentiment`](SentimentAnalyzer::get_sentiment); see
//! that method's documentation (in `src/parallel.rs`, built only when the
//! feature is on) for when it is worth reaching for over a plain loop and
//! what it costs.
//!
//! # The three things this port exists to get right
//!
//! **Negation is sticky.** One negation word flips the sign for the *rest of the
//! input* — there is no window, punctuation does not reset it, and the negation
//! word itself is never scored. `["not","good","good"]` is -2.
//!
//! **Stem collisions resolve last-wins in file order.** Supplying a stemmer
//! rebuilds the vocabulary; 1415 of English AFINN's 3382 keys collide, and 109
//! of those collisions change the stored polarity. A `HashMap` or `BTreeMap`
//! picks different winners and quietly changes scores.
//!
//! **The sum is left to right.** The reference's published golden values are
//! compared exactly; reordering the accumulation changes the last bits.
//!
//! Each is spelled out where it is implemented — see [`SentimentAnalyzer`] for
//! the scoring loop and [`Vocabulary`] for the tables.
//!
//! # How the lexicons ship, and why
//!
//! Three options were on the table: embed the JSON and parse it, read it from
//! disk at run time, or embed a prebuilt index. The third won, and the
//! measurements are the argument.
//!
//! The reference eagerly `require`s ~7.5 MB of JSON at import time whether or
//! not any of it is used — then discards every field except one polarity per
//! entry. `wordnet_id`, `sense`, `subjectivity`, `intensity` and `confidence`
//! are 84% of those bytes and no code path can read them. What survives the
//! projection is 75,803 `(key, polarity)` pairs: **1.2 MB**, shipped as
//! `key \0 polarity \0` blobs across thirteen `include_bytes!` files (the
//! fourteenth is deliberately empty; see the divergences below). Nothing is read
//! from disk, so the crate works from a single static binary, and nothing needs
//! a build script: the blobs are checked in as data, and their invariants are
//! re-proved by this crate's own tests on every run.
//!
//! Each table is decoded on first use and cached for the process. Measured on
//! one machine with `cargo bench -p verbora-sentiment`, against the reference
//! implementation's own figures for the same operations:
//!
//! | | the reference | this crate |
//! |---|---|---|
//! | import / first touch | 40 ms, all 7.5 MB, unconditionally | 0 — nothing is decoded until asked |
//! | first `("English", "afinn")` | 612 µs, and again every construction | 61 µs, once |
//! | first `("English", "senticon")` | 8.0 ms, and again every construction | 644 µs, once |
//! | every construction after the first | unchanged | **19 ns** |
//! | all fourteen tables | 40 ms + 8 ms each | 7.7 ms, once |
//! | `+ PorterStemmer`, senticon | 72 ms per construction | 23 ms per construction |
//! | scoring 2,816 English tokens | 55 µs | 30 µs |
//!
//! The stemmed row is the one that has not improved by orders of magnitude, and
//! it cannot: 24,839 words genuinely have to be re-stemmed, because a stemmer is
//! an arbitrary caller-supplied function and there is nothing to precompute.
//! Build the analyzer once and keep it.
//!
//! Scoring runs at roughly 94 M tokens/s for lowercase ASCII, 61 M for uppercase
//! ASCII (one `to_ascii_lowercase` per token) and 41 M for non-ASCII. That last
//! figure is the one that took work: `str::to_lowercase` is the only Rust
//! routine that matches the reference's algorithm, and calling it unconditionally
//! made this crate *slower* tha reference engine on Spanish and Greek text. It is now only
//! called when a scan proves the string would change, which most already-lowercase
//! non-English text does not.
//!
//! # Lexicon provenance and licensing
//!
//! The reference tree ships all of this under one MIT `LICENSE.txt`
//! (Chris Umbel, Rob Ellis, Russell Mull), with the sentiment sources carrying
//! their own MIT headers (Domingo Martín Mancera and Hugo W.L. ter Doest, based
//! on `dmarman/lorca`). The upstream lexicons themselves have separate
//! provenance and **no licence file of their own is shipped anywhere in the
//! reference tree**:
//!
//! | Vocabulary | Upstream | Licence found in the reference tree |
//! |---|---|---|
//! | AFINN English | `afinn-165`, Titus Wormer | MIT (the package's own `license` file) |
//! | AFINN Spanish, Portuguese | shipped JSON, no attribution | none — MIT by inheritance only |
//! | senticon (es, en, gl, ca, eu) | ML-SentiCon, converted by `tools/sentimentXmlParser` | none |
//! | pattern (nl, it, en, fr, de) | CLiPS Pattern project, converted by `tools/XmlParser4PatternData` | none |
//!
//! Redistributing them here is exactly as well-founded as the reference's own
//! redistribution, and no more. Anyone shipping this crate commercially should
//! confirm the ML-SentiCon and Pattern terms independently.
//!
//! # Divergences from the reference
//!
//! * **`afinnFinancialMarketNews` is empty, on purpose.** The reference imports
//!   `.afinnFinancialMarketNews` from a package that exports
//!   `.afinn165FinancialMarketNews`, so the vocabulary is `{}` at runtime and
//!   every score is 0. Shipping the package's real data would be a bug fix that
//!   breaks parity, so the table is carried as an explicit empty one.
//! * **`negations` is immutable.** The reference hands out the require-cache's
//!   own array: `a.negations.push('x')` is visible in every other analyzer of
//!   that language and permanently mutates the cached JSON module. Here it is a
//!   `&'static [&'static str]`, so the aliasing is unobservable. The library
//!   itself never mutates the list; only a caller reaching into the public field
//!   could tell the difference.
//! * **Arguments that are not strings.** `getSentiment(null)`,
//!   `getSentiment('good')` and `getSentiment([5])` are `TypeError`s in
//!   The reference. They cannot be written here: the tokens are `AsRef<str>`.
//! * **Sparse arrays** have no Rust equivalent, so the case where
//!   `words.length` exceeds the number of tokens `forEach` visited is expressed
//!   by [`SentimentAnalyzer::get_sentiment_over`].
//! * **The unstemmed vocabulary is shared, not copied.** The reference's
//!   `Object.assign({}, …)` gives each analyzer its own mutable copy, which the
//!   comment above it says is needed "or in subsequent execution the polarity
//!   will be undefined". Here an analyzer without a stemmer borrows the
//!   process-wide decode. Since [`Vocabulary`] exposes no way to mutate a table,
//!   the difference is unobservable — and it is what turns a 8 ms construction
//!   into a 19 ns one. A stemmed analyzer owns its rebuild, as the reference
//!   does.
//!
//! # Parity
//!
//! `tests/parity.rs` replays `fixtures/sentiment.json` — 82 suites recorded from
//! the real library, including its own published golden scores for twelve
//! configurations in ten languages, the full stemmed vocabularies of the three
//! AFINN tables, and an exhaustive enumeration of the constructor's
//! prototype-chain corners.

mod analyzer;
mod data;
#[cfg(feature = "parallel")]
mod parallel;
mod stemmer;
mod vocabulary;

pub use analyzer::{Contributions, Error, Score, SentimentAnalyzer};
pub use stemmer::{NoStemmer, Stemmer};
pub use vocabulary::{Polarity, Vocabulary};

/// Which family of lexicon a vocabulary comes from — the reference's `type`
/// constructor argument.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VocabularyKind {
    /// AFINN-165 word lists. Values are integers in `-5..=5`, and stay
    /// the reference *numbers*.
    Afinn,
    /// AFINN-165 tuned for financial market news — **empty at runtime**, see the
    /// crate documentation.
    AfinnFinancialMarketNews,
    /// ML-SentiCon. Values are the `.pol` field, and stay *strings*.
    Senticon,
    /// The CLiPS Pattern project's lexicons. Values are the `.polarity` field,
    /// and stay *strings*.
    Pattern,
}

impl VocabularyKind {
    /// Every family, in `languageFiles` order.
    pub const ALL: [Self; 4] = [
        Self::Afinn,
        Self::AfinnFinancialMarketNews,
        Self::Senticon,
        Self::Pattern,
    ];

    /// The string the reference's constructor matches on.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Afinn => "afinn",
            Self::AfinnFinancialMarketNews => "afinnFinancialMarketNews",
            Self::Senticon => "senticon",
            Self::Pattern => "pattern",
        }
    }

    /// The languages this family has a vocabulary for, in table order.
    #[must_use]
    pub fn languages(self) -> Vec<&'static str> {
        data::SOURCES
            .iter()
            .filter(|s| s.kind == self)
            .map(|s| s.language)
            .collect()
    }
}

impl std::fmt::Display for VocabularyKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A language the reference has at least one vocabulary for.
///
/// The constructor takes a `&str`, so this is a convenience rather than a gate:
/// unsupported names still fail with the reference's own message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[expect(missing_docs, reason = "each variant is its own English name")]
pub enum Language {
    English,
    Spanish,
    Portuguese,
    Galician,
    Catalan,
    Basque,
    Dutch,
    Italian,
    French,
    German,
}

impl Language {
    /// Every language, alphabetically.
    pub const ALL: [Self; 10] = [
        Self::Basque,
        Self::Catalan,
        Self::Dutch,
        Self::English,
        Self::French,
        Self::Galician,
        Self::German,
        Self::Italian,
        Self::Portuguese,
        Self::Spanish,
    ];

    /// The string the reference's constructor matches on.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::English => "English",
            Self::Spanish => "Spanish",
            Self::Portuguese => "Portuguese",
            Self::Galician => "Galician",
            Self::Catalan => "Catalan",
            Self::Basque => "Basque",
            Self::Dutch => "Dutch",
            Self::Italian => "Italian",
            Self::French => "French",
            Self::German => "German",
        }
    }
}

impl std::fmt::Display for Language {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Every `(kind, language)` pair the reference's `languageFiles` accepts, in
/// table order.
///
/// ```
/// use verbora_sentiment::{VocabularyKind, supported_pairs};
///
/// assert_eq!(supported_pairs().len(), 14);
/// assert_eq!(supported_pairs()[0], (VocabularyKind::Afinn, "English"));
/// ```
#[must_use]
pub fn supported_pairs() -> Vec<(VocabularyKind, &'static str)> {
    data::SOURCES.iter().map(|s| (s.kind, s.language)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_has_the_reference_s_fourteen_rows() {
        assert_eq!(supported_pairs().len(), 14);
        assert_eq!(
            VocabularyKind::Afinn.languages(),
            ["English", "Spanish", "Portuguese"]
        );
        assert_eq!(
            VocabularyKind::Senticon.languages(),
            ["Spanish", "English", "Galician", "Catalan", "Basque"]
        );
        assert_eq!(
            VocabularyKind::Pattern.languages(),
            ["Dutch", "Italian", "English", "French", "German"]
        );
        assert_eq!(
            VocabularyKind::AfinnFinancialMarketNews.languages(),
            ["English"]
        );
    }

    #[test]
    fn kind_and_language_round_trip_through_their_reference_spellings() {
        for kind in VocabularyKind::ALL {
            assert_eq!(VocabularyKind::from_pattern(kind.as_str()), Some(kind));
        }
        for language in Language::ALL {
            assert_eq!(Language::from_pattern(language.as_str()), Some(language));
        }
        assert_eq!(VocabularyKind::from_pattern("Afinn"), None);
        assert_eq!(Language::from_pattern("english"), None);
    }
}
