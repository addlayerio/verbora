//! Verbora vs. a real, pinned third-party Rust competitor — lexicon-based
//! sentiment scoring (`sentiment` 0.1.1, mount-research).
//!
//! One group, `sentiment_score_document`: **given a document as text, produce
//! its mean polarity.** That is the whole overlap between the two crates, and
//! it is deliberately the *only* thing measured here.
//!
//! # Read this before reading a number out of this file
//!
//! `docs/COMPETITIVE_BENCHMARKS.md` §1.14 marks this competitor `Partial` for
//! algorithmic equivalence and **`No`** for benchmarked, because it diverges
//! from Verbora three ways at once — older lexicon, no negation handling, its
//! own non-swappable tokenizer — "with no shared input domain where all three
//! become moot". That reasoning is right about arbitrary text. It is wrong
//! about *this* text: the three divergences turn out to be simultaneously
//! narrowable, and the corpus below is the narrowing. Each one is closed by
//! construction here and re-proved by assertion in
//! `../tests/sentiment_correctness.rs`, which runs the identical corpus
//! through both crates and fails if they ever disagree.
//!
//! **This file therefore runs ahead of the matrix.** §1.14's `No` verdict and
//! its "no `sentiment` group exists in `rust-competitors/benches/`" sentence
//! both need amending to `Selected cases`, citing this narrowed domain, before
//! any figure produced here is published. That is documentation debt, recorded
//! rather than quietly created.
//!
//! ## Divergence 1 — the lexicon, closed by intersecting the two tables
//!
//! `sentiment` embeds AFINN-111 (`src/afinn.json`, 2,462 entries, parsed once
//! behind `lazy_static`). Verbora ships AFINN-165 (3,382 entries) and offers
//! no public constructor for a caller-supplied `Vocabulary`, so "configure
//! them to match" is not available in either direction. Reading both tables
//! head to head instead:
//!
//! | | |
//! |---|---|
//! | keys in AFINN-111 but not AFINN-165 | **0** — it is a strict subset |
//! | shared keys with different polarity | **4** — `damn`, `exasperated`, `futile`, `irresponsible` |
//! | keys in AFINN-165 only | 871 |
//!
//! So the honest fair domain exists and is large: the **2,438** single-token,
//! lowercase-ASCII, non-negation keys on which the two tables agree exactly.
//! `sentiment_corpus::SCORING` is drawn from it. Comparing a 2,462-word table
//! against a 3,382-word one on arbitrary prose would be reporting a lexicon
//! difference as a speed difference; on this corpus there is no lexicon
//! difference left to report.
//!
//! ## Divergence 2 — negation, closed by excluding four words
//!
//! Verbora's English negation list is exactly `["not", "no", "never",
//! "neither"]` and one hit flips the sign for the rest of the input.
//! `sentiment` has no negation rule at all — its own `test_positivity` asserts
//! that "I do not like jam tarts" scores **+2**. Neither
//! `sentiment_corpus::SCORING` nor `sentiment_corpus::FILLER` contains any of
//! the four words, so the rule never fires and both sides compute a plain sum.
//!
//! ## Divergence 3 — the tokenizer, closed by choosing the input alphabet
//!
//! `sentiment::tokenize_with_no_punctuation` is fused into its API and cannot
//! be swapped: replace `[^a-zA-Z0 -]+` with a space, collapse runs of two or
//! more spaces, lowercase, `split(" ")`. Verbora takes tokens, and the
//! composition its own documentation shows is `WordTokenizer.tokens(text)`
//! piped into `get_sentiment`. On lowercase ASCII words joined by exactly one
//! space — which is all `sentiment_corpus::document` ever builds — the two
//! produce the *same token list*, so both sides tokenize the same bytes into
//! the same tokens and divide by the same denominator. (They would not on
//! punctuation, digits, accents or hyphens: `sentiment` keeps `-` and, through
//! a quirk of its character class, the digit `0` while stripping `1`-`9`.)
//!
//! Verbora's row therefore includes tokenization inside the timed region, the
//! same as its competitor's does. Charging one side for tokenizing and not the
//! other is the "excluding real costs from only one implementation" that
//! `AGENTS.md` § *Cross-Implementation Benchmark Fairness* forbids.
//!
//! # What is NOT narrowed away, and must be published beside the numbers
//!
//! Two costs are structural to `sentiment`'s published API and stay inside the
//! timed region, because a caller cannot avoid them either:
//!
//! * **Four `Regex`es compiled per call.** `analyze(phrase)` calls
//!   `negativity(phrase.clone())` and `positivity(phrase.clone())`, and each
//!   of those calls `tokenize_with_no_punctuation`, which does
//!   `Regex::new(...).unwrap()` twice. So one `analyze` tokenizes the document
//!   **twice** and constructs **four** regular expressions. This is a fixed
//!   per-call constant, not a per-token cost: at `n = 4` tokens it is
//!   essentially the entire measurement, and by `n = 1024` it has amortized.
//!   That is the reason this group sweeps five sizes rather than picking one —
//!   the constant and the slope must both be visible, or the row is a
//!   Rorschach test.
//! * **Two `Vec<String>` of matched words.** `Analysis` always carries
//!   `positive.words` and `negative.words`; there is no score-only entry
//!   point. Verbora's `get_sentiment` returns `Option<f64>` and allocates
//!   nothing per match.
//!
//! And one is structural to Verbora and likewise stays in: its scoring loop
//! carries the negation state and probes for multi-token phrase keys on every
//! token, even on a corpus where neither can ever match. That capability costs
//! something per token and the corpus does not refund it.
//!
//! Neither list is a caveat to be read as "so the loser doesn't really lose".
//! They are what the two crates *are*.
//!
//! # Not benchmarked, and why
//!
//! * **`vader-sentimental`** — §1.14 already excludes it, and this file agrees:
//!   VADER is a different lexicon *and* a different scoring algorithm
//!   (intensifiers, capitalisation and punctuation emphasis, degree
//!   modifiers). There is no input domain that makes those moot the way the
//!   three divergences above are made moot, because they are not divergences
//!   in the data — they are extra rules with no Verbora counterpart.
//! * **Verbora's other thirteen vocabularies** (senticon, pattern; Spanish,
//!   Portuguese, German, Dutch, ...). §1.14 records `NO FAIR COMPETITOR FOUND`
//!   for both families, and nothing found since changes that: no Rust crate
//!   embeds ML-SentiCon or the CLiPS Pattern lexicon. Their timing lives in
//!   `crates/verbora-sentiment/benches/sentiment.rs`, which measures all three
//!   families with no competitor row.
//! * **Stemmed analyzers.** `SentimentAnalyzer::with_stemmer` re-stems the
//!   whole vocabulary and stems every token; `sentiment` has no stemming step
//!   to compare against, so it would be a one-sided workload. Measured
//!   in-workspace instead (that bench's `afinn+porter` row).

