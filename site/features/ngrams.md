# N-grams

An n-gram is every window of `n` consecutive elements of a sequence, in order.
`verbora-ngrams` is that operation, the padding convention that stops the
elements at the two ends being under-represented, and the same operation over
the Unicode scalars of a `&str`.

It is four public items and no dependencies:

| Item | Windows | Yields |
|---|---|---|
| `ngrams(seq, n)` | the caller's `&[T]`, exactly as given | `&[T]` |
| `Padded::new(seq, n, start, end)` | a copy of it with boundary symbols attached | `&[T]` |
| `char_ngrams(text, n)` | the Unicode scalars of a `&str` | `&str` |
| `CharNGrams<'a>` | the iterator `char_ngrams` returns | — |

<div class="callout callout-spec">
<strong>Specification status.</strong> Every item is documented and test-pinned,
and <strong>no function in this crate panics on any input</strong>, in debug or
in release. <code>cargo test -p verbora-ngrams --all-features</code> runs
<strong>34</strong> tests and <strong>23</strong> doctests.
</div>

## When to use it

- You need bigrams, trigrams or arbitrary `n`-grams over elements you already
  have — tokens, tags, IDs, numbers, anything.
- You need boundary padding, so that the first and last element appear in as
  many windows as the elements in the middle.
- You need character n-grams as borrowed substrings, for language
  identification or fuzzy matching.
- You want to stream windows over a large corpus without allocating per window.

## When not to use it

- **You want a language model.** This crate produces windows. There is no
  smoothing, no probability estimation and no count-of-counts table; counting is
  a three-line fold you write at the call site, shown below.
- **You want string input with a tokenizer built in.** There is deliberately no
  string entry point for word n-grams: tokenize with
  [Tokenizers](./tokenizers.md), then window the slice, so the tokenizer is an
  argument rather than a hidden policy.
- **You want grapheme-cluster windows.** `char_ngrams` works in Unicode scalars.
  Segment first with a grapheme segmenter and build a `Padded` over the result.

## Quick example

```rust
use std::num::NonZeroUsize;
use verbora_ngrams::{Padded, char_ngrams, ngrams};

fn main() {
    let n = NonZeroUsize::new(2).expect("2 is not zero");
    let tokens = ["the", "quick", "brown", "fox"];

    // Windows over the caller's slice: nothing allocated, nothing copied.
    let grams: Vec<&[&str]> = ngrams(&tokens, n).collect();
    assert_eq!(grams, [["the", "quick"], ["quick", "brown"], ["brown", "fox"]]);

    // With boundary symbols, so "the" and "fox" each appear in n windows.
    let padded = Padded::new(&tokens, n, Some(&"<s>"), Some(&"</s>"));
    assert_eq!(padded.ngrams().len(), 5);
    assert_eq!(padded.ngrams().next(), Some(&["<s>", "the"][..]));

    // Character windows, borrowed from the input text.
    let chars: Vec<&str> = char_ngrams("👍你好", n).collect();
    assert_eq!(chars, ["👍你", "你好"]);
}
```

## `n` is a `NonZeroUsize`, and that is the point

`slice::windows` panics when the window size is zero. `ngrams` cannot, because
there is no zero to pass: the precondition is discharged by the type rather than
checked at the point of use, so no call site needs a guard and no input can reach
a panic. That is the entire difference between `ngrams` and `slice::windows`, and
the entire justification for the function existing.

```rust
use std::num::NonZeroUsize;
use verbora_ngrams::ngrams;

fn main() {
    let n = NonZeroUsize::new(3).expect("3 is not zero");

    // The elements are yours; nothing here assumes text.
    let seq = [1_i64, 2, 3, 4];
    let grams: Vec<&[i64]> = ngrams(&seq, n).collect();
    assert_eq!(grams, [[1, 2, 3], [2, 3, 4]]);

    // A window wider than the sequence is empty — the only available answer.
    assert_eq!(ngrams(&["a", "b"], n).count(), 0);
}
```

The iterator is `std::slice::Windows`, so it is `ExactSizeIterator`,
`DoubleEndedIterator` and `FusedIterator`, `len()` is free, and `collect()`
reserves exactly once.

## Choosing the right API

There is one operation in this crate — slide a window of `n` over a sequence —
and the three entry points differ only in *which sequence* is windowed.

| Property | `ngrams` | `Padded` | `char_ngrams` |
|---|---|---|---|
| Sequence windowed | the caller's `&[T]` | a padded copy of it | the scalars of a `&str` |
| Yields | `&[T]` borrowed from the caller's slice | `&[T]` borrowed from the `Padded` | `&str` borrowed from the input text |
| Allocates | **nothing** | one `Vec<T>`, once, in `Padded::new` | **nothing** |
| Clones elements | none | `len + k_start + k_end`, once | none |
| Windows | `len - n + 1`, or `0` | `len + k_start + k_end - n + 1`, or `0` | `scalars - n + 1`, or `0` |
| The first and last element appear in | `1` window each when `n > 1` | exactly `n` windows each, with both symbols supplied | `1` window each when `n > 1` |
| The borrow lives as long as | the caller's slice | the `Padded` value | the input `&str` |

