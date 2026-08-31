# Choosing an API: tokenization

Verbora gives you several ways to move tokens from a tokenizer to you. They are
not alternative *implementations* — there is exactly one implementation of each
tokenizer's behaviour, and every convenience method is defined on top of it.
They differ only in who owns the memory.

This page is about picking one. For what each tokenizer *does*, see
[Tokenizers](../features/tokenizers.md). For the conventions this page is an
instance of, see [API shapes](./api-shapes.md).

## The two traits

There is one token shape — `&'a str`, a contiguous slice of your input — so
there is no associated token type and nothing to abstract over. Two traits,
seven methods, and only two of them carry any behaviour:

```rust  ignore
pub trait Tokenizer {
    fn tokenize(&self, text: &str) -> Vec<String>;                   // provided
    fn tokenize_into(&self, text: &str, out: &mut Vec<String>);      // required
    fn tokenize_batch<S: AsRef<str>>(&self, texts: &[S]) -> Vec<Vec<String>>;
}

pub trait BorrowingTokenizer: Tokenizer {
    // The primitive. Every token is a contiguous slice of `text`.
    fn tokens<'a>(&self, text: &'a str) -> impl Iterator<Item = &'a str>;

    fn tokenize_borrowed<'a>(&self, text: &'a str) -> Vec<&'a str> {
        self.tokens(text).collect()
    }

    // Appends; does **not** clear `out`.
    fn tokenize_borrowed_into<'a>(&self, text: &'a str, out: &mut Vec<&'a str>) {
        out.extend(self.tokens(text));
    }
}
```

All three tokenizers in the workspace implement both. There is no behaviour in
any method that is not in `tokens`, so choosing between them is choosing a
memory strategy, never a result:

| Method | Peak memory | First token visible | Allocations |
|---|---|---|---|
| `tokens()` | one token | immediately — `find`/`any` can stop the scan | none |
| `tokenize_borrowed()` | the whole token list | after the last token is produced | one `Vec` of `&str` |
| `tokenize_borrowed_into()` | the whole token list | after the last token is produced | none once your buffer is warm |
| `tokenize()` | the whole token list, owned | after the last token is produced | one `Vec` **plus one `String` per token** |

<div class="callout callout-warn">
<strong>Careful.</strong> <code>tokenize_borrowed_into</code> and
<code>tokenize_into</code> do <strong>not</strong> clear <code>out</code>; the
body of the first is <code>out.extend(self.tokens(text))</code>. The
<code>buf.clear()</code> at the top of a reuse loop is <em>your</em> line, and
leaving it out gives you a buffer holding every document at once.
</div>

## Every entry point, compared

| API | Best for | Lazy | Buffer reuse | Allocations | Token type |
|---|---|:--:|:--:|---|---|
| `BorrowingTokenizer::tokens` | pipelines, folds, early exit | ✅ | n/a | none | `&'a str` |
| `BorrowingTokenizer::tokenize_borrowed` | one document, simplest zero-copy call | ❌ | ❌ | one `Vec` | `&'a str` |
| `BorrowingTokenizer::tokenize_borrowed_into` | a corpus through one buffer | ❌ | ✅ | none once warm | `&'a str` |
| `Tokenizer::tokenize` | tokens must outlive the input | ❌ | ❌ | one `Vec` **plus one `String` per token** | `String` |
| `Tokenizer::tokenize_into` | owned tokens, warm `Vec` | ❌ | ✅ (the `Vec` only) | one `String` per token | `String` |
| `Tokenizer::tokenize_batch` | a slice of documents in one call | ❌ | ❌ | outer `Vec` + inner `Vec` + `String` per token | `Vec<Vec<String>>` |
| `par_tokenize_batch` | many independent documents, feature `parallel` | ❌ | ❌ | one `Vec` per document | `Vec<Vec<&'a str>>` |

