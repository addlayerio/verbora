//! Windows over a slice, and the padded sequence they are taken from.
//!
//! Private. Everything here is re-exported from the crate root, which carries
//! the user-facing prose.

use std::num::NonZeroUsize;
use std::slice::Windows;

/// The `n`-grams of `seq`: every window of `n` consecutive elements, in order.
///
/// This is [`slice::windows`], with one difference that is the entire reason
/// the function exists: `slice::windows` **panics** when the window size is
/// zero, and `ngrams` cannot be called with zero because [`NonZeroUsize`] has
/// no zero to pass. The invalid state is unrepresentable rather than checked.
///
/// Nothing is allocated and nothing is cloned: each yielded window is a borrow
/// of `seq` at a distinct offset, and the borrow lives as long as `seq` does.
///
/// `n > seq.len()` yields an empty iterator — the same answer
/// [`slice::windows`] gives, and the only one available: there is no window of
/// `n` elements in a sequence that holds fewer than `n`.
///
/// # Examples
///
/// ```
/// use std::num::NonZeroUsize;
/// use verbora_ngrams::ngrams;
///
/// let n = NonZeroUsize::new(2).expect("2 is not zero");
/// let seq = ["a", "b", "c"];
///
/// let grams: Vec<&[&str]> = ngrams(&seq, n).collect();
/// assert_eq!(grams, [["a", "b"], ["b", "c"]]);
/// ```
///
/// A window wider than the sequence:
///
/// ```
/// use std::num::NonZeroUsize;
/// use verbora_ngrams::ngrams;
///
/// let n = NonZeroUsize::new(5).expect("5 is not zero");
/// assert_eq!(ngrams(&["a", "b", "c"], n).count(), 0);
/// ```
///
/// The elements are the caller's; nothing here assumes text:
///
/// ```
/// use std::num::NonZeroUsize;
/// use verbora_ngrams::ngrams;
///
/// let n = NonZeroUsize::new(3).expect("3 is not zero");
/// let seq = [1_i64, 2, 3, 4];
/// let grams: Vec<&[i64]> = ngrams(&seq, n).collect();
/// assert_eq!(grams, [[1, 2, 3], [2, 3, 4]]);
/// ```
#[inline]
pub fn ngrams<T>(seq: &[T], n: NonZeroUsize) -> Windows<'_, T> {
    // `NonZeroUsize` discharges the only precondition `slice::windows` has.
    seq.windows(n.get())
}

/// An empty `Vec<T>` with room for exactly `len` elements, or `None` when
/// [`Padded::new`] must refuse to build one.
///
/// For an element type that occupies memory, [`Vec::try_reserve_exact`] decides
/// this exactly: a Rust allocation is at most `isize::MAX` bytes, so the byte
/// count binds long before `usize` overflows, and a refusal here is what makes
/// `new` total.
///
/// A **zero-sized** element type has no byte count, so no reservation of it can
/// fail — `Vec::<()>::try_reserve_exact(usize::MAX)` succeeds. That is not
/// harmless, because the caller of this function then fills the buffer one
/// element at a time: a length near `usize::MAX` would be accepted and would
/// never finish, so totality would hold and termination would not.
///
/// The rule that closes it is to charge a zero-sized element **one byte** and
/// put the same question to a `Vec<u8>`. It is the smallest charge that bounds
/// anything, it refuses exactly where the smallest element type that does
/// occupy memory is refused, and it introduces no constant of this crate's own
/// invention — a fixed ceiling would either still admit lengths no machine can
/// step through or reject lengths a machine handles comfortably. The probe
/// allocation is released before this function returns.
fn reserved_buffer<T>(len: usize) -> Option<Vec<T>> {
    if size_of::<T>() == 0 {
        let mut probe: Vec<u8> = Vec::new();
        probe.try_reserve_exact(len).ok()?;
    }

    let mut buf = Vec::new();
    buf.try_reserve_exact(len).ok()?;
    Some(buf)
}

