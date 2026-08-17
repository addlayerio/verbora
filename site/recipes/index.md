# Recipes by workload

The [features](../features/index.md) section is organised by what a thing *is*.
This section is organised by what you are *doing*, because the right API usually
follows from the workload rather than from the operation.

## Four workloads

<div class="cards">

<a class="card" href="interactive">
<span class="card-title">Interactive request/response →</span>
<span class="card-desc">One input, an answer now. Priorities: latency, predictability, code you can read at 3 a.m.</span>
</a>

<a class="card" href="streaming">
<span class="card-title">Streaming →</span>
<span class="card-desc">Input larger than memory, or output needed before input ends. Priorities: bounded memory, lazy processing, early output.</span>
</a>

<a class="card" href="batch">
<span class="card-title">Batch corpora →</span>
<span class="card-desc">Many documents, offline, throughput is the metric. Priorities: memory reuse, shared setup, allocation removal.</span>
</a>

<a class="card" href="parallel-corpus">
<span class="card-title">Massive parallel corpora →</span>
<span class="card-desc">Enough work to be worth threads. Priorities: CPU utilisation, chunk sizing, per-worker state.</span>
</a>

</div>

## The workload decides the API

The same operation, four times:

| Workload | Tokenizer call | Why |
|---|---|---|
| Interactive | `tokenize()` | One allocation, invisible next to the network |
| Streaming | `tokens()` | One token resident at a time; can stop early |
| Batch | `tokenize_into()` | One buffer, reused across the corpus |
| Parallel | `map_init(Vec::new, …)` + `tokenize_into()` | A buffer per worker; `&mut` cannot be shared |

Notice that none of the rows is "the fast one". They are answers to different
questions. See [Choosing the right API](../choosing/index.md).

## Problem recipes

Complete programs for common tasks:

<div class="cards">

<a class="card" href="fuzzy-matching">
<span class="card-title">Fuzzy name matching →</span>
<span class="card-desc">Phonetic bucketing to cut the candidate set, then edit distance to rank. The order matters more than the metric.</span>
</a>

<a class="card" href="autocomplete">
<span class="card-title">Prefix autocomplete →</span>
<span class="card-desc">A trie, a lazy prefix iterator, and why <code>keys_with_prefix</code> is the wrong call when you only show ten suggestions.</span>
</a>

</div>

## The priority list, by workload

**Interactive.** Latency, ergonomics, predictability. Do not optimise here
without a profile — a request handler that allocates a `Vec` per request is not
your bottleneck, and the readable version is worth more than the saved
allocation.

**Streaming.** Bounded memory, lazy processing, early output. The reason to
stream is usually that you cannot afford not to. Use `tokens()`,
`ngrams_iter()`, `iter_keys_with_prefix()`, and avoid anything that collects.

**Batch.** Throughput, memory reuse, shared setup. Hoist construction, reuse one
buffer, pre-size outputs, and remove per-item allocations. Measure before and
after: this is the workload where those changes actually show up.

**Massive parallel.** CPU utilisation, batch sizing, cache behaviour. Optimise
the sequential version first — a `tokenize_into` loop that removed ten million
allocations may make the parallel version unnecessary, and it composes with it if
not. Then chunk so each task is large enough to dwarf scheduling overhead, and
measure the scaling curve rather than a single point.

## A note on premature optimisation

Every recipe in this section that uses a performance-oriented API says what it
costs you in code complexity, because that is a real cost. If you are not in the
workload the recipe is written for, take the simpler version from
[Your first program](../getting-started/first-program.md) instead.
