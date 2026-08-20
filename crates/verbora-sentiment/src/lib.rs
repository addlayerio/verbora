//! Lexicon-based sentiment analysis over fourteen vocabularies in ten
//! languages, drawn from three lexicon projects — AFINN, ML-SentiCon and the
//! CLiPS Pattern project.
//!
//! ```
//! use verbora_sentiment::{Language, SentimentAnalyzer, VocabularyKind};
//!
//! let analyzer = SentimentAnalyzer::without_stemmer(Language::English, VocabularyKind::Afinn)?;
//! assert_eq!(analyzer.get_sentiment(["good"]), Some(3.0));
//! assert_eq!(analyzer.get_sentiment(["not", "happy"]), Some(-1.5));
//! // The mean polarity of no text does not exist, and is reported as absent.
//! assert_eq!(analyzer.get_sentiment(Vec::<&str>::new()), None);
//! # Ok::<(), verbora_sentiment::UnsupportedPair>(())
//! ```
//!
//! # Iterator first
//!
//! [`SentimentAnalyzer::contributions`] is the primitive: it takes any iterator
//! of string-like tokens, yields one addend per scored unit, and materialises
//! nothing beyond a bounded lookahead. [`score`](SentimentAnalyzer::score) and
//! [`get_sentiment`](SentimentAnalyzer::get_sentiment) are folds over it, so a
//! tokenizer pipes straight in and a document is never collected into a `Vec`:
//!
//! ```
//! use verbora_sentiment::{Language, SentimentAnalyzer, VocabularyKind};
//! use verbora_tokenizers::{BorrowingTokenizer, WordTokenizer};
//!
//! let analyzer =
//!     SentimentAnalyzer::without_stemmer(Language::English, VocabularyKind::Afinn).unwrap();
//! let tokens = WordTokenizer.tokens("This is not a good day.");
//! // Six tokens; `not` flips `good`, so the total is -3 over 6.
//! assert_eq!(analyzer.get_sentiment(tokens), Some(-0.5));
//! ```
//!
//! # The four things this crate exists to get right
//!
//! **A lexicon key is text, not a token.** The tables spell entries `cover-up`,
//! `bad luck`, `son-of-a-bitch`, `Abfall` — and a UAX #29 token stream contains
//! none of those. Keys and tokens are therefore both reduced to a **lookup
//! form** (word pieces, lowercased, joined by one space) before they meet, and
//! the scoring loop matches the *longest span of tokens* that forms a key,
//! counting it as one unit. Without that, 14,273 of the 75,803 shipped entries
//! — every entry [`WordTokenizer`](verbora_tokenizers::WordTokenizer) cuts
//! into two or more pieces — are unreachable dead weight, and several invert
//! outright: `non-approved` (-2) scores +1 as `non` + `approved`, and
//! `son-of-a-bitch` (-5) scores -1.25 as four tokens.
//!
//! **The lookup form is derived once and used by both sides.** The table's
//! keys and the scoring loop's probes come from the same two functions over
//! the same pieces, because the alternative is two spellings of one rule that
//! stop agreeing — which is what happened when a stemmed table was indexed by
//! re-segmenting the stemmer's output and probed with the stemmer's output
//! verbatim: 228 keys across the fourteen vocabularies and the sixteen
//! installable stemmers scored some other entry's polarity or none.
//! `tests/reachability.rs` walks every key of every table through the
//! pipeline and asserts what it scores; `tests/key_derivation.rs` does it
//! again for every stemmer. See [`Vocabulary`].
//!
//! **Negation is sticky, and the lexicon outranks it.** One negation word flips
//! the sign for the *rest of the input* — there is no window, punctuation does
//! not reset it, and the negation word itself is never scored:
//! `["not","happy","happy"]` is -2. But the span scan runs first, so a phrase
//! the lexicon actually publishes wins: AFINN-165 scores `not good` at -2 and
//! `no fun` at -3, and those curated values are used rather than the heuristic's
//! guess. See [`SentimentAnalyzer`].
//!
//! **The sum is left to right, and no result is `NaN`.** [`Score::sum`] is
//! accumulated in `f64` strictly in token order and divided exactly once at the
//! end; reordering the accumulation changes the last bits, and the lexicon
//! values are dense enough for that to show. Where the division has no answer —
//! nothing was scored, or the caller supplied a denominator of zero — the
//! result is [`None`] rather than a `NaN`. Every shipped polarity is a finite
//! decimal (`tests/reachability.rs` enumerates all 75,803 of them), so no
//! arithmetic in the scoring loop can manufacture one either.
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
//! # How the lexicons ship, and why
//!
//! Three options were on the table: embed the source JSON and parse it, read it
//! from disk at run time, or embed a prebuilt index. The third won.
//!
//! The published lexicon files total ~7.5 MB, of which `wordnet_id`, `sense`,
//! `subjectivity`, `intensity` and `confidence` are 84% — and no code path in
//! this crate can read any of them. What survives the projection is 75,803
//! `(key, polarity)` pairs: **1,230,918 bytes**, shipped as `key \0 polarity
//! \0` blobs across thirteen `include_bytes!` files (the fourteenth is
//! deliberately empty; see the divergences below). Nothing is read from disk,
//! so the crate works from a single static binary, and nothing needs a build
//! script: the blobs are checked in as data, and their invariants are re-proved
//! by this crate's own tests on every run.
//!
//! Each table is decoded and indexed on first use of that one `(kind,
//! language)` pair and cached for the process, so a program that scores English
//! never pays for the Basque table.
//!
//! **Performance is currently unmeasured.** The figures this section used to
//! publish predate lookup forms and span matching, which changed both the
//! table-build path (every key is now segmented once, when its table is first
//! touched) and the scoring loop (one buffered token, and a lookahead only
//! where a phrase key could start). Rather than carry stale numbers, they are
//! withdrawn until `cargo bench -p verbora-sentiment` is run again on settled
//! code.
//!
//! # Lexicon provenance and licensing
//!
//! The upstream lexicons have separate provenance and their own terms:
//!
//! | Vocabulary | Upstream | Licence located |
//! |---|---|---|
//! | AFINN English | `afinn-165`, Titus Wormer | MIT (the package's own `license` file) |
//! | AFINN Spanish, Portuguese | shipped JSON, no attribution | none found |
//! | senticon (es, en, gl, ca, eu) | ML-SentiCon | none found |
//! | pattern (nl, it, en, fr, de) | CLiPS Pattern project | none found |
//!
//! Anyone shipping this crate commercially should confirm the ML-SentiCon and
//! Pattern terms independently.
//!
//! # Deliberate gaps
//!
//! * **[`VocabularyKind::AfinnFinancialMarketNews`] is empty.** The upstream
//!   package this vocabulary was drawn from exports the data under a different
//!   name than the table asked for, so no data was ever loaded and every score
//!   is 0. The row is carried as an explicit empty table rather than silently
//!   filled in, because filling it in would change published scores; supplying
//!   the real data is a separate, reviewable change.
//! * **Five languages ship no negation list.** Galician, Catalan, Basque,
//!   Italian and French have an empty [`SentimentAnalyzer::negations`], so the
//!   sticky-negation rule never fires for them and their scores are the plain
//!   lexicon sum. The upstream projects published no negation word list for
//!   those languages, and Verbora does not invent one: a guessed list would be
//!   behaviour with no citable basis, and it would change every score in the
//!   language.
//! * **1,488 keys have no word segment.** The Spanish and Portuguese AFINN
//!   tables key on emoji and circled letters, which
//!   [`WordTokenizer`](verbora_tokenizers::WordTokenizer) filters out — it
//!   yields word segments, and an emoji is not one. Those keys are reachable
//!   from a token spelled exactly like them, so pair the analyzer with a
//!   tokenizer that emits symbol runs if your text contains emoji.
//! * **102 keys are shadowed.** English senticon ships both `pitch-black` and
//!   `pitch black`; `pattern`/German ships both `Stolz` and `stolz` with
//!   different polarities. They reduce to one lookup form, and the later entry
//!   in file order wins. [`Vocabulary::len`] still counts both.
//! * **`negations` is immutable.** It is a `&'static [&'static str]`, so one
//!   analyzer cannot alter another's.

