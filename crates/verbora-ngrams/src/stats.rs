//! Frequency statistics — the `stats` argument of `NGrams.ngrams`.
//!
//! When the reference is called with a truthy fifth argument it returns an
//! object instead of an array:
//!
//! ```text
//! { ngrams, frequencies, Nr, numberOfNgrams }
//! ```
//!
//! [`NGramStats`] is that object. Two of its properties have **observable
//! iteration order**, and they do not have the same one, so they are represented
//! by different containers here — see the type's documentation.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::fmt::{self, Write as _};

use rustc_hash::FxHashMap;

use crate::engine::ngrams_iter;

/// Renders an n-gram the way the reference's private `arrayToKey` does:
/// `["a", "b"]` becomes `"(a, b)"`.
///
/// # The empty-n-gram quirk
///
/// `arrayToKey` builds `"(" + "a, " + "b, "` and then chops the trailing `", "`
/// with `result.substr(0, result.length - 2)`. For an **empty** n-gram — which
/// `n == 0` produces, one per position — the buffer is just `"("`, so the length
/// is 1 and `substr(0, -1)` is called. `String#substr` clamps a negative length
/// to zero (`String#slice` would have counted from the end), so the result is
/// the empty string and the key comes out as a lone `")"`, not `"()"`.
///
/// A port that writes `&s[..len - 2]` panics on that input instead.
/// `NGrams.ngrams('a b', 0, null, null, true).frequencies` really is `{")": 3}`.
///
/// ```
/// use verbora_ngrams::ngram_key;
///
/// assert_eq!(ngram_key(&["a", "b"]), "(a, b)");
/// assert_eq!(ngram_key(&["a"]), "(a)");
/// assert_eq!(ngram_key::<&str>(&[]), ")");
/// ```
pub fn ngram_key<T: fmt::Display>(ngram: &[T]) -> String {
    if ngram.is_empty() {
        return String::from(")");
    }
    // "(" + elements + ", " separators + ")". Sixteen bytes per element is a
    // deliberate over-estimate: it covers a typical word plus its separator in
    // one allocation, and one over-sized allocation is cheaper than a realloc.
    let mut key = String::with_capacity(2 + ngram.len() * 16);
    key.push('(');
    for (i, item) in ngram.iter().enumerate() {
        if i > 0 {
            key.push_str(", ");
        }
        // Writing into a String is infallible.
        let _ = write!(key, "{item}");
    }
    key.push(')');
    key
}

/// The n-grams of a sequence together with their frequency statistics.
///
/// Mirrors the object `NGrams.ngrams(…, stats)` returns when `stats` is truthy.
///
/// # Why `frequencies` is a `Vec` and `nr` is a `BTreeMap`
///
/// Both are plain the reference objects in the reference, and both have an order
/// that callers can observe with `Object.keys` — but not the *same* order.
///
/// * `frequencies` is keyed by [`ngram_key`], which always starts with `(` or is
///   `)`. Those are not integer-like, so the reference engine enumerates them in **insertion
///   order**, i.e. the order the n-grams were first seen. A `HashMap` loses that
///   and a `BTreeMap` would replace it with lexicographic order, so this is a
///   `Vec` of pairs. [`Self::frequency`] does the lookup.
/// * `Nr` is keyed by a frequency *count*, so its keys are integer-like, and
///   the reference hoists those to the front in **ascending numeric order**
///   regardless of insertion order. A corpus that produces the count 10 before
///   the count 2 still enumerates `2` first, which is exactly what
///   `BTreeMap<u64, _>` gives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NGramStats<'a, T: Clone> {
    /// The n-grams themselves, in generation order, exactly as [`crate::ngrams`]
    /// returns them.
    pub ngrams: Vec<Cow<'a, [T]>>,
    /// `(key, count)` pairs in first-seen order, keyed by [`ngram_key`].
    pub frequencies: Vec<(String, u64)>,
    /// How many distinct n-grams occurred exactly `r` times, keyed by `r`.
    ///
    /// This is the "count of counts" a Good–Turing estimator consumes.
    pub nr: BTreeMap<u64, u64>,
    /// Total number of n-grams counted, padding tuples included.
    pub number_of_ngrams: u64,
}

