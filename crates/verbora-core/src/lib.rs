//! Core traits and shared types for `verbora`.
//!
//! This crate defines the vocabulary the rest of the workspace is written
//! against. It has no dependencies on other `verbora-*` crates, which keeps the
//! crate graph acyclic and lets leaf crates (distance, phonetics, …) be used in
//! isolation without pulling in data assets they do not need.
//!
//! # Two API levels
//!
//! Every processing trait in this crate is offered at two levels, as required by
//! the project's performance charter:
//!
//! * a **high-level** API that takes an owned result and returns
//!   owned data ([`Tokenizer::tokenize`], [`Stemmer::stem`]);
//! * a **low-level** API that writes into a caller-supplied buffer
//!   ([`Tokenizer::tokenize_into`], [`Stemmer::stem_into`]) so that hot loops can
//!   amortise allocation across millions of calls.
//!
//! Tokenizers whose output is always a *substring* of the input additionally
//! implement [`BorrowingTokenizer`], which yields `&str` slices that borrow the
//! input and allocate nothing at all.

#![cfg_attr(docsrs, feature(doc_cfg))]

pub mod stopwords;
pub mod token;
pub mod whitespace;

pub use stopwords::StopWords;
pub use token::Token;
pub use whitespace::{collapse_whitespace, is_whitespace};

use std::borrow::Cow;

// ---------------------------------------------------------------------------
// Tokenization
// ---------------------------------------------------------------------------

/// Splits text into tokens.
///
/// This is the owned, ergonomic entry point every tokenizer provides.
/// Implementations must reproduce their documented output exactly,
/// including its treatment of empty tokens at the string boundaries (see
/// [`trim_edge_empties`]).
pub trait Tokenizer {
    /// Tokenizes `text`, returning owned tokens.
    ///
    /// This is the fully-owned API: one `String` per token, in order.
    fn tokenize(&self, text: &str) -> Vec<String> {
        let mut out = Vec::new();
        self.tokenize_into(text, &mut out);
        out
    }

    /// Tokenizes `text`, appending to `out`.
    ///
    /// `out` is **not** cleared, so callers can accumulate across inputs. Reusing
    /// a single buffer across calls is the recommended pattern for batch work:
    ///
    /// ```ignore
    /// let mut buf = Vec::new();
    /// for line in corpus {
    ///     buf.clear();
    ///     tokenizer.tokenize_into(line, &mut buf);
    ///     consume(&buf);
    /// }
    /// ```
    fn tokenize_into(&self, text: &str, out: &mut Vec<String>);

    /// Tokenizes a batch of inputs.
    ///
    /// The default implementation is a plain sequential map: one fresh
    /// `Vec<String>` per document, with no shared buffer and no parallelism. It
    /// therefore allocates *more* than a [`Self::tokenize_into`] loop over a
    /// reused buffer does, and is offered so that generic code can express the
    /// operation — not because it is the fast path.
    ///
    /// Implementations that can do better (amortising one buffer, or
    /// parallelising) should override it. None currently does.
    fn tokenize_batch<S: AsRef<str>>(&self, texts: &[S]) -> Vec<Vec<String>> {
        texts.iter().map(|t| self.tokenize(t.as_ref())).collect()
    }
}

/// A [`Tokenizer`] whose tokens are always contiguous substrings of the input.
///
/// This is the zero-copy path. Tokenizers that fold case, transliterate, or
/// otherwise rewrite the text cannot implement it.
pub trait BorrowingTokenizer: Tokenizer {
    /// Tokenizes `text` into slices borrowed from it. Allocates only the `Vec`.
    fn tokenize_borrowed<'a>(&self, text: &'a str) -> Vec<&'a str> {
        let mut out = Vec::new();
        self.tokenize_borrowed_into(text, &mut out);
        out
    }

    /// Tokenizes `text` into `out` as borrowed slices, appending. Allocation-free
    /// once `out` has sufficient capacity.
    fn tokenize_borrowed_into<'a>(&self, text: &'a str, out: &mut Vec<&'a str>);
}

