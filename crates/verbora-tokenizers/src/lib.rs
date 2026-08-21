//! Cuts text at [UAX #29] boundaries and returns the pieces, borrowed and in
//! order.
//!
//! Three tokenizers, one token shape. Every token this crate produces is a
//! contiguous slice of the input it was given, returned as `&'a str` borrowed
//! from that input:
//!
//! | Tokenizer | Yields |
//! |---|---|
//! | [`WordTokenizer`] | the word segments containing a letter or a digit |
//! | [`SegmentTokenizer`] | *every* word segment, so concatenation is the input |
//! | [`SentenceTokenizer`] | the sentences, untrimmed, with an optional abbreviation tailoring |
//!
//! # Nothing here rewrites its input
//!
//! No tokenizer in this crate folds case, strips punctuation, folds diacritics,
//! trims, or substitutes placeholders. That is not a missing feature: a
//! tokenizer that rewrote its input could not be composed with one that did
//! not, and its tokens would not be substrings, which is the guarantee
//! everything downstream is built on. Rewriting is `verbora-normalizers`' job,
//! and every function there is named for the rewrite it performs.
//!
//! Nor can anything here emit `U+FFFD` unless `U+FFFD` was in the input — the
//! crate has no UTF-16 representation to lose an astral scalar in, so
//! `"a😀b"` tokenizes to `["a", "b"]` and never to a pair of replacement
//! characters.
//!
//! # Iterator first
//!
//! [`BorrowingTokenizer::tokens`] is the primitive; everything else is defined
//! on it, so there is one implementation of each behaviour and no second copy
//! to drift.
//!
//! ```
//! use verbora_tokenizers::{BorrowingTokenizer, Tokenizer, WordTokenizer};
//!
//! let t = WordTokenizer;
//!
//! // Lazy, zero-copy: each token borrows the input.
//! assert_eq!(t.tokens("the quick brown fox").next(), Some("the"));
//!
//! // Collected, still borrowed.
//! assert_eq!(t.tokenize_borrowed("the quick"), ["the", "quick"]);
//!
//! // Into a buffer reused across documents. NOTE: `buf` is not cleared for you.
//! let mut buf = Vec::new();
//! for doc in ["one two", "three four"] {
//!     buf.clear();
//!     t.tokenize_borrowed_into(doc, &mut buf);
//! }
//!
//! // Owned, when the tokens must outlive the input.
//! let owned: Vec<String> = t.tokenize("one two");
//! assert_eq!(owned, ["one", "two"]);
//! ```
//!
//! # Choosing the right API
//!
//! | Call | Use when | Allocates |
//! |---|---|---|
//! | [`tokens`](BorrowingTokenizer::tokens) | streaming, composing with `map`/`filter`, early exit | nothing |
//! | [`tokenize_borrowed`](BorrowingTokenizer::tokenize_borrowed) | you want a `Vec` and the input outlives it | one `Vec` of `&str` |
//! | [`tokenize_borrowed_into`](BorrowingTokenizer::tokenize_borrowed_into) | one buffer reused across a corpus | nothing once warm — **`out` is not cleared** |
//! | [`Tokenizer::tokenize`] | tokens must outlive the input | one `String` per token |
//! | [`par_tokenize_batch`] | many independent documents | one `Vec` per document |
//!
//! The crossover point for [`par_tokenize_batch`] is **unmeasured**; prefer a
//! sequential loop for a handful of short strings.
//!
//! # The Unicode version is part of the contract
//!
//! Word and sentence boundaries are defined over the `Word_Break` and
//! `Sentence_Break` properties of the Unicode Character Database, so this crate
//! cannot promise results frozen for all time the way `verbora-distance` can —
//! a boundary rule frozen today is simply wrong for every character encoded
//! after the freeze. Instead:
//!
//! * The Unicode version is whichever version `unicode-segmentation` ships,
//!   pinned in `Cargo.lock`. At the version this crate is built against that is
//!   **Unicode 17.0.0**; [`unicode_version`] reports it at runtime, and the
//!   crate's conformance tests replay the UCD's own `WordBreakTest.txt` and
//!   `SentenceBreakTest.txt` for it.
//! * A UCD upgrade is a **semver-visible behaviour change** for this crate and
//!   is released as one.
//! * **Any structure that persists tokenizer-derived keys must stamp the
//!   Unicode version and refuse to load across a change.** An index, a model or
//!   an interned term table built before an upgrade does not match one built
//!   after it, and nothing else will tell you.
//!
//! Within one Unicode version the crate is fully deterministic: the same input
//! produces the same output on every platform and every build. There is no
//! global mutable state, no hash-order dependence, and no interior mutability.
//!
//! [UAX #29]: https://www.unicode.org/reports/tr29/

