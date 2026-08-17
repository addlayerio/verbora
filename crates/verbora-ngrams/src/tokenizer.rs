//! The tokenizer NGrams uses for string input, including its process-global
//! binding.
//!
//! # The state that has to be reproduced
//!
//! The reference `ngrams` opens with
//!
//! ```text
//! let tokenizer = new Tokenizer()          // WordTokenizer
//! exports.setTokenizer = function (t) { … tokenizer = t }
//! ```
//!
//! — a **module-level mutable variable**. `setTokenizer` rebinds it for the whole
//! process and for every caller, permanently, and the library's own spec suite
//! depends on that: `ngram_spec.ts` installs `AggressiveTokenizerFr` in one test
//! and `AggressiveTokenizer` in the next, and the second test's result depends on
//! the first test having run. Making the tokenizer a per-call parameter or a
//! plain struct field would be tidier and would *not* reproduce that.
//!
//! So this module mirrors [`verbora_core::stopwords`]: there is a real
//! process-global binding, guarded by an [`RwLock`], with an [`AtomicBool`] that
//! keeps the never-overridden case entirely lock-free — and every function that
//! consults it has a sibling that takes an explicit tokenizer, so code that wants
//! no global state can opt out completely.
//!
//! `NGramsZH` has no tokenizer at all (it splits characters), and none is added
//! here.
//!
//! # The default tokenizer
//!
//! The reference's default is `WordTokenizer`, whose pattern is
//! `/[^A-Za-zА-Яа-я0-9_]+/` in gaps mode. [`WordTokenizer`] reproduces exactly
//! that character class — see its documentation for why that matters more than
//! it looks.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

use verbora_core::{BorrowingTokenizer, Tokenizer};

/// An object-safe tokenizer, so the global binding can hold any implementation.
///
/// [`verbora_core::Tokenizer`] has a generic method (`tokenize_batch`) and is
/// therefore not object-safe, so it cannot be used behind a `dyn`. This trait is
/// the `dyn`-compatible projection of it, and the blanket implementation below
/// means **any** `verbora_core::Tokenizer` can be installed with
/// [`set_tokenizer`] without implementing anything extra.
pub trait NGramTokenizer: Send + Sync {
    /// Splits `text` into tokens.
    fn tokenize_text(&self, text: &str) -> Vec<String>;
}

impl<T: Tokenizer + Send + Sync + ?Sized> NGramTokenizer for T {
    fn tokenize_text(&self, text: &str) -> Vec<String> {
        self.tokenize(text)
    }
}

/// Whether `c` belongs to a `WordTokenizer` token.
///
/// The reference class is `[A-Za-zА-Яа-я0-9_]`, listed here literally rather than
/// as "alphanumeric": it is **ASCII letters, digits, underscore and one contiguous
/// slice of Cyrillic**, and nothing else.
///
/// Two consequences that a Unicode-aware port gets wrong:
///
/// * Accented Latin letters are **separators**, so `café` tokenizes as `caf`, and
///   `naïve` as `na` + `ve`.
/// * The Cyrillic range is exactly `U+0410..=U+042F` and `U+0430..=U+044F`, which
///   **excludes** `Ё` (U+0401) and `ё` (U+0451). `Ёж` tokenizes as `ж`.
///
/// Both are recorded in `fixtures/ngrams.json`.
#[inline]
const fn is_word_char(c: char) -> bool {
    matches!(c,
        'A'..='Z'
        | 'a'..='z'
        | '0'..='9'
        | '_'
        | '\u{0410}'..='\u{042F}'
        | '\u{0430}'..='\u{044F}'
    )
}

