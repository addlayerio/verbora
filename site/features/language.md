# Language and script detection

`verbora-language` answers a question [`verbora-phonetics`](phonetics.md)
cannot answer on its own: *given a word or a document, which phonetic encoder
should even be used?* It keeps three layers deliberately separate:

| Layer | Cost | Cargo feature |
|---|---|---|
| [Script detection](#script-detection) — which writing system | ~9 ns/word, allocation-free | none |
| [Language detection](#language-detection) — statistical, probabilistic | tens of µs per call | `language-detection` |
| [`recommend()`](#phonetic-strategy-recommend) — closed lookup, language → encoder | ~6 ns | none |

They compose only at the edge, and only if you ask for that composition. This
crate never encodes anything itself.

## Explicit vs. automatic

**This is the decision that matters most on this page.**

| Situation | Do this |
|---|---|
| You already know the language (locale, per-record field, user's picker) | Call `recommend(Language::X)` directly — a closed lookup, not a guess |
| You don't, and you have a paragraph or more | `WhatlangDetector` / `AutoPhoneticStrategy`, gated on `best_above(threshold)` — never bare `best()` |
| You don't, and you have one short word or a name | Do **not** trust automatic detection. Inspect `LanguageDetection.candidates`, or require a high threshold and treat `None` as the correct answer |

The two paths are not close in cost. `recommend(Language::German)` measures
**5.67–7.48 ns**. `AutoPhoneticStrategy::detect_and_recommend` on the same
language measures 28.09 µs for a short text and 147.8 µs for a long document —
roughly **4,260× to 22,400× more**. Automatic detection is worth every
microsecond when you genuinely don't know the language, and worth skipping
entirely when you do.

## When to use it

- **You have text and no reliable language hint**, and need one before picking
  a phonetic strategy, a tokenizer variant, or a stemmer. `detect_script` is
  nearly free and should run first regardless; `WhatlangDetector` earns its
  cost once you have at least a full sentence.
- **You're processing many independent texts** (reviews, tickets, log lines),
  each needing its own guess — `par_detect_batch`, feature `parallel`.
- **You want a phonetic-strategy recommendation** without hand-writing a
  per-language `match`.

## When not to use it

- **Detecting the language of a single word or short name, and trusting the
  answer.** See [Confidence and ambiguity](#confidence-and-ambiguity).
- **Inferring anything about a *person* from their name.** This detects
  language, not nationality, ethnicity, or name origin.
- **Language detection inside a hot per-token loop.** `WhatlangDetector::detect`
  costs tens to low hundreds of *microseconds*. Detect once per document.
- **Replacing `verbora-phonetics`.** `recommend()` only says which encoder to
  reach for.

## Quick example

```rust ignore
use verbora_language::{Language, PhoneticRecommendation, Script, detect_script, recommend};

fn main() {
    // Script detection needs no Cargo feature and never allocates.
    assert_eq!(detect_script("Müller"), Some(Script::Latin));
    assert_eq!(detect_script("Москва"), Some(Script::Cyrillic));

    // If you already know the language, `recommend` is a closed lookup —
    // no detector, no confidence, nothing to be uncertain about.
    let strategy = recommend(Language::German);
    assert_eq!(strategy.primary, Some(PhoneticRecommendation::SoundExDaitchMokotoff));
}
```

## Script detection

`detect_script` classifies the *writing system* a string is in — a majority
vote over each character's Unicode block, not a model. One pass over
`input.chars()`, a fixed 10-element counting array on the stack, no allocation
at any input length, no dependency beyond `std`.

```rust ignore
use verbora_language::{Script, detect_script};

fn main() {
    assert_eq!(detect_script("hello world"), Some(Script::Latin));
    assert_eq!(detect_script("café müller"), Some(Script::Latin)); // diacritics included
    assert_eq!(detect_script("こんにちは"), Some(Script::Hiragana));
    assert_eq!(detect_script("日本語"), Some(Script::Han));
    assert_eq!(detect_script("العربية"), Some(Script::Arabic));

    // No classifiable letters at all -> None. Not an error.
    assert_eq!(detect_script("123 !@# ..."), None);
}
```

`Script` has 11 variants:

| `Script` | What maps to it |
|---|---|
| `Latin` | Every Latin-script language, diacritics included (Vietnamese's Latin Extended Additional range too) |
| `Cyrillic` | Russian, Ukrainian, and other Cyrillic-script text |
| `Greek` | Modern Greek |
| `Arabic` | Arabic and other Arabic-script text |
| `Hebrew` | Hebrew |
| `Han` | CJK ideographs — Chinese, and the kanji portion of Japanese |
| `Hiragana` / `Katakana` | Japanese kana |
| `Hangul` | Korean (no `Language` variant covers Korean today) |
| `Devanagari` | Hindi and other Devanagari-script text |
| `Other` | A script with no dedicated variant — not an error |

Mixed-script input returns whichever script has the most characters; ties break
toward the first script checked, not toward whichever appeared first in the
input. `None` comes back only when there is nothing classifiable at all.

Script detection is reliable on short input precisely because it asks a coarser
question: knowing a word is Cyrillic does not tell you Russian from Ukrainian,
but it rules out every Latin-script language at zero cost.

## Language detection

`LanguageDetector` is the abstraction — one method, implemented by
`WhatlangDetector` and by anything you write yourself. The trait, `Language`,
`Script`/`detect_script`, `recommend`, and `AutoPhoneticStrategy<D>` all
compile with **zero** extra dependencies; only a real detector needs one.

```rust ignore
use verbora_language::{Language, LanguageCandidate, LanguageDetection, LanguageDetector};

struct AlwaysGerman;

impl LanguageDetector for AlwaysGerman {
    fn detect(&self, _input: &str) -> LanguageDetection {
        LanguageDetection {
            candidates: vec![LanguageCandidate {
                language: Language::German,
                confidence: 0.42,
            }],
        }
    }
}

fn main() {
    let detection = AlwaysGerman.detect("anything");

    // `best()` ignores confidence entirely — it returns the top candidate
    // no matter how low its confidence is.
    assert_eq!(detection.best().unwrap().language, Language::German);

    // `best_above()` is what a caller should actually gate on.
    assert_eq!(detection.best_above(0.5), None);  // 0.42 < 0.5
    assert!(detection.best_above(0.3).is_some()); // 0.42 >= 0.3
}
```

**There is no built-in default threshold anywhere in this crate.** What counts
as "confident enough" depends on your tolerance for a wrong guess, and the
right value varies with input length. You pick it; `best_above` enforces it.

### `WhatlangDetector`

Behind the `language-detection` Cargo feature, `WhatlangDetector` is a
zero-sized `LanguageDetector` backed by an n-gram frequency model. MIT
licensed, one transitive dependency, ~685 KB of compiled-in frequency tables,
covering 20 of this crate's 22 languages (Galician and Basque are the gaps).

```rust ignore
use verbora_language::{Language, LanguageDetector, WhatlangDetector};

fn main() {
    let detector = WhatlangDetector::new(); // zero-sized, nothing to construct

    let long_sentence = detector.detect(
        "This is a long, grammatically complete English sentence, written \
         specifically to be unambiguous to any reasonable language detector.",
    );
    assert_eq!(long_sentence.best().unwrap().language, Language::English);

    let single_word = detector.detect("hotel");
    assert!(single_word.best().is_none()); // no usable signal at all
}
```

`confidence` is `whatlang`'s **relative-margin score** — how much better the
winner scored than the runner-up — not a calibrated probability.
`WhatlangDetector` reports it as-is, halved when `whatlang` itself considers the
result unreliable, so a caller comparing against their own threshold still sees
something rather than a silently vanished candidate. **Confidence values from
two different `LanguageDetector` implementations are not comparable**; this
crate does not normalize across detectors.

## Confidence and ambiguity

Language detection is probabilistic, and this is why the rest of the crate is
shaped the way it is: `LanguageDetection` can be empty, `best_above` makes
*you* decide what counts as confident, and `AutoPhoneticStrategy` never
recommends a strategy your threshold wouldn't clear.

Real `WhatlangDetector::detect` results, each input detected completely alone:

| Input | `best()` |
|---|---|
| `"hotel"`, `"radio"`, `"piano"`, `"normal"`, `"color"` | no candidate at all |
| `"Kenji"` | no candidate at all |
| `"Panichella"` | `Italian`, confidence ≈ 0.073 |
| `"Ivan"` | `Dutch`, confidence ≈ 0.002 |
| `"Mueller"` | `Norwegian`, confidence ≈ 0.008 |
| `"Marie"` | `Polish`, confidence ≈ 0.000000 |
| A long, unambiguous English sentence | `English`, confidence 1.000000 |
| The same sentence in German | `German`, confidence 1.000000 |

Two things follow. First, common cross-language vocabulary returns *no
candidate at all* when detected alone — stronger than "low confidence": there
is no language with enough signal to report. Second, **names are not
language.** `"Mueller"` — a German-origin surname by anyone's intuition — comes
back as `Norwegian` at a confidence any threshold above ~1% rejects.
`"Panichella"` happens to score `Italian`, matching the name's real origin, but
at an order of magnitude below anything `best_above()` should accept. A name's
national association is a different claim from the text being written in that
language, and the numbers say so, not just the prose.

`tests/ambiguity.rs` pins this. It never asserts a specific language for
ambiguous input — that would be the false-confidence bug it exists to prevent.
It asserts that uncertainty is represented honestly (no candidate, or one a
caller can reject at a normal threshold), that a long unambiguous sentence
**must** clear the threshold, and that empty input reports zero candidates
rather than a guess.

```rust ignore
use verbora_language::{AutoPhoneticStrategy, WhatlangDetector};

fn main() {
    let auto = AutoPhoneticStrategy::new(WhatlangDetector::new(), 0.6);
    let result = auto.detect_and_recommend("hotel");

    // Below-threshold — or no candidate at all — never produces a strategy.
    assert!(result.strategy.is_none());

    // The raw detection is still there to inspect: "not confident enough to
    // act automatically" is not the same as "found nothing".
    match result.detection.best() {
        Some(c) => eprintln!("low-confidence guess: {:?} ({:.3})", c.language, c.confidence),
        None => eprintln!("no signal at all"), // the branch "hotel" actually takes
    }
}
```

## Phonetic strategy: `recommend()`

`recommend(language: Language) -> PhoneticStrategy` is a **closed lookup
table**, not a statistical process — once the language is known, there is
nothing left to be uncertain about. Its domain is exactly Verbora's four core
encoders, all English-oriented algorithms with no language-specific variants;
it answers which is the closest fit, and whether a transliteration step needs
to run first.

```rust ignore
use verbora_language::{Language, PhoneticRecommendation, TransliterationAdvice, recommend};

fn main() {
    let strategy = recommend(Language::German);
    assert_eq!(strategy.primary, Some(PhoneticRecommendation::SoundExDaitchMokotoff));
    assert_eq!(
        strategy.alternatives,
        vec![PhoneticRecommendation::DoubleMetaphone, PhoneticRecommendation::SoundEx],
    );
    assert_eq!(strategy.transliteration, TransliterationAdvice::NotNeeded);
}
```

| Field | Type | Meaning |
|---|---|---|
| `primary` | `Option<PhoneticRecommendation>` | The best-fit encoder. `None` only when no encoder was designed for the language's phonotactics (Persian, Hindi, Chinese) — recommending an encoder that cannot honestly do anything is the false confidence this type avoids. |
| `alternatives` | `Vec<PhoneticRecommendation>` | Other legitimate choices, **unranked**. Always empty when `primary` is `None`. |
| `transliteration` | `TransliterationAdvice` | Whether a transliteration step should run first — see [Transliteration integration](#transliteration-integration). |

`PhoneticRecommendation` names one of four `verbora-phonetics` encoders without
depending on their types: `SoundEx`, `Metaphone`, `DoubleMetaphone`, and
`SoundExDaitchMokotoff` (`SoundExDM`). The full language-by-language table —
all 22 `Language` variants — lives on
[Phonetics § Choosing a Phonetic Algorithm](phonetics.md#choosing-a-phonetic-algorithm),
so there is exactly one place it can go stale.

### Coarser: `recommend_for_script`

For callers who only ran [`detect_script`](#script-detection),
`recommend_for_script(script: Script) -> PhoneticStrategy` gives the same shape
of answer, coarser: a whole script maps to one strategy.

```rust ignore
use verbora_language::{PhoneticRecommendation, Script, TransliterationAdvice, recommend_for_script};

fn main() {
    let latin = recommend_for_script(Script::Latin);
    assert_eq!(latin.primary, Some(PhoneticRecommendation::DoubleMetaphone));

    // Cyrillic gets no confident primary at the script level -- Russian and
    // Ukrainian resolve differently once the actual `Language` is known.
    let cyrillic = recommend_for_script(Script::Cyrillic);
    assert_eq!(cyrillic.primary, None);
    assert_eq!(cyrillic.transliteration, TransliterationAdvice::Unsupported);
}
```

`Script::Han` is ambiguous between Chinese and Japanese kanji, so
`recommend_for_script` advises transliteration only when kana are present
alongside the Han characters; pure Han input gets `Unsupported`. A caller who
can determine the real `Language` should always prefer `recommend`.

## Transliteration integration

None of the four core encoders were designed for non-Latin scripts, and
applying one to un-transliterated text mostly doesn't fail — it quietly
produces a key with no phonetic meaning. `TransliterationAdvice` says what to
do about that:

| Variant | Meaning |
|---|---|
| `NotNeeded` | Latin script; encode directly. |
| `Recommended` | A transliteration step exists and should run before encoding. |
| `Unsupported` | No Verbora transliteration path exists for this script. |

```rust ignore
use verbora_language::{Language, TransliterationAdvice, apply_transliteration, recommend};

fn main() {
    // Japanese: Recommended -- runs verbora_transliterators::transliterate_ja.
    let strategy = recommend(Language::Japanese);
    assert_eq!(strategy.transliteration, TransliterationAdvice::Recommended);
    assert_eq!(apply_transliteration(strategy.transliteration, "にほん"), "nihon");

    // Russian: Unsupported -- the input passes through completely unchanged.
    let russian = recommend(Language::Russian);
    assert_eq!(russian.transliteration, TransliterationAdvice::Unsupported);
    assert_eq!(apply_transliteration(russian.transliteration, "Москва"), "Москва");
}
```

`apply_transliteration` returns `input` unchanged for **both** `NotNeeded` and
`Unsupported`. Your own `match` on `advice` is what should decide whether to
trust the phonetic step that follows.

Today exactly one language gets `Recommended`, and "unsupported" means two
different things:

| Language(s) | Advice | What it means |
|---|---|---|
| Every Latin-script language | `NotNeeded` | Nothing to transliterate. |
| Japanese | `Recommended` | [`transliterate_ja`](transliterators.md) romanizes kana/kanji into a form the encoders can work with. |
| Polish, Ukrainian, Russian | `Unsupported` | No Cyrillic transliterator — but `SoundExDM` is still `primary`. That recommendation only becomes meaningful once *you* romanize the input. |
| Persian, Hindi, Chinese | `Unsupported` | No transliterator **and no `primary` either** — no encoder was designed for these phonotactics regardless of script. |

## Batch and parallel detection

Behind the `parallel` Cargo feature — independent of `language-detection`, as
it is generic over any `D: LanguageDetector + Sync` —
`par_detect_batch(detector, texts) -> Vec<LanguageDetection>` fans a batch of
independent texts across a `rayon` pool, one `detect()` per text, preserving
input order.

```rust ignore
use verbora_language::{WhatlangDetector, par_detect_batch};

fn main() {
    let detector = WhatlangDetector::new();
    let texts = [
        "This is a long, unambiguous English sentence used for detection.",
        "Das ist ein eindeutig deutscher Satz, lang genug zum Erkennen.",
    ];
    let results = par_detect_batch(&detector, &texts);
    assert_eq!(results.len(), 2);
}
```

This is for a large, independent corpus — one guess per document — not for
detecting a single text faster.

## Performance characteristics

<div class="callout callout-note">
<strong>Machine-dependent.</strong> Every number below came from
<code>cargo bench -p verbora-language --all-features</code> on one development
machine. Ratios and orders of magnitude should hold; exact figures will not
reproduce identically on different hardware.
</div>

**`detect_script`** — linear in input length, no allocation, no jump at any
size:

| Input | Size | Time |
|---|---:|---:|
| A single word | 6 B | 8.61–8.71 ns |
| A short sentence | 68 B | 64.74–66.89 ns |
| A paragraph | 523 B | 527.09–530.50 ns |
| A long document | 10,460 B | 8.80–8.88 µs |

**`WhatlangDetector::detect`** — three to four orders of magnitude more than
script detection at comparable lengths:

| Input | Time |
|---|---:|
| A short sentence | 27.97–28.00 µs |
| A paragraph | 67.90–68.03 µs |
| A long document | 147.77–148.03 µs |

**`recommend()`** — 1.41–8.31 ns per language. `Persian`, `Hindi` and `Chinese`
(`primary: None`, empty `alternatives`, no `Vec` allocation) are fastest at
~1.4–1.6 ns; every other language sits at ~5.6–8.3 ns. All 22 back to back:
207.16–207.52 ns.

**Sequential vs. `par_detect_batch`**, short-text items, 32-core machine:

| Batch size | Sequential | Parallel | Speedup |
|---:|---:|---:|---:|
| 16 | 461.4–482.7 µs | 84.98–91.44 µs | ~5.3× |
| 64 | 1.862–2.057 ms | 223.2–334.5 µs | ~6–7× |
| 256 | 7.270–7.294 ms | 735.7–960.9 µs | ~8–9× |
| 1,024 | 29.15–29.37 ms | 2.643–3.510 ms | ~9–10× |
| 4,096 | 120.7–123.3 ms | 8.255–8.893 ms | ~13–14× |

Parallel wins at every tested size, including 16 — a single `detect()` call is
already expensive enough that `rayon`'s fork-join overhead is negligible. That
is specific to this crate; cheaper per-item primitives elsewhere in the
workspace do show a crossover. See
[Parallelism](../performance/parallelism.md).

**Memory and initialization.** The frequency tables are compile-time `static`
arrays in read-only data — no runtime load, no file I/O, no deserialization,
nothing to warm up. `WhatlangDetector::new()` costs **0 allocations, 0 bytes**.
`WhatlangDetector` is `Copy`, zero-sized, and automatically `Send + Sync`, with
no `unsafe impl` anywhere; `par_detect_batch` shares one instance across the
whole pool.

## Allocation behaviour

| Operation | Allocates |
|---|---|
| `detect_script` | Nothing — a fixed-size stack array and a `chars()` scan |
| `recommend` / `recommend_for_script` | Nothing for `Persian`/`Hindi`/`Chinese`; one short `Vec` of `alternatives` for every other language |
| `WhatlangDetector::detect` | Exactly 25 allocations, **constant in input length** (a 6 B word and a 10 KB document allocate identically), plus at most one single-element `Vec` for `candidates` |
| `AutoPhoneticStrategy::detect_and_recommend` | Whatever the wrapped detector allocates, plus `recommend`'s own short `Vec` only when confidence clears the threshold |
| `apply_transliteration` | Nothing for `NotNeeded`/`Unsupported` (`Cow::Borrowed`); whatever `transliterate_ja` allocates for `Recommended` |
| `par_detect_batch` | One output `Vec<LanguageDetection>`, plus whatever each `detect` allocates — no per-chunk buffering |

## Common mistakes

**Trusting `best()` instead of `best_above()` on short input.** `best()`
ignores confidence entirely. Gate on a threshold you chose, especially for
anything shorter than a full sentence.

**Treating a name's national or ethnic association as a language
determination.** `"Mueller"` detects as `Norwegian` at confidence ≈ 0.008.

**Calling `WhatlangDetector::detect` in a hot per-token loop.** Tens to low
hundreds of microseconds per call — detect once per document, and use
`par_detect_batch` for a real batch.

**Assuming `recommend()`'s `alternatives` are ranked.** They are not — "also
legitimate", no ordering beyond that.

**Assuming Polish/Ukrainian/Russian's `SoundExDM` primary means Cyrillic gets
romanized for you.** It doesn't. `apply_transliteration` passes the input
through unchanged for `Unsupported`.

**Confusing Persian/Hindi/Chinese's `primary: None` with a bug.** It is
deliberate: there is no encoder that fits.

## Related

- [Phonetics](phonetics.md) — the four encoders `recommend()` chooses between,
  and the [per-language table](phonetics.md#choosing-a-phonetic-algorithm) it
  backs directly.
- [Phonetic neighbors](phonetic-index.md) — the dictionary-wide index built
  from whichever encoder you land on.
- [Transliterators](transliterators.md) — `transliterate_ja`, the one
  transliteration path this crate composes with today.
- [Parallelism](../performance/parallelism.md).
- [Choosing the right API](../choosing/index.md).

## API reference

### Types

| Item | Description |
|---|---|
| `Language` | `#[non_exhaustive]` enum, 22 variants. `ALL`, `iso639_1() -> &str`, `name() -> &str`, `from_iso639_1(&str) -> Option<Self>`, `FromStr`, `Display` |
| `Script` | `#[non_exhaustive]` enum, 11 variants. `Display` |
| `LanguageDetector` | Trait: `fn detect(&self, input: &str) -> LanguageDetection` |
| `LanguageCandidate` | `{ language: Language, confidence: f32 }` (`0.0..=1.0`) |
| `LanguageDetection` | `{ candidates: Vec<LanguageCandidate> }`, descending by confidence |
| `WhatlangDetector` (feature `language-detection`) | Zero-sized `LanguageDetector` backed by `whatlang`. `Copy`, `Clone`, `Default`, `const fn new()` |
| `AutoPhoneticStrategy<D>` | Combines a `D: LanguageDetector` with `recommend`, gated by a threshold |
| `AutoResult` | `{ detection: LanguageDetection, strategy: Option<PhoneticStrategy> }` |
| `PhoneticRecommendation` | `SoundEx`, `Metaphone`, `DoubleMetaphone`, `SoundExDaitchMokotoff` |
| `TransliterationAdvice` | `NotNeeded`, `Recommended`, `Unsupported` |
| `PhoneticStrategy` | `{ primary: Option<PhoneticRecommendation>, alternatives: Vec<PhoneticRecommendation>, transliteration: TransliterationAdvice }` |
| `ParseLanguageError` | `Language::from_str` failure. Implements `std::error::Error` |

### Methods and functions

| Item | Signature |
|---|---|
| `Language::ALL` | `const [Language; 22]` |
| `Language::iso639_1` | `(self) -> &'static str` |
| `Language::name` | `(self) -> &'static str` |
| `Language::from_iso639_1` | `(code: &str) -> Option<Self>` (case-insensitive) |
| `detect_script` | `(input: &str) -> Option<Script>` |
| `LanguageDetection::none` | `() -> Self` |
| `LanguageDetection::best` | `(&self) -> Option<&LanguageCandidate>` |
| `LanguageDetection::best_above` | `(&self, threshold: f32) -> Option<&LanguageCandidate>` |
| `WhatlangDetector::new` (feature `language-detection`) | `const fn() -> Self` |
| `AutoPhoneticStrategy::new` | `(detector: D, threshold: f32) -> Self` |
| `AutoPhoneticStrategy::detect_and_recommend` | `(&self, input: &str) -> AutoResult` |
| `recommend` | `(language: Language) -> PhoneticStrategy` |
| `recommend_for_script` | `(script: Script) -> PhoneticStrategy` |
| `apply_transliteration` | `(advice: TransliterationAdvice, input: &str) -> Cow<'_, str>` |
| `par_detect_batch` (feature `parallel`) | `<D: LanguageDetector + Sync>(detector: &D, texts: &[&str]) -> Vec<LanguageDetection>` |
