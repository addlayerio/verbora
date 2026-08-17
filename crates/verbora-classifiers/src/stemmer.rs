//! The tokenizing stemmer a classifier turns text into features with.
//!
//! The reference's base `Classifier` stores `this.stemmer = stemmer || PorterStemmer`
//! and calls exactly one method on it, `tokenizeAndStem(text, keepStops)`. That
//! single call is the whole boundary between the classifier and the rest of the
//! library — and it is also where the classifiers inherit the library's
//! process-global mutable stop-word list, so two classifiers built at different
//! times in the same process can tokenise the same document differently.
//!
//! Note that this is a *different* contract from [`verbora_core::Stemmer`],
//! which only reduces one already-tokenised word.

use std::sync::Arc;

use verbora_stemmers::{PorterStemmer, TokenizeAndStem};

/// Turns a document into the token stream a classifier learns over.
///
/// Implement this to plug in a stemmer of your own; [`StemmerOf`] adapts
/// anything from `verbora-stemmers`.
pub trait Stemmer {
    /// Tokenises `text` and stems each token, dropping stop words unless
    /// `keep_stops`.
    fn tokenize_and_stem(&self, text: &str, keep_stops: bool) -> Vec<String>;
}

/// Adapts any `verbora-stemmers` stemmer to [`Stemmer`].
///
/// ```
/// use verbora_classifiers::{Stemmer, StemmerOf};
/// use verbora_stemmers::PorterStemmerFr;
///
/// let french = StemmerOf(PorterStemmerFr::new());
/// assert!(!french.tokenize_and_stem("les chiens courent", false).is_empty());
/// ```
#[derive(Debug, Clone, Copy, Default)]
pub struct StemmerOf<T>(pub T);

impl<T: TokenizeAndStem> Stemmer for StemmerOf<T> {
    fn tokenize_and_stem(&self, text: &str, keep_stops: bool) -> Vec<String> {
        self.0.tokenize_and_stem(text, keep_stops)
    }
}

/// The stemmer a classifier uses when none is supplied: English Porter.
///
/// Mirrors `stemmer || PorterStemmer`, which — because it is a truthiness test —
/// also selects Porter for `null`, `0`, `false` and `''`.
pub fn default_stemmer() -> Arc<dyn Stemmer + Send + Sync> {
    Arc::new(StemmerOf(PorterStemmer::new()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_stemmer_drops_stop_words() {
        let s = default_stemmer();
        assert_eq!(s.tokenize_and_stem("the a of", false), Vec::<String>::new());
        assert_eq!(
            s.tokenize_and_stem("my unit-tests failed.", false),
            vec!["unit-test".to_owned(), "fail".to_owned()]
        );
    }

    #[test]
    fn keep_stops_retains_them() {
        let s = default_stemmer();
        assert!(!s.tokenize_and_stem("the a of", true).is_empty());
    }
}
