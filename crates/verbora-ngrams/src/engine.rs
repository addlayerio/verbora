//! The n-gram engine: one lazy iterator that every other entry point is built on.
//!
//! The reference's `ngrams()` is three loops over one sequence — left padding, the
//! sliding window, right padding — and every public function in
//! The reference `ngrams` module is a call into it with a different `n`. This module is
//! that algorithm, written once, generically over the element type, so the
//! token path (`&str`), the ZH code-unit path (`&[u16]`) and the numeric path
//! (`i64`) all share a single implementation and cannot drift apart.
//!
//! # Why an iterator is the primitive
//!
//! The reference materialises a `string[][]`: every window is a fresh array.
//! Windows over a slice do not need to be copied at all, so [`NGramIter`] yields
//! [`Cow`]: [`Cow::Borrowed`] for the `count` unpadded windows — a pointer and a
//! length, no allocation — and [`Cow::Owned`] only for the `2(n-1)` padded
//! tuples, which genuinely mix pad symbols with sequence elements. Callers who
//! want the reference's shape call [`ngrams`] (a `Vec` of those `Cow`s) or
//! [`ngrams_owned`]; callers streaming a corpus keep the zero-allocation path.

use std::borrow::Cow;
use std::iter::FusedIterator;

/// Where the reference's `Array#slice(len - p, len)` actually starts.
///
/// # The quirk
///
/// Right padding slices with `sequence.slice(sequence.length - p, …)`, and when
/// `n - 1 > sequence.length` the index `p` runs past the end, making the start
/// **negative**. `Array#slice` re-anchors a negative start to `length + start`,
/// clamped at zero — it does *not* clamp to the beginning of the array.
///
/// So for a 3-element sequence and `p == 4`, the reference computes `slice(-1, 3)`
/// and returns just the **last** element, not all three. A port that reaches for
/// `len.saturating_sub(p)` gets `0` and returns the whole sequence, silently
/// producing a different (longer) tuple. This is
/// `NGrams.ngrams('a b c', 5, '<s>', '</s>')`'s two-element `['c', '</s>']`
/// tuple, and it is recorded in the fixtures.
#[inline]
const fn clamp_slice_start(len: usize, p: usize) -> usize {
    if p <= len {
        len - p
    } else {
        // start = len - p is negative here, so the reference uses max(len + (len - p), 0).
        // 2*len cannot overflow: a slice length never exceeds isize::MAX.
        (2 * len).saturating_sub(p)
    }
}

/// The number of unpadded windows, i.e. `_.max([0, sequence.length - n + 1])`.
///
/// `n == 0` is legal and yields `len + 1` windows, each of them **empty** —
/// `slice(i, i)`. Written with a branch rather than `saturating_sub` because
/// `len.saturating_sub(n) + 1` would answer `1` when `n > len`, where the reference
/// answers `0`.
#[inline]
const fn window_count(len: usize, n: usize) -> usize {
    if n > len { 0 } else { len - n + 1 }
}

/// A lazy iterator over the n-grams of a sequence.
///
/// Yields, in this exact order:
///
/// 1. `n - 1` left-padded tuples, if a start symbol was supplied;
/// 2. every window of length `n`, borrowed from the sequence;
/// 3. `n - 1` right-padded tuples, if an end symbol was supplied.
///
/// Padded tuples are **not** always of length `n`: when the sequence is shorter
/// than `n`, the sequence half of the tuple is short. See [`ngrams`] for the
/// worked example.
///
/// Created by [`ngrams_iter`].
#[derive(Debug, Clone)]
pub struct NGramIter<'a, T> {
    seq: &'a [T],
    n: usize,
    start: Option<T>,
    end: Option<T>,
    /// Countdown `n-1 … 1` for the left padding phase; `0` means "phase done".
    left_p: usize,
    /// Next window index.
    i: usize,
    /// Number of windows.
    count: usize,
    /// Countdown `n-1 … 1` for the right padding phase.
    right_p: usize,
}

