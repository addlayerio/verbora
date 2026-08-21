//! `CORRECTNESS BEFORE PERFORMANCE` for `benches/sentiment.rs`.
//!
//! This file is load-bearing for that benchmark in a way most correctness
//! tests are not. `docs/COMPETITIVE_BENCHMARKS.md` §1.14 marks `sentiment`
//! 0.1.1 **not benchmarkable** against Verbora, because the two diverge three
//! ways at once — a different AFINN revision, no negation rule, its own
//! non-swappable tokenizer — "with no shared input domain where all three
//! become moot". The benchmark's entire justification is the claim that such a
//! domain does exist and that
//! [`sentiment_corpus`](competitive_rust::sentiment_corpus) is inside it.
//!
//! Every assertion below is one clause of that claim. If a word is ever added
//! to either corpus list that breaks one, this suite fails rather than the
//! benchmark silently becoming unfair — which is the only reason it is safe
//! for the corpus to be a hand-curated list at all.
//!
//! | Test | Clause it proves |
//! |---|---|
//! | [`corpus_words_are_single_lowercase_ascii_tokens`] | both tokenizers cut the corpus identically |
//! | [`the_two_lexicons_agree_on_every_scoring_word`] | no lexicon-version difference survives on this corpus |
//! | [`neither_lexicon_scores_any_filler_word`] | the miss path is a miss on both sides |
//! | [`no_corpus_word_is_an_english_negation_word`] | Verbora's sticky negation never fires |
//! | [`no_span_matching_happens_on_the_corpus`] | Verbora's phrase scan never swallows a token, so the denominators match |
//! | [`document_scores_agree_at_every_benchmarked_size`] | the two crates compute the *same number* on the text the benchmark times |
//!
//! What this file does **not** assert, deliberately: that the two agree on
//! arbitrary English. They do not, and §1.14 is right about that. `sentiment`
//! scores "I do not like jam tarts" at +2 (its own test says so) where Verbora
//! scores it -2; the four keys `damn`, `exasperated`, `futile` and
//! `irresponsible` carry different polarities in AFINN-111 and AFINN-165; and
//! `sentiment`'s tokenizer keeps hyphens and the digit `0` while dropping
//! `1`-`9`. The benchmark's domain is narrow *because* of those, not in
//! ignorance of them.

use competitive_rust::sentiment_corpus::{FILLER, SCORING, SIZES, all_words, document};
use verbora_sentiment::{Language, SentimentAnalyzer, VocabularyKind};
use verbora_tokenizers::{BorrowingTokenizer, WordTokenizer};

/// Verbora configured the way `benches/sentiment.rs` configures it: English
/// AFINN, no stemmer — the closest available match to what `sentiment` 0.1.1
/// does, which has no stemming step at all.
fn analyzer() -> SentimentAnalyzer {
    SentimentAnalyzer::without_stemmer(Language::English, VocabularyKind::Afinn)
        .expect("English AFINN is a shipped pair")
}

/// `sentiment`'s document score. `analyze().score` is the polarity sum
/// (`positive.score - negative.score`); `.comparative` is that sum over the
/// token count, which is what Verbora's `get_sentiment` returns.
fn sentiment_sum(text: &str) -> f64 {
    f64::from(sentiment::analyze(text.to_owned()).score)
}

fn sentiment_mean(text: &str) -> f64 {
    f64::from(sentiment::analyze(text.to_owned()).comparative)
}

/// Both sides use `f32` somewhere — `sentiment` throughout, Verbora only in
/// the shipped table's decimal text — so exact bit equality is the wrong
/// assertion. Every polarity in play is a small integer and every denominator
/// is at most 1024, so the true values are exactly representable and the only
/// error is `sentiment`'s two `f32` divisions against Verbora's one `f64` one:
/// a couple of ULP at magnitudes near 1, far inside this.
const EPSILON: f64 = 1e-6;

/// Clause: `sentiment`'s regex tokenizer and `WordTokenizer` cut the corpus
/// the same way.
///
/// The Verbora half is direct. The `sentiment` half cannot be called directly
/// — `tokenize_with_no_punctuation` is private — so it is established
/// behaviourally: for a word the lexicon scores, `comparative == score` holds
/// if and only if that crate divided by a token count of exactly one.
#[test]
fn corpus_words_are_single_lowercase_ascii_tokens() {
    for word in all_words() {
        assert!(
            word.chars().all(|c| c.is_ascii_lowercase()),
            "{word:?} is not lowercase ASCII; the two tokenizers only provably \
             agree on lowercase ASCII words joined by single spaces"
        );
        assert_eq!(
            WordTokenizer.tokens(word).count(),
            1,
            "{word:?} is more than one UAX #29 word segment"
        );
    }

    for word in SCORING {
        let analysis = sentiment::analyze((*word).to_owned());
        assert!(
            (f64::from(analysis.comparative) - f64::from(analysis.score)).abs() < EPSILON,
            "{word:?}: sentiment divided by a token count other than 1 \
             (score {}, comparative {})",
            analysis.score,
            analysis.comparative
        );
    }
}

