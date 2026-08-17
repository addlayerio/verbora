# Choosing an API: tokenization

Verbora gives you several ways to split one string into tokens. They are not
alternative *implementations* — there is exactly one implementation of each
tokenizer's behaviour, and every convenience method is defined on top of it.
They are alternative ways of **moving the tokens from the tokenizer to you**,
and they differ in who owns the memory.

This page is about picking one. For what each tokenizer *does*, see
[Tokenizers](../features/tokenizers.md). For the general Verbora conventions
this page is an instance of, see [API shapes](./api-shapes.md) and
[Choosing the right API](./index.md).

## Why there is more than one API

A tokenizer produces a sequence. A sequence can be handed over in three ways,
and each one is right for a different caller:

1. **As an iterator.** The caller pulls tokens one at a time. Nothing is stored,
   so nothing is allocated, and the caller can stop early. This is
   `Tokenize::tokens`.
2. **As a fresh `Vec`.** The caller gets a container it owns and can keep,
   index, sort and pass around. One allocation, no ceremony. This is
   `Tokenize::tokenize`.
3. **Appended to a container the caller already has.** The caller supplies the
   memory, so a loop over a million documents allocates once instead of a
   million times. This is `Tokenize::tokenize_into`.

Verbora exposes all three because collapsing them loses something real. If only
(2) existed, a streaming pipeline would allocate a `Vec` per document for no
reason. If only (1) existed, the ninety-percent case — "give me the words in
this string" — would require the reader to know what `collect` is before they
could use the library at all.

Here is the entire trait, copied from `crates/verbora-tokenizers/src/lib.rs`
with its doc comments removed. (2) and (3) are one line each:

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
Choosing between them is choosing a memory strategy, never a result.

On top of those three sit two *traits from a different crate* —
`verbora_core::Tokenizer` and `verbora_core::BorrowingTokenizer` — which exist so
that code generic over "any tokenizer" can be written at all. They are the
fourth and fifth options, and they trade some efficiency for a signature that
does not mention `Self::Token<'a>`.

## The shape of the pipeline

The difference between `tokenize()` and `tokens()` is *when* work happens, and
how much of the document is alive at once.

`tokenize()` — **materialise, then consume**. The whole document is scanned, all
tokens are pushed into a `Vec`, and only then does your code see the first one:

```text
  input: "the quick brown fox"
     │
     ▼
  ┌───────────────────────────────────────────────┐
  │  scan the entire document                     │
  │  ───────────────────────────────────────────  │
  │  "the" ─┐                                     │
  │  "quick"├──► push ──► Vec (grows: 4→8→16…)    │
  │  "brown"│                                     │
  │  "fox"  ┘                                     │
  └───────────────────────────────────────────────┘
     │
     ▼  Vec<&str>  (one allocation; tokens still borrow the input)
     │
     ▼
  your code: for t in &tokens { … }
```

Peak memory: one `Vec` sized to the whole document's token count. Earliest
observation of token #1: after token #N has been produced.

`tokens()` — **token → consumer → token → consumer**. Each token is produced
and consumed before the next is produced. Nothing accumulates:

```text
  input: "the quick brown fox"
     │
     ▼
  ┌─────────┐  "the"   ┌───────────┐
  │ scanner │ ───────► │ your code │
  │         │ ◄─────── │  (next()) │
  │         │  "quick" │           │
  │         │ ───────► │           │
  │         │ ◄─────── │           │
  │         │  "brown" │           │
  │         │ ───────► │           │
  │         │ ◄─────── │           │
  │         │  "fox"   │           │
  │         │ ───────► │           │
  └─────────┘          └───────────┘
        no Vec, no allocation, and `break` stops the scan
```

Peak memory: one token. Earliest observation of token #1: immediately. A
`.any(…)` or `.find(…)` that hits on the first word never looks at the rest of
the document.

`tokenize_into()` — **materialise into memory you already own**. Identical to
`tokenize()` except that the box is yours and survives the call:

```text
  iteration 1        iteration 2         iteration 3
  ┌──────────┐       ┌──────────┐        ┌──────────┐
  │ buf.clear│       │ buf.clear│        │ buf.clear│   ← len = 0,
  └────┬─────┘       └────┬─────┘        └────┬─────┘     capacity kept
       ▼                  ▼                   ▼
   extend(doc1)       extend(doc2)        extend(doc3)
       │                  │                   │
       ▼                  ▼                   ▼
   ┌──────────────────────────────────────────────┐
   │ one heap buffer, allocated at most once      │
   └──────────────────────────────────────────────┘
```

