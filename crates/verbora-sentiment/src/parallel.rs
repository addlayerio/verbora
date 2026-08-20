//! Rayon-backed batch scoring, behind the `parallel` feature.
//!
//! # Why this exists
//!
//! A caller scoring a large, independent corpus (one score per review, per
//! ticket, per comment) pays [`SentimentAnalyzer::get_sentiment`] once per
//! document, serially, on one core. The analyzer itself needs no change to fan
//! that out: it borrows its vocabulary and negation list read-only, so the
//! exact same [`SentimentAnalyzer::get_sentiment`] can run from many threads at
//! once against one shared analyzer. [`SentimentAnalyzer::par_get_sentiment_batch`]
//! is that fan-out, and nothing else — its body is a `par_iter().map(...)`
//! over the sequential method, so any change to the scoring loop
//! ([`Contributions`](crate::Contributions), [`SentimentAnalyzer::score`])
//! is automatically reflected here with no second implementation to keep in
//! sync.
//!
//! # When to reach for it instead of the sequential loop
//!
//! `UNMEASURED`, in both directions. This module used to publish a per-token
//! throughput for the sequential path and a crossover rule of thumb for this
//! one; both are withdrawn. The throughput was quoted from the crate
//! documentation, which had already withdrawn it — the lookup-form and
//! span-matching work changed the scoring loop underneath it — and the
//! crossover was drawn from the same campaign. Neither may be restated until
//! `cargo bench -p verbora-sentiment` has run again on settled code.
//!
//! What can be said without a measurement is the shape of the trade: this
//! method adds Rayon's fork-join scheduling to work that is otherwise a tight
//! per-document loop, so there is a batch size and a document length below
//! which the sequential loop wins. Where that boundary sits is exactly the
//! unmeasured part. The `par_batch` Criterion group in `benches/sentiment.rs`
//! compares `docs.iter().map(get_sentiment)` against this method across
//! several `(document count, tokens per document)` combinations and is what
//! the next campaign should answer it with.
//!
//! If you are scoring one document, or you already have your own thread pool
//! and fan-out strategy, call [`SentimentAnalyzer::get_sentiment`] directly:
//! this method would only add Rayon's scheduling on top for no benefit.
//!
//! # Cost and allocation behaviour
//!
//! One `Vec<Option<f64>>` is allocated for the output, sized to `docs.len()` up
//! front, and nothing else is allocated by this method itself — each
//! document is scored by the same [`SentimentAnalyzer::get_sentiment`] that
//! the sequential path uses, with the same allocation behaviour described
//! there (none, for the common case of already-lowercase tokens). Work is
//! split across Rayon's global thread pool, which this crate never
//! configures; callers who need a bounded pool should install one with
//! [`rayon::ThreadPoolBuilder`] and call this method from within
//! [`rayon::ThreadPool::install`].
//!
//! Order is preserved: Rayon's `map` + `collect` on an indexed producer
//! (`&[D]`) always yields results in input order, so `out[i]` is the score of
//! `docs[i]`, exactly as `docs.iter().map(...).collect()` would produce.

use rayon::prelude::*;

use crate::analyzer::SentimentAnalyzer;
use crate::stemmer::Stemmer;

impl<S: Stemmer> SentimentAnalyzer<S> {
    /// Scores every document in `docs` in parallel, one
    /// [`get_sentiment`](Self::get_sentiment) call per document, preserving
    /// input order.
    ///
    /// This is a thin wrapper — see the module documentation for why it
    /// exists, when it is worth reaching for over the sequential loop, and
    /// what it costs. The scoring logic itself lives nowhere but
    /// [`get_sentiment`](Self::get_sentiment); this method adds no rules of
    /// its own.
    ///
    /// `docs` is a slice rather than an owned collection so the caller keeps
    /// ownership of their documents — nothing here needs to consume or clone
    /// them. Each document, `&D`, must itself be an iterator of `AsRef<str>`
    /// tokens, matching [`get_sentiment`](Self::get_sentiment)'s own bound:
    /// `&[Vec<String>]`, `&[Vec<&str>]`, `&[[&str; N]]` and similar all work.
    ///
    /// # Panics
    ///
    /// Never on its own account. A panic inside a stemmer or a pathological
    /// `AsRef<str>` implementation propagates out of the offending worker
    /// thread the same way it would out of a direct call, per Rayon's own
    /// panic-propagation rules.
    ///
    /// ```
    /// use verbora_sentiment::{Language, SentimentAnalyzer, VocabularyKind};
    ///
    /// let analyzer =
    ///     SentimentAnalyzer::without_stemmer(Language::English, VocabularyKind::Afinn).unwrap();
    /// let docs = vec![vec!["good"], vec!["not", "happy"], vec![]];
    /// let scores = analyzer.par_get_sentiment_batch(&docs);
    /// assert_eq!(scores[0], Some(3.0));
    /// assert_eq!(scores[1], Some(-1.5));
    /// // The third document has no tokens, so it has no mean.
    /// assert_eq!(scores[2], None);
    /// ```
    pub fn par_get_sentiment_batch<'d, D>(&self, docs: &'d [D]) -> Vec<Option<f64>>
    where
        S: Sync,
        D: Sync,
        &'d D: IntoIterator,
        <&'d D as IntoIterator>::Item: AsRef<str>,
    {
        docs.par_iter().map(|doc| self.get_sentiment(doc)).collect()
    }
}

#[cfg(test)]
mod tests {
    use verbora_stemmers::PorterStemmer;

    use crate::{Language, SentimentAnalyzer, VocabularyKind};

    fn afinn_en() -> SentimentAnalyzer {
        SentimentAnalyzer::without_stemmer(Language::English, VocabularyKind::Afinn).unwrap()
    }

    #[test]
    fn matches_the_sequential_loop_on_a_small_batch() {
        let a = afinn_en();
        let docs = vec![
            vec!["good"],
            vec!["not", "good"],
            vec!["not", "good", "good"],
        ];
        let sequential: Vec<Option<f64>> = docs.iter().map(|d| a.get_sentiment(d)).collect();
        let parallel = a.par_get_sentiment_batch(&docs);
        assert_eq!(sequential, parallel);
    }

    #[test]
    fn empty_batch_is_an_empty_vec() {
        let a = afinn_en();
        let docs: Vec<Vec<&str>> = vec![];
        assert_eq!(a.par_get_sentiment_batch(&docs), Vec::<Option<f64>>::new());
    }

    #[test]
    fn works_with_a_stemmer_supplied() {
        // `S` must be `Sync`, not just `NoStemmer`'s trivial case.
        let a = SentimentAnalyzer::with_stemmer(
            Language::English,
            VocabularyKind::Afinn,
            PorterStemmer::new(),
        )
        .unwrap();
        let docs = vec![vec!["running", "badly"], vec!["stemmed", "greatness"]];
        let sequential: Vec<Option<f64>> = docs.iter().map(|d| a.get_sentiment(d)).collect();
        let parallel = a.par_get_sentiment_batch(&docs);
        assert_eq!(sequential, parallel);
    }
}
