# Choosing an API: tokenization

Verbora gives you several ways to split one string into tokens. They are not
alternative *implementations* — there is exactly one implementation of each
tokenizer's behaviour, and every convenience method is defined on top of it.
They are alternative ways of **moving the tokens from the tokenizer to you**,
and they differ in who owns the memory.

This page is about picking one. For what each tokenizer *does*, see
[Tokenizers](../features/tokenizers.md). For the conventions this page is an
instance of, see [API shapes](./api-shapes.md).

## The three core shapes

Here is the entire trait. Two of the three methods are one line each:

```rust  ignore
pub trait Tokenize {
    type Token<'a>;

    fn tokens<'a>(&self, text: &'a str) -> impl Iterator<Item = Self::Token<'a>>;

    fn tokenize<'a>(&self, text: &'a str) -> Vec<Self::Token<'a>> {
        self.tokens(text).collect()
    }

    fn tokenize_into<'a>(&self, text: &'a str, out: &mut Vec<Self::Token<'a>>) {
        out.extend(self.tokens(text));
    }
}
```

There is no behaviour in `tokenize` or `tokenize_into` that is not in `tokens`.
Choosing between them is choosing a memory strategy, never a result:

| | Peak memory | First token visible | Allocations |
|---|---|---|---|
| `tokens()` | one token | immediately — `find`/`any` can stop the scan | none |
| `tokenize()` | the whole token list | after the last token is produced | one `Vec`, grown by doubling |
| `tokenize_into()` | the whole token list | after the last token is produced | none once your buffer is warm |

<div class="callout callout-warn">
<strong>Careful.</strong> <code>tokenize_into</code> does <strong>not</strong>
clear <code>out</code>; its body is
<code>out.extend(self.tokens(text))</code>. The <code>buf.clear()</code> at the
top of a reuse loop is <em>your</em> line, and leaving it out gives you a buffer
holding every document at once.
</div>

## Every entry point, compared

| API | Best for | Lazy | Buffer reuse | Allocations | Token type |
|---|---|:--:|:--:|---|---|
| `Tokenize::tokens` | pipelines, folds, early exit | ✅ | n/a | none, for the 13 slicing tokenizers | `Self::Token<'a>` |
| `Tokenize::tokenize` | one document, simplest call | ❌ | ❌ | one `Vec`, grown by doubling | `Self::Token<'a>` |
| `Tokenize::tokenize_into` | a corpus through one buffer | ❌ | ✅ | none once the buffer is warm | `Self::Token<'a>` |
| `verbora_core::Tokenizer::tokenize` | generic code; owned tokens | ❌ | ❌ | one `Vec` **plus one `String` per token** | `String` |
| `verbora_core::Tokenizer::tokenize_into` | generic code, warm `Vec` | ❌ | ✅ (the `Vec` only) | one `String` per token | `String` |
| `verbora_core::Tokenizer::tokenize_batch` | a slice of documents in one call | ❌ | ❌ | outer `Vec` + inner `Vec` + `String` per token | `Vec<Vec<String>>` |
| `verbora_core::BorrowingTokenizer::tokenize_borrowed` | generic code, zero-copy | ❌ | ❌ | one `Vec` | `&'a str` |
| `verbora_core::BorrowingTokenizer::tokenize_borrowed_into` | generic code, zero-copy, warm buffer | ❌ | ✅ | none once warm | `&'a str` |
| inherent methods on the four regex tokenizers | those four tokenizers | mixed | ✅ (`_into`) | mixed | wrapped in `Option` |

<div class="callout callout-note">
<strong>Five tokenizers are not lazy.</strong> <code>TreebankWordTokenizer</code>,
<code>TokenizerJa</code> and <code>SentenceTokenizer</code> build the whole list
before <code>tokens()</code> yields its first item, because their algorithms are
inherently whole-text; <code>AggressiveTokenizerNo</code> and
<code>AggressiveTokenizerSv</code> normalize the text first and then scan it
lazily; <code>CaseTokenizer</code> is lazy on ASCII only. Among the regex
tokenizers, <code>RegexpTokenizer::tokens</code> also collects, because it has to
know whether the scan found no match at all before it can hand you anything. The
signature stays uniform so generic code need not special-case them, but
<code>tokens()</code> will not save you an allocation there.
</div>

