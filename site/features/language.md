# Language and script detection

`verbora-language` answers a question [`verbora-phonetics`](phonetics.md)
cannot answer on its own: *given a word or a document, which phonetic encoder
should even be used?* It keeps three layers deliberately separate:

| Layer | Cost | Cargo feature |
|---|---|---|
| [Script detection](#script-detection) — which writing system | one pass over the input, allocation-free | none |
| [Language detection](#language-detection) — statistical, probabilistic | a full model evaluation per call | `language-detection` or `fast-language-detection` |
| [`recommend()`](#phonetic-strategy-recommend) — closed lookup, language → encoder | a `match` over 22 arms returning `Copy` data | none |

They compose only at the edge, and only if you ask for that composition. There
is deliberately **no** `auto_phonetic_encode(text) -> String` anywhere in this
crate: `recommend` takes a `Language`, never a `&str` and never a detector, so a
statistical guess cannot be laundered into a phonetic key without the caller
seeing it happen. This crate never encodes anything itself.

## Explicit vs. automatic

**This is the decision that matters most on this page.**

| Situation | Do this |
|---|---|
| You already know the language (locale, per-record field, user's picker) | Call `recommend(Language::X)` directly — a closed lookup, not a guess |
| You don't, and you have a paragraph or more | `AutoPhoneticStrategy` over a detector, gated on `best_above(threshold)` — never bare `best()` |
| You don't, and you have one short word or a name | Do **not** trust automatic detection. Inspect `LanguageDetection::candidates`, or require a high threshold and treat `None` as the correct answer |

The two paths are not remotely the same amount of work. `recommend` is a closed
`match` over 22 arms returning `Copy` data — no detector, no feature, no model,
no allocation, no I/O. `AutoPhoneticStrategy::detect_and_recommend` runs a full
statistical detection over the whole input *and then* that same `match`.
Automatic detection is worth its cost when you genuinely don't know the language,
and worth skipping entirely when you do.

## When to use it

- **You have text and no reliable language hint**, and need one before picking
  a phonetic strategy, a tokenizer variant, or a stemmer. `detect_script` is
  cheap and should run first regardless; a statistical detector earns its cost
  once you have at least a full sentence.
- **You're processing many independent texts** (reviews, tickets, log lines),
  each needing its own guess — `par_detect_batch`, feature `parallel`.
- **You want a phonetic-strategy recommendation** without hand-writing a
  per-language `match`, and want to know how firmly it is grounded.

## When not to use it

- **Detecting the language of a single word or short name, and trusting the
  answer.** See [Confidence and ambiguity](#confidence-and-ambiguity).
- **Inferring anything about a *person* from their name.** This detects
  language, not nationality, ethnicity, or name origin. `Language::Italian`
  means "this text's linguistic signal matches Italian", never "this name sounds
  Italian".
- **Language detection inside a hot per-token loop.** Detect once per document.
- **Replacing `verbora-phonetics`.** `recommend()` only says which encoder to
  reach for.

## Quick example

```rust ignore
use verbora_language::{Language, PhoneticRecommendation, Script, StrategyBasis, detect_script, recommend};

fn main() {
    // Script detection needs no Cargo feature and never allocates.
    assert_eq!(detect_script("Müller"), Some(Script::Latin));
    assert_eq!(detect_script("Москва"), Some(Script::Cyrillic));

    // If you already know the language, `recommend` is a closed lookup —
    // no detector, no confidence, nothing to be uncertain about.
    let de = recommend(Language::German);
    assert_eq!(de.primary, Some(PhoneticRecommendation::Cologne));
    assert_eq!(de.basis, StrategyBasis::Named);
}
```

## Script detection

`detect_script` classifies the *writing system* a string is in — a majority
vote over each scalar's Unicode block, not a model. Pure, allocation-free and
deterministic, with no dependency beyond `std`: the same `&str` gives the same
answer on every call, thread and platform.

```rust ignore
use verbora_language::{Script, detect_script};

fn main() {
    assert_eq!(detect_script("hello world"), Some(Script::Latin));
    assert_eq!(detect_script("café müller"), Some(Script::Latin)); // diacritics included
    assert_eq!(detect_script("こんにちは"), Some(Script::Hiragana));
    assert_eq!(detect_script("日本語"), Some(Script::Han));
    assert_eq!(detect_script("العربية"), Some(Script::Arabic));

    // Letters in a script with no dedicated variant are `Other`, not an error.
    assert_eq!(detect_script("ภาษาไทย"), Some(Script::Other));
    // No classifiable letters at all -> None.
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
| `Other` | A letter in a script with no dedicated variant — not an error |

**`Other` and `None` are different answers.** `Other` means "there are letters
here, in a script this crate models no language for"; `None` means "there are no
letters here at all". A caller routing to a language-specific pipeline wants to
tell them apart.

Only alphabetic scalars vote. Digits, punctuation, whitespace, symbols, emoji
and non-alphabetic combining marks are script-neutral — including the ones that
sit *inside* a script's block, such as `×` (U+00D7, in Latin-1 Supplement) and
the katakana middle dot `・`. `Other` is counted separately and needs a **strict**
majority over every named script to win, because it is the residual class rather
than a script: two letters in it may not even share a writing system.

**Ties between named scripts go to whichever of the tied scripts the text opens
with.** `detect_script("aЖ")` is `Latin` and `detect_script("Жa")` is `Cyrillic`.
That rule is a property of the text, which is the point: breaking ties by a fixed
order over the enum's variants would make the answer depend on the order the
variants happen to be declared in, so adding one or sorting the list would
silently change results for real input. A tie is a statement that the evidence is
balanced — a caller who cannot act on a coin flip should count the scripts
themselves rather than ask this function to invent a preference.

Script detection is reliable on short input precisely because it asks a coarser
question: knowing a word is Cyrillic does not tell you Russian from Ukrainian,
but it rules out every Latin-script language at almost no cost.

## Language detection

`LanguageDetector` is the abstraction — one method, `detect`, implemented by the
detectors below and by anything you write yourself. The trait, `Language`,
`Script`/`detect_script`, `recommend`, `FallbackDetector` and
`AutoPhoneticStrategy<D>` all compile with **zero** extra dependencies; only a
real detector needs one.

Every implementor owes the same contract: `detect` is **total** (no panic, no
error, for any `&str`), **deterministic**, **pure** (`&self`, no observable
mutation — which is what lets `par_detect_batch` share one detector across
threads), and it **abstains honestly**, returning an empty `LanguageDetection`
rather than the least-bad guess when there is no usable signal. A caller can act
on "I don't know"; it cannot act on a guess that looks like knowledge.

```rust ignore
use verbora_language::{Confidence, Language, LanguageDetection, LanguageDetector};

struct AlwaysGerman;

impl LanguageDetector for AlwaysGerman {
    fn detect(&self, _input: &str) -> LanguageDetection {
        LanguageDetection::single(
            Language::German,
            Confidence::new(0.42).expect("0.42 is in range"),
        )
    }
}

fn main() {
    let detection = AlwaysGerman.detect("anything");
    let half = Confidence::new(0.5).unwrap();
    let low = Confidence::new(0.3).unwrap();

    // `best()` ignores confidence entirely — it returns the top candidate
    // no matter how low its confidence is.
    assert_eq!(detection.best().unwrap().language, Language::German);

    // `best_above()` is what a caller should actually gate on.
    assert_eq!(detection.best_above(half), None);  // 0.42 < 0.5
    assert!(detection.best_above(low).is_some());  // 0.42 >= 0.3
}
```

**There is no built-in default threshold anywhere in this crate.** What counts
as "confident enough" depends on your tolerance for a wrong guess, on the input
length, and on which detector produced the number. You pick it; `best_above`
enforces it, with `>=` so a threshold met exactly passes.

### `Confidence` is a type, not an `f32`

A bare `f32` confidence has two values that make a detection meaningless: `NaN`,
which is neither above nor below any threshold and so silently turns every
comparison false, and anything outside `0.0..=1.0`, which makes "confidence" mean
nothing at all. Both are unrepresentable: `Confidence::new` is the only way in
from a float and returns `None` for either, `-0.0` normalises to `0.0` so zero has
one bit pattern, and `Confidence` is therefore `Ord`. Sorting candidates and
comparing against a threshold are total operations — no `unwrap`, no
`partial_cmp` fallback, no `NaN` escape.

**What the number means is the detector's business.** Confidence values from two
different `LanguageDetector` implementations are not comparable, and this crate
does not normalize across detectors. What *is* guaranteed everywhere is the
direction: within one detector, higher means more sure.

### Which detector

| Detector | Feature | What it is |
|---|---|---|
| `WhatlangDetector` | `language-detection` | The default. A zero-sized detector over compiled-in n-gram frequency tables, covering 20 of this crate's 22 languages (Galician and Basque are the gaps). |
| `HashedLinearDetector` | `fast-language-detection` | A linear model over hashed character features, with compiled-in weight tables and no extra dependency. Cheaper per call; less accurate on input shorter than a sentence. |
| `FallbackDetector<P, S>` | none | Pure composition: run `P`, and consult `S` **only when `P` returned no candidate at all**. |

`DefaultDetector` is a type alias for `WhatlangDetector` — the detector this
workspace means when it says "Verbora language detection" without naming one.

Measured on the published 13-language × 4-tier UDHR evaluation set, which
`tests/default_detector.rs` re-scores as an executed test rather than a quoted
claim:

| | short_word | short_phrase | sentence | paragraph | overall |
|---|---|---|---|---|---|
| `WhatlangDetector` (**default**) | 10/13 | 13/13 | 13/13 | 13/13 | **49/52** |
| `HashedLinearDetector` | 7/13 | 12/13 | 13/13 | 13/13 | 45/52 |
| `FallbackDetector<Hashed, Whatlang>` | 10/13 | 13/13 | 13/13 | 13/13 | **49/52** |

**Why the cheapest detector is deliberately not the default.**
`HashedLinearDetector` costs 4 of those 52 items, all on input shorter than a
sentence. Shipping it as the default would make the headline claim about speed
and leave the accuracy cost as fine print, so it stays opt-in, first-class and
separately labelled. `FallbackDetector` is the composition that buys the speed
back without the trade: 52 items is enough to show it recovers the abstentions
that cost the fast detector its short-input accuracy, and **not** enough to claim
general accuracy parity — it does not fix a *wrong* answer from the primary, only
an absent one. A caller who needs the most accurate answer this crate has on
hard, short input should use `WhatlangDetector` directly.

Two documented limitations worth knowing before choosing: `HashedLinearDetector`
abstains on Arabic-script input rather than mislabel it, because Arabic script is
shared by Arabic, Persian and Urdu and only Persian is a `Language` variant — so
`Persian` is the one language it can never return. `WhatlangDetector`'s trigram
profiles do distinguish Arabic-script languages.

### The reliability signal

`WhatlangDetector`'s confidence is a relative-margin score — how much better the
winner scored than the runner-up — not a calibrated probability. It is reported
as-is, with one Verbora-defined adjustment: when the underlying model's own
reliability signal says the result is not trustworthy, the confidence is
**halved**. Halving rather than dropping the candidate keeps the finding visible
to a caller who wants to see it, while pushing it below any threshold a caller
would plausibly act on. What the factor is chosen to preserve is the *ordering* —
an unreliable result can never outrank a reliable one of the same raw score — and
that ordering, not the constant, is what the crate's tests pin. A confidence
reported outside `0.0..=1.0`, or as `NaN`, is an abstention rather than a
meaningless number passed on.

## Confidence and ambiguity

Language detection is probabilistic, and this is why the rest of the crate is
shaped the way it is: `LanguageDetection` can be empty, `best_above` makes *you*
decide what counts as confident, and `AutoPhoneticStrategy` never recommends a
strategy your threshold wouldn't clear.

`tests/ambiguity.rs` pins the honest behaviour, as executed tests rather than
prose. It never asserts a specific language for ambiguous input — that would be
the false-confidence bug it exists to prevent. What it asserts is:

- **Common cross-language vocabulary detected alone must not clear a normal
  threshold.** `"hotel"`, `"radio"`, `"piano"`, `"normal"` and `"color"` are
  orthographically valid, common words in at least English and Spanish, and
  several more; none of them may resolve to a single confident language.
- **Short proper names must not either.** `"Panichella"`, `"Mueller"`,
  `"Kenji"`, `"Marie"` and `"Ivan"` are the direct test of "names are not
  language": a name with a clear national association carries almost none of the
  signal a statistical detector scores, and appearing in running text is a
  different claim from *being* that text's language.
- **A long, unambiguous sentence must clear the threshold.** Otherwise
  "ambiguous input is uncertain" would be indistinguishable from "detection is
  broken".
- **Empty input reports zero candidates**, not a guess.

```rust ignore
use verbora_language::{AutoPhoneticStrategy, Confidence, WhatlangDetector};

fn main() {
    let threshold = Confidence::new(0.6).expect("0.6 is in range");
    let auto = AutoPhoneticStrategy::new(WhatlangDetector::new(), threshold);
    let result = auto.detect_and_recommend("hotel");

    // Below-threshold — or no candidate at all — never produces a strategy.
    assert!(result.strategy.is_none());

    // The raw detection is still there to inspect: "not confident enough to
    // act automatically" is not the same as "found nothing".
    match result.detection.best() {
        Some(c) => eprintln!("low-confidence guess: {:?} ({})", c.language, c.confidence),
        None => eprintln!("no signal at all"),
    }
}
```

## Phonetic strategy: `recommend()`

`recommend(language: Language) -> PhoneticStrategy` is a **closed lookup table**,
not a statistical process — once the language is known, there is nothing left to
be uncertain about. Verbora ships twelve phonetic encoders, and exactly one was
designed for a language other than English (Cologne, for German); two more are
grounded in something wider than one language. So "a strategy per language"
cannot mean "a different algorithm for each of twenty-two languages" the way it
would for stemming. What it means here is two facts a caller cannot get from the
encoders themselves: **which encoder, if any, was actually designed for this
language**, and **whether a transliteration step has to run first**.

```rust ignore
use verbora_language::{Language, PhoneticRecommendation, StrategyBasis, TransliterationAdvice, recommend};

fn main() {
    let de = recommend(Language::German);
    assert_eq!(de.primary, Some(PhoneticRecommendation::Cologne));
    assert_eq!(de.basis, StrategyBasis::Named);
    assert_eq!(de.transliteration, TransliterationAdvice::NotNeeded);

    // It runs; it wasn't designed for it — and the type says so.
    assert_eq!(recommend(Language::Finnish).basis, StrategyBasis::Script);

    // Nothing fits, and nothing is invented.
    let zh = recommend(Language::Chinese);
    assert_eq!(zh.basis, StrategyBasis::NoFit);
    assert!(zh.primary.is_none());
    assert!(zh.alternatives.is_empty());
}
```

| Field | Type | Meaning |
|---|---|---|
| `primary` | `Option<PhoneticRecommendation>` | The best-fit encoder. `None` exactly when `basis` is `NoFit`. |
| `alternatives` | `&'static [PhoneticRecommendation]` | Other legitimate choices, most useful first, never containing `primary`. Empty when `primary` is `None`. |
| `basis` | `StrategyBasis` | How firmly `primary` is grounded — see below. |
| `transliteration` | `TransliterationAdvice` | Whether a transliteration step should run first — see [Transliteration integration](#transliteration-integration). |

`PhoneticStrategy` is `Copy` and allocation-free: `alternatives` borrows the
static table `recommend` is compiled from rather than building a `Vec` per call.
`encoders()` yields the primary followed by the alternatives, for a caller that
wants to try each in turn without special-casing the first.

### `StrategyBasis` — the distinction the crate exists for

| Variant | Meaning |
|---|---|
| `Named` | The primary encoder's own publication or rule corpus **names this language**. The strongest claim this crate makes. |
| `Script` | No encoder names this language. The recommendation follows from the script or alphabet it is written in: the encoder will read the text and produce a stable key, but it was not tuned for this language's phonology. |
| `NoFit` | Nothing in Verbora fits. `primary` is `None` and `alternatives` is empty — match on this and fall back to a different technique. |

Recommending Double Metaphone for Finnish and recommending Cologne for German are
not the same kind of statement, and collapsing them into one
`Option<PhoneticRecommendation>` would hide exactly the uncertainty a caller needs
in order to decide whether to trust a phonetic key at all.

| Language | Primary | Basis |
|---|---|---|
| German | `Cologne` | `Named` |
| English | `DoubleMetaphone` | `Named` |
| Polish | `DaitchMokotoff` | `Named` |
| Dutch, French, Italian, Spanish, Portuguese | `BeiderMorse` (that language's own rule table) | `Named` |
| Russian, Ukrainian | `BeiderMorse` (`cyrillic`) | `Script` |
| Persian | `BeiderMorse` (`arabic`) | `Script` |
| Norwegian, Swedish, Finnish, Galician, Catalan, Basque, Indonesian, Vietnamese | `DoubleMetaphone` | `Script` |
| Japanese | `DoubleMetaphone`, after transliteration | `Script` |
| Hindi, Chinese | — | `NoFit` |

The Hindi and Chinese rows are the point of the whole type. Naming an encoder for
them would produce a key — every encoder is total, so something always comes
back — and that key would be worthless, because no rule in any of them mentions a
Devanagari or Han character. A recommendation that cannot be honoured is exactly
the false confidence this module exists to avoid.

`BeiderMorse` is the one encoder in the workspace whose rule corpus reads native
scripts directly, which is why it, and not a Latin-alphabet encoder, is what
`recommend` names for Cyrillic and Arabic script. Its variant carries the language
tag to pass straight to the encoder. The per-encoder detail lives on
[Phonetics § Choosing a Phonetic Algorithm](phonetics.md#choosing-a-phonetic-algorithm).

### Coarser: `recommend_for_script`

For callers who only ran [`detect_script`](#script-detection),
`recommend_for_script(script: Script) -> PhoneticStrategy` gives the same shape of
answer, coarser: a whole script maps to one strategy, so its answers carry
`StrategyBasis::Script` at best and never `Named`.

```rust ignore
use verbora_language::{PhoneticRecommendation, Script, StrategyBasis, TransliterationAdvice, recommend_for_script};

fn main() {
    let latin = recommend_for_script(Script::Latin);
    assert_eq!(latin.primary, Some(PhoneticRecommendation::DoubleMetaphone));

    // Beider-Morse reads Cyrillic natively, so no transliteration is needed —
    // but this is a script-level claim, not a language-level one.
    let cyrillic = recommend_for_script(Script::Cyrillic);
    assert_eq!(cyrillic.basis, StrategyBasis::Script);
    assert_eq!(cyrillic.transliteration, TransliterationAdvice::NotNeeded);

    // Han is ambiguous between Chinese and Japanese kanji, so nothing fits.
    assert_eq!(recommend_for_script(Script::Han).basis, StrategyBasis::NoFit);
}
```

Kana (`Hiragana`, `Katakana`) is unambiguously Japanese and does get the
transliteration step. `Han`, `Hangul`, `Devanagari` and `Other` get `NoFit`:
guessing Japanese from Han alone would be a language claim this function has no
evidence for. A caller who can determine the real `Language` should always prefer
`recommend`.

## Transliteration integration

Not every encoder reads every script, and applying one to text it cannot read
mostly doesn't fail — it quietly produces a key with no phonetic meaning.
`TransliterationAdvice` says what to do about that:

| Variant | Meaning |
|---|---|
| `NotNeeded` | The primary encoder reads this text's own script directly. Feed it the text as it stands. |
| `Recommended` | Verbora has a transliterator for this script and the primary encoder needs it: run `apply_transliteration` first. |
| `Unsupported` | The primary needs romanized input and Verbora has no transliterator for this script — or nothing fits at all. |

```rust ignore
use verbora_language::{Language, TransliterationAdvice, apply_transliteration, recommend};

fn main() {
    // Japanese: Recommended -- runs verbora_transliterators::transliterate_ja.
    let ja = recommend(Language::Japanese);
    assert_eq!(ja.transliteration, TransliterationAdvice::Recommended);
    assert_eq!(apply_transliteration(ja.transliteration, "にほん"), "nihon");

    // Russian: nothing to transliterate -- the primary reads Cyrillic itself.
    let ru = recommend(Language::Russian);
    assert_eq!(ru.transliteration, TransliterationAdvice::NotNeeded);
    assert_eq!(apply_transliteration(ru.transliteration, "Москва"), "Москва");
}
```

`apply_transliteration` returns `input` unchanged — as `Cow::Borrowed`, with no
allocation — for **both** `NotNeeded` and `Unsupported`. That is deliberate and is
not a silent success: `Unsupported` means there is no romanization to offer, so
there is nothing the function could honestly do. Your own `match` on `advice` is
what should decide whether to trust the phonetic step that follows.

Today exactly one language gets `Recommended`:

| Language(s) | Advice | What it means |
|---|---|---|
| Every Latin-alphabet language | `NotNeeded` | Nothing to transliterate. |
| Russian, Ukrainian, Persian | `NotNeeded` | The primary encoder reads Cyrillic or Arabic script directly. |
| Japanese | `Recommended` | [`transliterate_ja`](transliterators.md) romanizes kana and kanji into a form the Latin-alphabet encoders can work with. |
| Hindi, Chinese | `Unsupported` | Nothing fits: no encoder reads Devanagari or Han, and no Verbora transliterator romanizes them. |

## Batch and parallel detection

Behind the `parallel` Cargo feature — independent of any detector feature, as it
is generic over any `D: LanguageDetector + Sync` —
`par_detect_batch(detector, texts) -> Vec<LanguageDetection>` fans a batch of
independent texts across a `rayon` pool, one `detect()` per text, preserving input
order. Its body is `par_iter().map(detect).collect()` over the same `detect` the
sequential path calls, so it cannot drift out of sync with single-text behaviour;
an equivalence test asserts it against the plain loop.

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

`texts` is any slice of things that borrow as `&str`, so `&[&str]`, `&[String]`
and `&[Cow<'_, str>]` all work without a conversion pass. The detector is
borrowed, not consumed — one instance serves every thread, which is why `D` must
be `Sync`; both detectors this crate ships are zero-sized and stateless, so they
satisfy it automatically.

This is for a large corpus of realistic-length texts — one guess per document,
thousands of times over — not for detecting a single text faster. Turning the
feature on changes nothing about `detect`: only this function ever touches
`rayon`.

## Performance characteristics

<div class="callout callout-note">
<strong>No timing figures are published for this crate.</strong> No measurement
describes its detectors, its <code>recommend</code> path or its parallel batch as
they now stand. <code>benches/language.rs</code> measures script detection, each
detector, <code>recommend</code> and the <code>par_batch</code> crossover;
measurement is pending, and this crate publishes no number it has not measured
against the code as it is.
</div>

What can be stated without measuring is the shape of the work:

- **`detect_script`** is one pass over the input, plus a second pass only when a
  tie actually occurs. ASCII bytes are counted straight off the byte slice with
  no UTF-8 decoding, and non-ASCII runs are walked with one `chars()` iterator
  per run, each scalar re-testing the block that matched the previous one before
  falling back to the block table. Linear in input length, with no jump at any
  size.
- **`recommend`** is a `match` over 22 arms returning `Copy` data.
- **A statistical `detect`** evaluates a model over the whole input. That is
  orders of magnitude more work than either of the above, which is why the
  explicit path exists.

**Memory and initialization.** Every model this crate can use is compiled in:
`WhatlangDetector`'s n-gram frequency tables and `HashedLinearDetector`'s weight
tables are `static` arrays in read-only data — no runtime load, no file I/O, no
deserialization, nothing to warm up. Both detector types are zero-sized, `Copy`
and automatically `Send + Sync`, with no `unsafe impl` anywhere, and both `new()`
functions are `const fn` with nothing to construct.

## Allocation behaviour

| Operation | Allocates |
|---|---|
| `detect_script` / `Script::of` | Nothing — a fixed-size stack array and a scan over the input |
| `recommend` / `recommend_for_script` | Nothing: `PhoneticStrategy` is `Copy` and `alternatives` is a `&'static` slice |
| `LanguageDetector::detect` | Whatever the detector's own model evaluation needs, plus at most one single-element `Vec` for the candidate — `LanguageDetection::none()` allocates nothing |
| `AutoPhoneticStrategy::detect_and_recommend` | Whatever the wrapped detector allocates; the recommendation itself adds nothing |
| `apply_transliteration` | Nothing for `NotNeeded`/`Unsupported` (`Cow::Borrowed`); whatever `transliterate_ja` allocates for `Recommended` |
| `par_detect_batch` | One output `Vec<LanguageDetection>`, plus whatever each `detect` allocates — no per-chunk buffering |

## Common mistakes

**Trusting `best()` instead of `best_above()` on short input.** `best()` ignores
confidence entirely. Gate on a threshold you chose, especially for anything
shorter than a full sentence.

**Comparing two detectors' confidences.** They are not on the same scale, and
this crate does not normalize across them. Only the direction is guaranteed:
within one detector, higher means more sure.

**Treating a name's national or ethnic association as a language
determination.** A surname can have Italian origins and appear in an English
sentence with no contradiction.

**Calling a statistical `detect` in a hot per-token loop.** Detect once per
document, and use `par_detect_batch` for a real batch.

**Assuming `recommend()`'s `alternatives` are unranked.** They are ordered, most
useful first — and they never repeat `primary`.

**Confusing `NoFit` with a bug.** It is deliberate: there is no encoder that
fits, and one would be invented only by lying about it.

## Related

- [Phonetics](phonetics.md) — the encoders `recommend()` chooses between, and
  the [per-language table](phonetics.md#choosing-a-phonetic-algorithm) it backs
  directly.
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
| `Language` | `#[non_exhaustive]` enum, 22 variants. `ALL`, `iso639_1()`, `name()`, `from_iso639_1()`, `FromStr`, `Display` |
| `Script` | `#[non_exhaustive]` enum, 11 variants. `Script::of(char)`, `Display` |
| `Confidence` | `0.0..=1.0`, never `NaN`. `ZERO`, `CERTAIN`, `new(f32) -> Option<Self>`, `get()`, `halved()`, `Ord` |
| `LanguageDetector` | Trait: `fn detect(&self, input: &str) -> LanguageDetection` |
| `LanguageCandidate` | `{ language: Language, confidence: Confidence }` |
| `LanguageDetection` | Candidates in descending confidence, held privately so the ordering is an invariant rather than a convention |
| `WhatlangDetector` (feature `language-detection`) | Zero-sized `LanguageDetector`. `Copy`, `Clone`, `Default`, `const fn new()` |
| `HashedLinearDetector` (feature `fast-language-detection`) | Zero-sized `LanguageDetector` over compiled-in weight tables. `Copy`, `Clone`, `Default`, `const fn new()` |
| `DefaultDetector` (feature `language-detection`) | Type alias for `WhatlangDetector` |
| `FallbackDetector<P, S>` | Runs `P`; consults `S` only when `P` abstains |
| `AutoPhoneticStrategy<D>` | Combines a `D: LanguageDetector` with `recommend`, gated by a threshold |
| `AutoResult` | `{ detection: LanguageDetection, strategy: Option<PhoneticStrategy> }` |
| `PhoneticRecommendation` | `SoundEx`, `Metaphone`, `DoubleMetaphone`, `DaitchMokotoff`, `Cologne`, `BeiderMorse { language: &'static str }` |
| `StrategyBasis` | `Named`, `Script`, `NoFit` |
| `TransliterationAdvice` | `NotNeeded`, `Recommended`, `Unsupported` |
| `PhoneticStrategy` | `Copy`. `{ primary, alternatives: &'static [PhoneticRecommendation], basis, transliteration }` |
| `ParseLanguageError` | `Language::from_str` failure. Implements `std::error::Error` |

### Methods and functions

| Item | Signature |
|---|---|
| `Language::ALL` | `const [Language; 22]` |
| `Language::iso639_1` | `(self) -> &'static str` |
| `Language::name` | `(self) -> &'static str` |
| `Language::from_iso639_1` | `(code: &str) -> Option<Self>` (case-insensitive) |
| `detect_script` | `(input: &str) -> Option<Script>` |
| `Script::of` | `(c: char) -> Option<Script>` |
| `Confidence::new` | `(value: f32) -> Option<Confidence>` |
| `LanguageDetection::none` | `() -> Self` |
| `LanguageDetection::single` | `(language: Language, confidence: Confidence) -> Self` |
| `LanguageDetection::ranked` | `(impl IntoIterator<Item = LanguageCandidate>) -> Self` |
| `LanguageDetection::candidates` | `(&self) -> &[LanguageCandidate]` |
| `LanguageDetection::best` | `(&self) -> Option<&LanguageCandidate>` |
| `LanguageDetection::best_above` | `(&self, threshold: Confidence) -> Option<&LanguageCandidate>` |
| `AutoPhoneticStrategy::new` | `(detector: D, threshold: Confidence) -> Self` |
| `AutoPhoneticStrategy::detect_and_recommend` | `(&self, input: &str) -> AutoResult` |
| `PhoneticStrategy::encoders` | `(&self) -> impl Iterator<Item = PhoneticRecommendation>` |
| `recommend` | `(language: Language) -> PhoneticStrategy` |
| `recommend_for_script` | `(script: Script) -> PhoneticStrategy` |
| `apply_transliteration` | `(advice: TransliterationAdvice, input: &str) -> Cow<'_, str>` |
| `par_detect_batch` (feature `parallel`) | `<D: LanguageDetector + Sync, S: AsRef<str> + Sync>(detector: &D, texts: &[S]) -> Vec<LanguageDetection>` |