None of the three is the fast one and none is the correct one. `ngrams` is the
right choice for the large majority of programs; the other two exist because
they answer a question it cannot.

```text
Is the input text rather than a sequence of elements?
├── yes, and the windows should be characters → char_ngrams(text, n)
├── yes, and the windows should be words      → tokenize first, then below
└── no
    └── Should the elements at the two ends appear in as many windows
        as the elements in the middle?
        ├── no  → ngrams(seq, n)
        │         (no allocation; the right default)
        └── yes → Padded::new(seq, n, Some(&start), Some(&end)).ngrams()
                  (one allocation, once; the windows are then free)
```

The full lazy-versus-materialised trade-off is on
[Choosing: n-grams](../choosing/ngrams.md).

## Padding

`Padded::new(seq, n, start, end)` is the sequence formed by prepending `k` copies
of `start` (if supplied) and appending `k` copies of `end` (if supplied), where
`k = n - 1`. `Padded::ngrams()` is `ngrams` over that sequence, and
`Padded::as_slice()` is the sequence itself.

The `n - 1` is Jurafsky & Martin, *Speech and Language Processing* (3rd ed.)
§3.1, which augments each sequence with `n - 1` start symbols so that every real
element appears as the final element of exactly one window. The **symmetry** —
`n - 1` end symbols rather than the single `</s>` a language model uses for
probability normalisation — is a Verbora decision, and the reason is that the two
symbols here are independent options: a rule reading "`n - 1` of the start symbol
but exactly one of the end symbol" is asymmetric for a reason that does not apply
when only the end symbol is supplied. Symmetric padding makes the first and last
real element each appear in exactly `n` windows, which is the property feature
extraction wants.

### What padding guarantees

Writing `len` for `seq.len()`, `k_start` for `k` when a start symbol was supplied
and `0` otherwise, and `k_end` likewise:

1. **Every window holds exactly `n` elements** — from `ngrams` and from
   `Padded::ngrams` alike. There is no short window and no ragged edge.
2. **Windows are emitted in left-to-right position order**, each starting one
   element after the previous.
3. **The window count is `len + k_start + k_end - n + 1`** when that is positive,
   and `0` otherwise.
4. **`n == 1` adds no padding**, even when both symbols are supplied — not
   because an argument is discarded, but because `k = n - 1` is zero and zero
   copies is what the formula says.
5. **An empty sequence still pads.** With `n = 4` and both symbols the padded
   sequence is `[S, S, S, E, E, E]` and there are `6 - 4 + 1 = 3` windows.

```rust
use std::num::NonZeroUsize;
use verbora_ngrams::{Padded, ngrams};

fn main() {
    let n = NonZeroUsize::new(3).expect("3 is not zero");
    let seq = ["a", "b", "c"];

    // Unpadded: "a" occurs in one window; there is only one window at all.
    assert_eq!(ngrams(&seq, n).len(), 1);

    // Padded: 3 + 2 + 2 = 7 elements, so 7 - 3 + 1 = 5 windows — and "a"
    // occurs in three of them, which is n of them.
    let padded = Padded::new(&seq, n, Some(&"<s>"), Some(&"</s>"));
    assert_eq!(padded.ngrams().len(), 5);
    assert_eq!(padded.ngrams().filter(|w| w.contains(&"a")).count(), 3);
    assert!(padded.ngrams().all(|w| w.len() == 3));

    // Either symbol may be omitted; they are independent options.
    assert_eq!(
        Padded::new(&["a", "b"], n, Some(&"S"), None).as_slice(),
        ["S", "S", "a", "b"]
    );
    assert_eq!(
        Padded::new(&["a", "b"], n, None, Some(&"E")).as_slice(),
        ["a", "b", "E", "E"]
    );

    // n == 1 gives k == 0, so nothing is padded even with both symbols.
    let one = NonZeroUsize::new(1).expect("1 is not zero");
    assert_eq!(
        Padded::new(&["a", "b"], one, Some(&"S"), Some(&"E")).as_slice(),
        ["a", "b"]
    );

    // An empty sequence still pads, and the windows are drawn from the symbols.
    let four = NonZeroUsize::new(4).expect("4 is not zero");
    let empty: [&str; 0] = [];
    let from_symbols = Padded::new(&empty, four, Some(&"S"), Some(&"E"));
    assert_eq!(from_symbols.as_slice(), ["S", "S", "S", "E", "E", "E"]);
    assert_eq!(from_symbols.ngrams().len(), 3);
}
```