## Decision table

| Your situation | Call |
|---|---|
| I name a concrete tokenizer type, and look at each token once | `tokens()` |
| I name a concrete type and want a `Vec` to keep, index or return | `tokenize()` |
| I name a concrete type and am in a loop over many documents | `buf.clear(); tokenize_into(doc, &mut buf)` |
| My function takes "some tokenizer" and needs owned `String`s | `verbora_core::Tokenizer` |
| My function takes "some tokenizer" and only needs slices | `verbora_core::BorrowingTokenizer` (13 of 24 types) |
| My tokenizer is `RegexpTokenizer` / `WordTokenizer` / `OrthographyTokenizer` / `WordPunctTokenizer` | the inherent methods — they return `Option`, `None` meaning "no match at all" |
| I have a slice of documents and want one call | `verbora_core::Tokenizer::tokenize_batch` (a sequential map — it saves typing, not allocation) |

## One example per variant

### `tokens()` — the primitive

Use it when the tokens flow straight into something else — a counter, a filter,
a hash, a writer — and you never need the list itself.

```rust
use std::collections::HashMap;

use verbora_tokenizers::{AggressiveTokenizer, Tokenize};

fn main() {
    let t = AggressiveTokenizer::new();
    let doc = "the cat sat on the mat";

    // A word-frequency map with zero token allocations: the keys borrow `doc`.
    let mut freq: HashMap<&str, usize> = HashMap::new();
    for token in t.tokens(doc) {
        *freq.entry(token).or_default() += 1;
    }
    assert_eq!(freq["the"], 2);

    // Early exit: the scan stops at "cat" and never reaches "mat".
    assert!(t.tokens(doc).any(|w| w == "cat"));
}
```

### `tokenize()` — the simple one

Random access, `len()`, sorting and returning the tokens all need the `Vec`.
This is the right default: one allocation, and for the thirteen slicing
tokenizers it allocates *only* the `Vec` — every token inside it points into
your original string.

```rust
use verbora_tokenizers::{AggressiveTokenizer, Tokenize};

fn first_and_last(doc: &str) -> Option<(&str, &str)> {
    let tokens = AggressiveTokenizer::new().tokenize(doc);
    Some((*tokens.first()?, *tokens.last()?))
}

fn main() {
    assert_eq!(first_and_last("the quick brown fox"), Some(("the", "fox")));
    assert_eq!(first_and_last("!!!"), None);
}
```

### `tokenize_into()` — the hot loop

One buffer for a whole corpus. Its advantage is one thing only: no `Vec` per
document. That matters at corpus scale and is invisible at document scale.

```rust
use verbora_tokenizers::{AggressiveTokenizer, Tokenize};

fn main() {
    let t = AggressiveTokenizer::new();
    let corpus = ["the quick brown fox", "jumps over", "the lazy dog"];

    // One heap buffer for the whole corpus. Reserve if you know the shape.
    let mut buf: Vec<&str> = Vec::with_capacity(64);
    let mut total = 0usize;
    for doc in corpus {
        buf.clear(); // required: tokenize_into appends
        t.tokenize_into(doc, &mut buf);
        total += buf.len();
    }
    assert_eq!(total, 9);
}
```

Dropping the `clear` concatenates the corpus into one token list, which is a
legitimate use and the reason the method does not clear on your behalf:

```rust
use verbora_tokenizers::{AggressiveTokenizer, Tokenize};

fn main() {
    let t = AggressiveTokenizer::new();
    let mut all: Vec<&str> = Vec::new();
    for doc in ["a b", "c d"] {
        t.tokenize_into(doc, &mut all);
    }
    assert_eq!(all, ["a", "b", "c", "d"]);
}
```

