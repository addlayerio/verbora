---
aside: false
editLink: false
lastUpdated: false
prev: false
next: false
---

<div class="hero">

# Verbora

<p class="hero-tagline">The whole NLP stack, in one Rust toolkit.</p>

<p class="hero-sub">Tokenizing, normalizing, stemming, string distance,
phonetics, n-grams, TF-IDF, sentiment, classifiers, POS tagging and WordNet —
nineteen focused <code>verbora-*</code> crates behind one coherent design.
Iterators first, borrowed tokens, and no allocation you did not ask for.</p>

<div class="hero-actions">
<a class="hero-btn" href="getting-started/installation">Get started</a>
<a class="hero-btn hero-btn--ghost" href="features/">Browse the features</a>
</div>

<p class="hero-note"><strong>0.2.0 is a breaking release.</strong> If you are on
<code>0.1</code>, some of it fails to compile and some of it quietly returns
something different — <a href="getting-started/upgrading">what changed and how to
migrate</a>.</p>

<div class="hero-signals" role="group" aria-label="Verbora at a glance">
<span><b>19 crates</b><small>Depend on what you use</small></span>
<span><b>Iterators first</b><small>Lazy, borrowed tokens</small></span>
<span><b>No <code>unsafe</code></b><small>Denied workspace-wide</small></span>
<span><b>Reproducible</b><small>Hardware and commands</small></span>
</div>

</div>

## Quick start

```toml
[dependencies]
verbora-tokenizers = "0.2"
verbora-distance = "0.2"
```

```rust
use verbora_tokenizers::{BorrowingTokenizer, WordTokenizer};

let tokens: Vec<&str> = WordTokenizer.tokenize_borrowed("Verbora reads text without copying it");

assert_eq!(tokens[0], "Verbora");
assert_eq!(tokens.len(), 6);
```

Every crate stands alone: depend on the ones you use. Git and path dependencies
are covered in [Installation](getting-started/installation.md).

<HomePaths />

## One toolkit. Every layer of language.

Four families over one shared core. Every leaf is a crate you can use on its
own, behind a fast, idiomatic Rust API.

<div class="capmap" role="group" aria-label="Verbora capability map: four capability families — Prepare, Match, Weigh and Understand — over one shared core">
<div class="capmap-root"><img class="capmap-logo" src="/logo.svg" alt="Verbora" width="1024" height="1024" /></div>
<div class="capmap-trunk" aria-hidden="true"></div>
<div class="capmap-branches">
<section class="capmap-branch cap-1">
<div class="capmap-node">
<h3><span class="capmap-idx" aria-hidden="true">01</span>Prepare</h3>
<p class="capmap-sub">Raw text into comparable units</p>
</div>
<ul class="capmap-leaves">
<li><a href="features/tokenizers">Tokenizers</a><span class="capmap-meta">3, UAX #29</span></li>
<li><a href="features/normalizers">Normalizers</a><span class="capmap-meta">5, all <code>Cow</code></span></li>
<li><a href="features/inflectors">Inflectors</a><span class="capmap-meta">6</span></li>
<li><a href="features/transliterators">Transliterators</a><span class="capmap-meta">kana → romaji</span></li>
<li><a href="features/stemmers">Stemmers</a><span class="capmap-meta">16</span></li>
</ul>
</section>
<section class="capmap-branch cap-2">
<div class="capmap-node">
<h3><span class="capmap-idx" aria-hidden="true">02</span>Match</h3>
<p class="capmap-sub">Similarity, sound and lookup</p>
</div>
<ul class="capmap-leaves">
<li><a href="features/distance">String distance</a><span class="capmap-meta">7 metrics</span></li>
<li><a href="features/phonetics">Phonetics</a><span class="capmap-meta">12 encoders</span></li>
<li><a href="features/phonetic-index">Phonetic index</a><span class="capmap-meta"><span class="capmap-status is-native">native</span></span></li>
<li><a href="features/trie">Trie</a><span class="capmap-meta">prefix + path</span></li>
<li><a href="features/spellcheck">Spellcheck</a><span class="capmap-meta">correction + index</span></li>
</ul>
</section>
<section class="capmap-branch cap-3">
<div class="capmap-node">
<h3><span class="capmap-idx" aria-hidden="true">03</span>Weigh</h3>
<p class="capmap-sub">Statistics and trained models</p>
</div>
<ul class="capmap-leaves">
<li><a href="features/ngrams">N-grams</a><span class="capmap-meta">windows + padding</span></li>
<li><a href="features/tfidf">TF-IDF</a><span class="capmap-meta">sparse, interned</span></li>
<li><a href="features/sentiment">Sentiment</a><span class="capmap-meta">14 lexicons</span></li>
<li><a href="features/classifiers">Classifiers</a><span class="capmap-meta">3 models</span></li>
</ul>
</section>
<section class="capmap-branch cap-4">
<div class="capmap-node">
<h3><span class="capmap-idx" aria-hidden="true">04</span>Understand</h3>
<p class="capmap-sub">Grammar, structure and meaning</p>
</div>
<ul class="capmap-leaves">
<li><a href="features/wordnet">WordNet</a><span class="capmap-meta">4 storages</span></li>
<li><a href="features/language">Language detection</a><span class="capmap-meta"><span class="capmap-status is-native">native</span></span></li>
<li><a href="features/tagger">POS tagger</a><span class="capmap-meta">Brill</span></li>
<li><a href="features/analyzers">Sentence analysis</a></li>
</ul>
</section>
</div>
<div class="capmap-trunk" aria-hidden="true"></div>
<div class="capmap-toolkit">High-performance language toolkit</div>
<p class="capmap-base">All four families rest on <a href="features/core">verbora-core</a> — five traits and <code>StopWords</code> — and on <code>verbora-util</code>: abbreviations, graphs, path trees.</p>
</div>

