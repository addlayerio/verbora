# Sentiment

`verbora-sentiment` turns a list of tokens into one polarity score, over
fourteen word-list vocabularies in ten languages drawn from three lexicon
projects: AFINN, ML-SentiCon and the CLiPS Pattern project. `SentimentAnalyzer`
does the whole job with one lazy iterator — no model, no training, no
allocation on the scoring path.

<div class="callout callout-warn">
<strong>The word lists are separately licensed.</strong> <code>verbora</code> is
MIT, but only AFINN English publishes a licence of its own (MIT). The
ML-SentiCon and CLiPS Pattern lists, and the unattributed AFINN Spanish and
Portuguese lists, are redistributed here under MIT terms by inheritance only.
<strong>Confirm those terms independently before shipping commercially.</strong>
No separate download is needed — the lexicons are embedded in the binary.
</div>

## When to use it

- Word-list sentiment scoring for one of the ten supported languages, when you
  want a fast, dependency-light score and do not need a trained model.
- Piping a tokenizer's output straight into a score, without collecting the
  document into a `Vec` first.
- A long-lived process scoring many documents: build one analyzer, reuse it.

## When not to use it

- **You need a trained or context-aware model.** This is a word-list lookup
  with sign flipping on negation — no aspect extraction, no sarcasm handling,
  and no window around negation (see [Sticky negation](#sticky-negation)).
- **You need `afinnFinancialMarketNews` to have real data.** That vocabulary
  ships empty; every score against it is `0.0`.
- **You want scores clamped to a fixed range like `[-1, 1]`.** They are not.
- **You cannot accept the lexicons' licensing caveats** for a commercial
  product.

## Quick example

```rust
use verbora_sentiment::SentimentAnalyzer;

fn main() {
    let analyzer = SentimentAnalyzer::without_stemmer("English", "afinn").unwrap();
    assert_eq!(analyzer.get_sentiment(["good"]), 3.0);
    assert_eq!(analyzer.get_sentiment(["not", "good"]), -1.5);
    // Sticky: the second "good" is still negated.
    assert_eq!(analyzer.get_sentiment(["not", "good", "good"]), -2.0);
}
```

It composes directly with a tokenizer's lazy `tokens()` — no document is ever
collected into a `Vec`:

```rust
use verbora_sentiment::SentimentAnalyzer;
use verbora_tokenizers::WordTokenizer;

fn main() {
    let analyzer = SentimentAnalyzer::without_stemmer("English", "afinn").unwrap();
    let tokenizer = WordTokenizer::new();
    let tokens = tokenizer.tokens("This is not a good day.").expect("splitting mode");
    // Six tokens; "not" flips "good", so the total is -3 over 6.
    assert_eq!(analyzer.get_sentiment(tokens), -0.5);
}
```

## Choosing the right API

### The four scoring entry points

`contributions` is the primitive: it borrows the analyzer, consumes any
`IntoIterator<Item: AsRef<str>>`, and yields one `f64` addend per token —
`0.0` for a token that scored nothing, including negation words themselves.
The other three are folds over it. There is exactly one scoring loop in the
crate, and it lives in `Contributions::next`.

| API | Answers | Lazy | Output |
|---|---|:--:|---|
| `contributions(words)` | one addend per token, in order | ✅ | `Contributions<'_, S, I>` → `f64` |
| `score(words)` | the running total and the token count, undivided | ❌ | `Score { sum, count }` |
| `get_sentiment(words)` | `score(words).value()` — divided by tokens visited | ❌ | `f64` |
| `get_sentiment_over(words, len)` | `score(words).over(len)` — explicit denominator | ❌ | `f64` |

None of the four allocates. Pick by what you need:

| You want | Call |
|---|---|
| The final score, denominator = token count | `get_sentiment(words)` |
| The final score with a denominator of your own | `get_sentiment_over(words, len)` |
| The sum and the count separately (combine segments, divide once) | `score(words)` |
| To inspect, filter or short-circuit per token | `contributions(words)` |

```rust
use verbora_sentiment::SentimentAnalyzer;

fn main() {
    let a = SentimentAnalyzer::without_stemmer("English", "afinn").unwrap();

    // The primitive: one addend per token, negation words included as 0.0.
    let deltas: Vec<f64> = a.contributions(["it", "is", "not", "good"]).collect();
    assert_eq!(deltas, [0.0, 0.0, 0.0, -3.0]);

    // get_sentiment is that, folded and divided by the token count.
    assert_eq!(a.get_sentiment(["it", "is", "not", "good"]), -0.75);

    // get_sentiment_over supplies the denominator instead.
    assert_eq!(a.get_sentiment_over(["good"], 2), 1.5);
}
```

`get_sentiment` on an empty input is `NaN` (`0.0 / 0`), not `0.0`.

### With or without a stemmer

`without_stemmer(language, kind)` builds `SentimentAnalyzer<NoStemmer>`;
`new(language, Some(stemmer), kind)` builds `SentimentAnalyzer<S>` for any
`S: Stemmer`. A stemmer does not change how a single token is scored — the
vocabulary is still checked unstemmed first. It changes what the vocabulary
*contains*: the whole table is rebuilt with every key stemmed, so a token that
only matches after stemming (`"goods"` finding `"good"`) becomes reachable.

That rebuild is the entire cost difference, and it is large:

| | No stemmer | + `PorterStemmer`, English AFINN | + `PorterStemmer`, English senticon |
|---|---|---|---|
| First construction | ~61 µs | ~2.5 ms (3,382 keys) | ~23 ms (24,839 keys) |
| Every construction after | **~19 ns** | ~2.5 ms again | ~23 ms again |

An unstemmed analyzer only borrows the process-wide decode of its vocabulary,
so it collapses to a pointer copy. A stemmed one cannot: a stemmer is an
arbitrary caller-supplied function, so nothing about its output can be cached
across constructions. **Build a stemmed analyzer once and reuse it.**

Any `verbora-stemmers` type works with no adapter, and so does your own:

```rust
use verbora_sentiment::{SentimentAnalyzer, Stemmer};
use verbora_stemmers::PorterStemmer;

fn main() {
    let analyzer =
        SentimentAnalyzer::new("English", Some(PorterStemmer::new()), "afinn").unwrap();
    assert_eq!(analyzer.get_sentiment(["not", "good"]), -1.5);

    struct Chop;
    impl Stemmer for Chop {
        fn stem<'a>(&self, word: &'a str) -> std::borrow::Cow<'a, str> {
            word.get(..4).unwrap_or(word).into()
        }
    }
    assert!(SentimentAnalyzer::new("English", Some(Chop), "afinn").is_ok());
}
```

### Vocabulary family and language

`VocabularyKind` has four members and they pair with the ten languages
unevenly. `supported_pairs()` returns all fourteen pairs; entry counts:

| Language | Afinn | AfinnFinancialMarketNews | Senticon | Pattern |
|---|---:|---:|---:|---:|
| Basque | — | — | 4,311 | — |
| Catalan | — | — | 7,270 | — |
| Dutch | — | — | — | 3,304 |
| English | 3,382 | 0 (ships empty) | 24,839 | 1,528 |
| French | — | — | — | 5,113 |
| Galician | — | — | 4,885 | — |
| German | — | — | — | 3,465 |
| Italian | — | — | — | 3,065 |
| Portuguese | 1,644 | — | — | — |
| Spanish | 1,653 | — | 11,344 | — |

75,803 entries total. A `—` cell is not a smaller vocabulary — it is
`Error::UnsupportedLanguage`: `("Dutch", "afinn")` fails rather than silently
falling back to an empty table.

Each vocabulary is paired with a negation list, and five languages have none:

| Language | Negation words |
|---|---|
| English | `not`, `no`, `never`, `neither` |
| Spanish | `no`, `nunca`, `jamás`, `ni` |
| Portuguese | `não`, `nunca`, `jamais`, `nem` |
| Dutch | `niet`, `nooit`, `niemand`, `niets`, `nee`, `neen` |
| German | `kein`, `nein`, `nicht` |
| Galician, Catalan, Basque, Italian, French | *(none)* |

Where there is no negation list, `is_negation` is never true and every token is
scored on the vocabulary lookup alone.

```rust
use verbora_sentiment::{Language, SentimentAnalyzer, VocabularyKind, supported_pairs};

fn main() {
    assert_eq!(supported_pairs().len(), 14);
    assert_eq!(
        VocabularyKind::Senticon.languages(),
        ["Spanish", "English", "Galician", "Catalan", "Basque"]
    );

    // Language and VocabularyKind are a typed convenience; the constructor
    // still takes &str either way.
    let analyzer = SentimentAnalyzer::without_stemmer(
        Language::English.as_str(),
        VocabularyKind::Afinn.as_str(),
    )
    .unwrap();
    assert_eq!(analyzer.get_sentiment(["good"]), 3.0);
}
```

### Scoring many documents

`par_get_sentiment_batch`, behind the `parallel` Cargo feature, is exactly
`docs.par_iter().map(|d| self.get_sentiment(d)).collect()` — the same
`get_sentiment`, fanned out, not a second scoring loop.

```rust  ignore
use verbora_sentiment::SentimentAnalyzer;

fn main() {
    let analyzer = SentimentAnalyzer::without_stemmer("English", "afinn").unwrap();
    let docs = vec![vec!["good"], vec!["not", "good"], vec![]];
    let scores = analyzer.par_get_sentiment_batch(&docs);
    assert_eq!(scores[0], 3.0);
    assert_eq!(scores[1], -1.5);
    assert!(scores[2].is_nan());
}
```

| Batch shape | Use |
|---|---|
| One document, or a handful under a few hundred tokens | `get_sentiment` in a loop — Rayon's fork-join overhead dominates |
| Hundreds of documents, or documents in the thousands of tokens | `par_get_sentiment_batch` — the win grows on both axes |
| You already have your own thread pool | `get_sentiment` — do your own fan-out |

Requires `S: Sync`, always true for `NoStemmer` and every `verbora-stemmers`
type. See [Parallelism](../performance/parallelism).

## Behaviour worth knowing

### Sticky negation

The negator is set to `-1.0` by the first negation word and is **never
restored** — not after one token, not after punctuation, not at a sentence
boundary:

```rust
use verbora_sentiment::SentimentAnalyzer;

fn main() {
    let a = SentimentAnalyzer::without_stemmer("English", "afinn").unwrap();
    // Both "good"s are worth 3.0 unnegated; the first "not" flips both.
    assert_eq!(a.get_sentiment(["not", "good", "good"]), -2.0);
    // A negation AFTER the positive word does not apply retroactively.
    assert_eq!(a.get_sentiment(["good", "not", "good"]), 0.0);
    // Punctuation between "not" and "good" does not reset the negator.
    assert_eq!(a.get_sentiment(["not", ".", "good"]), -1.0);
}
```

The negation test runs **before** the vocabulary lookup, so a word that is both
a lexicon entry and a negation word scores nothing at all — English `no` is
worth `-1` in AFINN and is also a negation word, so `get_sentiment(["no"])` is
`0.0`.

### Stem collisions resolve last-wins, in file order

Supplying a stemmer rebuilds the vocabulary in source-file order, letting a
later stem overwrite an earlier one. For English AFINN with the Porter stemmer,
3,382 keys collapse to 1,967 stems, and 109 of those collisions change the
stored polarity:

```rust
use verbora_sentiment::{Vocabulary, VocabularyKind};
use verbora_stemmers::PorterStemmer;

fn main() {
    let base = Vocabulary::shared(VocabularyKind::Afinn, "English").unwrap();
    let stemmed = base.stemmed(&PorterStemmer::new());
    assert_eq!(base.len(), 3382);
    assert_eq!(stemmed.len(), 1967);

    assert_eq!(base.polarity("affection"), Some(3.0));
    assert_eq!(stemmed.polarity("affect"), Some(3.0)); // affection's value wins
    assert_eq!(stemmed.polarity("arrest"), Some(-2.0)); // arrests's value wins
}
```

The order is file order, not hash or alphabetical order, and it is stable
across runs and platforms.

### Summation is left-to-right and bit-reproducible

`score` accumulates in `f64` strictly left to right and divides exactly once at
the end. `Contributions` yields a real `0.0` addend for every token that scored
nothing, so folding it by hand reproduces the same bits:

```rust
use verbora_sentiment::SentimentAnalyzer;

fn main() {
    let a = SentimentAnalyzer::without_stemmer("English", "senticon").unwrap();
    let tokens = ["good", "bad", "excellent", "awful", "fine"];

    let by_explicit_fold: f64 = a.contributions(tokens).fold(0.0, |acc, d| acc + d);
    assert_eq!(a.score(tokens).sum.to_bits(), by_explicit_fold.to_bits());
}
```

### Constructor errors

The lookup is two-level — vocabulary type, then language within it — with a
matching two-level failure:

```rust
use verbora_sentiment::SentimentAnalyzer;

fn main() {
    let e = SentimentAnalyzer::without_stemmer("English", "bogus").unwrap_err();
    assert_eq!(e.to_string(), "Type Language bogus not supported");

    let e = SentimentAnalyzer::without_stemmer("Klingon", "afinn").unwrap_err();
    assert_eq!(e.to_string(), "Type afinn for Language Klingon not supported");
}
```

Two rarer variants, `Error::ObjectPrototype` and `Error::RestrictedProperty`,
reject a small fixed set of reserved `language` arguments (`"toString"`,
`"constructor"`, `"__proto__"`, `"caller"`, `"arguments"`).

### Concurrency

`SentimentAnalyzer` is immutable after construction — nothing is cached or
mutated per call — and is `Send + Sync` whenever its stemmer type parameter is
(`NoStemmer` always is). Build once, wrap in an `Arc`, share read-only across
threads with no locking.

## Performance characteristics

`crates/verbora-sentiment/benches/sentiment.rs` covers cold decode,
construction with and without a stemmer, scoring at several document sizes, and
the token-shape cost of case-folding. Measured on one machine with
`cargo bench -p verbora-sentiment`:

| Operation | Cost |
|---|---|
| Process start / first touch | 0 — nothing is decoded until a vocabulary is asked for |
| First `("English", "afinn")` | 61 µs, once per process |
| First `("English", "senticon")` | 644 µs, once per process |
| Every construction after the first | **19 ns** |
| Decoding all fourteen tables | 7.7 ms, once |
| `+ PorterStemmer`, senticon | 23 ms, per construction |
| Scoring 2,816 English tokens | 30 µs |
| Throughput, lowercase ASCII | 94 M tokens/s |
| Throughput, uppercase ASCII | 61 M tokens/s (one `to_ascii_lowercase` per token) |
| Throughput, non-ASCII | 41 M tokens/s (full Unicode `to_lowercase`) |

The lexicons ship as prebuilt `key \0 polarity \0` blobs — 75,803 pairs, 1.2 MB
across thirteen `include_bytes!` blobs (the fourteenth is empty). Each is
decoded on first use with one `str::split_terminator('\0')` pass and cached for
the process behind a `OnceLock`. A program that never scores sentiment pays
nothing for the data being present. See
[Benchmarks](../benchmarks/index) to reproduce these numbers.

## Allocation behaviour

| Step | Allocates |
|---|---|
| First load of a vocabulary | One `Vec<Entry>` and one hash map, sized to the entry count. Keys are `Cow::Borrowed` slices of the embedded `'static` blob — no string data is copied. Once per process, per vocabulary. |
| Constructing without a stemmer | Two `String`s (the stored `language()` and `vocabulary_type()`). The table itself is a pointer copy. |
| Constructing with a stemmer | A fresh `Vec` and hash map, pre-sized. Each key becomes `Cow::Borrowed` if the stemmer left it unchanged, `Cow::Owned` if not. Polarity values are never copied. |
| Scoring | Nothing of its own. The lowercasing helper returns `Cow::Borrowed` for a token that is already lowercase (checked by a scan before committing to an allocation), and an owned `String` only for one that genuinely needs case-folding. `polarity` is one hash lookup and one `Vec` index. |

There is no `_into` variant and no caller-supplied output buffer anywhere in
this crate — nothing collects a document into an owned collection, so there is
nothing to reuse a buffer for. See
[Allocation](../performance/allocation) and
[Zero-copy](../performance/zero-copy).

## Unicode and language notes

- **Lowercasing is the full Unicode algorithm**, not `to_ascii_lowercase`:
  `İ` expands to two code units (`i̇`), and Greek Final_Sigma is honoured —
  `"ΑΣ"` lowercases to `"ας"`, not `"ασ"`.
- **Vocabulary lookup is exact — no accent folding, no normalisation beyond
  case.** `naïve` really is an AFINN-165 key, accent and all; `café`,
  `Ångström` and `crème` are not, and score `0.0`.
- **Capitalised and multi-word entries are unreachable from a single lowercased
  token.** German pattern has 1,234 capitalised entries; English senticon has
  5,960 containing a space. A stemmer does not fix this — stemming operates on
  one already-tokenized word.
- **Spanish and Portuguese AFINN key on emoji directly.** `😂` is a real entry
  (`1.0`). CJK and other astral-plane characters that are not entries score
  `0.0` without erroring, like any other miss.

## Common mistakes

**Constructing a stemmed analyzer inside a request handler or per-document
loop.** Every construction pays the full rebuild — 23 ms for English senticon.
Build it once:

```rust
use verbora_sentiment::SentimentAnalyzer;
use verbora_stemmers::PorterStemmer;

fn main() {
    // Build once...
    let analyzer =
        SentimentAnalyzer::new("English", Some(PorterStemmer::new()), "afinn").unwrap();

    // ...and reuse it for every document.
    let documents = [["not", "good"], ["good", "good"]];
    let scores: Vec<f64> = documents
        .iter()
        .map(|doc| analyzer.get_sentiment(doc.iter().copied()))
        .collect();
    assert_eq!(scores, [-1.5, 3.0]);
}
```

**Expecting `afinnFinancialMarketNews` to have real data.** It ships empty;
`a.vocabulary().len()` is `0` and every score against it is `0.0`.

**Assuming scores are normalized to `[-1, 1]`.** Nothing clamps anything.
`get_sentiment` is an average, so it stays within the extremes of the
vocabulary you chose — `-5..=5` for AFINN, `-1.0..=1.0` for senticon and
pattern — not within any universal range. `score().sum` has no bound at all:

```rust
use verbora_sentiment::SentimentAnalyzer;

fn main() {
    let a = SentimentAnalyzer::without_stemmer("English", "afinn").unwrap();
    let long_review: Vec<&str> = std::iter::repeat_n("good", 10_000).collect();

    // The AVERAGE stays within AFINN's own -5..=5 range.
    assert_eq!(a.get_sentiment(long_review.iter().copied()), 3.0);
    // The raw, undivided SUM does not: it grows with the document.
    assert_eq!(a.score(long_review.iter().copied()).sum, 30_000.0);
}
```

**Forgetting that a negation word is never also scored.** `no` is a real AFINN
entry worth `-1.0` *and* an English negation word, so it always contributes
`0.0` and flips what follows.

**Confusing the constructor's argument order.** It is `(language, stemmer,
vocabulary_type)` — not `(type, language, …)`. Getting it backwards does not
panic; both arguments are plain strings, so it produces a confusingly-worded
but valid `Error`.

## Related

- [Tokenizers](./tokenizers) — the other half of
  `analyzer.get_sentiment(tokenizer.tokens(text))`.
- [Stemmers](./stemmers) — every type there implements `Stemmer` directly.
- [Iterator vs. `_into`](../performance/iterator-vs-into) — the lazy/fold
  distinction behind `contributions` vs `score`.
- [Allocation](../performance/allocation) and
  [Zero-copy](../performance/zero-copy).
- [Parallelism](../performance/parallelism).
- [Benchmarks](../benchmarks/index).

## API reference

```rust ignore
// verbora_sentiment
pub enum VocabularyKind { Afinn, AfinnFinancialMarketNews, Senticon, Pattern }
impl VocabularyKind {
    pub const ALL: [Self; 4];
    pub const fn as_str(self) -> &'static str;
    pub fn languages(self) -> Vec<&'static str>;
    pub fn from_js(s: &str) -> Option<Self>;
}

pub enum Language { English, Spanish, Portuguese, Galician, Catalan, Basque, Dutch, Italian, French, German }
impl Language {
    pub const ALL: [Self; 10];                       // alphabetical
    pub const fn as_str(self) -> &'static str;
    pub fn from_js(s: &str) -> Option<Self>;
}

pub fn supported_pairs() -> Vec<(VocabularyKind, &'static str)>; // 14 pairs, table order

pub struct SentimentAnalyzer<S = NoStemmer> { /* private */ }
impl SentimentAnalyzer<NoStemmer> {
    pub fn without_stemmer(language: &str, vocabulary_type: &str) -> Result<Self, Error>;
}
impl<S: Stemmer> SentimentAnalyzer<S> {
    pub fn new(language: &str, stemmer: Option<S>, vocabulary_type: &str) -> Result<Self, Error>;
    pub fn language(&self) -> &str;
    pub fn vocabulary_type(&self) -> &str;
    pub fn vocabulary(&self) -> &Vocabulary;
    pub fn negations(&self) -> &'static [&'static str];
    pub fn stemmer(&self) -> Option<&S>;

    pub fn contributions<I>(&self, words: I) -> Contributions<'_, S, I::IntoIter>
        where I: IntoIterator, I::Item: AsRef<str>;
    pub fn score<I>(&self, words: I) -> Score
        where I: IntoIterator, I::Item: AsRef<str>;
    pub fn get_sentiment<I>(&self, words: I) -> f64
        where I: IntoIterator, I::Item: AsRef<str>;
    pub fn get_sentiment_over<I>(&self, words: I, len: usize) -> f64
        where I: IntoIterator, I::Item: AsRef<str>;

    // requires the `parallel` Cargo feature; S: Sync
    pub fn par_get_sentiment_batch<D>(&self, docs: &[D]) -> Vec<f64>
        where D: Sync, /* &D: IntoIterator<Item: AsRef<str>> */;
}

pub struct Contributions<'a, S, I> { /* private */ }
impl<S: Stemmer, I> Iterator for Contributions<'_, S, I>
    where I: Iterator, I::Item: AsRef<str> { type Item = f64; }

pub struct Score { pub sum: f64, pub count: usize }
impl Score {
    pub fn value(self) -> f64;            // sum / count   (NaN if count == 0)
    pub fn over(self, len: usize) -> f64; // sum / len
}

pub enum Error {
    UnsupportedType { vocabulary_type: String },
    UnsupportedLanguage { vocabulary_type: String, language: String },
    ObjectPrototype { value: char },
    RestrictedProperty,
}
impl Error { pub fn error_name(&self) -> &'static str; } // "Error" | "TypeError"

pub trait Stemmer {
    fn stem<'a>(&self, word: &'a str) -> std::borrow::Cow<'a, str>;
}
impl<T: Stemmer + ?Sized> Stemmer for &T {}
impl<T: Stemmer + ?Sized> Stemmer for Box<T> {}
impl<T: Stemmer + ?Sized> Stemmer for std::sync::Arc<T> {}
// Every verbora_stemmers type implements Stemmer with no adapter.

pub struct NoStemmer;
impl Stemmer for NoStemmer { /* identity: always Cow::Borrowed */ }

pub enum Polarity { Number(f64), Text(&'static str) }
impl Polarity {
    pub fn as_f64(self) -> f64;
    pub fn as_str(self) -> Option<&'static str>; // Some(_) only for Text
}

pub struct Vocabulary { /* private */ }
impl Vocabulary {
    pub fn kind(&self) -> Option<VocabularyKind>;
    pub fn len(&self) -> usize;
    pub fn is_empty(&self) -> bool;
    pub fn get(&self, word: &str) -> Option<Polarity>;
    pub fn polarity(&self, word: &str) -> Option<f64>;
    pub fn keys(&self) -> impl ExactSizeIterator<Item = &str>;
    pub fn iter(&self) -> impl ExactSizeIterator<Item = (&str, Polarity)>;
    pub fn shared(kind: VocabularyKind, language: &str) -> Option<&'static Self>;
    pub fn shared_for(kind: VocabularyKind, language: Language) -> Option<&'static Self>;
    pub fn stemmed<S: Stemmer + ?Sized>(&'static self, stemmer: &S) -> Self;
}
```

No `unsafe`, no global mutable state, no `_into` buffer-reuse variant.
`SentimentAnalyzer<S>` and `Vocabulary` are `Send + Sync` whenever `S` is.
`par_get_sentiment_batch` is the crate's only parallel entry point, gated
behind the `parallel` Cargo feature and off by default.
