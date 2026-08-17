//! Rayon-backed batch analysis, behind the `parallel` feature.
//!
//! # Why this exists
//!
//! [`SentenceAnalyzer`] holds no state beyond one sentence's own tags, and
//! nothing about running its pipeline on one sentence depends on any other
//! sentence, so analyzing many independent sentences is embarrassingly
//! parallel with zero coordination cost between them. [`par_analyze_batch`]
//! is exactly
//!
//! ```text
//! sentences.into_par_iter().map(|tags| {
//!     let mut a = SentenceAnalyzer::new(tags);
//!     a.part();
//!     let sen_type = a.type_of();
//!     AnalyzedSentence { sen_type, subject: a.subject_to_string(), predicate: a.predicate_to_string() }
//! }).collect()
//! ```
//!
//! — the same four calls, in the same order, that `benches/analyzers.rs`'s
//! own `end_to_end` group already measures on one sentence, fanned out over
//! many. [`SentenceAnalyzer::part`] and [`SentenceAnalyzer::type_of`] are not
//! reimplemented here, and cannot drift from the sequential path: this module
//! calls them, it does not re-derive what they do. If you need a different
//! composition in parallel — skipping the render step, say, or keeping the
//! `SentenceAnalyzer` around afterwards — apply the same
//! `into_par_iter().map(...)` pattern at your own call site (see
//! `site/performance/parallelism.md`).
//!
//! # When to reach for it vs. the sequential loop
//!
//! Unlike `remove_diacritics` or `transliterate_ja`, one sentence's worth of
//! work here is often *smaller* than a `rayon` task's own scheduling cost.
//! This crate's own `end_to_end` benchmark (`cargo bench -p verbora-analyzers
//! -- end_to_end`) measures roughly 126 ns for an 8-tag sentence, 389 ns for a
//! 32-tag one, up to roughly 20 µs for a 2048-tag one — only the longer
//! sentences clear the microsecond or so a `rayon` task costs to schedule
//! (`site/performance/parallelism.md`) on their own. This crate's own
//! `par_analyze_batch` Criterion group (`benches/analyzers.rs`) measures the
//! consequence directly at a fixed 32 tags/sentence: a 16-sentence batch is
//! roughly **6x slower** in parallel than the sequential loop, a 256-sentence
//! batch is a modest win, and a 4096-sentence batch wins more clearly — see
//! that group for current numbers. A plain
//! `sentences.into_iter().map(|tags| { .. }).collect()` loop is simpler and
//! usually at least as fast below a few hundred sentences of typical length;
//! reach for [`par_analyze_batch`] once you have hundreds of independent
//! sentences or more to analyze at once — tagging a whole document's worth of
//! sentences from a POS tagger, for example — and measure your own workload
//! rather than assuming the win.
//!
//! # Allocation behaviour
//!
//! One `Vec<AnalyzedSentence>` sized to `sentences.len()` for the output. Per
//! sentence: whatever [`SentenceAnalyzer::part`] and
//! [`SentenceAnalyzer::type_of`] already allocate (an implicit-'You' tag on a
//! verb-initial sentence; nothing otherwise), plus the two owned `String`s
//! [`SentenceAnalyzer::subject_to_string`] and
//! [`SentenceAnalyzer::predicate_to_string`] always allocate — unchanged from
//! calling all four sequentially. No additional buffering, no locking, no
//! per-call thread-pool construction — this uses whichever global `rayon`
//! pool is already installed (or `rayon`'s default one), so pool
//! configuration remains the caller's responsibility, not this crate's.
//!
//! # Order and errors
//!
//! Output order matches input order — `results[i]` is the analysis of
//! `sentences[i]` — via `rayon`'s order-preserving `map` + `collect`. Each
//! sentence carries its own [`SentenceAnalyzer::type_of`] result in
//! [`AnalyzedSentence::sen_type`], exactly as a sequential
//! `sentences.iter().map(...)` loop would produce: one sentence's
//! [`TypeError`] does not abort the others.

use rayon::prelude::*;

use crate::analyzer::{SentenceAnalyzer, TypeError};
use crate::sen_type::SenType;
use crate::tagged::TaggedWord;

/// One sentence's result from running the whole [`SentenceAnalyzer`]
/// pipeline: [`SentenceAnalyzer::part`], then [`SentenceAnalyzer::type_of`],
/// then both renderers.
///
/// Produced by [`par_analyze_batch`]. There is no sequential constructor
/// beyond building this by hand from the four `SentenceAnalyzer` calls it
/// names — this type exists to give the parallel batch a return value, not as
/// a new primitive to keep in sync with anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalyzedSentence {
    /// [`SentenceAnalyzer::type_of`]'s result, popped tag and all.
    pub sen_type: Result<Option<SenType>, TypeError>,
    /// [`SentenceAnalyzer::subject_to_string`]'s result, computed after
    /// `type_of` — so a tag `type_of` popped is already gone from it.
    pub subject: String,
    /// [`SentenceAnalyzer::predicate_to_string`]'s result, computed after
    /// `type_of` for the same reason.
    pub predicate: String,
}