/// `seq` with `n - 1` copies of each supplied symbol prepended and appended.
///
/// `Padded::new(seq, n, start, end)` is the sequence formed by prepending `k`
/// copies of `start` (if supplied) and appending `k` copies of `end` (if
/// supplied), where `k = n - 1`. [`Padded::ngrams`] is [`ngrams`] over that
/// sequence, and [`Padded::as_slice`] is the sequence itself.
///
/// The `n - 1` is Jurafsky & Martin, *Speech and Language Processing*
/// (3rd ed.) §3.1: augmenting a sequence with `n - 1` start symbols makes
/// every real element the final element of exactly one window. The symmetry —
/// `n - 1` end symbols rather than a single one — is Verbora's own decision;
/// the crate-root documentation gives the reason.
///
/// The padded sequence is materialised once, in [`Padded::new`], so the
/// windows are ordinary borrows of a slice with nothing allocated per window.
/// The cost is `len` clones of the sequence's elements plus `k_start + k_end`
/// clones of the pad symbols, paid once.
///
/// # Examples
///
/// ```
/// use std::num::NonZeroUsize;
/// use verbora_ngrams::Padded;
///
/// let n = NonZeroUsize::new(2).expect("2 is not zero");
/// let padded = Padded::new(&["a", "b"], n, Some(&"<s>"), Some(&"</s>"));
///
/// assert_eq!(padded.as_slice(), ["<s>", "a", "b", "</s>"]);
/// let grams: Vec<&[&str]> = padded.ngrams().collect();
/// assert_eq!(grams, [["<s>", "a"], ["a", "b"], ["b", "</s>"]]);
/// ```
///
/// Either symbol may be omitted; they are independent options, not one policy:
///
/// ```
/// use std::num::NonZeroUsize;
/// use verbora_ngrams::Padded;
///
/// let n = NonZeroUsize::new(3).expect("3 is not zero");
/// let start_only = Padded::new(&["a", "b"], n, Some(&"S"), None);
/// assert_eq!(start_only.as_slice(), ["S", "S", "a", "b"]);
///
/// let end_only = Padded::new(&["a", "b"], n, None, Some(&"E"));
/// assert_eq!(end_only.as_slice(), ["a", "b", "E", "E"]);
/// ```
///
/// `n == 1` gives `k == 0`, so no padding is added even when both symbols are
/// supplied — zero copies is what the formula says, not an argument being
/// discarded:
///
/// ```
/// use std::num::NonZeroUsize;
/// use verbora_ngrams::Padded;
///
/// let n = NonZeroUsize::new(1).expect("1 is not zero");
/// let padded = Padded::new(&["a", "b"], n, Some(&"S"), Some(&"E"));
/// assert_eq!(padded.as_slice(), ["a", "b"]);
/// ```
///
/// An empty sequence still pads, and the windows are drawn from the symbols
/// alone. With `n = 4` and both symbols the padded sequence is
/// `[S, S, S, E, E, E]`, which holds `6 - 4 + 1 = 3` windows:
///
/// ```
/// use std::num::NonZeroUsize;
/// use verbora_ngrams::Padded;
///
/// let n = NonZeroUsize::new(4).expect("4 is not zero");
/// let empty: [&str; 0] = [];
/// let padded = Padded::new(&empty, n, Some(&"S"), Some(&"E"));
///
/// assert_eq!(padded.as_slice(), ["S", "S", "S", "E", "E", "E"]);
/// let grams: Vec<&[&str]> = padded.ngrams().collect();
/// assert_eq!(
///     grams,
///     [
///         ["S", "S", "S", "E"],
///         ["S", "S", "E", "E"],
///         ["S", "E", "E", "E"],
///     ]
/// );
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Padded<T> {
    /// The padded sequence, materialised. Empty when `new` refused to build it
    /// (see [`Padded::new`]'s *Refusal* section).
    buf: Vec<T>,
    /// The window width the buffer was built for.
    n: NonZeroUsize,
}

