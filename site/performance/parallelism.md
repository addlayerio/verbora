# Parallelism

<div class="callout callout-note">
<strong>Thirteen crates ship an optional, feature-gated <code>par_*</code> batch
API.</strong> Each exists because a real benchmark showed a real win for a
real, common workload — not because parallelism is generically available.
Every other operation in the workspace has no built-in parallel API, and this
page also covers doing it yourself for those, exactly as it always has.
</div>

## The thirteen built-in APIs

Add the `parallel` feature to the crate you need it from:

```toml
[dependencies]
verbora-tokenizers = { version = "0.1", features = ["parallel"] }
```

`parallel` is never on by default. It pulls in `rayon` as an optional
dependency (`rayon = { workspace = true, optional = true }`, gated
`parallel = ["dep:rayon"]` — the same convention `verbora-core`'s `serde`
feature already established) and adds one or more `par_*` functions, each a
thin fan-out over the crate's existing sequential primitive:

| Crate | API | Granularity | Why |
|---|---|---|---|
| [`verbora-spellcheck`](../features/index) | `par_get_corrections_batch` | per word | The strongest candidate found — one correction search is milliseconds, ~9× faster at 64 words in this project's own measurement. |
| [`verbora-wordnet`](../features/wordnet) | `par_lookup_batch` | per word | Already immutable-after-construction and `Send + Sync` with no per-query state; a shared dictionary queried from every thread at once needed no new synchronization. |
| [`verbora-tagger`](../features/index) | `par_tag_batch` | per document | Hundreds of µs to ~1 ms per document, near-linear in token count, fully independent documents against one shared lexicon and rule set. |
| [`verbora-distance`](../features/distance) | `par_levenshtein_batch` and siblings, one per metric | per pair | Free functions with no state; cost ranges from nanoseconds to milliseconds depending on string length, so the win scales with what you're comparing. |
| [`verbora-tokenizers`](../features/tokenizers) | `par_tokenize_batch` | per document | A default method on `Tokenize`, so every implementor gets it for free; each task still runs the existing sequential `tokenize()` once. |
| [`verbora-normalizers`](../features/normalizers) | `par_remove_diacritics_batch` | per document | Sub-microsecond to tens of microseconds per call — the win is corpus scale, not per-call latency. |
| [`verbora-sentiment`](../features/sentiment) | `par_get_sentiment_batch` | per document | The analyzer is immutable after construction and `Sync` whenever its stemmer type parameter is. |
| [`verbora-analyzers`](../features/index) | `par_analyze_batch` | per sentence | Composes the crate's existing per-sentence calls in the same order the sequential path already uses — there was no single method to wrap directly. |
| [`verbora-transliterators`](../features/transliterators) | `par_transliterate_batch` | per document | Tens of µs per realistically-sized document, same profile as normalizers. |
| [`verbora-stemmers`](../features/index) | `par_tokenize_and_stem_batch` | **per document, not per word** | Per-word cost measured as low as ~26 ns (Lancaster) — below typical `rayon` task-dispatch overhead. Parallelizing per word would likely *lose*; per document does not. |
| [`verbora-phonetics`](../features/phonetics) | `par_encode_batch` / `par_encode_double_batch` | **chunked** (`par_chunks`), not per word | Same overhead problem as stemmers (~42–183 ns/word) — explicit chunking, not one task per word, is what makes this a real win when building an index over a large dictionary. |
| [`verbora-classifiers`](../features/classifiers) | `par_classify_batch` on `Classifier<E>` | per document | `Classifier<E>`'s stemmer field moved from `Rc<dyn Stemmer>` to `Arc<dyn Stemmer + Send + Sync>` to make this possible — verified as a pure representation change, zero behavior change. `MaxEntClassifier` is deliberately excluded: its `Rc<RefCell<_>>` state is load-bearing, not incidental. |
| [`verbora-tfidf`](../features/tfidf) | `par_add_documents_batch` | per document, narrower scope | `add_document` mutates shared corpus state (the interner, the incremental document-frequency table, the idf cache), so this isn't a plain wrapper — see below. |

