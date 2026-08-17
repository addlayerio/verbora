# Verbora Phase 1 — Production-Grade Transliteration & Script Processing

**Status:** design, not implemented. Nothing described here exists yet.
**Author:** research/design phase agent.
**Date:** 2026-08-15.
**Scope of this document:** a new, Verbora-native crate (working name `verbora-transliterate`)
that performs script conversion governed by *published standards*, plus the script
detection primitive it needs.

> **Honesty contract.** Per `AGENTS.md` § *Honesty rules*, this document must not
> describe a plausible design as though it shipped. Every API in section 4 is a
> **proposal**. Every number in section 5 is an **estimate derived from a cited
> upstream artefact size**, not a measurement — there are no benchmarks for code
> that does not exist. Where a linguistic fact could not be verified from a
> primary source, this document says "not verified" rather than guessing. Those
> markers are load-bearing: an implementer who resolves one must record what they
> found.

---

## 0. The boundary with `verbora-transliterators`

This is the first decision, and it constrains everything below.

### 0.1 What `verbora-transliterators` is

The reference ships exactly one transliterator: `TransliterateJa`, at
The reference `index`, re-exported from
The reference `index`. It is a **kana → modified-Hepburn romaji**
function with six strictly sequential phases, driven by three inline tables (20,
382 and 21 entries). Per `docs/specs/normalizers-translit.json`, its observable
behaviour includes:

| Input | Output | Note |
|---|---|---|
| `漢字` | `漢字` | **kanji are not touched at all** |
| `ｱｲｳ` | `ｱｲｳ` | halfwidth katakana untouched |
| `ヴ` | `v` | not `vu` |
| `ヴー` | `vー` | no rule exists for `vー` |
| `ッA` | `ッA` | the reference's ASCII-only `\B` leaves the sokuon in place before a Latin letter |
| `ッ漢` | `t漢` | …but not before a kanji |
| `あーー` | `āー` | only one prolongation is consumed |
| `ゐゑヰヱ` | `ieie` | archaic kana collapse |
| `・` | `" "` | katakana middle dot becomes an ASCII space |
| `ヷヸヹヺ`, `ゝゞヽヾ`, `ヵヶ` | unchanged | no rules |
| `ゃゅょャュョ` (bare) | unchanged | small kana only combine |

Two more, from the crate's own documentation page
(`site/features/transliterators.md`), because they are the clearest
statements of the gap:

* `こんにちは` → `konnichiha`, not `konnichiwa`. The topic particle `は` is
  romanized by its kana value. "Nothing here knows what a particle is."
* `これは日本語のテストです。` → `koreha日本語notesutodesu。`

Several of these are bugs by any standards-conformance measure. **They are also
the contract.** `verbora-transliterators` is a parity crate: its correctness
criterion is byte-equality against `fixtures/transliterators.json`, which the
book records as **143,060 calls** across 7 suites (including all 36,481 ordered
kana pairs) with the status *Verified*. The reference is right by definition
(`docs/PARITY.md`). The crate is not permitted to emit `vu`, to romanize `漢字`,
to fix the `ッA` case, or to learn what a particle is.

Its public surface, per the same page, is `transliterate_ja` plus
`ja::transliterate_normalized` and the individual phases. Those names are not
available to a standards-conformant implementation and must not be reused.

### 0.2 Why Verbora-native work cannot live there

Three independent reasons, any one of which is sufficient:

1. **Contradictory correctness criteria.** Parity says "reproduce `ヴ` → `v`".
   Standards conformance says Modified Hepburn writes `ヴ` as `vu` (and
   Hepburn-with-`b` variants write `bu`). A single crate cannot satisfy both, and
   a runtime flag that switches between them makes the parity fixture replay
   depend on configuration — which is exactly how recorded-parity suites rot.
2. **Parity percentage integrity.** `docs/PARITY.md` reports coverage of the
   the reference API surface. If Verbora-native romanization lived in the same
   crate, every new script would either dilute the denominator or require an
   ad-hoc "not part of parity" carve-out inside a crate whose entire point is
   that there is no carve-out. **PARITY_VERIFIED status and parity percentages
   must be computed only over the reference-derived surface.**
3. **Dependency weight.** Chinese and Japanese-Kanji romanization need real
   dictionaries (section 5, section 6). `verbora-transliterators` today is three
   static tables and a scanner; it must stay that way so that a caller who wants
   the reference parity does not pull a 10 MB dictionary or a `memmap2`
   dependency.

### 0.3 The rule

```text
verbora-transliterators          verbora-transliterate
─────────────────────────        ──────────────────────────────────
governed by:  the reference         governed by:  published standards
verified by:  fixtures/*.json    verified by:  conformance vectors
scope:        TransliterateJa    scope:        many scripts
may change output when:          may change output when:
  the reference changes            the standard is misimplemented
                                   (a bug fix, semver-minor at most)
depends on:   verbora-core        depends on:   verbora-core (+ data, gated)
```

They are **peers**, not layers. `verbora-transliterate` must not depend on
`verbora-transliterators`, and `verbora-transliterators` must not depend on
`verbora-transliterate`. Neither re-exports the other. A user who wants
"the reference's Japanese function" and a user who wants "Hepburn romaji" ask for
different crates and get different answers, and the docs must say why in both
places.

The one shared thing is `verbora-core`, which already defines the vocabulary
traits (`Tokenizer`, `Stemmer`, `Phonetic`) and the two-level API convention.
A `Transliterate` trait, if one is wanted for generic code, belongs there — but
see § 4.6 for the argument that it probably should *not* exist yet.

---

## 1. Conceptual boundaries

These seven operations are routinely conflated in NLP libraries, and the
conflation is a **correctness bug**, not a stylistic preference. Each has a
different domain, a different codomain, a different notion of "correct", and a
different failure mode. Mixing them produces output that is wrong in a way the
caller cannot detect, because the output *looks* plausible.

### 1.1 The seven operations

| # | Operation | Maps | Preserves | Correct when |
|---|---|---|---|---|
| 1 | **Normalization** | text → text, *same script* | identity of the word | the two forms a human would call "the same string" compare equal |
| 2 | **Transliteration** | script A → script B, *grapheme-driven* | orthography | the original can be reconstructed (ideally bijectively) |
| 3 | **Romanization** | script A → Latin, *often pronunciation-driven* | approximate pronunciation | a reader of the target language says roughly the right thing |
| 4 | **Phonetic encoding** | text → opaque key | *nothing legible* | similar-sounding inputs collide |
| 5 | **Textual similarity** | (text, text) → number | — | the number orders candidate pairs usefully |
| 6 | **Semantic similarity** | (text, text) → number | — | the number tracks *meaning*, not spelling |
| 7 | **Translation** | text → text, different language | meaning | a bilingual speaker accepts it |

Verbora is in the business of 1–5. It is **not** in the business of 6 or 7, and
this crate is squarely 2 and 3.

### 1.2 Worked examples of the distinctions

**`Şahin` → `Sahin` is normalization (diacritic folding), not transliteration.**
Both strings are Latin script. Nothing crossed a script boundary. The operation
is `verbora-normalizers::remove_diacritics`, and its purpose is to make two
spellings of one name compare equal. It is *lossy within Latin*: `Şahin` and
`Sahin` and `Sáhin` all collapse to `Sahin`, so the operation cannot be inverted
and must never be presented as "converting Turkish to English".

**`Александр` → `Aleksandr` is romanization.** Cyrillic → Latin, crossing a
script boundary. This particular name is an easy case — the major standards agree
on every letter in it — which is exactly why it is a bad example to design
around. Change one letter and they diverge:

| Cyrillic | ISO 9 | BGN/PCGN (Russian) | ALA-LC (Russian) |
|---|---|---|---|
| `Щ` | `Ŝ` | `Shch` | `Shch` |
| `Ю` | `Û` | `Yu` | `I͡U` |
| `Х` | `H` | `Kh` | `Kh` |
| `Ж` | `Ž` | `Zh` | `Zh` |