<div class="callout callout-warn">
<strong>Careful.</strong> <code>tokenize_into</code> does <strong>not</strong>
clear <code>out</code>; its body is
<code>out.extend(self.tokens(text))</code>. The <code>buf.clear()</code> in that
diagram is <em>your</em> line, and leaving it out gives you a buffer holding
every document at once.
</div>

## Comparison table

| API | Best for | Lazy | Materialises | Buffer reuse | Allocations | Token type |
|---|---|:--:|:--:|:--:|---|---|
| `Tokenize::tokens` | pipelines, folds, early exit | ✅ | ❌ | n/a | none, for the 13 slicing tokenizers | `Self::Token<'a>` |
| `Tokenize::tokenize` | one document, simplest call | ❌ | ✅ | ❌ | one `Vec`, grown by doubling | `Self::Token<'a>` |
| `Tokenize::tokenize_into` | a corpus through one buffer | ❌ | ✅ | ✅ | none once the buffer is warm | `Self::Token<'a>` |
| `verbora_core::Tokenizer::tokenize` | generic code; owned tokens | ❌ | ✅ | ❌ | one `Vec` **plus one `String` per token** | `String` |
| `verbora_core::Tokenizer::tokenize_into` | generic code, warm `Vec` | ❌ | ✅ | ✅ (the `Vec` only) | one `String` per token | `String` |
| `verbora_core::Tokenizer::tokenize_batch` | a slice of documents in one call | ❌ | ✅ | ❌ | outer `Vec` + inner `Vec` + `String` per token | `Vec<Vec<String>>` |
| `verbora_core::BorrowingTokenizer::tokenize_borrowed` | generic code, zero-copy | ❌ | ✅ | ❌ | one `Vec` | `&'a str` |
| `verbora_core::BorrowingTokenizer::tokenize_borrowed_into` | generic code, zero-copy, warm buffer | ❌ | ✅ | ✅ | none once warm | `&'a str` |
| inherent `tokens`/`tokenize`/`tokenize_into` on the four regex tokenizers | those four tokenizers | mixed | mixed | ✅ (`_into`) | see below | wrapped in `Option` |

"Lazy" means the tokenizer does no work until the iterator is advanced. Three of
the twenty `Tokenize` implementations are eager even from `tokens()`, because
their algorithm is inherently whole-text: `TreebankWordTokenizer` (seventeen
rewrite passes), `TokenizerJa` (a classifier over the entire string) and
`SentenceTokenizer` (the placeholder maps must be complete before any sentence
can be unmasked). Their `tokens()` builds the list and hands back
`into_iter()`. A fourth, `CaseTokenizer`, is lazy on ASCII input and eager on
anything else, because the non-ASCII path builds an intermediate UTF-16
buffer first. The signature stays uniform so generic code does not have
to special-case them, but `tokens()` will not save you an allocation there.

Among the four regex-driven tokenizers, `WordTokenizer`, `OrthographyTokenizer`
and `WordPunctTokenizer` are genuinely lazy; `RegexpTokenizer::tokens` is not —
its body collects into a `Vec` and returns `into_iter()`, because it has to know
whether the whole scan found no match at all — as distinct from matching zero
tokens — before it can hand you anything.

## Decision tree

```text
I need to tokenize text
│
├── Do I name a concrete tokenizer type in my code?
│   │
│   ├── YES ── use the `Tokenize` trait
│   │      │
│   │      ├── I look at each token once and never need them together
│   │      │      └── tokens()
│   │      │
│   │      ├── I want a Vec to keep, index, or return
│   │      │      └── tokenize()
│   │      │
│   │      └── I am in a loop over many documents
│   │             └── buf.clear(); tokenize_into(doc, &mut buf)
│   │
│   └── NO — my function takes "some tokenizer"
│          │
│          ├── I need owned Strings, or must accept any tokenizer
│          │      └── verbora_core::Tokenizer
│          │
│          └── I only need slices, and can require the zero-copy ones
│                 └── verbora_core::BorrowingTokenizer   (13 of 24 types)
│
├── My tokenizer is RegexpTokenizer / WordTokenizer /
│   OrthographyTokenizer / WordPunctTokenizer
│      └── inherent methods, all returning Option — `None` means no match at all
│
└── I have a slice of documents and want one call
       └── verbora_core::Tokenizer::tokenize_batch
           (a sequential map; it saves typing, not allocation)
```

