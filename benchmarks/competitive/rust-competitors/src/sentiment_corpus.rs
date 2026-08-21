//! The §1.14 sentiment corpus, written once and read by both
//! `benches/sentiment.rs` and `tests/sentiment_correctness.rs`.
//!
//! It lives here rather than in the bench file for the same reason
//! [`language_support`](crate::language_support) does: the test's whole job is
//! to prove that the *benchmark's* corpus keeps three specific fairness
//! properties, and a corpus copied into two files is two corpora that stop
//! agreeing the moment one is edited. Copying it would let the test go on
//! passing about text the benchmark no longer measures — the failure mode
//! `crates/verbora-sentiment`'s own documentation calls "two spellings of one
//! rule that stop agreeing".
//!
//! Nothing here depends on either implementation, so this module compiles
//! against neither `verbora-sentiment` nor the `sentiment` crate; it builds
//! strings, and the two consumers do the scoring.
//!
//! # The fairness properties this corpus exists to hold
//!
//! `sentiment` 0.1.1 and `verbora-sentiment` diverge three ways on arbitrary
//! text — a different AFINN revision, no negation rule, its own non-swappable
//! tokenizer. `benches/sentiment.rs`'s module doc comment explains each in
//! full. The corpus closes all three at once:
//!
//! 1. every word in [`SCORING`] is scored **identically** by both lexicons;
//! 2. every word in [`FILLER`] is scored by **neither**;
//! 3. no word in either list is a negation word or begins a multi-token
//!    Verbora key, and every word is a single lowercase ASCII token, so the
//!    two tokenizers cut the text the same way and the two denominators match.
//!
//! Those are not comments to be trusted. `tests/sentiment_correctness.rs`
//! asserts every one of them, word by word, through both crates' public APIs.

/// Document lengths in **tokens** — `scripts/collect-results.py`'s classic
/// `[4, 16, 64, 256, 1024]` convention grid, kept exactly.
///
/// The grid spans two and a half orders of magnitude on purpose. `sentiment`'s
/// `analyze` compiles four `Regex`es and tokenizes the input twice on every
/// call (see the bench's doc comment), which is a fixed per-call constant: at
/// `n = 4` it is essentially the entire measurement and by `n = 1024` it has
/// amortized. Both the constant and the slope have to be visible or the row
/// cannot be read. No cap is needed — 1024 tokens is roughly 5 kB of text.
pub const SIZES: [usize; 5] = [4, 16, 64, 256, 1024];

/// Words that **both** lexicons score, with the **same** polarity.
///
/// Drawn from the 2,438-word agreeing intersection of `sentiment` 0.1.1's
/// embedded AFINN-111 (2,462 entries) and Verbora's AFINN-165 (3,382). Every
/// entry satisfies all five properties the fair domain needs:
///
/// 1. present in both tables;
/// 2. same polarity in both — which excludes the only four shared keys that
///    disagree: `damn`, `exasperated`, `futile`, `irresponsible`;
/// 3. a single lowercase ASCII word, so `sentiment`'s regex tokenizer and
///    `WordTokenizer` cut it identically;
/// 4. not one of Verbora's four English negation words (`not`, `no`, `never`,
///    `neither`), which `sentiment` does not implement at all;
/// 5. not the first piece of any multi-token Verbora key — 33 of the 3,382
///    entries begin one (`bad luck`, `cover-up`, ...) — so Verbora's span scan
///    can never swallow a following token and change the denominator. This is
///    why the obvious `bad` is absent: it begins `bad luck`.
///
/// The polarity in each comment is the value **both** tables assign.
pub const SCORING: &[&str] = &[
    "good",      // +3
    "great",     // +3
    "terrible",  // -3
    "wonderful", // +4
    "awful",     // -3
    "lovely",    // +3
    "horrible",  // -3
    "nice",      // +3
    "ugly",      // -3
    "happy",     // +3
    "sad",       // -2
    "brilliant", // +4
    "stupid",    // -2
    "amazing",   // +4
    "boring",    // -3
    "beautiful", // +3
    "dreadful",  // -3
    "excellent", // +3
    "poor",      // -2
    "perfect",   // +3
    "worst",     // -3
    "hate",      // -3
    "love",      // +3
    "like",      // +2
    "dislike",   // -2
    "fun",       // +4
    "dull",      // -2
    "charming",  // +3
];

/// Words **neither** lexicon scores: function words and neutral film-review
/// vocabulary.
///
/// They exist so the corpus has a realistic hit rate instead of being 100%
/// lexicon hits. A document where every token scores would flatter whichever
/// crate has the cheaper hit path and would say nothing about real text. Same
/// five properties as [`SCORING`], with the first two inverted — absent from
/// both tables rather than present in both — so the *miss* path is what these
/// exercise.
pub const FILLER: &[&str] = &[
    "the", "of", "and", "to", "in", "a", "is", "it", "was", "for", "with", "that", "this", "on",
    "as", "at", "by", "an", "be", "are", "from", "were", "has", "had", "have", "but", "they",
    "their", "there", "when", "which", "while", "after", "before", "into", "over", "under",
    "about", "through", "his", "her", "our", "your", "its", "them", "these", "those", "film",
    "movie", "scene", "story", "actor", "director", "camera", "script", "screen", "minute", "hour",
    "part", "half", "end", "begin", "middle", "where", "would", "could", "should", "also", "very",
    "much", "many", "other", "another", "each", "every",
];

/// A document of exactly `tokens` tokens: lowercase ASCII words joined by one
/// space, every fourth drawn from [`SCORING`] and the rest from [`FILLER`].
///
/// Deterministic — no RNG, no seed, no data file — so the corpus is
/// reproducible by reading this function, and the correctness test measures
/// the identical strings the benchmark does.
///
/// One scoring word in four (25%) is deliberately denser than ordinary prose,
/// where an AFINN hit rate of 5-10% is more typical. Two reasons: at `n = 4` a
/// realistic rate would round to zero scored words, so the smallest size would
/// measure two tokenizers and nothing else; and the *hit* path is the only
/// place the two crates do materially different work per token (Verbora reads
/// a span bound out of the same hash slot as the polarity, `sentiment` pushes
/// a cloned `String` into a `Vec`), so under-representing it would hide the
/// difference the group exists to measure. The rate is identical for both
/// implementations, which is what fairness requires; realism is what is traded
/// away, and it is traded away here rather than silently.
#[must_use]
pub fn document(tokens: usize) -> String {
    let mut out = String::new();
    for i in 0..tokens {
        if i > 0 {
            out.push(' ');
        }
        if i % 4 == 3 {
            out.push_str(SCORING[(i / 4) % SCORING.len()]);
        } else {
            out.push_str(FILLER[i % FILLER.len()]);
        }
    }
    out
}

/// Every word the corpus can contain, in one iterator — [`SCORING`] then
/// [`FILLER`].
///
/// The correctness test walks this rather than the two lists separately for
/// the checks that apply to both (single lowercase ASCII token, not a negation
/// word, not a phrase-key prefix), so adding a word to either list cannot
/// escape them.
pub fn all_words() -> impl Iterator<Item = &'static str> {
    SCORING.iter().chain(FILLER.iter()).copied()
}
