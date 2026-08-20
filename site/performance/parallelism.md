# Parallelism

Fourteen crates ship an optional, feature-gated `par_*` batch API. For
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
| [`verbora-spellcheck`](../features/spellcheck) | `Spellcheck::par_corrections_batch` | per word |
| [`verbora-wordnet`](../features/wordnet) | `WordNet::par_lookup_batch` | per word |
| [`verbora-tagger`](../features/tagger) | `BrillTagger::par_tag_batch` | per document |
| [`verbora-distance`](../features/distance) | `par_levenshtein_batch` and siblings, one per metric | per pair |
| [`verbora-tokenizers`](../features/tokenizers) | `par_tokenize_batch` | per document |
| [`verbora-normalizers`](../features/normalizers) | `par_remove_diacritics_batch` | per document |
| [`verbora-sentiment`](../features/sentiment) | `SentimentAnalyzer::par_get_sentiment_batch` | per document |
| [`verbora-analyzers`](../features/analyzers) | `par_analyze_batch` | per sentence |
| [`verbora-transliterators`](../features/transliterators) | `par_transliterate_ja_batch` | per document |
| [`verbora-stemmers`](../features/stemmers) | `TokenizeAndStem::par_tokenize_and_stem_batch` | **per document, not per word** — per-word cost is as low as ~26 ns, far below task-dispatch overhead |
| [`verbora-phonetics`](../features/phonetics) | `par_encode_batch` / `par_encode_double_batch` | **chunked** (`par_chunks`) — same overhead problem at ~42–183 ns/word |
| [`verbora-classifiers`](../features/classifiers) | `par_classify_batch` on `Classifier<E>` | per document — `MaxEntClassifier` is excluded; it is already `Send + Sync`, but no `par_*` API ships here without sequential-vs-parallel benchmark evidence, and there is none yet for this model |
| [`verbora-tfidf`](../features/tfidf) | `TfIdf::par_add_documents` | per document, split phase — see below |
| [`verbora-language`](../features/language) | `par_detect_batch` | per text; generic over any `LanguageDetector + Sync` |

Two guarantees hold for all of them: **output is identical to the sequential
call** (each is a fan-out over the primitive you would have called in a loop,
not a second implementation), and **nothing runs in parallel unless you ask** —
the feature is opt-in and the functions are called by name.

<div class="callout callout-note">
<strong><code>verbora-tfidf</code> is the one split case.</strong>
<code>add_document</code> takes <code>&amp;mut self</code> and mutates the term
interner and the incremental document-frequency table, so
<code>par_add_documents</code> runs the stateless phase (tokenizing) in
parallel and replays the stateful phase (interning, stop-word filtering, the
document-frequency update) sequentially, in the same order. The result is
byte-for-byte identical to the sequential loop — down to the term-id assignment
order and the serialized bytes — and the sequential phase is a real,
un-parallelised fraction of the total, which bounds what fan-out can buy.
</div>

## Where there is deliberately no `par_*` API

- **`verbora-trie`** — a `contains` is one hash of the folded bytes and a short
  probe, at or below task-dispatch overhead; construction is inherently
  sequential against one shared arena (`insert` takes `&mut self`).
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
<strong>One list in the workspace is process-global and mutable.</strong>
<code>verbora_core</code>'s global stop-word list
(<code>add_global_stopword</code>, <code>remove_global_stopword</code>,
<code>reset_global_stopwords</code>, <code>is_global_stopword</code>) is stored
behind an <code>RwLock</code> plus an <code>AtomicBool</code>, so it is not a
memory-safety hazard — no data race, no undefined behaviour, whatever you do
concurrently. The hazard is <em>correctness</em>: a thread calling
<code>add_global_stopword</code> while others are reading gives those readers a
nondeterministic mix of the old and new value, with no error to tell you. Its
one consumer inside the workspace is <code>verbora-stemmers</code>' English and
Lancaster stop-word helpers.
<br><br>
Nothing else reads it. <code>phoneticize_tokens</code> takes an explicit
<code>&amp;StopWords</code> and has no global-reading variant; a
<code>TfIdf</code> owns its own <code>Analyzer</code> — its tokenizer, its case
folding and its stop-word list — so two corpora in one program cannot silently
change each other's answers. Prefer the owned or argument form everywhere; the
global list exists for programs that genuinely want one process-wide setting.
</div>