*(These four rows are the standards' well-known shapes and are checked against
§ 2.1's tables; the point they make does not depend on any single cell.)*

And change the *language* and the same letters change value again: Ukrainian `Г`
is `h` where Russian `Г` is `g`, so `Григорій` is `Hryhorii` and `Григорий` is
`Grigoriy`. **A single "Cyrillic → Latin" function that ignores language is wrong
for most of the six major Cyrillic-written languages.** See § 2.1.

**`Robert` → `R163` is phonetic encoding.** The output is a *key*, not text. It
is not pronounceable, not reversible, not human-readable, and comparing two keys
for equality is the only supported operation. `verbora_phonetics::SoundEx::new()
.process("Robert")` returns exactly this (verified in the crate's own doctest).
Calling this "transliteration" would license a caller to display `R163` to a
user.

**`levenshtein("kitten", "sitting") == 3.0` is textual similarity.** It measures
*spelling* distance. It knows nothing about sound and nothing about meaning.
`verbora_distance` documents this: `levenshtein` is a *distance* (lower is
closer) while `jaro_winkler` is a *similarity* (higher is closer), and the crate
refuses to normalise the two directions because doing so would change every
caller's results.

**Semantic similarity** would say `car` ≈ `automobile`. No function in this
design does that, and none should pretend to: `dice_coefficient("car",
"automobile")` is near zero, correctly, because they share almost no bigrams.

**Translation** would say `Александр` → `Alexander`. That is a *different
person's name in a different language*, arrived at by lexical lookup, not by any
letter mapping. `Александр` → `Aleksandr` is romanization; `Александр` →
`Alexander` is translation (or, more precisely, cross-lingual name matching
against an entity list). Verbora will do the former and must **never**
silently do the latter.

### 1.3 Why conflation is a correctness bug

Consider a sanctions/PEP screening system — a realistic consumer of this crate —
that matches an incoming name against a watchlist.

* If romanization is implemented as diacritic folding, `Аляксандр` (Belarusian)
  passes through untouched, because it has no Latin diacritics to fold. The name
  is never compared to the watchlist entry `Alyaksandr`. **A false negative that
  no test of the folding function would catch**, because the folding function
  did exactly what it says.
* If phonetic encoding is used where romanization was meant, the system displays
  `A4253` in an audit log and a human reviewer cannot verify the match.
* If a Russian mapping is applied to Ukrainian text, `Григорій` romanizes to
  `Grigoriy` instead of `Hryhorii`. Both are *plausible Latin strings*. Nothing
  in the output signals the error. The mismatch surfaces months later as an
  unexplained recall gap.
* If translation is smuggled in — `Александр` → `Alexander` — the system now
  matches on a name the source document does not contain, and the provenance
  chain from source text to decision is broken.

Every one of these produces output that passes eyeballing. That is the
definition of a silent correctness bug. The mitigations this design adopts:

1. **Distinct crates and distinct function names.** `remove_diacritics` lives in
   `verbora-normalizers`; `process` lives in `verbora-phonetics`;
   `transliterate` lives in `verbora-transliterate`. No crate offers an
   `anglicize` or `to_ascii` catch-all.
2. **Language is a required input, not an inferred one,** wherever the mapping
   is language-dependent (§ 2.1, § 2.8, § 4.5).
3. **The API refuses to guess** rather than picking a plausible default (§ 4.5).
4. **Lossiness is typed, not documented.** The result carries whether the
   conversion was reversible (§ 4.3).

---

## 2. Standards research

*(filled in below — see § 2.1 onward)*

---

## 3. Script detection

### 3.1 What the primitive must do

Two distinct questions, and they need two distinct answers:

* **Per-character:** "what script is this `char`?" — a total function, needed by
  every segmenting loop.
* **Per-string:** "what scripts occur in this text, and is there a dominant
  one?" — needed for dispatch and for the *refusal* logic in § 4.5.

Anything that returns a single `Script` for a whole string is the wrong shape:
`"Alexander Александр"` has no single answer, and a function that returns one
has silently discarded half the input.

### 3.2 The enum

```rust
/// A writing system, at the granularity this crate can act on.
///
/// This is deliberately **coarser** than ISO 15924 / UAX #24: it enumerates the
/// scripts `verbora-transliterate` has (or plans) a strategy for, plus the three
/// bucket variants every other codepoint falls into. Adding a variant is a
/// semver-minor change, so the enum is `#[non_exhaustive]`.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Script {
    /// Basic Latin plus Latin-1/Extended/Additional. Includes ASCII.
    Latin,
    Cyrillic,
    Greek,
    Arabic,
    Hebrew,
    /// Han ideographs (CJK Unified + Extensions + Compatibility).
    Han,
    /// Hiragana, katakana, and the halfwidth katakana forms.
    Kana,
    /// Precomposed Hangul syllables and conjoining jamo.
    Hangul,
    Devanagari,
    Thai,
    Armenian,
    Georgian,
    /// Digits, punctuation, symbols, whitespace, marks with no script of their
    /// own — anything `Script_Extensions` would call `Common`.
    Common,
    /// Combining marks that inherit the script of the preceding base character.
    Inherited,
    /// Assigned to a script this crate does not enumerate, or unassigned.
    Unknown,
}
```

Design notes, each with a reason:

* **`Kana` is one variant, not `Hiragana` + `Katakana`.** Every romanization
  strategy treats them as a mirrored pair — the reference's own table is 191
  katakana entries and the identical 191 in hiragana — and no caller has ever
  wanted to dispatch differently on them. A caller who genuinely needs the
  distinction can test the codepoint. (If a Japanese *tokenizer* later needs the
  split, it belongs in that crate, not here.)
* **`Han` does not distinguish simplified from traditional.** It cannot: the
  distinction is not a property of a codepoint. 中, 人, 大 are identical in both.
  See § 2.5.
* **`Inherited` is a real variant, not folded into `Common`.** U+0301 COMBINING
  ACUTE ACCENT after `е` is Cyrillic-in-context and after `e` is Latin-in-context.
  A detector that calls it `Common` makes `Алекса́ндр` look like mixed script; one
  that calls it `Inherited` lets the run-splitter attach it to the preceding run.
  This is the single most common bug in hand-rolled script detectors.
* **`Unknown` is honest.** Emoji, Braille, Linear B, PUA. The crate will not
  guess.

### 3.3 The algorithm

**Per character.** A sorted, static table of `(start: u32, end: u32, script:
Script)`, binary-searched.

```rust
static RANGES: &[(u32, u32, Script)] = &[ /* generated, sorted by start */ ];

pub fn script_of(c: char) -> Script {
    let cp = c as u32;
    match RANGES.binary_search_by(|&(lo, hi, _)| {
        if cp < lo { Ordering::Greater } else if cp > hi { Ordering::Less }
        else { Ordering::Equal }
    }) {
        Ok(i) => RANGES[i].2,
        Err(_) => Script::Unknown,
    }
}
```

With an **ASCII fast path in front of it**, matching the house pattern in
`verbora_distance::units` and `verbora_phonetics::units`:

```rust
if cp < 0x80 { return ASCII_TABLE[cp as usize]; }   // 128-byte static table
```

ASCII is the overwhelming majority of codepoints in real mixed text, and the
table lookup is one cache line rather than ~7 branch-mispredicting comparisons.

**Per string** — a *run iterator*, not a classifier:

```rust
/// A maximal run of characters belonging to one script.
pub struct ScriptRun<'a> {
    pub script: Script,
    pub text: &'a str,
    /// Byte offset of `text` within the original input.
    pub start: usize,
}

