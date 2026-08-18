# Zero-copy and `Cow`

Verbora tries very hard not to copy your text. Three mechanisms do the work:
borrowed tokens, `Cow` returns, and exact fast paths that avoid re-encoding
input. This page explains each, and what it means for the code you write.

## 1. Borrowed tokens

<a class="badge badge-zerocopy" href="../features/tokenizers">ZERO-COPY</a>

A tokenizer that only ever *cuts* its input can hand back slices of it. Fourteen
of Verbora's tokenizers do:

```rust
use verbora_tokenizers::{AggressiveTokenizer, Tokenize};

let text = String::from("the quick brown fox");
let t = AggressiveTokenizer::new();

let tokens: Vec<&str> = t.tokenize(&text);

// Each token points into `text`. Nothing was copied.
assert_eq!(tokens[1], "quick");
assert_eq!(tokens[1].as_ptr(), text[4..].as_ptr());
```

The `Vec` itself is a heap allocation; the tokens in it are not. For a
9.7 kB document that is one allocation instead of fifteen hundred.

The token type tells you which mechanism a tokenizer uses:

| Token type | Meaning | Tokenizers |
|---|---|---|
| `&'a str` | pure slicing — zero copies | the 13 character-class tokenizers, `WordTokenizer` |
| `Cow<'a, str>` | slices when the pre-pass changed nothing | `AggressiveTokenizerNo`, `…Sv`, `…Hi` |
| `Utf16Token<'a>` | slices unless a cut lands inside a surrogate pair | `WordPunctTokenizer`, `TreebankWordTokenizer`, `TokenizerJa`, `CaseTokenizer`, `OrthographyTokenizer` |
| `String` | the text is rewritten, so copying is unavoidable | `SentenceTokenizer` |

<div class="callout callout-note">
<strong>The price of borrowing is lifetimes.</strong> Tokens cannot outlive the
text they point into. If you need them to, call
<code>.to_owned()</code> / <code>.into_owned()</code> — deliberately, at the
boundary where ownership actually changes hands.
</div>

## 2. `Cow`: borrow until something changes

<a class="badge badge-cow" href="../features/normalizers">COW</a>

`Cow<'a, str>` is either a borrow of the input or an owned `String`. Verbora's
normalizers return it because **they are usually called on text that needs no
change at all** — a Latin sentence handed to the katakana converter, an ASCII
token handed to the diacritic folder.

```rust
use std::borrow::Cow;
use verbora_normalizers::remove_diacritics;

// No diacritics to fold: the input is handed straight back.
let a = remove_diacritics("plain ascii text");
assert!(matches!(a, Cow::Borrowed(_)));

// One fold: allocates once, at the first character that actually differs.
let b = remove_diacritics("café crème");
assert!(matches!(b, Cow::Owned(_)));
assert_eq!(b, "cafe creme");
```

You can usually ignore the distinction — `Cow<str>` derefs to `&str`, compares
against `&str`, and formats like one. It matters when you want to *know*:

```rust
use std::borrow::Cow;
use verbora_normalizers::normalize_ja;

fn was_changed(s: &str) -> bool {
    matches!(normalize_ja(s), Cow::Owned(_))
}

assert!(!was_changed("hello"));
```

### Carrying the borrow through a pipeline

A naive multi-stage pipeline allocates once per stage. Verbora's multi-stage
normalizers thread the `Cow` through instead, so a four-stage pipeline over
unchanged text still allocates nothing. The technique is worth copying when you
build your own pipelines:

```rust
use std::borrow::Cow;
use verbora_normalizers::{ja::converters, remove_diacritics};

fn pipeline(input: &str) -> Cow<'_, str> {
    // Each stage takes &str and returns Cow. Matching on the previous stage's
    // result keeps a borrow alive instead of forcing an allocation to continue.
    match remove_diacritics(input) {
        Cow::Borrowed(s) => converters::katakana_to_hiragana(s),
        Cow::Owned(s) => Cow::Owned(converters::katakana_to_hiragana(&s).into_owned()),
    }
}

assert!(matches!(pipeline("plain"), Cow::Borrowed(_)));
```

The `Cow::Owned` arm is where the cost is: once *any* stage has allocated, later
stages work on the owned buffer and their results must be re-owned to escape the
function. That is one allocation for the pipeline, not one per stage.

## 3. Exact fast paths

The subtlest form of not-copying: Verbora indexes strings by UTF-16 code
unit, and that is observable in the results — `levenshtein("a😀b", "ab")`
returns **2**, not 1 as a naive `char`-based implementation would give.
Getting that right could mean converting every input to `Vec<u16>` on every call.

Verbora does not, because for ASCII **one byte is one code unit**:

```text
input
  │
  ├── all ASCII?  ──yes──▶  operate on &[u8]        ← borrowed, no conversion
  │
  └── no ────────────────▶  promote to Vec<u16>     ← one allocation, exact
```

So the exact-UTF-16 guarantee is free on ordinary text and costs one allocation
on text that genuinely needs it. This is a *fast path*, not a *shortcut*: both
branches produce the same answer, because for ASCII the two representations are
identical by construction.

The mechanism lives in `verbora_distance::units` and `verbora_phonetics::units`.

## When copying is unavoidable

Being honest about the other direction:

- **`SentenceTokenizer`** substitutes placeholders into the text before
  splitting, so its tokens are `String`.
- **`Phonetic::process`** returns an owned `String` per call. Phonetic keys are
  computed, not sliced — there is nothing in the input to borrow.
- **`normalize` / `normalize_token`** return `Vec<String>`, because one
  contraction expands into several tokens.
- **`Trie::keys_with_prefix`** returns `Vec<String>`: trie keys are reconstructed
  by walking the tree, so there is no contiguous source to slice.
- **The Levenshtein family** allocates a working structure of its own on every
  call — a handful of bit-vector words in the common unit-cost cases (plain and
  restricted Damerau alike), rows or per-symbol row snapshots in the weighted
  and unrestricted-Damerau modes.

## What the fast path is worth

`verbora-distance` is where the effect is published, and it shows up as the gap
between the borrowed-`&[u8]` path and the promoted-`Vec<u16>` path on identical
input lengths:

| Benchmark | Representation | Time |
|---|---|--:|
| `levenshtein/ascii/16` | borrowed `&[u8]` | 41.8 ns |
| `levenshtein/cyrillic/16` | promoted `Vec<u16>` | 266.4 ns |
| `levenshtein/ascii/256` | borrowed `&[u8]` | 2.13 µs |
| `levenshtein/cyrillic/256` | promoted `Vec<u16>` | 3.97 µs |

The promotion is a fixed cost paid once per operand, so its share shrinks as the
comparison grows: the promoted path costs 6.4× the borrowed one at 16 units but
only 1.9× at 256. Exact UTF-16 semantics are free on ASCII text, and stay under
four microseconds even on a 256-unit non-ASCII comparison.

Where there is nothing to copy in the first place, there is nothing to save:
`hamming` on ASCII is a single scan that allocates nothing at all — 6.6 ns at
four units, 275.3 ns at 1024.

## Related

- [Allocation behaviour](allocation.md) — the per-API reference.
- [Normalizers](../features/normalizers.md) — the `Cow` story in full.
- [Benchmarks: string distance](../benchmarks/distance.md) — the full results
  table, including the bit-vector Levenshtein kernels.
