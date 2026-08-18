# Stemmers

`verbora-stemmers` reduces inflected words to stable stems for indexing,
matching and feature extraction. Sixteen stemmers ship: thirteen
Porter/Snowball implementations covering twelve languages, plus Lancaster,
a Japanese katakana stemmer and an Indonesian dictionary stemmer.

## When to use it

- You are building a search index, a TF-IDF model or a classifier and want
  `running`, `runs` and `ran`-style variants to collide on one key.
- You want stop words dropped and tokens stemmed in a single pass over text.
- You need stemming across languages behind one trait.

## When not to use it

- **You want a lemma.** A stemmer is reductive and its output is often not a
  word (`play` → `plai` under Porter). If you want real dictionary forms, see
  [Inflectors](inflectors.md) for the generative direction.
- **You want tokenization control.** `TokenizeAndStem` uses each language's own
  word-character class. Tokenize with [Tokenizers](tokenizers.md) and call
  `stem` per token when you need a different split.

## Quick example

```rust
use verbora_stemmers::{LancasterStemmer, PorterStemmer, TokenizeAndStem};

fn main() {
    let porter = PorterStemmer::new();
    assert_eq!(porter.stem("running"), "run");
    assert_eq!(LancasterStemmer::new().stem("maximum"), "maxim");
    assert_eq!(
        porter.tokenize_and_stem("My dog is very fun to play with", false),
        ["dog", "fun", "plai"]
    );
}
```

## Choosing the right API

| Need | API | Notes |
|---|---|---|
| Stem one token | the stemmer's `stem(&str)` | returns `Cow<'_, str>`; borrows when the token is already its own stem |
| Stream stems from text | `TokenizeAndStem::stems` | the lazy primitive: tokenizes, filters stop words, stems |
| Collect stems from text | `TokenizeAndStem::tokenize_and_stem` | `stems(..).collect()` |
| Same tokens, many documents | `TokenizeAndStem::tokenize_and_stem_cached` | you own a `token → stem` map, so each distinct token is stemmed once |
| Many documents at once | `TokenizeAndStem::par_tokenize_and_stem_batch` | feature `parallel`; one rayon task per document |
| Generic over any stemmer | `verbora_core::Stemmer` | implemented by every stemmer here |

`stems` is the primitive; the collecting, caching and parallel entry points all
preserve its tokenization, casing and stop-word behaviour exactly. Each takes a
`keep_stops: bool` — pass `false` to drop stop words.

```rust
use std::collections::HashMap;

use verbora_stemmers::{PorterStemmer, TokenizeAndStem};

fn main() {
    let porter = PorterStemmer::new();

    // Lazy: stop as soon as you have what you need, materialising nothing.
    let first = porter.stems("My dog is very fun to play with", false).next();
    assert_eq!(first.as_deref(), Some("dog"));

    // Cached: one entry per distinct token across the whole corpus.
    let mut cache: HashMap<String, String> = HashMap::new();
    let corpus = ["dogs playing", "dogs running"];
    for doc in corpus {
        let stems = porter.tokenize_and_stem_cached(doc, false, &mut cache);
        assert!(!stems.is_empty());
    }
    assert_eq!(cache["dogs"], "dog");
}
```

The cache is caller-owned so eviction, lifetime and hasher stay your decision,
and every stemmer stays zero-sized and `Sync`. It is consulted only for the
string that would have been handed to the stemmer — after the stop-word test, and
only for tokens that pass the language's gate — so results are identical to
`tokenize_and_stem`. Entries are trusted verbatim: do not share one map between
two different stemmers.

## Implementations

| Stemmer | Language |
|---|---|
| `PorterStemmer` | English |
| `PorterStemmerDe` / `PorterStemmerEs` / `PorterStemmerFa` / `PorterStemmerFr` | German, Spanish, Persian, French |
| `PorterStemmerIt` / `PorterStemmerNl` / `PorterStemmerNo` / `PorterStemmerPt` | Italian, Dutch, Norwegian, Portuguese |
| `PorterStemmerRu` / `PorterStemmerSv` / `PorterStemmerUk` | Russian, Swedish, Ukrainian |
| `CarryStemmerFr` | French, the Carry algorithm |
| `LancasterStemmer` | English, more aggressive than Porter |
| `StemmerJa` | Japanese katakana |
| `StemmerId` | Indonesian, dictionary-driven |

`PorterStemmerDe` takes options (`PorterStemmerDeOptions`), `PorterStemmerFr`
exposes its `Regions`, and `StemmerId` exposes its `Removal`, `RemovalKind` and
`RuleResult` types. Exact per-language behaviour is in the
[Rust API reference](../reference/api.md).

## Important behaviour

**Snowball algorithms index UTF-16 code units.** They compare positions against
constants, so they run over code units wherever position affects the result.
Lancaster, Carry, Japanese and Indonesian are unaffected by that distinction and
work directly on `&str`.

**Stop-word lists are process-global, per language.** English lives in
`verbora_core::stopwords` and is shared with `verbora-phonetics`, so a word
added through `PorterStemmer` is also seen by `LancasterStemmer`. Not every
stemmer exposes mutators:

| Stemmer | `add_stop_word(s)` | `remove_stop_word(s)` |
|---|:--:|:--:|
| `PorterStemmer`, `LancasterStemmer`, `StemmerId` | ✅ | ✅ |
| `PorterStemmerNo`, `PorterStemmerSv` | ✅ | ❌ |
| `PorterStemmerPt` | ✅ (`add_stop_words` only) | ❌ |
| all others | ❌ | ❌ |

<div class="callout callout-warn">
<strong>Careful.</strong> <code>PorterStemmerNl</code> is stateful. Its
<code>suffix_e_removed</code> flag is set by one step, read by a later one, and
never reset, so one instance reused across a corpus gives different answers from
a fresh instance per word — <code>stem("onaantastbar")</code> is
<code>"onaantastbar"</code> on a fresh stemmer and <code>"onaantast"</code> once
an earlier word has tripped the flag. It is therefore not <code>Sync</code>,
has no <code>par_tokenize_and_stem_batch</code>, and ignores the stem cache.
Construct one per document.
</div>

**Parallelism is per document, not per word.** Per-word stemming costs tens of
nanoseconds to a few microseconds — close enough to rayon's own per-task
scheduling cost that word-level parallelism would mostly measure the scheduler.
A whole document clears that floor comfortably, so
`par_tokenize_and_stem_batch` fans out one task per document and preserves input
order (`out[i]` is always `docs[i]`'s result).

## Related

- [Tokenizers](tokenizers.md) — what to run first when you need a different split
- [TF-IDF](tfidf.md) and [Classifiers](classifiers.md) — the usual consumers
- [Sentiment](sentiment.md)
- [Core traits](core.md) — `verbora_core::Stemmer`
- [Cargo features](../getting-started/cargo-features.md) — enabling `parallel`

## API reference

```bash
cargo doc -p verbora-stemmers --no-deps --open
```

Source: `crates/verbora-stemmers/src/`.
