# verbora-language

Script detection, language detection, and phonetic-strategy recommendation.
It answers one question and stops there: *given a word or a document, which of
Verbora's twelve phonetic encoders should I even use, and does a
transliteration step belong in front of it?* Three layers, kept separate on
purpose — Unicode-block script classification (no model, no allocation, no
dependency), statistical language detection, and a closed lookup table from
language to encoder.

## The contract

**Detection is statistical, so the guarantee is determinism, not agreement
with anyone else's output.** `detect_script` is pure, allocation-free and
deterministic: the same `&str` gives the same answer on every call, thread
and platform, with no hashing, no iteration-order dependence and no floating
point. `recommend` is not statistical at all — it is a `match` over a closed
set of 22 languages, so given the language there is nothing to be uncertain
about. Absence is `Option::None` throughout: `detect_script` returns `None`
when the input contains no letters at all (distinct from `Script::Other`,
which means "letters, in a script this crate models no language for"), and
`PhoneticStrategy::primary` is `None` exactly when `basis` is
`StrategyBasis::NoFit`.

**Uncertainty is not hidden.** `recommend` takes a `Language`, never a `&str`
and never a detector, so a statistical guess cannot be laundered into a
phonetic key without the caller seeing it happen; there is deliberately no
`auto_phonetic_encode(text)` anywhere. A detection can be empty,
`LanguageDetection::best_above` makes *you* say what "confident enough"
means, and `StrategyBasis` distinguishes "an encoder was designed for this
language" (`Named`) from "an encoder will read this script" (`Script`) from
"don't" (`NoFit`). This crate detects **language**, never nationality,
ethnicity or name origin.

**Two detectors exist, and the faster one is not the default.**
`DefaultDetector` is `WhatlangDetector` (feature `language-detection`), which
scores 49/52 on the published 13-language × 4-tier UDHR evaluation set that
`tests/default_detector.rs` re-runs as an executed test.
`HashedLinearDetector` (feature `fast-language-detection`, no extra
dependency) is the latency-first alternative — see
<https://verbora.dev/benchmarks/> for the measured comparison — but it scores
45/52, all four losses on input shorter than a sentence, so it stays opt-in
and separately labelled rather than folded into the default. `FallbackDetector<Hashed, Whatlang>` composes the
two and matches the default tier for tier. Only these detectors are
feature-gated; `Language`, `Script`/`detect_script`, `recommend` and
`AutoPhoneticStrategy` compile with zero extra dependencies.

## Example

```rust
use verbora_language::{
    Language, PhoneticRecommendation, Script, StrategyBasis, TransliterationAdvice,
    detect_script, recommend,
};

// Script classification: cheap, exact, and honest about "no letters here".
assert_eq!(detect_script("Müller"), Some(Script::Latin));
assert_eq!(detect_script("Иванов"), Some(Script::Cyrillic));
assert_eq!(detect_script("12345"), None);

// German is the one language with an encoder written for it.
let de = recommend(Language::German);
assert_eq!(de.primary, Some(PhoneticRecommendation::Cologne));
assert_eq!(de.basis, StrategyBasis::Named);
assert_eq!(de.transliteration, TransliterationAdvice::NotNeeded);

// Russian gets an encoder that reads Cyrillic — labelled as a script-level
// fit, not as a pedigree it does not have.
assert_eq!(recommend(Language::Russian).basis, StrategyBasis::Script);

// Japanese: romanize first, then it is Latin-alphabet text like any other.
assert_eq!(
    recommend(Language::Japanese).transliteration,
    TransliterationAdvice::Recommended
);

// And where nothing fits, it says so instead of returning a plausible key.
let hi = recommend(Language::Hindi);
assert_eq!(hi.primary, None);
assert_eq!(hi.basis, StrategyBasis::NoFit);
```

## See also

Full documentation, including the per-tier accuracy table and how to compose
detection with encoding: <https://verbora.dev/features/language>.

The encoders themselves live in `verbora-phonetics`; the romanization
`TransliterationAdvice::Recommended` is asking for is in
`verbora-transliterators`.
