# Sentiment

`verbora-sentiment` turns a token stream into one polarity score, over fourteen
word-list vocabularies in ten languages drawn from three lexicon projects:
AFINN, ML-SentiCon and the CLiPS Pattern project. `SentimentAnalyzer` does the
whole job with one lazy iterator — no model, no training, and a bounded,
reused lookahead as the only thing it allocates while scoring.

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

- **You need a trained or context-aware model.** This is a lexicon lookup with
  sign flipping on negation — no aspect extraction, no sarcasm handling, and no
  window around negation (see [Sticky negation](#sticky-negation)).
- **You need `AfinnFinancialMarketNews` to have real data.** That vocabulary
  ships empty; every score against it is `Some(0.0)`.
- **You want scores clamped to a fixed range like `[-1, 1]`.** They are not.
- **You cannot accept the lexicons' licensing caveats** for a commercial
  product.

## Quick example

```rust
use verbora_sentiment::{Language, SentimentAnalyzer, VocabularyKind};

fn main() {
    let analyzer = SentimentAnalyzer::without_stemmer(Language::English, VocabularyKind::Afinn).unwrap();
    assert_eq!(analyzer.get_sentiment(["good"]), Some(3.0));
    // Sticky negation: the second "happy" is still negated.
    assert_eq!(analyzer.get_sentiment(["not", "happy"]), Some(-1.5));
    assert_eq!(analyzer.get_sentiment(["not", "happy", "happy"]), Some(-2.0));
    // …but a phrase the lexicon itself publishes wins over the heuristic:
    // AFINN-165 scores "not good" at -2, as one unit.
    assert_eq!(analyzer.get_sentiment(["not", "good"]), Some(-2.0));
    // Nothing to score is None, not 0.0.
    let empty: [&str; 0] = [];
    assert_eq!(analyzer.get_sentiment(empty), None);
}
```

It composes directly with a tokenizer's lazy `tokens()` — no document is ever
collected into a `Vec`:

```rust
use verbora_sentiment::{Language, SentimentAnalyzer, VocabularyKind};
use verbora_tokenizers::{BorrowingTokenizer, WordTokenizer};

fn main() {
    let analyzer = SentimentAnalyzer::without_stemmer(Language::English, VocabularyKind::Afinn).unwrap();
    let tokens = WordTokenizer.tokens("This is not a good day.");
    // Six units; "not" flips "good", so the total is -3 over 6.
    assert_eq!(analyzer.get_sentiment(tokens), Some(-0.5));
}
```

## The unit, and why it is not the token

A lexicon key is *text*, not a token. The shipped tables spell entries
`cover-up`, `bad luck`, `son-of-a-bitch`, `Abfall` — and a
[UAX #29](../features/tokenizers.md) token stream contains none of those.

So keys and tokens are both reduced to a **lookup form** — word pieces,
lowercased, joined by one space — before they meet, and the scoring loop matches
the *longest span of tokens* that forms a key, counting that span as **one
unit**. The unit, not the token, is what the sum is divided by.

```rust
use verbora_sentiment::{Language, SentimentAnalyzer, VocabularyKind};
use verbora_tokenizers::{BorrowingTokenizer, WordTokenizer};

fn main() {
    let a = SentimentAnalyzer::without_stemmer(Language::English, VocabularyKind::Afinn).unwrap();

    // Four tokens, one unit: `son-of-a-bitch` is a shipped key worth -5.
    let deltas: Vec<f64> = a
        .contributions(WordTokenizer.tokens("son-of-a-bitch"))
        .collect();
    assert_eq!(deltas, [-5.0]);
    assert_eq!(a.get_sentiment(WordTokenizer.tokens("son-of-a-bitch")), Some(-5.0));

    // Spelling the same key with spaces reaches the same entry.
    assert_eq!(a.get_sentiment(WordTokenizer.tokens("son of a bitch")), Some(-5.0));

    // Longest match wins: `good` is a key too, but the span is taken first, so
    // this is two units, not three.
    let s = a.score(WordTokenizer.tokens("cover-up good"));
    assert_eq!((s.count, s.sum), (2, 0.0));
}
```

Without span matching a phrase divides its own polarity by its own length, and
several entries invert outright: `non-approved` (-2) would score +1 as `non` +
`approved`, and `son-of-a-bitch` (-5) would score -1.25 as four tokens.

## Choosing the right API

### The four scoring entry points

`contributions` is the primitive: it borrows the analyzer, consumes any
`IntoIterator<Item: AsRef<str>>`, and yields one `f64` addend per **unit** —
`0.0` for a unit that scored nothing, including negation words themselves. The
other three are folds over it. There is exactly one scoring loop in the crate,
and it lives in `Contributions::next`.

| API | Answers | Lazy | Output |
|---|---|:--:|---|
| `contributions(words)` | one addend per unit, in order | ✅ | `Contributions<'_, S, I>` → `f64` |
| `score(words)` | the running total and the unit count, undivided | ❌ | `Score { sum, count }` |
| `get_sentiment(words)` | `score(words).mean()` — divided by units scored | ❌ | `Option<f64>` |
| `get_sentiment_over(words, len)` | `score(words).over(len)` — explicit denominator | ❌ | `Option<f64>` |

Pick by what you need:

| You want | Call |
|---|---|
| The final score, denominator = unit count | `get_sentiment(words)` |
| The final score with a denominator of your own | `get_sentiment_over(words, len)` |
| The sum and the count separately (combine segments, divide once) | `score(words)` |
| To inspect, filter or short-circuit per unit | `contributions(words)` |

```rust
use verbora_sentiment::{Language, SentimentAnalyzer, VocabularyKind};

fn main() {
    let a = SentimentAnalyzer::without_stemmer(Language::English, VocabularyKind::Afinn).unwrap();

    // The primitive: one addend per unit, negation words included as 0.0.
    let deltas: Vec<f64> = a.contributions(["it", "is", "not", "happy"]).collect();
    assert_eq!(deltas, [0.0, 0.0, 0.0, -3.0]);

    // get_sentiment is that, folded and divided by the unit count.
    assert_eq!(a.get_sentiment(["it", "is", "not", "happy"]), Some(-0.75));

    // get_sentiment_over supplies the denominator instead.
    assert_eq!(a.get_sentiment_over(["good"], 2), Some(1.5));
    // A denominator of zero has no mean either.
    assert_eq!(a.get_sentiment_over(["good"], 0), None);
}
```

`get_sentiment` on an empty input is `None`, not `0.0`. The mean of no text is
not zero — returning `0.0` would make "nothing to say" indistinguishable from
"perfectly neutral" — and it is not `NaN`, which compares false against
everything including itself, so it sorts unpredictably and poisons any average
taken over a batch of scores. Absence is `None`, as it is everywhere else in
Verbora. `get_sentiment_over` answers `None` for a denominator of zero for the
same reason.

### With or without a stemmer

`without_stemmer(language, kind)` builds `SentimentAnalyzer<NoStemmer>`;
`with_stemmer(language, kind, stemmer)` builds `SentimentAnalyzer<S>` for any
`S: Stemmer`. Both arguments are enums, so a misspelling cannot reach the
constructor at all. A stemmer changes what the vocabulary *contains*: the whole table
is rebuilt with every key stemmed **piece by piece**, so a token that only
matches after stemming (`"goods"` finding `"good"`) becomes reachable, and a
phrase key stays reachable because each of its pieces is stemmed separately
rather than the key being handed to the stemmer whole.

A stemmer is **not** how capitalised keys are reached — those are reachable
without one, because every key is indexed by its lowercased lookup form. See
[Capitalised and phrase keys](#capitalised-and-phrase-keys).

That rebuild is the entire cost difference, and it is structural rather than a
tuning detail. Building without a stemmer borrows the process-wide decode of the
vocabulary, so every construction after the first collapses to a pointer copy.
Building *with* one cannot: a stemmer is an arbitrary caller-supplied function,
so nothing about its output can be cached across constructions, and every
construction pays exactly one `Stemmer::stem` call per piece of every key —
3,443 calls over English AFINN's 3,382 entries, 33,874 over English senticon's
24,839, counts the crate's own suite re-derives rather than quotes. **Build a
stemmed analyzer once and reuse it.**

How much wall-clock that costs is **unmeasured** — see
[Performance characteristics](#performance-characteristics) for why, and for
what else this crate does and does not currently publish a number for.

Any `verbora-stemmers` type works with no adapter, and so does your own:

```rust
use verbora_sentiment::{Language, SentimentAnalyzer, Stemmer, VocabularyKind};
use verbora_stemmers::PorterStemmer;

fn main() {
    let analyzer = SentimentAnalyzer::with_stemmer(
        Language::English,
        VocabularyKind::Afinn,
        PorterStemmer::new(),
    )
    .unwrap();
    assert_eq!(analyzer.get_sentiment(["not", "happy"]), Some(-1.5));
    // `goods` reaches `good` only because the table was rebuilt through the
    // stemmer; without one it scores nothing.
    assert_eq!(analyzer.get_sentiment(["goods"]), Some(3.0));

    let bare =
        SentimentAnalyzer::without_stemmer(Language::English, VocabularyKind::Afinn).unwrap();
    assert_eq!(bare.get_sentiment(["goods"]), Some(0.0));

    struct Chop;
    impl Stemmer for Chop {
        fn stem<'a>(&self, word: &'a str) -> std::borrow::Cow<'a, str> {
            word.get(..4).unwrap_or(word).into()
        }
    }
    assert!(
        SentimentAnalyzer::with_stemmer(Language::English, VocabularyKind::Afinn, Chop).is_ok()
    );
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

75,803 entries total. A `—` cell is not a smaller vocabulary — it is an
`UnsupportedPair` error: `(Language::Dutch, VocabularyKind::Afinn)` fails rather
than silently falling back to an empty table.

Each vocabulary is paired with a negation list, and five languages have none:

| Language | Negation words |
|---|---|
| English | `not`, `no`, `never`, `neither` |
| Spanish | `no`, `nunca`, `jamás`, `ni` |
| Portuguese | `não`, `nunca`, `jamais`, `nem` |
| Dutch | `niet`, `nooit`, `niemand`, `niets`, `nee`, `neen` |
| German | `kein`, `nein`, `nicht` |
| Galician, Catalan, Basque, Italian, French | *(none)* |

Where there is no negation list, no token is ever treated as a negator and every
unit is scored on the vocabulary lookup alone.

```rust
use verbora_sentiment::{Language, SentimentAnalyzer, VocabularyKind, supported_pairs};

fn main() {
    assert_eq!(supported_pairs().len(), 14);
    assert_eq!(
        VocabularyKind::Senticon.languages(),
        [
            Language::Spanish,
            Language::English,
            Language::Galician,
            Language::Catalan,
            Language::Basque,
        ]
    );

    // Each spells itself with its published identifier: an ISO 639-1 code for a
    // language, the upstream project's own name for a family.
    assert_eq!(Language::English.code(), "en");
    assert_eq!(VocabularyKind::Afinn.name(), "afinn");

    // …and parses back from one, for callers reading a config file.
    assert_eq!(Language::from_code("pt"), Some(Language::Portuguese));
    assert_eq!(VocabularyKind::from_name("senticon"), Some(VocabularyKind::Senticon));
    assert_eq!(Language::from_code("xx"), None);

    let analyzer =
        SentimentAnalyzer::without_stemmer(Language::English, VocabularyKind::Afinn).unwrap();
    assert_eq!(analyzer.get_sentiment(["good"]), Some(3.0));
}
```

### Scoring many documents

`par_get_sentiment_batch`, behind the `parallel` Cargo feature, is exactly
`docs.par_iter().map(|d| self.get_sentiment(d)).collect()` — the same
`get_sentiment`, fanned out, not a second scoring loop.

```rust  ignore
// Needs the `parallel` feature, which this site's snippet checker builds
// without — so this block is marked `ignore` rather than compiled.
use verbora_sentiment::{Language, SentimentAnalyzer, VocabularyKind};

fn main() {
    let analyzer = SentimentAnalyzer::without_stemmer(Language::English, VocabularyKind::Afinn).unwrap();
    let docs = vec![vec!["good"], vec!["not", "happy"], vec![]];
    let scores = analyzer.par_get_sentiment_batch(&docs);
    assert_eq!(scores[0], Some(3.0));
    assert_eq!(scores[1], Some(-1.5));
    assert_eq!(scores[2], None); // the empty document has no mean
}
```

**Where it starts paying is unmeasured.** The shape of the trade is known —
this method adds Rayon's fork-join scheduling on top of what is otherwise a
tight per-document loop, so below some batch size and some document length the
sequential loop wins — but *where* that boundary sits has no current
measurement, and no rule of thumb stands in for one. If you are scoring a single
document, or you already have your own thread pool and fan-out strategy, call
`get_sentiment` directly. The `par_batch` group in
`crates/verbora-sentiment/benches/sentiment.rs` is what the next run answers
this with.

Requires `S: Sync`, always true for `NoStemmer` and every `verbora-stemmers`
type. See [Parallelism](../performance/parallelism).

## Behaviour worth knowing

### Sticky negation

The negator is set to `-1.0` by the first negation word and is **never
restored** — not after one unit, not after punctuation, not at a sentence
boundary:

```rust
use verbora_sentiment::{Language, SentimentAnalyzer, VocabularyKind};

fn main() {
    let a = SentimentAnalyzer::without_stemmer(Language::English, VocabularyKind::Afinn).unwrap();
    // Both "happy"s are worth 3.0 unnegated; the first "not" flips both.
    assert_eq!(a.get_sentiment(["not", "happy", "happy"]), Some(-2.0));
    // A negation AFTER the positive word does not apply retroactively.
    assert_eq!(a.get_sentiment(["happy", "not", "happy"]), Some(0.0));
    // Punctuation between "not" and "happy" does not reset the negator.
    assert_eq!(a.get_sentiment(["not", ".", "happy"]), Some(-1.0));
}
```

The negation test runs **before** the single-token vocabulary lookup, so a word
that is both a lexicon entry and a negation word scores nothing at all — English
`no` is worth `-1` in AFINN and is also a negation word, so
`get_sentiment(["no"])` is `0.0`.

### The lexicon outranks the heuristic

The span scan runs *first*, so a phrase the lexicon actually publishes wins over
the sign-flipping guess. AFINN-165 scores `not good` at -2 and `no fun` at -3,
and those curated values are used rather than `-1 × good` and `-1 × fun`:

```rust
use verbora_sentiment::{Language, SentimentAnalyzer, VocabularyKind};

fn main() {
    let a = SentimentAnalyzer::without_stemmer(Language::English, VocabularyKind::Afinn).unwrap();

    // Published phrase keys, matched as one unit each.
    assert_eq!(a.get_sentiment(["not", "good"]), Some(-2.0));
    assert_eq!(a.get_sentiment(["no", "fun"]), Some(-3.0));

    // `not happy` is not a key, so the heuristic applies: two units, -3 total.
    assert_eq!(a.get_sentiment(["not", "happy"]), Some(-1.5));
}
```

Sticky negation still applies *around* a matched span — a span that follows a
negator has its published polarity flipped like anything else.

### Capitalised and phrase keys

Every key is indexed by its **lookup form**, which is lowercased, so a
capitalised entry is reachable from an ordinary lowercased token with no stemmer
involved. `pattern`/German ships 1,234 capitalised entries and both spellings
find them:

```rust
use verbora_sentiment::{Language, SentimentAnalyzer, VocabularyKind};

fn main() {
    let a = SentimentAnalyzer::without_stemmer(Language::German, VocabularyKind::Pattern).unwrap();
    assert_eq!(a.get_sentiment(["Abfall"]), Some(-0.0048));
    assert_eq!(a.get_sentiment(["abfall"]), Some(-0.0048));
}
```

Hyphenated and space-spelled keys reduce to the same lookup form, so
`cover-up` and `cover up` are one entry, reached from either spelling — and
scored as one unit either way.

### Stem collisions resolve last-wins, in file order

Supplying a stemmer rebuilds the vocabulary in source-file order, letting a
later stem overwrite an earlier one. For English AFINN with the Porter stemmer,
3,382 keys collapse to 1,967 stemmed keys, and 121 of the original keys end up
under a stemmed key whose stored polarity is not their own:

```rust
use verbora_sentiment::{Language, Polarity, Vocabulary, VocabularyKind};
use verbora_stemmers::PorterStemmer;

fn main() {
    let base = Vocabulary::shared(VocabularyKind::Afinn, Language::English).unwrap();
    let stemmed = base.stemmed(&PorterStemmer::new());
    assert_eq!(base.len(), 3382);
    assert_eq!(stemmed.len(), 1967);

    // `affection` (3) collides with a later key that stems to `affect` too,
    // and the later entry in file order wins.
    assert_eq!(base.get("affection").map(Polarity::value), Some(3.0));
    assert_eq!(stemmed.get("affect").map(Polarity::value), Some(3.0));

    // `arrested` (-3) and `arrests` (-2) both stem to `arrest`; last wins, so
    // `arrested` no longer answers with its own polarity.
    assert_eq!(base.get("arrested").map(Polarity::value), Some(-3.0));
    assert_eq!(base.get("arrests").map(Polarity::value), Some(-2.0));
    assert_eq!(stemmed.get("arrest").map(Polarity::value), Some(-2.0));
}
```

The order is file order, not hash or alphabetical order, and it is stable
across runs and platforms.

### Summation is left-to-right and bit-reproducible

`score` accumulates in `f64` strictly left to right and divides exactly once at
the end. `Contributions` yields a real `0.0` addend for every unit that scored
nothing, so folding it by hand reproduces the same bits:

```rust
use verbora_sentiment::{Language, SentimentAnalyzer, VocabularyKind};

fn main() {
    let a = SentimentAnalyzer::without_stemmer(Language::English, VocabularyKind::Senticon).unwrap();
    let tokens = ["good", "bad", "excellent", "awful", "fine"];

    let by_explicit_fold: f64 = a.contributions(tokens).fold(0.0, |acc, d| acc + d);
    assert_eq!(a.score(tokens).sum.to_bits(), by_explicit_fold.to_bits());
}
```

### Constructor errors

There is exactly one way a constructor can fail: no vocabulary ships for the
requested pair. Both arguments are enums, so a misspelling cannot reach the
constructor — only a pair the table genuinely lacks, such as AFINN Dutch.
`UnsupportedPair` is therefore a struct rather than an enum, and it carries the
pair that was asked for:

```rust
use verbora_sentiment::{Language, SentimentAnalyzer, VocabularyKind};

fn main() {
    let e = SentimentAnalyzer::without_stemmer(Language::Dutch, VocabularyKind::Afinn)
        .unwrap_err();
    assert_eq!(e.kind, VocabularyKind::Afinn);
    assert_eq!(e.language, Language::Dutch);

    // The message names the pair and what the family does ship for.
    assert_eq!(
        e.to_string(),
        "no afinn vocabulary ships for Dutch (nl); \
         afinn does ship for [English, Spanish, Portuguese]"
    );
}
```

A name read from a configuration file or a command line becomes an enum through
the `FromStr` implementations, which report an `UnknownName` carrying the names
that would have been accepted. Code that knows which vocabulary it wants names
the variant and never sees that error.

### Concurrency

`SentimentAnalyzer` is immutable after construction — nothing is cached or
mutated per call — and is `Send + Sync` whenever its stemmer type parameter is
(`NoStemmer` always is). Build once, wrap in an `Arc`, share read-only across
threads with no locking.

## Performance characteristics

**Timings are unmeasured.** Lookup forms and span matching changed both the
table-build path — every key is now segmented once, when its table is first
touched — and the scoring loop, which buffers one token and looks ahead only
where a phrase key could start. Every figure this section carried was measured
against the table-build and scoring paths that preceded that work, so all of
them are **withdrawn**, and nothing is estimated in their place until
`cargo bench -p verbora-sentiment` is run again on settled code.
`crates/verbora-sentiment/benches/sentiment.rs` is the suite that will answer
it: it covers cold decode, construction with and without a stemmer, scoring at
several document sizes, and the token-shape cost of case-folding.

What is structural, and therefore stated here rather than measured:

- **Nothing is decoded until a vocabulary is asked for.** A program that never
  scores sentiment pays nothing for the data being present.
- **Each `(kind, language)` table is decoded once per process.** Scoring English
  never touches the Basque table.
- **The lexicons ship as prebuilt `key \0 polarity \0` blobs** — 75,803 pairs,
  1.2 MB across thirteen `include_bytes!` blobs (the fourteenth is empty),
  decoded with one `str::split_terminator('\0')` pass and cached behind a
  `OnceLock`.
- **Case folding is the only per-token work that varies with input shape.** An
  already-lowercase token is compared as it stands; an uppercase ASCII token
  costs one `to_ascii_lowercase`; a non-ASCII token costs the full Unicode
  `to_lowercase`. Those are three different code paths, and which one a document
  takes is worth knowing — but their relative cost is part of what is
  unmeasured.

See [Benchmarks](../benchmarks/index).

## Allocation behaviour

| Step | Allocates |
|---|---|
| First load of a vocabulary | One `Vec<Entry>` and one hash map, sized to the entry count. Keys are `Cow::Borrowed` slices of the embedded `'static` blob — no string data is copied. Once per process, per vocabulary. |
| Constructing without a stemmer | Two `String`s (the stored `language()` and `vocabulary_type()`). The table itself is a pointer copy. |
| Constructing with a stemmer | A fresh `Vec` and hash map, pre-sized. Each key becomes `Cow::Borrowed` if the stemmer left it unchanged, `Cow::Owned` if not. Polarity values are never copied. |
| Scoring | A bounded lookahead: up to the length of the longest key that *begins with the current token*, a number stored in the same hash slot as that token's own polarity, so a token that begins no phrase reads it for free and never buffers past one. The buffered `String`s are cleared and reused rather than reallocated, so a long document allocates a small, fixed amount however many tokens it holds. `polarity` is one hash probe and one `Vec` index. |

The lookahead is pulled from the source iterator, so a `Contributions` dropped
mid-run may have consumed a few tokens more than it reported.

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
- **Capitalised and multi-word entries are reachable.** Keys are indexed by
  their lookup form — word pieces, lowercased, joined by one space — so German
  pattern's 1,234 capitalised entries answer to a lowercased token, and a
  multi-word entry is found by matching the longest span of tokens that forms
  it. A stemmer is not required for either.
- **Spanish and Portuguese AFINN key on emoji directly.** `😂` is a real entry
  (`1.0`). Across the fourteen tables, 1,488 keys are emoji or circled letters
  with no word segment at all: `WordTokenizer` filters symbol runs out, so
  reaching those keys needs a tokenizer that emits them, or a token spelled
  exactly like the key. Characters that are not entries score `0.0` without
  erroring, like any other miss.
- **102 keys are shadowed.** English senticon ships both `pitch-black` and
  `pitch black`; `pattern`/German ships both `Stolz` and `stolz` with different
  polarities. They reduce to one lookup form, and the later entry in file order
  wins. `Vocabulary::len` still counts both.

## Common mistakes

**Constructing a stemmed analyzer inside a request handler or per-document
loop.** Every construction pays the full rebuild — 33,874 `stem` calls for
English senticon, one per piece of every key, with nothing cached between
constructions. Build it once:

```rust
use verbora_sentiment::{Language, SentimentAnalyzer, VocabularyKind};
use verbora_stemmers::PorterStemmer;

fn main() {
    // Build once...
    let analyzer = SentimentAnalyzer::with_stemmer(
        Language::English,
        VocabularyKind::Afinn,
        PorterStemmer::new(),
    )
    .unwrap();

    // ...and reuse it for every document.
    let documents = [["not", "happy"], ["good", "good"]];
    let scores: Vec<Option<f64>> = documents
        .iter()
        .map(|doc| analyzer.get_sentiment(doc.iter().copied()))
        .collect();
    assert_eq!(scores, [Some(-1.5), Some(3.0)]);
}
```

**Expecting `AfinnFinancialMarketNews` to have real data.** It ships empty;
`a.vocabulary().len()` is `0` and every score against it is `Some(0.0)`.

**Assuming scores are normalized to `[-1, 1]`.** Nothing clamps anything.
`get_sentiment` is an average, so it stays within the extremes of the
vocabulary you chose — `-5..=5` for AFINN, `-1.0..=1.0` for senticon and
pattern — not within any universal range. `score().sum` has no bound at all:

```rust
use verbora_sentiment::{Language, SentimentAnalyzer, VocabularyKind};

fn main() {
    let a = SentimentAnalyzer::without_stemmer(Language::English, VocabularyKind::Afinn).unwrap();
    let long_review: Vec<&str> = std::iter::repeat_n("good", 10_000).collect();

    // The AVERAGE stays within AFINN's own -5..=5 range.
    assert_eq!(a.get_sentiment(long_review.iter().copied()), Some(3.0));
    // The raw, undivided SUM does not: it grows with the document.
    assert_eq!(a.score(long_review.iter().copied()).sum, 30_000.0);
}
```

**Forgetting that a negation word is never also scored.** `no` is a real AFINN
entry worth `-1.0` *and* an English negation word, so it always contributes
`0.0` and flips what follows.

**Treating `None` as zero.** `get_sentiment` answers `None` when nothing was
scored, and collapsing that to `0.0` with `unwrap_or(0.0)` puts an empty
document and a genuinely neutral one in the same bucket. Decide what an unscored
document means for your aggregate — usually excluding it from the average rather
than voting zero into it.

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
    pub const ALL: [Self; 4];                        // table order
    pub const NAMES: [&'static str; 4];              // in ALL order
    pub const fn name(self) -> &'static str;         // "afinn", "senticon", …
    pub fn from_name(name: &str) -> Option<Self>;    // exact match, no case folding
    pub fn languages(self) -> Vec<Language>;         // table order
}
impl Display for VocabularyKind;                     // writes name()
impl FromStr for VocabularyKind { type Err = UnknownName; }

pub enum Language {
    Basque, Catalan, Dutch, English, French,
    Galician, German, Italian, Portuguese, Spanish,  // alphabetical by English name
}
impl Language {
    pub const ALL: [Self; 10];
    pub const CODES: [&'static str; 10];             // in ALL order
    pub const fn code(self) -> &'static str;         // ISO 639-1: "en", "es", …
    pub fn from_code(code: &str) -> Option<Self>;    // exact match, lower-case only
}
impl Display for Language;                           // writes code()
impl FromStr for Language { type Err = UnknownName; }

pub struct UnknownName { pub name: String, pub accepted: &'static [&'static str] }

pub fn supported_pairs() -> impl ExactSizeIterator<Item = (VocabularyKind, Language)>; // 14

pub struct SentimentAnalyzer<S = NoStemmer> { /* private */ }
impl SentimentAnalyzer<NoStemmer> {
    pub fn without_stemmer(language: Language, kind: VocabularyKind)
        -> Result<Self, UnsupportedPair>;
}
impl<S: Stemmer> SentimentAnalyzer<S> {
    pub fn with_stemmer(language: Language, kind: VocabularyKind, stemmer: S)
        -> Result<Self, UnsupportedPair>;
    pub fn language(&self) -> Language;
    pub fn kind(&self) -> VocabularyKind;
    pub fn vocabulary(&self) -> &Vocabulary;
    pub fn negations(&self) -> &'static [&'static str];
    pub fn stemmer(&self) -> Option<&S>;

    pub fn contributions<I>(&self, words: I) -> Contributions<'_, S, I::IntoIter>
        where I: IntoIterator, I::Item: AsRef<str>;
    pub fn score<I>(&self, words: I) -> Score
        where I: IntoIterator, I::Item: AsRef<str>;
    pub fn get_sentiment<I>(&self, words: I) -> Option<f64>
        where I: IntoIterator, I::Item: AsRef<str>;
    pub fn get_sentiment_over<I>(&self, words: I, len: usize) -> Option<f64>
        where I: IntoIterator, I::Item: AsRef<str>;

    // requires the `parallel` Cargo feature; S: Sync
    pub fn par_get_sentiment_batch<'d, D>(&self, docs: &'d [D]) -> Vec<Option<f64>>
        where D: Sync, /* &D: IntoIterator<Item: AsRef<str>> */;
}

pub struct Contributions<'a, S, I> { /* private */ }
impl<S: Stemmer, I> Iterator for Contributions<'_, S, I>
    where I: Iterator, I::Item: AsRef<str> { type Item = f64; }

pub struct Score { pub sum: f64, pub count: usize } // count is UNITS, not tokens
impl Score {
    pub fn mean(self) -> Option<f64>;            // sum / count  (None if count == 0)
    pub fn over(self, len: usize) -> Option<f64>; // sum / len   (None if len == 0)
}

// The one way a constructor can fail: no vocabulary ships for that pair.
pub struct UnsupportedPair { pub kind: VocabularyKind, pub language: Language }
impl Display for UnsupportedPair;
impl std::error::Error for UnsupportedPair;

pub trait Stemmer {
    fn stem<'a>(&self, word: &'a str) -> std::borrow::Cow<'a, str>;
}
impl<T: Stemmer + ?Sized> Stemmer for &T {}
impl<T: Stemmer + ?Sized> Stemmer for Box<T> {}
impl<T: Stemmer + ?Sized> Stemmer for std::sync::Arc<T> {}
// Every verbora_stemmers type implements Stemmer with no adapter.

pub struct NoStemmer;
impl Stemmer for NoStemmer { /* identity: always Cow::Borrowed */ }

// A value and the text the source file published it as. Fields are private and
// there is no public constructor, which is what makes `value` total.
pub struct Polarity { /* private */ }
impl Polarity {
    pub fn value(self) -> f64;                    // always finite
    pub fn as_written(self) -> Option<&'static str>; // None for AFINN's integers
}
impl Display for Polarity;                        // as_written, else value

pub struct Vocabulary { /* private */ }
impl Vocabulary {
    pub fn kind(&self) -> VocabularyKind;
    pub fn len(&self) -> usize;
    pub fn is_empty(&self) -> bool;
    // `get` matches by lookup form, so `get("Cover-Up")`, `get("cover-up")`
    // and `get("cover up")` are one lookup.
    pub fn get(&self, word: &str) -> Option<Polarity>;
    pub fn keys(&self) -> impl ExactSizeIterator<Item = &str>;
    pub fn iter(&self) -> impl ExactSizeIterator<Item = (&str, Polarity)>;
    pub fn shared(kind: VocabularyKind, language: Language) -> Option<&'static Self>;
    pub fn stemmed<S: Stemmer + ?Sized>(&'static self, stemmer: &S) -> Self;
}
```

No `unsafe`, no global mutable state, no `_into` buffer-reuse variant.
`SentimentAnalyzer<S>` and `Vocabulary` are `Send + Sync` whenever `S` is.
`par_get_sentiment_batch` is the crate's only parallel entry point, gated
behind the `parallel` Cargo feature and off by default.
