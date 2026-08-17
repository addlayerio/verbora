//! N-gram generation and frequency tables for Rust.
//!
//! N-gram windows and frequency statistics — both `NGrams` and `NGramsZH`.
//!
//! ```
//! use verbora_ngrams::{bigrams, ngrams_str, zh::ngrams_zh};
//!
//! // Pre-tokenized input: windows borrow from the slice, nothing is copied.
//! let tokens = ["the", "quick", "brown", "fox"];
//! assert_eq!(bigrams(&tokens, None, None).len(), 3);
//!
//! // String input: tokenized first.
//! assert_eq!(ngrams_str("a b c", 2, None, None), vec![vec!["a", "b"], vec!["b", "c"]]);
//!
//! // Chinese: split per character rather than tokenized.
//! assert_eq!(ngrams_zh("中文", 2, None, None), vec![vec!["中", "文"]]);
//! ```
//!
//! # Three things a naive port gets wrong
//!
//! **Padding is not symmetric, and padded tuples are not always `n` long.** Both
//! padding loops clamp their sequence half independently, and the right-hand one
//! slices with an index that can go negative — where the definition re-anchors to
//! `length + start` rather than to zero. `ngrams(['a','b','c'], 5, '<s>', '</s>')`
//! therefore contains a *two*-element tuple. See [`ngrams`].
//!
//! **An absent pad disables padding; an empty pad does not.** The API gates
//! on `typeof x !== 'undefined' && x !== null`, so an empty start symbol pads with
//! empty strings. [`Option`] maps this correctly as long as nothing tests the
//! symbol for emptiness.
//!
//! **The frequency map's key for an empty n-gram is `")"`.** Not `"()"` — the
//! key builder chops two characters off a one-character buffer with
//! `String#substr`, which clamps instead of counting from the end. Reachable
//! whenever `n == 0`. See [`ngram_key`].
//!
//! # Layout
//!
//! | Module | Reference |
//! |---|---|
//! | [`engine`] | the shared algorithm; array input for both `NGrams` and `NGramsZH` |
//! | [`stats`] | the `{ngrams, frequencies, Nr, numberOfNgrams}` return shape |
//! | [`tokenizer`] | the default `WordTokenizer` and the process-global `setTokenizer` binding |
//! | [`text`] | string input for `NGrams` |
//! | [`zh`] | `NGramsZH`, including its UTF-16 code-unit splitting |
//!
//! # Global state
//!
//! `NGrams` keeps its tokenizer in a module-level variable that `setTokenizer`
//! rebinds process-wide. That is observable behaviour — this crate's own spec
//! suite depends on it — so [`set_tokenizer`] reproduces it rather than quietly
//! making the tokenizer a parameter. Every function that reads the global has a
//! sibling that takes a tokenizer explicitly; see [`tokenizer`].

pub mod engine;
pub mod stats;
pub mod text;
pub mod tokenizer;
pub mod zh;

pub use engine::{NGramIter, bigrams, multrigrams, ngrams, ngrams_iter, ngrams_owned, trigrams};
pub use stats::{
    NGramStats, bigrams_with_stats, multrigrams_with_stats, ngram_key, ngrams_with_stats,
    trigrams_with_stats,
};
pub use text::{
    bigrams_str, bigrams_str_with_stats, multrigrams_str, multrigrams_str_with_stats, ngrams_str,
    ngrams_str_with, ngrams_str_with_stats, trigrams_str, trigrams_str_with_stats,
};
pub use tokenizer::{
    FnTokenizer, NGramTokenizer, WordTokenizer, current_tokenizer, reset_tokenizer, set_tokenizer,
    tokenize,
};
pub use zh::{bigrams_zh, ngrams_zh, trigrams_zh};
