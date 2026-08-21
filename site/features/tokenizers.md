# Tokenizers

`verbora-tokenizers` cuts text at [UAX #29](https://www.unicode.org/reports/tr29/)
boundaries and returns the pieces, borrowed and in order. There are three
tokenizers and one token shape: every token is a contiguous `&'a str` slice of
the input it was given.

| Tokenizer | Yields |
|---|---|
| `WordTokenizer` | the word segments containing a letter or a digit |
| `SegmentTokenizer` | *every* word segment, so concatenation is the input |
| `SentenceTokenizer` | the sentences, untrimmed, with an optional abbreviation tailoring |

Each is built on one lazy iterator, `tokens()`, and every convenience method is
defined on top of it — so there is one implementation of each behaviour and no
second copy to drift.

<div class="callout callout-spec">
<strong>Specification status.</strong> Three tokenizers, one error type, one
version accessor and one parallel entry point — the crate root is the whole
public surface. <code>cargo test -p verbora-tokenizers --all-features</code>
runs <strong>21</strong> tests and <strong>7</strong> doctests, including a
conformance suite that replays the Unicode Character Database's own
<code>WordBreakTest.txt</code> and <code>SentenceBreakTest.txt</code>.
</div>

## When to use it

- You want word or sentence segmentation that follows a published standard
  rather than a hand-written character class.
- You want tokens that are substrings of your input, so offsets, highlighting
  and re-assembly stay possible.
- You want zero allocation per token, and a lazy iterator you can `map`,
  `filter` and short-circuit over.

## When not to use it

- **You want word segmentation for a language that does not use spaces.**
  UAX #29 §4 says outright that its default rules do not segment Thai, Lao,
  Khmer, Myanmar, Chinese or Japanese, and that a dictionary or statistical
  approach is required. `"日本語"` is three tokens, one per scalar. Verbora ships
  no dictionary segmenter.
- **You want the tokenizer to fold case, strip punctuation or remove accents.**
  Nothing here rewrites its input; that is [Normalizers](normalizers.md)' job,
  and every function there is named for the rewrite it performs.
- **You want Penn Treebank tokenization.** PTB directionalizes quotes, so its
  tokens are not substrings. It is not in this crate.
- **You want subword or BPE tokenization for a neural model.** Nothing here
  does that, and nothing is planned.

## Quick example

```rust
use verbora_tokenizers::{BorrowingTokenizer, WordTokenizer};

fn main() {
    assert_eq!(
        WordTokenizer.tokenize_borrowed("the quick brown fox"),
        ["the", "quick", "brown", "fox"]
    );
}
```

All three tokenizers are plain values. `WordTokenizer` and `SegmentTokenizer`
are zero-sized unit structs — write the name, there is no constructor to call.
`SentenceTokenizer` owns one `Vec<String>` of abbreviations, empty unless you
supply some.

## Choosing the right API

`BorrowingTokenizer::tokens` is the primitive; everything else is defined on it.

| Call | Use when | Allocates |
|---|---|---|
| `tokens(text)` | streaming, composing with `map`/`filter`, early exit | nothing |
| `tokenize_borrowed(text)` | you want a `Vec` and the input outlives it | one `Vec` of `&str` |
| `tokenize_borrowed_into(text, &mut buf)` | one buffer reused across a corpus | nothing once warm — **`buf` is not cleared** |
| `Tokenizer::tokenize(text)` | tokens must outlive the input | one `String` per token |
| `Tokenizer::tokenize_batch(texts)` | a slice of documents, one call | one outer `Vec`, one inner `Vec` and one `String` per token |
| `par_tokenize_batch(&t, texts)` | many independent documents, feature `parallel` | one `Vec` per document |

```rust
use verbora_tokenizers::{BorrowingTokenizer, Tokenizer, WordTokenizer};

fn main() {
    let t = WordTokenizer;

    // Lazy and zero-copy: stops as soon as it finds a match, and never builds
    // a `Vec` at all.
    assert!(t.tokens("the quick brown fox").any(|w| w == "quick"));

    // Collected, still borrowed from the input.
    assert_eq!(t.tokenize_borrowed("the quick"), ["the", "quick"]);

    // Owned, when the tokens must outlive the input.
    let owned: Vec<String> = t.tokenize("one two");
    assert_eq!(owned, ["one", "two"]);

    let batch: Vec<Vec<String>> = t.tokenize_batch(&["one two", "three four"]);
    assert_eq!(batch[1], ["three", "four"]);
}
```

### `tokenize_borrowed_into()` — the hot loop

<div class="callout callout-warn">
<strong>Careful.</strong> <code>tokenize_borrowed_into</code> does
<strong>not</strong> clear <code>out</code>. Its body is
<code>out.extend(self.tokens(text))</code>. Forgetting <code>buf.clear()</code>
in a loop produces a buffer that accumulates every document — which is a real
use case, but rarely the one you meant.
</div>

```rust
use verbora_tokenizers::{BorrowingTokenizer, WordTokenizer};

fn main() {
    let t = WordTokenizer;
    let corpus = ["the quick brown fox", "jumps over the lazy dog"];

    let mut buf: Vec<&str> = Vec::new();
    for doc in corpus {
        buf.clear(); // appends without this, the buffer grows across documents
        t.tokenize_borrowed_into(doc, &mut buf);
        assert!(!buf.is_empty());
    }
    assert_eq!(buf, ["jumps", "over", "the", "lazy", "dog"]);
}
```

One lifetime constraint follows from zero-copy: `Vec<&'a str>` ties the buffer
to `'a`, so a buffer holding borrowed tokens can only be reused across documents
that all outlive the loop. If your documents come from a `String` dropped each
iteration, move the `Vec` inside the loop or switch to
`Tokenizer::tokenize_into`, whose `Vec<String>` owns its contents.

## `WordTokenizer`

Yields the UAX #29 word segments that contain at least one scalar with the
`Alphabetic` property, or one whose `General_Category` is `Nd`, `Nl` or `No`.
Whitespace runs, punctuation runs and symbol runs are dropped.

### What UAX #29 does that a character-class scan does not

The rules keep a word together across interior punctuation where the standard
says a word is one thing, and break it where the standard says it is two.

| Input | Tokens | Rule |
|---|---|---|
| `"well-known"` | `["well", "known"]` | `U+002D` is `Word_Break=Other`; WB999 breaks |
| `"and/or"` | `["and", "or"]` | `U+002F` is `Word_Break=Other` |
| `"don't"` | `["don't"]` | WB6/WB7 over `MidNumLetQ` |
| `"3.14"` | `["3.14"]` | WB11/WB12, `Numeric × MidNumLet × Numeric` |
| `"1,000"` | `["1,000"]` | WB11/WB12, `MidNum` |
| `"node_js"` | `["node_js"]` | WB13a/WB13b, `ExtendNumLet` |
| `"a:b"` | `["a:b"]` | WB6/WB7, `MidLetter` |
| `"café naïve"` | `["café", "naïve"]` | `é`, `ï` are `ALetter` |
| `"привет, мир"` | `["привет", "мир"]` | Cyrillic is `ALetter` |
| `"日本語"` | `["日", "本", "語"]` | Han is `Other`; WB999 breaks between each |

```rust
use verbora_tokenizers::{BorrowingTokenizer, WordTokenizer};

fn main() {
    let t = WordTokenizer;

    assert_eq!(
        t.tokenize_borrowed("The quick (\"brown\") fox can't jump 32.3 feet, right?"),
        ["The", "quick", "brown", "fox", "can't", "jump", "32.3", "feet", "right"]
    );

    // Nothing is folded, stripped or substituted: tokens are substrings.
    assert_eq!(t.tokenize_borrowed("İstanbul"), ["İstanbul"]);
    assert_eq!(t.tokenize_borrowed("Äpfel Öl weiß"), ["Äpfel", "Öl", "weiß"]);
    assert_eq!(t.tokenize_borrowed("a×b÷c"), ["a", "b", "c"]);

    // An astral scalar is a scalar: it is not a word, and it is not replaced.
    assert_eq!(t.tokenize_borrowed("a😀b"), ["a", "b"]);

    // Empty and whitespace-only input yield no tokens at all.
    assert!(t.tokenize_borrowed("").is_empty());
    assert!(t.tokenize_borrowed("   ").is_empty());
}
```

## `SegmentTokenizer`

Yields *every* segment the UAX #29 word boundary rules delimit — whitespace and
punctuation runs included — so concatenating the tokens reproduces the input
byte for byte. That is the guarantee a highlighter, a re-assembler or an offset
consumer needs, and it is why both tokenizers exist: the word filter is
irreversible.

```rust
use verbora_tokenizers::{BorrowingTokenizer, SegmentTokenizer};

fn main() {
    let t = SegmentTokenizer;

    assert_eq!(
        t.tokenize_borrowed("The quick (\"brown\") fox"),
        ["The", " ", "quick", " ", "(", "\"", "brown", "\"", ")", " ", "fox"]
    );

    // Concatenation is the input, byte for byte.
    let text = "a\r\nb — c\u{0301}!";
    assert_eq!(t.tokens(text).collect::<String>(), text);
}
```

`WordTokenizer`'s output is a subsequence of `SegmentTokenizer`'s on the same
input, with equal pointer identity for corresponding tokens. Neither ever yields
the empty string.

## `SentenceTokenizer`

Sentences are the segments produced by the UAX #29 §5 boundary rules, in order,
**with no trimming**: a sentence includes its own trailing whitespace, so
concatenation reproduces the input and `tokens("   ")` is one token. Callers who
want trimmed sentences write `.map(str::trim)` — trimming here would produce
tokens that are not substrings of the input.

```rust
use verbora_tokenizers::{BorrowingTokenizer, SentenceTokenizer};

fn main() {
    let t = SentenceTokenizer::new();

    assert_eq!(
        t.tokenize_borrowed("Dr. Smith arrived. He left."),
        ["Dr. ", "Smith arrived. ", "He left."]
    );

    // Whitespace-only input is one sentence, not an empty token.
    assert_eq!(t.tokenize_borrowed("   "), ["   "]);
    assert!(t.tokenize_borrowed("").is_empty());

    // Trailing whitespace belongs to the sentence; trim at the call site.
    let trimmed: Vec<&str> = t.tokens("One. Two.").map(str::trim).collect();
    assert_eq!(trimmed, ["One.", "Two."]);
}
```

### Abbreviations are a tailoring, and the standard says one is needed

UAX #29 §5 breaks after any sentence terminator, so `"Dr. Smith"` is two
sentences under the default rules, and the annex notes that abbreviation
handling requires tailoring. Verbora's tailoring is stated exactly:

> Let `B` be the set of boundary positions the default rules produce over
> `text`. A position `b` with `0 < b < text.len()` is **suppressed** if some
> abbreviation `a` in the set satisfies
> `text[..b].trim_end_matches(char::is_whitespace).ends_with(a)`.
> Suppressed boundaries are not emitted; the segments on either side are
> joined. The final boundary at `text.len()` is never suppressed.

```rust
use verbora_tokenizers::{AbbreviationError, BorrowingTokenizer, SentenceTokenizer};

fn main() {
    let t = SentenceTokenizer::with_abbreviations(["Dr."]).unwrap();
    assert_eq!(
        t.tokenize_borrowed("Dr. Smith arrived. He left."),
        ["Dr. Smith arrived. ", "He left."]
    );
    assert_eq!(t.abbreviations(), ["Dr."]);

    // Suffix matching over-suppresses, and you choose the set:
    // "casino." ends with "no.", so this is one sentence.
    let over = SentenceTokenizer::with_abbreviations(["no."]).unwrap();
    assert_eq!(
        over.tokenize_borrowed("Visit the casino. Then leave."),
        ["Visit the casino. Then leave."]
    );

    // An empty abbreviation would suppress every interior boundary, so the
    // constructor refuses it rather than documenting the hazard.
    assert_eq!(
        SentenceTokenizer::with_abbreviations(["Dr.", ""]),
        Err(AbbreviationError::Empty { index: 1 })
    );
}
```

Four consequences, each deliberate:

- **Matching is case-sensitive**, and is an exact scalar-sequence comparison.
  Case-insensitive matching needs a case-folding decision that belongs to you;
  supply both casings if you want both.
- **Whitespace is Unicode `White_Space`** (`char::is_whitespace`). This is the
  one place the crate consults a whitespace set at all.
- **Suppression is suffix matching, so it can over-suppress**, as above.
  Qualifying the match by a word boundary would fix `"casino."` and break
  `"e.g."`, `"i.e."` and `"Ph.D."`, whose interior periods are themselves
  boundaries.
- **The last sentence is never lost.** An abbreviation at end of input
  suppresses nothing, because the boundary at `text.len()` is exempt.

## Composing with the rest of the workspace

Because tokens are `&str`, the whole workspace consumes them without an adapter:

```rust
use verbora_ngrams::ngrams;
use verbora_normalizers::remove_diacritics;
use verbora_tokenizers::{BorrowingTokenizer, WordTokenizer};
use std::num::NonZeroUsize;

fn main() {
    let text = "Café au lait, s'il vous plaît";

    // Fold accents per token, only where the fold actually fires.
    let keys: Vec<String> = WordTokenizer
        .tokens(text)
        .map(|t| remove_diacritics(&t.to_lowercase()).into_owned())
        .collect();
    assert_eq!(keys[0], "cafe");

    // Word n-grams are the composition, written out so the tokenizer is an
    // argument rather than a hidden policy.
    let tokens = WordTokenizer.tokenize_borrowed(text);
    let n = NonZeroUsize::new(2).expect("2 is not zero");
    assert_eq!(ngrams(&tokens, n).next(), Some(&["Café", "au"][..]));
}
```

## Parallelism

`par_tokenize_batch` is the crate's one parallel entry point, behind the
`parallel` Cargo feature (`parallel = ["dep:rayon"]`, never on by default). It is
a free function rather than a trait method so that `verbora-core` acquires
neither a feature nor a `rayon` dependency for one method body. Its whole body is
`texts.par_iter().map(|t| tokenizer.tokenize_borrowed(t)).collect()` — one task
per **document**, never per token, and output order matches input order.

```rust  ignore
// Needs the `parallel` feature, which this site's snippet checker builds
// without — so this block is marked `ignore` rather than compiled. Every
// other block on this page compiles and runs in CI.
use verbora_tokenizers::{WordTokenizer, par_tokenize_batch};

let docs = ["the quick brown fox", "jumps over the lazy dog"];
let batches: Vec<Vec<&str>> = par_tokenize_batch(&WordTokenizer, &docs);
assert_eq!(batches[0], ["the", "quick", "brown", "fox"]);
```

**When it pays.** The crossover — the batch size and document length at which
fan-out beats a sequential loop — is unmeasured for this implementation. Rayon
costs on the order of a microsecond to schedule a task, so for a handful of
short strings `texts.iter().map(…)` is the better default. Every tokenizer here
is stateless and `Send + Sync`, so you can also roll your own fan-out with
`rayon` in your own `Cargo.toml` and no Cargo feature at all. See
[Parallelism](../performance/parallelism.md).

## The Unicode version is part of the contract

Word and sentence boundaries are defined over the `Word_Break` and
`Sentence_Break` properties of the Unicode Character Database, so this crate
cannot promise results frozen for all time the way
[string distance](distance.md) can — a boundary rule frozen today is simply
wrong for every character encoded after the freeze.

- The Unicode version is whichever version the segmentation dependency ships,
  pinned in `Cargo.lock`. At the version this crate is built against that is
  **Unicode 17.0.0**, and `unicode_version()` reports it at run time.
- A UCD upgrade is a **semver-visible behaviour change** for this crate and is
  released as one.
- **Any structure that persists tokenizer-derived keys must stamp the Unicode
  version and refuse to load across a change.** An index, a model or an interned
  term table built before an upgrade does not match one built after it, and
  nothing else will tell you.

```rust
fn main() {
    // The version is a fact about the build, not a constant to hardcode —
    // record it, compare it, refuse to load an artifact whose stamp differs.
    let (major, _minor, _update) = verbora_tokenizers::unicode_version();
    assert!(major >= 17);
}
```

Within one Unicode version the crate is fully deterministic: the same input
produces the same output on every platform and every build. There is no global
mutable state, no hash-order dependence and no interior mutability.

## Performance and allocation

Every tokenizer is **O(n) in the input length**, a single pass over the input
with no backtracking.

- **Construction allocates nothing** for `WordTokenizer` and `SegmentTokenizer`
  (both zero-sized) and nothing for `SentenceTokenizer::new`.
  `with_abbreviations` holds one `Vec<String>`.
- **`tokens()` allocates nothing**, for all three.
- **`tokenize_borrowed()`** adds one `Vec`; **`tokenize_borrowed_into()`** adds
  nothing once the buffer has capacity; **`Tokenizer::tokenize`** adds one
  `String` per token; **`tokenize_batch`** adds one outer `Vec` on top of that
  per document.

No tokenizer benchmark results are published: no benchmark has been run
against the current implementation, and no figure is estimated in place of one.
The allocation behaviour above is a property of the implementation, not a timing
claim. See [Benchmarks](../benchmarks/index.md) for what *has* been measured.

## Common mistakes

**Forgetting `buf.clear()`.** `tokenize_borrowed_into` and `tokenize_into` both
append. (In `verbora_core`, `Stemmer::stem_into` is the one exception — it clears
first.)

**Expecting `SentenceTokenizer` to trim.** It does not, on purpose: a trimmed
sentence would not be a substring of the input. `.map(str::trim)` at the call
site.

**Expecting `"日本語"` to be one token.** It is three. UAX #29 does not segment
languages that do not use spaces, and this crate says so rather than shipping an
unattributable model.

**Reaching for `tokenize_batch` for speed.** It is a sequential `map` over
`tokenize`, allocating a fresh `Vec` and one `String` per token per document. It
is a convenience, not an optimisation — `par_tokenize_batch` is the parallel one.

**Persisting tokens or token-derived keys without a Unicode stamp.** Boundaries
move between Unicode versions; an index built under one and queried under
another mismatches silently.

## Related

- [Choosing an API: tokenization](../choosing/tokenization.md) — the long-form
  decision, with pipeline diagrams
- [API shapes](../choosing/api-shapes.md) — the workspace-wide convention that
  `_into` appends and `tokens()` is the primitive
- [Core traits](core.md) — `verbora_core::Tokenizer`, `BorrowingTokenizer`
- [N-grams](ngrams.md) — consumes a tokenizer ·
  [Normalizers](normalizers.md) — the crate whose job *is* rewriting
- [Zero-copy](../performance/zero-copy.md) ·
  [Buffer reuse](../performance/buffer-reuse.md) ·
  [Allocation](../performance/allocation.md) ·
  [Parallelism](../performance/parallelism.md)
- [Benchmarks](../benchmarks/index.md) · [Recipes](../recipes/index.md)

## API reference

```bash
cargo doc -p verbora-tokenizers --no-deps --open
```

| Item | Path |
|---|---|
| `WordTokenizer`, `SegmentTokenizer` | `verbora_tokenizers` |
| `SentenceTokenizer`, `AbbreviationError` | `verbora_tokenizers` |
| `unicode_version` | `verbora_tokenizers::unicode_version` |
| `par_tokenize_batch` (feature `parallel`) | `verbora_tokenizers::par_tokenize_batch` |
| `Tokenizer`, `BorrowingTokenizer` | `verbora_core`, re-exported from the crate root |

Source: `crates/verbora-tokenizers/src/`.