/// Runs the whole [`SentenceAnalyzer`] pipeline over many independent
/// sentences in parallel, preserving input order. Requires the `parallel`
/// feature.
///
/// See the [module documentation](self) for why this exists, when to reach
/// for it over a sequential loop, and what it costs.
///
/// ```
/// use verbora_analyzers::{SenType, TaggedWord as W, par_analyze_batch};
///
/// let sentences = vec![
///     vec![W::new("The", "DT"), W::new("bear", "NN"), W::new("ran", "VB")],
///     vec![W::new("Vote", "VB"), W::new("for", "IN"), W::new("me", "PRP")],
/// ];
/// let results = par_analyze_batch(sentences);
/// assert_eq!(results[0].sen_type, Ok(Some(SenType::Unknown)));
/// assert_eq!(results[0].subject.trim(), "The bear");
/// // Verb-initial: `part` appends an implicit 'You', but the default
/// // `punct()` is empty, so `type_of` immediately pops that very tag back
/// // off to classify the sentence — it never survives to the render step,
/// // exactly as a direct `part(); type_of()` call would behave. `for` and
/// // `me` sit inside the prepositional phrase `preposition_phrases` flags,
/// // so `part` never gives them a subject/predicate slot at all.
/// assert_eq!(results[1].sen_type, Ok(Some(SenType::Command)));
/// assert_eq!(results[1].predicate.trim(), "Vote");
/// ```
#[must_use]
pub fn par_analyze_batch(sentences: Vec<Vec<TaggedWord<'_>>>) -> Vec<AnalyzedSentence> {
    sentences
        .into_par_iter()
        .map(|tags| {
            let mut a = SentenceAnalyzer::new(tags);
            a.part();
            let sen_type = a.type_of();
            AnalyzedSentence {
                sen_type,
                subject: a.subject_to_string(),
                predicate: a.predicate_to_string(),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w<'a>(token: &'a str, pos: &'a str) -> TaggedWord<'a> {
        TaggedWord::new(token, pos)
    }

    /// The sequential composition `par_analyze_batch` fans out, spelled out
    /// exactly the way the parallel path calls it — the comparison a parity
    /// test needs when there is no pre-existing named sequential primitive to
    /// call by name (see the crate's `SentenceAnalyzer` doc: every method
    /// mutates, so there is nothing purer to extract this from).
    fn analyze_sequential(tags: Vec<TaggedWord<'_>>) -> AnalyzedSentence {
        let mut a = SentenceAnalyzer::new(tags);
        a.part();
        let sen_type = a.type_of();
        AnalyzedSentence {
            sen_type,
            subject: a.subject_to_string(),
            predicate: a.predicate_to_string(),
        }
    }

    /// Runs both paths over `sentences` and asserts they agree, sentence by
    /// sentence.
    fn assert_parity(sentences: Vec<Vec<TaggedWord<'_>>>) {
        let sequential: Vec<AnalyzedSentence> =
            sentences.iter().cloned().map(analyze_sequential).collect();
        let parallel = par_analyze_batch(sentences);
        assert_eq!(
            parallel,
            sequential,
            "batch of {} sentences diverged from the sequential loop",
            sequential.len()
        );
    }

    #[test]
    fn empty_input_produces_an_empty_output() {
        assert_parity(Vec::new());
    }

    #[test]
    fn one_sentence() {
        assert_parity(vec![vec![w("The", "DT"), w("bear", "NN"), w("ran", "VB")]]);
        // The empty sentence: `part` still appends an implicit 'You'.
        assert_parity(vec![Vec::new()]);
    }

    #[test]
    fn many_sentences_preserve_order() {
        // The exact sentence shapes `analyzer.rs`'s own tests assert by
        // value: a plain declarative sentence, a verb-initial command, the
        // existential-`there` case that keeps the verb in the subject, and
        // the empty sentence — cycled well past any plausible rayon task
        // count.
        let declarative = vec![
            w("The", "DT"),
            w("angry", "JJ"),
            w("bear", "NN"),
            w("chased", "VB"),
            w("the", "DT"),
            w("frightened", "JJ"),
            w("little", "JJ"),
            w("squirrel", "NN"),
        ];
        let verb_initial = vec![w("Vote", "VB"), w("for", "IN"), w("me", "PRP")];
        let existential = vec![
            w("There", "EX"),
            w("is", "VB"),
            w("a", "DT"),
            w("house", "NN"),
            w("in", "IN"),
            w("the", "DT"),
            w("valley", "DT"),
        ];
        let empty: Vec<TaggedWord<'_>> = Vec::new();

        let base = [declarative, verb_initial, existential, empty];
        let sentences: Vec<Vec<TaggedWord<'_>>> = base.iter().cloned().cycle().take(500).collect();
        assert_parity(sentences);
    }

    #[test]
    fn unicode_and_pathological_inputs() {
        // The exotic tokens `tokens_survive_exotic_text` (analyzer.rs) already
        // exercises, plus the 20,000-tag sentence
        // `very_long_sentences_are_linear_and_correct` uses, scaled down so
        // the test suite stays fast while still spanning several rayon
        // chunks.
        let exotic = [
            "",
            " ",
            "café",
            "Москва",
            "日本語",
            "😀",
            "a😀b",
            "\u{feff}",
        ];
        let exotic_sentence: Vec<TaggedWord<'_>> = exotic.iter().map(|t| w(t, "NN")).collect();

        let n = 4_000;
        let mut long = Vec::with_capacity(n);
        for i in 0..n {
            long.push(TaggedWord::new("w", if i == 1 { "VB" } else { "DT" }));
        }

        assert_parity(vec![exotic_sentence, long]);
    }
}