## One example per variant

### `tokens()` — the primitive

<div class="perf">
<div class="perf-row"><span class="perf-k">Execution</span><span class="perf-v">Lazy (eager for Treebank, Japanese, Sentence, Norwegian/Swedish, non-ASCII Case)</span></div>
<div class="perf-row"><span class="perf-k">Output</span><span class="perf-v">Borrowed <code>&amp;str</code> for the 13 slicing tokenizers</span></div>
<div class="perf-row"><span class="perf-k">Allocations</span><span class="perf-v">None for a slicing tokenizer; the five eager exceptions above build a <code>Vec</code>/rewritten <code>String</code> before <code>tokens()</code> yields its first item</span></div>
<div class="perf-row"><span class="perf-k">Buffer reuse</span><span class="perf-v">N/A</span></div>
<div class="perf-row"><span class="perf-k">Batch</span><span class="perf-v">No</span></div>
<div class="perf-row"><span class="perf-k">Parallel</span><span class="perf-v">No</span></div>
<div class="perf-row"><span class="perf-k">Best for</span><span class="perf-v">Streaming token processing</span></div>
</div>

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

<div class="perf">
<div class="perf-row"><span class="perf-k">Execution</span><span class="perf-v">Eager</span></div>
<div class="perf-row"><span class="perf-k">Output</span><span class="perf-v"><code>Vec&lt;Self::Token&lt;'a&gt;&gt;</code></span></div>
<div class="perf-row"><span class="perf-k">Allocations</span><span class="perf-v">One <code>Vec</code>; no per-token allocation for the 13 slicing tokenizers</span></div>
<div class="perf-row"><span class="perf-k">Buffer reuse</span><span class="perf-v">No</span></div>
<div class="perf-row"><span class="perf-k">Batch</span><span class="perf-v">No</span></div>
<div class="perf-row"><span class="perf-k">Parallel</span><span class="perf-v">No</span></div>
<div class="perf-row"><span class="perf-k">Best for</span><span class="perf-v">One document, when the list is the answer</span></div>
</div>

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

Random access, `len()`, sorting and returning the tokens all need the `Vec`.
Reaching for `tokens().collect()` here would be the same code with extra steps —
`tokenize` *is* `tokens().collect()`.

### `tokenize_into()` — the hot loop

<div class="perf">
<div class="perf-row"><span class="perf-k">Execution</span><span class="perf-v">Eager</span></div>
<div class="perf-row"><span class="perf-k">Output</span><span class="perf-v">Appended to the caller's <code>Vec</code></span></div>
<div class="perf-row"><span class="perf-k">Allocations</span><span class="perf-v">None once the buffer's capacity is sufficient</span></div>
<div class="perf-row"><span class="perf-k">Buffer reuse</span><span class="perf-v">Yes</span></div>
<div class="perf-row"><span class="perf-k">Batch</span><span class="perf-v">No</span></div>
<div class="perf-row"><span class="perf-k">Parallel</span><span class="perf-v">No</span></div>
<div class="perf-row"><span class="perf-k">Best for</span><span class="perf-v">A corpus, where the per-document <code>Vec</code> would dominate</span></div>
</div>

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

Appending is occasionally what you want. Dropping the `clear` concatenates the
corpus into one token list, which is a legitimate use and the reason the method
does not clear on your behalf:

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

<div class="perf">
<div class="perf-row"><span class="perf-k">Execution</span><span class="perf-v">Eager</span></div>
<div class="perf-row"><span class="perf-k">Output</span><span class="perf-v"><code>Vec&lt;String&gt;</code></span></div>
<div class="perf-row"><span class="perf-k">Allocations</span><span class="perf-v">One <code>Vec</code> and one <code>String</code> per token</span></div>
<div class="perf-row"><span class="perf-k">Buffer reuse</span><span class="perf-v"><code>tokenize_into</code> reuses the <code>Vec</code>, never the <code>String</code>s</span></div>
<div class="perf-row"><span class="perf-k">Batch</span><span class="perf-v"><code>tokenize_batch</code>, sequential</span></div>
<div class="perf-row"><span class="perf-k">Parallel</span><span class="perf-v">No</span></div>
<div class="perf-row"><span class="perf-k">Best for</span><span class="perf-v">Functions that must accept any tokenizer</span></div>
</div>

