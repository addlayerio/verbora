//! The two module-level `let`s `tfidf` shares between every instance.
//!
//! ```text
//! let tokenizer = new Tokenizer()                       // module scope
//! let stopwords = require('../util/stopwords').words    // module scope
//! ```
//!
//! `setTokenizer` and `setStopwords` are declared as *instance* methods but
//! assign to those, so calling either on one `TfIdf` changes how every other
//! `TfIdf` in the process tokenizes its next document. The reference spec depends on
//! it: one `it` block installs `TreebankWordTokenizer` and never puts it back.
//!
//! A port that made these `&mut self` fields would look tidier and behave
//! differently, so this module reproduces the sharing — but pays nothing for it
//! in the default case. Two `AtomicBool`s record whether either global has ever
//! been reassigned; until one has, the hot paths run the default
//! `WordTokenizer` monomorphically and answer stop-word membership from
//! `verbora_core`'s static table, taking no lock at all.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, RwLock};

use rustc_hash::FxHashSet;
use verbora_tokenizers::regexp::WordTokens;
use verbora_tokenizers::{Tokenize, WordTokenizer};

// ---------------------------------------------------------------------------
// Tokenizer
// ---------------------------------------------------------------------------

/// A tokenizer `TfIdf` can be given.
///
/// The reference duck-types this: `setTokenizer` accepts any object with a callable
/// `tokenize`, and throws `Expected a valid Tokenizer` otherwise. Rust checks
/// the same requirement at compile time, so the "not a tokenizer" error has no
/// runtime counterpart here.
///
/// The trait is object-safe on purpose — `verbora_tokenizers::Tokenize` uses a
/// generic associated type and `impl Iterator`, neither of which can live behind
/// a `dyn`, and a process-global slot needs `dyn`.
pub trait TfIdfTokenizer: Send + Sync + std::fmt::Debug {
    /// Tokenizes `text`, appending to `out`.
    ///
    /// `out` is not cleared, matching the buffer-reuse convention the rest of
    /// the workspace uses.
    fn tokenize_into(&self, text: &str, out: &mut Vec<String>);

    /// Tokenizes `text`.
    fn tokenize(&self, text: &str) -> Vec<String> {
        let mut out = Vec::new();
        self.tokenize_into(text, &mut out);
        out
    }
}

impl TfIdfTokenizer for WordTokenizer {
    fn tokenize_into(&self, text: &str, out: &mut Vec<String>) {
        if let Some(tokens) = self.tokens(text) {
            out.extend(tokens.map(str::to_owned));
        }
    }
}

impl TfIdfTokenizer for verbora_tokenizers::TreebankWordTokenizer {
    fn tokenize_into(&self, text: &str, out: &mut Vec<String>) {
        out.extend(self.tokens(text).map(|t| t.to_string()));
    }
}

impl TfIdfTokenizer for verbora_tokenizers::WordPunctTokenizer {
    fn tokenize_into(&self, text: &str, out: &mut Vec<String>) {
        if let Some(tokens) = self.tokens(text) {
            out.extend(tokens.map(|t| t.to_string()));
        }
    }
}

impl TfIdfTokenizer for verbora_tokenizers::AggressiveTokenizer {
    fn tokenize_into(&self, text: &str, out: &mut Vec<String>) {
        out.extend(self.tokens(text).map(|t| t.to_string()));
    }
}

/// Set once `set_tokenizer` has been called; until then the default path runs.
static TOKENIZER_REPLACED: AtomicBool = AtomicBool::new(false);

/// The process-global tokenizer, mirroring `let tokenizer` in `tfidf`.
static TOKENIZER: LazyLock<RwLock<Option<Arc<dyn TfIdfTokenizer>>>> =
    LazyLock::new(|| RwLock::new(None));

/// Installs a process-global tokenizer — the reference's `TfIdf#setTokenizer`.
///
/// This affects **every** `TfIdf` in the process, including ones created
/// earlier. That is the reference's behaviour, not an oversight here.
pub fn set_tokenizer(tokenizer: Arc<dyn TfIdfTokenizer>) {
    *TOKENIZER.write().expect("tokenizer lock poisoned") = Some(tokenizer);
    TOKENIZER_REPLACED.store(true, Ordering::Release);
}

/// Restores the default `WordTokenizer`.
///
/// The reference has no such API — the only way back is `setTokenizer(new
/// WordTokenizer())`, which is exactly what this does — but tests and
/// long-running processes need a way to undo global state.
pub fn reset_tokenizer() {
    *TOKENIZER.write().expect("tokenizer lock poisoned") = None;
    TOKENIZER_REPLACED.store(false, Ordering::Release);
}

/// Whether a non-default tokenizer is installed.
pub fn tokenizer_is_default() -> bool {
    !TOKENIZER_REPLACED.load(Ordering::Acquire)
}

/// Tokens produced by whichever tokenizer is currently installed.
///
/// The default variant borrows straight out of `text`, so the common path
/// allocates nothing per token; only a custom tokenizer, which must be reached
/// through `dyn` and returns owned strings, does.
#[derive(Debug)]
pub enum GlobalTokens<'a> {
    /// The default `WordTokenizer`, yielding slices of the input.
    Default(WordTokens<'a>),
    /// A tokenizer installed through [`set_tokenizer`].
    Custom(std::vec::IntoIter<String>),
}

