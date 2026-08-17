<div class="hero">

# Verbora

<p class="hero-tagline">The whole NLP stack, in one Rust toolkit, built for speed. Iterators first, borrowed tokens, and no allocation you did not ask for.</p>

<p class="hero-sub">Tokenizing, normalizing, stemming, string distance, phonetics,
n-grams, TF-IDF, sentiment, classifiers, POS tagging and WordNet — seventeen
<code>verbora-*</code> crates, 105 public APIs, one coherent design. Behaviour
is locked down by a 526,341-case regression suite, and every performance number
on this site is measured, with the hardware and the commands to reproduce it.</p>

</div>

## One toolkit. Multiple layers of language.

From splitting and normalising raw text, through string distance, phonetics and
statistical models, to part-of-speech tags and WordNet's lexical relationships:
every crate below is a layer you can use on its own, behind a fast, idiomatic
Rust API — not a search engine, not a framework, a toolkit you call into from
your own.

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
<li><a href="features/tokenizers">Tokenizers</a><span class="capmap-meta">25</span></li>
<li><a href="features/normalizers">Normalizers</a><span class="capmap-meta">6 + 17 ja</span></li>
<li><a href="features/inflectors">Inflectors</a><span class="capmap-meta">6</span></li>
<li><a href="features/transliterators">Transliterators</a><span class="capmap-meta">kana → romaji</span></li>
<li class="is-pending">Stemmers<span class="capmap-meta">16 · <span class="capmap-status">no page yet</span></span></li>
</ul>
</section>
<section class="capmap-branch cap-2">
<div class="capmap-node">
<h3><span class="capmap-idx" aria-hidden="true">02</span>Match</h3>
<p class="capmap-sub">Similarity, sound and lookup</p>
</div>
<ul class="capmap-leaves">
<li><a href="features/distance">String distance</a><span class="capmap-meta">8 metrics</span></li>
<li><a href="features/phonetics">Phonetics</a><span class="capmap-meta">4 encoders</span></li>
<li><a href="features/phonetic-index">Phonetic index</a><span class="capmap-meta"><span class="capmap-status is-native">native</span></span></li>
<li><a href="features/trie">Trie</a><span class="capmap-meta">prefix + path</span></li>
<li class="is-pending">Spellcheck<span class="capmap-meta">norvig · <span class="capmap-status">no page yet</span></span></li>
</ul>
</section>
<section class="capmap-branch cap-3">
<div class="capmap-node">
<h3><span class="capmap-idx" aria-hidden="true">03</span>Weigh</h3>
<p class="capmap-sub">Statistics and trained models</p>
</div>
<ul class="capmap-leaves">
<li><a href="features/ngrams">N-grams</a><span class="capmap-meta">+ chinese</span></li>
<li><a href="features/tfidf">TF-IDF</a><span class="capmap-meta">idf cache</span></li>
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
<li class="is-pending">POS tagger<span class="capmap-meta">brill · <span class="capmap-status">no page yet</span></span></li>
<li class="is-pending">Sentence analysis<span class="capmap-meta"><span class="capmap-status">no page yet</span></span></li>
</ul>
</section>
</div>
<div class="capmap-trunk" aria-hidden="true"></div>
<div class="capmap-toolkit">High-performance language toolkit</div>
<p class="capmap-base">All four families rest on <a href="features/core">verbora-core</a> — six traits, <code>Token</code>, whitespace helpers — and on <code>verbora-util</code> <span class="capmap-status">no page yet</span>: stop words, abbreviations, digraphs, storage.</p>
</div>

Everything on the map is implemented and tested today; none of it is roadmap.
Seventeen crates are replayed against recorded golden output, so their
behaviour is verified case by case. The two entries marked **native** are
Verbora's own designs — they are tested and benchmarked like the rest, but
carry no golden recordings, because they answer questions no earlier
implementation asked. Entries marked **no page yet** ship and pass
their suites; what is missing is the feature page, and that gap is tracked on
the [roadmap](features/roadmap) rather than left silently absent.

Each leaf links to what it actually does and when to reach for it — including,
for [WordNet](features/wordnet), how to choose a loading strategy for the
database, which is separately licensed and not bundled with the crate.

## Simple when you want it. Explicit when you need it.

Most libraries give you one way to do a thing. Verbora usually gives you
several, because a tokenizer called once per HTTP request and a tokenizer called
forty million times in a batch job are not the same problem. Each shape below is
a real API; the difference between them is what they do with memory.

<div class="ladder">

<div class="rung rung-1">
<h3>Ergonomic</h3>

```rust  ignore
tokenizer.tokenize(text)
```

<p>Returns a <code>Vec</code> of tokens. The right call for the large majority of
programs — and it is not the "slow" one.</p>
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

<p>A sequential batch entry point by default. Thirteen crates, including this
one, also ship an opt-in <code>par_*</code> batch API behind a
<code>parallel</code> Cargo feature — see
<a href="performance/parallelism">Parallelism</a> for which crates, and how
to add <code>rayon</code> yourself everywhere else.</p>
</div>

</div>

Which one you should call is not left to guesswork. It is the subject of an
entire section of this site.

<div class="cards">

<a class="card" href="choosing/">
<span class="card-title">Choosing the right API →</span>
<span class="card-desc">Why several APIs exist for one operation, and how to pick between them: comparison tables, decision trees, and the trade-off behind each.</span>
</a>