impl<'a, T: Clone> NGramIter<'a, T> {
    /// Builds the tuple of `p` start symbols followed by `sequence[0 .. n - p]`.
    ///
    /// The tail is `slice(0, n - p)`, and `Array#slice` clamps an end index past
    /// the array length, so the tail is short whenever `n - p > len`. That is
    /// why a padded tuple can be shorter than `n`.
    fn left_tuple(&self, p: usize) -> Cow<'a, [T]> {
        let symbol = self.start.as_ref().expect("left phase requires a symbol");
        let tail = &self.seq[..(self.n - p).min(self.seq.len())];
        let mut out = Vec::with_capacity(p + tail.len());
        out.extend(std::iter::repeat_n(symbol.clone(), p));
        out.extend_from_slice(tail);
        Cow::Owned(out)
    }

    /// Builds the tuple of `sequence[len - p .. len]` followed by `n - p` end
    /// symbols, with the reference's negative-start re-anchoring (see
    /// [`clamp_slice_start`]).
    fn right_tuple(&self, p: usize) -> Cow<'a, [T]> {
        let symbol = self.end.as_ref().expect("right phase requires a symbol");
        let head = &self.seq[clamp_slice_start(self.seq.len(), p)..];
        let blanks = self.n - p;
        let mut out = Vec::with_capacity(head.len() + blanks);
        out.extend_from_slice(head);
        out.extend(std::iter::repeat_n(symbol.clone(), blanks));
        Cow::Owned(out)
    }
}

impl<'a, T: Clone> Iterator for NGramIter<'a, T> {
    type Item = Cow<'a, [T]>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.left_p > 0 {
            let p = self.left_p;
            self.left_p -= 1;
            return Some(self.left_tuple(p));
        }
        if self.i < self.count {
            // The one hot path: a window is a borrow, never a copy.
            let window = &self.seq[self.i..self.i + self.n];
            self.i += 1;
            return Some(Cow::Borrowed(window));
        }
        if self.right_p > 0 {
            let p = self.right_p;
            self.right_p -= 1;
            return Some(self.right_tuple(p));
        }
        None
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.left_p + (self.count - self.i) + self.right_p;
        (remaining, Some(remaining))
    }
}

impl<T: Clone> ExactSizeIterator for NGramIter<'_, T> {}
impl<T: Clone> FusedIterator for NGramIter<'_, T> {}

/// Returns a lazy iterator over the n-grams of `sequence`.
///
/// This is the primitive; [`ngrams`] and friends are thin wrappers. Nothing is
/// computed until the iterator is advanced, and unpadded windows borrow from
/// `sequence` rather than being copied.
///
/// `start_symbol`/`end_symbol` are `None` for both of the reference's "no padding"
/// spellings (`undefined` and `null`). `Some` of an **empty** element still
/// pads — the reference gates on `typeof x !== 'undefined' && x !== null`, not
/// on emptiness, so `NGrams.ngrams('a b c', 2, '')` really does begin with
/// `['', 'a']`.
///
/// ```
/// use verbora_ngrams::ngrams_iter;
///
/// let tokens = ["a", "b", "c"];
/// let grams: Vec<&[&str]> = ngrams_iter(&tokens, 2, None, None)
///     .map(|gram| match gram {
///         // Unpadded windows are borrowed: no copy is made here.
///         std::borrow::Cow::Borrowed(window) => window,
///         std::borrow::Cow::Owned(_) => unreachable!("no padding was requested"),
///     })
///     .collect();
/// assert_eq!(grams, [["a", "b"], ["b", "c"]]);
/// ```
pub fn ngrams_iter<T: Clone>(
    sequence: &[T],
    n: usize,
    start_symbol: Option<T>,
    end_symbol: Option<T>,
) -> NGramIter<'_, T> {
    // `n - 1` padded tuples per side; `n <= 1` means the reference loop
    // `for (p = n - 1; p > 0; p--)` never runs.
    let pad_steps = n.saturating_sub(1);
    NGramIter {
        left_p: if start_symbol.is_some() { pad_steps } else { 0 },
        right_p: if end_symbol.is_some() { pad_steps } else { 0 },
        count: window_count(sequence.len(), n),
        i: 0,
        seq: sequence,
        n,
        start: start_symbol,
        end: end_symbol,
    }
}