<div class="callout callout-good">
<strong>Every window holds exactly <code>n</code> elements</strong>, padded or
not. There is no case in which a window is short, so
<code>gram[i]</code> for <code>i &lt; n</code> is always in range.
</div>

### Overflow is a refusal, not a panic

`Padded::new` materialises the padded sequence, so it must be able to hold it
*and* to finish writing it, and `k = n - 1` is your number. Two things can make
that impossible: the padded length can overflow `usize`, and a buffer of that
length can fail to reserve. **Neither is a panic.** In both cases `Padded` holds
an empty buffer, `as_slice()` is `&[]`, and `ngrams()` yields nothing with a
`len()` of `0`.

```rust
use std::num::NonZeroUsize;
use verbora_ngrams::Padded;

fn main() {
    // k = usize::MAX - 1 copies of "S" cannot exist; nothing panics.
    let padded = Padded::new(&["a"], NonZeroUsize::MAX, Some(&"S"), None);
    assert!(padded.as_slice().is_empty());
    assert_eq!(padded.ngrams().len(), 0);
}
```

A **zero-sized** element type reaches neither condition on its own — no
reservation of it can fail, however long the sequence — while still costing one
write per element to build. So a zero-sized element is charged **one byte** for
the reservation test, and is refused exactly where `u8` is refused. Without that
charge `new` is total but not terminating: it would accept a padded length of
`usize::MAX - 1` and then count to it.

## Character n-grams

`char_ngrams(text, n)` yields `&str` windows holding exactly `n` consecutive
Unicode scalar values, each a substring of `text`. Because the windows are
substrings, they are usable as map keys directly, and none can contain `U+FFFD`
unless `text` does.

```rust
use std::num::NonZeroUsize;
use verbora_ngrams::char_ngrams;

fn main() {
    let n = NonZeroUsize::new(3).expect("3 is not zero");

    let profile: Vec<&str> = char_ngrams("naïve", n).collect();
    assert_eq!(profile, ["naï", "aïv", "ïve"]);
    assert!(profile.iter().all(|w| "naïve".contains(w)));

    // Astral scalars are scalars like any other: nothing is split into
    // surrogate halves and nothing is replaced.
    let two = NonZeroUsize::new(2).expect("2 is not zero");
    assert_eq!(char_ngrams("👍你好", two).collect::<Vec<_>>(), ["👍你", "你好"]);

    // A window wider than the input yields nothing.
    assert_eq!(char_ngrams("ab", n).count(), 0);

    // ExactSizeIterator and DoubleEndedIterator.
    assert_eq!(char_ngrams("naïve", n).len(), 3);
    assert_eq!(char_ngrams("abcd", two).rev().next(), Some("cd"));
}
```

**The unit is the scalar, not the grapheme cluster.** A window of `n` scalars may
split a combining sequence: `char_ngrams("e\u{0301}f", 2)` yields `"e\u{0301}"`
and `"\u{0301}f"`, the second beginning with a combining acute. That is
deliberate. Grapheme clusters would avoid it, but they change with the Unicode
version and would tie character n-gram keys — which language identification
persists — to that version.

This crate consults no character database at all: `char_ngrams` decodes UTF-8 and
does nothing else. Its output is therefore **stable across Unicode versions**,
which is exactly the property `verbora-tokenizers` and `verbora-normalizers`
cannot offer.

```rust
use std::num::NonZeroUsize;
use verbora_ngrams::char_ngrams;

fn main() {
    let two = NonZeroUsize::new(2).expect("2 is not zero");
    let grams: Vec<&str> = char_ngrams("e\u{0301}f", two).collect();
    assert_eq!(grams, ["e\u{0301}", "\u{0301}f"]);
}
```

## Word n-grams: the composition

There is no string-input entry point here and no tokenizer. A word n-gram is the
composition, written out at the call site:

```rust
use std::num::NonZeroUsize;
use verbora_ngrams::{Padded, ngrams};
use verbora_tokenizers::{BorrowingTokenizer, WordTokenizer};

fn main() {
    let tokens = WordTokenizer.tokenize_borrowed("the quick brown fox");
    let n = NonZeroUsize::new(2).expect("2 is not zero");

    assert_eq!(ngrams(&tokens, n).len(), 3);
    assert_eq!(ngrams(&tokens, n).next(), Some(&["the", "quick"][..]));

    // Padding a tokenized document is the same two lines.
    let padded = Padded::new(&tokens, n, Some(&"<s>"), Some(&"</s>"));
    assert_eq!(padded.ngrams().len(), 5);
}
```

Tokenize once and window many times: the tokens are `&str` slices of the input,
and `ngrams` slices those, so the whole pipeline allocates one `Vec` of pointers
and nothing else.

