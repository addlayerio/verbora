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

| Workload | Tokenizer call | What you optimise | Watch out for |
|---|---|---|---|
| Interactive | `tokenize()` | Latency and readability | Doing `O(n)` work over a whole corpus per request |
| Streaming | `tokens()` | Peak memory, time to first result | Any `collect()` in the middle of the pipeline |
| Batch | `tokenize_into()` | Allocations per document | Forgetting `buf.clear()` at the top of the loop |
| Parallel | `map_init(Vec::new, …)` + `tokenize_into()` | CPU utilisation | Tasks too small to pay for scheduling |

None of those rows is "the fast one" — they are answers to different questions.
See [Choosing the right API](../choosing/index.md).

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

## Before you optimise

Every recipe here that uses a performance-oriented API also says what it costs
you in code complexity, because that is a real cost. Two rules of thumb:

- **Optimise the sequential version first.** A `tokenize_into` loop that removes
  ten million allocations may make threads unnecessary — and it composes with
  them if not.
- **Measure before and after.** Batch is the one workload where these techniques
  reliably show up. If the numbers do not move, put the simple version back.

If you are not in the workload a recipe was written for, take the simpler
version from [Your first program](../getting-started/first-program.md) instead.