/// Clause: on the benchmark's scoring vocabulary, AFINN-111 and AFINN-165 are
/// the same function.
///
/// This is the assertion that turns "different lexicon versions" from a reason
/// not to benchmark into a reason to choose the corpus carefully. It is
/// checked word by word rather than in aggregate, so a failure names the word.
#[test]
fn the_two_lexicons_agree_on_every_scoring_word() {
    let analyzer = analyzer();
    for word in SCORING {
        let verbora = analyzer.score([word]).sum;
        let theirs = sentiment_sum(word);
        assert!(
            (verbora - theirs).abs() < EPSILON,
            "{word:?}: Verbora (AFINN-165) scores {verbora}, \
             sentiment (AFINN-111) scores {theirs}"
        );
        assert!(
            verbora != 0.0,
            "{word:?} scores 0 — a scoring word that scores nothing exercises \
             the miss path, which is what FILLER is for"
        );
    }
}

/// Clause: the filler vocabulary is a miss on both sides, so the corpus
/// exercises the miss path without either crate quietly scoring something the
/// other does not.
#[test]
fn neither_lexicon_scores_any_filler_word() {
    let analyzer = analyzer();
    for word in FILLER {
        assert!(
            analyzer.vocabulary().get(word).is_none(),
            "{word:?} is in Verbora's AFINN-165 table; it cannot be filler"
        );
        assert!(
            sentiment_sum(word).abs() < EPSILON,
            "{word:?} is scored by sentiment's AFINN-111 table; it cannot be filler"
        );
    }
}

/// Clause: Verbora's sticky negation never fires, so it is not applying a rule
/// its competitor does not have.
///
/// Checked against the analyzer's own published list rather than a copy of it
/// here, so a change to the shipped negation words is caught rather than
/// missed.
#[test]
fn no_corpus_word_is_an_english_negation_word() {
    let analyzer = analyzer();
    let negations = analyzer.negations();
    assert!(
        !negations.is_empty(),
        "English is documented as having a negation list; an empty one would \
         make this test vacuous"
    );
    for word in all_words() {
        assert!(
            !negations.contains(&word),
            "{word:?} is one of Verbora's English negation words {negations:?}; \
             sentiment 0.1.1 implements no negation rule, so the corpus must \
             contain none of them"
        );
    }
}

/// Clause: Verbora's multi-token phrase scan never matches, so one addend is
/// produced per token and the two crates divide by the same denominator.
///
/// Behavioural rather than structural: instead of guessing which keys are
/// multi-token, this asks the scoring loop itself. `contributions` yields one
/// addend per *unit*, and a matched span is one unit covering several tokens —
/// so a count equal to the token count is a proof that no span matched. The
/// second assertion catches the same thing from the other side: with no spans
/// and no negation, scoring the document must equal scoring its tokens
/// separately and adding up.
#[test]
fn no_span_matching_happens_on_the_corpus() {
    let analyzer = analyzer();
    for n in SIZES {
        let text = document(n);
        let tokens: Vec<&str> = WordTokenizer.tokens(&text).collect();
        assert_eq!(tokens.len(), n, "document({n}) is not {n} tokens");

        assert_eq!(
            analyzer.contributions(tokens.iter()).count(),
            n,
            "document({n}): a phrase key matched — some span covered more than \
             one token, so the denominator no longer matches sentiment's"
        );

        let whole = analyzer.score(tokens.iter()).sum;
        let piecewise: f64 = tokens.iter().map(|t| analyzer.score([t]).sum).sum();
        assert!(
            (whole - piecewise).abs() < EPSILON,
            "document({n}): scoring the whole document ({whole}) differs from \
             scoring each token alone and summing ({piecewise}) — some \
             cross-token rule fired"
        );
    }
}

/// The clause all the others exist to support: on the exact strings
/// `benches/sentiment.rs` times, the two crates return the **same number**.
///
/// Sum and mean are both checked. The sum proves the lexicons agreed; the mean
/// proves the denominators did too, since equal sums with equal means force
/// equal token counts.
#[test]
fn document_scores_agree_at_every_benchmarked_size() {
    let analyzer = analyzer();
    for n in SIZES {
        let text = document(n);

        let verbora_sum = analyzer.score(WordTokenizer.tokens(&text)).sum;
        let theirs_sum = sentiment_sum(&text);
        assert!(
            (verbora_sum - theirs_sum).abs() < EPSILON,
            "document({n}): polarity sums differ — Verbora {verbora_sum}, \
             sentiment {theirs_sum}"
        );

        let verbora_mean = analyzer
            .get_sentiment(WordTokenizer.tokens(&text))
            .expect("a non-empty document has a mean");
        let theirs_mean = sentiment_mean(&text);
        assert!(
            (verbora_mean - theirs_mean).abs() < EPSILON,
            "document({n}): mean polarities differ — Verbora {verbora_mean}, \
             sentiment {theirs_mean}"
        );

        // Stated directly as well as implied: `sum / mean` recovers the
        // denominator each crate used, and both must be the document's token
        // count. A benchmark whose two rows divide by different counts is
        // comparing two different questions.
        assert!(
            verbora_mean != 0.0,
            "document({n}) has a zero mean; the denominator check below would \
             be a division by zero"
        );
        let their_denominator = theirs_sum / theirs_mean;
        assert!(
            (their_denominator - n as f64).abs() < 1e-3,
            "document({n}): sentiment divided by {their_denominator}, not {n}"
        );
    }
}
