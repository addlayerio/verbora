# The four API shapes

Verbora's naming is regular. Once you know the four shapes, you can predict what
a function does — and what it costs — from its name alone.

| Shape | Name pattern | Returns | Allocates | Lazy |
|---|---|---|---|:--:|
| Eager | `verb(input)` | owned collection or `String` | yes — the container | ❌ |
| Lazy | `nouns(input)`, `verb_iter(input)`, `iter_*` | an `Iterator` | no | ✅ |
| Into-buffer | `verb_into(input, &mut out)` | `()` or `bool` | no, after warm-up | ❌ |
| Batch | `verb_batch(&[input])` | `Vec<…>` per input | yes | ❌ |

<div class="callout callout-note">
<strong>Naming is a promise, not a guarantee of existence.</strong> Not every
subsystem has all four. Phonetics has only the eager shape; distance has only
eager scalars. The feature pages say exactly which exist.
</div>

## 1. Eager — `tokenize()`, `process()`, `pluralize()`, `keys_with_prefix()`

<a class="badge badge-owned" href="../performance/allocation">OWNED</a>

Does the work now, hands back a complete result you own.

```rust
use verbora_tokenizers::{AggressiveTokenizer, Tokenize};

let t = AggressiveTokenizer::new();
let tokens = t.tokenize("the quick brown fox");

assert_eq!(tokens.len(), 4);
assert_eq!(tokens[2], "brown");
```

**Use it when** the result is small, you need random access or a length, you are
passing it somewhere that wants a slice, or you simply want the code to read
well. That is most code.

**Do not use it when** you are about to consume the result once, in order, and
throw it away — that is what the lazy shape is for — or when you are calling it
in a loop millions of times and the container allocation shows up in a profile.

**Cost.** One container allocation, plus growth reallocations as it fills. Note
what is *not* allocated: for the thirteen character-class tokenizers the tokens
are `&str` slices of your input, so there is no per-token `String`.

## 2. Lazy — `tokens()`, `ngrams_iter()`, `iter_keys_with_prefix()`

<a class="badge badge-lazy" href="../performance/iterator-vs-into">LAZY</a>
<a class="badge badge-zerocopy" href="../performance/zero-copy">ZERO-COPY</a>

Returns an iterator. Nothing happens until you pull.

```rust
use verbora_tokenizers::{AggressiveTokenizer, Tokenize};

let t = AggressiveTokenizer::new();

let shouty: Vec<String> = t
    .tokens("the quick brown fox")
    .filter(|w| w.len() > 3)
    .map(|w| w.to_uppercase())
    .collect();

assert_eq!(shouty, ["QUICK", "BROWN"]);
```

**Use it when** you are building a pipeline, when you might stop early, when the
input is large enough that materialising every token at once matters, or when
you want to hand a stream to another API that takes `IntoIterator` — such as
[`phoneticize_tokens`](../features/phonetics.md), which composes directly with a
tokenizer's iterator and never builds the intermediate `Vec`.

**Do not use it when** you need the result more than once, need its length up
front, or need indexing. Re-running an iterator means re-doing the work; a `Vec`
you can read twice.

**Cost.** For `AggressiveTokenizer` and the other twelve character-class
tokenizers, nothing per token — the iterator is a small struct on the stack,
scanning as it goes. Not every `Tokenize` implementor is this lazy end to end:
`TreebankWordTokenizer`, `TokenizerJa`, `SentenceTokenizer`,
`AggressiveTokenizerNo`/`Sv` and `CaseTokenizer` on non-ASCII input each do some
eager work inside `tokens()` before yielding the first item — see
[Tokenizers](../features/tokenizers.md) for which.

<div class="callout callout-note">
<strong>Borrow-checker note.</strong> A lazy iterator borrows the input string
for as long as it lives. If you want to return tokens from a function that owns
its input, you must either collect or restructure so the caller owns the text.
This is not a Verbora limitation — it is the price of not copying.
</div>

## 3. Into-buffer — `tokenize_into()`, `pluralize_into()`, `stem_into()`

<a class="badge badge-reuse" href="../performance/buffer-reuse">BUFFER REUSE</a>

Writes into storage *you* own and keep between calls.

```rust
use verbora_tokenizers::{AggressiveTokenizer, Tokenize};

let t = AggressiveTokenizer::new();
let corpus = ["the quick brown fox", "jumps over the lazy dog"];

let mut buf = Vec::new();
let mut total = 0;

for document in corpus {
    buf.clear();                          // capacity survives; contents do not
    t.tokenize_into(document, &mut buf);
    total += buf.len();
}

assert_eq!(total, 9);
```

**Use it when** the same operation runs in a tight loop and the container
allocation is a real cost — batch jobs, corpus indexing, offline processing.

