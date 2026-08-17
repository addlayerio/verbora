# Massive parallel corpora

Enough work that threads are worth the complexity.

**Priorities:** CPU utilisation, chunk sizing, per-worker state.
**Prerequisite:** the sequential version, already optimised and measured.

<div class="callout callout-note">
<strong>Check for a built-in first.</strong> Thirteen crates now ship an
optional, feature-gated <code>par_*_batch</code> function — a thin
<code>rayon</code> fan-out over the crate's own sequential primitive,
benchmarked and tested, not a second implementation. If the table on
<a href="../performance/parallelism">Parallelism</a> already lists an entry
for the operation you're about to hand-roll below, enable that crate's
<code>parallel</code> feature and call it instead — it's less code, and it's
the version this project has actually measured.
</div>

## When a built-in already covers your workload

`verbora-tokenizers` is one of the thirteen. Tokenizing a whole corpus in
parallel needs no `rayon` in your own `Cargo.toml` at all:

```toml
[dependencies]
verbora-tokenizers = { version = "0.1", features = ["parallel"] }
```

```rust  ignore
use verbora_tokenizers::{AggressiveTokenizer, Tokenize};

fn tokenize_corpus<'a>(corpus: &[&'a str]) -> Vec<Vec<&'a str>> {
    let tokenizer = AggressiveTokenizer::new();
    tokenizer.par_tokenize_batch(corpus)   // one tokenize() call per document, fanned out
}
```

`par_tokenize_batch` is a default method on the `Tokenize` trait, so every
tokenizer in the crate gets it for free, and output order matches input order.
See [Parallelism](../performance/parallelism.md) for the other twelve
built-ins — WordNet lookups, spellcheck corrections, sentiment, stemming,
phonetics, distance, classification, TF-IDF ingestion, and more, including the
real measured crossover numbers reported for two of them — before writing
anything below by hand.

## Rolling your own

Nothing below is obsolete. It's what you still reach for whenever no built-in
fits:

- an operation the audit evaluated and explicitly rejected — `verbora-trie`
  (query cost ~67 ns, at or below `rayon`'s own dispatch overhead),
  `verbora-inflectors` (~360 ns/word, the same overhead problem), or
  `verbora-util` (its graph algorithms operate on one shared graph per call,
  not independent items — there is no batch shape to parallelize);
- `verbora-ngrams`, not yet evaluated for a `par_*` API either way;
- a computation none of the thirteen wraps — a derived value instead of the
  wrapped function's own return type (a *count*, not the tokens themselves,
  exactly the example below), a reduction into a shared structure, a
  multi-stage pipeline, or a shared read-only index built from a crate with no
  batch primitive at all.

This is possible precisely because Verbora's operations — including the ones
with no built-in `par_*` API — are stateless and `Send + Sync`.

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
use verbora_tokenizers::{AggressiveTokenizer, Tokenize};
use rayon::prelude::*;

fn token_counts(corpus: &[String]) -> Vec<usize> {
    let tokenizer = AggressiveTokenizer::new();   // zero-sized: shared freely

    corpus
        .par_iter()
        .map(|doc| tokenizer.tokens(doc).count())
        .collect()
}
```

`AggressiveTokenizer` is a zero-sized type, so sharing it across threads costs
nothing and requires no synchronisation.

## Per-worker buffers

A `&mut Vec` cannot be shared, so buffer reuse and parallelism combine through
`map_init`, which gives each worker its own:

```rust  ignore
use verbora_tokenizers::{AggressiveTokenizer, Tokenize};
use rayon::prelude::*;

fn token_counts(corpus: &[String]) -> Vec<usize> {
    let tokenizer = AggressiveTokenizer::new();

    corpus
        .par_iter()
        .map_init(Vec::new, |buf, doc| {
            buf.clear();
            tokenizer.tokenize_into(doc, buf);
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
use verbora_tokenizers::{AggressiveTokenizer, Tokenize};
use rayon::prelude::*;

fn total_tokens(corpus: &[String]) -> usize {
    let tokenizer = AggressiveTokenizer::new();

    corpus
        .par_chunks(1024)                       // one task per 1024 documents
        .map(|chunk| {
            let mut buf = Vec::new();           // one buffer per chunk
            let mut local = 0;

            for doc in chunk {
                buf.clear();
                tokenizer.tokenize_into(doc, &mut buf);
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
once. **Construction cannot be parallelised** — `add_string` takes `&mut self`.
Build it on one thread, then share it.

## Parallel reduction into a shared map

```rust  ignore
use std::collections::HashMap;

use verbora_tokenizers::{AggressiveTokenizer, Tokenize};
use rayon::prelude::*;

fn term_frequencies(corpus: &[String]) -> HashMap<String, usize> {
    let tokenizer = AggressiveTokenizer::new();

    corpus
        .par_chunks(512)
        .map(|chunk| {
            // Each task builds its own map — no contention at all.
            let mut local: HashMap<String, usize> = HashMap::new();
            for doc in chunk {
                for token in tokenizer.tokens(doc) {
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

## The two things you must not share

<div class="callout callout-warn">
<strong>Process-global state.</strong> Two APIs read a process-wide mutable
binding. Do not mutate either from a worker, and prefer the explicit sibling in
all concurrent code:
<ul>
<li><code>verbora_ngrams::set_tokenizer</code> → use
<code>ngrams_str_with(…, &amp;tokenizer)</code></li>
<li><code>verbora_core::stopwords</code>'s global list → use
<code>phoneticize_tokens_with(…, &amp;stops, …)</code></li>
</ul>
Both globals exist to reproduce the reference's process-wide behaviour, and both are
read by the convenience entry points.
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

## When to stop

- The stage takes less than a second of total CPU — you would be measuring the
  scheduler.
- You are already running one request per thread — intra-request parallelism
  there usually reduces total throughput.
- The scaling curve is flat past two threads — find the real bottleneck.

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
