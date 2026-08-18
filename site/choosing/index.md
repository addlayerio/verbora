# Why there is more than one API

`verbora-tokenizers` offers three ways to split a string:

```rust  ignore
tokenizer.tokenize(text)                     // a Vec you own
tokenizer.tokens(text)                       // an iterator
tokenizer.tokenize_into(text, &mut buffer)   // appended into memory you keep
```

They are not competing implementations. Each tokenizer has exactly one
implementation of its behaviour; these are three ways of moving its output to
you, and they differ only in who owns the memory. Same result, every time.

This section tells you which one to pick — here and everywhere else Verbora
offers a choice. Every group of similar-looking functions on this site comes
with the same information: what each one does, when to use it, when not to,
whether it allocates, whether it is lazy, and — when the difference is a
performance difference — what measurement supports the recommendation.

## The one thing to internalise

<div class="callout callout-good">
<strong>The simple API is not the bad API.</strong>
<code>tokenize()</code> is the right call for the overwhelming majority of
programs. The other shapes are not "the fast version" — they are answers to
questions <em>most code never asks</em>. Reaching for
<code>tokenize_into()</code> in a web handler that runs once per request buys you
nothing and costs you a mutable buffer to manage.
</div>

The variants exist because these workloads have genuinely different bottlenecks:

| Workload | Bottleneck | Shape that helps |
|---|---|---|
| One string, once | Nothing. Readability wins. | `tokenize()` |
| Feed tokens into a filter/map chain | Building an intermediate `Vec` you immediately consume | `tokens()` |
| Find the first token matching a predicate | Splitting the whole string when you needed a prefix of it | `tokens()` |
| 40M documents in a loop | One allocation per document | `tokenize_into()` |
| Documents don't fit in memory | Peak memory | `tokens()` |
| 16 idle cores | Wall clock | A crate's own `par_*_batch`, or `rayon` at your call site |

## Where to start

<div class="cards">

<a class="card" href="api-shapes">
<span class="card-title">The four API shapes →</span>
<span class="card-desc">The vocabulary: eager, lazy, into-buffer, batch. What each one costs, and the naming conventions that tell you which is which.</span>
</a>

<a class="card" href="tokenization">
<span class="card-title">Tokenization →</span>
<span class="card-desc">The canonical worked example: a full comparison table, a decision table, and one runnable example per variant.</span>
</a>

<a class="card" href="distance">
<span class="card-title">String distance →</span>
<span class="card-desc">Which metric for which problem; scalar vs search; free functions vs the <code>StringMetric</code> trait; what to do about bulk comparison.</span>
</a>

<a class="card" href="ngrams">
<span class="card-title">N-grams →</span>
<span class="card-desc">Lazy windows vs materialised vectors, borrowed vs owned, string input vs pre-tokenized input, with-stats vs without.</span>
</a>

<a class="card" href="decision-trees">
<span class="card-title">Quick answers →</span>
<span class="card-desc">Every choice on the site condensed into one page of tables, for when you know what you need and just want the answer.</span>
</a>

<a class="card" href="../performance/">
<span class="card-title">Performance guide →</span>
<span class="card-desc">The concepts underneath these choices — borrowing, laziness, <code>Cow</code>, buffer reuse, batching, parallelism.</span>
</a>

</div>

Subsystems with only one sensible API — phonetics, normalizers, inflectors,
tries — carry their "Choosing the right API" section on their own feature page,
because the choice there is usually *which type* rather than *which call shape*.

## What Verbora does not have

Knowing the absences saves you the search:

<div class="callout callout-warn">
<strong>Most of Verbora's API is sequential by design.</strong> Thirteen
crates ship a curated, opt-in <code>par_*_batch</code> function behind a
<code>parallel</code> Cargo feature — never on by default, never a second
implementation, each one added because a benchmark showed a real win.
Everything else has no <code>par_*</code> function and no internal thread
pool; this site shows you how to write it at your own call site with your own
<code>rayon</code> dependency and explains when it actually pays. See
<a href="../performance/parallelism">Parallelism</a> for the full table.
</div>

- **Batch APIs are minimal.** `verbora_core::Tokenizer::tokenize_batch` and
  `verbora_core::Stemmer::stem_batch` are provided trait methods with sequential
  default bodies. No other crate has a batch entry point.
- **`_into` variants are rare.** Only tokenizers (`tokenize_into`,
  `tokenize_borrowed_into`), inflectors (`pluralize_into`, `singularize_into`),
  the `Stemmer` trait (`stem_into`) and `CaseMode::apply_into` have one. Distance,
  phonetics, normalizers and n-grams do not.
- **No scratch-buffer API.** There is no `levenshtein_with_scratch`. The
  Levenshtein family builds its own working state per call.

Where an absence is inconvenient, the relevant page shows the call-site
workaround rather than pretending an API exists.

## Getting the order right

Suppose you are writing a spell-check suggestion endpoint: one misspelled word
per request, a dictionary of 100,000 candidates, and you want the ten closest by
edit distance. The instinct is to look for `levenshtein_batch`. It does not
exist — and it would not be the biggest win available anyway.

1. **Cut the candidate set first.** 100,000 Levenshtein calls to return ten
   results is the wrong shape regardless of how fast each call is. A
   [`Trie`](../features/trie.md) prefix query or a
   [phonetic key](../features/phonetics.md) bucket reduces the candidates by
   orders of magnitude, and *that* is the optimisation that matters.
2. **Then pick the metric.** For typos, `levenshtein`; for names,
   `jaro_winkler`, which weights a common prefix. See
   [Choosing a distance API](distance.md).
3. **Then pick the call shape.** Hoist `Options` out of the loop, keep inputs
   ASCII where you can so the byte fast path applies, and only then reach for
   `verbora-distance`'s own `par_levenshtein_batch` (behind its `parallel`
   feature) or `rayon` at your call site.

This section is organised to make step 3 easy, so you can spend your attention
on steps 1 and 2 — see [Recipes by workload](../recipes/index.md) for that half
of the problem.
