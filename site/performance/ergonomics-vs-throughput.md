# Ergonomics vs throughput

Verbora offers a high-level, ergonomic API for the common case, and a
low-level one that gives you control over memory. It is tempting to read that as
"the good one and the slow one". It is not that.

## The claim, stated plainly

> `tokenize()` is exactly the right choice for around 95% of programs.
> `tokenize_into()` is preferable for pipelines processing millions of
> documents.

Both halves matter. The second half is why the low-level API exists; the first
half is why it is not the default.

## What the high-level API actually costs

Take the most common shape in real code:

```rust
use verbora_tokenizers::{AggressiveTokenizer, Tokenize};

fn word_count(text: &str) -> usize {
    AggressiveTokenizer::new().tokenize(text).len()
}

assert_eq!(word_count("counting words is not complicated"), 5);
```

The cost of `tokenize()` over `tokens()` here is **one `Vec` allocation**, plus
its growth reallocations. Not one per token — the tokens are `&str` slices
borrowed from `text`. If this function runs once per HTTP request, that
allocation is invisible next to the syscall that delivered the request.

Now put it in a loop over ten million documents and it is ten million
allocations, and it shows up.

Same function. Different workload. That is the whole distinction.

## What the low-level API costs *you*

Optimising is not free, and the cost is usually paid in code you have to keep
correct:

```rust
use verbora_tokenizers::{AggressiveTokenizer, Tokenize};

let t = AggressiveTokenizer::new();
let mut buf = Vec::new();          // ← state you now own
let mut total = 0;

for document in ["one two", "three four"] {
    buf.clear();                   // ← forget this and results accumulate
    t.tokenize_into(document, &mut buf);
    total += buf.len();
}

assert_eq!(total, 4);
```

You have gained: amortised-zero allocations. You have lost: a mutable binding
that lives longer than the operation, a `clear()` whose absence is a silent
correctness bug, and a function that can no longer be a one-liner.

<div class="callout callout-warn">
<strong>The classic bug.</strong> <code>Tokenize::tokenize_into</code>
<em>appends</em>. Omitting <code>buf.clear()</code> does not produce an error; it
produces a growing buffer and quietly wrong counts. If you are not in a hot
loop, you have taken on that risk for nothing.
</div>

## A test for whether you should optimise

Ask, in this order:

```text
1. Has a profiler told me this line is hot?
        no  → use the high-level API and stop reading
        yes → continue

2. Is the cost the container allocation, or the work inside it?
        the work → a different API will not help; look at the algorithm,
                   the input size, or how many candidates you are comparing
        the container → continue

3. Do I consume the result once, in order?
        yes → tokens()          (no container at all)
        no  → tokenize_into()   (one container, reused)
```

Step 2 is the one people skip. If you are computing Levenshtein against 100,000
candidates, the fix is a [trie](../features/trie.md) or a
[phonetic bucket](../features/phonetics.md) that cuts the candidate set — not a
different call shape for the metric. Verbora's own benchmark table shows
`levenshtein/ascii/1024` at 3.24 ms *per call*: at that size no allocation
strategy is the story, the O(nm) sweep is.

## Where the high-level API is genuinely better

Not just "acceptable" — better:

**When you need the result twice.** An iterator is consumed. A `Vec` can be read,
sorted, indexed and passed to two different functions.

**When you need a length before you start.** `tokens().count()` re-runs the scan.

**When you are handing data across an API boundary.** `&[&str]` is a simpler
signature than `impl Iterator<Item = &str>`, compiles faster, and does not leak
your implementation into your callers' types.

**When lifetimes would fight you.** A lazy iterator borrows the input for as long
as it lives. If the text is owned locally and the tokens must outlive it, you are
going to collect anyway — do it deliberately rather than after three rounds with
the borrow checker.

## Where the low-level API is genuinely better

**Corpus processing.** Millions of documents, same shape of output each time,
one buffer reused. This is what it is for.

**Bounded-memory streaming.** `tokens()` lets you process a document larger than
you would want to materialise, because only one token exists at a time.

**Composition without an intermediate.** `phoneticize_tokens` takes
`IntoIterator<Item = &str>`, so a tokenizer's iterator feeds it directly:

```rust
use verbora_phonetics::{Metaphone, phoneticize_tokens};
use verbora_tokenizers::{AggressiveTokenizer, Tokenize};

let tokenizer = AggressiveTokenizer::new();
let metaphone = Metaphone::new();

// No intermediate Vec<String> between the two stages.
let keys = phoneticize_tokens(tokenizer.tokens("the quick brown fox"), false, |t| {
    metaphone.process(t)
});

assert_eq!(keys, ["KK", "BRN", "FKS"]);
```

The `tokenize()` version of that pipeline builds a `Vec` that exists for exactly
as long as it takes to iterate it once.

## The summary you can act on

| | High-level | Low-level |
|---|---|---|
| Default choice | ✅ | |
| Called once per request | ✅ | |
| Called in a loop over a corpus | | ✅ |
| Result needed twice | ✅ | |
| Result consumed once, in order | | ✅ (`tokens()`) |
| Result larger than you want in memory | | ✅ (`tokens()`) |
| Code that others will maintain | ✅ | only where it earns it |

Next: the pair people most often confuse —
[iterator vs reusable buffer](iterator-vs-into.md).
