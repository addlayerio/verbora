# Parallelism

Thirteen crates ship an optional, feature-gated `par_*` batch API. For
everything else, Verbora's types are built so that parallelising at your own
call site is a two-line change.

## The built-in APIs

Enable the `parallel` feature on the crate you need it from:

```toml
[dependencies]
verbora-tokenizers = { version = "0.1", features = ["parallel"] }
```

`parallel` is never on by default. It pulls in `rayon` as an optional dependency
and adds one or more `par_*` functions, each a thin fan-out over the crate's
existing sequential primitive:

| Crate | API | Granularity |
|---|---|---|
| [`verbora-spellcheck`](../features/index) | `par_get_corrections_batch` | per word |
| [`verbora-wordnet`](../features/wordnet) | `par_lookup_batch` | per word |
| [`verbora-tagger`](../features/index) | `par_tag_batch` | per document |
| [`verbora-distance`](../features/distance) | `par_levenshtein_batch` and siblings, one per metric | per pair |
| [`verbora-tokenizers`](../features/tokenizers) | `par_tokenize_batch` | per document |
| [`verbora-normalizers`](../features/normalizers) | `par_remove_diacritics_batch` | per document |
| [`verbora-sentiment`](../features/sentiment) | `par_get_sentiment_batch` | per document |
| [`verbora-analyzers`](../features/index) | `par_analyze_batch` | per sentence |
| [`verbora-transliterators`](../features/transliterators) | `par_transliterate_batch` | per document |
| [`verbora-stemmers`](../features/index) | `par_tokenize_and_stem_batch` | **per document, not per word** — per-word cost is as low as ~26 ns, far below task-dispatch overhead |
| [`verbora-phonetics`](../features/phonetics) | `par_encode_batch` / `par_encode_double_batch` | **chunked** (`par_chunks`) — same overhead problem at ~42–183 ns/word |
| [`verbora-classifiers`](../features/classifiers) | `par_classify_batch` on `Classifier<E>` | per document — `MaxEntClassifier` is excluded, its `Rc<RefCell<_>>` state is load-bearing |
| [`verbora-tfidf`](../features/tfidf) | `par_add_documents_batch` | per document, split phase — see below |

Two guarantees hold for all of them: **output is identical to the sequential
call** (each is a fan-out over the primitive you would have called in a loop,
not a second implementation), and **nothing runs in parallel unless you ask** —
the feature is opt-in and the functions are called by name.

<div class="callout callout-note">
<strong><code>verbora-tfidf</code> is the one split case.</strong>
<code>add_document</code> takes <code>&amp;mut self</code> and mutates the
interner, the incremental document-frequency table and the idf cache, so
<code>par_add_documents_batch</code> runs the stateless phase (tokenizing) in
parallel and replays the stateful phase (interning, stop-word filtering, the idf
update) sequentially, in the same order. The result is byte-for-byte identical
to the sequential loop, and the sequential phase is a real, un-parallelised
fraction of the total — which is why its speedups below are modest.
</div>

## Where there is deliberately no `par_*` API

- **`verbora-trie`** — a query costs ~67 ns, at or below task-dispatch
  overhead; construction is inherently sequential against one shared arena
  (`add_string` takes `&mut self`).
- **`verbora-inflectors`** — ~360 ns/word, the same overhead problem.
- **`verbora-util`** — its graph algorithms operate on one shared graph per
  call, not independent items; there is no batch shape to parallelize.

`verbora-ngrams` has not been quantified for Rayon — read its absence as "not
yet evaluated" rather than "evaluated and rejected".

For anything else, parallelising at your call site is easy because Verbora's
types are already built for it: tokenizers, phonetic encoders, inflectors and
distance functions are stateless values (most are zero-sized types), the
distance and normalizer entry points are free functions, `Trie` is
`Send + Sync`, and nothing has interior mutability on a hot path.

## The exception you must know about

<div class="callout callout-warn">
<strong>Two APIs touch process-global mutable state.</strong> Both are stored
behind a <code>RwLock</code> plus an <code>AtomicBool</code>, so neither is a
memory-safety hazard — no data race, no undefined behaviour, whatever you do
concurrently. The hazard is <em>correctness</em>: a thread calling
<code>set_tokenizer</code> or <code>add_global_stopword</code> while others are
reading gives those readers a nondeterministic mix of the old and new value,
with no error to tell you.
<ul>
<li><code>verbora_ngrams::set_tokenizer</code> / <code>reset_tokenizer</code> —
rebinds the tokenizer every <code>*_str</code> entry point reads. Use
<code>ngrams_str_with</code>, which takes the tokenizer explicitly.</li>
<li><code>verbora_core::stopwords</code>'s global list
(<code>add_global_stopword</code>, <code>remove_global_stopword</code>,
<code>reset_global_stopwords</code>) — read by
<code>phoneticize_tokens</code>. Use <code>phoneticize_tokens_with</code>, which
takes a <code>&amp;StopWords</code>.</li>
</ul>
The same applies to <code>verbora_tfidf</code>'s own tokenizer and stop-word
globals (<a href="../features/tfidf#the-process-global-tokenizer-and-stop-word-list">documented on its
page</a>). In concurrent code, always use the explicit-argument sibling.
</div>