impl<T: Clone> NGramStats<'_, T> {
    /// Detaches the n-grams from the sequence they were built over.
    ///
    /// Copies the windows that were still borrowed; the padded tuples were owned
    /// already. Needed when the sequence is a temporary — for instance when it
    /// came from tokenizing a string inside the same call.
    pub fn into_owned(self) -> NGramStats<'static, T> {
        NGramStats {
            ngrams: self
                .ngrams
                .into_iter()
                .map(|gram| Cow::Owned(gram.into_owned()))
                .collect(),
            frequencies: self.frequencies,
            nr: self.nr,
            number_of_ngrams: self.number_of_ngrams,
        }
    }

    /// How many times the n-gram with this key occurred; `0` if it never did.
    ///
    /// Linear in the number of distinct n-grams: the type preserves the reference's
    /// insertion order, and lookups are not the reason this structure exists.
    /// Build a `HashMap` from [`Self::frequencies`] if you need many lookups.
    pub fn frequency(&self, key: &str) -> u64 {
        self.frequencies
            .iter()
            .find(|(k, _)| k == key)
            .map_or(0, |(_, c)| *c)
    }
}

/// The n-grams of `sequence` plus frequency statistics — `NGrams.ngrams` with a
/// truthy `stats` argument.
///
/// Padded tuples are counted too, and they are counted under their own key, so
/// `numberOfNgrams` is the length of `ngrams` and not the number of windows.
///
/// ```
/// use verbora_ngrams::ngrams_with_stats;
///
/// let seq = ["a", "b", "a", "b", "a"];
/// let stats = ngrams_with_stats(&seq, 2, None, None);
/// assert_eq!(stats.number_of_ngrams, 4);
/// assert_eq!(stats.frequency("(a, b)"), 2);
/// assert_eq!(stats.nr.get(&2), Some(&2)); // two n-grams occur twice
/// ```
pub fn ngrams_with_stats<T: Clone + fmt::Display>(
    sequence: &[T],
    n: usize,
    start_symbol: Option<T>,
    end_symbol: Option<T>,
) -> NGramStats<'_, T> {
    let iter = ngrams_iter(sequence, n, start_symbol, end_symbol);
    let mut ngrams = Vec::with_capacity(iter.len());
    let mut number_of_ngrams = 0;

    // Counting needs a map; the public view needs first-seen order. Rather than
    // keep the key in both — one `String` clone per distinct n-gram — the map
    // remembers *where* each key belongs, and the ordered `Vec` is filled by
    // moving the keys out of the map once, at the end.
    let mut index: FxHashMap<String, (usize, u64)> = FxHashMap::default();

    for gram in iter {
        number_of_ngrams += 1;
        let position = index.len();
        index
            .entry(ngram_key(&gram))
            .and_modify(|(_, count)| *count += 1)
            .or_insert((position, 1));
        ngrams.push(gram);
    }

    let mut slots: Vec<Option<(String, u64)>> = (0..index.len()).map(|_| None).collect();
    let mut nr: BTreeMap<u64, u64> = BTreeMap::new();
    for (key, (position, count)) in index {
        *nr.entry(count).or_insert(0) += 1;
        slots[position] = Some((key, count));
    }

    NGramStats {
        ngrams,
        frequencies: slots
            .into_iter()
            .map(|slot| slot.expect("every position was assigned exactly once"))
            .collect(),
        nr,
        number_of_ngrams,
    }
}

/// [`ngrams_with_stats`] with `n = 2` — `NGrams.bigrams(…, stats)`.
pub fn bigrams_with_stats<T: Clone + fmt::Display>(
    sequence: &[T],
    start_symbol: Option<T>,
    end_symbol: Option<T>,
) -> NGramStats<'_, T> {
    ngrams_with_stats(sequence, 2, start_symbol, end_symbol)
}

