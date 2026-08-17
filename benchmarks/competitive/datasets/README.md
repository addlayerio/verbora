# Competitive benchmark datasets

Real, sourced, licensed test data used by the competitive benchmarks in this
workspace. Per `Fase 6 Benchmark.md`'s `REALISTIC DATASETS` and `DO NOT USE
FAKE NUMBERS` sections, nothing here is synthetic prose invented to flatter
one implementation — every file documents exactly where its text came from.

## `language-accuracy/` — Language Detection accuracy dataset

Used by `rust-competitors/examples/language_accuracy.rs` (§1.9 of
`docs/COMPETITIVE_BENCHMARKS.md`) to measure **accuracy**, not just speed, for
Verbora's `WhatlangDetector`, `lingua`, and `whichlang` — required by the
spec's own `LANGUAGE DETECTION BENCHMARKS` section ("aquí performance sola NO
es suficiente. Medir también accuracy").

### Source

- **Corpus**: *UDHR in XML* (the direct successor to the "UDHR in Unicode"
  project formerly hosted at `unicode.org/udhr`, retired January 2024 — its
  own landing page now points here). Compiles the Office of the United
  Nations High Commissioner for Human Rights' (OHCHR) official **Universal
  Declaration of Human Rights Translation Project** into a uniform,
  machine-readable XML form.
- **Repository**: <https://github.com/eric-muller/udhr> (maintainer: Eric
  Muller, the same person who built the original Unicode Consortium corpus).
- **Files used**: `data/udhr/udhr_<key>.xml`, one per language — the exact
  `<key>` for each language is recorded in `dataset.json`'s own
  `udhr_key` field for traceability.
- **Retrieved**: 2026-08-16, via `raw.githubusercontent.com` (commit at time
  of retrieval: `main` branch, HEAD as of that date).
- **License**: The Universal Declaration of Human Rights is a UN General
  Assembly resolution with no reproduction restriction for informational/
  educational use, and the compiling project states plainly on its own site
  (`website/index.xml`): *"The UDHR and all its translations are obviously
  part of our commons."* No additional license file is asserted by the
  repository beyond that statement and the UN's own free-reproduction policy
  for the Declaration itself.

### Which 13 languages, and why exactly these

Verbora's `Language` enum has 22 variants (`crates/verbora-language/src/language.rs`).
The three real competitors in `docs/COMPETITIVE_BENCHMARKS.md` §1.9 do **not**
all cover the same subset:

| Detector | Coverage of Verbora's 22 |
|---|---|
| `whatlang` 0.18.0 | 20/22 (missing Galician, Basque) |
| `lingua` 1.8.0 | 21/22 (missing Galician) |
| `whichlang` 0.1.1 | 13/22 |

Per the spec's own instruction ("a representative subset of Verbora's 22
languages, **at least** the ones whatlang/lingua/whichlang all three
cover"), this dataset uses exactly the **13-language triple overlap** —
English, Spanish, French, German, Italian, Portuguese, Dutch, Russian,
Hindi, Vietnamese, Japanese, Chinese, Swedish — so every one of the three
detectors is scored on the *identical* set of true-answer classes. Scoring
`whichlang` on languages it structurally cannot represent (it has no
`Lang::Ukrainian` etc.) would not be measuring accuracy, it would be
measuring a category error; restricting to the honest overlap is what makes
the three accuracy numbers comparable at all. `whatlang` and `lingua` are
not further restricted beyond this — both would score similarly on the 20/21
languages outside this overlap too, but a fourth column with no `whichlang`
entry would break the side-by-side table the spec requires (see the
`ACCURACY + PERFORMANCE` section).

### The four length tiers, and the exact extraction rule

The spec requires **short word / short phrase / sentence / paragraph**
categories. All four are sliced from the *same* two source paragraphs
(UDHR Article 1, and Article 1 + Article 2) for every language, using one
fixed, mechanical rule per tier — not hand-picked per language to flatter any
result:

| Tier | Rule |
|---|---|
| `short_word` | The word meaning **"dignity"**, exactly as it appears inflected in that language's Article 1 (e.g. English `dignity`, Russian `достоинстве` — prepositional case, German `Würde`). Chosen because Article 1 of the UDHR happens to contain a "dignity"-cognate in all 13 languages, giving a semantically matched single-word probe across every language rather than an arbitrary token. Verified programmatically: the extraction script asserts the word is a literal substring of that language's Article 1 text before writing it out. |
| `short_phrase` | For space-delimited languages: the first 5 whitespace-separated tokens of Article 1's first sentence (e.g. English `"All human beings are born"`, Spanish `"Todos los seres humanos nacen"` — same meaning by construction, since both are translations of the same UN sentence). Vietnamese is space-delimited by syllable per its own orthography, not by word, which is documented here rather than silently normalized away. For Japanese and Chinese (no whitespace segmentation): the first clause up to the first native comma (`、`/`，`). |
| `sentence` | The first full sentence of Article 1, split at the first sentence-terminal mark for that script (`.` for Latin/Cyrillic scripts, `。` for Japanese/Chinese, `।` for Hindi's Devanagari danda). Spanish's Article 1 is a single long sentence in the official translation, so its "sentence" tier is the whole article — a faithful reflection of the source, not an extraction artifact. |
| `paragraph` | Article 1 (both sentences, full) followed by Article 2 (full) — a genuine multi-sentence passage, ~700–810 characters for the 11 space-delimited languages and ~194–313 characters for Chinese/Japanese (CJK text carries more information per character, so a proportionally shorter *character* count still represents a comparable amount of *content* — reported as-is rather than padded to match a target byte count). |

The extraction script (Python, not checked into this repository — a one-time
data-preparation step, not part of the reproducible benchmark pipeline) is
described here in full so the process is auditable even without the script
itself: parse each `udhr_<key>.xml`, join every `<para>` of the given
`<article>` with a single space, apply the four rules above, and assert the
`short_word` substring check. `dataset.json` is the committed output — it is
the artifact that matters for reproducibility, not the throwaway script that
produced it once from a third-party source.

### Format

```jsonc
{
  "languages": [
    {
      "iso639_1": "en",          // the expected answer every detector is scored against
      "verbora_language": "English", // matches verbora_language::Language::name()
      "udhr_key": "eng",         // the exact UDHR source file, for traceability
      "items": {
        "short_word": "dignity",
        "short_phrase": "All human beings are born",
        "sentence": "All human beings are born free and equal in dignity and rights.",
        "paragraph": "All human beings are born free and equal in dignity and rights. ..."
      }
    },
    // ... 12 more languages
  ]
}
```

### Known limitation, stated plainly

13 languages × 4 tiers = 52 items is a small evaluation set — real
language-ID accuracy papers use thousands of examples per language. This is
adequate to demonstrate the *direction and rough magnitude* of the accuracy
gap between detectors at each length tier (which is what the spec's
"short word / short phrase / sentence / paragraph" categories are asking to
demonstrate — that accuracy rises with input length, and does so at
different rates for different algorithms), but a single-digit number of
mistakes at the `short_word` tier swings the reported percentage by
multiple points. `docs/PERFORMANCE.md`'s own Language Detection section
reports raw correct/total counts alongside the percentage for exactly this
reason — a reader should not mistake "76.9% (10/13)" for a claim with the
statistical precision "76.9%" alone would imply.