**Do not use it when** you call the operation once. You have added a mutable
binding and a manual `clear()` to your code in exchange for one saved
allocation.

**Cost.** After the buffer reaches its high-water mark, zero allocations. Before
that, the same growth pattern as the eager shape.

<div class="callout callout-warn">
<strong>The two conventions.</strong> <code>Tokenize::tokenize_into</code>
<em>appends</em> — it does not clear, so you can accumulate across inputs on
purpose. <code>Stemmer::stem_into</code> <em>clears first</em>. They differ
because appending tokens across documents is useful and appending stem fragments
is not, but you must check per API. Each one says so in its rustdoc.
</div>

## 4. Batch — `tokenize_batch()`, `stem_batch()`

<a class="badge badge-batch" href="../performance/batch-vs-streaming">BATCH</a>

Takes a slice of inputs, returns a result per input.

```rust
use verbora_core::Tokenizer;
use verbora_tokenizers::AggressiveTokenizer;

let t = AggressiveTokenizer::new();
let out = t.tokenize_batch(&["one two", "three four five"]);

assert_eq!(out.len(), 2);
assert_eq!(out[1].len(), 3);
```

<div class="callout callout-warn">
<strong>Read this before reaching for it.</strong> Verbora's batch methods are
provided methods on the <code>verbora_core::Tokenizer</code> and
<code>verbora_core::Stemmer</code> traits, and their default bodies are a plain
sequential <code>map</code> — one fresh <code>Vec&lt;String&gt;</code> per input,
no shared buffer, no parallelism. They exist so that generic code can express
"process these all" and so implementations can override them later. Today,
<code>tokenize_batch</code> is not faster than a loop, and it allocates
<em>more</em> than <code>tokenize_into</code> does — note that it also produces
<code>Vec&lt;String&gt;</code> rather than the borrowed <code>&amp;str</code>
that <code>Tokenize::tokenize</code> gives you. This is about the generic
<code>verbora_core</code> traits specifically, not the workspace as a whole —
see the note below.
</div>

**Use it when** you are writing generic code over the `Tokenizer` trait and want
the batch operation to improve automatically if an implementation overrides it.

**Do not use it when** you want throughput today. Reach for
`verbora-tokenizers`'s own `Tokenize::par_tokenize_batch` (behind its
`parallel` feature) or `tokenize_into` with a reused buffer, or `rayon` over
your own slice for anything without a built-in `par_*` variant — see
[Parallelism](../performance/parallelism.md).

## Two more shapes you will meet

These are not "levels" — they are type choices that change what you can do with
a result.

### `Cow`-returning functions

<a class="badge badge-cow" href="../performance/zero-copy">COW</a>

Four of the six top-level normalizers, plus every one of the 17
`ja::converters` and `Stemmer::stem`, return `Cow<'_, str>`: borrowed when
nothing changed, owned when something did. (`normalize` and `normalize_token`
are the two exceptions — see [Normalizers](../features/normalizers.md) for
why: one contraction can expand into several tokens, so the result is a
`Vec<String>` regardless.)

```rust
use std::borrow::Cow;
use verbora_normalizers::remove_diacritics;

// Nothing to fold — no allocation at all.
assert!(matches!(remove_diacritics("plain ascii"), Cow::Borrowed(_)));

// A fold happened — one String.
assert!(matches!(remove_diacritics("café"), Cow::Owned(_)));
assert_eq!(remove_diacritics("café"), "cafe");
```

This matters because these functions are usually called on text that needs no
change — a Latin sentence handed to the katakana converter, an ASCII token
handed to the diacritic folder. See
[Zero-copy and `Cow`](../performance/zero-copy.md).

### `Option`-returning tokenizers

<span class="badge badge-fallible">FALLIBLE</span>

The four regex-driven tokenizers return `Option<…>`, because "no match at
all" is observably different from "matched, but produced no tokens" — and
`Option` is how Verbora keeps that distinction expressible.

```rust
use verbora_tokenizers::WordTokenizer;

let t = WordTokenizer::new();
assert!(t.tokenize("hello world").is_some());
```

## Summary

```text
Which shape?
│
├── I want a result I can hold, index, or pass on
│      └── eager:        tokenize(), process(), keys_with_prefix()
│
├── I want to consume it once, in order — maybe not all of it
│      └── lazy:         tokens(), ngrams_iter(), iter_keys_with_prefix()
│
├── I am doing this millions of times with the same shape of output
│      └── into-buffer:  tokenize_into(), pluralize_into()
│
└── I am writing generic code over the trait
       └── batch:        tokenize_batch()   (sequential today)
```

Next: the same reasoning applied concretely, in
[Tokenization](tokenization.md).