/// Splits `text` into maximal single-script runs. Lazy; allocates nothing.
pub fn script_runs(text: &str) -> impl Iterator<Item = ScriptRun<'_>>;
```

Run-merging rules, which are where the correctness lives:

1. `Inherited` **always** joins the preceding run. At the start of input, where
   there is no preceding run, it becomes `Common`.
2. `Common` joins the preceding run **iff** the following character is the same
   script as the preceding run. Otherwise it forms its own run. This is what
   makes `"Alexander Александр"` split as `[Latin("Alexander"), Common(" "),
   Cyrillic("Александр")]` rather than gluing the space onto either side — and
   what makes `"Tokyo-to"` stay one Latin run across the hyphen.
   *Implementation note: this requires one character of lookahead, so the
   iterator buffers a pending `Common` span. It does not require a second pass.*
3. Nothing else merges. `"東京"` and `"Tokyo"` in `"東京 Tokyo"` are separate runs
   and must be handled by separate strategies.

**Worked cases** (these become unit tests verbatim):

| Input | Runs |
|---|---|
| `"Alexander Александр"` | `Latin("Alexander")`, `Common(" ")`, `Cyrillic("Александр")` |
| `"東京 Tokyo"` | `Han("東京")`, `Common(" ")`, `Latin("Tokyo")` |
| `"محمد Ali"` | `Arabic("محمد")`, `Common(" ")`, `Latin("Ali")` |
| `"Алекса́ндр"` | `Cyrillic("Алекса́ндр")` — one run; U+0301 inherits |
| `"café"` | `Latin("café")` |
| `"2026年"` | `Common("2026")`, `Han("年")` |
| `"東京都に住んでいる"` | `Han("東京都")`, `Kana("に")`, `Han("住")`, `Kana("んでいる")` |
| `"😀"` | `Unknown("😀")` |
| `""` | *(no runs)* |

The last-but-one row is the one to think about. Japanese text alternates `Han`
and `Kana` at nearly every morpheme boundary, so `script_runs` produces many
short runs — four, here, for a five-morpheme phrase. That is exactly right for a
script detector and completely useless as a Japanese processing strategy:
romanizing the `Kana` runs and leaving the `Han` runs alone yields
`東京都ni住ndeiru`, which is not romaji and not Japanese. **Japanese handling
must not be built on run-splitting**; see § 2.6 and § 4.5.

### 3.4 Why not `unicode-script`, ICU, or a full UCD table

The alternatives, and the reasoning:

| Approach | Data size | Verdict |
|---|---|---|
| Full UAX #24 `Script` property (168 scripts, ~2,000 ranges) | tens of KB of tables plus a 168-variant enum | Over-specified. 155 of the variants would map to a single "we have no strategy" arm. |
| `Script_Extensions` (sets per codepoint) | larger still; requires set-valued results | Correct for *identifier security* (UTS #39 confusables), which is not this problem. A romanizer needs "which strategy do I run", a single answer. |
| ICU4X `icu_properties` | pulls a data-provider architecture, `zerovec`, baked-data crates | Real dependency weight for one predicate. `AGENTS.md` § *Dependencies* requires justifying binary and compile-time impact; this fails that test for a table we can generate in ~40 lines. |
| `unicode-script` crate | small, does exactly UAX #24 | Closest reasonable alternative. Rejected because its enum is the full 168-variant one, so we would still need our own coarse mapping on top — at which point we own the mapping anyway and the dependency buys only the ranges. **Worth revisiting** if the range table proves hard to keep current across Unicode versions. |
| **Sorted range table + binary search (this design)** | ~14 scripts × ~4 ranges avg ≈ **60 entries × 12 bytes ≈ 720 bytes**, plus a 128-byte ASCII table | Chosen. |

Speed reasoning: 60 entries is `log2(60) ≈ 6` comparisons worst case, all within
a single 4 KB page and in practice 1–2 cache lines once warm; the ASCII path is
a single indexed load. There is no allocation, no lazy initialisation, no
`OnceLock`, nothing to poison across threads. The whole table is `static`, so
`script_of` is `const`-friendly and trivially `Send + Sync`.

Correctness caveat that must be recorded in the crate docs: **this table is a
deliberate approximation of UAX #24.** It is generated from `Scripts.txt` for a
pinned Unicode version by collapsing the 168 script values into the 15 variants
above, and the table derivation must assert that the collapse is total — every
`Scripts.txt` range maps to exactly one variant, and no range is dropped. If a
future Unicode version adds a range inside an existing block, regeneration
catches it; nothing else will.

---

## 4. Architecture

### 4.1 Module layout

```text
crates/verbora-transliterate/
  Cargo.toml
  src/
    lib.rs              // crate docs, the ergonomic API, re-exports
    script.rs           // Script, script_of, script_runs, ScriptRun
    scan.rs             // the shared leftmost-longest table scanner
    error.rs            // TransliterateError, Reversibility
    cyrillic/
      mod.rs            // CyrillicStandard, per-language dispatch
      tables.rs         // GENERATED
    greek/
      mod.rs            // GreekStandard, contextual rules
      tables.rs         // GENERATED
    arabic/
      mod.rs
      normalize.rs      // WITHIN-script normalization — see § 2.3
      tables.rs         // GENERATED
    hebrew/
      mod.rs
      tables.rs         // GENERATED
    hangul/
      mod.rs            // algorithmic decomposition; no table
      rr.rs             // Revised Romanization + assimilation rules
    kana/
      mod.rs            // Hepburn / Kunrei / Nihon-shiki
      tables.rs         // GENERATED
    pinyin/             // FEATURE-GATED: `pinyin`
      mod.rs
      data.rs           // loader for the compiled reading table
  data/                 // build artefacts, checked in, see § 5
  tests/
    conformance/        // one file per standard, see § 9
  benches/
    transliterate.rs
```

`scan.rs` is the one piece of machinery every table-driven script shares: a
leftmost-longest matcher over a sorted static table of `(&str, &str)` pairs.
`verbora-normalizers` and `verbora-transliterators` already each contain such a
scanner for their own tables; this crate needs a third because the tables differ
in shape (context-sensitive replacements), and **it must not import theirs** —
that would create exactly the parity/standards coupling § 0 forbids. Duplicating
~120 lines of scanner is the cheaper mistake.

### 4.2 Strategy selection

The mechanism is a plain enum per script family, not a trait object, not a
registry, not a string key. Reasons: it is exhaustively matchable (adding a
standard breaks the build at every site that must consider it), it monomorphises,
and it appears in the public API so `rustdoc` enumerates the supported standards
for free.

```rust
/// Which published standard to follow.
///
/// One enum per script family rather than one global enum, because the
/// standards do not overlap: there is no meaningful `Standard::Iso9` for Greek.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CyrillicStandard {
    /// ISO 9:1995 — strictly bijective, language-independent, diacritic-heavy.
    Iso9,
    /// BGN/PCGN, per language. Requires the language; see [`Language`].
    BgnPcgn,
    /// ALA-LC, per language.
    AlaLc,
    /// The romanization enacted in national law for the given language.
    National,
}
```

and, orthogonally:

```rust
/// The language a text is written in.
///
/// Required wherever the script→Latin mapping is language-dependent, which is
/// **most of the time** for Cyrillic (§ 2.1). This is an explicit parameter and
/// is never inferred from the text: see § 4.5.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Language {
    Russian, Ukrainian, Bulgarian, Serbian, Macedonian, Belarusian,
    Greek, Arabic, Hebrew, Japanese, Korean, Mandarin, Hindi, Sanskrit,
}
```

### 4.3 The result type: lossiness is typed

Every conversion returns not just text but whether the text can be trusted to
round-trip. This is the mechanism that stops § 1.3's silent failures.

```rust
/// Whether a conversion can be inverted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reversibility {
    /// Every input character maps to a distinct output sequence and the inverse
    /// is implemented and tested. ISO 9 Cyrillic, Hangul jamo decomposition.
    Bijective,
    /// The mapping is deterministic and total but not injective: distinct inputs
    /// can produce identical output. BGN/PCGN Russian (`е` and `э` → `e`).
    Lossy,
    /// The output depends on information not present in the input, and the
    /// implementation supplied a documented default. Unpointed Hebrew vowels,
    /// unvocalised Arabic. **Callers must not treat this output as canonical.**
    Underdetermined,
}
```

`Underdetermined` is the important one. It is how the API tells a caller "I
produced a string, and it is a guess". A screening system can log it, refuse it,
or route it for review. A system that only got a `String` back cannot.

### 4.4 The public API — three shapes

Following `AGENTS.md` § *Efficient Primitives First*: one core, several
interfaces. The core is the buffer-writing form; the others are defined on top.

**(a) Ergonomic.** For the caller who has one string and a known intent.

```rust
/// Romanizes Russian text with BGN/PCGN.
///
/// Returns [`Cow::Borrowed`] when the input contained no Cyrillic at all, which
/// is the common case for already-Latin input in a mixed corpus.
pub fn romanize_ru(text: &str) -> Cow<'_, str>;
pub fn romanize_uk(text: &str) -> Cow<'_, str>;
pub fn romanize_el(text: &str) -> Cow<'_, str>;
pub fn romanize_ko(text: &str) -> Cow<'_, str>;
// … one per (language, default standard) pair that has a defensible default.
```

`Cow` for unchanged input follows `verbora-normalizers`, which returns
`Cow::Borrowed` from every single-string API and "allocates only at the first
replacement". Same rule here.

**(b) Configurable.** For the caller who needs a specific standard.

```rust
#[derive(Debug, Clone, Copy)]
pub struct Transliterator {
    strategy: Strategy,
}

