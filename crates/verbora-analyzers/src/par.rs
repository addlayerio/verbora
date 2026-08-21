use rayon::prelude::*;

use crate::analysis::{SentenceAnalysis, analyze};
use crate::word::TaggedWord;

/// Analyses many independent sentences in parallel, preserving input order.
/// Requires the `parallel` feature.
///
/// Exactly `sentences.par_iter().map(|s| analyze(s)).collect()`. It **calls**
/// [`analyze`] rather than re-deriving what it does, so the parallel and
/// sequential paths cannot drift: every rule, every stage and every edge case
/// documented for [`analyze`] holds here unchanged, per sentence.
///
/// ```
/// use verbora_analyzers::{SentenceType, TaggedWord as W, par_analyze_batch};
///
/// let sentences = vec![
///     vec![W::new("The", "DT"), W::new("bear", "NN"), W::new("ran", "VBD"), W::new(".", ".")],
///     vec![W::new("Vote", "VB"), W::new("for", "IN"), W::new("me", "PRP"), W::new("!", ".")],
/// ];
/// let results = par_analyze_batch(&sentences);
///
/// assert_eq!(results[0].sentence_type(), Some(SentenceType::Declarative));
/// assert_eq!(results[0].subject_to_string(), "The bear");
/// assert_eq!(results[1].sentence_type(), Some(SentenceType::Imperative));
/// ```
///
/// # When to reach for it, and when not to
///
/// One sentence's analysis is small — a handful of linear passes over a handful
/// of words — and can easily be **smaller than the cost of scheduling a `rayon`
/// task**. That makes this the wrong default: a plain
///
/// ```
/// # use verbora_analyzers::{TaggedWord as W, analyze};
/// # let sentences: Vec<Vec<W<'_>>> = vec![vec![W::new("It", "PRP"), W::new("rained", "VBD")]];
/// let results: Vec<_> = sentences.iter().map(|s| analyze(s)).collect();
/// # assert_eq!(results.len(), 1);
/// ```
///
/// loop is simpler, needs no feature flag, and is usually at least as fast for
/// small batches of typical sentences. Reach for `par_analyze_batch` when you
/// have hundreds of independent sentences or more to analyse at once — a whole
/// document's worth of tagger output, say — and measure your own workload
/// rather than assuming the win.
///
/// **This crate publishes no timing figures for either path.** The sequential
/// and parallel groups in `benches/analyzers.rs` measure exactly this
/// comparison, but no run against the current implementation exists, so the
/// break-even batch size above is stated as a shape, not a number.
///
/// # Order, borrowing and allocation
///
/// * **Order.** `results[i]` is the analysis of `sentences[i]`, via `rayon`'s
///   order-preserving `map` + `collect`. Analysing one sentence cannot observe
///   or affect another.
/// * **Borrowing.** The sentences are borrowed, not moved, and each returned
///   [`SentenceAnalysis`] borrows its own sentence — so the caller keeps its
///   tagger output and the results index straight back into it.
/// * **Allocation.** One `Vec<SentenceAnalysis>` sized to `sentences.len()`,
///   plus whatever [`analyze`] allocates per sentence (one vector of
///   [`Role`](crate::Role), and one of index ranges when the sentence has a
///   prepositional phrase). No buffering, no locking, and no per-call thread
///   pool: this uses whichever global `rayon` pool is installed, so pool
///   configuration stays the caller's choice.
#[cfg_attr(docsrs, doc(cfg(feature = "parallel")))]
#[must_use]
pub fn par_analyze_batch<'w>(sentences: &'w [Vec<TaggedWord<'w>>]) -> Vec<SentenceAnalysis<'w>> {
    sentences.par_iter().map(|words| analyze(words)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w<'a>(token: &'a str, tag: &'a str) -> TaggedWord<'a> {
        TaggedWord::new(token, tag)
    }

    /// The parallel path must agree with the sequential one sentence for
    /// sentence — it is the same call, so any divergence is a scheduling or
    /// ordering bug rather than a rule disagreement.
    fn assert_parity(sentences: &[Vec<TaggedWord<'_>>]) {
        let sequential: Vec<SentenceAnalysis<'_>> = sentences.iter().map(|s| analyze(s)).collect();
        let parallel = par_analyze_batch(sentences);
        assert_eq!(
            parallel.len(),
            sequential.len(),
            "batch of {} sentences changed length",
            sequential.len()
        );
        for (i, (par, seq)) in parallel.iter().zip(&sequential).enumerate() {
            assert_eq!(par, seq, "sentence {i} diverged from the sequential loop");
        }
    }

    #[test]
    fn empty_input_produces_an_empty_output() {
        assert_parity(&[]);
        assert!(par_analyze_batch(&[]).is_empty());
    }

    #[test]
    fn a_single_sentence_and_an_empty_one() {
        assert_parity(&[vec![w("The", "DT"), w("bear", "NN"), w("ran", "VBD")]]);
        assert_parity(&[Vec::new()]);
    }

    #[test]
    fn many_sentences_preserve_order() {
        let declarative = vec![
            w("The", "DT"),
            w("angry", "JJ"),
            w("bear", "NN"),
            w("chased", "VBD"),
            w("the", "DT"),
            w("squirrel", "NN"),
            w(".", "."),
        ];
        let imperative = vec![w("Vote", "VB"), w("for", "IN"), w("me", "PRP"), w("!", ".")];
        let existential = vec![
            w("There", "EX"),
            w("is", "VBZ"),
            w("a", "DT"),
            w("house", "NN"),
            w("in", "IN"),
            w("the", "DT"),
            w("valley", "NN"),
            w(".", "."),
        ];
        let empty: Vec<TaggedWord<'_>> = Vec::new();

        let base = [declarative, imperative, existential, empty];
        let sentences: Vec<Vec<TaggedWord<'_>>> = base.iter().cloned().cycle().take(500).collect();
        assert_parity(&sentences);

        // Order is positional, so every fourth result must be the same shape.
        let results = par_analyze_batch(&sentences);
        for (i, analysis) in results.iter().enumerate() {
            assert_eq!(
                analysis.roles().len(),
                sentences[i].len(),
                "sentence {i} lost a role"
            );
        }
    }

    #[test]
    fn unicode_and_long_sentences_survive_the_fan_out() {
        let exotic: Vec<TaggedWord<'_>> = [
            "",
            " ",
            "café",
            "cafe\u{0301}",
            "Москва",
            "日本語",
            "😀",
            "a😀b",
            "\u{feff}",
        ]
        .iter()
        .map(|t| w(t, "NN"))
        .collect();

        let n = 4_000;
        let long: Vec<TaggedWord<'_>> = (0..n)
            .map(|i| TaggedWord::new("w", if i == 1 { "VBD" } else { "DT" }))
            .collect();

        assert_parity(&[exotic, long]);
    }
}
