# Massive parallel corpora

Enough work that threads are worth the complexity.

**Priorities:** CPU utilisation, chunk sizing, per-worker state.
**Prerequisite:** the sequential version, already optimised and measured.

<div class="callout callout-note">
<strong>Check for a built-in first.</strong> Fourteen crates ship an optional,
feature-gated <code>par_*_batch</code> function — a thin <code>rayon</code>
fan-out over the crate's own sequential primitive, benchmarked and tested, not a
second implementation. If the table on
<a href="../performance/parallelism">Parallelism</a> lists an entry for the
operation you are about to hand-roll, enable that crate's
<code>parallel</code> feature and call it instead: less code, and it is the
version this project has measured.
</div>

## When a built-in already covers your workload

`verbora-tokenizers` is one of the fourteen. Tokenizing a whole corpus in
parallel needs no `rayon` in your own `Cargo.toml` at all:

```toml
[dependencies]
verbora-tokenizers = { version = "0.1", features = ["parallel"] }
```

```rust  ignore
use verbora_tokenizers::{WordTokenizer, par_tokenize_batch};

fn tokenize_corpus<'a>(corpus: &[&'a str]) -> Vec<Vec<&'a str>> {
    // One tokenize_borrowed() call per document, fanned out.
    par_tokenize_batch(&WordTokenizer, corpus)
}
```

`par_tokenize_batch` is a free function generic over any `BorrowingTokenizer`,
so every tokenizer in the crate works with it, and output order matches input
order. See
[Parallelism](../performance/parallelism.md) for the other thirteen built-ins —
WordNet lookups, spellcheck corrections, sentiment, stemming, phonetics,
distance, classification, TF-IDF ingestion, language detection and more — before
writing anything below by hand.

## Rolling your own

Reach for the patterns below when no built-in fits — a crate with no `par_*`
function (such as `verbora-ngrams`), or a computation none of the built-ins
wraps: a derived value rather than the wrapped function's return type (a
*count*, not the tokens themselves — exactly the example below), a reduction
into a shared structure, a multi-stage pipeline, or a shared read-only index.

All of it works because Verbora's operations — including the ones with no
built-in `par_*` API — are stateless and `Send + Sync`.

## Before you start

Do these first, in order. Each is cheaper than threading and each composes with
it:

1. **Reuse buffers.** See [Batch corpora](batch.md). Removing ten million
   allocations may make step 3 unnecessary.
2. **Narrow the work.** If you are comparing every document against every other,
   the fix is an index, not more cores.
3. **Measure the sequential version.** You need a baseline, or you cannot tell
   whether threading helped.

## Setup

```toml
[dependencies]
rayon = "1"
verbora-tokenizers = "0.1"
```

## The basic fan-out

```rust  ignore
use rayon::prelude::*;
use verbora_tokenizers::{BorrowingTokenizer, WordTokenizer};

fn token_counts(corpus: &[String]) -> Vec<usize> {
    corpus
        .par_iter()
        .map(|doc| WordTokenizer.tokens(doc).count())
        .collect()
}
```

`WordTokenizer` is a zero-sized type, so sharing it across threads costs nothing
and requires no synchronisation.

## Per-worker buffers

A `&mut Vec` cannot be shared, so buffer reuse and parallelism combine through
`map_init`, which gives each worker its own:

```rust  ignore
use rayon::prelude::*;
use verbora_tokenizers::{BorrowingTokenizer, WordTokenizer};

fn token_counts(corpus: &[String]) -> Vec<usize> {
    corpus
        .par_iter()
        .map_init(Vec::new, |buf, doc| {
            buf.clear();
            WordTokenizer.tokenize_borrowed_into(doc, buf);
            buf.len()
        })
        .collect()
}
```

`map_init` calls the initialiser once per worker thread, not once per item.

## Chunking for granularity

Per-document tasks over short documents are dominated by scheduling. Give each
task real work:

```rust  ignore
use rayon::prelude::*;
use verbora_tokenizers::{BorrowingTokenizer, WordTokenizer};

fn total_tokens(corpus: &[String]) -> usize {
    corpus
        .par_chunks(1024)                        // one task per 1024 documents
        .map(|chunk| {
            let mut buf: Vec<&str> = Vec::new(); // one buffer per chunk
            let mut local = 0;

            for doc in chunk {
                buf.clear();
                WordTokenizer.tokenize_borrowed_into(doc, &mut buf);
                local += buf.len();
            }

            local
        })
        .sum()
}
```

