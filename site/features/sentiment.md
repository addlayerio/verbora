# Sentiment

`verbora-sentiment` turns a list of tokens into one polarity score, over
fourteen word-list vocabularies in ten languages drawn from three lexicon
projects: AFINN, ML-SentiCon and the CLiPS Pattern project. `SentimentAnalyzer`
does the whole job with one lazy iterator: sticky negation, stem-collision
resolution and left-to-right floating-point summation.

<div class="callout callout-spec">
<strong>Specification status.</strong> Every stemmed and unstemmed vocabulary,
every negation list and the lower-casing rule are documented and test-pinned.
<code>cargo test -p verbora-sentiment</code> runs <strong>34</strong> unit
tests and <strong>7</strong> doctests.
</div>

## The lexicons are separately licensed

`verbora` is MIT. The **word lists themselves** carry separate upstream
provenance, and the reference tree — which this crate's fixtures and shipped
data are drawn from — ships **no licence file of its own** for three of the
four families:

| Vocabulary | Upstream | Licence found in the reference tree |
|---|---|---|
| AFINN English | `afinn-165`, Titus Wormer | MIT (the package's own `license` file) |
| AFINN Spanish, Portuguese | shipped JSON, no attribution | none — MIT by inheritance only |
| senticon (es, en, gl, ca, eu) | ML-SentiCon, converted by `tools/sentimentXmlParser` | none |
| pattern (nl, it, en, fr, de) | CLiPS Pattern project, converted by `tools/XmlParser4PatternData` | none |

The reference itself ships all of this under one MIT `LICENSE.txt` (Chris Umbel,
Rob Ellis, Russell Mull), with the sentiment sources carrying their own MIT
headers (Domingo Martín Mancera and Hugo W.L. ter Doest, based on
`dmarman/lorca`). Redistributing the AFINN, senticon and pattern word lists
here is exactly as well-founded as the reference's own redistribution — no more,
no less. **If you plan to ship this crate commercially, confirm the
ML-SentiCon and Pattern terms independently**; neither has a licence file
anywhere in the tree this was ported from.

Unlike [WordNet](./wordnet), none of this requires a separate download: the
lexicons are 1.2 MB of `key \0 polarity \0` blobs embedded directly in the
crate binary (see [How the lexicons ship](#how-the-lexicons-ship-and-why)
below) — but the licensing caveat above travels with the data regardless of
how it is packaged.

## When to use it

- **Porting the reference that called the reference's `SentimentAnalyzer`.** Every
  observable behaviour — sticky negation, stem-collision winners, the
  `score / words.length` division including its `NaN` on empty input — is
  reproduced exactly, verified against 11,608 recorded cases.
- **Word-list sentiment scoring for one of the ten supported languages**,
  where you want a fast, dependency-light score and do not need a trained
  model.
- **Piping a tokenizer's output straight into a score** without collecting a
  document into a `Vec` first — see
  [`contributions`](#contributions-score-and-get-sentiment-the-lazy-primitive-and-its-folds).
- **A long-lived process scoring many documents.** Build one
  `SentimentAnalyzer` and reuse it: construction without a stemmer is a
  pointer copy after the first call, and even a stemmed analyzer is built
  once and scores arbitrarily many documents afterwards.

## When not to use it

- **You need a trained or context-aware sentiment model.** This is a
  word-list lookup with sign flipping on negation — there is no machine
  learning here, no aspect extraction, no sarcasm handling, and no windowing
  around negation (see [Sticky negation](#sticky-negation-there-is-no-window)).
- **You need `afinnFinancialMarketNews` to have real data.** It is empty on
  purpose — a reproduced upstream bug, not a missing feature. See
  [Deliberate divergences](#deliberate-divergences-fallible).
- **You want scores normalized to a fixed range like `[-1, 1]`.** They are
  not; see [Common mistakes](#common-mistakes).
- **You cannot accept the lexicons' licensing caveats** for a commercial
  product — see [above](#the-lexicons-are-separately-licensed).

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

`contributions` is the primitive the whole crate is built on, and it composes
directly with a tokenizer's own lazy `tokens()` — the same "iterator first,
convenience methods fold over it" shape [Tokenizers](./tokenizers) and
[Phonetics](./phonetics) use:

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

No document is ever collected into a `Vec` here: `tokens()` yields `&str`
lazily, `get_sentiment` folds `contributions` over it, and nothing between the
two allocates.

## Choosing the right API

Five independent decisions sit under `SentimentAnalyzer`, and none of them has
one universally correct answer: whether to supply a stemmer, which of the
three scoring entry points to call, which vocabulary family and language to
score against, whether `get_sentiment` or `get_sentiment_over` is the one your
denominator needs, and — once there is more than one document to score —
whether the sequential loop or [`par_get_sentiment_batch`](#scoring-many-documents-par-get-sentiment-batch)
is worth reaching for.

### With or without a stemmer

`SentimentAnalyzer::without_stemmer(language, kind)` builds
`SentimentAnalyzer<NoStemmer>`; `SentimentAnalyzer::new(language,
Some(stemmer), kind)` builds `SentimentAnalyzer<S>` for any `S: Stemmer`.
Supplying a stemmer does not change how a *single token* is scored — the
vocabulary is still checked unstemmed first — it changes what the vocabulary
itself contains: the whole table is rebuilt by stemming every one of its
keys, so a token that only matches a lexicon entry after stemming (`"goods"`
finding `"good"`) becomes reachable.

That rebuild is the entire cost difference between the two constructors, and
it is large. Measured on one machine with `cargo bench -p verbora-sentiment`
against the reference (methodology and full table under
[Performance characteristics](#performance-characteristics)):

| | No stemmer | + `PorterStemmer`, English AFINN | + `PorterStemmer`, English senticon |
|---|---|---|---|
| First-ever construction (builds the table) | ~61 µs | ~2.5 ms (stem 3,382 keys) | ~23 ms (stem 24,839 keys) |
| Every construction after that | **~19 ns** | ~2.5 ms again | ~23 ms again |
| Reference (the reference), every construction | 612 µs | 8.0 ms | 72 ms |

The "no stemmer" column collapses to 19 nanoseconds after the first call
because an unstemmed analyzer only ever borrows the process-wide decode of its
vocabulary — see
[The unstemmed vocabulary is shared, not copied](#deliberate-divergences-fallible). A
stemmed analyzer cannot get that discount: a stemmer is an arbitrary
caller-supplied function, so nothing about its output can be precomputed or
cached across constructions. **This is the one construction cost that does
not improve with reuse of the crate's internal cache — only with reuse of the
`SentimentAnalyzer` itself.** Build it once.

```text
Do I need a stemmer?
│
├── The vocabulary's own inflected forms are enough
│      └── SentimentAnalyzer::without_stemmer(language, kind)
│             — construction is a pointer copy after the first call
│
└── Input tokens won't match the vocabulary's exact spelling
       (plurals, verb forms, e.g. German pattern's 1,234
        capitalised entries that need case+stem normalisation)
       └── SentimentAnalyzer::new(language, Some(stemmer), kind)
              — pays a real, non-amortizable rebuild every construction;
                build once, keep the analyzer, reuse it for every document
```

Any `verbora-stemmers` type works with no adapter — `Stemmer` is implemented
for all fifteen of them via a bridging macro — and so does your own:

```rust
use verbora_sentiment::{SentimentAnalyzer, Stemmer};
use verbora_stemmers::PorterStemmer;

fn main() {
    let analyzer =
        SentimentAnalyzer::new("English", Some(PorterStemmer::new()), "afinn").unwrap();
    assert_eq!(analyzer.get_sentiment(["not", "good"]), -1.5);

    // Anything with a `.stem()` composes, exactly as the reference's duck
    // typing allows any object with `.stem(word)` as the second constructor
    // argument.
    struct Chop;
    impl Stemmer for Chop {
        fn stem<'a>(&self, word: &'a str) -> std::borrow::Cow<'a, str> {
            word.get(..4).unwrap_or(word).into()
        }
    }
    assert!(SentimentAnalyzer::new("English", Some(Chop), "afinn").is_ok());
}
```

### `contributions`, `score` and `get_sentiment`: the lazy primitive and its folds

`contributions` is the primitive: it borrows the analyzer, consumes any
`IntoIterator<Item: AsRef<str>>`, and yields one `f64` addend per token —
`0.0` for a token that scored nothing, including negation words themselves —
without materialising anything. `score` and `get_sentiment` are folds over
it. Nothing here is a second implementation of the scoring loop; there is
exactly one, and it lives in `Contributions::next`.

| API | Answers | Lazy | Output | Allocates |
|---|---|:--:|---|---|
| `contributions(words)` | one addend per token, in order | ✅ | `Contributions<'_, S, I>` → `f64` | none — borrows the analyzer and the caller's iterator |
| `score(words)` | the running total and the token count, undivided | ❌ (folds the above) | `Score { sum, count }` | none |
| `get_sentiment(words)` | `score(words).value()` | ❌ | `f64` | none |
| `get_sentiment_over(words, len)` | `score(words).over(len)` — an explicit denominator | ❌ | `f64` | none |

None of the four allocates: `score` is a two-variable fold (`sum: f64`,
`count: usize`) over `contributions`, and `get_sentiment`/`get_sentiment_over`
are one division on top of that.

```text
I have tokens and a built SentimentAnalyzer
│
├── I want the final score, denominator = token count
│      └── get_sentiment(words)
│
├── I want the final score, but the denominator is something else
│      (a sparse-array hole count — see get_sentiment vs get_sentiment_over)
│      └── get_sentiment_over(words, len)
│
├── I need the sum AND the count separately
│      (combining several segments before dividing once)
│      └── score(words)  →  Score { sum, count }
│
└── I want to inspect, filter or short-circuit per-token
       (find the first nonzero contribution, log per-token deltas, …)
       └── contributions(words)   → lazy, one f64 per token
```

```rust
use verbora_sentiment::SentimentAnalyzer;

fn main() {
    let a = SentimentAnalyzer::without_stemmer("English", "afinn").unwrap();

    // The primitive: one addend per token, negation words included as 0.0.
    let deltas: Vec<f64> = a.contributions(["it", "is", "not", "good"]).collect();
    assert_eq!(deltas, [0.0, 0.0, 0.0, -3.0]);

    // get_sentiment is contributions folded and divided by the token count.
    assert_eq!(a.get_sentiment(["it", "is", "not", "good"]), -0.75);
}
```

<div class="perf">
<div class="perf-row"><span class="perf-k">Execution</span><span class="perf-v">Lazy — one hash lookup per token, on demand</span></div>
<div class="perf-row"><span class="perf-k">Output</span><span class="perf-v"><code>Contributions&lt;'_, S, I&gt;</code> → <code>f64</code></span></div>
<div class="perf-row"><span class="perf-k">Allocations</span><span class="perf-v">None of its own; one owned <code>String</code> only for a token whose case-folding actually changes it (see <a href="#allocation-behaviour">Allocation behaviour</a>)</span></div>
<div class="perf-row"><span class="perf-k">Buffer reuse</span><span class="perf-v">N/A — no caller-supplied buffer</span></div>
<div class="perf-row"><span class="perf-k">Best for</span><span class="perf-v">Piping a tokenizer straight in, or any early exit / per-token inspection</span></div>
</div>

### Scoring many documents: `par_get_sentiment_batch`

`get_sentiment` is already cheap per call — the token-shape numbers below
run tens of millions of tokens a second — but a caller scoring a large,
independent corpus (one score per review, per ticket, per comment) still pays
that cost once per document, serially, on one core. Nothing about the
analyzer needs to change to fan that out: it borrows its vocabulary and
negation list read-only, so `SentimentAnalyzer::par_get_sentiment_batch`,
behind this crate's `parallel` Cargo feature, is exactly
`docs.par_iter().map(|doc| self.get_sentiment(doc)).collect()` — a thin
wrapper over the same [`get_sentiment`](#contributions-score-and-get-sentiment-the-lazy-primitive-and-its-folds),
not a second scoring loop to keep in sync.

<div class="perf">
<div class="perf-row"><span class="perf-k">Execution</span><span class="perf-v">Eager — one <code>get_sentiment</code> call per document, fanned out over Rayon's global thread pool</span></div>
<div class="perf-row"><span class="perf-k">Output</span><span class="perf-v">Owned <code>Vec&lt;f64&gt;</code>, input order preserved</span></div>
<div class="perf-row"><span class="perf-k">Allocations</span><span class="perf-v">One <code>Vec&lt;f64&gt;</code> sized to <code>docs.len()</code>; each document scored with exactly <code>get_sentiment</code>'s own allocation behaviour</span></div>
<div class="perf-row"><span class="perf-k">Buffer reuse</span><span class="perf-v">N/A</span></div>
<div class="perf-row"><span class="perf-k">Parallel</span><span class="perf-v">Yes — behind the <code>parallel</code> Cargo feature; requires <code>S: Sync</code> (always true for <code>NoStemmer</code>, and for every <code>verbora-stemmers</code> type)</span></div>
<div class="perf-row"><span class="perf-k">Best for</span><span class="perf-v">Hundreds of documents, or documents in the thousands-of-tokens range</span></div>
</div>

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

Per-document cost is small enough that Rayon's own fork-join overhead
dominates for short documents or small batches — this crate's own
`par_batch` Criterion group (`benches/sentiment.rs`) compares the sequential
loop against this method at several `(document count, tokens per document)`
combinations. As a rule of thumb, read directly from that group's own doc
comment: a handful of documents, or documents under a few hundred tokens
each, and the sequential loop wins or ties — use it; hundreds of documents,
or documents in the thousands-of-tokens range, and this method wins, with the
win growing along both axes. If you are scoring one document, or you already
have your own thread pool and fan-out strategy, call `get_sentiment` directly
— this method would only add Rayon's scheduling on top for no benefit. See
[Parallelism](../performance/parallelism) for the same reasoning applied
workspace-wide, and its table for how this crate's own entry compares to the
other twelve.

### Choosing a vocabulary family and language

`VocabularyKind` has four members — `Afinn`, `AfinnFinancialMarketNews`,
`Senticon`, `Pattern` — and the reference's `languageFiles` table pairs them
with languages unevenly. `supported_pairs()` returns all fourteen pairs in
table order; here is the same table with entry counts, from
`crates/verbora-sentiment/src/data/mod.rs`:

| Language | Afinn | AfinnFinancialMarketNews | Senticon | Pattern |
|---|---:|---:|---:|---:|
| Basque | — | — | 4,311 | — |
| Catalan | — | — | 7,270 | — |
| Dutch | — | — | — | 3,304 |
| English | 3,382 | 0 (empty, see divergences) | 24,839 | 1,528 |
| French | — | — | — | 5,113 |
| Galician | — | — | 4,885 | — |
| German | — | — | — | 3,465 |
| Italian | — | — | — | 3,065 |
| Portuguese | 1,644 | — | — | — |
| Spanish | 1,653 | — | 11,344 | — |

75,803 entries total, across 1,230,918 bytes of embedded data. A `—` cell is
not a smaller vocabulary, it is `Error::UnsupportedLanguage`: constructing
`("Dutch", "afinn")` fails, it does not silently fall back to an empty table
(that only happens through the prototype-chain corners — see
[Advanced usage](#advanced-usage)).

Each vocabulary is also paired with a negation list, and five of the ten
languages have none at all:

| Language | Negation words | Kinds paired with it |
|---|---|---|
| English | `not`, `no`, `never`, `neither` | Afinn, AfinnFinancialMarketNews, Senticon, Pattern |
| Spanish | `no`, `nunca`, `jamás`, `ni` | Afinn, Senticon |
| Portuguese | `não`, `nunca`, `jamais`, `nem` | Afinn |
| Dutch | `niet`, `nooit`, `niemand`, `niets`, `nee`, `neen` | Pattern |
| German | `kein`, `nein`, `nicht` | Pattern |
| Galician, Catalan, Basque, Italian, French | *(none)* | Senticon (gl/ca/eu), Pattern (it/fr) |

For a language with no negation list, `is_negation` can never be true, so
every token in that language is scored on the vocabulary lookup alone.

Pick a member with [`VocabularyKind`] and a language with [`Language`] rather
than typing the reference's own strings if you would rather the compiler
catch a typo — both are a convenience only, not a gate: the constructor still
takes `&str` and unsupported combinations fail with the reference's own error
text either way.

```rust
use verbora_sentiment::{Language, SentimentAnalyzer, VocabularyKind, supported_pairs};

fn main() {
    assert_eq!(supported_pairs().len(), 14);
    assert_eq!(
        VocabularyKind::Senticon.languages(),
        ["Spanish", "English", "Galician", "Catalan", "Basque"]
    );

    // VocabularyKind and Language are a typed convenience over the reference's
    // own strings — the constructor still takes &str either way.
    let analyzer =
        SentimentAnalyzer::without_stemmer(Language::English.as_str(), VocabularyKind::Afinn.as_str())
            .unwrap();
    assert_eq!(analyzer.get_sentiment(["good"]), 3.0);
}
```

### `get_sentiment` vs `get_sentiment_over`

`getSentiment(words)` in the reference divides by `words.length`, which for a
**sparse** array is not the number of elements `forEach` actually visited —
`['good', <2 empty slots>, 'bad']` has `length` 4, and `forEach` skips the two
holes, so the reference scores `(3 + -3) / 4`, not `/ 2`. A Rust iterator has
no concept of a hole: every item you hand `contributions` gets visited. So
where the reference's denominator and its visited-item count can diverge,
this crate makes the denominator an explicit parameter instead of trying to
manufacture the concept of a "hole" that doesn't exist here:

```rust
use verbora_sentiment::SentimentAnalyzer;

fn main() {
    let a = SentimentAnalyzer::without_stemmer("English", "afinn").unwrap();

    // get_sentiment(["good"]): one token, divide by 1.
    assert_eq!(a.get_sentiment(["good"]), 3.0);

    // get_sentiment_over(["good"], 2): still one token scored, but the
    // caller supplies a denominator of 2 — reproducing what a sparse array
    // with one extra hole would have divided by.
    assert_eq!(a.get_sentiment_over(["good"], 2), 1.5);
}
```

Reach for `get_sentiment_over` only when you are deliberately reproducing a
sparse-array call site from a reference caller; for ordinary Rust
collections — where every element you hand in is visited — `get_sentiment`'s
implicit token-count denominator is what you want.

## Three things a naive port gets wrong

The module documentation names these as the three details a careful reading
of the reference still gets wrong. Each has its own fixture coverage.

### Sticky negation: there is no window

`negator` is set to `-1.0` by the first negation word and is **never
restored** — not after one token, not after punctuation, not at a sentence
boundary. The two "obvious" ports — negate only the next word, or reset the
negator after each hit — both disagree with the reference on a third token:

```rust
use verbora_sentiment::SentimentAnalyzer;

fn main() {
    let a = SentimentAnalyzer::without_stemmer("English", "afinn").unwrap();
    // Both "good"s are worth 3.0 unnegated; the first "not" flips both.
    assert_eq!(a.get_sentiment(["not", "good", "good"]), -2.0);
    // A negation AFTER the positive word does not retroactively apply to it.
    assert_eq!(a.get_sentiment(["good", "not", "good"]), 0.0);
    // Punctuation between "not" and "good" does not reset the negator.
    assert_eq!(a.get_sentiment(["not", ".", "good"]), -1.0);
}
```

The negation test also runs **before** the vocabulary lookup, so a word that
is both an AFINN entry and a negation word scores nothing at all: English
`no` is worth `-1` in AFINN and is also an English negation word, so
`get_sentiment(["no"])` is `0.0`, not `-1.0` — see
[Common mistakes](#common-mistakes).

### Stem-collision resolution is last-wins, in file order

Supplying a stemmer rebuilds the whole vocabulary by iterating its entries in
source-file order and letting a later stem overwrite an earlier one with the
same key. For English AFINN with the Porter stemmer, 1,415 of 3,382 keys
collide down to 1,967 distinct stems, and 109 of those collisions change the
*stored polarity* — `affect` (−1) loses to `affection` (3); `arrest` (−2)
wins over `arrested` (−3):

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
    assert_eq!(base.polarity("arrested"), Some(-3.0));
    assert_eq!(base.polarity("arrests"), Some(-2.0));
    assert_eq!(stemmed.polarity("arrest"), Some(-2.0)); // arrests's value wins
}
```

An ordinary `HashMap` or `BTreeMap` built by inserting these same 3,382 pairs
would pick a *different* winner for all 109 value-changing collisions — a
`HashMap`'s iteration order is unrelated to insertion order, and a
`BTreeMap`'s is alphabetical, neither of which is "the order the source file
listed them in." [`Vocabulary`] avoids both failure modes by storing entries
in a `Vec` in insertion order and using its hash map only to map a key to a
slot in that `Vec`; re-assigning an existing key keeps its original slot and
only replaces the value, exactly as the reference object-property assignment
does.

### Left-to-right summation is bit-exact-observable

`score` accumulates in `f64`, strictly left to right, and divides exactly
once at the end. The reference's own golden values are compared for exact
bit equality in the test suite, and reordering the sum, dividing per term,
or accumulating in `f32` all change the last bits of the result:

```rust
use verbora_sentiment::SentimentAnalyzer;

fn main() {
    let a = SentimentAnalyzer::without_stemmer("English", "senticon").unwrap();
    let tokens = ["good", "bad", "excellent", "awful", "fine"];

    let by_explicit_fold: f64 = a.contributions(tokens).fold(0.0, |acc, d| acc + d);
    assert_eq!(a.score(tokens).sum.to_bits(), by_explicit_fold.to_bits());
}
```

`Contributions` yields a real `0.0` addend for every token that scored
nothing — including the negation words themselves — rather than skipping
them, specifically so that summing it left to right reproduces the
reference's `score += …` sequence term for term. Adding `+0.0` to a
running sum that starts at `+0.0` cannot perturb it, so the extra addends
are free and exact.

## Advanced usage

### Handling constructor errors

`without_stemmer`/`new` mirror the reference's own two-level lookup —
`languageFiles[type][language]` — and its two-level failure: an unsupported
`vocabulary_type` fails first, an unsupported `language` (for a real type)
fails second, and the message wording follows the reference's own,
including its "Type Language" phrasing:

```rust
use verbora_sentiment::SentimentAnalyzer;

fn main() {
    let e = SentimentAnalyzer::without_stemmer("English", "bogus").unwrap_err();
    assert_eq!(e.to_string(), "Type Language bogus not supported");

    let e = SentimentAnalyzer::without_stemmer("Klingon", "afinn").unwrap_err();
    assert_eq!(e.to_string(), "Type afinn for Language Klingon not supported");
}
```

Two rarer variants — `Error::ObjectPrototype` and `Error::RestrictedProperty`
— exist only because `languageFiles` is a plain the reference object literal, so
a `language` argument named after an `Object.prototype` member (`"toString"`,
`"constructor"`, `"__proto__"`, …) resolves on the prototype chain instead of
failing outright, and one of those paths reaches a dead
`Object.create(languageFiles[type][language][0])` that throws a `TypeError`.
Reproducing that corner is what a `HashMap`-based lookup could not do on its
own; see `Error`'s own documentation.

### Concurrency

`SentimentAnalyzer` is immutable after construction; nothing is cached or
mutated per call. It is `Send + Sync` whenever its stemmer type parameter is
(the default `NoStemmer` always is), so one built analyzer can be wrapped in
an `Arc` and shared read-only across threads with no locking — the same
pattern [Parallelism](../performance/parallelism) describes for `Trie` and
[WordNet](./wordnet#concurrency) uses for dictionary lookups:

```rust
use verbora_sentiment::SentimentAnalyzer;

fn main() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<SentimentAnalyzer>(); // SentimentAnalyzer<NoStemmer>
}
```

Build once, share the `Arc`, and score documents from as many threads as you
like — construction is the only expensive step, and it happens once. For a
batch of documents against one shared analyzer specifically, reach for
[`par_get_sentiment_batch`](#scoring-many-documents-par-get-sentiment-batch)
before writing your own `rayon` fan-out: it is exactly that fan-out, already
written, behind this crate's `parallel` Cargo feature.

## Deliberate divergences <span class="badge badge-fallible">FALLIBLE</span>

| | Reference | Here |
|---|---|---|
| `afinnFinancialMarketNews` | Imports `.afinnFinancialMarketNews` from a package that actually exports `.afinn165FinancialMarketNews` — the table is `{}` at runtime and every score is `0` | Reproduced as an explicit, documented empty table. Shipping the real data would be a bug *fix* that breaks behaviour, so it stays empty on purpose |
| `negations` mutability | The require-cache's own array: `a.negations.push('x')` mutates every other analyzer of that language, permanently, for the life of the process | `&'static [&'static str]`. The library itself never mutates the list, so the aliasing is unobservable from any public API — only a caller reaching into the field could tell, and there is no such field here |
| Non-string tokens | `getSentiment(null)`, `getSentiment('good')`, `getSentiment([5])` are runtime `TypeError`s | Cannot be written: tokens are `AsRef<str>`, so the mistake is a compile error instead of a runtime one |
| Sparse arrays | `words.length` can exceed the count `forEach` actually visited | No Rust equivalent; expressed as an explicit denominator via [`get_sentiment_over`](#get-sentiment-vs-get-sentiment-over) |
| Unstemmed vocabulary sharing | `Object.assign({}, vocabulary)` gives every analyzer its own mutable copy — the reference's own comment says this is needed "or in subsequent execution the polarity will be undefined" | An unstemmed analyzer borrows the process-wide decode instead of copying it. [`Vocabulary`] exposes no way to mutate a table through a `SentimentAnalyzer`, so the difference is unobservable — and it is what turns an 8 ms construction into a 19 ns one. A stemmed analyzer still owns its rebuild, as the reference does |

The last row is worth restating plainly, since it is the biggest single
performance decision in this crate: **only the stemmed path pays a
per-construction cost.** An unstemmed `SentimentAnalyzer` for a pair that has
already been constructed once anywhere in the process is, from then on, a
handful of pointer and string copies.

## Performance characteristics

`crates/verbora-sentiment/benches/sentiment.rs` is a Criterion suite covering
cold decode, construction (with and without a stemmer), scoring at several
document sizes, and the ASCII/non-ASCII token-shape cost of case-folding.
Measured on one machine with `cargo bench -p verbora-sentiment`, against the
reference implementation's own figures for the same operations:

| | Reference | this crate |
|---|---|---|
| import / first touch | 40 ms, all 7.5 MB, unconditionally | 0 — nothing is decoded until asked |
| first `("English", "afinn")` | 612 µs, and again every construction | 61 µs, once |
| first `("English", "senticon")` | 8.0 ms, and again every construction | 644 µs, once |
| every construction after the first | unchanged | **19 ns** |
| all fourteen tables | 40 ms + 8 ms each | 7.7 ms, once |
| `+ PorterStemmer`, senticon | 72 ms per construction | 23 ms per construction |
| scoring 2,816 English tokens | 55 µs | 30 µs |

### How the lexicons ship, and why

The reference eagerly `require`s ~7.5 MB of JSON at import time whether or not
any of it is used, then discards every field except one polarity per entry —
`wordnet_id`, `sense`, `subjectivity`, `intensity` and `confidence` are 84% of
those bytes and no code path reads them. Three options were on the table for
this crate, and the measurements above are the argument for the one it took:

1. **Embed the referenceON and parse it at startup.** Simplest to build, but pays the
   reference's own cost straight back — a `serde_json` parse of 7.5 MB is not
   free, and it would run on every process start whether or not sentiment
   analysis is ever used.
2. **Read the lexicons from disk at run time**, the way [WordNet](./wordnet)
   reads its (much larger, separately licensed) database. Rejected here: it
   would need a build or install step, would not work from a single static
   binary, and the payload is two orders of magnitude smaller than WordNet's —
   small enough that shipping it *in* the binary has none of the downsides
   that make WordNet's on-disk database the right call for 28 MB of licensed
   data.
3. **Embed a prebuilt index of only what the analyzer reads.** This is what
   shipped: each source JSON file was machine-projected
   down to the one field every code path actually uses — `key \0 polarity \0`,
   repeated — and dumps it as a binary blob. What survives the projection is
   75,803 `(key, polarity)` pairs, 1.2 MB across thirteen `include_bytes!`
   blobs (the fourteenth, `afinnFinancialMarketNews`, is deliberately empty —
   see [Deliberate divergences](#deliberate-divergences-fallible)).

`crates/verbora-sentiment/Cargo.toml` explains the two dependencies this
decision rules out, in the comment above where each would otherwise go:

> Deliberately no `serde_json`: the lexicons ship as `key \0 polarity \0`
> blobs machine-dumped from the reference tables, which is 1.2 MB instead of
> the reference's 7.5 MB of JSON and needs no parser at all.
>
> Deliberately no `verbora-core`: nothing here splits or trims on `\s`, so
> `is_whitespace` has no call site. The only the reference string semantics in
> play is `toLowerCase`, which `str::to_lowercase` already matches exactly.

Each blob is decoded on first use — one `str::split_terminator('\0')` pass,
no JSON parser — and cached for the process behind a `OnceLock` per
vocabulary, which is what makes every construction after the first a pointer
copy (see [With or without a stemmer](#with-or-without-a-stemmer)).

<div class="callout callout-note">
Scoring throughput was also measured by token shape: roughly <strong>94 M
tokens/s</strong> for lowercase ASCII (the allocation-free path), <strong>61
M/s</strong> for uppercase ASCII (one <code>to_ascii_lowercase</code> per
token), and <strong>41 M/s</strong> for non-ASCII (the full Unicode
<code>to_lowercase</code>, which correctness requires). See
<a href="#unicode-and-language-notes">Unicode and language notes</a> for why
that last figure needed real work to get there, and
<a href="../benchmarks/index">Benchmarks</a> for how to reproduce all of the
above yourself.
</div>

## Allocation behaviour

**At load.** `Vocabulary::load` decodes one embedded blob into a `Vec<Entry>`
plus an `FxHashMap<Cow<'static, str>, u32>` sized to the source's entry count.
Every key is a `Cow::Borrowed` slice of the embedded `'static` blob — no
string data is copied for an unstemmed table — so the allocation is exactly
one `Vec` and one hash map's backing storage. This happens at most once per
process per vocabulary, behind a `OnceLock`.

**Constructing without a stemmer.** `Table::Shared(&'static Vocabulary)` is a
pointer copy; the only allocation anywhere in `SentimentAnalyzer::new` on this
path is the two owned `String`s the struct always stores for `language()` and
`vocabulary_type()` (the reference stores these verbatim too, and they are
never validated).

**Constructing with a stemmer.** `Vocabulary::stemmed` allocates a fresh
`Vec<Entry>` and `FxHashMap`, both pre-sized to the source table's length.
Each key becomes `stemmer.stem(key)`: `Cow::Borrowed` (no allocation) for a
word the algorithm left unchanged, `Cow::Owned` for one it rewrote. Values are
never touched by stemming — `raw: &'static str` still points into the
original embedded blob even after the *key* it is filed under has moved — so
a stemmed senticon table still answers with borrowed string polarities and no
value ever needs a fresh allocation.

**Scoring.** `contributions`/`score`/`get_sentiment`/`get_sentiment_over`
allocate nothing of their own: `contributions` borrows the analyzer and the
caller's iterator, and its private lowercasing helper returns
`Cow::Borrowed` for any token that is already-lowercase ASCII or
already-lowercase non-ASCII (checked by a byte or code-point scan before
committing to an allocation), falling back to an owned `String` only for a
token that genuinely needs case-folding. `Vocabulary::polarity` is one hash
lookup and one `Vec` index — no allocation regardless of a hit or a miss.
`score` is a two-`f64`/`usize` fold with no heap traffic at all.

There is no `_into` variant and no caller-supplied output buffer anywhere in
this crate — there is nothing to reuse a buffer for, since nothing
collects a document into an owned collection in the first place. See
[Allocation](../performance/allocation) and [Zero-copy](../performance/zero-copy).

## Unicode and language notes

- **Lowercasing is the full Unicode algorithm**, not `to_ascii_lowercase`:
  `İ` expands to two code units (`i̇`), and Greek Final_Sigma is honoured —
  `"ΑΣ"` lowercases to `"ας"`, not `"ασ"`. This matters here more than it
  might elsewhere: ten of the fourteen shipped `(kind, language)` vocabularies are non-English,
  so most tokens this crate ever scores are accented Latin, Greek or
  Cyrillic — text that is *already* lowercase, where the crate's job is to
  avoid paying for `to_lowercase` at all rather than to get it right, since
  correctness was never in question.
- **Vocabulary lookup is exact — no accent folding, no normalisation beyond
  case.** `naïve` really is an AFINN-165 key, accent and all; `café`,
  `Ångström` and `crème` are not, and score `0.0`. Capitalised entries
  (German pattern has 1,234 of them) and multi-word entries (English senticon
  has 5,960 entries containing a space) are unreachable from a single
  lowercased token, by construction — a stemmer does not fix this, since
  stemming operates on one already-tokenized word.
- **Spanish and Portuguese AFINN key on emoji directly.** `😂` is a real
  entry (`1.0`); CJK text and other astral-plane characters that are not
  vocabulary entries score `0.0` without erroring, the same as any other
  miss.
- **`toLowerCase` itself is tested independently of scoring**: the
  fixture replays it over representative code points so a scoring
  discrepancy can be attributed to the vocabulary or to case-folding without
  a bisection.

## Common mistakes

**Constructing a stemmed analyzer inside a request handler or a per-document
loop.** Every construction with a stemmer pays the full rebuild cost again —
23 ms for English senticon, measured — because a stemmer is caller-supplied
and nothing about its output can be cached across constructions. This is the
one cost in the whole crate that reuse of the internal vocabulary cache
cannot help with; only reusing the `SentimentAnalyzer` itself can:

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

**Expecting `afinnFinancialMarketNews` to have real data.** It is empty by
design, reproducing an upstream package-naming mismatch in the reference — see
[Deliberate divergences](#deliberate-divergences-fallible). Every score against it is
`0.0`, for any input:

```rust
use verbora_sentiment::SentimentAnalyzer;

fn main() {
    let a = SentimentAnalyzer::without_stemmer("English", "afinnFinancialMarketNews").unwrap();
    assert_eq!(a.vocabulary().len(), 0);
    assert_eq!(a.get_sentiment(["bankruptcy", "raises"]), 0.0);
}
```

**Assuming scores are clamped to `[-1, 1]`, the way some other sentiment
tools normalize their output.** Nothing in the algorithm clamps or
normalizes anything: `get_sentiment` is the sum of per-token contributions
divided by the token count, and because it is an *average*, it happens to
stay within the extremes of the vocabulary you chose — `-5..=5` for AFINN's
integers, `-1.0..=1.0` for senticon and pattern's decimals as shipped — not
within any fixed universal range. Do not assume `±1` for a vocabulary you
have not checked. And `score()`'s raw `Score.sum` — the un-averaged total —
has no such bound at all: it grows with document length, not with any
per-token limit:

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

**Not accounting for a word that is both a vocabulary entry and a negation
word.** The negation check runs before the vocabulary lookup, so such a word
always scores `0.0`, never its lexicon value. English `no` is a real AFINN
entry worth `-1.0` and also an English negation word:

```rust
use verbora_sentiment::SentimentAnalyzer;

fn main() {
    let a = SentimentAnalyzer::without_stemmer("English", "afinn").unwrap();
    assert_eq!(a.vocabulary().polarity("no"), Some(-1.0));
    assert_eq!(a.get_sentiment(["no"]), 0.0); // not -1.0
    assert_eq!(a.get_sentiment(["no", "good"]), -1.5); // "no" negates "good"
}
```

**Confusing the constructor's argument order.** The signature is `(language,
stemmer, vocabulary_type)`, mirroring the reference's `new
SentimentAnalyzer(language, stemmer, type)` — not `(type, language, …)`.
Getting it backwards does not panic; it produces a confusingly-worded but
entirely valid `Error`, because both arguments are plain strings the type
system cannot tell apart:

```rust
use verbora_sentiment::SentimentAnalyzer;

fn main() {
    // "afinn" in the language slot, "English" in the type slot:
    let e = SentimentAnalyzer::without_stemmer("afinn", "English").unwrap_err();
    // The error correctly reports "English" as the unsupported *type* — it
    // has no way to know the caller meant it as a language.
    assert_eq!(e.to_string(), "Type Language English not supported");
}
```

## Related

- [Choosing an API](../choosing/index) and
  [Decision trees](../choosing/decision-trees) — the cross-crate version of
  the trees on this page.
- [Iterator vs. `_into`](../performance/iterator-vs-into) — the lazy/fold
  distinction behind `contributions` vs `score`/`get_sentiment`.
- [Allocation](../performance/allocation) and
  [Zero-copy](../performance/zero-copy) — what "borrowed" means for a shared
  `Vocabulary` and its `Cow`-based lowercasing.
- [Parallelism](../performance/parallelism) — the `Arc`-wrapped, read-only
  sharing pattern this page reuses from `Trie` and `WordNet`.
- [Benchmarks](../benchmarks/index) — how to reproduce the measured numbers
  on this page.
- [Tokenizers](./tokenizers) and [Phonetics](./phonetics) — the tokenizer
  half of the `contributions(tokenizer.tokens(text))` pipeline, and the
  precedent for composing a lazy token stream with a per-token transform.
- [WordNet](./wordnet) — the other feature page with a separately-licensed,
  third-party data section; contrast its at-runtime, opt-in database against
  this crate's embedded-at-compile-time lexicons.
- [Recipes](../recipes/index) — end-to-end pipelines.

## API reference

Everything the crate exports:

```rust ignore
// verbora_sentiment
pub enum VocabularyKind { Afinn, AfinnFinancialMarketNews, Senticon, Pattern }
impl VocabularyKind {
    pub const ALL: [Self; 4];
    pub const fn as_str(self) -> &'static str;       // "afinn" | "afinnFinancialMarketNews" | "senticon" | "pattern"
    pub fn languages(self) -> Vec<&'static str>;
    pub fn from_js(s: &str) -> Option<Self>;
}
impl std::fmt::Display for VocabularyKind { /* as_str() */ }

pub enum Language { English, Spanish, Portuguese, Galician, Catalan, Basque, Dutch, Italian, French, German }
impl Language {
    pub const ALL: [Self; 10];                       // alphabetical
    pub const fn as_str(self) -> &'static str;
    pub fn from_js(s: &str) -> Option<Self>;
}
impl std::fmt::Display for Language { /* as_str() */ }

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
    pub fn value(self) -> f64;       // sum / count   (NaN if count == 0)
    pub fn over(self, len: usize) -> f64; // sum / len
}

pub enum Error {
    UnsupportedType { vocabulary_type: String },
    UnsupportedLanguage { vocabulary_type: String, language: String },
    ObjectPrototype { value: char },
    RestrictedProperty,
}
impl Error {
    pub fn error_name(&self) -> &'static str; // "Error" | "TypeError"
}
impl std::fmt::Display for Error { /* reference's own message text */ }
impl std::error::Error for Error {}

pub trait Stemmer {
    fn stem<'a>(&self, word: &'a str) -> std::borrow::Cow<'a, str>;
}
impl<T: Stemmer + ?Sized> Stemmer for &T {}
impl<T: Stemmer + ?Sized> Stemmer for Box<T> {}
impl<T: Stemmer + ?Sized> Stemmer for std::sync::Arc<T> {}
// Every verbora_stemmers type implements Stemmer with no adapter:
// CarryStemmerFr, LancasterStemmer, PorterStemmer, PorterStemmerDe,
// PorterStemmerEs, PorterStemmerFa, PorterStemmerFr, PorterStemmerIt,
// PorterStemmerNl, PorterStemmerNo, PorterStemmerPt, PorterStemmerRu,
// PorterStemmerSv, PorterStemmerUk, StemmerJa.

pub struct NoStemmer;
impl Stemmer for NoStemmer { /* identity: always Cow::Borrowed */ }

pub enum Polarity { Number(f64), Text(&'static str) }
impl Polarity {
    pub fn as_f64(self) -> f64;
    pub fn as_str(self) -> Option<&'static str>; // Some(_) only for Text
}
impl std::fmt::Display for Polarity { /* n or s, verbatim */ }

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
`SentimentAnalyzer<S>` and `Vocabulary` are `Send + Sync` whenever `S` is;
nothing here depends on what was scored before.
`SentimentAnalyzer::par_get_sentiment_batch` is the crate's only parallel
entry point, gated behind the `parallel` Cargo feature and off by default —
see [Scoring many documents](#scoring-many-documents-par-get-sentiment-batch)
above and [Parallelism](../performance/parallelism).
