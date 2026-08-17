# Batch vs streaming

The [iterator vs `_into`](iterator-vs-into.md) question was about one document.
This one is about a collection of them, and it is a different trade-off: batching
buys setup amortisation and locality, streaming buys bounded memory and early
output.

## The two shapes

```text
STREAMING                             BATCH

for doc in reader {                   let docs = collect_all();
    process(doc);                     process_all(&docs);
    emit(result);
}

memory: one document                  memory: every document
first output: immediately             first output: after the last input
input size: unbounded                 input size: must fit
setup: repeated per item              setup: once, shared
locality: whatever the loop touches   locality: can be arranged
```

## What batching can buy

**Preallocation.** When you know there are 10,000 documents you can size the
output once instead of doubling a `Vec` fourteen times.

**Shared setup.** Anything constructed per item can be hoisted. In Verbora most
tokenizers are zero-sized types so this is nearly free, but it is real for
`OrthographyTokenizer::new(lang)`, `SentenceTokenizer::with_abbreviations(...)`
and the regex-driven tokenizers, which hold compiled patterns.

**Cache behaviour.** Processing documents that are contiguous in memory beats
chasing pointers to documents scattered across the heap. This is the same
principle as `verbora-trie`'s flat arena, one level up — see
[Cache locality](cache-locality.md).

**A place to put parallelism.** You cannot split a stream you have not read.
A slice, you can. See [Parallelism](parallelism.md).

## What streaming can buy

**Bounded memory.** The reason to stream is usually that you cannot afford not
to. A tokenizer's `tokens()` iterator holds one token at a time regardless of
document size.

**Early output.** A search that stops at the first hit does not tokenize the rest
of the corpus:

```rust
use verbora_tokenizers::{AggressiveTokenizer, Tokenize};

let t = AggressiveTokenizer::new();
let corpus = ["alpha beta", "gamma delta", "epsilon zeta"];

// Stops at document 2, token 1. Documents 3 onward are never touched.
let found = corpus
    .iter()
    .flat_map(|doc| t.tokens(doc))
    .position(|w| w == "gamma");

assert_eq!(found, Some(2));
```

**Low latency.** Time-to-first-result for a streaming pipeline is the cost of one
item. For a batch it is the cost of all of them.

**Back-pressure.** A stream can be throttled. A batch is already in memory.

## What Verbora actually provides

<div class="callout callout-warn">
<strong>Verbora's sequential batch surface is two provided trait methods, and
they are not optimised.</strong> <code>verbora_core::Tokenizer::tokenize_batch</code>
and <code>verbora_core::Stemmer::stem_batch</code> have sequential default
bodies: a plain <code>map</code> over the inputs, one fresh allocation per
item, no shared buffer and no parallelism — nothing in the workspace overrides
either. Thirteen other crates additionally expose an optional, <em>parallel</em>
<code>par_*_batch</code> function behind a <code>parallel</code> Cargo feature
(see <a href="parallelism">Parallelism</a>) — a real <code>rayon</code> fan-out
over the existing sequential primitive, benchmarked and tested. That
solves a different problem than this section, though: none of the thirteen
reuse a buffer across items either, so it is not the allocation story below.
</div>

They exist so that generic code over the traits can express "process all of
these", and so an implementation can override them later without changing its
callers. Today, `tokenize_batch` allocates **more** than a `tokenize_into` loop
does, and it yields `Vec<Vec<String>>` — owned strings — where
`Tokenize::tokenize` would have given you borrowed `&str`.

So for this sequential, allocation-conscious shape, "batch" in Verbora is still
code you write, not a function you call:

```rust
use verbora_tokenizers::{AggressiveTokenizer, Tokenize};

/// A batch loop with the three batch advantages made explicit.
fn token_counts(corpus: &[&str]) -> Vec<usize> {
    let tokenizer = AggressiveTokenizer::new();   // setup hoisted
    let mut counts = Vec::with_capacity(corpus.len());  // output pre-sized
    let mut buf = Vec::new();                     // working buffer reused

    for document in corpus {
        buf.clear();
        tokenizer.tokenize_into(document, &mut buf);
        counts.push(buf.len());
    }

    counts
}

assert_eq!(token_counts(&["a b c", "d e"]), [3, 2]);
```

That loop is what a good `tokenize_batch` implementation would do internally.
Writing it yourself also means you can decide what "process" means without the
library guessing.

## Choosing

```text
Does the whole input fit in memory comfortably?
│
├── No ────────────────────────────────▶ stream
│
└── Yes
     │
     ├── Do I need the first result before the last input arrives?
     │      └── Yes ─────────────────────▶ stream
     │
     ├── Am I going to parallelise?
     │      └── Yes ─────────────────────▶ batch (you need a slice to split)
     │
     ├── Is per-item setup expensive?
     │      └── Yes ─────────────────────▶ batch (hoist it)
     │
     └── Otherwise ──────────────────────▶ either; pick the clearer one
```

## The hybrid worth knowing

Chunked streaming gets most of both: bounded memory, plus a slice big enough to
amortise setup and to hand to a thread pool.

```rust
use verbora_tokenizers::{AggressiveTokenizer, Tokenize};

fn process_chunked(corpus: &[&str], chunk: usize) -> usize {
    let tokenizer = AggressiveTokenizer::new();
    let mut buf = Vec::new();
    let mut total = 0;

    for window in corpus.chunks(chunk) {
        // Peak memory is bounded by `chunk`, not by `corpus.len()`.
        for document in window {
            buf.clear();
            tokenizer.tokenize_into(document, &mut buf);
            total += buf.len();
        }
    }

    total
}

assert_eq!(process_chunked(&["a b", "c d", "e f"], 2), 6);
```

Reading a corpus off disk, a chunk size in the low thousands is usually enough to
amortise everything amortisable while keeping resident memory flat.

## Related

- [Iterator vs reusable buffer](iterator-vs-into.md)
- [Parallelism](parallelism.md)
- [Streaming recipes](../recipes/streaming.md) · [Batch recipes](../recipes/batch.md)
