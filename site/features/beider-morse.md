# Beider-Morse Phonetic Matching

The four encoders in [Phonetics](phonetics.md) are each tuned for one
language's — mostly English's — orthography. None of them solve the problem
a genealogical name index actually has: the *same* historical family name
plausibly has several "correct" spellings depending on which country's
conventions transcribed it. A name carried from Russia through Poland to
Germany accumulates several distinct, all-legitimate spellings, not one.
`BeiderMorse` solves that: it encodes a name into every plausible spelling
across up to 18 languages at once (or restricted to one, if you already know
it), rather than a single language-specific key.

<div class="callout callout-note">
<strong>Verbora-native extension — not a ported feature.</strong>
The reference has no Beider-Morse implementation, so <code>BeiderMorse</code>
is not a port. Correctness here was instead established during development
against a disposable, non-dependency build of <code>rphonetic</code> (a
mature, independently-verified Rust port of the same underlying algorithm)
reading the identical rule-file corpus this crate embeds. See
<a href="#correctness-and-a-known-edge-case">Correctness and a known edge
case</a> below for what that verification covered, the two real bugs it
caught, and the one still-open discrepancy it didn't resolve. See
<a href="phonetics">Phonetics</a> for the four tested encoders this
extension sits alongside.
</div>

## When to use it

- **Matching historical or immigrant family names** across the spelling
  drift that crossing a language boundary introduces — the textbook use case
  this algorithm was designed for.
- **Building a genealogical or name-matching search index** where recall
  across plausible transliterations matters more than a single canonical
  spelling.
