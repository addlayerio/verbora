# Choosing an n-gram API

`verbora-ngrams` gives you three entry points for one operation — slide a window
of `n` over a sequence — and they differ only in **which sequence is windowed**.
Everything else follows from where your input already is.

For the full surface, see [Features: n-grams](../features/ngrams.md).

<div class="callout callout-note">
<strong>Note.</strong> Blocks marked <code>rust,ignore</code> on this page do not
compile on purpose, and the prose says why. Every other Rust block is a complete
program whose assertions pass.
</div>

## The decision that matters: which sequence

| API | Sequence windowed | Yields | Allocates | Windows live as long as |
|---|---|---|---|---|
| `ngrams(seq, n)` | the caller's `&[T]`, exactly as given | `&[T]` | **nothing** | the caller's slice |
| `Padded::new(seq, n, s, e)` | a copy of it with boundary symbols attached | `&[T]` | one `Vec<T>`, once | the `Padded` value |
| `char_ngrams(text, n)` | the Unicode scalars of a `&str` | `&str` | **nothing** | the input `&str` |

| If you… | Call |
|---|---|
| have a slice of elements and want its windows — **the default** | `ngrams(seq, n)` |
| want the first and last element to appear in as many windows as the middle ones | `Padded::new(seq, n, Some(&start), Some(&end)).ngrams()` |
| have text and want character windows (language ID, fuzzy matching) | `char_ngrams(text, n)` |
| have text and want word windows | tokenize first, then `ngrams` over the token slice |
| need the windows to outlive the sequence | copy them: `.map(<[_]>::to_vec).collect()` |

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

## `ngrams()` <a class="badge badge-lazy" href="../performance/iterator-vs-into">LAZY</a> <a class="badge badge-zerocopy" href="../performance/zero-copy">ZERO-COPY</a>

Nothing happens until you advance it, and nothing is ever copied: each window is
a borrow of your slice at a distinct offset. The iterator is
`std::slice::Windows`, so it is `ExactSizeIterator` (`len()` without consuming),
`DoubleEndedIterator` (`rev()`, `next_back()`) and `FusedIterator`, and it
derives `Clone`, so a scan is cheap to restart.

`n` is a `NonZeroUsize`. That is the whole reason the function exists rather than
`slice::windows`: `slice::windows` panics on a zero size, and `ngrams` has no
zero to pass, so the precondition is discharged by the type rather than checked
at every call site.

```rust
use std::num::NonZeroUsize;
use verbora_ngrams::ngrams;

fn main() {
    let n = NonZeroUsize::new(2).expect("2 is not zero");
    let tokens: Vec<&str> = "the quick brown fox jumps over the lazy dog"
        .split(' ')
        .collect();

    // len() without consuming anything.
    assert_eq!(ngrams(&tokens, n).len(), 8);

    // Only five windows are ever produced, regardless of how long `tokens` is.
    let first_five: Vec<&[&str]> = ngrams(&tokens, n).take(5).collect();
    assert_eq!(first_five.len(), 5);

    // Stops at the first match; the tail of the sequence is never windowed.
    let hit = ngrams(&tokens, n).find(|gram| gram[0] == "brown");
    assert_eq!(hit, Some(&["brown", "fox"][..]));

    // A membership test needs no Vec at all.
    assert!(!ngrams(&tokens, n).any(|gram| gram[0] == gram[1]));

    // Backwards, too.
    assert_eq!(ngrams(&tokens, n).next_back(), Some(&["lazy", "dog"][..]));
}
```

<div class="callout callout-good">
<strong>Every window holds exactly <code>n</code> elements</strong>, padded or
not, so <code>gram[i]</code> for <code>i &lt; n</code> can never be out of range.
</div>

## Collecting, and when you must

`ngrams(seq, n).collect::<Vec<_>>()` gives you indexable windows at the cost of
one `Vec` of fat pointers — the windows themselves are still views into your
slice, so nothing is copied. That is the middle road and usually the right one
when you consume everything anyway.

The windows cannot outlive the slice, and that is a real signal rather than a
nuisance. This does not compile:

```rust  ignore
use std::num::NonZeroUsize;
use verbora_ngrams::ngrams;
use verbora_tokenizers::{BorrowingTokenizer, WordTokenizer};

// error[E0515]: cannot return value referencing local variable `tokens`
fn bigrams_of(text: &str) -> Vec<&[&str]> {
    let tokens = WordTokenizer.tokenize_borrowed(text);
    let n = NonZeroUsize::new(2).expect("2 is not zero");
    ngrams(&tokens, n).collect() // `tokens` is dropped when this returns
}
```

The fix is either to hoist `tokens` into the caller — keeping the zero-copy path
— or to copy the windows out:

```rust
use std::num::NonZeroUsize;
use verbora_ngrams::ngrams;
use verbora_tokenizers::{BorrowingTokenizer, WordTokenizer};

fn bigrams_owned_of(text: &str) -> Vec<Vec<String>> {
    let tokens = WordTokenizer.tokenize_borrowed(text);
    let n = NonZeroUsize::new(2).expect("2 is not zero");
    ngrams(&tokens, n)
        .map(|w| w.iter().map(|t| (*t).to_owned()).collect())
        .collect()
}

fn main() {
    assert_eq!(bigrams_owned_of("a b c").len(), 2);
    assert_eq!(bigrams_owned_of("a b c")[0], ["a", "b"]);
}
```

Copying is the expensive option: with `n = 2` over `k` tokens it allocates
roughly `2k` strings plus `k` vectors. Reach for it when the borrow checker tells
you to, not by default.

## Where laziness actually wins

Laziness does not make producing a window cheaper — a window is a borrow either
way. What it saves is the outer `Vec` and the windows past your stopping point.

On a nine-word sentence the saving is irrelevant. On a 20,000-token document it
is 20,000 fat pointers written to the heap and thrown away. If you consume every
n-gram anyway, collecting costs one extra allocation and gives you `len()`,
indexing and slicing in return — take it.

The same argument applies when you consume everything but never need the list —
a frequency count, a maximum, a filter into some other structure. Key the count
on the **window itself**, not on a rendering of it: a rendering is where two
different n-grams collide.

```rust
use std::collections::HashMap;
use std::num::NonZeroUsize;
use verbora_ngrams::ngrams;

fn main() {
    let tokens = ["a", "b", "a", "b", "a"];
    let n = NonZeroUsize::new(2).expect("2 is not zero");

    let mut counts: HashMap<&[&str], u32> = HashMap::new();
    for gram in ngrams(&tokens, n) {
        *counts.entry(gram).or_default() += 1;
    }

    assert_eq!(counts[&["a", "b"][..]], 2);
    // An n-gram that never occurred is `None`, not `0`.
    assert_eq!(counts.get(&["b", "b"][..]), None);
}
```

## Padded or not

`Padded::new(seq, n, start, end)` prepends `k` copies of `start` and appends `k`
copies of `end`, where `k = n - 1`, then windows that. The two symbols are
independent options: supply one, both or neither.

| | `ngrams(seq, n)` | `Padded::new(seq, n, s, e).ngrams()` |
|---|---|---|
| Allocates | nothing | one `Vec<T>`, once, in `new` |
| Clones elements | none | `len + k_start + k_end`, once |
| Windows | `len - n + 1`, or `0` | `len + k_start + k_end - n + 1`, or `0` |
| First/last real element appears in | 1 window | exactly `n` windows, with both symbols |
| `n` exceeds the length | empty | not necessarily empty — padding lengthens the sequence |

```rust
use std::num::NonZeroUsize;
use verbora_ngrams::{Padded, ngrams};

fn main() {
    let n = NonZeroUsize::new(3).expect("3 is not zero");
    let seq = ["a", "b", "c"];

    // Unpadded: one window, and "a" is in it once.
    assert_eq!(ngrams(&seq, n).len(), 1);

    // Padded: 3 + 2 + 2 = 7 elements, so 5 windows, and "a" is in three.
    let padded = Padded::new(&seq, n, Some(&"<s>"), Some(&"</s>"));
    assert_eq!(padded.ngrams().len(), 5);
    assert_eq!(padded.ngrams().filter(|w| w.contains(&"a")).count(), 3);
}
```

Padding is what makes a feature vector treat the ends of a sequence like the
middle. It is *not* what a language model's `</s>` does — for probability
normalisation you want a single end symbol, which you build by appending it to
the sequence yourself before calling `ngrams`.

<div class="callout callout-note">
<strong>Note.</strong> <code>Padded</code>'s windows borrow from the
<code>Padded</code> value, not from your original slice. Keep it alive as long as
its windows, and remember that a padded count-of-counts includes the padding
tuples.
</div>

## Pre-tokenized slice vs text

There is deliberately no string entry point for word n-grams. A word n-gram is
the composition, written out at the call site so that the tokenizer is an
argument rather than a hidden policy:

```rust
use std::num::NonZeroUsize;
use verbora_ngrams::ngrams;
use verbora_tokenizers::{BorrowingTokenizer, WordTokenizer};

fn main() {
    let text = "the quick brown fox";
    let n2 = NonZeroUsize::new(2).expect("2 is not zero");
    let n3 = NonZeroUsize::new(3).expect("3 is not zero");

    // One tokenization; both results borrow from `tokens`, nothing is copied.
    let tokens = WordTokenizer.tokenize_borrowed(text);
    assert_eq!((ngrams(&tokens, n2).len(), ngrams(&tokens, n3).len()), (3, 2));
}
```