/// [`ngrams_with_stats`] with `n = 3` — `NGrams.trigrams(…, stats)`.
pub fn trigrams_with_stats<T: Clone + fmt::Display>(
    sequence: &[T],
    start_symbol: Option<T>,
    end_symbol: Option<T>,
) -> NGramStats<'_, T> {
    ngrams_with_stats(sequence, 3, start_symbol, end_symbol)
}

/// An exact alias of [`ngrams_with_stats`] — `NGrams.multrigrams(…, stats)`.
pub fn multrigrams_with_stats<T: Clone + fmt::Display>(
    sequence: &[T],
    n: usize,
    start_symbol: Option<T>,
    end_symbol: Option<T>,
) -> NGramStats<'_, T> {
    ngrams_with_stats(sequence, n, start_symbol, end_symbol)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_format_matches_the_reference() {
        assert_eq!(ngram_key(&["a", "b"]), "(a, b)");
        assert_eq!(ngram_key(&["a"]), "(a)");
        assert_eq!(ngram_key(&[1, 2]), "(1, 2)");
        // Elements are concatenated raw: separators inside a token are not
        // escaped, so keys are not injective. That is the reference's behaviour.
        assert_eq!(ngram_key(&["a, b"]), "(a, b)");
    }

    #[test]
    fn empty_ngram_key_is_a_lone_closing_paren() {
        assert_eq!(ngram_key::<&str>(&[]), ")");
        let seq = ["a", "b"];
        let stats = ngrams_with_stats(&seq, 0, None, None);
        assert_eq!(stats.frequencies, vec![(String::from(")"), 3)]);
        assert_eq!(stats.nr, BTreeMap::from([(3, 1)]));
        assert_eq!(stats.number_of_ngrams, 3);
    }

    #[test]
    fn frequencies_keep_first_seen_order() {
        let seq = ["b", "a", "b", "a", "b"];
        let stats = ngrams_with_stats(&seq, 2, None, None);
        assert_eq!(
            stats.frequencies,
            vec![(String::from("(b, a)"), 2), (String::from("(a, b)"), 2)]
        );
    }

    #[test]
    fn nr_is_ordered_by_count_not_by_insertion() {
        // "a" is seen 10 times before "b" is seen twice, so insertion order
        // would put 10 first. The reference's integer-key hoisting puts 2 first.
        let mut seq = vec!["a"; 10];
        seq.extend(["b", "b"]);
        let stats = ngrams_with_stats(&seq, 1, None, None);
        assert_eq!(
            stats.frequencies,
            vec![(String::from("(a)"), 10), (String::from("(b)"), 2)]
        );
        assert_eq!(stats.nr.keys().copied().collect::<Vec<_>>(), vec![2, 10]);
    }

    #[test]
    fn padded_tuples_are_counted() {
        let seq = ["a", "b"];
        let stats = ngrams_with_stats(&seq, 2, Some("S"), Some("E"));
        assert_eq!(stats.number_of_ngrams, 3);
        assert_eq!(stats.frequency("(S, a)"), 1);
        assert_eq!(stats.frequency("(b, E)"), 1);
        assert_eq!(stats.frequency("(nope)"), 0);
    }

    #[test]
    fn empty_sequence_without_padding_has_no_counts() {
        let seq: [&str; 0] = [];
        let stats = ngrams_with_stats(&seq, 2, None, None);
        assert_eq!(stats.number_of_ngrams, 0);
        assert!(stats.frequencies.is_empty());
        assert!(stats.nr.is_empty());
    }

    #[test]
    fn aliases_agree() {
        let seq = ["a", "b", "c"];
        assert_eq!(
            bigrams_with_stats(&seq, None, None),
            ngrams_with_stats(&seq, 2, None, None)
        );
        assert_eq!(
            trigrams_with_stats(&seq, None, None),
            ngrams_with_stats(&seq, 3, None, None)
        );
        assert_eq!(
            multrigrams_with_stats(&seq, 2, None, None),
            ngrams_with_stats(&seq, 2, None, None)
        );
    }
}