#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Strategy {
    Cyrillic { standard: CyrillicStandard, language: Language },
    Greek   { standard: GreekStandard },
    Arabic  { standard: ArabicStandard },
    Hebrew  { standard: HebrewStandard },
    Hangul  { standard: KoreanStandard },
    Kana    { standard: JapaneseStandard },
    #[cfg(feature = "pinyin")]
    Pinyin  { tones: ToneStyle },
}

impl Transliterator {
    pub const fn new(strategy: Strategy) -> Self;

    /// Converts `text`. Borrows when nothing changed.
    pub fn transliterate<'a>(&self, text: &'a str) -> Cow<'a, str>;

    /// Converts `text` and reports what was lost.
    pub fn transliterate_detailed<'a>(&self, text: &'a str) -> Transliterated<'a>;
}

pub struct Transliterated<'a> {
    pub text: Cow<'a, str>,
    pub reversibility: Reversibility,
    /// Byte ranges of the input this strategy could not handle, left verbatim
    /// in the output. Empty on a clean conversion.
    pub unhandled: Vec<Range<usize>>,
}
```

**(c) Reusable buffer.** The primitive. Everything above calls this.

```rust
impl Transliterator {
    /// Appends the conversion of `text` to `out`.
    ///
    /// `out` is **not** cleared — same contract as
    /// [`verbora_core::Tokenizer::tokenize_into`] and
    /// [`verbora_tokenizers::Tokenize::tokenize_into`] — so a caller can
    /// accumulate across inputs or, the intended use, reuse one `String`'s
    /// capacity across a corpus.
    pub fn transliterate_into(&self, text: &str, out: &mut String);
}
```

```rust
let t = Transliterator::new(Strategy::Cyrillic {
    standard: CyrillicStandard::BgnPcgn,
    language: Language::Ukrainian,
});
let mut buf = String::new();
for name in corpus {
    buf.clear();
    t.transliterate_into(name, &mut buf);
    consume(&buf);
}
```

**Appending, not clearing** — and the choice is not arbitrary. The book states
Verbora's rule as "the convention follows what the output type is actually for":
`Tokenize::tokenize_into` appends because accumulating tokens across documents is
a real use case; `Stemmer::stem_into` clears because accumulating stem fragments
into one `String` "would produce gibberish". Transliteration is on the appending
side — concatenating the romanization of many segments into one document buffer
is exactly what a caller does — and `verbora-transliterators`'s own
`ja::transliterate_into` already appends. Callers who want replacement call
`out.clear()` themselves. **Do not add a second `_replacing` variant**; two
conventions in one workspace is already the documented "single easiest mistake to
make with these APIs", and a third would be worse.

**(d) Introspection.** Not a fourth way to get the text — a way to get the
*decisions*.

```rust
/// One replacement the strategy would make.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rewrite<'a> {
    /// Byte range in the input.
    pub start: usize,
    pub end: usize,
    /// The matched input slice.
    pub from: &'a str,
    /// What it becomes.
    pub to: &'a str,
}

impl Transliterator {
    /// Lazily yields every rewrite this strategy would perform on `text`.
    /// Allocates nothing, ever.
    pub fn rewrites<'a>(&self, text: &'a str) -> impl Iterator<Item = Rewrite<'a>>;
}
```

This has direct precedent: `verbora-transliterators` ships
`Phase::rewrites(s) -> Rewrites<'_>`, documented as "**none, ever**" for
allocations and the only lazy API in that crate. The same shape here answers
"why did this name romanize that way?" without materialising a string, and it is
what a debugging tool or an audit log should consume.

**No `Iterator<Item = char>` output form is proposed.** `AGENTS.md`
§ *Iterator-First Design* says to evaluate an iterator API "whenever an operation
naturally produces a sequence". Transliteration produces *one string*; a
char-by-char output iterator would be slower (per-item dispatch, and multi-char
replacements need internal buffering anyway) and harder to use. The two
sequence-shaped primitives in this crate are `script_runs` and `rewrites`, and
both are iterators. That is the right place for laziness.

### 4.5 Automatic dispatch, and where it must refuse

```rust
/// Attempts to romanize `text` without being told the language.
///
/// Returns `Err` rather than guessing whenever the choice of strategy would
/// change the output. **This function refuses far more often than callers
/// expect, and that is the feature.**
pub fn romanize_auto(text: &str) -> Result<Cow<'_, str>, AmbiguousScript>;
```

**Where automatic dispatch is safe** (the strategy is determined by the script
alone, and no reasonable alternative differs):

* **Hangul.** Revised Romanization is the standard of the Republic of Korea and
  the decomposition is algorithmic (§ 2.7). Auto-dispatch is defensible.
* **Greek.** One modern language, one script. Auto-dispatch to the default
  standard is defensible.
* **Kana.** One language. Auto-dispatch to Hepburn is defensible *for kana*.

**Where automatic dispatch must refuse:**

* **Cyrillic — always.** The script is shared by ≥ 6 languages with mutually
  incompatible mappings (§ 2.1). `Григорій` is Ukrainian and `Григорий` is
  Russian, and while *those two strings* are distinguishable by the presence of
  `і`, `Виктор` / `Віктор` and countless others are not: a Cyrillic string
  containing only letters common to Russian and Bulgarian has **no** intrinsic
  language. Heuristics ("does it contain `ї`, `є`, `ґ`, `і` → Ukrainian") work
  on the easy cases and fail silently on the hard ones, which is the worst
  possible failure profile. **Refuse.**
  A separate, clearly-named, explicitly-heuristic
  `guess_cyrillic_language(text) -> Option<(Language, Confidence)>` may be
  offered later; it must never be wired into `romanize_auto`.
* **Han.** No reading can be assigned without knowing whether the text is
  Mandarin, Cantonese, Japanese, or Korean-with-hanja. `山` is `shān`, `yama`/
  `san`, or `san` depending. **Refuse.**
* **Mixed Han + Kana.** This *is* diagnostic of Japanese, but see § 2.6: the
  kanji part needs a dictionary and a morphological analyser, so the correct
  behaviour is to refuse (or, under the `japanese` feature, to hand off to the
  documented dictionary path — never to romanize the kana and pass the kanji
  through unchanged, which is what the reference does and what makes its output
  unusable as romaji).