Everything on the map ships today; none of it is roadmap. The two entries marked
**native** — the phonetic index and language detection — are Verbora's own
designs rather than implementations of a published algorithm, and are tested and
benchmarked like the rest. One thing to know before you plan around it:
[WordNet](features/wordnet)'s database is separately licensed and not bundled
with the crate.

## One operation, four shapes

A tokenizer called once per HTTP request and one called forty million times in a
batch job are not the same problem. Each shape below is a real API; what differs
is what it does with memory.

<div class="ladder">

<div class="rung rung-1">
<h3>Ergonomic</h3>

```rust  ignore
tokenizer.tokenize(text)
```

<p>Returns a <code>Vec</code> of tokens. The right call for most programs — and
it is not the "slow" one.</p>
</div>

<div class="rung rung-2">
<h3>Lazy · zero-copy</h3>

```rust  ignore
tokenizer.tokens(text)
```

<p>An iterator. Each token borrows the input, nothing is materialised, and you
can stop early.</p>
</div>

<div class="rung rung-3">
<h3>Buffer reuse</h3>

```rust  ignore
tokenizer.tokenize_into(text, &mut buf)
```

<p>Appends into a buffer you own, so a hot loop amortises one allocation across
a whole corpus.</p>
</div>

<div class="rung rung-4">
<h3>Scale</h3>

```rust  ignore
Tokenizer::tokenize_batch(&docs)
```

<p>Sequential by default. Most crates also ship an opt-in <code>par_*</code>
batch API behind a <code>parallel</code> Cargo feature — see
<a href="performance/parallelism">Parallelism</a>.</p>
</div>

</div>

[Choosing the right API](choosing/index.md) has the comparison tables and
decision trees for every subsystem that offers more than one shape.

## Why it is fast

- **Tokenizers are iterators first.** `tokens()` is the primitive; `tokenize()`
  and `tokenize_into()` are written on top of it, so there is one implementation
  and no second copy to drift.
- **Nothing is copied that need not be.** Every token all three tokenizers yield
  is a `&str` slice of your input; all five normalizers return `Cow<'_, str>` and
  allocate only when the result would differ from what you passed in.
- **The per-call path has almost nothing to fail on.** Every phonetic encoder
  and every inflector method is total: no `Result`, no panic, on any `&str`. An
  input the algorithm recognises nothing in yields an empty key or an unchanged
  word — an answer you branch on, not an error you handle. What is fallible is
  construction, where a bad abbreviation list or a rule that will not compile is
  rejected once, before the first call.
- **The data layout is chosen, not inherited.** The trie is a flat arena
  addressed by `u32`, not one heap object per node; unit-cost Levenshtein is
  bit-parallel at every length — one 64-bit word of state for a short pattern,
  contiguous blocks of them for a long one — and a dynamic-programming row
  appears only in the weighted forms, which have no bit-parallel formulation.

Every number on this site carries its hardware, its method and the command to
reproduce it, and a figure whose code has changed underneath it is marked pending
rather than restated from memory — see [Performance](performance/index.md) and
[Benchmarks](benchmarks/index.md) for the current results, including the ones
Verbora loses.

<div class="callout callout-note">
<strong>Correctness and scope.</strong> Behaviour is pinned by an executable
specification: each algorithm's output is recorded case by case, checked into
the repository as data, and replayed by that crate's test suite asserting exact
equality. Verbora is pre-1.0 — APIs may still be refined, but documented
behaviour changes only together with those recordings. See
<a href="features/roadmap">Status and scope</a>.
</div>

## Go deeper

<div class="cards">

<a class="card" href="features/">
<span class="card-title">Features →</span>
<span class="card-desc">Every crate: what it does, when to reach for it, the API surface and its guarantees.</span>
</a>

<a class="card" href="performance/">
<span class="card-title">Performance →</span>
<span class="card-desc">Borrowing, laziness, <code>Cow</code>, buffer reuse, batching and parallelism — plus the measured results.</span>
</a>

<a class="card" href="recipes/">
<span class="card-title">Recipes by workload →</span>
<span class="card-desc">Start from your problem — request/response, streaming, batch, huge corpora — not from a function name.</span>
</a>

</div>

For exact type signatures see the [Rust API reference](reference/api.md); for
how these pages stay in step with the code, see [Documentation is part of the
code](reference/docs-are-code.md).
