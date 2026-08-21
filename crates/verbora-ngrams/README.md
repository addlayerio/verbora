# verbora-ngrams

Every window of `n` consecutive elements of a sequence, in order. `ngrams`
windows a slice of anything; `Padded` windows a copy of it with boundary
symbols attached, so the elements at the two ends appear in as many windows as
the ones in the middle; `char_ngrams` windows the Unicode scalars of a `&str`
and yields substrings. Four items, no dependencies — this is the feature
extraction step for a language identifier, a fuzzy matcher or a classifier, and
nothing more.

## Contract

`n` is a `NonZeroUsize`, which is the whole reason this crate exists next to
`slice::windows`: the precondition that panics there is discharged by the type
here, so no call site needs a guard and **no input to any function in this crate
panics**, in debug or release. Every window holds exactly `n` elements — there
is no short window and no ragged edge — windows come out in left-to-right
position order, and when `n` exceeds the input the result is simply empty
(`len - n + 1` windows, or `0`). Padding is not applied unless you ask for it
with `Padded`, which prepends and appends `k = n - 1` copies of the symbols you
supply — the `n - 1` is Jurafsky & Martin, *Speech and Language Processing*
(3rd ed.) §3.1; the symmetry at the end is Verbora's own decision and is argued
for in the crate documentation. `char_ngrams` measures in Unicode scalar values
and consults no character database at all, so its output is stable across
Unicode versions, which matters when n-gram keys are persisted.

## Example

```rust
use std::num::NonZeroUsize;

use verbora_ngrams::{Padded, char_ngrams, ngrams};

let n = NonZeroUsize::new(2).expect("2 is not zero");
let tokens = ["the", "quick", "brown", "fox"];

// Windows over the caller's slice: nothing allocated, nothing copied.
let grams: Vec<&[&str]> = ngrams(&tokens, n).collect();
assert_eq!(grams, [["the", "quick"], ["quick", "brown"], ["brown", "fox"]]);

// `n` past the end is empty, not an error and not a short window.
let big = NonZeroUsize::new(9).expect("9 is not zero");
assert_eq!(ngrams(&tokens, big).len(), 0);

// Boundary symbols, only when you ask: "the" and "fox" now occur in n windows.
let padded = Padded::new(&tokens, n, Some(&"<s>"), Some(&"</s>"));
assert_eq!(padded.ngrams().len(), 5);
assert_eq!(padded.ngrams().next(), Some(&["<s>", "the"][..]));

// Character windows, borrowed from the input text.
let chars: Vec<&str> = char_ngrams("👍你好", n).collect();
assert_eq!(chars, ["👍你", "你好"]);
```

## See also

Full documentation: <https://verbora.dev/features/ngrams>.

There is no string-input entry point and no tokenizer here on purpose — word
n-grams are the composition, with
[`verbora-tokenizers`](https://crates.io/crates/verbora-tokenizers) supplying
the tokens so that the tokenizer stays an argument rather than a hidden policy.
If you wanted character n-grams in order to *score* string similarity, the
metrics are already written in
[`verbora-distance`](https://crates.io/crates/verbora-distance); if you wanted
them as document features, see
[`verbora-tfidf`](https://crates.io/crates/verbora-tfidf) and
[`verbora-classifiers`](https://crates.io/crates/verbora-classifiers).
