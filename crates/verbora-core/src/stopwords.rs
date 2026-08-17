//! Stop-word lists and the shared, mutable stop-word state.
//!
//! # Why this module is more complicated than it looks
//!
//! In the reference, the reference `stopwords` exports a single mutable array
//! (`exports.words`). Every stemmer's `addStopWord`/`removeStopWord` mutates
//! *that one array*, and both the stemmers and the phonetics module read from it.
//! Adding a stop word through one stemmer therefore changes the behaviour of
//! every other stemmer and of `tokenizeAndPhoneticize`, process-wide.
//!
//! That is genuinely part of the observable behaviour, so `verbora` reproduces
//! it rather than quietly "fixing" it — but it reproduces it in a way that is
//! thread-safe and that costs nothing when the feature is unused:
//!
//! * The default list is a `&'static [&'static str]` with no runtime setup.
//! * A [`std::sync::atomic::AtomicBool`] records whether the global has ever been
//!   mutated. Until it has, [`is_default_stopword`] answers from the static list
//!   via binary search — no lock, no allocation.
//! * Only after a mutation does the lookup consult the locked global list.
//!
//! Code that wants no global state at all should construct its own
//! [`StopWords`] and pass it explicitly; every consumer in this workspace accepts
//! one.

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{LazyLock, RwLock};

/// The default English stop-word list, in reference source order.
///
/// Order is preserved because the reference exposes this array directly as
/// The reference's `stopwords` array, so callers can observe it.
pub static DEFAULT_EN: &[&str] = &[
    "about",
    "above",
    "after",
    "again",
    "all",
    "also",
    "am",
    "an",
    "and",
    "another",
    "any",
    "are",
    "as",
    "at",
    "be",
    "because",
    "been",
    "before",
    "being",
    "below",
    "between",
    "both",
    "but",
    "by",
    "came",
    "can",
    "cannot",
    "come",
    "could",
    "did",
    "do",
    "does",
    "doing",
    "during",
    "each",
    "few",
    "for",
    "from",
    "further",
    "get",
    "got",
    "has",
    "had",
    "he",
    "have",
    "her",
    "here",
    "him",
    "himself",
    "his",
    "how",
    "if",
    "in",
    "into",
    "is",
    "it",
    "its",
    "itself",
    "like",
    "make",
    "many",
    "me",
    "might",
    "more",
    "most",
    "much",
    "must",
    "my",
    "myself",
    "never",
    "now",
    "of",
    "on",
    "only",
    "or",
    "other",
    "our",
    "ours",
    "ourselves",
    "out",
    "over",
    "own",
    "said",
    "same",
    "see",
    "she",
    "should",
    "since",
    "so",
    "some",
    "still",
    "such",
    "take",
    "than",
    "that",
    "the",
    "their",
    "theirs",
    "them",
    "themselves",
    "then",
    "there",
    "these",
    "they",
    "this",
    "those",
    "through",
    "to",
    "too",
    "under",
    "until",
    "up",
    "very",
    "was",
    "way",
    "we",
    "well",
    "were",
    "what",
    "where",
    "when",
    "which",
    "while",
    "who",
    "whom",
    "with",
    "would",
    "why",
    "you",
    "your",
    "yours",
    "yourself",
    "a",
    "b",
    "c",
    "d",
    "e",
    "f",
    "g",
    "h",
    "i",
    "j",
    "k",
    "l",
    "m",
    "n",
    "o",
    "p",
    "q",
    "r",
    "s",
    "t",
    "u",
    "v",
    "w",
    "x",
    "y",
    "z",
    "$",
    "1",
    "2",
    "3",
    "4",
    "5",
    "6",
    "7",
    "8",
    "9",
    "0",
    "_",
];

