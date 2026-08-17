# Why there is more than one API

Open `verbora-tokenizers` and you will find three ways to split a string:

```rust  ignore
tokenizer.tokenize(text)
tokenizer.tokens(text)
tokenizer.tokenize_into(text, &mut buffer)
```

A reasonable first reaction is suspicion. Three functions that do the same thing
usually means two of them are mistakes, or that the library could not decide.
Neither is true here, and this section exists so you never have to guess which
one you want.

## The editorial rule

This project holds itself to one rule about API surface, written into its
`AGENTS.md`:

> Whenever Verbora exposes more than one API for the same conceptual operation,
> the documentation **must** explain why each variant exists and when you should
> choose it. If a real difference cannot be explained, the second API should not
> exist.

So every group of similar-looking functions on this site comes with: what each
one does, when to use it, when *not* to, whether it allocates, whether it is
lazy, whether it can reuse memory, and — when the difference is a performance
difference — what evidence supports the recommendation.

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

## What Verbora does *not* have

Being explicit about absence is part of the same rule. As of this version:

<div class="callout callout-warn">
<strong>Most of Verbora's API is sequential-only by design.</strong> Thirteen
crates ship a curated, opt-in <code>par_*</code> batch function behind a
<code>parallel</code> Cargo feature — never on by default, never a second
implementation, each one added because a real benchmark showed a real win.
Everything else has no <code>par_*</code> function and no internal thread
pool; this site shows you how to write it at your own call site with your own
<code>rayon</code> dependency and explains when it actually pays. See
<a href="../performance/parallelism">Parallelism</a> for the full table.
</div>

- **Batch APIs are minimal.** `verbora_core::Tokenizer::tokenize_batch` and
  `verbora_core::Stemmer::stem_batch` exist as provided trait methods with
  sequential default bodies. No other crate has a batch entry point.
- **`_into` variants are rare.** Only tokenizers (`tokenize_into`,
  `tokenize_borrowed_into`), inflectors (`pluralize_into`, `singularize_into`),
  the `Stemmer` trait (`stem_into`) and `CaseMode::apply_into` have one. Distance,
  phonetics, normalizers and n-grams do not.
- **No scratch-buffer API.** There is no `levenshtein_with_scratch`. The
  Levenshtein family allocates its own working rows per call.

Where an absence is inconvenient, the relevant page shows the call-site
workaround rather than pretending an API exists.

## How to use this section

<div class="cards">

<a class="card" href="api-shapes">
<span class="card-title">The four API shapes →</span>
<span class="card-desc">The vocabulary: eager, lazy, into-buffer, batch. What each one costs, and the naming conventions that tell you which is which.</span>
</a>

<a class="card" href="tokenization">
<span class="card-title">Tokenization →</span>
<span class="card-desc">The canonical worked example. Pipeline diagrams, a full comparison table, a decision tree, and one example per variant.</span>
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
<span class="card-title">Every decision tree →</span>
<span class="card-desc">All the trees on one page, for when you know what you need and just want the answer.</span>
</a>

<a class="card" href="../performance/">
<span class="card-title">Performance guide →</span>
<span class="card-desc">The concepts underneath these choices — borrowing, laziness, <code>Cow</code>, buffer reuse, batching, parallelism.</span>
</a>

</div>

Subsystems with only one sensible API — phonetics, normalizers, inflectors,
tries — carry their "Choosing the right API" section on their own feature page
rather than here, because the choice there is usually *which type* rather than
*which call shape*.

## A worked example of the reasoning

Suppose you are writing a spell-check suggestion endpoint. Per request, you have
one misspelled word and a dictionary of 100,000 candidates, and you want the ten
closest by edit distance.

The naive read of this site's advice — "use the fast API" — would send you
looking for `levenshtein_batch`. It does not exist, and even if it did it would
not be the biggest win available. The actual reasoning goes:

1. **Cut the candidate set first.** 100,000 Levenshtein calls to return ten
   results is the wrong shape regardless of how fast each call is. A
   [`Trie`](../features/trie.md) prefix query or a
   [phonetic key](../features/phonetics.md) bucket reduces the candidates by
   orders of magnitude, and *that* is the optimisation that matters.
2. **Then pick the metric.** For typos, `levenshtein`; for names,
   `jaro_winkler`, which weights a common prefix. See
   [Choosing a distance metric](distance.md).
3. **Then worry about the call shape.** Hoist `Options` out of the loop, keep the
   input `&str`s ASCII where you can so the byte fast path applies, and only then
   reach for `verbora-distance`'s own `par_levenshtein_batch` (behind its
   `parallel` feature) or `rayon` at your call site.

Getting the *order* right is the point. This section is organised to make step 3
easy so you can spend your attention on steps 1 and 2 — see
[Recipes by workload](../recipes/index.md) for that half of the problem.