/// The n-grams of `sequence`, in the reference's order.
///
/// Mirrors `NGrams.ngrams(sequence, n, startSymbol, endSymbol)` for the
/// array-input form. Unpadded windows are borrowed; call `.into_owned()` on an
/// element, or use [`ngrams_owned`], for owned tuples.
///
/// # Padding is not simply "n-1 symbols on each side"
///
/// The left padding emits, for `p = n-1 … 1`, `p` copies of the start symbol
/// followed by the first `n - p` elements; the right padding is the mirror image
/// *except* that its sequence half is taken with the reference's negative-index
/// slice semantics. Both halves are clamped independently, so tuples shorter
/// than `n` are normal for short sequences:
///
/// ```
/// use verbora_ngrams::ngrams;
///
/// let seq = ["a", "b", "c"];
/// let got: Vec<Vec<&str>> = ngrams(&seq, 5, Some("<s>"), Some("</s>"))
///     .into_iter()
///     .map(std::borrow::Cow::into_owned)
///     .collect();
/// assert_eq!(got, vec![
///     vec!["<s>", "<s>", "<s>", "<s>", "a"],
///     vec!["<s>", "<s>", "<s>", "a", "b"],
///     vec!["<s>", "<s>", "a", "b", "c"],
///     vec!["<s>", "a", "b", "c"],   // tail clamped: only 3 elements exist
///     vec!["c", "</s>"],            // slice(-1, 3) — the last element only
///     vec!["a", "b", "c", "</s>", "</s>"],
///     vec!["b", "c", "</s>", "</s>", "</s>"],
///     vec!["c", "</s>", "</s>", "</s>", "</s>"],
/// ]);
/// ```
pub fn ngrams<T: Clone>(
    sequence: &[T],
    n: usize,
    start_symbol: Option<T>,
    end_symbol: Option<T>,
) -> Vec<Cow<'_, [T]>> {
    ngrams_iter(sequence, n, start_symbol, end_symbol).collect()
}

/// [`ngrams`] with every tuple materialised, matching the reference's
/// `string[][]` shape exactly.
///
/// Prefer [`ngrams`] or [`ngrams_iter`] unless you need owned tuples: this
/// copies each window.
pub fn ngrams_owned<T: Clone>(
    sequence: &[T],
    n: usize,
    start_symbol: Option<T>,
    end_symbol: Option<T>,
) -> Vec<Vec<T>> {
    ngrams_iter(sequence, n, start_symbol, end_symbol)
        .map(Cow::into_owned)
        .collect()
}

/// The 2-grams of `sequence` — `NGrams.bigrams`.
pub fn bigrams<T: Clone>(
    sequence: &[T],
    start_symbol: Option<T>,
    end_symbol: Option<T>,
) -> Vec<Cow<'_, [T]>> {
    ngrams(sequence, 2, start_symbol, end_symbol)
}

/// The 3-grams of `sequence` — `NGrams.trigrams`.
pub fn trigrams<T: Clone>(
    sequence: &[T],
    start_symbol: Option<T>,
    end_symbol: Option<T>,
) -> Vec<Cow<'_, [T]>> {
    ngrams(sequence, 3, start_symbol, end_symbol)
}

