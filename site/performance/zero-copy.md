# Zero-copy and `Cow`

Verbora tries very hard not to copy your text. Three mechanisms do the work:
borrowed tokens, `Cow` returns, and exact fast paths that avoid re-encoding
input. This page explains each, and what it means for the code you write.

## 1. Borrowed tokens

<a class="badge badge-zerocopy" href="../features/tokenizers">ZERO-COPY</a>

A tokenizer that only ever *cuts* its input can hand back slices of it. Every
tokenizer Verbora ships does, because none of them rewrites the text:

```rust
use verbora_tokenizers::{BorrowingTokenizer, WordTokenizer};

let text = String::from("the quick brown fox");

let tokens: Vec<&str> = WordTokenizer.tokenize_borrowed(&text);

// Each token points into `text`. Nothing was copied.
assert_eq!(tokens[1], "quick");
assert_eq!(tokens[1].as_ptr(), text[4..].as_ptr());
```

The `Vec` itself is a heap allocation; the tokens in it are not. For a
9.7 kB document that is one allocation instead of fifteen hundred.

| Call | Token type | Meaning |
|---|---|---|
| `tokens` / `tokenize_borrowed` / `tokenize_borrowed_into` | `&'a str` | pure slicing — zero copies |
| `Tokenizer::tokenize` / `tokenize_into` / `tokenize_batch` | `String` | the owned path: one `String` per token, for tokens that must outlive the input |

That there is only one token *shape* is deliberate. A tokenizer that rewrote its
input could not be composed with one that did not, and its tokens would not be
substrings — which is the guarantee everything downstream is built on.

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
change at all** — an already-composed string handed to `nfc`, an ASCII token
handed to the diacritic fold. For those five functions the borrow is a
*guarantee*: `Cow::Borrowed` if and only if the result is byte-identical to the
input, so branching on it is correct code rather than an optimisation that might
stop working.

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
use verbora_normalizers::nfkc;

fn was_changed(s: &str) -> bool {
    matches!(nfkc(s), Cow::Owned(_))
}

assert!(!was_changed("hello"));
assert!(was_changed("ﬁ"));
```

### Carrying the borrow through a pipeline

A naive multi-stage pipeline allocates once per stage. Threading the `Cow`
through instead means a pipeline over unchanged text allocates nothing at all.
Verbora deliberately does *not* pre-compose the normalization forms for you —
`remove_diacritics(&nfkc(text))` keeps both rewrites visible at the call site —
so this is the technique to use when you build your own:

```rust
use std::borrow::Cow;
use verbora_normalizers::{nfkc, remove_diacritics};

fn key(input: &str) -> Cow<'_, str> {
    // Each stage takes &str and returns Cow. Matching on the previous stage's
    // result keeps a borrow alive instead of forcing an allocation to continue.
    match nfkc(input) {
        Cow::Borrowed(s) => remove_diacritics(s),
        Cow::Owned(s) => Cow::Owned(remove_diacritics(&s).into_owned()),
    }
}

assert!(matches!(key("plain ascii"), Cow::Borrowed(_)));
assert_eq!(key("ﬁancée"), "fiancee");
```

The `Cow::Owned` arm is where the cost is: once *any* stage has allocated, later
stages work on the owned buffer and their results must be re-owned to escape the
function. That is one allocation for the pipeline, not one per stage.

## 3. Exact fast paths

The subtlest form of not-copying: `verbora-distance` counts in Unicode scalar
values, and that is observable in the results — `levenshtein("a😀b", "ab")`
returns **1**, because the emoji is a single unit. Getting that right could mean
converting every input to `Vec<char>` on every call.

Verbora does not, because for ASCII **one byte is one scalar**:

```text
input
  │
  ├── all ASCII?  ──yes──▶  operate on &[u8]        ← borrowed, no conversion
  │
  └── no ────────────────▶  promote to Vec<char>    ← one allocation, exact
```

So the exact-scalar guarantee is free on ordinary text and costs one allocation
on text that genuinely needs it. This is a *fast path*, not a *shortcut*: both
branches produce the same answer, because for ASCII the two representations are
identical by construction.

In `verbora-distance` the branch is an internal detail with nothing to
configure: it is chosen per call, from the input alone. `verbora-phonetics`
needs no equivalent, because every encoder there reads one Unicode scalar at a
time on every path — there is no second representation to promote to, and no
input can be split in the middle of a character.

## When copying is unavoidable

Being honest about the other direction:

- **`Tokenizer::tokenize`** returns `Vec<String>` by construction — that is the
  point of the owned path, and it is the one to reach for when tokens must
  outlive the text they came from.
- **`Phonetic::process`** returns an owned `String` per call. Phonetic keys are
  computed, not sliced — there is nothing in the input to borrow.
- **`Padded::new`** materialises the padded sequence once, because the padding
  symbols are not in your slice and a window has to be contiguous.
- **`Trie::keys_with_prefix`** returns `Vec<String>`: trie keys are reconstructed
  by walking the tree, so there is no contiguous source to slice.
- **The Levenshtein family** allocates a working structure of its own on every
  call — a handful of bit-vector words in the common unit-cost cases
  (`levenshtein` and `osa` alike), three rolling rows for unit-cost
  `damerau_levenshtein`, and rows or full matrices in the weighted modes.

## What the fast path is worth

`verbora-distance` is where the effect is published, and it shows up as the gap
between the borrowed path and the promoted path on identical input lengths:

| Benchmark | Path | Time |
|---|---|--:|
| `levenshtein/ascii/16` | borrowed, no conversion | 41.8 ns † |
| `levenshtein/cyrillic/16` | promoted, one allocation per operand | 266.4 ns † |
| `levenshtein/ascii/256` | borrowed, no conversion | 2.13 µs † |
| `levenshtein/cyrillic/256` | promoted, one allocation per operand | 3.97 µs † |

† Measured against the kernel generation that preceded the current
Levenshtein implementation and its scalar-unit dispatch. The figures are left
as recorded rather than replaced with a guess, and are **pending
re-measurement**; the ratios drawn from them are not current either.

The promotion is a fixed cost paid once per operand, so its share shrinks as the
comparison grows. Exact scalar semantics are free on ASCII text, and the
promotion is bounded, one-off work on the text that needs it.

Where there is nothing to copy in the first place, there is nothing to save:
`hamming` on ASCII is a single scan that allocates nothing at all — 6.6 ns † at
four units, 275.3 ns † at 1024.

† Pending re-measurement, on the same terms as the table above.

## Related

- [Allocation behaviour](allocation.md) — the per-API reference.
- [Normalizers](../features/normalizers.md) — the `Cow` story in full.
- [Benchmarks: string distance](../benchmarks/distance.md) — the full results
  table, including the bit-vector Levenshtein kernels.
