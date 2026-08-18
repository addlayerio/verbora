# Batch corpora

Many documents, offline, throughput is the metric. Index building, corpus
statistics, bulk import.

**Priorities:** memory reuse, shared setup, removing per-item allocations.
**Non-priority:** latency of any single item.

## The canonical loop

```rust
use verbora_tokenizers::{AggressiveTokenizer, Tokenize};

fn token_counts(corpus: &[&str]) -> Vec<usize> {
    // 1. Setup hoisted out of the loop.
    let tokenizer = AggressiveTokenizer::new();

    // 2. Output pre-sized: no growth reallocation.
    let mut counts = Vec::with_capacity(corpus.len());

    // 3. One working buffer for the whole corpus.
    let mut buf = Vec::new();

    for document in corpus {
        buf.clear();                              // capacity survives
        tokenizer.tokenize_into(document, &mut buf);
        counts.push(buf.len());
    }

    counts
}

assert_eq!(token_counts(&["a b c", "d e", "f"]), [3, 2, 1]);
```

Three techniques, in order of how much they usually matter:

1. **Reuse the buffer.** Turns *n* allocations into roughly `log₂(max_tokens)`.
2. **Pre-size the output.** Turns `log₂(n)` reallocations into one.
3. **Hoist setup.** Free for the zero-sized tokenizers; real for
   `OrthographyTokenizer`, `SentenceTokenizer::with_abbreviations` and anything
   holding a compiled regex.

<div class="callout callout-warn">
<strong><code>buf.clear()</code> is yours to write.</strong>
<code>Tokenize::tokenize_into</code> <em>appends</em>. Omitting the clear does
not error — it silently accumulates and your counts grow monotonically. This is
the most common bug in this pattern.
</div>

## Do not use `tokenize_batch`

```rust  ignore
// Looks right. Is not what you want.
let all = tokenizer.tokenize_batch(corpus);
```

`verbora_core::Tokenizer::tokenize_batch` is a provided trait method whose
default body is a sequential `map`: one fresh `Vec<String>` per document, no
shared buffer, no parallelism, and **owned** `String` tokens rather than the
borrowed `&str` that `Tokenize::tokenize` gives you. Nothing overrides it, so it
allocates strictly more than the loop above. It exists so generic code over the
trait can say "process all of these" — for throughput, write the loop.

<div class="callout callout-note">
<strong>Not the same as <code>par_tokenize_batch</code>.</strong>
<code>verbora_tokenizers::Tokenize</code> — the trait
<code>AggressiveTokenizer</code> implements and this page uses throughout —
does have a real batch primitive, <code>par_tokenize_batch</code>, behind the
<code>parallel</code> Cargo feature. It fans <code>tokenize()</code> out across
threads with <code>rayon</code>, one fresh <code>Vec</code> per document, so it
buys throughput rather than memory reuse. See
<a href="parallel-corpus">Massive parallel corpora</a>.
</div>

## Accumulating instead of clearing

Because `tokenize_into` appends, gathering a whole corpus into one flat buffer
needs no extra API:

```rust
use verbora_tokenizers::{AggressiveTokenizer, Tokenize};

let corpus = ["the quick brown", "fox jumps over"];
let tokenizer = AggressiveTokenizer::new();

let mut all = Vec::new();
for document in corpus {
    tokenizer.tokenize_into(document, &mut all);   // deliberately no clear
}

assert_eq!(all.len(), 6);
assert_eq!(all[3], "fox");
```

Every token borrows its own document, so the documents must outlive `all`.

## Counting across a corpus

A frequency pass, with one allocation per *distinct* term rather than per
occurrence:

```rust
use std::collections::HashMap;

use verbora_normalizers::remove_diacritics;
use verbora_tokenizers::{AggressiveTokenizer, Tokenize};

fn term_frequencies(corpus: &[&str]) -> HashMap<String, usize> {
    let tokenizer = AggressiveTokenizer::new();
    let mut freq: HashMap<String, usize> = HashMap::new();

    for document in corpus {
        let folded = remove_diacritics(document);   // borrowed when unaccented

        for token in tokenizer.tokens(&folded) {
            let lower = token.to_lowercase();
            // Only allocates a key the first time a term is seen.
            *freq.entry(lower).or_insert(0) += 1;
        }
    }

    freq
}

let freq = term_frequencies(&["Café café", "cafe"]);
assert_eq!(freq["cafe"], 3);
```

Note the shape: `tokens()` rather than `tokenize_into`, because each token is
consumed immediately and never needs to sit in a collection.

## N-grams over a corpus

```rust
use verbora_ngrams::ngrams;
use verbora_tokenizers::{AggressiveTokenizer, Tokenize};

fn bigram_count(corpus: &[&str]) -> usize {
    let tokenizer = AggressiveTokenizer::new();
    let mut tokens = Vec::new();
    let mut total = 0;

    for document in corpus {
        tokens.clear();
        tokenizer.tokenize_into(document, &mut tokens);
        total += ngrams(&tokens, 2, None, None).len();
    }

    total
}

assert_eq!(bigram_count(&["a b c", "d e"]), 3);
```

`ngrams` takes a slice, which is exactly what the reused buffer gives you — this
is a case where `tokenize_into` fits better than `tokens()`, because the next
stage wants the whole collection.

Use `ngrams_iter` instead if you only need to *stream* the windows; use
`ngrams_with_stats` only if you actually want the frequency map, which costs a
`String` key per distinct n-gram.

## Chunking to bound memory

If the corpus does not fit, process it in windows and keep the same buffer:

```rust
use verbora_tokenizers::{AggressiveTokenizer, Tokenize};

fn process_chunked(corpus: &[&str], chunk: usize) -> usize {
    let tokenizer = AggressiveTokenizer::new();
    let mut buf = Vec::new();
    let mut total = 0;

    for window in corpus.chunks(chunk) {
        for document in window {
            buf.clear();
            tokenizer.tokenize_into(document, &mut buf);
            total += buf.len();
        }
        // Per-chunk work (flush an index segment, write a shard, …) goes here.
    }

    total
}

assert_eq!(process_chunked(&["a b", "c d", "e f"], 2), 6);
```

This is also the shape you want if you may parallelise later — see
[Massive parallel corpora](parallel-corpus.md).

## Measuring the change

Batch is the one workload where these techniques reliably show up, so verify
rather than assume:

```bash
# Before and after, with Criterion's built-in comparison.
cargo bench -p verbora-tokenizers -- --save-baseline before
# ... apply the buffer reuse ...
cargo bench -p verbora-tokenizers -- --baseline before
```

If the numbers do not move, the allocation was not your bottleneck — put the
simple version back.

## Checklist

- [ ] One working buffer, reused, with `clear()` at the top of the loop
- [ ] Output `Vec`s pre-sized with `with_capacity`
- [ ] Expensive constructions hoisted out of the loop
- [ ] `tokenize_batch` **not** used
- [ ] `_with_stats` only where the statistics are actually consumed
- [ ] Before/after measured

## Related

- [Buffer reuse](../performance/buffer-reuse.md)
- [Batch vs streaming](../performance/batch-vs-streaming.md)
- [Massive parallel corpora](parallel-corpus.md)