/// An exact alias of [`ngrams`] — `NGrams.multrigrams`.
///
/// The reference's `multrigrams` is literally
/// `return ngrams(sequence, n, startSymbol, endSymbol, stats)`. It is kept here
/// so that code ported from the reference keeps compiling under its original name;
/// there is no behavioural difference to look for.
pub fn multrigrams<T: Clone>(
    sequence: &[T],
    n: usize,
    start_symbol: Option<T>,
    end_symbol: Option<T>,
) -> Vec<Cow<'_, [T]>> {
    ngrams(sequence, n, start_symbol, end_symbol)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Collects into owned tuples so assertions read like the reference output.
    fn owned<'a, I: IntoIterator<Item = Cow<'a, [&'a str]>>>(it: I) -> Vec<Vec<&'a str>> {
        it.into_iter().map(Cow::into_owned).collect()
    }

    #[test]
    fn plain_windows_borrow() {
        let seq = ["a", "b", "c"];
        let grams = ngrams(&seq, 2, None, None);
        assert!(grams.iter().all(|g| matches!(g, Cow::Borrowed(_))));
        assert_eq!(owned(grams), vec![vec!["a", "b"], vec!["b", "c"]]);
    }

    #[test]
    fn unigrams_never_pad() {
        // The padding loop is `for (p = n - 1; p > 0; p--)`, so n == 1 skips it
        // even though symbols were supplied.
        let seq = ["a", "b", "c"];
        assert_eq!(
            owned(ngrams(&seq, 1, Some("<s>"), Some("</s>"))),
            vec![vec!["a"], vec!["b"], vec!["c"]]
        );
    }

    #[test]
    fn zero_n_yields_len_plus_one_empty_tuples() {
        let seq = ["a", "b", "c"];
        assert_eq!(
            owned(ngrams(&seq, 0, None, None)),
            vec![Vec::<&str>::new(); 4]
        );
    }

    #[test]
    fn n_greater_than_len_yields_no_windows() {
        let seq = ["a", "b", "c"];
        assert!(ngrams(&seq, 5, None, None).is_empty());
    }

    #[test]
    fn empty_sequence_still_pads() {
        let seq: [&str; 0] = [];
        assert_eq!(
            owned(ngrams(&seq, 2, Some("<s>"), Some("</s>"))),
            vec![vec!["<s>"], vec!["</s>"]]
        );
        assert_eq!(
            owned(ngrams(&seq, 3, Some("S"), Some("E"))),
            vec![vec!["S", "S"], vec!["S"], vec!["E"], vec!["E", "E"]]
        );
    }

    #[test]
    fn empty_symbol_enables_padding() {
        // `''` is not `null`: The reference pads with it.
        let seq = ["a", "b", "c"];
        assert_eq!(
            owned(ngrams(&seq, 2, Some(""), None)),
            vec![vec!["", "a"], vec!["a", "b"], vec!["b", "c"]]
        );
    }

    #[test]
    fn right_padding_reanchors_a_negative_slice_start() {
        // len 1, n 3: p == 2 gives slice(-1, 1), i.e. the LAST element.
        let seq = ["a"];
        assert_eq!(
            owned(ngrams(&seq, 3, Some("<s>"), Some("</s>"))),
            vec![
                vec!["<s>", "<s>", "a"],
                vec!["<s>", "a"],
                vec!["a", "</s>"],
                vec!["a", "</s>", "</s>"],
            ]
        );
        // len 2, n 4: p == 3 gives slice(-1, 2) => ['b'], not ['a','b'].
        let seq = ["a", "b"];
        assert_eq!(
            owned(ngrams(&seq, 4, Some("S"), Some("E"))),
            vec![
                vec!["S", "S", "S", "a"],
                vec!["S", "S", "a", "b"],
                vec!["S", "a", "b"],
                vec!["b", "E"],
                vec!["a", "b", "E", "E"],
                vec!["b", "E", "E", "E"],
            ]
        );
    }

    #[test]
    fn negative_start_clamps_at_zero_when_doubly_out_of_range() {
        // len 1, p 5: len + (len - p) == -3, clamped to 0.
        let seq = ["a"];
        let got = owned(ngrams(&seq, 6, Some("S"), Some("E")));
        assert_eq!(got[5], vec!["a", "E"]);
        assert_eq!(got.last().unwrap(), &vec!["a", "E", "E", "E", "E", "E"]);
    }

    #[test]
    fn slice_start_matches_the_reference() {
        assert_eq!(clamp_slice_start(3, 1), 2);
        assert_eq!(clamp_slice_start(3, 3), 0);
        assert_eq!(clamp_slice_start(3, 4), 2); // slice(-1, 3)
        assert_eq!(clamp_slice_start(1, 5), 0); // slice(-4, 1) => max(1-4, 0)
        assert_eq!(clamp_slice_start(0, 2), 0);
    }

    #[test]
    fn size_hint_is_exact() {
        let seq = ["a", "b", "c"];
        let mut it = ngrams_iter(&seq, 2, Some("<s>"), Some("</s>"));
        assert_eq!(it.len(), 4);
        it.next();
        assert_eq!(it.len(), 3);
        assert_eq!(it.count(), 3);
    }

    #[test]
    fn aliases_agree_with_ngrams() {
        let seq = ["a", "b", "c", "d"];
        assert_eq!(bigrams(&seq, None, None), ngrams(&seq, 2, None, None));
        assert_eq!(trigrams(&seq, None, None), ngrams(&seq, 3, None, None));
        assert_eq!(
            multrigrams(&seq, 3, Some("S"), None),
            ngrams(&seq, 3, Some("S"), None)
        );
    }

    #[test]
    fn works_on_non_string_elements() {
        let seq = [1_i64, 2, 3];
        assert_eq!(
            ngrams_owned(&seq, 2, None, Some(0)),
            vec![vec![1, 2], vec![2, 3], vec![3, 0]]
        );
    }

    #[test]
    fn very_long_input_is_linear_and_borrowed() {
        let seq: Vec<String> = (0..10_000).map(|i| i.to_string()).collect();
        let grams = ngrams(&seq, 3, None, None);
        assert_eq!(grams.len(), 9_998);
        assert!(grams.iter().all(|g| matches!(g, Cow::Borrowed(_))));
    }
}