/// Removes empty strings from both ends of a token list.
///
/// Trimming pops trailing empty strings and shifts leading
/// ones, but leaves *interior* empties untouched. Several tokenizers depend on
/// that asymmetry, so it is reproduced exactly here rather than being
/// generalised to "remove all empties".
pub fn trim_edge_empties<T: AsRef<str>>(tokens: &mut Vec<T>) {
    while tokens.last().is_some_and(|t| t.as_ref().is_empty()) {
        tokens.pop();
    }
    let lead = tokens
        .iter()
        .position(|t| !t.as_ref().is_empty())
        .unwrap_or(tokens.len());
    if lead > 0 {
        tokens.drain(..lead);
    }
}

// ---------------------------------------------------------------------------
// Stemming
// ---------------------------------------------------------------------------

/// Reduces a word to its stem.
///
/// Reduces one token to its stem.
pub trait Stemmer {
    /// Stems a single token.
    ///
    /// Returns [`Cow::Borrowed`] when the token is already its own stem, which is
    /// the common case for short and irregular words. Callers that need an owned
    /// `String` can use `.into_owned()`.
    fn stem<'a>(&self, token: &'a str) -> Cow<'a, str>;

    /// Stems `token`, writing the result into `out`.
    ///
    /// `out` is cleared first. Intended for hot loops that reuse one `String`.
    fn stem_into(&self, token: &str, out: &mut String) {
        out.clear();
        out.push_str(&self.stem(token));
    }

    /// Stems a batch of tokens.
    fn stem_batch<S: AsRef<str>>(&self, tokens: &[S]) -> Vec<String> {
        tokens
            .iter()
            .map(|t| self.stem(t.as_ref()).into_owned())
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Phonetics
// ---------------------------------------------------------------------------

/// Maps a word to a phonetic key, so that similar-sounding words collide.
///
/// Encodes a word phonetically, and compares two words by their encodings.
pub trait Phonetic {
    /// Computes the phonetic key for `token`.
    fn process(&self, token: &str) -> String;

    /// Returns whether two strings share a phonetic key.
    ///
    /// This is defined as `process(a) == process(b)`, and so is
    /// the default implementation here — both keys are computed, so this
    /// allocates two `String`s. An implementation able to compare incrementally
    /// may override it to avoid the second; none currently does.
    fn compare(&self, a: &str, b: &str) -> bool {
        self.process(a) == self.process(b)
    }
}

/// A phonetic algorithm that yields a primary *and* an alternate key.
///
/// Double Metaphone and Daitch–Mokotoff Soundex both return two keys; modelling
/// that here avoids forcing single-key algorithms to return a tuple.
pub trait DoubleKeyPhonetic {
    /// Computes `(primary, alternate)` keys for `token`.
    fn process_double(&self, token: &str) -> (String, String);
}

// ---------------------------------------------------------------------------
// Distance
// ---------------------------------------------------------------------------

/// A string similarity or distance metric.
///
/// The metrics are plain functions with deliberately different conventions: some
/// return distances (lower is closer), others similarities (higher is closer).
/// This trait deliberately does **not** normalise that — [`Self::IS_SIMILARITY`]
/// records which convention each metric uses so generic code can adapt without
/// changing any metric's observable output.
pub trait StringMetric {
    /// `true` when a larger value means "more similar" (e.g. Jaro–Winkler,
    /// Dice), `false` when a larger value means "further apart" (e.g.
    /// Levenshtein, Hamming).
    const IS_SIMILARITY: bool;

    /// Computes the metric between two strings.
    fn measure(&self, a: &str, b: &str) -> f64;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trim_removes_only_edge_empties() {
        let mut v = vec!["", "", "a", "", "b", "", ""];
        trim_edge_empties(&mut v);
        // Interior empties survive — this asymmetry is specified, not incidental.
        assert_eq!(v, vec!["a", "", "b"]);
    }

    #[test]
    fn trim_handles_all_empty() {
        let mut v = vec!["", "", ""];
        trim_edge_empties(&mut v);
        assert!(v.is_empty());
    }

    #[test]
    fn trim_handles_empty_input() {
        let mut v: Vec<&str> = vec![];
        trim_edge_empties(&mut v);
        assert!(v.is_empty());
    }

    #[test]
    fn trim_leaves_clean_input_untouched() {
        let mut v = vec!["a", "b"];
        trim_edge_empties(&mut v);
        assert_eq!(v, vec!["a", "b"]);
    }
}