Pick the chunk size so each task takes on the order of 100 µs or more. For short
documents that is usually hundreds to thousands of them.

## A shared read-only index

```rust  ignore
use verbora_trie::Trie;
use rayon::prelude::*;
use std::sync::Arc;

fn lookup_all(index: Arc<Trie>, queries: &[String]) -> Vec<bool> {
    queries
        .par_iter()
        .map(|q| index.contains(q))     // &self: no locking
        .collect()
}
```

`Trie` is `Send + Sync`, so an `Arc<Trie>` can be queried from every thread at
once. **Construction cannot be parallelised** — `insert` takes `&mut self`.
Build it on one thread, then share it. When the index is finished, share
`trie.freeze()` instead: `FrozenTrie` is `Send + Sync` too, and its `keys_slice`
lets a worker read a prefix's matches without allocating a `Vec<String>` per
query.

## Parallel reduction into a shared map

```rust  ignore
use std::collections::HashMap;

use rayon::prelude::*;
use verbora_tokenizers::{BorrowingTokenizer, WordTokenizer};

fn term_frequencies(corpus: &[String]) -> HashMap<String, usize> {
    corpus
        .par_chunks(512)
        .map(|chunk| {
            // Each task builds its own map — no contention at all.
            let mut local: HashMap<String, usize> = HashMap::new();
            for doc in chunk {
                for token in WordTokenizer.tokens(doc) {
                    *local.entry(token.to_lowercase()).or_insert(0) += 1;
                }
            }
            local
        })
        .reduce(HashMap::new, |mut a, b| {
            for (k, v) in b {
                *a.entry(k).or_insert(0) += v;
            }
            a
        })
}
```

Per-task maps plus a merge beats a `Mutex<HashMap>` by a wide margin: no lock is
taken on the hot path, and the merge is `O(distinct terms)` rather than
`O(tokens)`.

## The things you must not share

<div class="callout callout-warn">
<strong>Process-global state.</strong> One binding in the workspace is
process-wide and mutable: <code>verbora_core</code>'s global stop-word list
(<code>add_global_stopword</code> and friends), which models a set-once,
read-everywhere default. Set it before you start workers, never from inside one —
a reader concurrent with a writer sees a nondeterministic mix of the old and new
value, with no error to tell you. Its only consumer in the workspace is
<code>verbora-stemmers</code>' English and Lancaster stop-word helpers.
<br><br>
Nothing else needs guarding. <code>phoneticize_tokens</code> takes a
<code>&amp;StopWords</code> argument and reads no global; a <code>TfIdf</code>
owns its own <code>Analyzer</code>, so two corpora on two threads cannot change
each other's notion of a term.
</div>

## Verifying it helped

Three checks, in order:

**Wall clock, not CPU time.** Halving latency while quadrupling CPU time is a bad
trade on a shared machine.

**A scaling curve, not a point.**

```bash
for n in 1 2 4 8 16; do
  RAYON_NUM_THREADS=$n cargo run --release --example your_benchmark
done
```

A curve that flattens after 4 threads means you are bound by memory bandwidth or
I/O, not by the CPU. Tokenization is a linear scan that allocates little, so it
saturates bandwidth relatively early; distance calculations on longer inputs do
more arithmetic per byte and scale further.

**Determinism.** `par_iter().collect()` preserves order. `for_each` with shared
mutable state does not. If your output ordering changed, you have a bug, not a
speedup.

Stop, and go back to the sequential version, if the stage takes less than a
second of total CPU, if you are already running one request per thread, or if
the scaling curve is flat past two threads.

## Checklist

- [ ] Sequential version optimised and measured first
- [ ] `par_chunks` sized so each task is ≥ ~100 µs
- [ ] `map_init` or per-chunk locals for scratch state, never a shared `&mut`
- [ ] Shared read-only structures behind `Arc`, built before the fan-out
- [ ] No global-state mutation from workers
- [ ] Scaling curve measured at 1/2/4/8/16 threads
- [ ] Output verified identical to the sequential version

## Related

- [Parallelism](../performance/parallelism.md)
- [Batch corpora](batch.md)
- [Buffer reuse](../performance/buffer-reuse.md)