<div class="callout callout-good">
<strong>Every tokenizer here is lazy, and every one is allocation-free in
<code>tokens()</code>.</strong> There is no exception to special-case: the
crate's three tokenizers are all single-pass boundary scanners over the input,
so <code>find</code>, <code>any</code> and <code>take</code> genuinely stop
early.
</div>

## Decision table

| Your situation | Call |
|---|---|
| I look at each token once, or stop early | `tokens()` |
| I want a `Vec` to index or iterate, and the input outlives it | `tokenize_borrowed()` |
| I am in a loop over many documents that all outlive the loop | `buf.clear(); tokenize_borrowed_into(doc, &mut buf)` |
| My tokens must outlive the text they came from | `Tokenizer::tokenize` |
| My documents are read and dropped one at a time, and I still want buffer reuse | `Tokenizer::tokenize_into` |
| I have a slice of documents and want one call | `Tokenizer::tokenize_batch` (a sequential map — it saves typing, not allocation) |
| I have many independent documents and the `parallel` feature | `par_tokenize_batch(&t, texts)` |

## One example per variant

### `tokens()` — the primitive

Use it when the tokens flow straight into something else — a counter, a filter,
a hash, a writer — and you never need the list itself.

```rust
use std::collections::HashMap;

use verbora_tokenizers::{BorrowingTokenizer, WordTokenizer};

fn main() {
    let t = WordTokenizer;
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

### `tokenize_borrowed()` — the simple one

Random access, `len()`, sorting and returning the tokens all need the `Vec`.
This is the right default: one allocation, and every token inside it points into
your original string.

```rust
use verbora_tokenizers::{BorrowingTokenizer, WordTokenizer};

fn first_and_last(doc: &str) -> Option<(&str, &str)> {
    let tokens = WordTokenizer.tokenize_borrowed(doc);
    Some((*tokens.first()?, *tokens.last()?))
}

fn main() {
    assert_eq!(first_and_last("the quick brown fox"), Some(("the", "fox")));
    assert_eq!(first_and_last("!!!"), None);
}
```

### `tokenize_borrowed_into()` — the hot loop

One buffer for a whole corpus. Its advantage is one thing only: no `Vec` per
document. That matters at corpus scale and is invisible at document scale.

```rust
use verbora_tokenizers::{BorrowingTokenizer, WordTokenizer};

fn main() {
    let t = WordTokenizer;
    let corpus = ["the quick brown fox", "jumps over", "the lazy dog"];

    // One heap buffer for the whole corpus. Reserve if you know the shape.
    let mut buf: Vec<&str> = Vec::with_capacity(64);
    let mut total = 0usize;
    for doc in corpus {
        buf.clear(); // required: tokenize_borrowed_into appends
        t.tokenize_borrowed_into(doc, &mut buf);
        total += buf.len();
    }
    assert_eq!(total, 9);
}
```

Dropping the `clear` concatenates the corpus into one token list, which is a
legitimate use and the reason the method does not clear on your behalf:

```rust
use verbora_tokenizers::{BorrowingTokenizer, WordTokenizer};

fn main() {
    let t = WordTokenizer;
    let mut all: Vec<&str> = Vec::new();
    for doc in ["a b", "c d"] {
        t.tokenize_borrowed_into(doc, &mut all);
    }
    assert_eq!(all, ["a", "b", "c", "d"]);
}
```

<div class="callout callout-note">
<strong>Note.</strong> A <code>Vec&lt;&amp;'a str&gt;</code> ties itself to the
lifetime of the text it borrows from, so reusing one buffer across documents
requires every document to outlive the buffer. When your documents are read and
dropped one at a time, use <code>Tokenizer::tokenize_into</code>, whose
<code>Vec&lt;String&gt;</code> owns its contents — you keep the buffer reuse and
pay one <code>String</code> per token.
</div>

### `Tokenizer` — owned tokens, and generic code

Reach for the owned path when the tokens must outlive the text, or when you are
writing one signature that works for anything implementing the trait:

```rust
use verbora_tokenizers::{SentenceTokenizer, Tokenizer, WordTokenizer};