use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};

use verbora_sentiment::{Language, SentimentAnalyzer, VocabularyKind};
use verbora_tokenizers::{BorrowingTokenizer, WordTokenizer};

// The corpus itself lives in `competitive_rust::sentiment_corpus`, not in this
// file, because `../tests/sentiment_correctness.rs` has to assert its fairness
// properties against the *same* strings this benchmark measures. A corpus
// copied into both files would be two corpora, and the test would go on
// passing about text the benchmark no longer uses. See that module's own doc
// comment for the word lists and the five properties each entry satisfies.
use competitive_rust::sentiment_corpus::{SIZES, document};

/// Text in, one number out — the question both crates answer.
///
/// `WordTokenizer` piped into `get_sentiment` is the composition
/// `verbora-sentiment`'s own crate documentation shows, not an adapter written
/// for this benchmark.
fn verbora_score(analyzer: &SentimentAnalyzer, text: &str) -> Option<f64> {
    analyzer.get_sentiment(WordTokenizer.tokens(text))
}

fn bench_score_document(c: &mut Criterion) {
    let analyzer = SentimentAnalyzer::without_stemmer(Language::English, VocabularyKind::Afinn)
        .expect("English AFINN is a shipped pair");

    let mut g = c.benchmark_group("sentiment_score_document");
    for n in SIZES {
        let text = document(n);
        // Tokens, not bytes: both implementations iterate tokens, and the
        // in-workspace `crates/verbora-sentiment/benches/sentiment.rs` reports
        // its own throughput in tokens for the same reason.
        g.throughput(Throughput::Elements(n as u64));

        g.bench_with_input(BenchmarkId::new("verbora", n), &text, |b, text| {
            b.iter(|| black_box(verbora_score(&analyzer, black_box(text))));
        });
        // `analyze` takes the phrase **by value**. A caller who does not
        // already own its text pays this `to_owned`, so it is inside the timed
        // region — but it is a single memcpy of at most ~5 kB and is dwarfed
        // by the two tokenizations and four `Regex` constructions `analyze`
        // performs internally on every call. `.comparative` is the field that
        // corresponds to Verbora's `get_sentiment`: both are the polarity sum
        // divided by the token count (see `../tests/sentiment_correctness.rs`,
        // which asserts the two numbers are equal on this corpus, not merely
        // analogous).
        g.bench_with_input(BenchmarkId::new("sentiment", n), &text, |b, text| {
            b.iter(|| black_box(sentiment::analyze(black_box(text).to_owned()).comparative));
        });
    }
    g.finish();
}

criterion_group!(benches, bench_score_document);
criterion_main!(benches);

// CORRECTNESS BEFORE PERFORMANCE: see `../tests/sentiment_correctness.rs`, not
// a `#[cfg(test)] mod` in this file. A Criterion `[[bench]]` target compiles
// with `harness = false`, which replaces the standard libtest runner with
// `criterion_main!`'s own `fn main` — an in-file `#[test]` here would be dead
// code that `cargo test` never invokes. That file is load-bearing for this
// module in a way it is not for most: every fairness claim the doc comment
// above makes about the corpus is an assertion there, so a corpus edit that
// broke one would fail the suite instead of silently making the benchmark
// unfair.
