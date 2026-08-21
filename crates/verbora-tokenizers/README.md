# verbora-tokenizers

Cuts text at [UAX #29](https://www.unicode.org/reports/tr29/) boundaries and
hands back the pieces. Three tokenizers, one token shape: `WordTokenizer` yields
the word segments containing a letter or a digit, `SegmentTokenizer` yields
*every* word segment so that concatenation reproduces the input, and
`SentenceTokenizer` yields sentences with an optional abbreviation tailoring so
that `Dr. Smith` is not two of them. It is for anyone who needs boundaries that
match the standard rather than a hand-rolled `split_whitespace`, in a search
indexer, a feature extractor or a highlighter.

## Contract

Word and sentence boundaries are the rules of [UAX #29](https://www.unicode.org/reports/tr29/)
§4 and §5, computed over the `Word_Break` and `Sentence_Break` properties of the
Unicode Character Database; the version in force is whatever
`unicode-segmentation` ships and `unicode_version()` reports it at runtime, so
anything that persists tokenizer-derived keys should stamp that version and
refuse to load across a change. Every token is a contiguous slice borrowed from
the input — nothing here folds case, trims, strips punctuation or substitutes
placeholders, and no token is ever the empty string. Sentences are *untrimmed*,
so a sentence carries its own trailing whitespace and concatenation is the input
byte for byte; write `.map(str::trim)` at the call site if you want otherwise.
Note the limitation UAX #29 §4 states itself: the default rules do not segment
languages written without spaces, so each Han scalar becomes its own token.

## Example

```rust
use verbora_tokenizers::{BorrowingTokenizer, SentenceTokenizer, WordTokenizer};

// Words: zero-copy slices of the input, no folding, no stripping.
assert_eq!(
    WordTokenizer.tokenize_borrowed("The quick (\"brown\") fox can't jump 32.3 feet, right?"),
    ["The", "quick", "brown", "fox", "can't", "jump", "32.3", "feet", "right"],
);

// Sentences, with the abbreviation tailoring the standard says is needed.
let text = "Dr. Smith arrived. He left.";
assert_eq!(
    SentenceTokenizer::new().tokenize_borrowed(text),
    ["Dr. ", "Smith arrived. ", "He left."],
);
assert_eq!(
    SentenceTokenizer::with_abbreviations(["Dr."]).unwrap().tokenize_borrowed(text),
    ["Dr. Smith arrived. ", "He left."],
);
```

## See also

Full documentation: <https://verbora.dev/features/tokenizers>.

Tokenizing is usually the first step of something else. If you wanted the text
*rewritten* rather than cut — case folded, composed, accent-stripped — that is
[`verbora-normalizers`](https://crates.io/crates/verbora-normalizers), and
nothing here will do it for you. To reduce the tokens to stems, and drop stop
words in the same pass, see [`verbora-stemmers`](https://crates.io/crates/verbora-stemmers);
for windows over the tokens, [`verbora-ngrams`](https://crates.io/crates/verbora-ngrams).
The `Tokenizer` and `BorrowingTokenizer` traits themselves live in
[`verbora-core`](https://crates.io/crates/verbora-core).