## Doing it yourself

For anything without a built-in API, add `rayon` to *your* `Cargo.toml`:

```toml
[dependencies]
rayon = "1"
```

### Independent documents

```rust  ignore
use rayon::prelude::*;
use verbora_tokenizers::{BorrowingTokenizer, WordTokenizer};

let counts: Vec<usize> = corpus
    .par_iter()
    .map(|doc| WordTokenizer.tokens(doc).count())   // zero-sized: free
    .collect();
```

### With a per-thread buffer

`map_init` gives each worker its own scratch, which is how you combine
[buffer reuse](buffer-reuse.md) with parallelism — a `&mut Vec` cannot be shared:

```rust  ignore
use rayon::prelude::*;
use verbora_tokenizers::{BorrowingTokenizer, WordTokenizer};

let counts: Vec<usize> = corpus
    .par_iter()
    .map_init(Vec::new, |buf, doc| {
        buf.clear();
        WordTokenizer.tokenize_borrowed_into(doc, buf);
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

`Trie` construction cannot be parallelised — `insert` takes `&mut self`. Build on
one thread, then share. If the index never changes afterwards, share the
`FrozenTrie` that `freeze()` returns instead: it is `Send + Sync` too, and its
`keys_slice` hands each worker a borrowed `&[String]` rather than a fresh `Vec`.

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

<div class="callout callout-warn">
<strong>The two crossover tables this section carried are withdrawn.</strong>
<code>verbora-spellcheck</code>'s correction contract and
<code>verbora-tfidf</code>'s ingestion path both changed underneath the figures
that described them, so neither set is current, and both crates now say so in
their own documentation rather than carrying a number forward. Which batch size
each one crosses over at must come from a re-run, not from the direction of the
change. <code>spellcheck_par_corrections_batch_d2</code> in
<code>benches/spellcheck.rs</code> and <code>parallel_batch</code> in
<code>benches/tfidf.rs</code> compare the sequential loop against each parallel
method across several batch sizes, and are what the next campaign should answer
this with.
</div>

What can be said without a measurement is the shape of each trade, which is a
property of the code rather than of a run:

- **Spellcheck is the favourable shape.** Each correction is independent work of
  its own, with no shared mutable state, so `par_corrections_batch` is a plain
  fan-out and the only question is whether the batch is big enough to amortize
  fork-join.
- **TF-IDF is Amdahl-limited by construction.** `par_add_documents` parallelises
  tokenizing and replays interning and counting sequentially, in order, so the
  sequential fraction is real and bounded below by the corpus update itself. It
  also allocates one `String` per term, which `add_document` does not — its terms
  are borrowed from the text. A handful of short documents will be slower this
  way.

Four checks before you parallelise anything yourself:

**Compare the per-item cost to the scheduling cost.** A `rayon` task costs on
the order of a microsecond to schedule. Verbora's fastest stemmer processes a
word in ~26 ns — three orders of magnitude below that floor.

**Check whether you are memory-bound.** Tokenization is a linear scan that
allocates little. Sixteen cores scanning sixteen documents can saturate memory
bandwidth long before they saturate the ALUs. Distance calculations on longer
inputs, which do real arithmetic per cell, scale better.

**Check whether the work is already small.** `hamming/4` is 6.6 ns † — pending
re-measurement, but orders of magnitude below a task's scheduling cost either
way. No amount of threading makes an operation that small faster; you would be
measuring the scheduler.

**Check what else is running.** In a web server every request already occupies a
thread. Adding intra-request parallelism there usually *reduces* total
throughput by oversubscribing the CPU, even when it improves one request's
latency.

If the operation has a built-in `par_*` API, enable the `parallel` feature and
call it — each one's doc comment states what it costs and, where a crossover has
been measured, where it sits. Otherwise, leave it sequential unless total CPU
time in the stage is measured in seconds, the items are independent, and you are
not already running one request per thread. Then `par_chunks` with a chunk size
that makes each task ≥ ~100 µs, and measure.

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
