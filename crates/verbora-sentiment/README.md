# verbora-sentiment

Lexicon-based sentiment scoring for ten languages, from fourteen vocabularies
drawn from three published projects — AFINN, ML-SentiCon and the CLiPS Pattern
project. Feed it tokens, get a polarity back. There is no model to train, no
data to download and no file read at run time: the 75,803 `(key, polarity)`
pairs are compiled into the binary as packed blobs, and each table is decoded on
first use of that one `(kind, language)` pair.

## What it guarantees

The input is **any iterator of string-like tokens**, and the scoring loop
consumes it one *unit* at a time — where a unit is the **longest span of tokens
that spells a lexicon key**, not necessarily one token. That is the whole point:
lexicons publish entries such as `cover-up`, `bad luck` and `son-of-a-bitch`,
which no UAX #29 token stream ever contains, so keys and tokens are both reduced
to the same lookup form before they meet and 14,273 of the shipped entries stay
reachable instead of becoming dead weight. Negation is *sticky* — one negation
word flips the sign for the rest of the input, with no window and no reset on
punctuation — but the span scan runs first, so a phrase the lexicon actually
publishes (`not good` at -2) beats the heuristic's guess. The sum is accumulated
in `f64` strictly left to right and divided exactly once at the end; where the
division has no answer, because nothing was scored or the denominator is zero,
the result is `None` rather than a `NaN`.

Nothing here is normalised, stemmed or lowercased on your behalf beyond the
lookup form, and `SentimentAnalyzer` is read-only once built, so one instance
can be shared across threads.

**No performance figures are published.** The ones this crate used to carry
predated lookup forms and span matching and were withdrawn as unmeasured rather
than left to go stale.

## Example

```rust
use verbora_sentiment::{Language, SentimentAnalyzer, VocabularyKind};

let analyzer =
    SentimentAnalyzer::without_stemmer(Language::English, VocabularyKind::Afinn).unwrap();

assert_eq!(analyzer.get_sentiment(["good"]), Some(3.0));

// Sticky negation: `not` flips `happy`, and the mean is over both tokens.
assert_eq!(analyzer.get_sentiment(["not", "happy"]), Some(-1.5));

// `cover-up` is one lexicon key. The tokenizer cuts it in two; the span scan
// puts it back together and scores it as a single unit.
assert_eq!(analyzer.get_sentiment(["cover", "up"]), Some(-3.0));

// A phrase the lexicon publishes outranks the negation heuristic.
assert_eq!(analyzer.get_sentiment(["not", "good"]), Some(-2.0));

// The mean polarity of no text does not exist, and is reported as absent.
assert_eq!(analyzer.get_sentiment(Vec::<&str>::new()), None);
```

## See also

- Full documentation: <https://verbora.dev/features/sentiment>
- [`verbora-tokenizers`](https://crates.io/crates/verbora-tokenizers) — where the
  tokens come from; `WordTokenizer.tokens(text)` pipes straight into
  `get_sentiment`.
- [`verbora-classifiers`](https://crates.io/crates/verbora-classifiers) — if a
  fixed lexicon is the wrong tool and you have labelled examples to train on
  instead.
- [`verbora-stemmers`](https://crates.io/crates/verbora-stemmers) — every
  Verbora stemmer implements this crate's `Stemmer` trait, so
  `SentimentAnalyzer::with_stemmer` composes with them without an adapter.
