//! Character windows over a `&str`, yielded as borrowed substrings.
//!
//! Private. Everything here is re-exported from the crate root, which carries
//! the user-facing prose.

use std::iter::FusedIterator;
use std::num::NonZeroUsize;

/// The `n`-grams of `text`'s Unicode scalars, as borrowed slices of `text`.
///
/// Each yielded window is a `&str` holding exactly `n` consecutive Unicode
/// scalar values (`char`s), in order, and is a substring of `text` — so no
/// window can contain a scalar `text` does not, and in particular none can
/// contain `U+FFFD` unless `text` does.
///
/// `n` larger than the number of scalars in `text` yields an empty iterator,
/// and so does an empty `text`.
///
/// # The unit is the scalar, not the grapheme cluster
///
/// A window of `n` scalars may split a combining sequence: `char_ngrams` over
/// `"e\u{0301}f"` with `n = 2` yields `"e\u{0301}"` and `"\u{0301}f"`, the
/// second of which begins with a combining acute. That is deliberate.
/// Grapheme clusters would avoid it, but they change with the Unicode version
/// and would tie character n-gram keys — which language identification
/// persists — to that version. A caller who wants grapheme windows segments
/// first and builds a [`Padded`](crate::Padded) over the resulting slice.
///
/// # Cost
///
/// `char_ngrams` counts the scalars of `text` when it is called, so that
/// [`ExactSizeIterator::len`] is exact from the first call; that is one pass
/// over `text`. Iteration itself allocates nothing and copies nothing.
///
/// # Examples
///
/// ```
/// use std::num::NonZeroUsize;
/// use verbora_ngrams::char_ngrams;
///
/// let n = NonZeroUsize::new(2).expect("2 is not zero");
/// let grams: Vec<&str> = char_ngrams("abc", n).collect();
/// assert_eq!(grams, ["ab", "bc"]);
/// ```
///
/// Astral scalars are scalars like any other; nothing is split into surrogate
/// halves and nothing is replaced:
///
/// ```
/// use std::num::NonZeroUsize;
/// use verbora_ngrams::char_ngrams;
///
/// let n = NonZeroUsize::new(2).expect("2 is not zero");
/// let grams: Vec<&str> = char_ngrams("👍你好", n).collect();
/// assert_eq!(grams, ["👍你", "你好"]);
///
/// let n1 = NonZeroUsize::new(1).expect("1 is not zero");
/// assert_eq!(char_ngrams("👍", n1).collect::<Vec<_>>(), ["👍"]);
/// ```
///
/// A window wider than the input yields nothing:
///
/// ```
/// use std::num::NonZeroUsize;
/// use verbora_ngrams::char_ngrams;
///
/// let n = NonZeroUsize::new(3).expect("3 is not zero");
/// assert_eq!(char_ngrams("ab", n).count(), 0);
/// assert_eq!(char_ngrams("", n).count(), 0);
/// ```
#[must_use]
#[inline]
pub fn char_ngrams(text: &str, n: NonZeroUsize) -> CharNGrams<'_> {
    CharNGrams::new(text, n)
}

/// Iterator over the character `n`-grams of a `&str`, created by
/// [`char_ngrams`].
///
/// Yields `&'a str` borrowed from the original text, in left-to-right order.
/// It is an [`ExactSizeIterator`] — [`len`](ExactSizeIterator::len) is
/// `text.chars().count().saturating_sub(n - 1)` — a [`FusedIterator`], and a
/// [`DoubleEndedIterator`], so `.rev()` walks the same windows from the right
/// and mixing `next` with `next_back` consumes from both ends of the same
/// sequence.
///
/// # Examples
///
/// ```
/// use std::num::NonZeroUsize;
/// use verbora_ngrams::char_ngrams;
///
/// let n = NonZeroUsize::new(2).expect("2 is not zero");
/// let mut it = char_ngrams("abcd", n);
///
/// assert_eq!(it.len(), 3);
/// assert_eq!(it.next(), Some("ab"));
/// assert_eq!(it.next_back(), Some("cd"));
/// assert_eq!(it.len(), 1);
/// assert_eq!(it.next(), Some("bc"));
/// assert_eq!(it.next(), None);
/// assert_eq!(it.next_back(), None);
/// ```
#[derive(Debug, Clone)]
pub struct CharNGrams<'a> {
    /// The text every yielded window borrows from.
    text: &'a str,
    /// Byte index at which the next front window starts.
    front: usize,
    /// Byte index one past the end of the next front window.
    front_end: usize,
    /// Byte index at which the next back window starts.
    back: usize,
    /// Byte index one past the end of the next back window.
    back_end: usize,
    /// Windows not yet yielded, from either end.
    remaining: usize,
}

impl<'a> CharNGrams<'a> {
    /// Positions both cursors and counts the windows.
    ///
    /// `remaining` is `scalars - (n - 1)` floored at zero, so `remaining > 0`
    /// implies `text` holds at least `n` scalars — which is what makes the two
    /// boundary walks below terminate inside `text`.
    fn new(text: &'a str, n: NonZeroUsize) -> Self {
        let n = n.get();
        let remaining = text.chars().count().saturating_sub(n - 1);
        if remaining == 0 {
            return Self {
                text,
                front: 0,
                front_end: 0,
                back: 0,
                back_end: 0,
                remaining: 0,
            };
        }

        let mut front_end = 0;
        for _ in 0..n {
            front_end = next_boundary(text, front_end);
        }
        let mut back = text.len();
        for _ in 0..n {
            back = prev_boundary(text, back);
        }

        Self {
            text,
            front: 0,
            front_end,
            back,
            back_end: text.len(),
            remaining,
        }
    }
}

impl<'a> Iterator for CharNGrams<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }
        let window = &self.text[self.front..self.front_end];
        self.remaining -= 1;
        if self.remaining > 0 {
            // Another window remains between the cursors, so both ends of the
            // front window have a scalar after them.
            self.front = next_boundary(self.text, self.front);
            self.front_end = next_boundary(self.text, self.front_end);
        }
        Some(window)
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, Some(self.remaining))
    }

    #[inline]
    fn count(self) -> usize {
        self.remaining
    }
}

impl DoubleEndedIterator for CharNGrams<'_> {
    fn next_back(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }
        let window = &self.text[self.back..self.back_end];
        self.remaining -= 1;
        if self.remaining > 0 {
            // Another window remains between the cursors, so both ends of the
            // back window have a scalar before them.
            self.back = prev_boundary(self.text, self.back);
            self.back_end = prev_boundary(self.text, self.back_end);
        }
        Some(window)
    }
}

impl ExactSizeIterator for CharNGrams<'_> {
    #[inline]
    fn len(&self) -> usize {
        self.remaining
    }
}

impl FusedIterator for CharNGrams<'_> {}

/// The next `char` boundary strictly after `i`, or `text.len()`.
///
/// A UTF-8 scalar is at most four bytes, so this advances at most four times.
#[inline]
fn next_boundary(text: &str, i: usize) -> usize {
    let mut j = i.saturating_add(1).min(text.len());
    while j < text.len() && !text.is_char_boundary(j) {
        j += 1;
    }
    j
}

/// The previous `char` boundary strictly before `i`, or `0`.
#[inline]
fn prev_boundary(text: &str, i: usize) -> usize {
    let mut j = i.saturating_sub(1);
    while j > 0 && !text.is_char_boundary(j) {
        j -= 1;
    }
    j
}