## Counting n-grams

This crate ships no frequency table, no count-of-counts and no key function.
Counting is a fold over the windows, keyed on the n-gram **itself** rather than
on a rendering of it — a rendering is where two different n-grams collide:

```rust
use std::collections::HashMap;
use std::num::NonZeroUsize;
use verbora_ngrams::ngrams;

fn main() {
    let tokens = ["a", "b", "a", "b", "c"];
    let n = NonZeroUsize::new(2).expect("2 is not zero");

    let mut counts: HashMap<&[&str], u64> = HashMap::new();
    for window in ngrams(&tokens, n) {
        *counts.entry(window).or_default() += 1;
    }

    assert_eq!(counts[&["a", "b"][..]], 2);
    assert_eq!(counts.get(&["c", "a"][..]), None);
}
```

An n-gram that does not occur is `None`, not `0`. A count of zero is a count; it
is never "not found".

The same fold over `Padded::ngrams` counts padded n-grams — and the padding
tuples are then part of the totals, which is why a count-of-counts taken over a
padded corpus is not a count of the corpus. That is your decision to make
explicitly.

## Performance and allocation

- **`ngrams` allocates nothing and clones nothing.** Each yielded window is a
  borrow of the caller's slice at a distinct offset.
- **`char_ngrams` allocates nothing and clones nothing.** It counts the scalars
  of `text` once when it is called, so `len()` is exact from the first call; that
  is one pass over the input. Iteration itself is free.
- **`Padded` pays once, in `new`**: one allocation and `len + k_start + k_end`
  element clones, so it is `O(padded length)` in time as well as space. The
  windows are then ordinary borrows with nothing allocated per window.

**Timings are unmeasured.** No benchmark has been run against the current
implementation, so no figure is published here and none is estimated in place
of one. The allocation behaviour above is structural — a property of the
implementation, not a timing claim. See [Benchmarks](../benchmarks/index.md).

Every entry point is a free function or an inherent method with no interior
state, so windowing parallelises across documents with `rayon` in your own crate;
there is no `par_*` API here because there is no per-item work to fan out.

## Common mistakes

**Reaching for `slice::windows` and adding a zero guard.** That is what `ngrams`
is: the guard is the `NonZeroUsize`, so the guard is not needed.

**Expecting `Padded::ngrams()` to borrow from the original slice.** It borrows
from the `Padded` value, which owns the padded copy. Keep the `Padded` alive as
long as its windows.

**Assuming `Padded::new` cannot fail quietly.** It never panics, but a padded
length it cannot hold leaves the buffer empty and `ngrams()` yielding nothing.
Check `as_slice()` if `n` came from untrusted input.

**Counting padded n-grams and calling the result a corpus count.** The padding
tuples are in the totals.

**Keying a frequency table on a rendered string.** `"a, b"` as one token and
`"a"`, `"b"` as two render identically. Key on the window itself, as above.

**Expecting `char_ngrams` to respect grapheme clusters.** It works in scalars, on
purpose, so that its keys do not move with the Unicode version.

## Related

- [Choosing: n-grams](../choosing/ngrams.md) — the lazy-vs-materialised decision
  in full
- [Tokenizers](./tokenizers.md) — what produces the slice you window
- [Zero-copy](../performance/zero-copy.md) ·
  [Allocation](../performance/allocation.md)
- [Batch vs streaming](../performance/batch-vs-streaming.md) ·
  [Parallelism](../performance/parallelism.md)
- [Benchmarks](../benchmarks/index.md) · [Recipes](../recipes/index.md)

## API reference

```bash
cargo doc -p verbora-ngrams --no-deps --open
```

```rust ignore
// verbora_ngrams — the crate root is the whole public surface
pub fn ngrams<T>(seq: &[T], n: NonZeroUsize) -> std::slice::Windows<'_, T>;

pub struct Padded<T> { /* private */ }
impl<T: Clone> Padded<T> {
    pub fn new(seq: &[T], n: NonZeroUsize, start: Option<&T>, end: Option<&T>) -> Self;
    pub fn ngrams(&self) -> std::slice::Windows<'_, T>;
    pub fn as_slice(&self) -> &[T];
}

pub fn char_ngrams(text: &str, n: NonZeroUsize) -> CharNGrams<'_>;
pub struct CharNGrams<'a> { /* private */ }
// Iterator<Item = &'a str> + ExactSizeIterator + DoubleEndedIterator + FusedIterator
```

`Padded` derives `Debug`, `Clone`, `PartialEq` and `Eq`; `CharNGrams` derives
`Debug` and `Clone`. There is no `Result` anywhere in this crate, and no
dependency either.

Source: `crates/verbora-ngrams/src/`.
