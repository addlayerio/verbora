//! String input: the entry points that tokenize before building n-grams.
//!
//! `NGrams.ngrams` accepts either an array of tokens or a string, and takes the
//! second path through `if (!_.isArray(sequence)) sequence = tokenizer.tokenize(sequence)`.
//! The functions here are that path.
//!
//! # The cheap path
//!
//! Tokenizing produces owned `String`s that these functions have to hand back,
//! so every tuple is a copy. When throughput matters, tokenize once and use the
//! slice API, which borrows:
//!
//! ```
//! use verbora_ngrams::{ngrams, tokenize};
//!
//! let tokens = tokenize("the quick brown fox");
//! let grams = ngrams(&tokens, 2, None, None); // Cow::Borrowed windows
//! assert_eq!(grams.len(), 3);
//! ```

use crate::stats::{NGramStats, ngrams_with_stats};
use crate::tokenizer::{NGramTokenizer, tokenize};
use crate::{engine, ngrams_owned};

/// Turns the borrowed pad symbols into the element type of the token sequence.
///
/// `None` covers both of the reference's "no padding" spellings; `Some("")` still
/// pads, because the reference tests the symbol against `undefined` and `null`,
/// never against emptiness.
#[inline]
fn owned_symbol(symbol: Option<&str>) -> Option<String> {
    symbol.map(str::to_owned)
}

/// The n-grams of `text`, tokenized with the process-global tokenizer.
///
/// Mirrors `NGrams.ngrams(text, n, startSymbol, endSymbol)` for string input.
/// The tokenizer is the one [`set_tokenizer`](crate::set_tokenizer) last
/// installed, defaulting to [`WordTokenizer`](crate::WordTokenizer).
///
/// ```
/// use verbora_ngrams::ngrams_str;
///
/// assert_eq!(ngrams_str("a b c", 2, None, None), vec![vec!["a", "b"], vec!["b", "c"]]);
/// // The default tokenizer drops punctuation and splits accented Latin.
/// assert_eq!(ngrams_str("café", 1, None, None), vec![vec!["caf"]]);
/// ```
pub fn ngrams_str(
    text: &str,
    n: usize,
    start_symbol: Option<&str>,
    end_symbol: Option<&str>,
) -> Vec<Vec<String>> {
    let tokens = tokenize(text);
    ngrams_owned(
        &tokens,
        n,
        owned_symbol(start_symbol),
        owned_symbol(end_symbol),
    )
}

/// [`ngrams_str`] with an explicitly supplied tokenizer, leaving the
/// process-global binding untouched.
///
/// This is the escape hatch from the global state that
/// [`set_tokenizer`](crate::set_tokenizer) reproduces.
pub fn ngrams_str_with<T: NGramTokenizer + ?Sized>(
    tokenizer: &T,
    text: &str,
    n: usize,
    start_symbol: Option<&str>,
    end_symbol: Option<&str>,
) -> Vec<Vec<String>> {
    let tokens = tokenizer.tokenize_text(text);
    ngrams_owned(
        &tokens,
        n,
        owned_symbol(start_symbol),
        owned_symbol(end_symbol),
    )
}

/// The 2-grams of `text` — `NGrams.bigrams` with string input.
pub fn bigrams_str(
    text: &str,
    start_symbol: Option<&str>,
    end_symbol: Option<&str>,
) -> Vec<Vec<String>> {
    ngrams_str(text, 2, start_symbol, end_symbol)
}

/// The 3-grams of `text` — `NGrams.trigrams` with string input.
pub fn trigrams_str(
    text: &str,
    start_symbol: Option<&str>,
    end_symbol: Option<&str>,
) -> Vec<Vec<String>> {
    ngrams_str(text, 3, start_symbol, end_symbol)
}

/// An exact alias of [`ngrams_str`] — `NGrams.multrigrams` with string input.
pub fn multrigrams_str(
    text: &str,
    n: usize,
    start_symbol: Option<&str>,
    end_symbol: Option<&str>,
) -> Vec<Vec<String>> {
    ngrams_str(text, n, start_symbol, end_symbol)
}

/// The n-grams of `text` plus frequency statistics — `NGrams.ngrams(text, …,
/// true)`.
///
/// The tuples are owned (`'static`) because the token sequence they came from is
/// created and dropped inside this call; use
/// [`ngrams_with_stats`] over a pre-tokenized slice to
/// keep them borrowed.
pub fn ngrams_str_with_stats(
    text: &str,
    n: usize,
    start_symbol: Option<&str>,
    end_symbol: Option<&str>,
) -> NGramStats<'static, String> {
    let tokens = tokenize(text);
    ngrams_with_stats(
        &tokens,
        n,
        owned_symbol(start_symbol),
        owned_symbol(end_symbol),
    )
    .into_owned()
}