#![cfg_attr(doctest, doc = include_str!("../README.md"))]
#![cfg_attr(docsrs, feature(doc_cfg))]
// The docs above link to the `parallel`-gated batch helper. That link
// resolves on docs.rs, which builds all features; without the feature the
// target does not exist and the lint would fire on every plain `cargo doc`.
// It stays armed in the all-features build, which is the one that ships.
#![cfg_attr(not(feature = "parallel"), allow(rustdoc::broken_intra_doc_links))]

mod sentence;
mod word;

pub use sentence::{AbbreviationError, SentenceTokenizer};
pub use word::{SegmentTokenizer, WordTokenizer};

pub use verbora_core::{BorrowingTokenizer, Tokenizer};

/// The Unicode version whose `Word_Break` and `Sentence_Break` assignments this
/// crate's boundaries are computed from, as `(major, minor, update)`.
///
/// Stamp this into anything that persists tokenizer-derived keys, and refuse to
/// load an artifact whose stamp differs: boundaries move between Unicode
/// versions, so an index built under one and queried under another silently
/// mismatches rather than failing.
///
/// ```
/// // The version is a fact about the build, not a constant to assert against
/// // in application code — record it, compare it, do not hardcode it.
/// let (major, _minor, _update) = verbora_tokenizers::unicode_version();
/// assert!(major >= 17);
/// ```
#[must_use]
pub fn unicode_version() -> (u64, u64, u64) {
    unicode_segmentation::UNICODE_VERSION
}

/// Tokenizes many independent documents in parallel, preserving input order.
///
/// `results[i]` is `tokenizer.tokenize_borrowed(texts[i])`. This is exactly
/// `texts.par_iter().map(|t| tokenizer.tokenize_borrowed(t)).collect()` — a
/// thin `rayon` fan-out over the sequential path, not a second implementation
/// of tokenization. It uses whichever global rayon pool is already installed,
/// and constructs none of its own.
///
/// It is a free function rather than a trait method so that `verbora-core`
/// acquires neither a `parallel` feature nor a `rayon` dependency for one
/// method body.
///
/// # When to reach for it
///
/// The crossover — the batch size and document length at which parallel fan-out
/// beats a sequential loop — is **unmeasured** for this implementation. Rayon
/// costs on the order of a microsecond to schedule a task, so for a handful of
/// short strings a plain `texts.iter().map(...)` loop is the better default.
///
/// Requires the `parallel` feature.
///
/// ```
/// # #[cfg(feature = "parallel")] {
/// use verbora_tokenizers::{WordTokenizer, par_tokenize_batch};
///
/// let docs = ["one two", "three"];
/// assert_eq!(
///     par_tokenize_batch(&WordTokenizer, &docs),
///     [vec!["one", "two"], vec!["three"]],
/// );
/// # }
/// ```
#[cfg(feature = "parallel")]
#[cfg_attr(docsrs, doc(cfg(feature = "parallel")))]
#[must_use]
pub fn par_tokenize_batch<'a, T>(tokenizer: &T, texts: &[&'a str]) -> Vec<Vec<&'a str>>
where
    T: BorrowingTokenizer + Sync,
{
    use rayon::prelude::*;

    texts
        .par_iter()
        .map(|text| tokenizer.tokenize_borrowed(text))
        .collect()
}