#![cfg_attr(doctest, doc = include_str!("../README.md"))]

mod analyzer;
mod data;
#[cfg(feature = "parallel")]
mod parallel;
mod stemmer;
mod vocabulary;

pub use analyzer::{Contributions, Score, SentimentAnalyzer, UnsupportedPair};
pub use stemmer::{NoStemmer, Stemmer};
pub use vocabulary::{Polarity, Vocabulary};

/// A name that spells no shipped [`Language`] or [`VocabularyKind`].
///
/// Returned by the [`FromStr`](std::str::FromStr) implementations, which exist
/// for callers reading a configuration file or a command line. Code that knows
/// which vocabulary it wants names the variant and never sees this.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownName {
    /// The name that was offered, verbatim.
    pub name: String,
    /// The names that would have been accepted, in variant order.
    pub accepted: &'static [&'static str],
}

impl std::fmt::Display for UnknownName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?} is not one of {:?}", self.name, self.accepted)
    }
}

impl std::error::Error for UnknownName {}

/// Which family of lexicon a vocabulary comes from.
///
/// The three families publish their values differently, and that difference
/// survives into [`Polarity`]: AFINN writes integers, ML-SentiCon and Pattern
/// write decimal strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VocabularyKind {
    /// AFINN-165 word lists. Every shipped value is an integer in `-5..=5`.
    Afinn,
    /// AFINN-165 tuned for financial market news — **empty**, see the crate
    /// documentation.
    AfinnFinancialMarketNews,
    /// ML-SentiCon. Values are the project's `pol` field.
    Senticon,
    /// The CLiPS Pattern project's lexicons. Values are its `polarity` field.
    Pattern,
}