`Tokenize::Token<'a>` is a generic associated type. That is what lets a slicing
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

Twenty of the twenty-four tokenizer types implement it. Note that `Tokenizer` is
**not object-safe** — `tokenize_batch` is generic — so `Box<dyn Tokenizer>` does
not compile. `verbora-ngrams` needed exactly that and solved it with a small
`dyn`-compatible trait plus a blanket impl; do the same if you need runtime
dispatch.

### `verbora_core::BorrowingTokenizer` — generic, zero-copy

<div class="perf">
<div class="perf-row"><span class="perf-k">Execution</span><span class="perf-v">Eager</span></div>
<div class="perf-row"><span class="perf-k">Output</span><span class="perf-v">Borrowed <code>&amp;'a str</code></span></div>
<div class="perf-row"><span class="perf-k">Allocations</span><span class="perf-v">One <code>Vec</code>; none at all for <code>_into</code> with a warm buffer</span></div>
<div class="perf-row"><span class="perf-k">Buffer reuse</span><span class="perf-v">Yes, via <code>tokenize_borrowed_into</code></span></div>
<div class="perf-row"><span class="perf-k">Batch</span><span class="perf-v">No</span></div>
<div class="perf-row"><span class="perf-k">Parallel</span><span class="perf-v">No</span></div>
<div class="perf-row"><span class="perf-k">Best for</span><span class="perf-v">Generic code that can restrict itself to the 13 slicers</span></div>
</div>

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

<div class="perf">
<div class="perf-row"><span class="perf-k">Execution</span><span class="perf-v">Lazy, except <code>RegexpTokenizer::tokens</code></span></div>
<div class="perf-row"><span class="perf-k">Output</span><span class="perf-v"><code>Option</code> of the token sequence — <code>None</code> means no match at all</span></div>
<div class="perf-row"><span class="perf-k">Allocations</span><span class="perf-v">None for <code>WordTokenizer</code> splitting mode; one <code>Vec</code> for <code>RegexpTokenizer::tokens</code></span></div>
<div class="perf-row"><span class="perf-k">Buffer reuse</span><span class="perf-v">Yes, via their inherent <code>tokenize_into</code></span></div>
<div class="perf-row"><span class="perf-k">Batch</span><span class="perf-v">No</span></div>
<div class="perf-row"><span class="perf-k">Parallel</span><span class="perf-v">No</span></div>
<div class="perf-row"><span class="perf-k">Best for</span><span class="perf-v">Callers that must distinguish "no match" from "no tokens"</span></div>
</div>

`RegexpTokenizer`, `WordTokenizer`, `OrthographyTokenizer` and
`WordPunctTokenizer` implement neither trait. In matching mode, "no match at
all" is a distinct outcome from "matched, but produced zero tokens," and no
trait in this workspace can express that distinction. They keep the same
three method names as inherent methods, wrapped in `Option`; their
`tokenize_into` returns `bool` instead, `false` meaning "no match at all," in
which case nothing was appended.

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

### `tokenize_batch` — what it actually is

<div class="perf">
<div class="perf-row"><span class="perf-k">Execution</span><span class="perf-v">Eager, sequential</span></div>
<div class="perf-row"><span class="perf-k">Output</span><span class="perf-v"><code>Vec&lt;Vec&lt;String&gt;&gt;</code></span></div>
<div class="perf-row"><span class="perf-k">Allocations</span><span class="perf-v">One outer <code>Vec</code>, one inner <code>Vec</code> per document, one <code>String</code> per token</span></div>
<div class="perf-row"><span class="perf-k">Buffer reuse</span><span class="perf-v">None — despite the doc comment</span></div>
<div class="perf-row"><span class="perf-k">Batch</span><span class="perf-v">Yes, in the sense that it takes a slice</span></div>
<div class="perf-row"><span class="perf-k">Parallel</span><span class="perf-v">No</span></div>
<div class="perf-row"><span class="perf-k">Best for</span><span class="perf-v">Shortening a call site</span></div>
</div>

`tokenize_batch` is a provided method on `verbora_core::Tokenizer`. Its default
body, in full:

```rust  ignore
fn tokenize_batch<S: AsRef<str>>(&self, texts: &[S]) -> Vec<Vec<String>> {
    texts.iter().map(|t| self.tokenize(t.as_ref())).collect()
}
```