* **Arabic script.** Shared by Arabic, Persian, Urdu, Pashto, Kurdish, Uyghur…
  with different letter inventories and very different values for the shared
  letters. **Refuse.**
* **Any input with more than one substantive script run.** `"Alexander
  Александр"` must not be half-converted without the caller asking for that.
  `romanize_auto` returns `Err(AmbiguousScript::Mixed { runs })`, and the caller
  who *does* want per-run handling uses `script_runs` explicitly.

```rust
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub enum AmbiguousScript {
    /// The script is shared by several languages whose mappings differ.
    /// Call the configurable API with an explicit `Language`.
    LanguageRequired { script: Script, candidates: &'static [Language] },
    /// The input contains substantive runs in more than one script.
    Mixed { scripts: Vec<Script> },
    /// No strategy exists for this script.
    Unsupported { script: Script },
}
```

The design rule, stated once so it can be cited in review: **automatic detection
may choose a strategy only when every candidate strategy would produce the same
output.** Otherwise it returns an error naming what it needs.

### 4.6 Should there be a `Transliterate` trait in `verbora-core`?

**Recommendation: not in phase 1.** `verbora-core`'s existing traits earn their
place by having several implementors that downstream code is generic over
(twenty-five tokenizers, four phonetic encoders). A `Transliterate` trait would
have one implementor (`Transliterator`) and no generic consumer. Adding it now
fixes the signature before we know whether `transliterate_detailed` or the
`Language` parameter belongs in it. Revisit when a second implementor exists.

If one is added later, it must **not** be implemented by
`verbora-transliterators`. That crate's function has a different contract
(§ 0.1) and putting both behind one trait would let generic code substitute a
parity function for a standards function.

---

## 5. Performance architecture

### 5.1 Choosing a representation

The decision rule, applied per dataset:

```text
< ~500 entries, fixed at compile time
        → `match` on char, or a sorted static array + binary search
~500 – ~50,000 entries, fixed at compile time
        → sorted static array + binary search, or a generated perfect hash
multi-character keys needing leftmost-longest
        → sorted static array + a scanner (prefix-free invariant asserted)
> ~100,000 entries, or > ~1 MB of data
        → build artefact: FST or rkyv, memory-mapped, feature-gated
```

`AGENTS.md` § *Static and Precomputed Data* requires that invariant linguistic
data be evaluated for compile-time generation, and § *Anti-Patterns* names
"runtime construction of compile-time-known data". Everything under ~1 MB is
therefore compiled in.

### 5.2 Per-dataset recommendations

| Dataset | Entries | Est. size | Representation | Rationale |
|---|---|---|---|---|
| Script ranges | ~60 | **~0.7 KB** | sorted `static` + binary search | § 3.4 |
| ASCII script fast path | 128 | **128 B** | flat `static [Script; 128]` | one indexed load |
| Cyrillic, all standards × 6 languages | ~120 letters × ~4 tables ≈ **500** | **~12 KB** | one sorted static table per (standard, language), selected by `match` | tiny; no reason for anything cleverer |
| Greek | ~50 letters + ~40 contextual rules | **~4 KB** | static table + hand-written contextual pass | contextual rules are code, not data (§ 2.2) |
| Arabic normalization | ~80 mappings | **~2 KB** | `match` on `char` | dense, contiguous ranges; `match` compiles to a jump table |
| Arabic transliteration | ~60 letters + context rules | **~4 KB** | static table + code | |
| Hebrew | ~40 letters + niqqud | **~2 KB** | static table | |
| Hangul | **0** | **0 B** | pure arithmetic (§ 2.7) | this is the point of § 2.7 |
| Hangul RR assimilation rules | ~40 pairs | **~1 KB** | 19×28 jamo-pair table, direct-indexed | a 532-entry `[u8; 532]` beats any search |
| Kana (Hepburn/Kunrei/Nihon-shiki) | ~400 per standard × 3 | **~30 KB** | sorted static table + leftmost-longest scanner | matches what `verbora-transliterators` already does for its 382 entries |
| Devanagari (IAST / ISO 15919) | ~120 + conjunct handling | **~6 KB** | static table + code | *if* it ships; see § 2.8 |
| **Chinese: per-character readings** | ~**42,000** Han chars with a Mandarin reading | **~500 KB – 1.5 MB** raw | see below | |
| **Chinese: phrase readings** | ~**125,000** CC-CEDICT entries | **~9 MB** raw text | FST or rkyv + mmap, feature-gated | licence problem — § 6 |
| **Japanese: Kanji readings** | ~**13,100** kanji, multiple readings each | **~1–2 MB** raw | rkyv + mmap, feature-gated | licence problem — § 6 |
| **Japanese: morphological dictionary** | ~**400,000+** entries | **~50–100 MB** unpacked | out of scope — § 10 | |

**Chinese per-character readings, in detail.** ~42,000 codepoints is small
enough that a compiled-in representation is arguable, but the *values* are the
problem, not the keys: with tone marks, a pinyin syllable is up to ~7 bytes
UTF-8, and polyphonic characters carry several. A reasonable layout:

* keys: the Han codepoints are *not* contiguous (CJK Unified U+4E00–U+9FFF,
  Ext-A U+3400–U+4DBF, Ext-B U+20000–U+2A6DF, …), so a sorted `&[u32]` +
  binary search, ~168 KB for 42,000 `u32`s;
* values: an index into a deduplicated syllable pool. Mandarin has ~1,300
  distinct toned syllables, so a `u16` index per reading and a ~10 KB pool.
* Total for **single-reading-per-char**: ~168 KB keys + ~84 KB values + ~10 KB
  pool ≈ **260 KB**. That fits comfortably in a compiled-in `static`.

That is the whole argument for why *toneless or single-reading* pinyin can be a
compile-time table while *correct* pinyin cannot: the phrase dictionary needed
for polyphony (§ 2.5) is 30–50× larger and has a licence that a compiled-in
table cannot satisfy (§ 6).

**Where mmap earns its keep.** Only two datasets exceed ~1 MB: the Chinese
phrase dictionary and the Japanese kanji/morphology data. For those:

* `fst::Map` — excellent when keys share prefixes/suffixes (Chinese words do)
  and values are `u64`. The crate is explicitly built so a `Map` "can be memory
  mapped and searched without necessarily loading the entire map into memory".
  Values must be encoded as a `u64` index into a side table.
* `rkyv` + `memmap2` — better when values are structured (a kanji with N
  readings, each with a type tag). Zero-copy access to `#[derive(Archive)]`
  types straight out of the mapped bytes.

**Recommendation:** `fst` for the Chinese phrase dictionary (key-heavy, value is
one index), `rkyv` for kanji readings (value-heavy, structured). **Both are
feature-gated and neither is in the default build.** Neither dependency appears
in `Cargo.toml` unless its feature is on.

### 5.3 Size budget for the default build

| Component | Size |
|---|---|
| Script tables | ~0.9 KB |
| Cyrillic + Greek + Arabic + Hebrew + Hangul + Kana tables | ~55 KB |
| Code | ~30–60 KB (estimate, not measured) |
| **Default-build data total** | **≈ 56 KB** |

For calibration: `deunicode`, which maps ~75,000 codepoints to ASCII, uses about
450 KB (160 KB gzipped). This design's default build is roughly an eighth of
that, because it is not trying to cover all of Unicode — and it produces
*standards-conformant* output for a small set of scripts rather than
*best-effort ASCII* for all of them. Those are different products; the size
comparison is only a sanity check that ~56 KB is not absurd.

### 5.4 Allocation behaviour

Stated as a design requirement, to be verified by benchmark once implemented:

* `script_of`: no allocation, no branch beyond the binary search.
* `script_runs`: no allocation; each `ScriptRun` borrows the input.
* `transliterate` on input with no target-script characters: **no allocation**
  (returns `Cow::Borrowed`).