/// [`DEFAULT_EN`] sorted, for branch-predictable binary-search membership tests.
///
/// Built once on first use. Sorting here rather than hand-maintaining a second
/// literal keeps the two lists from drifting apart; a unit test asserts they
/// remain permutations of one another.
static DEFAULT_EN_SORTED: LazyLock<Box<[&'static str]>> = LazyLock::new(|| {
    let mut v = DEFAULT_EN.to_vec();
    v.sort_unstable();
    v.into_boxed_slice()
});

/// Whether the process-global list has ever been mutated.
///
/// `Relaxed` is sufficient: this flag only selects between two lookup
/// strategies that are both correct, and the `RwLock` provides the actual
/// synchronisation for the mutated case.
static GLOBAL_MUTATED: AtomicBool = AtomicBool::new(false);

/// The process-global stop-word list, mirroring `stopwords.words` in the reference.
static GLOBAL: LazyLock<RwLock<StopWords>> = LazyLock::new(|| RwLock::new(StopWords::english()));

/// An ordered stop-word list with O(1) membership testing.
///
/// Insertion order is preserved (so the list can be exposed the way the reference
/// exposes its array) while lookups go through a hash set.
#[derive(Debug, Clone, Default)]
pub struct StopWords {
    ordered: Vec<String>,
    lookup: HashSet<String>,
}

impl StopWords {
    /// Creates an empty list.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates the default English list.
    pub fn english() -> Self {
        Self::from_iter_of(DEFAULT_EN.iter().copied())
    }

    /// Builds a list from any iterator of string-likes, preserving order and
    /// dropping duplicates from the lookup set (but not from the ordered view,
    /// which mirrors the reference array faithfully).
    pub fn from_iter_of<I, S>(words: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let ordered: Vec<String> = words.into_iter().map(Into::into).collect();
        let lookup = ordered.iter().cloned().collect();
        Self { ordered, lookup }
    }

    /// Returns whether `word` is in the list.
    pub fn contains(&self, word: &str) -> bool {
        self.lookup.contains(word)
    }

    /// Returns the words in insertion order.
    pub fn words(&self) -> &[String] {
        &self.ordered
    }

    /// Returns the number of words.
    pub fn len(&self) -> usize {
        self.ordered.len()
    }

    /// Returns whether the list is empty.
    pub fn is_empty(&self) -> bool {
        self.ordered.is_empty()
    }

    /// Appends a stop word.
    ///
    /// Mirrors the reference `addStopWord`, which pushes unconditionally — so a
    /// duplicate does appear twice in [`Self::words`], exactly as it would in
    /// the reference.
    pub fn add(&mut self, word: impl Into<String>) {
        let word = word.into();
        self.ordered.push(word.clone());
        self.lookup.insert(word);
    }

    /// Appends several stop words.
    pub fn add_all<I, S>(&mut self, words: I)
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        for w in words {
            self.add(w);
        }
    }

    /// Removes the **first** occurrence of `word`.
    ///
    /// The reference uses `indexOf` + `splice(idx, 1)`, which removes only the first
    /// match; a word added twice therefore needs removing twice. That is
    /// reproduced here.
    pub fn remove(&mut self, word: &str) {
        if let Some(idx) = self.ordered.iter().position(|w| w == word) {
            self.ordered.remove(idx);
            // Only drop from the lookup set once no occurrences remain.
            if !self.ordered.iter().any(|w| w == word) {
                self.lookup.remove(word);
            }
        }
    }

    /// Removes the first occurrence of each of `words`.
    pub fn remove_all<'a, I>(&mut self, words: I)
    where
        I: IntoIterator<Item = &'a str>,
    {
        for w in words {
            self.remove(w);
        }
    }
}

impl FromIterator<String> for StopWords {
    fn from_iter<I: IntoIterator<Item = String>>(iter: I) -> Self {
        Self::from_iter_of(iter)
    }
}

/// Tests membership against the process-global stop-word list.
///
/// Fast path (no mutation has ever occurred): a binary search over a static
/// sorted slice, with no locking and no allocation.
pub fn is_default_stopword(word: &str) -> bool {
    if !GLOBAL_MUTATED.load(Ordering::Relaxed) {
        return DEFAULT_EN_SORTED.binary_search(&word).is_ok();
    }
    GLOBAL
        .read()
        .expect("stop-word lock poisoned")
        .contains(word)
}