<div class="callout callout-warn">
<strong>Every one of these is a thin wrapper over the existing sequential
primitive, never a second implementation.</strong> This is a permanent rule
(<code>AGENTS.md</code>'s "Rayon Policy"): if adding parallelism required
writing new algorithmic logic, that was treated as a sign the design was
wrong, not a reason to duplicate the logic. <code>verbora-tfidf</code> is the
one case that could not be a literal wrapper — <code>add_document</code>
takes <code>&amp;mut self</code> and mutates state a naive
<code>par_iter().for_each</code> cannot compile against, and a
<code>Mutex</code>-wrapped corpus would just serialise the real work back.
The shipped design splits <code>add_document</code>'s own existing steps into
a parallel, stateless phase (tokenizing) and a sequential replay phase
(interning, stop-word filtering, the idf cache update) — still zero new
algorithmic logic, just reordered, and verified byte-for-byte identical to
the sequential loop by <a href="../features/tfidf#the-process-global-tokenizer-and-stop-word-list">its own behaviour
suite</a>. A map-reduce design (build N partial corpora, merge) was
considered and rejected: it would need a new interner-merge algorithm with no
analogue in the sequential code, which is exactly the "second implementation"
the rule above exists to prevent.
</div>

## Why not everything

Three crates were evaluated and deliberately do not have a `par_*` API:

- **`verbora-trie`** — query cost measured at ~67 ns, at or below typical
  `rayon` dispatch overhead; construction is inherently sequential against
  one shared arena (`add_string` takes `&mut self`).
- **`verbora-inflectors`** — ~360 ns/word, the same overhead problem.
- **`verbora-util`** — its graph algorithms (`EdgeWeightedDigraph`,
  `Topological`, path trees) operate on one shared graph per call, not
  independent items; there is no batch shape to parallelize.

`verbora-ngrams` has not been separately quantified for Rayon — treat its
absence here as "not yet evaluated," not "evaluated and rejected."

For everything else — any crate above without a `par_*` API, or any workload
those APIs don't fit — the reasoning that follows still applies. This
section is unchanged from before the thirteen APIs existed:

**The library does not know your workload.** A parallel API has to pick a
chunk size, decide whether to build a thread pool, and decide what to do when
it is called from inside someone else's pool. Those decisions belong to the
application by default. The thirteen exceptions above exist because the
answer was unambiguous enough, and measured, to ship as a default.

**Verbora's APIs are already trivially parallelisable.** Everything you need
is already true:

- Tokenizers, phonetic encoders, inflectors and distance functions are
  stateless values — most are zero-sized types. Constructing one per thread
  is free.
- The distance and normalizer functions are free functions with no state at
  all.
- `Trie` is `Send + Sync`, so an `Arc<Trie>` can be queried from every thread
  at once.
- Nothing has interior mutability on a hot path.

So the parallel version of anything not in the table above is a two-line
change at your call site, and you keep control of it.

## The exception you must know about

<div class="callout callout-warn">
<strong>Two APIs touch process-global mutable state.</strong> Both are stored
behind a <code>RwLock</code> plus an <code>AtomicBool</code> (see
<a href="../features/core">Core vocabulary</a>), so neither one is a memory-
safety hazard — no data race, no undefined behaviour, whatever you do with them
concurrently. The hazard is <em>correctness</em>: a thread calling
<code>set_tokenizer</code> or <code>add_global_stopword</code> while other
threads are reading gives those readers a nondeterministic mix of the old and
new value, with no error to tell you it happened.
<ul>
<li><code>verbora_ngrams::set_tokenizer</code> / <code>reset_tokenizer</code> —
rebinds the tokenizer that every <code>*_str</code> entry point reads. Use
<code>ngrams_str_with</code>, which takes the tokenizer explicitly.</li>
<li><code>verbora_core::stopwords</code>'s global list
(<code>add_global_stopword</code>, <code>remove_global_stopword</code>,
<code>reset_global_stopwords</code>) — read by
<code>phoneticize_tokens</code>. Use <code>phoneticize_tokens_with</code>, which
takes a <code>&amp;StopWords</code>.</li>
</ul>
This also applies to <code>verbora_tfidf</code>'s own tokenizer/stop-word
globals (<a href="../features/tfidf#the-process-global-tokenizer-and-stop-word-list">documented on its own
page</a>) and is exactly why <code>par_add_documents_batch</code>'s parallel
phase only ever calls pure, global-state-free functions — the stateful
global reads happen in the sequential replay phase, on one thread, same as
the ordinary sequential loop.
Both globals model process-wide default configuration, set once and read from
many call sites. Both have an explicit-argument sibling, and in concurrent code you should use the sibling —
not because the global will crash you, but because its result would depend on
timing you do not control.
</div>

## Doing it yourself

For anything not in the table above, add `rayon` to *your* `Cargo.toml`:

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

Note that `Trie` construction cannot be parallelised: `add_string` takes
`&mut self`. Build the trie on one thread, then share it. This is exactly why
`verbora-trie` has no `par_*` API of its own — the query side is too cheap
per call to be worth it (~67 ns, measured), and the build side can't be
parallelised without changing the data structure.

### Chunking to control granularity

Per-item tasks that take a few hundred nanoseconds are dominated by scheduling
overhead. Give each task more work — this is exactly what
`verbora-phonetics::par_encode_batch` does internally, with a chunk size
tuned and measured rather than guessed:

```rust  ignore
use rayon::prelude::*;

let total: usize = corpus
    .par_chunks(1024)                       // one task per 1024 documents
    .map(|chunk| chunk.iter().map(|d| tokenizer.tokens(d).count()).sum::<usize>())
    .sum();
```

## When parallelism actually helps

> Parallel does not automatically mean faster.

Real crossover numbers exist for the thirteen built-in APIs, measured rather
than estimated:

| API | Sequential | Parallel | Speedup | At |
|---|---|---|---|---|
| `verbora-spellcheck::par_get_corrections_batch` | 104.2 ms | 57.2 ms | ~1.8× (high variance — near the crossover) | batch of 8 |
| `verbora-spellcheck::par_get_corrections_batch` | 1.57 s | 168.6 ms | ~9.3× | batch of 64 |
| `verbora-spellcheck::par_get_corrections_batch` | 8.22 s | 865.3 ms | ~9.5× | batch of 512 |
| `verbora-tfidf::par_add_documents_batch` | 3.46 ms | 3.69 ms | ~7% *slower* | 128 small documents — not yet amortized |
| `verbora-tfidf::par_add_documents_batch` | 25.6 ms | 23.5 ms | ~8% faster | 1,024 small documents |
| `verbora-tfidf::par_add_documents_batch` | 211.6 ms | 183.2 ms | ~13% faster | 8,192 small documents |

The `spellcheck` and `tfidf` rows are deliberately shown side by side: one is
the strongest candidate found in the whole audit (each item is a millisecond
of real, independent work), the other is the modest, Amdahl's-law-limited
case (the sequential replay phase inside `par_add_documents_batch` is a real,
un-parallelised fraction of the total). Both are correct, honestly reported
outcomes — see [Benchmarks](../benchmarks/index) for how to reproduce them
and `docs/PERFORMANCE_MATRIX.md` for every crate's own numbers.

The reasoning that produced the granularity choices above, for your own
workloads:

**Compare the per-item cost to the scheduling cost.** A `rayon` task costs on
the order of a microsecond to schedule. `verbora-stemmers`' fastest stemmer
(Lancaster) processes a word in ~26 ns — three orders of magnitude below
that floor, which is exactly why its parallel API batches at the document
level, not the word level, and why `verbora-phonetics` uses explicit
`par_chunks` rather than one task per word.

**Check whether you are memory-bound.** Tokenization is a linear scan that
allocates little. Sixteen cores scanning sixteen documents can saturate memory
bandwidth long before they saturate the ALUs, at which point extra threads add
nothing. Distance calculations on longer inputs — which do real arithmetic per
cell — scale better.

**Check whether the work is already small.** The published `hamming/4` benchmark
is 6.6 ns. There is no amount of threading that makes a 6.6 ns operation faster;
you would be measuring the scheduler. This is exactly why `verbora-trie` and
`verbora-inflectors` have no `par_*` API.

**Check what else is running.** In a web server every request already occupies a
thread. Adding intra-request parallelism there usually *reduces* total throughput
by oversubscribing the CPU, even when it improves the latency of one request.
This is why every `par_*` API in the table above is opt-in via a Cargo
feature and never runs unless you call it by name — nothing in this
workspace starts consuming multiple cores just because `rayon` is somewhere
in your dependency graph.

A rough decision:

```text
Is there already a par_* API for this operation? (see the table above)
│
├── Yes ─────────────────────────────────▶ enable the "parallel" feature and
│                                          call it — its own doc comment
│                                          states the measured crossover
└── No
     │
     ├── Is total CPU time in this stage measured in seconds or more?
     │      └── No ─────────────────────▶ don't parallelise; you would be
     │                                     measuring the thread pool
     │
     ├── Am I already running one request per thread?
     │      └── Yes ─────────────────────▶ leave it sequential; the parallelism
     │                                      is at the request level
     │
     ├── Are the items independent?
     │      └── No ──────────────────────▶ restructure first
     │
     └── Yes ────────────────────────────▶ par_chunks with a chunk size that
                                            makes each task ≥ ~100 µs, then MEASURE
```

## What to measure

If you parallelise, verify these three, in order:

1. **Wall-clock, not CPU time.** Parallel code that halves latency while
   quadrupling CPU time is a bad trade in a shared environment.
2. **Scaling curve, not a single point.** Run at 1, 2, 4, 8, 16 threads. A flat
   curve past 4 means you are bound by something other than the CPU.
3. **The sequential version, optimised first.** A `tokenize_into` loop that
   removed ten million allocations may make the parallel version unnecessary. Do
   that first — it is cheaper and it composes.

This is the same discipline the thirteen built-in APIs were held to:
`docs/PERFORMANCE_MATRIX.md` records, per crate, whether allocation, data
structures and parallelism were reviewed, and `AGENTS.md`'s "Rayon Policy"
requires a sequential-vs-parallel equivalence test for every one of them —
asserting the two produce identical output over the same inputs, not just
that the parallel one is faster.

## Related

- [Batch vs streaming](batch-vs-streaming.md) — you need a batch before you can
  split it.
- [Buffer reuse](buffer-reuse.md) — `map_init` is how the two combine.
- [Massive parallel corpora](../recipes/parallel-corpus.md) — a complete worked
  example.
- [Benchmarks](../benchmarks/index) — how to reproduce every number on this page.