<div class="callout callout-note">
<strong>Note.</strong> The buffer's element type is
<code>Self::Token&lt;'a&gt;</code>, so a <code>Vec&lt;&amp;'a str&gt;</code> ties
itself to the lifetime of the text it borrows from. Reusing one buffer across
documents requires every document to outlive the buffer. When your documents are
read and dropped one at a time, use
<code>verbora_core::Tokenizer::tokenize_into</code>, whose
<code>Vec&lt;String&gt;</code> owns its contents — you keep the buffer reuse and
pay one <code>String</code> per token.
</div>

### `verbora_core::Tokenizer` — generic, owned

`Tokenize::Token<'a>` is a generic associated type, which is what lets a slicing
tokenizer say "my token is a slice of your input" — but it also means a function
generic over `T: Tokenize` cannot do much with the tokens without a `for<'a>`
bound that not every token type satisfies (`Utf16Token` does not implement
`AsRef<str>`). When you need one signature that works for all of them, use
`verbora_core::Tokenizer` and accept the `String`s:

```rust
use verbora_core::Tokenizer;
use verbora_tokenizers::{AggressiveTokenizer, SentenceTokenizer, TokenizerJa};

fn longest_token<T: Tokenizer>(t: &T, text: &str) -> Option<String> {
    t.tokenize(text).into_iter().max_by_key(String::len)
}

fn main() {
    assert_eq!(
        longest_token(&AggressiveTokenizer::new(), "a bb ccc"),
        Some("ccc".to_string())
    );
    assert_eq!(
        longest_token(&TokenizerJa::new(), "日本語"),
        Some("日本語".to_string())
    );
    assert_eq!(
        longest_token(&SentenceTokenizer::new(), "Hi. Hello there."),
        Some("Hello there.".to_string())
    );
}
```

Twenty of the twenty-four tokenizer types implement it. `Tokenizer` is **not
object-safe** — `tokenize_batch` is generic — so `Box<dyn Tokenizer>` does not
compile. If you need runtime dispatch, wrap it in a small `dyn`-compatible trait
of your own with a blanket impl.

### `verbora_core::BorrowingTokenizer` — generic, zero-copy

The compromise between the two: still generic, still zero-copy, but only the
thirteen tokenizers whose tokens are always contiguous substrings implement it
(the twelve character-class variants and `AggressiveTokenizerFa`).

```rust
use verbora_core::BorrowingTokenizer;
use verbora_tokenizers::{AggressiveTokenizer, AggressiveTokenizerRu};

fn count_long<T: BorrowingTokenizer>(t: &T, docs: &[&str], min: usize) -> usize {
    let mut buf: Vec<&str> = Vec::new();
    let mut n = 0;
    for doc in docs {
        buf.clear();
        t.tokenize_borrowed_into(doc, &mut buf);
        // `str::len()` is bytes. Counting characters keeps the threshold
        // meaningful for non-Latin scripts — "мир" is 3 characters and 6 bytes.
        n += buf.iter().filter(|w| w.chars().count() >= min).count();
    }
    n
}

fn main() {
    assert_eq!(count_long(&AggressiveTokenizer::new(), &["a bb ccc", "dddd"], 3), 2);
    assert_eq!(count_long(&AggressiveTokenizerRu::new(), &["мир да"], 3), 1);
}
```

### The four `Option`-returning tokenizers

`RegexpTokenizer`, `WordTokenizer`, `OrthographyTokenizer` and
`WordPunctTokenizer` implement neither trait. In matching mode, "no match at
all" is a distinct outcome from "matched, but produced zero tokens," and no
trait in this workspace can express that distinction. They keep the same three
method names as inherent methods, wrapped in `Option`; their `tokenize_into`
returns `bool` instead, `false` meaning "no match at all," in which case nothing
was appended.