/// [`bigrams_str`] with statistics — `NGrams.bigrams(text, …, true)`.
pub fn bigrams_str_with_stats(
    text: &str,
    start_symbol: Option<&str>,
    end_symbol: Option<&str>,
) -> NGramStats<'static, String> {
    ngrams_str_with_stats(text, 2, start_symbol, end_symbol)
}

/// [`trigrams_str`] with statistics — `NGrams.trigrams(text, …, true)`.
pub fn trigrams_str_with_stats(
    text: &str,
    start_symbol: Option<&str>,
    end_symbol: Option<&str>,
) -> NGramStats<'static, String> {
    ngrams_str_with_stats(text, 3, start_symbol, end_symbol)
}

/// An exact alias of [`ngrams_str_with_stats`] —
/// `NGrams.multrigrams(text, …, true)`.
pub fn multrigrams_str_with_stats(
    text: &str,
    n: usize,
    start_symbol: Option<&str>,
    end_symbol: Option<&str>,
) -> NGramStats<'static, String> {
    ngrams_str_with_stats(text, n, start_symbol, end_symbol)
}

/// Convenience: the n-grams of an already-tokenized sequence of `&str`.
///
/// Exactly [`ngrams`](crate::ngrams); re-stated here so that the "I have tokens
/// already" case is discoverable from the string-input module.
pub fn ngrams_of_tokens<'a>(
    tokens: &'a [&'a str],
    n: usize,
    start_symbol: Option<&'a str>,
    end_symbol: Option<&'a str>,
) -> Vec<std::borrow::Cow<'a, [&'a str]>> {
    engine::ngrams(tokens, n, start_symbol, end_symbol)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tokenizer::{FnTokenizer, exclusive_global};

    #[test]
    fn string_input_tokenizes_then_windows() {
        // These entry points read the process-global tokenizer, so they have to
        // be serialised against the tests that replace it.
        let _guard = exclusive_global();
        assert_eq!(
            ngrams_str("a b c", 2, None, None),
            vec![vec!["a", "b"], vec!["b", "c"]]
        );
        assert_eq!(
            ngrams_str("a b", 2, None, Some("</s>")),
            vec![vec!["a", "b"], vec!["b", "</s>"]]
        );
    }

    #[test]
    fn empty_text_yields_only_padding() {
        let _guard = exclusive_global();
        assert!(ngrams_str("", 2, None, None).is_empty());
        assert_eq!(
            ngrams_str("", 2, Some("<s>"), Some("</s>")),
            vec![vec!["<s>"], vec!["</s>"]]
        );
    }

    #[test]
    fn punctuation_only_input_has_no_tokens() {
        let _guard = exclusive_global();
        assert!(ngrams_str("!!! ???", 1, None, None).is_empty());
        assert!(ngrams_str("...", 2, None, None).is_empty());
    }

    #[test]
    fn aliases_agree() {
        let _guard = exclusive_global();
        let text = "the quick brown fox";
        assert_eq!(
            bigrams_str(text, None, None),
            ngrams_str(text, 2, None, None)
        );
        assert_eq!(
            trigrams_str(text, None, None),
            ngrams_str(text, 3, None, None)
        );
        assert_eq!(
            multrigrams_str(text, 4, None, None),
            ngrams_str(text, 4, None, None)
        );
    }

    #[test]
    fn explicit_tokenizer_bypasses_the_global() {
        let chars = FnTokenizer(|s: &str| s.chars().map(String::from).collect());
        assert_eq!(
            ngrams_str_with(&chars, "abc", 2, None, None),
            vec![vec!["a", "b"], vec!["b", "c"]]
        );
    }

    #[test]
    fn stats_over_string_input() {
        let _guard = exclusive_global();
        let stats = ngrams_str_with_stats("a b a b a", 2, None, None);
        assert_eq!(stats.number_of_ngrams, 4);
        assert_eq!(stats.frequency("(a, b)"), 2);
        assert_eq!(stats.frequency("(b, a)"), 2);
        assert_eq!(stats.nr.get(&2), Some(&2));
        assert_eq!(
            bigrams_str_with_stats("a b a b a", None, None),
            ngrams_str_with_stats("a b a b a", 2, None, None)
        );
        assert_eq!(
            trigrams_str_with_stats("a b a", None, None),
            multrigrams_str_with_stats("a b a", 3, None, None)
        );
    }

    #[test]
    fn pre_tokenized_helper_borrows() {
        let tokens = ["a", "b", "c"];
        let grams = ngrams_of_tokens(&tokens, 2, None, None);
        assert!(
            grams
                .iter()
                .all(|g| matches!(g, std::borrow::Cow::Borrowed(_)))
        );
    }
}