impl VocabularyKind {
    /// Every family, in table order.
    pub const ALL: [Self; 4] = [
        Self::Afinn,
        Self::AfinnFinancialMarketNews,
        Self::Senticon,
        Self::Pattern,
    ];

    /// The names [`Self::from_name`] accepts, in [`Self::ALL`] order.
    pub const NAMES: [&'static str; 4] = [
        "afinn",
        "afinn-financial-market-news",
        "senticon",
        "pattern",
    ];

    /// This family's Verbora name: the upstream project's own, lowercased and
    /// hyphenated.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Afinn => "afinn",
            Self::AfinnFinancialMarketNews => "afinn-financial-market-news",
            Self::Senticon => "senticon",
            Self::Pattern => "pattern",
        }
    }

    /// The family with this [`name`](Self::name), or `None`.
    ///
    /// Exact match, no case folding: a name is an identifier here, not text.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.name() == name)
    }

    /// The languages this family has a vocabulary for, in table order.
    #[must_use]
    pub fn languages(self) -> Vec<Language> {
        data::SOURCES
            .iter()
            .filter(|s| s.kind == self)
            .map(|s| s.language)
            .collect()
    }
}

impl std::fmt::Display for VocabularyKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

impl std::str::FromStr for VocabularyKind {
    type Err = UnknownName;

    fn from_str(s: &str) -> Result<Self, UnknownName> {
        Self::from_name(s).ok_or_else(|| UnknownName {
            name: s.to_owned(),
            accepted: &Self::NAMES,
        })
    }
}

/// A language with at least one shipped vocabulary.
///
/// Spelled by its ISO 639-1 code — `en`, `es`, `pt`, … — because that is a
/// published identifier for a language and an English display name is not. The
/// variant name is the English name, so `Debug` still reads as prose.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[expect(missing_docs, reason = "each variant is its own English name")]
pub enum Language {
    Basque,
    Catalan,
    Dutch,
    English,
    French,
    Galician,
    German,
    Italian,
    Portuguese,
    Spanish,
}

impl Language {
    /// Every language, alphabetically by English name — which is also
    /// alphabetical by ISO 639-1 code for every pair but `eu`/`ca`.
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

    /// The codes [`Self::from_code`] accepts, in [`Self::ALL`] order.
    pub const CODES: [&'static str; 10] =
        ["eu", "ca", "nl", "en", "fr", "gl", "de", "it", "pt", "es"];

    /// This language's ISO 639-1 two-letter code.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Basque => "eu",
            Self::Catalan => "ca",
            Self::Dutch => "nl",
            Self::English => "en",
            Self::French => "fr",
            Self::Galician => "gl",
            Self::German => "de",
            Self::Italian => "it",
            Self::Portuguese => "pt",
            Self::Spanish => "es",
        }
    }

    /// The language with this ISO 639-1 [`code`](Self::code), or `None`.
    ///
    /// Exact match: ISO 639-1 codes are lowercase, and accepting `EN` would be
    /// inventing a spelling the standard does not define.
    #[must_use]
    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|l| l.code() == code)
    }
}