```rust
use verbora_tokenizers::WordTokenizer;

fn main() {
    // Splitting mode (the default) always succeeds.
    let split = WordTokenizer::new();
    assert_eq!(split.tokenize("hello, world"), Some(vec!["hello", "world"]));
    assert_eq!(split.tokenize(""), Some(vec![]));

    // Matching mode can return `None` when nothing matches.
    let m = WordTokenizer::matching();
    assert_eq!(m.tokenize("abc def"), Some(vec![" "]));
    assert_eq!(m.tokenize("abcdef"), None);

    // `tokenize_into` reports the same distinction as a bool.
    let mut buf: Vec<&str> = Vec::new();
    assert!(!m.tokenize_into("abcdef", &mut buf));
    assert!(buf.is_empty());
}
```

If your application does not care about the distinction, collapse it *visibly*
with `.unwrap_or_default()` rather than letting it disappear into a `?`.

### `tokenize_batch` — a shorter call site, not a faster one

`tokenize_batch` is a provided method on `verbora_core::Tokenizer`. Its default
body, in full, is a sequential `map` calling `tokenize` once per document — and
`tokenize` allocates a fresh `Vec` every time:

```rust  ignore
fn tokenize_batch<S: AsRef<str>>(&self, texts: &[S]) -> Vec<Vec<String>> {
    texts.iter().map(|t| self.tokenize(t.as_ref())).collect()
}
```

No tokenizer in this workspace overrides it. Use it when you want
`Vec<Vec<String>>` and a shorter call site; if you are reaching for it to make a
corpus faster, write the loop with `tokenize_into` instead — that is the API
that actually reuses memory.

```rust
use verbora_core::Tokenizer;
use verbora_tokenizers::AggressiveTokenizer;

fn main() {
    let t = AggressiveTokenizer::new();
    let docs = ["one two", "three four"];

    // Convenient.
    let batch: Vec<Vec<String>> = t.tokenize_batch(&docs);
    assert_eq!(batch[1], ["three", "four"]);

    // Equivalent, and allocates exactly as much.
    let manual: Vec<Vec<String>> = docs.iter().map(|d| t.tokenize(d)).collect();
    assert_eq!(batch, manual);
}
```

## What does not exist

Stated plainly, so you do not go looking:

- **Parallel tokenization is opt-in, not default.** `Tokenize::par_tokenize_batch`
  is a provided method behind `verbora-tokenizers`'s `parallel` Cargo feature,
  and it is exactly `texts.par_iter().map(|t| self.tokenize(t)).collect()`.
  Without that feature every tokenizer is single-threaded. Either way,
  tokenizers are zero-sized (or immutable), stateless and `Send + Sync`, so
  parallelising across documents in your own code is straightforward. See
  [Parallelism](../performance/parallelism.md).
- **There is no streaming reader API.** Every entry point takes a `&str` that is
  fully in memory. There is no `tokenize_read(impl BufRead)`.
- **There is no buffer-reusing batch call.** "Many documents, one buffer" exists
  only as a loop you write around `tokenize_into`. None of the four
  `Option`-returning tokenizers has a batch method at all.
- **`_into` never clears** anywhere in the tokenizers. (`Stemmer::stem_into` in
  `verbora_core` *does* clear its `String` — that difference is documented on
  [API shapes](./api-shapes.md).)

## Related

- [Tokenizers](../features/tokenizers.md) — the catalogue of all twenty-five.
- [API shapes](./api-shapes.md) — the workspace-wide conventions.
- [Iterator vs `_into`](../performance/iterator-vs-into.md) ·
  [Buffer reuse](../performance/buffer-reuse.md) ·
  [Zero-copy](../performance/zero-copy.md) ·
  [Allocation](../performance/allocation.md)
- [Batch vs streaming](../performance/batch-vs-streaming.md) ·
  [Parallelism](../performance/parallelism.md)
- [Performance](../performance/index.md) and [Benchmarks](../benchmarks/index.md)
