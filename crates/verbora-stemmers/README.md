# verbora-stemmers

Sixteen stemmers for fourteen languages, so that `running`, `runs` and `run`
collide on one key in a search index, a TF-IDF model or a classifier. English
gets Porter and Lancaster; Snowball-family stemmers cover eleven more languages;
and beyond those there is the Carry variant as a second option for French, a
Japanese katakana stemmer and a dictionary-driven Indonesian one. Fifteen of the
sixteen also implement `TokenizeAndStem`, which cuts text into tokens, drops
stop words and stems what is left in a single pass.

## Contract

The algorithms are published ones and each stemmer's own documentation names the
one it implements: Porter's suffix-stripping algorithm (M. F. Porter, "An
algorithm for suffix stripping", *Program* 14(3), 1980, 130–137) and its
Snowball successors, the Lancaster stemmer of Paice and Husk, and
Sastrawi/Nazief–Adriani for Indonesian. **The text unit is the Unicode scalar
value** — R1, R2 and RV, every short-word gate and every rule's removal `size`
count scalars rather than UTF-16 code units, in every stemmer here: below
`U+10000` the two readings coincide exactly and the crate
sweeps the Basic Multilingual Plane proving it, while above it they part, so
`PorterStemmer::stem("😀s")` returns `"😀s"` untouched because two scalars do not
clear Porter's three-letter gate. Stemming is total — every `&str` has a stem,
including `""`, with no error case and no panic — and a stem is *not* guaranteed
to be a prefix, a substring, or shorter than its input. Two pieces of state
outlive a call and are part of the observable behaviour: `PorterStemmerNl`
carries a sticky `suffix_e_removed` flag that nothing resets, and the stop-word
lists are process-global, so adding one through `PorterStemmer` also changes
`LancasterStemmer`. One stemmer does less than its name suggests, and says so:
`PorterStemmerFa`'s `stem` is the identity function, so all its `tokenize_and_stem`
does for Persian is tokenize and drop stop words.

## Example

```rust
use verbora_stemmers::{LancasterStemmer, PorterStemmer, TokenizeAndStem};

let porter = PorterStemmer::new();
assert_eq!(porter.stem("running"), "run");

// Tokenize, drop stop words, stem — one pass. `false` means drop them.
assert_eq!(
    porter.tokenize_and_stem("My dog is very fun to play with", false),
    ["dog", "fun", "plai"],
);

// A stemmer is reductive: `plai` is not a word, and that is not a bug.
// A different algorithm, on the same shared English stop-word list.
assert_eq!(LancasterStemmer::new().stem("maximum"), "maxim");
```

## See also

Full documentation: <https://verbora.dev/features/stemmers>.

If you wanted a real dictionary form rather than a reductive key, a stemmer is
the wrong tool — see [`verbora-inflectors`](https://crates.io/crates/verbora-inflectors)
for the generative direction. If you want a tokenization other than the one
`TokenizeAndStem` performs, take the tokens from
[`verbora-tokenizers`](https://crates.io/crates/verbora-tokenizers) and call
`stem` per token. To be generic over any stemmer, the `Stemmer` trait is in
[`verbora-core`](https://crates.io/crates/verbora-core); to weight the resulting
stems, [`verbora-tfidf`](https://crates.io/crates/verbora-tfidf).
