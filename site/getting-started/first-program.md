# Your first program

One task, written four ways. The task: count how many tokens in a document are
longer than six characters. All four versions are correct — they differ in what
they do with memory, which is the choice Verbora asks you to make.

Every snippet on this page compiles and runs against the real crates.

## 1. The straightforward version

```rust
use verbora_tokenizers::{AggressiveTokenizer, Tokenize};

fn long_tokens(text: &str) -> usize {
    let tokenizer = AggressiveTokenizer::new();
    let tokens = tokenizer.tokenize(text);
    tokens.iter().filter(|t| t.len() > 6).count()
}

assert_eq!(long_tokens("tokenizing documents efficiently is not automatic"), 4);
```

`tokenize()` returns a `Vec<&str>` — one heap allocation for the vector, and the
tokens themselves are slices borrowed from `text`. Reach for this by default.
For a program that tokenizes a few thousand strings, that one allocation is not
measurable.


## 2. The lazy version

```rust
use verbora_tokenizers::{AggressiveTokenizer, Tokenize};

fn long_tokens(text: &str) -> usize {
    let tokenizer = AggressiveTokenizer::new();
    tokenizer.tokens(text).filter(|t| t.len() > 6).count()
}

assert_eq!(long_tokens("tokenizing documents efficiently is not automatic"), 4);
```

`tokens()` is an iterator. No `Vec` is built at all: each token is produced,
tested and dropped before the next one is scanned, and the filter fuses into the
scan rather than running as a second pass over a materialised collection.

Laziness pays twice as much when you can stop early:

```rust
use verbora_tokenizers::{AggressiveTokenizer, Tokenize};

let tokenizer = AggressiveTokenizer::new();

// Stops scanning at the first match. `tokenize()` would have split the whole
// string first, then searched it.
let first_long = tokenizer
    .tokens("a bb ccc dddddddd eeeeeeeee")
    .find(|t| t.len() > 6);

assert_eq!(first_long, Some("dddddddd"));
```


## 3. The buffer-reusing version

Now the task changes shape: you have a corpus, and you need *all* the tokens of
each document at once — perhaps to pass a slice to something else.

```rust
use verbora_tokenizers::{AggressiveTokenizer, Tokenize};

fn count_across(corpus: &[&str]) -> usize {
    let tokenizer = AggressiveTokenizer::new();
    let mut buf = Vec::new();
    let mut total = 0;

    for document in corpus {
        buf.clear();                                // keeps the capacity
        tokenizer.tokenize_into(document, &mut buf);
        total += buf.iter().filter(|t| t.len() > 6).count();
    }

    total
}

assert_eq!(count_across(&["tokenizing documents", "efficiently automatic"]), 4);
```

`tokenize_into` **appends** to the buffer — it does not clear it for you. The
`buf.clear()` is yours to write, and forgetting it is the classic bug here.
Clearing a `Vec` drops its elements but keeps its allocation, so after the first
few documents the loop stops calling the allocator entirely.

<div class="callout callout-warn">
<strong>Careful.</strong> <code>Tokenize::tokenize_into</code> appends;
<code>verbora_core::Stemmer::stem_into</code> clears first. The two conventions
differ deliberately, and each is documented on its own trait. Check before you
assume.
</div>


## 4. The parallel version

`Tokenize::par_tokenize_batch`, behind `verbora-tokenizers`' `parallel` Cargo
feature, fans `tokenize()` out across documents with `rayon` and hands back
`Vec<Vec<Self::Token<'a>>>` — the tokens themselves. This task wants a count
rather than the tokens, so the fan-out goes at your call site instead:

```rust  ignore
use rayon::prelude::*;

let total: usize = corpus
    .par_iter()
    .map(|doc| {
        let tokenizer = AggressiveTokenizer::new();  // cheap: a unit struct
        tokenizer.tokens(doc).filter(|t| t.len() > 6).count()
    })
    .sum();
```

This works because Verbora's tokenizers are stateless values with no interior
mutability: nothing is shared, so nothing needs locking. Verbora never guesses a
chunk size for you or spins up a thread pool you did not ask for — every
built-in `par_*` API is opt-in via a Cargo feature and runs only when you call
it by name. [Parallelism](../performance/parallelism.md) lists the built-ins and
the measured crossover points, because the answer to "is this faster?" is not
always yes.

## Which one should I have written?

| What you are doing | Call |
|---|---|
| Counting tokens in one string, once | `tokenize()` — start here |
| Feeding tokens into a pipeline, or stopping early | `tokens()` |
| Re-tokenizing document after document in a loop | `tokenize_into()` with one reused buffer |
| A corpus large enough that CPU time actually matters | `par_tokenize_batch` (`parallel` feature) if it fits, otherwise `rayon` at your call site over `tokens()` |

The full version of this reasoning, with comparison tables and every subsystem's
variants, is in [Choosing the right API](../choosing/index.md).

## Where to go next

- [The workspace map](workspace.md) — what each crate is for.
- [Tokenizers](../features/tokenizers.md) — all 25 of them.
- [Iterator vs reusable buffer](../performance/iterator-vs-into.md) — why
  versions 2 and 3 are *not* substitutes for one another.