impl std::fmt::Display for Language {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.code())
    }
}

impl std::str::FromStr for Language {
    type Err = UnknownName;

    fn from_str(s: &str) -> Result<Self, UnknownName> {
        Self::from_code(s).ok_or_else(|| UnknownName {
            name: s.to_owned(),
            accepted: &Self::CODES,
        })
    }
}

/// Every `(kind, language)` pair that has a shipped vocabulary, in table
/// order.
///
/// ```
/// use verbora_sentiment::{Language, VocabularyKind, supported_pairs};
///
/// assert_eq!(supported_pairs().len(), 14);
/// let pairs: Vec<_> = supported_pairs().collect();
/// assert_eq!(pairs[0], (VocabularyKind::Afinn, Language::English));
/// ```
pub fn supported_pairs() -> impl ExactSizeIterator<Item = (VocabularyKind, Language)> {
    data::SOURCES.iter().map(|s| (s.kind, s.language))
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;

    #[test]
    fn the_table_has_fourteen_rows() {
        assert_eq!(supported_pairs().len(), 14);
        assert_eq!(
            VocabularyKind::Afinn.languages(),
            [Language::English, Language::Spanish, Language::Portuguese]
        );
        assert_eq!(
            VocabularyKind::Senticon.languages(),
            [
                Language::Spanish,
                Language::English,
                Language::Galician,
                Language::Catalan,
                Language::Basque,
            ]
        );
        assert_eq!(
            VocabularyKind::Pattern.languages(),
            [
                Language::Dutch,
                Language::Italian,
                Language::English,
                Language::French,
                Language::German,
            ]
        );
        assert_eq!(
            VocabularyKind::AfinnFinancialMarketNews.languages(),
            [Language::English]
        );
    }

    /// The name tables and the variant lists cannot drift: every variant's own
    /// spelling is at its own index, and every spelling round-trips.
    #[test]
    fn every_name_and_code_round_trips_at_its_own_index() {
        for (i, kind) in VocabularyKind::ALL.into_iter().enumerate() {
            assert_eq!(VocabularyKind::NAMES[i], kind.name());
            assert_eq!(VocabularyKind::from_name(kind.name()), Some(kind));
            assert_eq!(VocabularyKind::from_str(kind.name()), Ok(kind));
            assert_eq!(kind.to_string(), kind.name());
        }
        for (i, language) in Language::ALL.into_iter().enumerate() {
            assert_eq!(Language::CODES[i], language.code());
            assert_eq!(Language::from_code(language.code()), Some(language));
            assert_eq!(Language::from_str(language.code()), Ok(language));
            assert_eq!(language.to_string(), language.code());
        }
        // ISO 639-1 codes are two lowercase ASCII letters, and every one here
        // is distinct.
        let mut codes = Language::CODES;
        codes.sort_unstable();
        codes.windows(2).for_each(|w| assert_ne!(w[0], w[1]));
        for code in Language::CODES {
            assert_eq!(code.len(), 2, "{code}");
            assert!(code.bytes().all(|b| b.is_ascii_lowercase()), "{code}");
        }
    }

    #[test]
    fn an_unknown_name_names_what_was_accepted() {
        let e = Language::from_str("English").unwrap_err();
        assert_eq!(e.name, "English");
        assert_eq!(e.accepted, &Language::CODES);
        assert_eq!(
            e.to_string(),
            r#""English" is not one of ["eu", "ca", "nl", "en", "fr", "gl", "de", "it", "pt", "es"]"#
        );
        assert!(Language::from_str("EN").is_err());
        assert!(VocabularyKind::from_str("Afinn").is_err());
        assert!(VocabularyKind::from_str("afinnFinancialMarketNews").is_err());
        assert_eq!(
            VocabularyKind::from_str("afinn-financial-market-news"),
            Ok(VocabularyKind::AfinnFinancialMarketNews)
        );
    }
}