impl<T: Clone> Padded<T> {
    /// Builds the padded sequence: `k` copies of `start`, then `seq`, then `k`
    /// copies of `end`, where `k = n - 1`. An omitted symbol contributes no
    /// copies on its side.
    ///
    /// # Refusal, in place of a panic
    ///
    /// `new` materialises the padded sequence, so it must be able to hold it —
    /// and, because it fills the buffer one element at a time, it must also be
    /// able to *finish*. Two things can make that impossible:
    ///
    /// - the padded length `seq.len() + k_start + k_end` overflows `usize`;
    /// - a buffer of that length cannot be reserved — which for every element
    ///   type that occupies memory is the ceiling reached long before `usize`
    ///   overflows, since a Rust allocation is at most `isize::MAX` bytes.
    ///
    /// A **zero-sized `T`** is the one element type whose reservation cannot
    /// fail, so the second condition would never refuse it however large the
    /// padded length grew — and the buffer would still be filled one element
    /// at a time, which for a length near `usize::MAX` does not finish. Such a
    /// `T` is therefore charged **one byte per element** for the purpose of the
    /// reservation test: `Padded<T>` refuses a zero-sized `T` at exactly the
    /// lengths `Padded<u8>` refuses, rather than at a constant this crate
    /// invented. The probe buffer is released immediately and is the only
    /// allocation a zero-sized `T` ever costs.
    ///
    /// In every refusing case the buffer is left **empty**: [`as_slice`] is
    /// `&[]`, [`ngrams`] yields nothing and its `len()` is `0`. Nothing panics
    /// and nothing runs away, on any input, in debug or in release.
    ///
    /// Otherwise `new` costs one allocation and `padded length` clones — so it
    /// is `O(padded length)` in time as well as space.
    ///
    /// [`as_slice`]: Padded::as_slice
    /// [`ngrams`]: Padded::ngrams
    ///
    /// # Examples
    ///
    /// ```
    /// use std::num::NonZeroUsize;
    /// use verbora_ngrams::Padded;
    ///
    /// let n = NonZeroUsize::new(5).expect("5 is not zero");
    /// let padded = Padded::new(&["a", "b", "c"], n, Some(&"<s>"), Some(&"</s>"));
    ///
    /// // 3 + 4 + 4 = 11 elements, so 11 - 5 + 1 = 7 windows of exactly five.
    /// assert_eq!(padded.as_slice().len(), 11);
    /// let grams: Vec<&[&str]> = padded.ngrams().collect();
    /// assert_eq!(grams.len(), 7);
    /// assert!(grams.iter().all(|w| w.len() == 5));
    /// assert_eq!(grams[0], ["<s>", "<s>", "<s>", "<s>", "a"]);
    /// assert_eq!(grams[6], ["c", "</s>", "</s>", "</s>", "</s>"]);
    /// ```
    ///
    /// A padded length that cannot exist yields an empty `Padded` rather than
    /// a panic:
    ///
    /// ```
    /// use std::num::NonZeroUsize;
    /// use verbora_ngrams::Padded;
    ///
    /// let padded = Padded::new(&["a"], NonZeroUsize::MAX, Some(&"S"), None);
    /// assert!(padded.as_slice().is_empty());
    /// assert_eq!(padded.ngrams().len(), 0);
    /// ```
    #[must_use]
    pub fn new(seq: &[T], n: NonZeroUsize, start: Option<&T>, end: Option<&T>) -> Self {
        let k = n.get() - 1;
        let k_start = if start.is_some() { k } else { 0 };
        let k_end = if end.is_some() { k } else { 0 };

        let empty = Self { buf: Vec::new(), n };

        // Checked, because `k` is the caller's `n` and both sides are added.
        let Some(padded_len) = seq
            .len()
            .checked_add(k_start)
            .and_then(|len| len.checked_add(k_end))
        else {
            return empty;
        };

        // A length that fits in `usize` still need not fit in memory, and a
        // length that fits in memory for free still need not be one this can
        // finish writing; refusing here is what keeps `new` total *and*
        // terminating for a huge `n` on a small sequence, where the addition
        // above does not overflow.
        let Some(mut buf) = reserved_buffer::<T>(padded_len) else {
            return empty;
        };

        if let Some(symbol) = start {
            buf.extend(std::iter::repeat_n(symbol.clone(), k_start));
        }
        buf.extend_from_slice(seq);
        if let Some(symbol) = end {
            buf.extend(std::iter::repeat_n(symbol.clone(), k_end));
        }

        Self { buf, n }
    }

    /// The `n`-grams of the padded sequence: every window of `n` consecutive
    /// elements, in left-to-right order.
    ///
    /// Every window holds exactly `n` elements. The count is
    /// `len + k_start + k_end - n + 1` when that is positive and `0`
    /// otherwise — including when [`Padded::new`] refused to build the buffer,
    /// where it is `0`.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::num::NonZeroUsize;
    /// use verbora_ngrams::Padded;
    ///
    /// let n = NonZeroUsize::new(3).expect("3 is not zero");
    /// let padded = Padded::new(&["a", "b"], n, Some(&"S"), None);
    ///
    /// // 2 + 2 = 4 elements, so 4 - 3 + 1 = 2 windows.
    /// let grams: Vec<&[&str]> = padded.ngrams().collect();
    /// assert_eq!(grams, [["S", "S", "a"], ["S", "a", "b"]]);
    /// ```
    #[inline]
    pub fn ngrams(&self) -> Windows<'_, T> {
        ngrams(&self.buf, self.n)
    }

    /// The padded sequence itself.
    ///
    /// This is the sequence [`Padded::ngrams`] windows over, so it is `k`
    /// copies of the start symbol, then the caller's elements, then `k` copies
    /// of the end symbol, with an omitted symbol contributing nothing. It is
    /// empty when [`Padded::new`] refused to build the buffer.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::num::NonZeroUsize;
    /// use verbora_ngrams::Padded;
    ///
    /// let n = NonZeroUsize::new(3).expect("3 is not zero");
    /// let padded = Padded::new(&[1_u8, 2], n, None, Some(&0));
    /// assert_eq!(padded.as_slice(), [1, 2, 0, 0]);
    /// ```
    #[must_use]
    #[inline]
    pub fn as_slice(&self) -> &[T] {
        &self.buf
    }
}