fn longest_token<T: Tokenizer>(t: &T, text: &str) -> Option<String> {
    t.tokenize(text).into_iter().max_by_key(String::len)
}

fn main() {
    assert_eq!(
        longest_token(&WordTokenizer, "a bb ccc"),
        Some("ccc".to_string())
    );
    assert_eq!(
        longest_token(&SentenceTokenizer::new(), "Hi. Hello there."),
        Some("Hello there.".to_string())
    );
}
```

`Tokenizer` is **not object-safe** — `tokenize_batch` is generic — so
`Box<dyn Tokenizer>` does not compile. If you need runtime dispatch, wrap it in a
small `dyn`-compatible trait of your own with a blanket impl.

### `BorrowingTokenizer` — generic *and* zero-copy

The compromise between the two: still generic, still zero-copy. Every tokenizer
in the crate implements it, because every token is a substring by construction.

```rust
use verbora_tokenizers::{BorrowingTokenizer, SegmentTokenizer, WordTokenizer};

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
    assert_eq!(count_long(&WordTokenizer, &["a bb ccc", "dddd"], 3), 2);
    assert_eq!(count_long(&WordTokenizer, &["мир да"], 3), 1);
    // SegmentTokenizer sees the whitespace runs too, but none is 3 long here.
    assert_eq!(count_long(&SegmentTokenizer, &["a bb ccc"], 3), 1);
}
```

### `tokenize_batch` — a shorter call site, not a faster one

`tokenize_batch` is a provided method on `Tokenizer`. Its default body, in full,
is a sequential `map` calling `tokenize` once per document — and `tokenize`
allocates a fresh `Vec` and one `String` per token every time:

```rust  ignore
fn tokenize_batch<S: AsRef<str>>(&self, texts: &[S]) -> Vec<Vec<String>> {
    texts.iter().map(|t| self.tokenize(t.as_ref())).collect()
}
```

No tokenizer in this workspace overrides it. Use it when you want
`Vec<Vec<String>>` and a shorter call site; if you are reaching for it to make a
corpus faster, write the loop with `tokenize_borrowed_into` instead — that is the
API that actually reuses memory.

```rust
use verbora_tokenizers::{Tokenizer, WordTokenizer};

fn main() {
    let t = WordTokenizer;
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

- **Parallel tokenization is opt-in, not default.** `par_tokenize_batch` is a
  free function behind `verbora-tokenizers`'s `parallel` Cargo feature, and it is
  exactly `texts.par_iter().map(|t| tokenizer.tokenize_borrowed(t)).collect()`.
  Without that feature every tokenizer is single-threaded. Either way,
  tokenizers are zero-sized or immutable, stateless and `Send + Sync`, so
  parallelising across documents in your own code is straightforward. See
  [Parallelism](../performance/parallelism.md).
- **There is no streaming reader API.** Every entry point takes a `&str` that is
  fully in memory. There is no `tokenize_read(impl BufRead)`.
- **There is no buffer-reusing batch call.** "Many documents, one buffer" exists
  only as a loop you write around `tokenize_borrowed_into`.
- **There are no byte-offset accessors.** `SegmentTokenizer`'s concatenation
  guarantee makes offsets recoverable by running length when you need them.
- **`_into` never clears** anywhere in the tokenizers. (`Stemmer::stem_into` in
  `verbora_core` *does* clear its `String` — that difference is documented on
  [API shapes](./api-shapes.md).)

## Related

- [Tokenizers](../features/tokenizers.md) — what each of the three does.
- [API shapes](./api-shapes.md) — the workspace-wide conventions.
- [Iterator vs `_into`](../performance/iterator-vs-into.md) ·
  [Buffer reuse](../performance/buffer-reuse.md) ·
  [Zero-copy](../performance/zero-copy.md) ·
  [Allocation](../performance/allocation.md)
- [Batch vs streaming](../performance/batch-vs-streaming.md) ·
  [Parallelism](../performance/parallelism.md)
- [Performance](../performance/index.md) and [Benchmarks](../benchmarks/index.md)