- **You know (or can guess) which country's conventions apply.** Restricting
  to one language with [`encode_language`](#choosing-a-name-type-rule-type-and-language)
  produces a tighter, faster candidate set than the default auto-detected
  "any language" sweep.

## When not to use it

- **Ordinary same-language fuzzy matching.** If every name in your dataset
  is already in one language/orthography, [`Metaphone`](phonetics.md) or
  [`SoundEx`](phonetics.md) is cheaper and simpler — Beider-Morse's value is
  specifically cross-language spelling drift.
- **A search engine.** `encode`/`encode_language` generate candidate
  spellings; they do not rank them, apply an edit-distance threshold, or
  accept a query language — the same boundary
  [`PhoneticIndex::neighbors`](phonetic-index.md) draws. Composing with
  [`verbora-distance`](https://docs.rs/verbora-distance) for ranking is left
  to the call site.
- **Indexing at `PhoneticIndex` scale, today.** `BeiderMorseCode`'s
  variable-length output doesn't fit `PhoneticIndex`'s `PhoneticEncoder`
  trait (built around one-or-two fixed codes per entry) — composing the two
  is a real, open question, not implemented yet.

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
| `RuleType` | How wide a net the final refinement pass casts — `Approx` (the default: widest net across plausible historical/cross-language spelling drift) or `Exact` (a narrower pass, closer to "how the name reads today," smaller candidate sets) |
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

`encode()` runs a full regex sweep over the word's own spelling before doing
any rule-table work — the same heuristic layer every reference
implementation uses, ported into this crate rather than skipped. A confident
single-language guess (`"Renault"` → French) loads that language's own rule
file and starts every candidate phoneme pre-filtered to it; an ambiguous
guess falls back to the `"any"` file with the (possibly still narrowed)
guessed language set as the starting point. This is a real, measurable cost
— see [Performance characteristics](#performance-characteristics) — which is
exactly why `encode_language` exists as an escape hatch for callers who
already know the answer.

### Prefixes and multi-word names

Two shapes real names take that a plain per-character rule sweep would get
wrong are handled explicitly:

- **A leading apostrophe or name prefix** (`"d'Angelo"`, `"van Gogh"`, `"de
  la Cruz"` — the last only for `NameType::Generic`) splits the name into
  `(without-the-prefix)-(with-the-prefix-fused-on)`, each half re-encoded
  independently. `BeiderMorseCode::spellings` holds exactly one
  already-composed string for this case, not independent candidates — see
  the type's own doc comment.
- **A name with more than one word** is, by default (`concat: true` — every
  reference implementation's own real default, despite what its own doc
  comment claims; confirmed against the constructor source), fused into one
  lookup: `"Jean Paul"` is encoded as the single string `"jean paul"`,
  producing one cross-product candidate set spanning both words. Calling
  [`with_concat(false)`](https://docs.rs/verbora-phonetics/latest/verbora_phonetics/struct.BeiderMorse.html#method.with_concat)
  instead encodes each word independently and hyphen-joins the results —
  useful when a middle name might be present on one side of a match and
  absent on the other.

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

Real numbers, one development machine, `cargo bench -p verbora-phonetics
--bench beider_morse` (16-surname batches; treat exact figures as
machine-dependent, orders of magnitude as the reproducible part). Full
methodology in the bench file's own module doc comment, and the complete
aspect-by-aspect review in `docs/PERFORMANCE_MATRIX.md`'s "Beider-Morse
Phonetic Matching" section.

| Comparison | Result |
|---|---|
| `encode` (guesses the language) vs. `encode_language` (already known) | ~104 µs vs. ~48 µs per 16-name batch — knowing the language ahead of time is roughly **2.2× cheaper** |
| `RuleType::Approx` vs. `Exact` | ~104 µs vs. ~71 µs — `Exact`'s narrower final pass is measurably cheaper, not just a smaller *result* |
| Multi-word `concat: true` (fused) vs. `false` (split, hyphen-joined) | ~104 µs vs. ~86 µs |

The one genuinely surprising result, published rather than smoothed over:
`NameType::Ashkenazi` (10 languages) measured **slower** than `Generic` (18
languages) on the same 16-surname list, and `Sephardic` (5 languages)
measured fastest of the three — the opposite of "fewer languages is
faster." The list used is Romance/Slavic/Greek-biased (picked to suit
`Generic`); under Ashkenazi's own narrower language pool, most of those
names guess ambiguously rather than to one confident language, falling back
to the wider `"any"` rule file and producing a *larger* candidate set than
Generic's own mostly-singleton guesses do. The real cost driver, confirmed
by this benchmark, is guess confidence and resulting candidate-set size —
not raw language count.

### Input length

`encode`/`encode_language` cap the normalized input at 512 characters
(silently truncating anything longer) and skip prefix-splitting above 128
characters, falling through to ordinary multi-word handling instead. No
real name comes close to either limit — this exists because an independent
audit found that a *repeated* Generic name prefix (`"de de de ... cruz"`)
recursed once per repetition, each level costing roughly as much as the
original input: ~3,000 characters of repeated prefix cost 14+ seconds
before this cap existed. See
[Correctness and a known edge case](#correctness-and-a-known-edge-case)
below for the rest of what that audit found.

## Correctness and a known edge case

Every other Verbora-native extension in this workspace has something to
lean on for verification (`PhoneticIndex` reuses this crate's own
tested encoders; [Language](language.md) benchmarks against real
competitor crates). Beider-Morse has neither, so correctness was
established during development against a disposable build of `rphonetic`
reading the identical rule-file corpus this crate embeds — chosen
specifically to isolate engine-algorithm correctness from rule-corpus
correctness, since both implementations read the same underlying data. That
process caught two real bugs before landing (a compound language tag like
`gv[portuguese+spanish]` being silently dropped instead of resolved, and the
Rules pass wrongly passing an unmatched character through instead of
skipping it) and then swept 106 Generic surnames (96.2% exact match), 10
Ashkenazi and 10 Sephardic surnames (100%), `RuleType::Exact` on 12 names
(100%), 16 explicit single-language calls (100%), and 5 prefix/multi-word
names (4/5). A follow-up independent audit of the finished module then
found and fixed one real blocker (the repeated-prefix cost above) plus
several minor doc/test gaps — see `AGENTS.md`'s Beider-Morse section for
the full findings list.

The remaining mismatches all cluster around one still-open edge case: names
ending in a bare word-final consonant cluster like `-poulos` or `-gh`
(`"Angelopoulos"`, `"Balogh"`, `"van Gogh"`). Traced by hand through the
actual rule files without finding an attributable bug on Verbora's own side
— recorded here rather than silently accepted, per this workspace's
"measure and disclose, don't smooth over" discipline. If your dataset leans
heavily on names with that ending, verify your own results before relying on
exact agreement with other Beider-Morse implementations.

## Licensing

The 127 embedded rule files (`crates/verbora-phonetics/data/beider-morse/`)
are Apache-2.0-licensed data from Apache Commons Codec — itself a Java
re-implementation of Alexander Beider and Stephen P. Morse's original,
GPL-3.0-licensed PHP reference. Verbora copies from the Apache-2.0 chain
only, never touches the GPL-3.0 PHP source, and preserves every file's own
license header plus a top-level `NOTICE.md` recording the full provenance
chain. The engine and parser reading that data
(`crates/verbora-phonetics/src/beider_morse/{engine,rule,lang}.rs`) are
Verbora's own MIT-licensed Rust.
