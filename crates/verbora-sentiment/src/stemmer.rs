//! The stemmer the analyzer talks to.
//!
//! `SentimentAnalyzer`'s second constructor argument is any object that can
//! stem a word. [`Stemmer`] is that contract, narrowed to the one method the
//! analyzer actually calls, so a caller can supply a Verbora stemmer, a
//! wrapper around someone else's, or a closure-like type of their own.
//!
//! Every stemmer in [`verbora_stemmers`] implements it, so the two crates
//! compose without an adapter:
//!
//! ```
//! use verbora_sentiment::{Language, SentimentAnalyzer, Stemmer, VocabularyKind};
//! use verbora_stemmers::PorterStemmer;
//!
//! let porter = PorterStemmer::new();
//! let analyzer =
//!     SentimentAnalyzer::with_stemmer(Language::English, VocabularyKind::Afinn, porter).unwrap();
//! assert_eq!(analyzer.get_sentiment(["not", "happy"]), Some(-1.5));
//!
//! // …and so does anything else that stems a word.
//! struct Chop;
//! impl Stemmer for Chop {
//!     fn stem<'a>(&self, word: &'a str) -> std::borrow::Cow<'a, str> {
//!         word.get(..4).unwrap_or(word).into()
//!     }
//! }
//! assert!(SentimentAnalyzer::with_stemmer(Language::English, VocabularyKind::Afinn, Chop).is_ok());
//! ```

use std::borrow::Cow;

/// Anything the analyzer can stem a word with.
///
/// The analyzer calls `stem` in two places — once per *piece* of every key when
/// the vocabulary is rebuilt, and once per unmatched token (or span piece) when
/// text is scored — and does nothing else with the object. Returning [`Cow`]
/// rather than `String` matters for the first of those: rebuilding a
/// 24,839-entry vocabulary allocates nothing for the pieces the algorithm
/// leaves alone.
///
/// A stemmer is expected to be **pure**: the same word must stem to the same
/// result whenever it is asked, because the table is built once and queried
/// afterwards, and a stemmer whose answer drifts would make a key reachable at
/// build time and unreachable at scoring time.
pub trait Stemmer {
    /// The stem of `word`, borrowed when the algorithm did not change it.
    fn stem<'a>(&self, word: &'a str) -> Cow<'a, str>;
}

impl<T: Stemmer + ?Sized> Stemmer for &T {
    fn stem<'a>(&self, word: &'a str) -> Cow<'a, str> {
        (**self).stem(word)
    }
}

impl<T: Stemmer + ?Sized> Stemmer for Box<T> {
    fn stem<'a>(&self, word: &'a str) -> Cow<'a, str> {
        (**self).stem(word)
    }
}

impl<T: Stemmer + ?Sized> Stemmer for std::sync::Arc<T> {
    fn stem<'a>(&self, word: &'a str) -> Cow<'a, str> {
        (**self).stem(word)
    }
}

/// The identity stemmer, and the default type parameter of
/// [`SentimentAnalyzer`](crate::SentimentAnalyzer).
///
/// It exists so that `SentimentAnalyzer::without_stemmer` has a concrete type
/// to name for its `None`. Installing it deliberately changes no answer — the
/// analyzer looks a token up unstemmed *before* it stems it, so an identity
/// stem can only repeat a lookup that already missed — but it is not free: it
/// still rebuilds the whole table, which collapses keys that share a lookup
/// form. Prefer `without_stemmer`.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct NoStemmer;

impl Stemmer for NoStemmer {
    fn stem<'a>(&self, word: &'a str) -> Cow<'a, str> {
        Cow::Borrowed(word)
    }
}

/// Bridges [`verbora_stemmers`]'s stemmers, whose `stem` is an inherent
/// method rather than a trait one.
macro_rules! bridge {
    ($($ty:ty),* $(,)?) => {
        $(
            impl Stemmer for $ty {
                fn stem<'a>(&self, word: &'a str) -> Cow<'a, str> {
                    <$ty>::stem(self, word)
                }
            }
        )*
    };
}

bridge!(
    verbora_stemmers::CarryStemmerFr,
    verbora_stemmers::LancasterStemmer,
    verbora_stemmers::PorterStemmer,
    verbora_stemmers::PorterStemmerDe,
    verbora_stemmers::PorterStemmerEs,
    verbora_stemmers::PorterStemmerFa,
    verbora_stemmers::PorterStemmerFr,
    verbora_stemmers::PorterStemmerIt,
    verbora_stemmers::PorterStemmerNl,
    verbora_stemmers::PorterStemmerNo,
    verbora_stemmers::PorterStemmerPt,
    verbora_stemmers::PorterStemmerRu,
    verbora_stemmers::PorterStemmerSv,
    verbora_stemmers::PorterStemmerUk,
    verbora_stemmers::StemmerJa,
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_stemmer_borrows() {
        assert!(matches!(
            NoStemmer.stem("running"),
            Cow::Borrowed("running")
        ));
    }

    #[test]
    fn blanket_impls_forward() {
        let boxed: Box<dyn Stemmer> = Box::new(NoStemmer);
        assert_eq!(boxed.stem("x"), "x");
        let by_ref: &dyn Stemmer = &NoStemmer;
        assert_eq!(by_ref.stem("x"), "x");
        assert_eq!(std::sync::Arc::new(NoStemmer).stem("x"), "x");
    }

    #[test]
    fn verbora_stemmers_bridge() {
        assert_eq!(
            verbora_stemmers::PorterStemmer::new().stem("running"),
            "run"
        );
    }
}
