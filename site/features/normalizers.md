# Normalizers

`verbora-normalizers` rewrites text, and says in each function's name exactly
what the rewrite is. Five functions: four are the Unicode normalization forms of
[UAX #15](https://www.unicode.org/reports/tr15/), the fifth is a Verbora-defined
combining-mark fold built out of them.

| Function | Rewrite | Authority |
|---|---|---|
| `nfd` | Canonical Decomposition | UAX #15 §1.2 |
| `nfc` | Canonical Decomposition, then Canonical Composition | UAX #15 §1.2 |
| `nfkd` | Compatibility Decomposition | UAX #15 §1.2 |
| `nfkc` | Compatibility Decomposition, then Canonical Composition | UAX #15 §1.2 |
| `remove_diacritics` | NFD, drop every scalar with `Canonical_Combining_Class != 0`, NFC | Verbora, defined on the item |

This is the one crate in Verbora's text-shaping group whose job *is* rewriting.
[Tokenizers](./tokenizers.md) and [n-grams](./ngrams.md) never alter the text
they are given; every function here does, and none of them hides it behind a name
that suggests otherwise.

This is also the crate where `Cow` earns its keep. Normalizers are usually called
on text that needs no change at all — an ASCII token handed to the diacritic
fold, an already-composed string handed to `nfc`. Every function returns
`Cow::Borrowed` when and only when it changed nothing.

<div class="callout callout-spec">
<strong>Specification status.</strong> Five functions plus one parallel batch
entry point and one version accessor — the crate root is the whole public
surface, and nothing here is fallible.
<code>cargo test -p verbora-normalizers --all-features</code> runs
<strong>31</strong> tests and <strong>10</strong> doctests, including a
conformance suite that replays the Unicode Character Database's own
<code>NormalizationTest.txt</code>.
</div>

## When to use it

- You want one canonical spelling per abstract character before storing,
  comparing or hashing text → `nfc`.
- You want a matching key that ignores width, ligation and circling — halfwidth
  katakana, fullwidth Latin, `ﬁ`, `Ⅻ` → `nfkc`.
- You want accent-insensitive matching of Latin-script text →
  `remove_diacritics`.
- You are about to inspect or filter combining marks yourself → `nfd` or `nfkd`.

## When not to use it

- **You want case folding.** Nothing here folds case. `ß → ss` is case folding
  ([UAX #21](https://www.unicode.org/reports/tr21/)), not diacritic removal, and
  `remove_diacritics("ß")` is `"ß"`.
- **You want to fold Thai or Devanagari blindly.** `remove_diacritics` removes
  every non-zero combining class, which for those scripts changes the word rather
  than de-accenting it — read [What survives](#what-survives-and-why) first.
- **You want transliteration.** Kana → romaji is
  [Transliterators](./transliterators.md); this crate never changes script.
- **You want whitespace trimming, stopword removal or contraction expansion.**
  None of these functions do any of that.

## Quick example

```rust
use verbora_normalizers::{nfc, nfkc, remove_diacritics};

fn main() {
    assert_eq!(nfc("e\u{0301}"), "é");                 // compose
    assert_eq!(nfkc("ｶﾞ"), "ガ");                       // width- and mark-fold
    assert_eq!(remove_diacritics("crème brûlée"), "creme brulee");
}
```

## Choosing the right API

All five functions have the same shape — `&str` in, `Cow<str>` out — so the
choice is entirely about which rewrite you want.

| Call | Use when | Loses | Allocates |
|---|---|---|---|
| `nfc` | you want one canonical spelling per abstract character, for storage, comparison or hashing. **The default for text you will show a human again.** | nothing: the result is canonically equivalent to the input, so only the spelling changes | one `String`, and only if the input was not already NFC |
| `nfd` | you are about to inspect or filter combining marks yourself | nothing, for the same reason | as `nfc` |
| `nfkc` | you want a *matching* key that ignores width, ligation and circling | formatting distinctions, irreversibly | as `nfc` |
| `nfkd` | the same, and you will inspect the marks yourself | as `nfkc` | as `nfc` |
| `remove_diacritics` | you want accent-insensitive matching of Latin-script text | every combining mark — read the table below before using it on non-Latin text | nothing for ASCII; one `String` otherwise |
| `par_remove_diacritics_batch` (feature `parallel`) | many independent documents at once | the same as `remove_diacritics` | one `Vec` plus the per-input cost |

A decision tree, for the common question "which one do I store?":

```text
Do you need the text back, readable, exactly as written?
 ├─ yes → nfc
 └─ no, it is a lookup key
     ├─ must "ｱ" match "ア" and "Ａ" match "A"?
     │   ├─ yes → nfkc
     │   └─ no  → nfc
     └─ must "resume" match "résumé"?
         └─ yes → remove_diacritics (after nfkc, if you also wanted that)
```

The forms compose in the obvious way and nothing here does it for you:
`remove_diacritics(&nfkc(text))` is the width-, ligature- and
accent-insensitive key, spelled out so both rewrites are visible at the call
site.

```rust
use verbora_normalizers::{nfc, nfd, nfkc, nfkd, remove_diacritics};

fn main() {
    // Canonical: the spelling changes, the abstract characters do not.
    assert_eq!(nfd("é"), "e\u{0301}");
    assert_eq!(nfc("e\u{0301}"), "é");

    // Compatibility: formatting distinctions are folded away, irreversibly.
    assert_eq!(nfkc("ﬁ"), "fi");
    assert_eq!(nfkc("Ⅻ"), "XII");
    assert_eq!(nfkc("Ａ"), "A");
    assert_eq!(nfkc("Ⓐ"), "A");
    assert_eq!(nfkd("ｶﾞ"), "カ\u{3099}");
    assert_eq!(nfkc("ｶﾞ"), "ガ");

    // The composition, written out: width, ligature and accent insensitive.
    assert_eq!(remove_diacritics(&nfkc("ﬁancée")), "fiancee");
}
```

## Every function returns `Cow`, and the borrow means something

Each of the five returns `Cow::Borrowed` **if and only if** the result is
byte-identical to the input. That is a guarantee, not a description of a fast
path: branching on it is correct code, not an optimisation that might stop
working.

```rust
use std::borrow::Cow;
use verbora_normalizers::{nfc, remove_diacritics};

fn main() {
    let key = match nfc("already composed") {
        Cow::Borrowed(s) => s.to_owned(), // the input was already in NFC
        Cow::Owned(s) => s,               // it was not, and `s` is the NFC form
    };
    assert_eq!(key, "already composed");

    // Pure ASCII returns immediately, borrowed — exact, not a heuristic.
    assert!(matches!(remove_diacritics("plain ascii"), Cow::Borrowed(_)));
    assert!(matches!(remove_diacritics("résumé"), Cow::Owned(_)));
}
```

The quick-check properties of UAX #15 §9 decide this without materialising the
result in the common case; where a quick check answers `Maybe`, the
implementation compares and still returns `Borrowed` when it can.

## `remove_diacritics`

> `remove_diacritics(s)` is `s` under Canonical Decomposition (NFD), with every
> scalar whose `Canonical_Combining_Class` is non-zero removed, under Canonical
> Composition (NFC).

Three parts of that sentence are load-bearing:

- **NFD first**, so the answer does not depend on how the text was typed.
  `remove_diacritics("é")` and `remove_diacritics("e\u{0301}")` are both `"e"`.
- **`ccc != 0`, not `General_Category ∈ {Mn, Mc, Me}`.** The non-zero classes are
  exactly the marks canonical ordering reorders, which is the technical sense of
  "accent". Stripping all marks instead would destroy Thai, Indic and Hangul text
  wholesale.
- **NFC last**, because NFD decomposes Hangul syllables into `ccc = 0` jamo and
  only composition puts them back. Without it `remove_diacritics("한국")` would
  return decomposed jamo — a different string that renders identically.

### Guarantees

- **The output is in NFC.**
- **Idempotent.** `remove_diacritics(remove_diacritics(s)) == remove_diacritics(s)`,
  for every `s`.
- **Independent of the input's normalization form.**
- **Every occurrence folds, not the first.** `remove_diacritics("ààà")` is
  `"aaa"`.
- **Position-independent**: the same word folds the same way wherever it appears
  in a document.
- `Cow::Borrowed` if and only if the result is byte-identical to the input.

```rust
use verbora_normalizers::{nfd, remove_diacritics};

fn main() {
    assert_eq!(remove_diacritics("piñon ça va über résumé"), "pinon ca va uber resume");

    // Independent of how the text was typed, and idempotent.
    assert_eq!(remove_diacritics("e\u{0301}"), "e");
    assert_eq!(remove_diacritics(&nfd("résumé")), "resume");
    assert_eq!(remove_diacritics(&remove_diacritics("résumé")), "resume");

    // Every occurrence, not the first.
    assert_eq!(remove_diacritics("ààà"), "aaa");

    // Letters whose mark is part of their identity are left alone.
    assert_eq!(remove_diacritics("blåbærsyltetøy"), "blabærsyltetøy");

    // Hangul comes back composed, not as jamo.
    assert_eq!(remove_diacritics("한국"), "한국");
}
```

### What survives, and why

| Input | Result | Reason |
|---|---|---|
| `ø`, `Æ`, `đ`, `ł`, `ħ`, `ŋ`, `ı` | unchanged | empty `Decomposition_Mapping`; the stroke or bar is part of the letter's identity, not a mark applied to it |
| `ß` | `ß` | not a diacritic. `ß → ss` is *case folding*, UAX #21 |
| `Ａ` (fullwidth), `Ⓐ` (circled), `ǅ`, `ſ` | unchanged | *compatibility* decompositions, not canonical. Compose with `nfkc` first if that is what you want |
| `Å` U+212B | `A` | canonical singleton to U+00C5, which decomposes to `A` + U+030A |
| `İ` U+0130 | `I` | canonical decomposition `0049 0307`, and `ccc(U+0307) = 230` |
| Devanagari matras, Thai `SARA I`-class vowel signs, Hangul jamo | unchanged | `ccc = 0` |
| Hebrew niqqud (`ccc` 10–26), Arabic harakat (`ccc` 27–34), Devanagari nukta (`ccc` 7) | removed | non-zero, and this is the operation those scripts call diacritic removal |
| Devanagari virama U+094D (`ccc` 9), Thai tone marks (`ccc` 107), Thai `SARA U`/`SARA UU` (`ccc` 103) | **removed** | also non-zero. `ccc != 0` is not the same as "is an accent" |

```rust
use verbora_normalizers::{nfkc, remove_diacritics};

fn main() {
    // Not diacritics: identity is in the letter, or the mapping is compatibility.
    assert_eq!(remove_diacritics("ß"), "ß");
    assert_eq!(remove_diacritics("ø"), "ø");
    assert_eq!(remove_diacritics("ſ"), "ſ");
    assert_eq!(remove_diacritics("Ⓐ"), "Ⓐ");
    assert_eq!(nfkc("ſ"), "s"); // …which is what nfkc is for

    // Canonical decompositions that do fold.
    assert_eq!(remove_diacritics("İ"), "I");

    // Hebrew and Arabic: this is what those scripts call diacritic removal.
    assert_eq!(remove_diacritics("שָׁלוֹם"), "שלום");
}
```

<div class="callout callout-warn">
<strong>Careful.</strong> That last table row is the honest limit of the
definition. <code>remove_diacritics</code> is safe to apply blindly to
Latin-script text and is <strong>not</strong> safe to apply blindly to Thai or
Devanagari: <code>remove_diacritics("ก้")</code> is <code>"ก"</code>, which drops
a tone mark rather than an accent. It is a fold for matching and indexing, not a
transliteration.
</div>

## Nothing here can fabricate a replacement character

There is no UTF-16 anywhere in this crate — no public type, no internal buffer,
no index. The direct consequence is that **nothing can emit `U+FFFD` unless
`U+FFFD` was in the input**: an astral scalar has no halves to split.

```rust
use verbora_normalizers::{nfkc, remove_diacritics};

fn main() {
    assert_eq!(nfkc("😀々"), "😀々");
    assert_eq!(remove_diacritics("😀々"), "😀々");
}
```

## Parallelism

`par_remove_diacritics_batch` is behind the `parallel` Cargo feature
(`parallel = ["dep:rayon"]`, never on by default). Its whole body is
`inputs.par_iter().map(remove_diacritics).collect()` — one task per document,
order preserving, not a second implementation of the fold.

```rust  ignore
// Needs the `parallel` feature, which this site's snippet checker builds
// without — so this block is marked `ignore` rather than compiled.
use verbora_normalizers::par_remove_diacritics_batch;

let docs = ["résumé", "plain ascii", "crème brûlée"];
let folded = par_remove_diacritics_batch(&docs);
assert_eq!(folded[0], "resume");
```

There is deliberately **no** `par_nfc_batch` and no sibling for the other three
forms. They are thin adapters with no Verbora-side per-item work to fan out, so
`inputs.par_iter().map(nfc).collect()` at the call site is the same code with one
fewer name to learn. See [Parallelism](../performance/parallelism.md).

## The Unicode version is part of the contract

Normalization is defined by UCD properties — `Decomposition_Mapping`,
`Canonical_Combining_Class`, `Composition_Exclusion` — so this crate cannot
promise results frozen for all time. A frozen table would be wrong for every
character encoded after the freeze, which is worse than a moving one.

- The Unicode version is whichever version the normalization dependency ships,
  pinned in `Cargo.lock`. At the version this crate is built against that is
  **Unicode 17.0.0**, and `unicode_version()` reports it at run time.
- A UCD upgrade is a **semver-visible behaviour change** and is released as one.
- **Any structure that persists normalizer-derived keys must stamp the Unicode
  version and refuse to load across a change.** A search index, a trained model
  or an interned term table built before an upgrade does not match one built
  after it, and nothing else will tell you.

```rust
fn main() {
    // The version is a fact about the build, not a constant to hardcode —
    // record it, compare it, refuse to load an artifact whose stamp differs.
    let (major, _minor, _update) = verbora_normalizers::unicode_version();
    assert!(major >= 17);
}
```

Within one Unicode version the crate is fully deterministic: same input, same
output, on every platform and every build. There is no global mutable state, no
hash-order dependence, no interior mutability and no floating point.

## Performance and allocation

- **`remove_diacritics` allocates nothing for pure-ASCII input**, which returns
  immediately. That is exact rather than a heuristic: every ASCII scalar is its
  own NFD and NFC and has combining class 0, so the definition is the identity on
  ASCII. Nothing is allocated either for text that contains no non-zero-class
  scalar and is already in NFC. One `String` otherwise.
- **The four forms allocate one `String`**, and only when the input was not
  already in the target form.

**Timings are unmeasured.** No benchmark has been run against the current
implementation of any function in this crate, and no figure is estimated in
place of one. The allocation behaviour above is a property of the
implementation and is stated as such; no timing claim is made, and none should
be inferred. See [Benchmarks](../benchmarks/index.md).

## Common mistakes

**Expecting `remove_diacritics` to fold `ß`, `ø` or `ł`.** Those letters have no
canonical decomposition; the mark is part of the letter's identity. If you want
`ø → o` you want a transliteration table, not a normalization form.

**Expecting `remove_diacritics` to fold `Ａ` or `ſ`.** Those are *compatibility*
decompositions. Run `nfkc` first if that is what you want.

**Applying `remove_diacritics` to Thai or Devanagari.** It removes every non-zero
combining class, including tone marks and the virama.

**Comparing an unnormalized key against a normalized one.** `"e\u{0301}"` and
`"é"` are different strings and hash differently. Normalize both sides, with the
same function.

**Persisting normalizer-derived keys without a Unicode stamp.** Mappings move
between Unicode versions; an index built under one and queried under another
mismatches silently rather than failing.

## Related

- [Tokenizers](./tokenizers.md) — what to run before or after normalizing
- [N-grams](./ngrams.md) — the third text-shaping crate, and the one whose
  output is stable across Unicode versions
- [Transliterators](./transliterators.md) — changing script, which is a different
  operation
- [Zero-copy](../performance/zero-copy.md) ·
  [Allocation](../performance/allocation.md) ·
  [Parallelism](../performance/parallelism.md)
- [Benchmarks](../benchmarks/index.md) · [Recipes](../recipes/index.md)

## API reference

```bash
cargo doc -p verbora-normalizers --no-deps --open
```

```rust ignore
// verbora_normalizers — the crate root is the whole public surface
pub fn nfd(s: &str) -> Cow<'_, str>;
pub fn nfc(s: &str) -> Cow<'_, str>;
pub fn nfkd(s: &str) -> Cow<'_, str>;
pub fn nfkc(s: &str) -> Cow<'_, str>;
pub fn remove_diacritics(s: &str) -> Cow<'_, str>;

pub fn unicode_version() -> (u64, u64, u64);

#[cfg(feature = "parallel")]
pub fn par_remove_diacritics_batch<'a>(inputs: &[&'a str]) -> Vec<Cow<'a, str>>;
```

Every function is `#[must_use]`, nothing is fallible, and nothing holds state:
there is no builder, no options struct and no trait to implement.

Source: `crates/verbora-normalizers/src/`.