That is a sequential `map` calling `tokenize` once per document, and `tokenize`
allocates a fresh `Vec` every time. **No tokenizer in this workspace overrides
it.** Its doc comment says the default "reuses one output buffer's capacity
across documents"; the code does not do that, and the doc comment is wrong.

So `tokenize_batch` is a convenience, not an optimisation. Use it when you want
`Vec<Vec<String>>` and a shorter call site. If you are reaching for it to make a
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

## The simple API is not the bad API

There is a failure mode in performance-conscious libraries where every example
uses the lowest-level call, and readers conclude that the readable one is a trap.
It is not, and this section says so on purpose.

**`tokenize()` is the right default.** It is one allocation. For the thirteen
slicing tokenizers it allocates *only* the `Vec` — every token inside it is a
`&str` pointing into your original string, so there is no per-token cost to
avoid. If you are tokenizing one document, or a hundred, or you are tokenizing
inside code that then does real work per token, the `Vec` is not what your
profile will be about.

**Choose `tokens()` because it fits, not because it is faster.** It fits when
tokens flow straight into a consumer, when you might stop early, or when the
document is large enough that you would rather not hold the whole token list.
Wrapping `tokens()` in a `collect()` to get a `Vec` back is literally the body of
`tokenize()`; you have written the same program with more words.

**Choose `tokenize_into()` when you have measured a loop.** Its advantage is
one thing only: it does not allocate a `Vec` per document. That matters at
corpus scale and is invisible at document scale — and it costs you a mutable
buffer, a `clear()` you must not forget, and a lifetime relationship between
the buffer and the text.

**None of these choices changes your results.** All three go through the same
iterator. If you pick the simplest one now and a benchmark later tells you the
allocation matters, the change is mechanical.

> Verbora has no recorded tokenizer benchmark results, so this page states no
> crossover point between the three APIs.
> Not yet benchmarked — see [Benchmarks](../benchmarks/index.md).
> The harness exists at `crates/verbora-tokenizers/benches/tokenizers.rs` and
> its `api-shape` group measures exactly this comparison.

## What does not exist

Stated plainly, so you do not go looking:

- **Parallel tokenization is opt-in, not default.** `Tokenize::par_tokenize_batch`
  is a provided method behind verbora-tokenizers's `parallel` Cargo feature —
  never on unless you enable it — and it is exactly
  `texts.par_iter().map(|t| self.tokenize(t)).collect()`, a thin `rayon`
  fan-out over `Self::tokenize` (the `tokenize()` call shown above), not a
  second implementation. Every tokenizer is single threaded without that
  feature.
  What Verbora gives you either way is tokenizers that are zero-sized (or
  immutable), stateless and `Send + Sync`, so parallelising *across documents*
  in your own code is also straightforward for anything the built-in method
  doesn't cover — `rayon`'s `docs.par_iter().map(|d| tokenizer.tokenize(d))`
  works, with `rayon` as your own dependency. See
  [Parallelism](../performance/parallelism.md).
- **There is no streaming reader API.** Every entry point takes a `&str` that is
  fully in memory. There is no `tokenize_read(impl BufRead)`.
- **Sequential batch means one thing only:** `verbora_core::Tokenizer::tokenize_batch`,
  the sequential `map` described above. `Tokenize::par_tokenize_batch` is the
  only other batch-shaped method in this crate, and it exists solely under the
  `parallel` feature; none of the four `Option`-returning tokenizers has a
  batch method at all.
- **There is no buffer-reusing batch call.** The combination "many documents,
  one buffer" exists only as a loop you write around `tokenize_into`.
- **`_into` never clears** anywhere in the tokenizers. (`Stemmer::stem_into` in
  `verbora_core` *does* clear its `String` — that inconsistency is real and is
  documented on [API shapes](./api-shapes.md).)

## Related

- [Tokenizers](../features/tokenizers.md) — the catalogue of all twenty-five.
- [API shapes](./api-shapes.md) — the workspace-wide conventions.
- [Iterator vs `_into`](../performance/iterator-vs-into.md)
- [Buffer reuse](../performance/buffer-reuse.md)
- [Zero-copy](../performance/zero-copy.md)
- [Allocation](../performance/allocation.md)
- [Batch vs streaming](../performance/batch-vs-streaming.md)
- [Parallelism](../performance/parallelism.md)
- [Performance](../performance/index.md) and [Benchmarks](../benchmarks/index.md)
