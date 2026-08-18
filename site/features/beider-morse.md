# Beider-Morse Phonetic Matching

The encoders in [Phonetics](phonetics.md) each key on one language's
orthography. `BeiderMorse` solves a different problem: the *same* historical
family name plausibly has several correct spellings, depending on which
country's conventions transcribed it. A name carried from Russia through
Poland to Germany accumulates several distinct, all-legitimate spellings, not
one.

`BeiderMorse` encodes a name into every plausible spelling across up to 18
languages at once — or restricted to one language, when you already know it —
rather than into a single language-specific key.

## When to use it

- **Matching historical or immigrant family names** across the spelling drift
  that crossing a language boundary introduces.
- **Building a genealogical or name-matching search index** where recall
  across plausible transliterations matters more than a single canonical
  spelling.
- **You know (or can guess) which country's conventions apply.** Restricting
  to one language with [`encode_language`](#choosing-a-name-type-rule-type-and-language)
  produces a tighter, faster candidate set than the default auto-detected
  "any language" sweep.

## When not to use it

- **Ordinary same-language fuzzy matching.** If every name in your dataset is
  already in one orthography, [`Metaphone` or `SoundEx`](phonetics.md) is
  cheaper and simpler. Beider-Morse's value is specifically cross-language
  spelling drift.
- **Ranking.** `encode`/`encode_language` generate candidate spellings; they
  do not rank them or apply an edit-distance threshold — the same boundary
  [`PhoneticIndex::neighbors`](phonetic-index.md) draws. Compose with
  [`verbora-distance`](https://docs.rs/verbora-distance) at the call site for
  scoring.
- **Indexing with `PhoneticIndex`.** `BeiderMorseCode`'s variable-length
  output does not fit `PhoneticIndex`'s `PhoneticEncoder` trait, which is
  built around one or two fixed codes per entry.

## Quick example

```rust
use verbora_phonetics::{BeiderMorse, NameType, RuleType};

fn main() {
    let bm = BeiderMorse::new(NameType::Generic, RuleType::Approx);

    // Language auto-detected from the spelling itself: "Renault" guesses
    // to French with high confidence, so only French's own rule file (plus
    // the shared "common" rules) is consulted.
    let code = bm.encode("Renault");
    assert!(code.spellings.contains(&"rinD".to_owned()));
    assert!(code.spellings.contains(&"rinalt".to_owned()));

    // Ambiguous or multi-origin names fall back to the wider "any"
    // language file, hedging across every language this NameType knows.
    let code = bm.encode("Schwarz");
    assert!(code.spellings.len() > 1);
}
```

## Choosing a name type, rule type, and language

Two independent axes, plus an optional third override:

| | Purpose |
|---|---|
| `NameType` | Which family-naming convention's rule tables to draw from — `Generic` (18 languages: the default, general-purpose choice), `Ashkenazi` (10, tuned for Ashkenazi Jewish naming conventions), `Sephardic` (5, tuned for Sephardic Jewish naming conventions) |
| `RuleType` | How wide a net the final refinement pass casts — `Approx` (the default: widest net across plausible cross-language spelling drift) or `Exact` (a narrower pass, closer to "how the name reads today," smaller candidate sets) |
| Language | `encode()` guesses it from the spelling; `encode_language(word, "french")` restricts to one explicitly, skipping the guess entirely |

```rust
use verbora_phonetics::{BeiderMorse, NameType, RuleType};

fn main() {
    // Ashkenazi naming conventions, the narrower Exact refinement pass.
    let bm = BeiderMorse::new(NameType::Ashkenazi, RuleType::Exact);
    let code = bm.encode("Cohen");
    assert!(!code.spellings.is_empty());

    // Skip auto-detection when you already know the language -- smaller
    // candidate set, and roughly 2x cheaper per the benchmarks below.
    let generic = BeiderMorse::new(NameType::Generic, RuleType::Approx);
    let code = generic.encode_language("Rodriguez", "spanish").unwrap();
    assert!(!code.spellings.is_empty());

    // An unknown language name returns None rather than panicking.
    assert!(generic.encode_language("Rodriguez", "klingon").is_none());
}
```

### Language auto-detection

`encode()` sweeps the word's own spelling before doing any rule-table work. A
confident single-language guess (`"Renault"` → French) loads that language's
own rule file and starts every candidate phoneme pre-filtered to it; an
ambiguous guess falls back to the `"any"` file with the (possibly still
narrowed) guessed language set as the starting point. That sweep is a real,
measurable cost — see [Performance characteristics](#performance-characteristics)
— which is why `encode_language` exists for callers who already know the
answer.

### Prefixes and multi-word names

Two shapes real names take are handled explicitly, rather than left to a plain
per-character rule sweep:

- **A leading apostrophe or name prefix** (`"d'Angelo"`, `"van Gogh"`, `"de la
  Cruz"` — the last only for `NameType::Generic`) splits the name into
  `(without-the-prefix)-(with-the-prefix-fused-on)`, each half re-encoded
  independently. `BeiderMorseCode::spellings` then holds exactly one
  already-composed string, not independent candidates.
- **A name with more than one word** is fused into one lookup by default
  (`concat: true`): `"Jean Paul"` is encoded as the single string `"jean
  paul"`, producing one cross-product candidate set spanning both words.
  [`with_concat(false)`](https://docs.rs/verbora-phonetics/latest/verbora_phonetics/struct.BeiderMorse.html#method.with_concat)
  encodes each word independently and hyphen-joins the results — useful when a
  middle name might be present on one side of a match and absent on the other.

```rust
use verbora_phonetics::{BeiderMorse, NameType, RuleType};

fn main() {
    let bm = BeiderMorse::new(NameType::Generic, RuleType::Approx);

    // Prefix split: exactly one composed "(...)-(...)" result.
    let code = bm.encode("von Neumann");
    assert_eq!(code.spellings.len(), 1);
    assert!(code.spellings[0].contains(")-("));

    // Multi-word, concat on (default): one fused candidate set.
    let fused = bm.encode("Jean Paul");
    assert!(fused.spellings.len() > 1);

    // Multi-word, concat off: independent per-word results, hyphen-joined.
    let split = bm.with_concat(false).encode("Jean Paul");
    assert_eq!(split.spellings.len(), 1);
    assert!(split.spellings[0].contains('-'));
}
```

## Performance characteristics

Measured with `cargo bench -p verbora-phonetics --bench beider_morse` on one
development machine, over 16-surname batches. Treat the exact figures as
machine-dependent and the orders of magnitude as the reproducible part.

| Choice | Cost per 16-name batch |
|---|---|
| `encode` (guesses the language) | ~104 µs |
| `encode_language` (language already known) | ~48 µs — roughly **2.2× cheaper** |
| `RuleType::Approx` | ~104 µs |
| `RuleType::Exact` | ~71 µs — the narrower final pass is cheaper to *run*, not just smaller in its result |
| Multi-word, `concat: true` (fused) | ~104 µs |
| Multi-word, `concat: false` (split, hyphen-joined) | ~86 µs |

**Language count is not the cost driver; guess confidence is.**
`NameType::Ashkenazi` (10 languages) measures *slower* than `Generic` (18
languages) on the same 16-surname list, and `Sephardic` (5 languages) measures
fastest of the three. Under Ashkenazi's narrower language pool most of those
names guess ambiguously rather than resolving to one confident language, so
they fall back to the wider `"any"` rule file and produce a *larger* candidate
set than Generic's mostly-singleton guesses do. Candidate-set size is what you
pay for.

### Input length

`encode`/`encode_language` cap the normalized input at 512 characters
(silently truncating anything longer) and skip prefix-splitting above 128
characters, falling through to ordinary multi-word handling instead. No real
name comes close to either limit; the caps exist to bound a pathological input
— a repeated name prefix such as `"de de de ... cruz"` — whose splitting cost
would otherwise compound once per repetition.

## Known edge case

Names ending in a bare word-final consonant cluster such as `-poulos` or `-gh`
(`"Angelopoulos"`, `"Balogh"`, `"van Gogh"`) are this encoder's weakest area:
the rule files resolve those endings less confidently than the rest of the
corpus. Everything else — including compound language tags, the rule pass, the
prefix split and all three `NameType`s — is pinned by the crate's own test
suite. If your dataset leans heavily on names with that ending, verify the
candidate sets you get before relying on them.

## Licensing

The 127 embedded rule files (`crates/verbora-phonetics/data/beider-morse/`)
are Apache-2.0-licensed data, sourced exclusively from the Apache-2.0
provenance chain. Every file keeps its own license header, and a top-level
`NOTICE.md` records that chain in full. The engine and parser reading the data
(`crates/verbora-phonetics/src/beider_morse/{engine,rule,lang}.rs`) are
Verbora's own MIT-licensed Rust.

## Related

- [Phonetics](phonetics.md) — the single-key encoders this one sits alongside.
- [Phonetic neighbors](phonetic-index.md) — dictionary-wide lookup for the
  fixed-code encoders.
- [String distance](distance.md) — the scoring step that ranks candidates.
