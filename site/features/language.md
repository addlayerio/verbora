# Language and script detection

`verbora-language` answers a question [`verbora-phonetics`](phonetics.md) cannot
answer on its own: *given a word or a document, which of Verbora's four phonetic
encoders should even be used?* It answers that by keeping three layers
deliberately separate — cheap, allocation-free [script detection](#script-detection);
optional, statistical [language detection](#language-detection) behind a real
detector; and a closed, non-statistical
[lookup from language to phonetic strategy](#phonetic-strategy-recommend) — composed
only at the edge, and only if you ask for that composition.

<div class="callout callout-note">
<strong>Verbora-native extension — not a ported feature.</strong>
<code>verbora-language</code> has no counterpart in the reference. What follows is
backed by this workspace's own evidence: 33 unit tests
with zero Cargo features enabled, growing to 40 unit tests plus the 8
ambiguity tests in <code>tests/ambiguity.rs</code> with every feature
enabled, and the Criterion benchmarks in
<code>crates/verbora-language/benches/language.rs</code>. See
<a href="phonetics">Phonetics</a> and <a href="phonetic-index">Phonetic
neighbors</a> for the four tested encoders this crate recommends
between — it never replaces them, and never encodes anything itself.
</div>

## Explicit vs. automatic

**This is the decision that matters most on this page** — more than any
individual API below, and it is deliberately the first thing after the
extension notice. `AutoPhoneticStrategy` composes the pieces further down
this page for you, but it is an opt-in composition, not the default path,
and knowing when to skip it entirely is worth more than any detector's
accuracy number.

```text
Do you already know the language — a locale setting, a per-record field,
a user's own language picker?
│
├── YES → call `recommend(Language::X)` directly.
│         A closed lookup table, not a guess — see Quick example below.
│
└── NO → how much text do you actually have?
    │
    ├── A paragraph or more → `WhatlangDetector` / `AutoPhoneticStrategy`
    │   may help — see Language detection below. Still gate on
    │   `best_above(threshold)`, never bare `best()`.
    │
    └── One short word, or a name → do NOT blindly trust automatic
        language detection. Inspect `LanguageDetection.candidates`
        yourself, or require a high `best_above()` threshold and treat
        `None` as the expected, correct answer — see Confidence and
        ambiguity below.
```

How much does "just detect it automatically" cost compared to already
knowing? `recommend(Language::German)` alone — a closed `match` over 22
arms, no detector involved — measured at **5.67–7.48 ns** in this crate's
own benchmark suite (`manual_path`; 6.33–6.42 ns in the all-features run).
`AutoPhoneticStrategy::detect_and_recommend` on the *same* language measured
28.09–28.12 µs for a short text, 67.73–67.86 µs for a paragraph, and
147.76–147.98 µs for a long document (`auto_end_to_end`, same report) —
roughly **4,260× to 22,400× more expensive**, depending on how much text
there is to work with. That is not an argument against automatic
detection — it exists precisely so a caller without a language hint has
somewhere to turn, and it is worth every one of those microseconds when a
caller genuinely doesn't know the language. It *is* the argument for not
reaching for it when you already do.

Restated as the three cases this crate's own design and tests hold to:

- **If you know the language** — a locale setting, a per-record field, a
  user's own language picker — call `recommend(Language::X)` directly. See
  [Quick example](#quick-example).
- **If you have enough context** — a paragraph, a document, anything long
  enough to carry real statistical signal — `WhatlangDetector` /
  `AutoPhoneticStrategy` can help. See [Language detection](#language-detection).
- **If you only have one short word or a name**, do not blindly trust
  automatic language detection. Either inspect `LanguageDetection.candidates`
  yourself, or require a `best_above()` threshold high enough that `None`
  being the normal outcome for most single words doesn't surprise you. See
  [Confidence and ambiguity](#confidence-and-ambiguity) — the single most
  important section on this page.

## When to use it

- **You have text and no reliable language hint**, and need one before
  picking a phonetic strategy, a tokenizer variant, or a stemmer.
  `detect_script` is nearly free and should run first regardless of how
  much text you have; `WhatlangDetector` earns its cost once you have at
  least a full sentence.
- **You're processing many independent texts** (reviews, support tickets,
  log lines) where each needs its own language guess. `par_detect_batch`
  (feature `parallel`) fans this out across a thread pool — see
  [Performance characteristics](#performance-characteristics).
- **You want a phonetic-strategy recommendation without hand-writing a
  per-language `match` yourself.** `recommend(language)` is exactly that
  lookup table, extracted once so every caller doesn't reimplement it —
  see [Phonetic strategy: `recommend()`](#phonetic-strategy-recommend).

## When not to use it

- **Detecting the language of a single word or a short name, and trusting
  the answer.** See [Confidence and ambiguity](#confidence-and-ambiguity) —
  this is not a corner case glossed over here, it is a documented, tested
  property of the problem itself.
- **Inferring anything about a *person* from their name.** This crate
  detects language, not nationality, ethnicity, or name origin — see
  ["Names are not language"](#names-are-not-language) below. Using it to
  answer "what is this name's origin" is a misuse its own test suite
  actively guards against.
- **Language detection inside a hot per-token loop.** `WhatlangDetector::detect`
  costs tens to low hundreds of *microseconds* (see
  [Performance characteristics](#performance-characteristics)) — orders of
  magnitude more than any single phonetic-encoder call. Detect once per
  document, not once per token.
- **Replacing `verbora-phonetics`.** This crate never encodes anything
  itself; `recommend()` only says which of `verbora-phonetics`'s four
  encoders to reach for.

## Quick example

<div class="callout callout-note">
<strong>Note.</strong> Every Rust block on this page is marked
<code>ignore</code>, including the ones below that need no Cargo feature at
all. <code>verbora-examples</code> — the crate this site's own snippet
checker, <code>check-snippets.py</code>, compiles every non-<code>ignore</code>d
block against — does not (yet) carry <code>verbora-language</code> as a
dependency, so nothing on this page is compiled by that check today. Every
block was still compiled and run by hand against the real crate, with
<code>--features language-detection</code> where a block needed it, before
publication — the real output from those runs is quoted alongside the
blocks that produce non-obvious results (see
<a href="#language-detection">Language detection</a> and
<a href="#confidence-and-ambiguity">Confidence and ambiguity</a> in
particular). Wiring <code>verbora-language</code> into
<code>verbora-examples</code> so these blocks compile in CI like every
other feature page's do is tracked as follow-up work, not done here.
</div>

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
vote over each character's Unicode block, not a model, and not the same
question as "what language is this." It needs no Cargo feature and no
crate dependency beyond `std`: one pass over `input.chars()`, a fixed
10-element counting array on the stack, no allocation at any input length.

Script detection is more reliable than language detection on short input
precisely because it asks a coarser question. Knowing a word is written in
Cyrillic does not tell you whether it is Russian or Ukrainian — but it
rules out every Latin-script language in `Language::ALL` at zero cost,
before spending anything on statistical detection. That trade — cheap and
reliable at the coarse grain it operates at, uninformative at the fine
grain only a real language model can resolve — is exactly why
[the crate-level design](#explicit-vs-automatic) always has script
detection cost nothing while language detection stays behind a feature
flag.

```rust ignore
use verbora_language::{Script, detect_script};

fn main() {
    assert_eq!(detect_script("hello world"), Some(Script::Latin));
    assert_eq!(detect_script("café müller"), Some(Script::Latin)); // diacritics included
    assert_eq!(detect_script("こんにちは"), Some(Script::Hiragana));
    assert_eq!(detect_script("日本語"), Some(Script::Han)); // 3 Han chars outvote nothing else
    assert_eq!(detect_script("العربية"), Some(Script::Arabic));

    // No classifiable letters at all -> None. Not an error; plenty of real
    // text is script-neutral.
    assert_eq!(detect_script("123 !@# ..."), None);
}
```

`Script` has 11 variants, mapped from Unicode ranges:

| `Script` | What maps to it |
|---|---|
| `Latin` | Every Latin-script language `Language` enumerates — English, Spanish, French, … — diacritics included (Vietnamese's Latin Extended Additional range too) |
| `Cyrillic` | Russian, Ukrainian, and other Cyrillic-script text |
| `Greek` | Modern Greek |
| `Arabic` | Arabic and other Arabic-script text |
| `Hebrew` | Hebrew |
| `Han` | CJK ideographs — Chinese, and the kanji portion of Japanese |
| `Hiragana` / `Katakana` | Japanese kana |
| `Hangul` | Korean (no `Language` variant covers Korean today — see [`Language`](#api-reference)'s own scope) |
| `Devanagari` | Hindi and other Devanagari-script text |
| `Other` | A script this classifier has no dedicated variant for — not an error |

Mixed-script input (a loan word, a foreign proper noun) returns whichever
script has the most characters; ties break toward the first script checked,
not toward whichever appeared first in the input. `detect_script` returns
`None` only when there is nothing classifiable at all — empty input, or
purely digits/punctuation/whitespace.

For callers who only have a script and not a full language guess,
[`recommend_for_script`](#phonetic-strategy-recommend) gives the same shape
of answer `recommend` does, just coarser — see
[Phonetic strategy: `recommend()`](#phonetic-strategy-recommend) below.

## Language detection

`LanguageDetector` is the detection abstraction — one method, implemented by
[`WhatlangDetector`](#whatlangdetector) and by anything you write yourself:

```rust ignore
use verbora_language::{Language, LanguageCandidate, LanguageDetection, LanguageDetector};

/// A trivial detector, entirely real — no `language-detection` feature
/// needed, because the trait itself has zero dependencies.
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
    assert_eq!(detection.best().unwrap().language, Language::German);
}
```

`verbora-language`'s own core API — `Language`, `Script`/`detect_script`,
`recommend`, the `LanguageDetector` trait itself, and `AutoPhoneticStrategy<D>`
(generic over any `D: LanguageDetector`) — compiles and works with **zero**
extra dependencies. Only a *real* detector needs one.

### `WhatlangDetector`

Behind the `language-detection` Cargo feature (`whatlang = { version = "0.18",
optional = true }`), `WhatlangDetector` is a zero-sized `LanguageDetector`
backed by `whatlang`'s n-gram frequency model. It was chosen over `lingua`
and `whichlang` — the other two actively maintained Rust language-detection
crates — after a real comparison:

| | `whatlang` | `lingua` | `whichlang` |
|---|---|---|---|
| License | MIT | Apache-2.0 | MIT |
| Dependencies | 1 (`hashbrown`) | ~15, incl. `rayon`, `dashmap`, per-language model crates | 0 |
| Coverage of this crate's 22 languages | 20/22 (missing Galician, Basque) | 21/22 (missing Galician) | 13/22 |
| Honest low-confidence signal | `is_reliable()` | self-reported accuracy tables only | none |
| Footprint | ~685 KB compiled-in frequency tables | up to ~300 MB of per-language models if all enabled | ~775 KB, baked-in weights |

`whichlang` doesn't cover enough of this crate's language list to be
useful; `lingua`'s dependency graph is disproportionate to "guess the
language of a short phrase" and conflicts with this workspace's
dependency-light stance. `whatlang` is the one candidate that is
simultaneously MIT-licensed, nearly dependency-free, covers the language
list, and — critically — already exposes a reliability signal instead of
forcing this crate to invent one.

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
    assert!(single_word.best().is_none()); // no usable signal at all — see below
}
```

### What `confidence` means

`whatlang`'s own `Info::confidence()` is a **relative-margin score** — how
much better the winning language scored than the runner-up — not a
calibrated probability. `WhatlangDetector` reports it as-is, scaled by
whether `whatlang` itself considers the result reliable: an unreliable
result's confidence is **halved, not hidden**, so a caller comparing
against their own threshold still sees *something* rather than a silently
vanished candidate, while `best_above()` with any reasonable threshold
still correctly rejects it.

`LanguageCandidate::confidence` is `0.0..=1.0`, but what it means depends on
the detector — **two different `LanguageDetector` implementations' confidence
values are not necessarily comparable to each other.** This crate does not
normalize across detectors; it reports what each one actually said.

Real numbers, from running `WhatlangDetector::detect` directly (single
words and short names, alone — no surrounding sentence):

| Input | `best()` |
|---|---|
| `"hotel"` | no candidate at all |
| `"radio"` | no candidate at all |
| `"piano"` | no candidate at all |
| `"normal"` | no candidate at all |
| `"color"` | no candidate at all |
| A long, unambiguous English sentence | `English`, confidence 1.000000 |
| The same sentence, in German | `German`, confidence 1.000000 |

Five common, genuinely cross-language words return **no candidate at all**
when detected alone — stronger than "low confidence," this is `whatlang`
finding no language with enough signal to report anything, so
`LanguageDetection::candidates` is empty outright. A full, unambiguous
sentence in either language clears the maximum score. That gap — nothing at
all for five characters versus perfect confidence for a full sentence — is
the same "long text is easier, single words are ambiguous" property this
crate's own tests assert; see
[Confidence and ambiguity](#confidence-and-ambiguity) next for the harder
case: names.

## Confidence and ambiguity

**Language detection is probabilistic**, and this section is the reason the
rest of this crate is built the way it is: `LanguageDetection` can be
empty, `LanguageDetection::best_above` makes *you* decide what "confident
enough" means, and neither `AutoPhoneticStrategy` nor anything else in this
crate silently recommends a strategy your own threshold wouldn't clear.

### `best()` versus `best_above(threshold)`

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

    // `best()` ignores confidence entirely -- it returns the top
    // candidate no matter how low its confidence is.
    assert_eq!(detection.best().unwrap().language, Language::German);

    // `best_above()` is what a caller should actually gate on.
    assert_eq!(detection.best_above(0.5), None);       // 0.42 < 0.5
    assert!(detection.best_above(0.3).is_some());       // 0.42 >= 0.3
}
```

**There is no built-in default threshold anywhere in this crate, by
design.** `LanguageDetection::best_above`'s own doc comment is explicit
about why: *"what counts as 'confident enough' depends on the caller's own
tolerance for a wrong guess, and this crate does not assume it knows that
for you."* `AutoPhoneticStrategy::new` says the same thing from the other
side: a threshold was measured to vary enough by input length that *"a
single baked-in number would either be too permissive for single words or
too conservative for full sentences."* Both are deliberate refusals to
invent a number this crate cannot honestly justify — see
[the honesty rules](../reference/docs-are-code.md#honesty-rules) this whole
site holds itself to.

### Names are not language

`Language::Italian` means "this text's linguistic signal matches Italian" —
never "this name sounds Italian." A surname can have Italian origins and
appear in an English sentence with no contradiction; the two are different
claims, and this crate answers only the first one. Nothing here infers
anything about a *person* from their name — it is a *language* detector, and
a bare name carries almost none of the trigram signal `whatlang` actually
scores.

Real numbers make this concrete better than the principle alone does. Each
of these is a short proper name, detected completely alone:

| Name | `best()` |
|---|---|
| `"Panichella"` | `Italian`, confidence ≈ 0.073 |
| `"Mueller"` | `Norwegian`, confidence ≈ 0.008 |
| `"Kenji"` | no candidate at all |
| `"Marie"` | `Polish`, confidence ≈ 0.000000 (indistinguishable from zero) |
| `"Ivan"` | `Dutch`, confidence ≈ 0.002 |

`"Mueller"` — a German-origin surname, by any reasonable person's
intuition — does not even come back as a guess of `German`. It comes back
as `Norwegian`, at a confidence low enough that any threshold above roughly
1% rejects it outright. That is the "names are not language" principle
made concrete: not merely "the detector isn't sure," but the detector's own
best guess disagreeing with the name's actual ethnic association, at a
confidence any sane `best_above()` call throws away. Trusting `best()`
alone on a name — instead of `best_above()` with a real threshold — is not
a hypothetical mistake; it is a specific, reproducible wrong answer.

`"Panichella"` does happen to score `Italian`, matching the name's real
origin — but at confidence ≈ 0.073, an order of magnitude below anything a
`best_above()` call should accept. A name having a national or ethnic
association is not the same claim as the text being written in that
language, and this crate's own confidence numbers, not just its prose, say
so.

### The crate's own ambiguity tests

`crates/verbora-language/tests/ambiguity.rs` pins exactly this behaviour.
Its own doc comment states the rule plainly, quoting `Fase 5 Language.md`'s
own wording directly: *"los tests no deben exigir un idioma específico si
lingüísticamente es ambiguo"* ("the tests must not require a specific
language if it is linguistically ambiguous") — asserting one language would
itself be the false-confidence bug the test module exists to prevent. What
it checks instead is that the *uncertainty is represented
honestly*: either no candidate at all, or a candidate whose confidence a
caller can actually reject at a normal threshold (the test module picks
`0.6` as one such threshold — a value it chooses for its own assertions,
never one this crate bakes in anywhere).

- `hotel`, `radio`, `piano`, `normal`, `color` — five words that are
  legitimate, common vocabulary in at least English and Spanish (several
  also in French/Italian/Portuguese) — must never resolve to a single
  confident language.
- `Panichella`, `Mueller`, `Kenji`, `Marie`, `Ivan` — short proper names,
  the direct test of "names are not language" above — must not make the
  detector confident either.
- The flip side: a long, grammatically complete, unambiguous sentence
  **must** clear the threshold. Otherwise "ambiguous input is honestly
  uncertain" would be indistinguishable from "detection is simply broken" —
  see the real 1.000000 confidence figures above.
- Empty input must report zero candidates, not a guess.

### Low-confidence handling, worked

```rust ignore
use verbora_language::{AutoPhoneticStrategy, WhatlangDetector};

fn main() {
    let auto = AutoPhoneticStrategy::new(WhatlangDetector::new(), 0.6);
    let result = auto.detect_and_recommend("hotel");

    // Below-threshold — or no candidate at all — never produces a
    // strategy. `AutoPhoneticStrategy` will not guess on your behalf.
    assert!(result.strategy.is_none());

    // The raw detection is still there to inspect: "not confident enough
    // to act automatically" is not the same as "found nothing".
    match result.detection.best() {
        Some(candidate) => {
            // Fall back to your own handling: ask the user, require more
            // text, or use a different technique entirely.
            eprintln!("low-confidence guess: {:?} ({:.3})", candidate.language, candidate.confidence);
        }
        None => eprintln!("no signal at all"), // this is the branch "hotel" actually takes
    }
}
```

Running the block above for real: `detect_and_recommend("hotel")` returns
`strategy: None` and `detection.best(): None` — `whatlang` found nothing to
report at all, so there is nothing to log even at low confidence, and no
strategy comes back. That is the honest answer for a five-letter word
shared across half of Western Europe's vocabulary, not a bug to work
around.

## Phonetic strategy: `recommend()`

`recommend(language: Language) -> PhoneticStrategy` is a **closed lookup
table**, not a statistical process — once the language is known, there is
nothing left to be uncertain about. It answers a narrower question than
"which language": given that Verbora's four phonetic encoders are all
English-oriented algorithms with no language-specific variants, *which one
is the closest fit for this language, and does a transliteration step need
to run first?*

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

`PhoneticStrategy` has three fields:

| Field | Type | Meaning |
|---|---|---|
| `primary` | `Option<PhoneticRecommendation>` | The best-fit encoder — `None` only when `transliteration` is `Unsupported` *and* no encoder was designed for the language's phonotactics either (Persian, Hindi, Chinese; see below). Recommending an encoder that cannot honestly do anything is exactly the false confidence this type exists to avoid. |
| `alternatives` | `Vec<PhoneticRecommendation>` | Other reasonable choices, in no particular ranking beyond "also legitimate" — always empty when `primary` is `None`. |
| `transliteration` | `TransliterationAdvice` | Whether a transliteration step should run before encoding — see [Transliteration integration](#transliteration-integration) below. |

`PhoneticRecommendation` names one of the four `verbora-phonetics` encoders
without depending on their types directly:

| `PhoneticRecommendation` variant | `verbora-phonetics` encoder |
|---|---|
| `SoundEx` | `SoundEx` |
| `Metaphone` | `Metaphone` |
| `DoubleMetaphone` | `DoubleMetaphone` |
| `SoundExDaitchMokotoff` | `SoundExDM` (Daitch–Mokotoff) |

The full language-by-language table — every one of `Language`'s 22
variants, with the `Recommended` / `Alternative` / `Not designed for this
language` categories — lives on
[Phonetics § Choosing a Phonetic Algorithm](phonetics.md#choosing-a-phonetic-algorithm)
rather than being duplicated here, so there is exactly one place it can go
stale.

### Coarser: `recommend_for_script`

For callers who only ran [`detect_script`](#script-detection) and don't
have — or don't want — a full language guess, `recommend_for_script(script:
Script) -> PhoneticStrategy` gives the same shape of answer, coarser: a
whole script maps to one strategy instead of one language.

```rust ignore
use verbora_language::{PhoneticRecommendation, Script, TransliterationAdvice, recommend_for_script};

fn main() {
    let latin = recommend_for_script(Script::Latin);
    assert_eq!(latin.primary, Some(PhoneticRecommendation::DoubleMetaphone));

    // Cyrillic gets no confident primary at the script level -- Russian
    // and Ukrainian resolve differently once the actual `Language` is
    // known (see the per-language table linked above), but `Script` alone
    // can't distinguish them.
    let cyrillic = recommend_for_script(Script::Cyrillic);
    assert_eq!(cyrillic.primary, None);
    assert_eq!(cyrillic.transliteration, TransliterationAdvice::Unsupported);
}
```

`Script::Han` is a deliberate special case: it is ambiguous between Chinese
and Japanese kanji, so `recommend_for_script` only advises transliteration
when the input is *unambiguously* Japanese (hiragana or katakana present
alongside the Han characters) — pure Han input gets
`TransliterationAdvice::Unsupported`, on the theory that a caller with pure
Han text should determine the actual language rather than have this coarse
fallback guess between two languages with very different encoders. A caller
who can determine the real `Language` should always prefer `recommend`
over `recommend_for_script` — this function exists for the case where that
determination genuinely isn't available.

## Transliteration integration

`TransliterationAdvice` is honest about a hard constraint: none of
Verbora's four phonetic encoders were designed for non-Latin scripts, and
applying one directly to un-transliterated text mostly doesn't fail — it
quietly produces a key that carries no real phonetic meaning (see
[Phonetics § Unicode and language notes](phonetics.md#unicode-and-language-notes)).
Three variants say what to do about that:

| Variant | Meaning |
|---|---|
| `NotNeeded` | Latin-script; none of the four encoders need a transliteration step first. |
| `Recommended` | A transliteration step exists and should run before encoding. |
| `Unsupported` | No Verbora transliteration path exists for this language's script. |

`apply_transliteration(advice: TransliterationAdvice, input: &str) -> Cow<'_, str>`
applies it:

```rust ignore
use verbora_language::{Language, TransliterationAdvice, apply_transliteration, recommend};

fn main() {
    // Japanese: Recommended -- runs verbora_transliterators::transliterate_ja.
    let strategy = recommend(Language::Japanese);
    assert_eq!(strategy.transliteration, TransliterationAdvice::Recommended);
    let romanized = apply_transliteration(strategy.transliteration, "にほん");
    assert_eq!(romanized, "nihon");

    // Russian: Unsupported -- the input passes through completely unchanged.
    let russian = recommend(Language::Russian);
    assert_eq!(russian.transliteration, TransliterationAdvice::Unsupported);
    assert_eq!(apply_transliteration(russian.transliteration, "Москва"), "Москва");
}
```

`apply_transliteration` returns `input` unchanged for **both** `NotNeeded`
and `Unsupported` — deliberately. The caller's own `match` on `advice` is
what should decide whether to trust the phonetic step that follows; this
function does not silently do nothing useful and let a caller assume it
handled the unsupported case for them.

**Today, exactly one language gets `Recommended`: Japanese**, via
[`verbora_transliterators::transliterate_ja`](transliterators.md). Every
other non-Latin-script language this crate has an opinion about is honestly
`Unsupported` — but "unsupported" means two different things depending on
the language:

| Language(s) | `TransliterationAdvice` | What it means here |
|---|---|---|
| Every Latin-script language `recommend()` covers | `NotNeeded` | Nothing to transliterate. |
| Japanese | `Recommended` | `transliterate_ja` romanizes kana/kanji into a form the encoders can actually work with. |
| Polish, Ukrainian, Russian | `Unsupported` | Verbora has **no Cyrillic transliterator** — but `SoundExDM` is still `primary`. That recommendation only becomes meaningful once *you* romanize the input yourself; `apply_transliteration` will not do it for you. |
| Persian, Hindi, Chinese | `Unsupported` | No Verbora transliterator for Arabic, Devanagari or Han script — **and no `primary` recommendation either**, because no encoder was designed for these phonotactics regardless of script (Chinese is also tonal, which none of the four model). |

That last row is the important distinction: Polish/Ukrainian/Russian's
`Unsupported` is a gap in *transliteration support*, with a real encoder
recommendation waiting on the other side of it. Persian/Hindi/Chinese's
`Unsupported` is paired with `primary: None` — there is no honest
recommendation to make at all, transliteration or not. See
[Phonetics § Choosing a Phonetic Algorithm](phonetics.md#choosing-a-phonetic-algorithm)
for that category spelled out as "Not designed for this language."

## Batch and parallel detection

Behind the `parallel` Cargo feature (independent of `language-detection` —
it is generic over any `D: LanguageDetector + Sync`, not tied to
`WhatlangDetector`), `par_detect_batch(detector: &D, texts: &[&str]) ->
Vec<LanguageDetection>` fans a batch of independent texts across a `rayon`
thread pool, one `detect()` call per text, preserving input order.

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

This exists for a caller holding a large, independent corpus — one language
guess per document, per review, per record — not for detecting a single
word or document faster. `Fase 5 Language.md`'s own specification is
explicit that Rayon must never become the *implementation* of detecting one
word's language, and the design agrees: a single `detect()` call does not
benefit from a thread pool. See
[Performance characteristics](#performance-characteristics) below for the
real crossover this project measured, and
[Parallelism](../performance/parallelism.md) for the same chunked-vs.-per-item
reasoning applied elsewhere in this workspace.

## Performance characteristics

<div class="callout callout-note">
<strong>Machine-dependent.</strong> Every number below came from
<code>cargo bench -p verbora-language --all-features</code> on one
development machine. Ratios and orders of magnitude should hold; exact
nanosecond/microsecond figures will not reproduce identically on different
hardware. See <code>crates/verbora-language/benches/language.rs</code>'s own
module documentation for the full methodology.
</div>

### Script detection cost

`detect_script`, at four input lengths:

| Input | Size | Time |
|---:|---:|---:|
| A single word | 6 B | 8.61–8.71 ns |
| A short sentence | 68 B | 64.74–66.89 ns |
| A paragraph | 523 B | 527.09–530.50 ns |
| A long document | 10,460 B | 8.80–8.88 µs |

Linear in input length, as the one-pass `chars()` scan implies — no jump at
any of the four sizes, because there is no allocation or model to amortize.

### Language detection cost (feature `language-detection`)

`WhatlangDetector::detect`, at three input lengths:

| Input | Time |
|---|---:|
| A short sentence | 27.972–28.003 µs |
| A paragraph | 67.900–68.026 µs |
| A long document | 147.77–148.03 µs |

Three to four orders of magnitude more expensive than script detection at
comparable lengths — the cost of a real statistical model versus a Unicode
range lookup.

### Strategy lookup cost

`recommend()`, called once per `Language::ALL` variant (22 total): **1.41–8.31
ns each.** `Persian`, `Hindi` and `Chinese` — `primary: None`, empty
`alternatives`, so no `Vec` allocation at all — are fastest, around 1.4–1.6
ns; every other language (which allocates a short `alternatives` `Vec`)
sits around 5.6–8.3 ns. All 22 back to back: **207.16–207.52 ns** — roughly
9.4 ns per language on average.

### Manual versus automatic path

This is the number behind [Explicit vs. automatic](#explicit-vs-automatic)
above, stated precisely: `recommend(Language::German)` alone measured
**5.67–7.48 ns** (6.33–6.42 ns with every feature enabled).
`AutoPhoneticStrategy::detect_and_recommend` on the same language measured
28.09–28.12 µs (short text), 67.73–67.86 µs (paragraph), and
147.76–147.98 µs (long document) in the same benchmark run. **The automatic
path costs roughly 4,260× to 22,400× more than the manual one**, depending
on how much text there is — a real, measured gap, not a hand-wave, and the
reason "if you know the language, say so" is this page's most prominent
guidance rather than a footnote.

### Batch and parallel detection (features `language-detection` + `parallel`)

Sequential versus `par_detect_batch`, short-text-sized items, measured on a
32-core machine:

| Batch size | Sequential | Parallel | Speedup |
|---:|---:|---:|---:|
| 16 | 461.4–482.7 µs | 84.98–91.44 µs | ~5.3× |
| 64 | 1.862–2.057 ms | 223.2–334.5 µs | ~6–7× |
| 256 | 7.270–7.294 ms | 735.7–960.9 µs | ~8–9× |
| 1,024 | 29.15–29.37 ms | 2.643–3.510 ms | ~9–10× |
| 4,096 | 120.7–123.3 ms | 8.255–8.893 ms | ~13–14× |

Parallel wins at **every** tested size here, including the smallest (16) —
no sequential-favoring crossover in this range, because a single `detect()`
call (tens of microseconds, per the table above) is already expensive
enough that `rayon`'s own fork-join overhead is negligible by comparison.
That is a real finding specific to this crate, not a general rule — smaller
per-item primitives elsewhere in this workspace (see
[Phonetics § `par_encode_batch`](phonetics.md#par-encode-batch-par-encode-double-batch-—-parallel-batch-feature-parallel))
do show a crossover, because a single `process()` call there costs tens to
low hundreds of *nanoseconds*, close enough to `rayon`'s scheduling cost
that per-item dispatch is unpredictable. Language detection's per-call cost
is high enough that this concern doesn't apply the same way.

### Memory, initialization, and allocation

`whatlang` 0.18's frequency tables are five compile-time `static` arrays
(`LATIN_LANGS`, `CYRILLIC_LANGS`, `ARABIC_LANGS`, `DEVANAGARI_LANGS`,
`HEBREW_LANGS`) baked directly into the binary's read-only data — no
runtime load, no file I/O, no deserialization.

**Initialization is free, measured, not assumed.** `WhatlangDetector::new()`
— and `whatlang::Detector::new()` underneath it — costs **0 heap
allocations, 0 bytes**, deterministic across repeated runs, measured with an
external counting allocator run as a separate, non-workspace Cargo project
(this workspace's `unsafe_code = "deny"` forbids writing that probe inside
the crate itself, the same constraint
[`verbora-phonetics`'s own benchmarks](phonetic-index.md#the-honest-trade-off-uniform-neighbors-vs-a-raw-slice)
document hitting).

**Allocations per detection are constant, not input-dependent.**
`whatlang::Detector::detect()` costs exactly **25 heap
allocations/reallocations** — identical from a single word (6 B) through a
long document (10,460 B); only the bytes moved scale up with input length,
not the allocation count. `WhatlangDetector::detect` adds at most
one more: a single-element `Vec` for `LanguageDetection.candidates` (26
total for a `Some` result, 25 for `LanguageDetection::none()`, since an
empty `Vec::new()` doesn't allocate).

**Lazy initialization was deliberately not added.** There is no
runtime-loaded model on the `verbora-language` side to guard —
`WhatlangDetector` is zero-sized and stateless, and `whatlang`'s own
trigram tables need no construction step. The one thing `whatlang` *does*
build at runtime — an inverted character-to-language map — already sits
behind a `std::sync::LazyLock` inside `whatlang` itself, one layer down.
Adding a second `OnceLock`/`LazyLock` around `WhatlangDetector` in this
crate would add an atomic check to every `detect()` call to guard a cache
holding nothing — synchronization for its own sake, which this workspace's
anti-overengineering stance argues against.

**Thread safety.** `WhatlangDetector` is `Copy`, zero-sized, and
automatically `Send + Sync` — no `unsafe impl` anywhere. `par_detect_batch`
is generic over `D: LanguageDetector + Sync` and shares one detector
instance across every thread in the pool.

## Allocation behaviour

| Operation | Allocates |
|---|---|
| `detect_script` | Nothing — a fixed-size stack array and a `chars()` scan |
| `recommend` / `recommend_for_script` | Nothing for `Persian`/`Hindi`/`Chinese` (empty `alternatives`); one short `Vec` for `alternatives` for every other language |
| `WhatlangDetector::detect` | 25 allocations inside `whatlang` itself (see above), plus at most one more — a single-element `Vec` for `LanguageDetection.candidates` |
| `AutoPhoneticStrategy::detect_and_recommend` | Whatever the wrapped detector allocates, plus `recommend`'s own (at most one short `Vec`) only when confidence clears the threshold |
| `apply_transliteration` | Nothing for `NotNeeded`/`Unsupported` (`Cow::Borrowed`); whatever `transliterate_ja` allocates for `Recommended` |
| `par_detect_batch` | One output `Vec<LanguageDetection>`, plus whatever each `detect` call allocates — no extra buffering per chunk |

## Common mistakes

**Trusting `best()` instead of `best_above()` on short input.** `best()`
ignores confidence entirely — it returns the top candidate no matter how
low. Gate on `best_above(threshold)` with a threshold you chose, especially
for anything shorter than a full sentence.

**Treating a name's national or ethnic association as a language
determination.** `"Mueller"` — a German-origin surname — detects as
`Norwegian` at confidence ≈ 0.008 when run alone, not `German` at any
useful confidence. See [Names are not language](#names-are-not-language).

**Calling `WhatlangDetector::detect` inside a hot per-token loop.** It
costs tens to low hundreds of *microseconds* per call — detect once per
document, and reach for `par_detect_batch` for a real batch of independent
texts, not a hand-rolled `rayon` loop or per-token dispatch.

**Assuming `recommend()`'s `alternatives` are ranked.** They are not —
"also legitimate," no ordering beyond that. See
[Phonetics § How to read "Alternative"](phonetics.md#how-to-read-alternative).

**Assuming Polish/Ukrainian/Russian's `SoundExDM` primary means Verbora
romanizes Cyrillic input for you.** It doesn't. `TransliterationAdvice::Unsupported`
for these three specifically means *you* have to do that step first —
`apply_transliteration` passes the input through unchanged for
`Unsupported`, exactly as it does for `NotNeeded`.

**Confusing Persian/Hindi/Chinese's `primary: None` with a bug.** It is
deliberate — see [Phonetic strategy: `recommend()`](#phonetic-strategy-recommend)
and [Phonetics § Choosing a Phonetic Algorithm](phonetics.md#choosing-a-phonetic-algorithm).

## Related

- [Phonetics](phonetics.md) — the four tested encoders `recommend()`
  chooses between, and the
  [per-language table](phonetics.md#choosing-a-phonetic-algorithm) this
  crate's `recommend()` backs directly.
- [Phonetic neighbors](phonetic-index.md) — the dictionary-wide index built
  from whichever encoder you land on.
- [Transliterators](transliterators.md) — `transliterate_ja`, the one
  transliteration path this crate composes with today.
- [Parallelism](../performance/parallelism.md) — the same
  chunked-batch-versus-per-item reasoning `par_detect_batch` follows.
- [Choosing the right API](../choosing/index.md) — the cross-crate version
  of "explicit vs. automatic."

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