Splitting the text is the expensive half; sliding a window over a slice you
already have is close to free. Tokenize once, window as many times as you like.

### If your loop is over documents

Tokenize into one reused buffer and window it. The tokens are slices of the
document and the buffer is the only allocation:

```rust
use std::num::NonZeroUsize;
use verbora_ngrams::ngrams;
use verbora_tokenizers::{BorrowingTokenizer, WordTokenizer};

fn main() {
    let corpus = ["the quick brown fox", "jumps over the lazy dog"];
    let n = NonZeroUsize::new(2).expect("2 is not zero");

    // `tokenize_borrowed_into` appends rather than clearing, so the caller
    // clears; the allocation survives the loop.
    let mut buf: Vec<&str> = Vec::new();
    let mut total = 0usize;
    for doc in corpus {
        buf.clear();
        WordTokenizer.tokenize_borrowed_into(doc, &mut buf);
        total += ngrams(&buf, n).len();
    }
    assert_eq!(total, 7);
}
```

See [Buffer reuse](../performance/buffer-reuse.md) and
[Zero-copy](../performance/zero-copy.md).

## Character windows

`char_ngrams` is the one place this crate chooses a unit, and it says so in its
name: **Unicode scalar values**. Every window is a borrowed substring holding
exactly `n` of them, so the windows are usable as map keys directly.

| | `char_ngrams(text, n)` | `ngrams(&text.chars().collect::<Vec<_>>(), n)` |
|---|---|---|
| Element | scalar, as a `&str` window | `char` |
| Allocates | nothing | one `Vec<char>`, and the windows borrow it |
| Window type | `&str` — a substring of `text` | `&[char]` |
| Usable as a map key without copying | ✅ | ❌ (a `&[char]` key is fine, but it is not the text) |

```rust
use std::num::NonZeroUsize;
use verbora_ngrams::char_ngrams;

fn main() {
    let n = NonZeroUsize::new(3).expect("3 is not zero");
    let profile: Vec<&str> = char_ngrams("naïve", n).collect();
    assert_eq!(profile, ["naï", "aïv", "ïve"]);
    assert!(profile.iter().all(|w| "naïve".contains(w)));
}
```

The unit is **not** the grapheme cluster, so a window may split a combining
sequence. That is deliberate: grapheme clusters move with the Unicode version,
and character n-gram keys are the kind of thing programs persist. `char_ngrams`
consults no character database at all, so its output is stable across Unicode
versions — which is exactly what [tokenizers](../features/tokenizers.md) and
[normalizers](../features/normalizers.md) cannot promise.

If you want padded character n-grams, build a `Padded` over
`text.chars().collect::<Vec<_>>()` — one line, and it makes the allocation
visible.

## What this crate does not have

- **No frequency table, no `Nr`, no key function.** Counting is the three-line
  fold shown above, keyed on the n-gram itself.
- **No parallel API.** Every entry point is a free function or an inherent method
  over borrowed input with no interior state, so you can parallelise across
  documents yourself:

  ```rust  ignore
  // `rayon` is not a dependency of verbora-ngrams itself; add it to YOUR
  // crate to write this.
  use rayon::prelude::*;

  let counts: Vec<usize> = corpus
      .par_iter()
      .map(|doc| {
          let tokens = WordTokenizer.tokenize_borrowed(doc);
          verbora_ngrams::ngrams(&tokens, n).len()
      })
      .collect();
  ```

  See [Parallelism](../performance/parallelism.md).

- **No `_into` API.** There is nothing to write into: `ngrams` and `char_ngrams`
  allocate nothing at all. Buffer reuse *is* available one layer down, on the
  tokenizer (`tokenize_borrowed_into`), which is where the per-document
  allocations actually are. See
  [Iterator vs `_into`](../performance/iterator-vs-into.md).

- **No batch API and no global state.** Loop over your documents yourself. See
  [Batch vs streaming](../performance/batch-vs-streaming.md).

- **No dependencies.** The crate's dependency list is empty.

## Related

- [Features: n-grams](../features/ngrams.md) — the full surface and the padding
  definition
- [Choosing an API](./index.md) · [API shapes](./api-shapes.md) ·
  [Choosing a tokenizer](./tokenization.md)
- [Performance](../performance/index.md) ·
  [Iterator vs `_into`](../performance/iterator-vs-into.md)
- [Zero-copy](../performance/zero-copy.md) ·
  [Allocation](../performance/allocation.md)
- [Batch vs streaming](../performance/batch-vs-streaming.md) ·
  [Parallelism](../performance/parallelism.md)
- [Benchmarks](../benchmarks/index.md) · [Recipes](../recipes/index.md)