* `transliterate` otherwise: exactly one `String` allocation, sized by a
  single up-front estimate (`input.len() * expansion_factor`, where the factor
  is a per-strategy constant — Hangul RR expands ~3×, ISO 9 Cyrillic ~1.1×).
  One allocation, not one per replacement.
* `transliterate_into` with a warm buffer: **zero allocations**.

---

## 6. Dataset licensing

*(filled in below — see § 6.1 onward)*

---

## 7. Feature flags

The constraint from `AGENTS.md` § *Cargo Features*: meaningful optional
capabilities, no complicated feature graph, ergonomic default.

```toml
[features]
default = ["std"]

std = []

# --- capability features -------------------------------------------------
# Chinese → pinyin. Pulls a compiled reading table (~260 KB) and, for phrase
# disambiguation, an mmapped dictionary. See § 6 before enabling in a
# distribution: the phrase data is CC BY-SA.
pinyin = ["dep:fst", "dep:memmap2"]

# Japanese kanji → reading. Pulls KANJIDIC-derived data. CC BY-SA; see § 6.
kanji = ["dep:rkyv", "dep:memmap2"]

# Devanagari → IAST / ISO 15919. Pure tables; small. Separate only because
# § 2.8 may defer it past 1.0.
indic = []

# --- integration features ------------------------------------------------
serde = ["dep:serde"]
```

**Five flags, and each has to justify itself:**

* `std` — because `no_std` is genuinely reachable for the table-driven scripts
  (no allocation is required by `script_of`/`script_runs`, and
  `transliterate_into` could take a `&mut dyn fmt::Write`). Not a phase-1
  deliverable, but the flag reserves the shape. `deunicode` demonstrates that a
  transliteration crate can be `no_std`.
* `pinyin`, `kanji` — these are the only two features that pull megabytes and
  new dependencies. Gating them is the whole reason features exist here.
* `indic` — arguably should just be on by default (it is ~6 KB of tables). It
  is separate *only* so that phase 1 can ship the crate with Indic off while the
  question in § 2.8 is settled, and so that "we shipped Indic" is a visible,
  deliberate decision rather than a side effect.
* `serde` — for `Script`, `Strategy`, `Language`, `Reversibility`. Standard.

**Flags deliberately NOT created**, and why:

* **Not one flag per script.** `cyrillic`, `greek`, `arabic`, `hebrew`,
  `hangul`, `kana` would be six flags gating ~55 KB total — flag explosion for
  no measurable benefit, and 2⁶ = 64 build configurations for CI to worry about.
* **Not one flag per standard.** `iso9`, `bgn-pcgn`, `ala-lc` … would make the
  `Strategy` enum's variants conditional, which makes exhaustive `match` in user
  code break on feature changes. Standards cost bytes, not dependencies.
* **Not `parallel`.** Transliteration is per-string and embarrassingly
  parallel *at the caller's level*; `rayon` inside the crate would violate
  `AGENTS.md` § *Threading Policy* ("do not silently spawn threads") for an
  operation that is a few hundred nanoseconds.

CI must build `--no-default-features`, default, and `--all-features`. That is
three configurations, which is the point of keeping the count low.

---

## 8. Composability

Verbora provides primitives. Callers compose them. This section shows the
composition working with crates **that already exist** (`verbora-normalizers`,
`verbora-phonetics`, `verbora-distance` are all PARITY_VERIFIED) plus the
proposed one.

**There is deliberately no `Pipeline` type, no `Stage` trait, and no builder.**
The reasons:

1. Function composition in Rust already works, is zero-cost, and is readable.
2. A generic pipeline forces a single value type through every stage —
   almost certainly `String` — which destroys the `Cow` borrowing that
   `verbora-normalizers` and this design both depend on, and inserts an
   allocation per stage.
3. Stage order is *not* free. Diacritic-folding before romanizing is wrong for
   Greek (it destroys the dialytika that disambiguates `αϊ` from `αι`) and wrong
   for Vietnamese. A framework that makes arbitrary orders expressible invites
   callers to express wrong ones. Documented recipes make the correct order the
   easy path.
4. `AGENTS.md` § *Anti-Patterns* names "the reference architecture reproduced
   literally"; a stage-registry pipeline is exactly that shape.

### 8.1 The canonical composition

```rust
use std::borrow::Cow;
use verbora_normalizers::remove_diacritics;
use verbora_phonetics::SoundEx;          // `process` is an inherent method;
                                         // `verbora_core::Phonetic` is only
                                         // needed for generic code.
use verbora_distance::jaro_winkler;
use verbora_transliterate::{Transliterator, Strategy, CyrillicStandard, Language};

/// Builds a comparison key for a person's name written in any supported script.
fn name_key(raw: &str, lang: Language) -> String {
    // 1. Cross-script: Cyrillic → Latin, with the language's own standard.
    let t = Transliterator::new(Strategy::Cyrillic {
        standard: CyrillicStandard::BgnPcgn,
        language: lang,
    });
    let latin: Cow<'_, str> = t.transliterate(raw);

    // 2. Within-script: fold whatever Latin diacritics the standard emitted.
    //    (BGN/PCGN Ukrainian emits none; ISO 9 emits many. Folding is safe here
    //    *because* the previous step already committed to a lossy standard.)
    let folded: Cow<'_, str> = remove_diacritics(&latin);

    // 3. Phonetic key.
    SoundEx::new().process(&folded)
}

fn similar(a: &str, b: &str, lang: Language) -> bool {
    // Cheap exact test on the phonetic key…
    if name_key(a, lang) == name_key(b, lang) {
        return true;
    }
    // …then a graded textual test on the romanized forms.
    let t = Transliterator::new(Strategy::Cyrillic {
        standard: CyrillicStandard::BgnPcgn, language: lang,
    });
    jaro_winkler(&t.transliterate(a), &t.transliterate(b), &Default::default()) > 0.92
}
```

Each step is separately testable, separately benchmarkable, and separately
wrong-able. The `Cow` chain means an already-Latin, already-unaccented input
allocates only once, in `SoundEx::process`.

### 8.2 Ordering rules that must be documented

| Rule | Why |
|---|---|
| **Unicode-normalize before transliterating.** NFC (or NFD, per strategy) first. | `й` can be U+0439 or `и`+U+0306. A table keyed on U+0439 silently misses the decomposed form. See § 9.2. |
| **Transliterate before diacritic-folding.** | Folding first destroys Greek dialytika and Cyrillic `ё`/`е`, changing the romanization. |
| **Diacritic-fold before phonetic encoding.** | `verbora-phonetics` encoders are ASCII-oriented; `SoundEx` on `ž` is undefined-ish. |
| **Never phonetic-encode across scripts.** | `SoundEx("Александр")` is meaningless. Romanize first or not at all. |
| **Never transliterate a string twice.** | Romanized output is Latin; running a Cyrillic strategy over it is a no-op, but running a *Latin* strategy over it (if one is ever added) would compound losses. |

### 8.3 What composability does *not* mean

