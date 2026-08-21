use std::sync::Arc;

use rustc_hash::FxHashMap;
use verbora_stemmers::{PorterStemmer, TokenizeAndStem};

/// A `token → stem` memo owned by one classifier.
///
/// Stemming is the dominant cost of every text-shaped call in this crate —
/// Porter's `stem_token` measures around 400 ns against roughly 25 ns for the
/// whole tokenizing walk of a twelve-word document — and a classifier sees the
/// same few thousand distinct tokens over and over: once per occurrence while
/// training, then again in every probe that shares vocabulary with the corpus.
/// Paying for each distinct token once is therefore worth a map, and the map
/// belongs to the classifier rather than to the stemmer so that every stemmer
/// stays zero-sized and `Sync`.
///
/// # Why it cannot change an answer
///
/// A cached stem is only substituted for a fresh call when the fresh call
/// could not have answered differently: [`Stemmer::tokenize_and_stem_cached`]
/// ignores the cache by default, so a hand-written [`Stemmer`] with internal
/// state keeps its exact behaviour, and the [`StemmerOf`] implementation
/// forwards to `verbora-stemmers`, which consults the cache only for pure
/// stemmers and only *after* the stop-word test — so mutating the
/// process-global stop-word list still takes effect between calls.
#[derive(Debug, Clone, Default)]
pub struct StemCache(FxHashMap<String, String>);

impl StemCache {
    /// An empty memo.
    pub fn new() -> Self {
        Self::default()
    }

    /// How many distinct tokens are memoized.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether nothing is memoized yet.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Forgets every entry. Only ever a performance decision — the next call
    /// re-stems what it needs and gets the same answer.
    pub fn clear(&mut self) {
        self.0.clear();
    }
}

/// Turns a document into the token stream a classifier learns over.
///
/// The tokenizing stemmer a classifier turns text into features with.
///
/// A [`Classifier`](crate::Classifier) calls exactly one method on its
/// stemmer, `tokenize_and_stem(text, keep_stops)`. That single call is the whole
/// boundary between a classifier and the rest of the library — and it is also
/// where a classifier inherits `verbora-stemmers`' process-global mutable
/// stop-word list, so two classifiers built at different moments in one process
/// can tokenise the same document differently. No compatibility stamp can cover
/// that; see [`ArtifactStamp`](crate::ArtifactStamp).
///
/// Note that this contract's unit is a **document**: one call tokenises,
/// drops stop words, and stems what is left. It is deliberately not the
/// single-word stemming contract — one already-tokenised word in, its stem
/// out, no tokenizing and no filtering — which the `verbora-stemmers` types
/// also implement, and which [`StemmerOf`] does *not* adapt: `StemmerOf`
/// forwards to [`TokenizeAndStem`], the document-level one. That single-word
/// trait is named in prose rather than linked because it lives in
/// `verbora-core`, which this crate deliberately does not depend on — see the
/// comment on that absence in this crate's `Cargo.toml`.
///
/// Implement this to plug in a stemmer of your own; [`StemmerOf`] adapts
/// anything from `verbora-stemmers`.
pub trait Stemmer {
    /// Tokenises `text` and stems each token, dropping stop words unless
    /// `keep_stops`.
    fn tokenize_and_stem(&self, text: &str, keep_stops: bool) -> Vec<String>;

    /// [`Self::tokenize_and_stem`], allowed to answer from `cache`.
    ///
    /// The returned token stream must be indistinguishable from
    /// [`Self::tokenize_and_stem`]'s for the same arguments — a classifier
    /// calls whichever of the two is convenient and its observable state may
    /// not depend on which. The default implementation therefore ignores the
    /// cache entirely, which is the right answer for any implementation whose
    /// stemming is stateful: only override it when a stem is a pure function
    /// of its token.
    fn tokenize_and_stem_cached(
        &self,
        text: &str,
        keep_stops: bool,
        cache: &mut StemCache,
    ) -> Vec<String> {
        let _ = cache;
        self.tokenize_and_stem(text, keep_stops)
    }
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

    /// Forwards to `verbora-stemmers`' own cached entry point, which owns the
    /// parity contract: it walks the same tokenizer, applies the same
    /// stop-word and gate tests in the same order, and falls back to a fresh
    /// stem for any stemmer whose `PURE_STEM` is `false` (the Dutch Porter
    /// stemmer's sticky `suffix_e_removed` flag is the one such case).
    fn tokenize_and_stem_cached(
        &self,
        text: &str,
        keep_stops: bool,
        cache: &mut StemCache,
    ) -> Vec<String> {
        self.0
            .tokenize_and_stem_cached(text, keep_stops, &mut cache.0)
    }
}

/// The stemmer a classifier uses when none is supplied: English Porter.
///
/// It is what [`Classifier::new`](crate::Classifier::new) installs and what
/// [`Classifier::restore`](crate::Classifier::restore) demands, so a model
/// trained through [`Classifier::with_stemmer`](crate::Classifier::with_stemmer)
/// must be read back with
/// [`Classifier::restore_with`](crate::Classifier::restore_with).
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
        // U+002D HYPHEN-MINUS is `Word_Break=Other`, so `"unit-tests"` is two
        // words; `"my"` is a stop word.
        assert_eq!(
            s.tokenize_and_stem("my unit-tests failed.", false),
            vec!["unit".to_owned(), "test".to_owned(), "fail".to_owned()]
        );
    }

    #[test]
    fn keep_stops_retains_them() {
        let s = default_stemmer();
        assert!(!s.tokenize_and_stem("the a of", true).is_empty());
    }
}
