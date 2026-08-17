# Your first program

This page writes the *same* task four times. Not because three of them are
wrong, but because each answers a different question about memory — and knowing
which question you are asking is the whole skill this site tries to teach.

The task: count how many tokens in a document are longer than six characters.

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
tokens themselves are slices borrowed from `text`. This is the call you should
reach for by default. It is readable, it is correct, and for a program that
tokenizes a few thousand strings the allocation is not measurable.

<div class="perf">
<div class="perf-row"><span class="perf-k">Execution</span><span class="perf-v">Eager</span></div>
<div class="perf-row"><span class="perf-k">Output</span><span class="perf-v"><code>Vec&lt;&amp;str&gt;</code> — tokens borrow the input</span></div>
<div class="perf-row"><span class="perf-k">Allocations</span><span class="perf-v">One <code>Vec</code>, grown as it fills; none per token</span></div>
<div class="perf-row"><span class="perf-k">Best for</span><span class="perf-v">Almost everything</span></div>
</div>

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
tested and dropped before the next one is scanned. It is also *composable* — the
filter fuses into the scan rather than running as a second pass over a
materialised collection.

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

<div class="perf">
<div class="perf-row"><span class="perf-k">Execution</span><span class="perf-v">Lazy</span></div>
<div class="perf-row"><span class="perf-k">Output</span><span class="perf-v">Borrowed <code>&amp;str</code>, one at a time</span></div>
<div class="perf-row"><span class="perf-k">Allocations</span><span class="perf-v">None</span></div>
<div class="perf-row"><span class="perf-k">Best for</span><span class="perf-v">Pipelines, early termination, bounded memory</span></div>
</div>

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

<div class="perf">
<div class="perf-row"><span class="perf-k">Execution</span><span class="perf-v">Eager, into caller storage</span></div>
<div class="perf-row"><span class="perf-k">Output</span><span class="perf-v">Appended to your <code>Vec</code></span></div>
<div class="perf-row"><span class="perf-k">Allocations</span><span class="perf-v">Amortised to zero once the buffer reaches its high-water mark</span></div>
<div class="perf-row"><span class="perf-k">Best for</span><span class="perf-v">Tight loops over many documents</span></div>
</div>

## 4. Rolling your own parallel version

Most operations in Verbora still have no dedicated parallel entry point, but
`Tokenize::par_tokenize_batch` — behind verbora-tokenizers' `parallel` Cargo
feature — is one of thirteen exceptions across the workspace; see
[Parallelism](../performance/parallelism.md) for the full table. It fans
`tokenize()` out across documents with `rayon` and hands back
`Vec<Vec<Self::Token<'a>>>` — the tokens themselves, not a derived count. This
example needs a count, which the built-in doesn't produce, so it is still a
case where you write the fan-out yourself:

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
mutability, so nothing is shared and nothing needs locking. What Verbora does
*not* do is guess a chunk size for you or spin up a thread pool you did not ask
for — every built-in `par_*` API, `par_tokenize_batch` included, is opt-in via
a Cargo feature and never runs unless you call it by name. See
[Parallelism](../performance/parallelism.md) for the other twelve built-ins and
when any of this is actually faster — the answer is not "always".

## Which one should I have written?

```text
Counting tokens in one string, once
        └── tokenize()   ← start here

Feeding tokens into a pipeline, or stopping early
        └── tokens()

Re-tokenizing document after document in a loop
        └── tokenize_into() with one reused buffer

Corpus large enough that CPU time actually matters
        └── par_tokenize_batch (parallel feature) if it fits, otherwise
            rayon at your call site, over tokens()
```

The full version of this reasoning, with comparison tables and every subsystem's
variants, is in [Choosing the right API](../choosing/index.md).

## Where to go next

- [The workspace map](workspace.md) — what the nine crates are for.
- [Tokenizers](../features/tokenizers.md) — all 25 of them.
- [Iterator vs reusable buffer](../performance/iterator-vs-into.md) — why
  versions 2 and 3 are *not* substitutes for one another.