## Doing it yourself

For anything without a built-in API, add `rayon` to *your* `Cargo.toml`:

```toml
[dependencies]
rayon = "1"
```

### Independent documents

```rust  ignore
use verbora_tokenizers::{AggressiveTokenizer, Tokenize};
use rayon::prelude::*;

let counts: Vec<usize> = corpus
    .par_iter()
    .map(|doc| {
        let tokenizer = AggressiveTokenizer::new();   // zero-sized: free
        tokenizer.tokens(doc).count()
    })
    .collect();
```

### With a per-thread buffer

`map_init` gives each worker its own scratch, which is how you combine
[buffer reuse](buffer-reuse.md) with parallelism — a `&mut Vec` cannot be shared:

```rust  ignore
use verbora_tokenizers::{AggressiveTokenizer, Tokenize};
use rayon::prelude::*;

let tokenizer = AggressiveTokenizer::new();

let counts: Vec<usize> = corpus
    .par_iter()
    .map_init(Vec::new, |buf, doc| {
        buf.clear();
        tokenizer.tokenize_into(doc, buf);
        buf.len()
    })
    .collect();
```

### A shared read-only index

```rust  ignore
use verbora_trie::Trie;
use rayon::prelude::*;
use std::sync::Arc;

let trie = Arc::new(build_trie());          // built once, sequentially

let hits: Vec<bool> = queries
    .par_iter()
    .map(|q| trie.contains(q))              // &self — no locking needed
    .collect();
```

`Trie` construction cannot be parallelised — `add_string` takes `&mut self`.
Build on one thread, then share.

### Chunking to control granularity

Per-item tasks that take a few hundred nanoseconds are dominated by scheduling
overhead. Give each task more work:

```rust  ignore
use rayon::prelude::*;

let total: usize = corpus
    .par_chunks(1024)                       // one task per 1024 documents
    .map(|chunk| chunk.iter().map(|d| tokenizer.tokens(d).count()).sum::<usize>())
    .sum();
```

## When parallelism actually helps

> Parallel does not automatically mean faster.

The crossover points for the two built-in APIs with the widest measured range:

| API | Sequential | Parallel | Speedup | At |
|---|--:|--:|--:|---|
| `verbora-spellcheck::par_get_corrections_batch` | 104.2 ms | 57.2 ms | ~1.8× (high variance — near the crossover) | batch of 8 |
| `verbora-spellcheck::par_get_corrections_batch` | 1.57 s | 168.6 ms | ~9.3× | batch of 64 |
| `verbora-spellcheck::par_get_corrections_batch` | 8.22 s | 865.3 ms | ~9.5× | batch of 512 |
| `verbora-tfidf::par_add_documents_batch` | 3.46 ms | 3.69 ms | ~7% *slower* | 128 small documents |
| `verbora-tfidf::par_add_documents_batch` | 25.6 ms | 23.5 ms | ~8% faster | 1,024 small documents |
| `verbora-tfidf::par_add_documents_batch` | 211.6 ms | 183.2 ms | ~13% faster | 8,192 small documents |

Spellcheck is the ideal shape — each item is a millisecond of independent work.
TF-IDF is Amdahl-limited by its sequential replay phase, and below ~1,000
documents the fan-out does not even pay for itself.

Four checks before you parallelise anything yourself:

**Compare the per-item cost to the scheduling cost.** A `rayon` task costs on
the order of a microsecond to schedule. Verbora's fastest stemmer processes a
word in ~26 ns — three orders of magnitude below that floor.

**Check whether you are memory-bound.** Tokenization is a linear scan that
allocates little. Sixteen cores scanning sixteen documents can saturate memory
bandwidth long before they saturate the ALUs. Distance calculations on longer
inputs, which do real arithmetic per cell, scale better.

**Check whether the work is already small.** `hamming/4` is 6.6 ns. No amount of
threading makes a 6.6 ns operation faster; you would be measuring the scheduler.

**Check what else is running.** In a web server every request already occupies a
thread. Adding intra-request parallelism there usually *reduces* total
throughput by oversubscribing the CPU, even when it improves one request's
latency.

If the operation is in the table above, enable the `parallel` feature and call
it — its doc comment states the measured crossover. Otherwise, leave it
sequential unless total CPU time in the stage is measured in seconds, the items
are independent, and you are not already running one request per thread. Then
`par_chunks` with a chunk size that makes each task ≥ ~100 µs, and measure.

## What to measure

1. **Wall-clock, not CPU time.** Parallel code that halves latency while
   quadrupling CPU time is a bad trade in a shared environment.
2. **Scaling curve, not a single point.** Run at 1, 2, 4, 8, 16 threads. A flat
   curve past 4 means you are bound by something other than the CPU.
3. **The sequential version, optimised first.** A `tokenize_into` loop that
   removed ten million allocations may make the parallel version unnecessary. It
   is cheaper, and it composes.

## Related

- [Batch vs streaming](batch-vs-streaming.md) — you need a batch before you can
  split it.
- [Buffer reuse](buffer-reuse.md) — `map_init` is how the two combine.
- [Massive parallel corpora](../recipes/parallel-corpus.md) — a complete worked
  example.
- [Benchmarks](../benchmarks/index) — how to reproduce every number on this page.