/// The default tokenizer for string input — the reference's `WordTokenizer`.
///
/// # Why this lives here rather than in `verbora-tokenizers`
///
/// `NGrams` needs a default at module scope, and it must be the *reference's*
/// default or every string-input result changes. This is a self-contained
/// reproduction of the one pattern involved, kept deliberately tiny; installing
/// the full tokenizer crate's implementation with [`set_tokenizer`] replaces it
/// process-wide, exactly as `NGrams.setTokenizer` does in the reference.
///
/// # Behaviour
///
/// The reference splits on the gap pattern and then drops `''` and `' '` from the
/// result (`_.without(results, '', ' ')`; `discardEmpty` is `options.discardEmpty
/// || true`, so it is unconditionally true). Splitting on maximal runs of
/// separators and discarding the empty pieces is the same thing as keeping the
/// maximal runs of word characters, which is what this implementation does — and
/// it can borrow rather than allocate.
///
/// ```
/// use verbora_ngrams::WordTokenizer;
/// use verbora_core::BorrowingTokenizer;
///
/// let t = WordTokenizer;
/// assert_eq!(t.tokenize_borrowed("She said 'hello'."), ["She", "said", "hello"]);
/// // Accented letters split, exactly as the reference regex does.
/// assert_eq!(t.tokenize_borrowed("café naïve"), ["caf", "na", "ve"]);
/// ```
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct WordTokenizer;

impl Tokenizer for WordTokenizer {
    fn tokenize_into(&self, text: &str, out: &mut Vec<String>) {
        out.extend(
            text.split(|c| !is_word_char(c))
                .filter(|piece| !piece.is_empty())
                .map(str::to_owned),
        );
    }
}

impl BorrowingTokenizer for WordTokenizer {
    fn tokenize_borrowed_into<'a>(&self, text: &'a str, out: &mut Vec<&'a str>) {
        out.extend(
            text.split(|c| !is_word_char(c))
                .filter(|piece| !piece.is_empty()),
        );
    }
}

/// Adapts a closure into a [`verbora_core::Tokenizer`], and hence into an
/// [`NGramTokenizer`].
///
/// Useful for the one-off tokenizers that the reference callers write inline, e.g.
/// `NGrams.setTokenizer({ tokenize: s => s.split(' ') })`:
///
/// ```
/// use verbora_ngrams::{FnTokenizer, ngrams_str_with};
///
/// let by_space = FnTokenizer(|s: &str| s.split(' ').map(str::to_owned).collect());
/// let grams = ngrams_str_with(&by_space, "a  b", 2, None, None);
/// // The empty token between the two spaces survives, as it does in the reference.
/// assert_eq!(grams, vec![vec!["a", ""], vec!["", "b"]]);
/// ```
#[derive(Debug, Clone, Copy)]
pub struct FnTokenizer<F>(pub F);

impl<F: Fn(&str) -> Vec<String>> Tokenizer for FnTokenizer<F> {
    fn tokenize_into(&self, text: &str, out: &mut Vec<String>) {
        out.extend((self.0)(text));
    }
}

/// Whether [`set_tokenizer`] has ever been called.
///
/// `Relaxed` is enough: the flag only selects between two paths that are both
/// correct, and the `RwLock` provides the actual synchronisation for the
/// overridden case.
static OVERRIDDEN: AtomicBool = AtomicBool::new(false);

/// The process-global tokenizer, mirroring the reference's module-level `let`.
/// `None` means "still the default".
static GLOBAL: RwLock<Option<Arc<dyn NGramTokenizer>>> = RwLock::new(None);

/// Installs `tokenizer` as the process-global tokenizer for string input.
///
/// Mirrors `NGrams.setTokenizer`, **including** its reach: this affects every
/// subsequent call to [`tokenize`], [`ngrams_str`](crate::ngrams_str) and their
/// relatives, from every thread, until it is replaced or [`reset_tokenizer`] is
/// called. That is the reference's behaviour, not an accident of this port; use
/// [`ngrams_str_with`](crate::ngrams_str_with) to pass a tokenizer explicitly and
/// leave the global alone.
///
/// # Divergence
///
/// `NGrams.setTokenizer` throws `Error('Expected a valid Tokenizer')` for an
/// object without a callable `tokenize`, and a `TypeError` for `null`. Neither is
/// reachable here: the argument's type already proves it can tokenize.
pub fn set_tokenizer<T: NGramTokenizer + 'static>(tokenizer: T) {
    *GLOBAL.write().expect("tokenizer lock poisoned") = Some(Arc::new(tokenizer));
    OVERRIDDEN.store(true, Ordering::Relaxed);
}

