//! N-gram windows for Rust.
//!
//! An n-gram is every window of `n` consecutive elements of a sequence, in
//! order. This crate is that operation, the padding convention that makes the
//! elements at the two ends of a sequence appear in as many windows as the
//! elements in the middle, and the same operation over the Unicode scalars of
//! a `&str`. It is four items and no dependencies.
//!
//! ```
//! use std::num::NonZeroUsize;
//! use verbora_ngrams::{Padded, char_ngrams, ngrams};
//!
//! let n = NonZeroUsize::new(2).expect("2 is not zero");
//! let tokens = ["the", "quick", "brown", "fox"];
//!
//! // Windows over the caller's slice: nothing is allocated, nothing is copied.
//! let grams: Vec<&[&str]> = ngrams(&tokens, n).collect();
//! assert_eq!(grams, [["the", "quick"], ["quick", "brown"], ["brown", "fox"]]);
//!
//! // With boundary symbols, so "the" and "fox" each appear in n windows.
//! let padded = Padded::new(&tokens, n, Some(&"<s>"), Some(&"</s>"));
//! assert_eq!(padded.ngrams().len(), 5);
//! assert_eq!(padded.ngrams().next(), Some(&["<s>", "the"][..]));
//!
//! // Character windows, borrowed from the input text.
//! let chars: Vec<&str> = char_ngrams("👍你好", n).collect();
//! assert_eq!(chars, ["👍你", "你好"]);
//! ```
//!
//! # `n` is a `NonZeroUsize`, and that is the point
//!
//! [`slice::windows`] panics when the window size is zero. [`ngrams`] cannot,
//! because there is no zero to pass: the precondition is discharged by the
//! type rather than checked at the point of use, so no call site needs a guard
//! and no input can reach a panic. That is the entire difference between
//! [`ngrams`] and [`slice::windows`], and the entire justification for the
//! function existing.
//!
//! Everything else here is total as well. No function in this crate panics on
//! any input, in debug or in release; the one place where that takes work is
//! [`Padded::new`], and it is described under *Overflow* below.
//!
//! # The unit is the caller's element
//!
//! [`ngrams`] and [`Padded`] are generic over `T`, and this crate deliberately
//! does not choose what `T` is. An n-gram is a windowing operation over a
//! sequence; what the sequence holds is the caller's decision, and a crate
//! that fixed the unit here would only be fixing a tokenizer.
//!
//! [`char_ngrams`] is the one place a unit is chosen, and it says so in its
//! name: it works in **Unicode scalar values**, and every window it yields is
//! a borrowed substring of the input holding exactly `n` of them. The unit is
//! *not* the grapheme cluster. Grapheme clusters would be defensible, but they
//! change with the Unicode version in a way that scalars do not, and they
//! would tie character n-gram keys — which language identification persists —
//! to that version. A caller who wants grapheme windows segments first and
//! uses [`Padded`] or [`ngrams`] over the resulting slice.
//!
//! This crate consults no character database at all: [`char_ngrams`] decodes
//! UTF-8 and does nothing else. Its output is therefore **stable across
//! Unicode versions**, which matters for anything that persists n-gram keys.
//!
//! # Padding
//!
//! [`Padded::new(seq, n, start, end)`](Padded::new) is the sequence formed by
//! prepending `k` copies of `start` (if supplied) and appending `k` copies of
//! `end` (if supplied), where `k = n - 1`. [`Padded::ngrams`] is [`ngrams`]
//! over that sequence.
//!
//! The `n - 1` is Jurafsky & Martin, *Speech and Language Processing*
//! (3rd ed.) §3.1, which augments each sequence with `n - 1` start symbols so
//! that every real element appears as the final element of exactly one window.
//!
//! The **symmetry** — `n - 1` end symbols rather than the single `</s>` a
//! language model uses for probability normalisation — is a Verbora decision,
//! and the reason is that this crate's two symbols are independent options: a
//! rule reading "`n - 1` of the start symbol but exactly one of the end
//! symbol" is asymmetric for a reason that does not apply when only the end
//! symbol is supplied. Symmetric padding makes the first and last real element
//! each appear in exactly `n` windows, which is the property feature
//! extraction wants.
//!
//! ## What padding guarantees
//!
//! Writing `len` for `seq.len()`, `k_start` for `k` when a start symbol was
//! supplied and `0` otherwise, and `k_end` likewise:
//!
//! 1. **Every window holds exactly `n` elements** — from [`ngrams`] and from
//!    [`Padded::ngrams`] alike. There is no short window and no ragged edge.
//! 2. **Windows are emitted in left-to-right position order**, each starting
//!    one element after the previous.
//! 3. **The window count is `len + k_start + k_end - n + 1`** when that is
//!    positive, and `0` otherwise. For [`ngrams`], where `k_start` and `k_end`
//!    are both zero, that is `len - n + 1` or `0`.
//! 4. **`n == 1` adds no padding**, even when both symbols are supplied — not
//!    because an argument is discarded, but because `k = n - 1` is zero and
//!    zero copies is what the formula says. A caller who wants boundary
//!    symbols in a unigram model prepends them to the sequence.
//! 5. **An empty sequence still pads.** With `n = 4` and both symbols the
//!    padded sequence is `[S, S, S, E, E, E]` and there are `6 - 4 + 1 = 3`
//!    windows, all drawn from the symbols.
//!
//! ## Overflow
//!
//! [`Padded::new`] materialises the padded sequence, so it must be able to
//! hold it and to finish writing it, and `k = n - 1` is the caller's number.
//! Two things can make that impossible: the padded length can overflow
//! `usize`, and a buffer of that length can fail to reserve. **Neither is a
//! panic.** In both cases `Padded` holds an empty buffer,
//! [`Padded::as_slice`] is `&[]`, and [`Padded::ngrams`] yields nothing with a
//! `len()` of `0`.
//!
//! ```
//! use std::num::NonZeroUsize;
//! use verbora_ngrams::Padded;
//!
//! // k = usize::MAX - 1 copies of "S" cannot exist; nothing panics.
//! let padded = Padded::new(&["a"], NonZeroUsize::MAX, Some(&"S"), None);
//! assert!(padded.as_slice().is_empty());
//! assert_eq!(padded.ngrams().len(), 0);
//! ```
//!
//! A **zero-sized** element type reaches neither condition on its own — no
//! reservation of it can fail, however long the sequence — while still costing
//! one write per element to build. So a zero-sized element is charged **one
//! byte** for the reservation test, and is refused exactly where `u8` is
//! refused. Without that charge `new` is total but not terminating: it accepts
//! a padded length of `usize::MAX - 1` and then counts to it.
//!
//! ```
//! use std::num::NonZeroUsize;
//! use verbora_ngrams::Padded;
//!
//! let empty: [(); 0] = [];
//! let padded = Padded::new(&empty, NonZeroUsize::MAX, Some(&()), None);
//! assert!(padded.as_slice().is_empty());
//!
//! // Ordinary lengths are unaffected: () pads like any other element.
//! let n = NonZeroUsize::new(3).expect("3 is not zero");
//! assert_eq!(Padded::new(&[(), ()], n, Some(&()), None).as_slice().len(), 4);
//! ```
//!
//! # Counting n-grams
//!
//! This crate ships no frequency table, no `Nr` count-of-counts and no key
//! function. Counting is a fold over the windows, keyed on the n-gram itself
//! rather than on a rendering of it — a rendering is where two different
//! n-grams collide:
//!
//! ```
//! use std::collections::HashMap;
//! use std::num::NonZeroUsize;
//! use verbora_ngrams::ngrams;
//!
//! let tokens = ["a", "b", "a", "b", "c"];
//! let n = NonZeroUsize::new(2).expect("2 is not zero");
//!
//! let mut counts: HashMap<&[&str], u64> = HashMap::new();
//! for window in ngrams(&tokens, n) {
//!     *counts.entry(window).or_default() += 1;
//! }
//!
//! assert_eq!(counts[&["a", "b"][..]], 2);
//! assert_eq!(counts.get(&["c", "a"][..]), None);
//! ```
//!
//! An n-gram that does not occur is [`None`], not `0`. A count of zero is a
//! count; it is never "not found".
//!
//! The same fold over [`Padded::ngrams`] counts padded n-grams — and the
//! padding tuples are then part of the totals, which is why a count-of-counts
//! taken over a padded corpus is not a count of the corpus. That is the
//! caller's decision to make explicitly.
//!
//! # Text input
//!
//! There is no string-input entry point here and no tokenizer. A word n-gram
//! is the composition, written out at the call site so that the tokenizer is
//! an argument rather than a hidden policy:
//!
//! ```
//! use std::num::NonZeroUsize;
//! use verbora_ngrams::ngrams;
//!
//! // In a real program this is a tokenizer — `verbora_tokenizers`'
//! // `WordTokenizer::tokenize_borrowed`, for instance. It stays visible.
//! let tokens: Vec<&str> = "the quick brown fox".split_whitespace().collect();
//!
//! let n = NonZeroUsize::new(2).expect("2 is not zero");
//! assert_eq!(ngrams(&tokens, n).len(), 3);
//! ```
//!
//! # Choosing the Right API
//!
//! ## The conceptual difference
//!
//! There is one operation in this crate — slide a window of `n` over a
//! sequence — and the three entry points differ only in *which sequence* is
//! windowed:
//!
//! - [`ngrams`] windows the caller's slice exactly as it was given;
//! - [`Padded`] windows a copy of it with boundary symbols attached, so the
//!   elements at the ends are not under-represented;
//! - [`char_ngrams`] windows the Unicode scalars of a `&str`, yielding
//!   substrings rather than sub-slices.
//!
//! None of the three is the fast one and none is the correct one. [`ngrams`]
//! is the right choice for the large majority of programs, and the other two
//! exist because they answer a question it cannot.
//!
//! ## Comparison
//!
//! | | [`ngrams`] | [`Padded`] | [`char_ngrams`] |
//! |---|---|---|---|
//! | Sequence windowed | the caller's `&[T]` | a padded copy of the caller's `&[T]` | the scalars of a `&str` |
//! | Yields | `&[T]` borrowed from the caller's slice | `&[T]` borrowed from the [`Padded`] | `&str` borrowed from the input text |
//! | Allocates | **nothing** | one `Vec<T>`, once, in [`Padded::new`] | **nothing** |
//! | Clones elements | none | `len + k_start + k_end`, once, in [`Padded::new`] | none |
//! | Windows | `len - n + 1`, or `0` | `len + k_start + k_end - n + 1`, or `0` | `scalars - n + 1`, or `0` |
//! | The first and last element appear in | `1` window each when `n > 1` | exactly `n` windows each, with both symbols supplied | `1` window each when `n > 1` |
//! | The borrow lives as long as | the caller's slice | the [`Padded`] value | the input `&str` |
//! | `n` exceeds the length | empty | not necessarily empty — padding lengthens the sequence | empty |
//! | Iterator | [`slice::Windows`](std::slice::Windows) | [`slice::Windows`](std::slice::Windows) | [`CharNGrams`] — also [`DoubleEndedIterator`] |
//!
//! ## Examples
//!
//! The difference padding makes, on the same input:
//!
//! ```
//! use std::num::NonZeroUsize;
//! use verbora_ngrams::{Padded, ngrams};
//!
//! let n = NonZeroUsize::new(3).expect("3 is not zero");
//! let seq = ["a", "b", "c"];
//!
//! // Unpadded: "a" occurs in one window; there is only one window at all.
//! assert_eq!(ngrams(&seq, n).len(), 1);
//!
//! // Padded: 3 + 2 + 2 = 7 elements, so 7 - 3 + 1 = 5 windows — and "a"
//! // occurs in three of them, which is n of them.
//! let padded = Padded::new(&seq, n, Some(&"<s>"), Some(&"</s>"));
//! assert_eq!(padded.ngrams().len(), 5);
//! assert_eq!(padded.ngrams().filter(|w| w.contains(&"a")).count(), 3);
//! ```
//!
//! Character windows are substrings, so they can be used as keys directly:
//!
//! ```
//! use std::num::NonZeroUsize;
//! use verbora_ngrams::char_ngrams;
//!
//! let n = NonZeroUsize::new(3).expect("3 is not zero");
//! let profile: Vec<&str> = char_ngrams("naïve", n).collect();
//! assert_eq!(profile, ["naï", "aïv", "ïve"]);
//! assert!(profile.iter().all(|w| "naïve".contains(w)));
//! ```
//!
//! ## Decision tree
//!
//! ```text
//! Is the input text rather than a sequence of elements?
//! ├── yes, and the windows should be characters → char_ngrams(text, n)
//! ├── yes, and the windows should be words      → tokenize first, then below
//! └── no
//!     └── Should the elements at the two ends appear in as many windows
//!         as the elements in the middle?
//!         ├── no  → ngrams(seq, n)
//!         │         (no allocation; the right default)
//!         └── yes → Padded::new(seq, n, Some(&start), Some(&end)).ngrams()
//!                   (one allocation, once; the windows are then free)
//! ```
//!
//! ## Performance
//!
//! `UNMEASURED`. No benchmark has been run against this shape of the crate,
//! and no figure is published here until one has. What can be said without
//! measuring is structural and is in the table above: [`ngrams`] and
//! [`char_ngrams`] allocate nothing at all, and [`Padded`] pays for the padded
//! sequence once in [`Padded::new`] rather than per window.

mod chars;
mod engine;

pub use chars::{CharNGrams, char_ngrams};
pub use engine::{Padded, ngrams};