/// Adds a stop word to the process-global list.
///
/// Mirrors `stemmer.addStopWord`. This affects every stemmer and the phonetics
/// helpers, process-wide, exactly as it does in the reference.
pub fn add_global_stopword(word: impl Into<String>) {
    GLOBAL.write().expect("stop-word lock poisoned").add(word);
    GLOBAL_MUTATED.store(true, Ordering::Relaxed);
}

/// Adds several stop words to the process-global list.
pub fn add_global_stopwords<I, S>(words: I)
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    GLOBAL
        .write()
        .expect("stop-word lock poisoned")
        .add_all(words);
    GLOBAL_MUTATED.store(true, Ordering::Relaxed);
}

/// Removes a stop word from the process-global list.
pub fn remove_global_stopword(word: &str) {
    GLOBAL
        .write()
        .expect("stop-word lock poisoned")
        .remove(word);
    GLOBAL_MUTATED.store(true, Ordering::Relaxed);
}

/// Removes several stop words from the process-global list.
pub fn remove_global_stopwords<'a, I>(words: I)
where
    I: IntoIterator<Item = &'a str>,
{
    GLOBAL
        .write()
        .expect("stop-word lock poisoned")
        .remove_all(words);
    GLOBAL_MUTATED.store(true, Ordering::Relaxed);
}

/// Returns a snapshot of the process-global list, in order.
///
/// Corresponds to reading the reference's `stopwords` array in the reference.
pub fn global_stopwords() -> Vec<String> {
    if !GLOBAL_MUTATED.load(Ordering::Relaxed) {
        return DEFAULT_EN.iter().map(|s| (*s).to_owned()).collect();
    }
    GLOBAL
        .read()
        .expect("stop-word lock poisoned")
        .words()
        .to_vec()
}

/// Restores the process-global list to the default English list.
///
/// Has no counterpart in the reference; provided so that tests which exercise the
/// global-mutation behaviour can isolate themselves from one another.
pub fn reset_global_stopwords() {
    *GLOBAL.write().expect("stop-word lock poisoned") = StopWords::english();
    GLOBAL_MUTATED.store(false, Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sorted_list_is_a_permutation_of_source_order() {
        // Guards against the two lists drifting apart.
        assert_eq!(DEFAULT_EN_SORTED.len(), DEFAULT_EN.len());
        let mut a = DEFAULT_EN.to_vec();
        a.sort_unstable();
        assert_eq!(&*a, &**DEFAULT_EN_SORTED);
        assert!(DEFAULT_EN_SORTED.windows(2).all(|w| w[0] <= w[1]));
    }

    #[test]
    fn default_membership_matches_linear_scan() {
        for w in DEFAULT_EN {
            assert!(is_default_stopword(w), "{w} should be a stop word");
        }
        for w in ["zebra", "quixotic", "", "About"] {
            assert!(!is_default_stopword(w), "{w} should not be a stop word");
        }
    }

    #[test]
    fn add_is_unconditional_like_the_reference() {
        let mut s = StopWords::new();
        s.add("dup");
        s.add("dup");
        // The reference `push` does not deduplicate.
        assert_eq!(s.words(), &["dup", "dup"]);
        assert!(s.contains("dup"));

        // ...and `remove` splices only the first match.
        s.remove("dup");
        assert_eq!(s.words(), &["dup"]);
        assert!(s.contains("dup"));

        s.remove("dup");
        assert!(!s.contains("dup"));
        assert!(s.is_empty());
    }

    #[test]
    fn removing_absent_word_is_a_noop() {
        let mut s = StopWords::english();
        let before = s.len();
        s.remove("definitely-not-present");
        assert_eq!(s.len(), before);
    }
}