<a class="card" href="getting-started/installation">
<span class="card-title">Getting started →</span>
<span class="card-desc">Add the crates, write the first program, and understand how the workspace is laid out.</span>
</a>

<a class="card" href="features/">
<span class="card-title">Features →</span>
<span class="card-desc">Tokenizers, string distance, phonetics, n-grams, normalizers, inflectors, tries, WordNet, TF-IDF, sentiment, classifiers — and what has no feature page yet.</span>
</a>

<a class="card" href="performance/">
<span class="card-title">Performance →</span>
<span class="card-desc">Borrowing, laziness, <code>Cow</code>, buffer reuse, batching, parallelism — and the measured results against competing libraries, with the hardware, method and commands to reproduce them.</span>
</a>

<a class="card" href="recipes/">
<span class="card-title">Recipes by workload →</span>
<span class="card-desc">Start from your problem — request/response, streaming, batch, huge corpora — instead of from a function name.</span>
</a>

</div>

## What is Verbora?

Verbora is a Rust-native natural language processing toolkit with one goal:
cover the NLP work a real application needs — preparation, matching, weighting
and understanding — without forcing you to assemble half a dozen unrelated
crates with half a dozen conventions, and without paying for that breadth in
speed. Every subsystem
is checked against an **executable specification** rather than against a written
spec someone might have misread: a generator exercises each algorithm and records
its output case by case, and the Rust test suites replay those recordings and
assert exact equality.

```text
recorded behaviour  ──checked in as data──▶  each crate's own tests
```

That distinction matters more than it sounds. Every numeric module built so far
contained at least one behaviour that contradicted a careful reading of the
algorithm as written. Recording beats transcribing.

## Designed in Rust, for Rust

Verbora was written in Rust from its first line. The API is the one a Rust
programmer would design from scratch, and the type system is load-bearing
rather than decorative.

- **Tokenizers are iterators first.** `tokens()` is the primitive; `tokenize()`
  and `tokenize_into()` are written on top of it, so there is one implementation
  of the behaviour and no second copy to drift.
- **Nothing is copied that need not be.** Thirteen tokenizers yield `&str`
  slices of your input. Four of the six normalizers return `Cow<'_, str>` and
  allocate only at the first character they actually change.
- **Errors are values.** Where the classic implementations throw — an empty
  token to an inflector, a regex metacharacter as the first letter of a SoundEx
  input — you get a `Result` you can handle.
- **The data layout is chosen, not inherited.** The trie is a flat arena
  addressed by `u32` instead of one heap object per node; Levenshtein reaches
  for a bit-parallel word first and falls back to two rows in L1, never
  materialising a million heap cells.

## Performance is the point

Speed is not a side effect of the language here; it is the thing the library is
built around. Which is why "it's Rust, so it's fast" is not accepted as an
argument — that assumption ships regressions. This project measured a
Jaro–Winkler benchmark running **slower** than a widely-used JavaScript NLP
library, found two per-call `vec![false; len]` allocations, and moved them to
the stack: the benchmark now measures **15.3 ns**, a **1.8×** speedup over
that library. That story is
[in the benchmarks](benchmarks/distance.md#a-measured-regression-and-its-fix),
not hidden.

On the one subsystem benchmarked end-to-end against a competing implementation
so far — `verbora-distance`, 26 benchmarks — the median speedup is **23.4×**, ranging from
1.4× on four-character Hamming inputs to 3307.7× on 1024-character Levenshtein.
The small numbers are published next to the large ones on purpose.

<div class="callout callout-note">
<strong>Scope, stated plainly.</strong> All 105 public APIs are implemented
and covered by the regression suite — the library itself is complete. What is still
catching up is documentation: twelve of the seventeen <code>verbora-*</code>
crates have a full feature page on this site today. The other five —
sentence analysis, stop-word/graph utilities, spellcheck, stemmers and the
Brill POS tagger — are implemented and tested but not yet written up here;
see the <a href="features/roadmap">roadmap</a>. This site documents what
exists and says so where a page does not exist yet.
</div>

## Install

```toml
[dependencies]
verbora-tokenizers = "0.1"
verbora-distance = "0.1"
```

Then:

```rust
use verbora_tokenizers::{AggressiveTokenizer, Tokenize};

let tokenizer = AggressiveTokenizer::new();
let tokens: Vec<&str> = tokenizer.tokenize("Verbora reads text without copying it");

assert_eq!(tokens[0], "Verbora");
assert_eq!(tokens.len(), 6);
```

Full instructions, including using the crates from a git checkout, are in
[Installation](getting-started/installation.md).

## How this documentation is organised

| If you want to… | Go to |
|---|---|
| Get something running in five minutes | [Getting started](getting-started/installation.md) |
| Decide *which* of several similar functions to call | [Choosing the right API](choosing/index.md) |
| Learn what a subsystem can do | [Features](features/index.md) |
| Understand allocations, laziness, batching, parallelism, and see the measured numbers | [Performance](performance/index.md) |
| Solve a concrete problem end to end | [Recipes](recipes/index.md) |
| Know exactly how correctness is verified | Correctness |
| Read the type signatures | [Rust API reference](reference/api.md) |

## The rule this project holds itself to

> A feature does not exist until its code, its tests **and** its documentation
> are updated together. Documentation drift is a bug.

That is written into the repository's `AGENTS.md`, and
[explained here](reference/docs-are-code.md).
