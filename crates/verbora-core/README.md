# verbora-core

The vocabulary the rest of [Verbora](https://verbora.dev) is written against.
Two things live here and nothing else does: the five traits more than one crate
has to agree on — `Tokenizer`, `BorrowingTokenizer`, `Stemmer`, `Phonetic` and
`DoubleKeyPhonetic` — and the sixteen stop-word lists, which are shared data
rather than shared behaviour. Reach for it when you are writing code that is
generic over tokenizers or stemmers, plugging your own implementation into a
Verbora pipeline, or filtering stop words without wanting a stemmer for the
sake of its list. If you only want to tokenize or stem some text, install the
crate that does that; it re-exports what it needs from here.

## Contract

There are no dependencies on other `verbora-*` crates and no data assets beyond
the stop-word lists themselves, which is what lets a leaf crate be used in
isolation, and the crate root is the entire public surface — every module is
private, so there is exactly one path to each item. The traits are total: a
`Stemmer` has a stem for every `&str` including `""`, a `Tokenizer` never yields
an empty token and yields tokens in input order, a `Phonetic` encoder answers
`""` rather than an error for a token it cannot index, and
`DoubleKeyPhonetic`'s alternate key is `Option::None` when there is none rather
than a duplicate of the primary standing in as a sentinel. Stop-word membership
is exact `str` equality over Unicode scalar sequences — no case folding, no
normalization, no trimming — and `StopWordLanguage::En` describes the *shipped*
list while the `*_global_*` functions read and write a mutable process-global
one, so the two need not agree after anyone has changed the global.

## Example

```rust
use verbora_core::{StopWordLanguage, StopWords, Tokenizer};

// Anything can be a Verbora tokenizer: one required method.
struct Whitespace;

impl Tokenizer for Whitespace {
    fn tokenize_into(&self, text: &str, out: &mut Vec<String>) {
        out.extend(text.split_whitespace().map(str::to_owned));
    }
}

let stops = StopWords::for_language(StopWordLanguage::En);
let kept: Vec<String> = Whitespace
    .tokenize("the quick brown fox")
    .into_iter()
    .filter(|token| !stops.contains(token))
    .collect();
assert_eq!(kept, ["quick", "brown", "fox"]);

// Membership is exact — nothing is folded on your behalf.
assert!(StopWordLanguage::En.is_stopword("the"));
assert!(!StopWordLanguage::En.is_stopword("The"));
```

## See also

Full documentation: <https://verbora.dev/features/core>.

You probably want an implementation rather than the traits:
[`verbora-tokenizers`](https://crates.io/crates/verbora-tokenizers) for UAX #29
word and sentence boundaries, [`verbora-stemmers`](https://crates.io/crates/verbora-stemmers)
for sixteen stemmers, [`verbora-phonetics`](https://crates.io/crates/verbora-phonetics)
for Soundex, Metaphone, Double Metaphone and friends. The stop-word lists are
re-exported by [`verbora-util`](https://crates.io/crates/verbora-util) under
that crate's older `Language` name, alongside abbreviation tables.