/// Restores the default [`WordTokenizer`].
///
/// Has no counterpart in the reference — there is no way to un-set the reference's
/// module-level binding — and exists so that tests exercising the global can
/// isolate themselves from one another.
pub fn reset_tokenizer() {
    *GLOBAL.write().expect("tokenizer lock poisoned") = None;
    OVERRIDDEN.store(false, Ordering::Relaxed);
}

/// The currently installed tokenizer, or `None` while the default is in force.
pub fn current_tokenizer() -> Option<Arc<dyn NGramTokenizer>> {
    if !OVERRIDDEN.load(Ordering::Relaxed) {
        return None;
    }
    GLOBAL.read().expect("tokenizer lock poisoned").clone()
}

/// Tokenizes `text` with the process-global tokenizer.
///
/// Takes no lock at all until [`set_tokenizer`] has been called at least once.
pub fn tokenize(text: &str) -> Vec<String> {
    match current_tokenizer() {
        // The `Arc` is cloned out of the lock before tokenizing, so a tokenizer
        // that itself touches the global cannot deadlock.
        Some(custom) => custom.tokenize_text(text),
        None => WordTokenizer.tokenize(text),
    }
}

/// Serialises the tests that read or mutate the process-global binding.
///
/// The binding is global *by design*, and the tests in one binary share a
/// process, so a test that installs a custom tokenizer would otherwise change
/// what a concurrently running test observes.
#[cfg(test)]
pub(crate) fn exclusive_global() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn word_tokenizer_matches_the_reference_class() {
        let t = WordTokenizer;
        assert_eq!(t.tokenize("a b c"), ["a", "b", "c"]);
        assert_eq!(t.tokenize("  double  spaces  "), ["double", "spaces"]);
        assert_eq!(t.tokenize("don't"), ["don", "t"]);
        assert_eq!(t.tokenize("U.S.A."), ["U", "S", "A"]);
        assert_eq!(t.tokenize("hello_world 123"), ["hello_world", "123"]);
        assert_eq!(t.tokenize("ALLCAPS"), ["ALLCAPS"]);
        assert_eq!(t.tokenize("3.14"), ["3", "14"]);
    }

    #[test]
    fn word_tokenizer_treats_accented_latin_as_separators() {
        let t = WordTokenizer;
        assert_eq!(
            t.tokenize("café naïve 日本 hello_world 123"),
            ["caf", "na", "ve", "hello_world", "123"]
        );
        assert_eq!(t.tokenize("Ångström"), ["ngstr", "m"]);
    }

    #[test]
    fn word_tokenizer_covers_only_the_core_cyrillic_range() {
        let t = WordTokenizer;
        assert_eq!(t.tokenize("Москва"), ["Москва"]);
        // Ё/ё are outside U+0410..=U+044F and so are separators.
        assert_eq!(t.tokenize("Ёж ёлка"), ["ж", "лка"]);
        // Greek is not in the class at all.
        assert!(t.tokenize("Ελλάδα").is_empty());
    }

    #[test]
    fn word_tokenizer_edge_inputs() {
        let t = WordTokenizer;
        assert!(t.tokenize("").is_empty());
        assert!(t.tokenize("   ").is_empty());
        assert!(t.tokenize("!!! ???").is_empty());
        assert!(t.tokenize("😀").is_empty());
        assert_eq!(t.tokenize("a😀b"), ["a", "b"]);
        assert_eq!(t.tokenize("A"), ["A"]);
        let long = "ab ".repeat(5_000);
        assert_eq!(t.tokenize(&long).len(), 5_000);
    }

    #[test]
    fn global_binding_is_process_wide_and_resettable() {
        let _guard = exclusive_global();
        assert!(current_tokenizer().is_none());
        assert_eq!(tokenize("a-b"), ["a", "b"]);

        set_tokenizer(FnTokenizer(|s: &str| {
            s.split('-').map(str::to_owned).collect()
        }));
        assert!(current_tokenizer().is_some());
        assert_eq!(tokenize("a-b"), ["a", "b"]);
        assert_eq!(tokenize("a b"), ["a b"]);

        reset_tokenizer();
        assert!(current_tokenizer().is_none());
        assert_eq!(tokenize("a b"), ["a", "b"]);
    }
}