It does not mean this crate calls the others. `verbora-transliterate` depends on
`verbora-core` and nothing else in the workspace. The composition happens in the
caller (or in `verbora-examples` / the book's recipes). That keeps the crate
graph acyclic and lets someone use romanization without pulling phonetics.

---

## 9. Testing and benchmarking

### 9.1 Conformance vectors

Recorded, never transcribed — the same principle `docs/PARITY.md` states for
The reference parity, for the same reason: hand-written expectations encode the
implementer's reading of the standard, not the standard.

The difference from parity testing is that there is no executable reference to
record from. So the discipline is:

1. **Vectors come from a cited artefact**, one file per standard, with the
   citation in the file header: URL, title, retrieval date, and the specific
   table/section. A vector with no citation is not a vector.
2. **Every vector file is committed** under `tests/conformance/`, in a simple
   line format (`input<TAB>expected<TAB>note`), so a reviewer can diff it
   against the source document by eye.
3. **Disputed cells are marked, not silently resolved.** Where two readings of a
   standard disagree, the file records both and the test asserts the chosen one
   with a comment naming the other.

Where authoritative vectors can be obtained — to be confirmed by the implementer,
since availability changes:

| Standard | Likely source | Confidence |
|---|---|---|
| BGN/PCGN | NGA/PCGN published romanization tables (US Government works) | high — tables are published as PDFs with full letter inventories |
| ALA-LC | Library of Congress romanization tables (public PDFs) | high |
| UNGEGN | UN Group of Experts on Geographical Names working-paper reports, per language | high |
| ISO 9, ISO 843, ISO 233, ISO 259, ISO 15919, ISO 7098 | ISO — **paywalled**; see § 6 | tables are widely reproduced; the *documents* are not free |
| Revised Romanization (Korean) | National Institute of Korean Language (korean.go.kr) — the rules include worked examples | high |
| Hangul decomposition | UAX / Unicode Standard § 3.12; the arithmetic is normative | certain |
| Hepburn | Kenkyūsha's dictionary tables; widely reproduced | medium — "Modified Hepburn" has variants |
| Pinyin | GB/T 16159-2012; Unihan readings | medium |
| ISO 15919 / IAST | see § 2.8 | to be determined |

**A standard with no obtainable vectors does not ship.** That is the gate that
keeps "partial linguistic support" from being presented as production-ready.

### 9.2 Unicode tests

These apply to *every* strategy and belong in a shared test module:

* **Normalization forms.** For each strategy, and for a corpus of inputs in that
  script: `NFC`, `NFD`, `NFKC`, `NFKD` of the input. The requirement is
  explicit per strategy — either the strategy normalizes internally and all four
  agree, or it documents which form it requires and the others are *documented*
  divergences. Silence here is the bug: `й` (U+0439) vs `и` + U+0306 must not
  produce different romanizations by accident.
* **Combining marks.** Standalone combining marks, marks at string start, long
  runs of marks on one base, marks on characters from the wrong script.
* **Emoji and astral input.** `"😀"`, `"a😀b"`, `"👨‍👩‍👧‍👦"` (ZWJ sequence),
  flag sequences. Expected behaviour: passed through verbatim, classified
  `Unknown`, never split mid-scalar.
* **Unsupported characters inside supported runs.** `"Алекс😀андр"` — the
  romanization of the Cyrillic parts must be unaffected by the emoji.
* **Presentation forms.** Arabic U+FB50–FDFF / U+FE70–FEFF and halfwidth
  katakana U+FF61–FF9F must be handled or explicitly rejected, not silently
  passed through as if they were the base forms. (`verbora-transliterators`
  passes halfwidth katakana through untouched because the reference does; this
  crate must not.)
* **Bidi controls.** U+200E/U+200F/U+202A–U+202E in Arabic and Hebrew input.

### 9.3 The panic property

> **For every strategy `s` and every `&str` input `t`, `s.transliterate(t)`
> returns.** No panic, no unbounded loop, no `unreachable!`.

This is the single most important test in the crate, because it ingests
arbitrary text. `verbora-tokenizers` already encodes exactly this discipline in
its `nothing_panics_on_pathological_input` test over a 29-input battery including
`"\u{feff}"`, `"e\u{301}"`, `"İstanbul"` and `"\u{212a}\u{17f}"`. This crate
adopts the same battery plus its own, and additionally:

* **Property test** (`proptest` or `quickcheck`, as a dev-dependency only):
  `any::<String>()` → no panic, for every strategy. Note that `String` is
  already valid UTF-8, which is exactly the property being claimed.
* **Fuzz target** (`cargo-fuzz`, per `AGENTS.md` § *Fuzzing*): the same, driven
  by `Arbitrary`, plus a *round-trip* target for the bijective strategies —
  `inverse(transliterate(x)) == x` for every `x` in the source script.
* **Slicing discipline.** The workspace sets `clippy::indexing_slicing =
  "allow"` at the workspace level and expects it "enabled per-crate where
  inputs are pre-validated". This crate should **deny** it: its inputs are not
  pre-validated. All string access goes through `char_indices`, `get`, or
  checked slicing.

### 9.4 Round-trip tests

For every strategy whose `Reversibility` is `Bijective`, an inverse must exist
and must be tested exhaustively over the source alphabet plus a corpus:

* ISO 9 Cyrillic: exhaustive over all ~118 characters in the standard's tables,
  plus round-trip over a Russian/Ukrainian/Bulgarian/Serbian corpus.
* Hangul: exhaustive over **all 11,172** precomposed syllables U+AC00–U+D7A3.
  This is cheap and total, and it is the strongest correctness evidence in the
  whole crate.

For `Lossy` strategies, the test is the opposite: assert the *known collisions*
are still collisions (BGN/PCGN Russian `е` and `э` must both give `e`), so that
a future "improvement" that accidentally makes them distinct is caught.

### 9.5 Benchmark matrix

Following `benches/` conventions and `AGENTS.md` § *Benchmark API Variants*
(every API variant benchmarked, not just the ergonomic one):

| Axis | Levels |
|---|---|
| Input size | 16 B (a name), 1 KB (a paragraph), 1 MB (a document) |
| Input composition | pure target script; 50/50 mixed with Latin; pure Latin (the `Cow::Borrowed` path) |
| Strategy | one per script family, at its default standard |
| API shape | `transliterate` (allocating) / `transliterate_into` (warm buffer) / `transliterate_detailed` |
| Script detection | `script_of` per char; `script_runs` over each input |

Baselines to report, if and only if they are measured (§ *Honesty rules* forbids
invented numbers):

* `deunicode` on the same inputs — not an equivalence test (it produces
  different, non-conformant output) but a throughput reference point for
  "table-driven Unicode → ASCII in Rust".
* `verbora-transliterators` on Japanese kana input — same reason, and it makes
  the § 0 boundary visible in the benchmark output.
* The identity function, to establish the floor.

Report throughput (MiB/s), allocations per call, and the borrowed-vs-owned ratio
on mixed input. Results go in `docs/PERFORMANCE.md`; nothing goes in the book
until it is in there.

---

## 10. Scope boundary and phasing

### 10.1 Explicitly out of scope

Verbora provides **primitives**. The following are systems built *on* primitives
and are not part of this crate, this phase, or this project:

* **Inverted indexes.** No posting lists, no term dictionaries, no segment
  merging. Use `tantivy`.
* **Ranking and scoring.** No BM25, no TF-IDF *ranking* (the existing
  `verbora-tfidf` work implements the *statistic*, verified against the reference,
  not a retrieval engine), no learning-to-rank.
* **Query engines.** No query parser, no boolean/phrase/proximity operators, no
  query planner.
* **Retrieval.** No candidate generation, no blocking/bucketing strategies, no
  approximate nearest neighbour, no embeddings.
* **Entity databases.** No name lists, no sanctions lists, no gazetteers, no
  place-name authority data. This crate romanizes what it is given; it does not
  know who anyone is.
* **Translation.** § 1.1, item 7. Not now, not later.
* **Language identification.** `script_runs` reports *scripts*. It does not
  report languages, and `guess_cyrillic_language` (§ 4.5) is explicitly a
  heuristic side-utility, not a language identifier. If real language ID is
  wanted, it is a different crate with a different evaluation methodology.
* **Full morphological analysis of Japanese.** § 2.6. That is `lindera` /
  `vibrato` territory: a 50–100 MB dictionary and a Viterbi decoder. Verbora
  should *interoperate* with such a crate, not contain one.
* **Speech: IPA, G2P, TTS front-ends.** Phonetic *encoding* (§ 1.1 item 4) is in
  scope because the reference has it. Phonetic *transcription* to IPA is not.

### 10.2 Phasing

The gate for "ships": a standard is implemented completely, has cited
conformance vectors that pass, has a documented `Reversibility`, and has no
known input class that produces silently-wrong output.

#### Phase 1 — ships in the first production-quality release

| Deliverable | Why it can reach production quality now |
|---|---|
| `Script`, `script_of`, `script_runs` | Pure Unicode data; total function; exhaustively testable. |
| **Cyrillic**, ISO 9 + BGN/PCGN + national, per language | Table-driven, small, standards are published, per-language mappings are enumerable, ISO 9 gives an exhaustively testable bijection. |
| **Greek**, modern | One language; contextual rules are a bounded, enumerable set; vectors obtainable. |
| **Hangul → Revised Romanization** | Decomposition is normative arithmetic (§ 2.7); exhaustive round-trip over 11,172 syllables; official rules published with examples. |
| **Kana → Hepburn / Kunrei-shiki / Nihon-shiki** | Closed table problem (§ 2.6). *Kana only* — kanji explicitly rejected, not passed through. |
| **Arabic normalization** (within-script) | A well-defined, widely-implemented search-normalization operation, distinct from transliteration (§ 2.3). |
| The API shapes in § 4.4, the `Reversibility` type, refusal logic in § 4.5 | |

#### Phase 2 — designed for, deliberately deferred

| Deferred | Ships when |
|---|---|
| **Arabic → Latin transliteration** | The unvocalised-text question in § 2.3 has a documented answer and the chosen standard's vectors pass. Shipping a mapping that silently invents vowels is worse than shipping nothing. |
| **Hebrew → Latin** | Same, for niqqud vs unpointed (§ 2.4). A strategy that only handles *pointed* text is defensible and small — that may be the phase-2 shape. |
| **Chinese → Pinyin** | The licence question in § 6 is resolved *and* the accuracy claim in § 2.5 can be stated honestly with a measured number. Single-character toneless pinyin from Unihan may ship earlier under a name that says what it is. |
| **Japanese Kanji readings** | Only as an *integration* with an external analyser, never as an in-crate dictionary. Licence + size (§ 5.2, § 6). |
| **Devanagari / Indic** | § 2.8's conclusion. If transliteration (ISO 15919/IAST) is genuinely mechanical it may be pulled into phase 1; romanization-as-pronunciation is a phase-3-or-never item. |
| `no_std` support | After the `std` build is stable. |
| Inverse transliteration (`untransliterate`) | After phase 1 ships and the bijective strategies have a proven forward direction. |

#### The conservatism rule

**Anything that cannot reach production quality is deferred, and the reason is
recorded.** Concretely, the following would each be a violation of this design:

* Shipping Arabic transliteration that quietly assumes vowels.
* Shipping "Chinese pinyin" that is per-character Unihan lookup, without the
  documentation saying exactly what accuracy that achieves on running text.
* Shipping Japanese romanization that passes kanji through unchanged. (That is
  what `verbora-transliterators` does, correctly, *because that is the reference's
  behaviour*. In a standards-governed crate it is a defect.)
* Shipping a `Devanagari` variant that handles Sanskrit but is quietly wrong for
  Hindi, or vice versa, without the `Language` parameter distinguishing them.
* Presenting any of the above in `README.md`, the book's feature list, or the
  crate description as supported.

Partial support gets a name that says it is partial (`romanize_kana`, not
`romanize_japanese`), lives behind a feature or a clearly-labelled function, and
appears on the roadmap page rather than the features page — the rule
`AGENTS.md` already states for placeholder crates.

---

## 11. Open questions and risks

### 11.1 R1 — The workspace is not a git repository *(blocking, non-linguistic)*

`ls -d .git` returns nothing. `.github/workflows/docs.yml` exists and is
well-developed: six gates, then `actions/upload-pages-artifact@v3` and
`actions/deploy-pages@v4`, conditioned on `github.ref == 'refs/heads/main'`.
`Cargo.toml` declares `repository = "https://github.com/addlayerio/verbora"`.

So the publishing pipeline is fully specified and **cannot run**, because:

* there is no git history, so no `main` branch and no `github.ref`;
* nothing has been pushed to `addlayerio/verbora`, so GitHub Pages has no
  source, no `github-pages` environment, and no Pages configuration;
* seven agents are concurrently writing crates with **no version control**,
  which is a much larger risk than the Pages one: there is no way to review a
  diff, revert a bad change, or attribute a regression.

**Recommended action, in order:** (1) `git init`, commit the current tree as a
baseline *before* more concurrent edits land; (2) create
`github.com/addlayerio/verbora` and push; (3) enable Pages with source
"GitHub Actions" and create the `github-pages` environment; (4) verify the docs
workflow end-to-end on a throwaway branch. Steps 1 and 2 are prerequisites for
*any* of this design being reviewable.

This document does not perform any of those steps — it is outside its remit and
the working tree is concurrently owned by other agents.

### 11.2 R2 — Crate naming and the `verbora-` prefix

Every existing crate is `verbora-*`. This proposes `verbora-transliterate`,
following the rebrand visible in `README.md`, the book, and the `repository`
URL. Open questions the workspace owner must settle:

* The rebrand is settled: every crate is `verbora-*`, so this crate is named
  for the end state directly, with no re-export shims.
* Is `verbora-transliterate` or `verbora-translit` preferred? (Recommend the
  full word; `verbora-transliterators` uses the long form.)
* Are both names available on crates.io? **Not verified.**

### 11.3 R3 — Licence contamination is the biggest technical risk

See § 6. If the datasets that make Chinese and Japanese-Kanji correct are
share-alike, then an MIT crate cannot vendor derived binaries of them without
either (a) accepting the share-alike obligation on that derived data, (b) moving
that data to a separate, separately-licensed crate, or (c) downloading at build
time — which breaks offline builds, reproducible builds, and `crates.io`
packaging, and is therefore not acceptable. **Recommendation (b)**, plus a
`THIRD_PARTY_LICENSES.md` at the workspace root as `AGENTS.md` § *Licensing*
requires.

### 11.4 R4 — Standards drift and version pinning

Standards get revised (BGN/PCGN Bulgarian changed in 2013; ISO 15919 has
amendments; Unihan changes every Unicode release). The crate must pin: every
`*Standard` enum variant's doc comment states the exact edition/year it
implements, and every generated table carries the source artefact's version and
retrieval date in a header comment. A standard revision is a **new enum
variant**, never a silent change to an existing one — that is the only way a
caller's output stays stable across a patch release.

### 11.5 R5 — The `Language` parameter is a usability tax

Requiring `Language` for Cyrillic is correct (§ 4.5) and will be unpopular:
callers who have a mixed-language corpus and no metadata cannot use the API at
all. Mitigations: the `romanize_ru`/`romanize_uk`/… ergonomic functions make the
common single-language case one call; `AmbiguousScript::LanguageRequired` names
the candidates so the error is actionable. **The mitigation must not become
"pick Russian by default"** — Russian-as-default is a real bug in several
shipping libraries and is exactly § 1.3's silent failure.

### 11.6 R6 — Things this document could not verify

Recorded so they are not mistaken for settled:

* Availability of machine-readable conformance vectors for most standards
  (§ 9.1). The confidence column is a judgement, not a check.
* crates.io name availability (§ 11.2).
* Exact byte sizes in § 5.2/§ 5.3 — these are estimates from cited upstream
  artefact sizes, not measurements of an implementation that does not exist.
* Whether the six existing PARITY_VERIFIED crates' actual generated-table
  conventions accommodate a table generated from Unicode
  data rather than from the reference runtime. The existing generator "dumps
  the reference's own tables at runtime rather than transcribing them", which
  has no analogue here; a Unicode-sourced generator is a new pattern and needs
  the same self-proving assertions (§ 3.4).
* Anything marked "not verified" in § 2 and § 6.

---