impl GlobalTokens<'_> {
    /// Applies `f` to each token in order.
    ///
    /// A `for_each` rather than an `Iterator` impl because the two variants
    /// yield different lifetimes (`&'a str` versus an owned `String` the
    /// iterator holds), and collapsing them into one item type would force the
    /// default path to allocate.
    pub fn for_each(self, mut f: impl FnMut(&str)) {
        match self {
            Self::Default(it) => it.for_each(&mut f),
            Self::Custom(it) => it.for_each(|t| f(&t)),
        }
    }
}

/// Tokenizes with the process-global tokenizer.
pub fn tokenize_global(text: &str) -> GlobalTokens<'_> {
    if tokenizer_is_default() {
        // `WordTokenizer` in splitting mode never returns the reference's `null`.
        return GlobalTokens::Default(
            WordTokenizer::new()
                .tokens(text)
                .expect("WordTokenizer splitting mode always matches"),
        );
    }
    let guard = TOKENIZER.read().expect("tokenizer lock poisoned");
    match guard.as_ref() {
        Some(t) => GlobalTokens::Custom(t.tokenize(text).into_iter()),
        None => GlobalTokens::Default(
            WordTokenizer::new()
                .tokens(text)
                .expect("WordTokenizer splitting mode always matches"),
        ),
    }
}

// ---------------------------------------------------------------------------
// Stop words
// ---------------------------------------------------------------------------

/// Set once `set_stopwords` has replaced the shared list.
static STOPWORDS_REPLACED: AtomicBool = AtomicBool::new(false);

/// The list installed by [`set_stopwords`], if any.
static STOPWORDS: LazyLock<RwLock<Option<Arc<StopwordSet>>>> = LazyLock::new(|| RwLock::new(None));

/// An installed stop-word list: the order the reference exposes plus a hash set,
/// because the reference's `indexOf` is a linear scan of 170 strings per token.
#[derive(Debug)]
struct StopwordSet {
    ordered: Vec<String>,
    lookup: FxHashSet<String>,
}

/// The argument shape `setStopwords` validates.
///
/// The reference returns `false` for a non-array, and `false` for an array with
/// any element whose `typeof` is not `'string'` — checking every element before
/// deciding, so a bad element late in the list is still caught. Only the
/// `typeof` matters, which is why the non-string variant carries no payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StopwordList {
    /// Not an array: rejected outright.
    NotAnArray,
    /// An array, with each element's stringness recorded.
    Array(Vec<StopwordElement>),
}

/// One element of a candidate stop-word list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StopwordElement {
    /// A primitive string — the only accepted kind.
    Str(String),
    /// Anything else, including `new String('x')`, whose `typeof` is `'object'`.
    NotAString,
}

impl StopwordList {
    /// Builds a well-formed list from string-likes.
    pub fn of<I, S>(words: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self::Array(
            words
                .into_iter()
                .map(|w| StopwordElement::Str(w.into()))
                .collect(),
        )
    }
}

/// Installs a process-global stop-word list — the reference's `setStopwords`.
///
/// Returns `false` and changes nothing when the list is rejected.
pub fn set_stopwords(list: &StopwordList) -> bool {
    let StopwordList::Array(elements) = list else {
        return false;
    };
    if elements.contains(&StopwordElement::NotAString) {
        return false;
    }
    let ordered: Vec<String> = elements
        .iter()
        .map(|e| match e {
            StopwordElement::Str(s) => s.clone(),
            StopwordElement::NotAString => unreachable!("rejected above"),
        })
        .collect();
    let lookup = ordered.iter().cloned().collect();
    *STOPWORDS.write().expect("stopwords lock poisoned") =
        Some(Arc::new(StopwordSet { ordered, lookup }));
    STOPWORDS_REPLACED.store(true, Ordering::Release);
    true
}

/// Rebinds the global back to the shared `util/stopwords` array.
///
/// The reference reaches this state by `setStopwords(stopwords)`, which
/// restores the very array `tfidf` captured at load time — so mutations made
/// through `stopwords.push(...)` are visible again. This restores the
/// same aliasing to [`verbora_core::stopwords`]'s process-global list.
pub fn reset_stopwords() {
    *STOPWORDS.write().expect("stopwords lock poisoned") = None;
    STOPWORDS_REPLACED.store(false, Ordering::Release);
}

/// The currently installed list, in order, or `None` while the shared
/// `util/stopwords` array is bound.
pub fn stopwords() -> Option<Vec<String>> {
    if !STOPWORDS_REPLACED.load(Ordering::Acquire) {
        return None;
    }
    STOPWORDS
        .read()
        .expect("stopwords lock poisoned")
        .as_ref()
        .map(|s| s.ordered.clone())
}

/// Whether `term` is filtered out of a string-built document.
pub fn is_stopword(term: &str) -> bool {
    if !STOPWORDS_REPLACED.load(Ordering::Acquire) {
        // Aliased to the shared array, so `verbora_core`'s global mutations —
        // `addStopWord` on any stemmer, or a direct push — are visible here,
        // exactly as they are in the reference.
        return verbora_core::stopwords::is_default_stopword(term);
    }
    STOPWORDS
        .read()
        .expect("stopwords lock poisoned")
        .as_ref()
        .is_some_and(|s| s.lookup.contains(term))
}
