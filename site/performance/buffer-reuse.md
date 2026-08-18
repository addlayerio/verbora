# Buffer reuse

The `_into` APIs write into storage you own. This page is the mechanics: what
`clear()` actually does, the two clearing conventions in Verbora, how to size a
buffer up front, and where reuse does *not* help.

## The problem it solves

```rust  ignore
for document in corpus {          // ten million iterations
    let tokens = tokenizer.tokenize(document);
    consume(&tokens);
}                                  // Vec dropped here — allocation returned
```

Every iteration allocates a `Vec`, grows it, and frees it. The allocator sees ten
million malloc/free pairs for buffers that are all roughly the same size.

```rust  ignore
let mut tokens = Vec::new();
for document in corpus {
    tokens.clear();
    tokenizer.tokenize_into(document, &mut tokens);
    consume(&tokens);
}
```

Now the allocator sees a handful of calls in total: the buffer grows to the
largest document's token count within the first few iterations and stays there.

## What `clear()` does

`Vec::clear` drops every element and sets the length to zero. It **does not**
free the buffer — capacity is untouched. That is precisely what makes the pattern
work:

```rust
let mut v: Vec<&str> = Vec::new();
v.push("a");
v.push("b");

let capacity_before = v.capacity();
v.clear();

assert_eq!(v.len(), 0);
assert_eq!(v.capacity(), capacity_before);   // the allocation survives
```

For a `Vec<&str>` the elements are just fat pointers, so dropping them is free.
For a `Vec<String>` each `clear()` frees every string it held — reuse then saves
the *vector's* allocation but not the per-string ones. Which of those you have
depends on the tokenizer:

| Tokenizer's token type | `clear()` frees | Reuse saves |
|---|---|---|
| `&'a str` — the thirteen character-class tokenizers, `WordTokenizer` | nothing (just pointers) | the whole allocation cost |
| `Cow<'a, str>` — `AggressiveTokenizerNo`, `…Sv`, `…Hi` | only the tokens that were rewritten | the vector, plus the borrowed tokens |
| `Utf16Token<'a>` — `WordPunctTokenizer`, `TreebankWordTokenizer`, `TokenizerJa`, `CaseTokenizer`, `OrthographyTokenizer` | only the owned variants | the vector, plus the borrowed tokens |
| `String` — `SentenceTokenizer` | every element | the vector only |

## The two conventions

<div class="callout callout-warn">
<strong>Verbora has two, deliberately.</strong> Check which one you are calling —
this is the single easiest mistake to make with these APIs.
</div>

| Method | Convention |
|---|---|
| `Tokenize::tokenize_into` | **appends** — you call `clear()` |
| `verbora_core::Tokenizer::tokenize_into` | **appends** |
| `NounInflector::pluralize_into` / `singularize_into` | **appends** |
| `verbora_inflectors::CaseMode::apply_into` | **appends** |
| `verbora_core::Stemmer::stem_into` | **clears first** |

Appending is the default because accumulating tokens across documents is a real
use case. Accumulating stem fragments into one `String` is not, so `stem_into`
clears. Each method states its convention in its own rustdoc.

## Accumulating on purpose

Because `tokenize_into` appends, gathering a whole corpus into one buffer needs
no special API:

```rust
use verbora_tokenizers::{AggressiveTokenizer, Tokenize};

let t = AggressiveTokenizer::new();
let mut all = Vec::new();

for document in ["one two", "three four"] {
    t.tokenize_into(document, &mut all);      // no clear
}

assert_eq!(all, ["one", "two", "three", "four"]);
```

Note the lifetime: every token borrows its own document, so all of them must
outlive `all`. Here they are `'static` literals; in real code the documents must
stay alive.

## Sizing the buffer up front

`Vec::with_capacity` skips the growth reallocations entirely when you can
estimate the size:

```rust
use verbora_tokenizers::{AggressiveTokenizer, Tokenize};

let t = AggressiveTokenizer::new();

// English averages ~5 characters plus a separator per token; over-estimating
// slightly is much cheaper than three doublings.
let text = "the quick brown fox jumps over the lazy dog";
let mut buf = Vec::with_capacity(text.len() / 5);

t.tokenize_into(text, &mut buf);
assert_eq!(buf.len(), 9);
```

Verbora's own iterators help here. `WordRuns::size_hint` reports an upper bound
derived from the input length — one token needs at least one byte, plus a
separator between any two — so `collect()` on a tokenizer's iterator can
pre-size in one shot rather than doubling repeatedly.

`Trie::reserve(additional)` does the equivalent for a bulk load, growing the node
arena once instead of as the words go in.

## Where reuse does not help

**One-shot calls.** You have introduced mutable state and a manual `clear()` to
save a single allocation.

**When the elements dominate.** If the buffer holds `String`s that are freed on
every `clear()`, reuse saves the vector and nothing else. Prefer a tokenizer that
yields `&str` if you have the choice.

**When the work dwarfs the allocation.** A `levenshtein/ascii/1024` call takes
29.08 µs, and a weighted call evaluates the full scalar recurrence on top of
that. Container reuse is not the dominant question at that scale — see the
[shape suite](../benchmarks/distance.md#current-levenshtein-shape-suite) for how
much the input shape matters instead.

**Across threads.** A buffer is `&mut`, so it cannot be shared. Under `rayon`,
give each worker its own — `map_init` exists exactly for this:

```rust  ignore
use rayon::prelude::*;

let counts: Vec<usize> = corpus
    .par_iter()
    .map_init(Vec::new, |buf, doc| {          // one buffer per worker thread
        buf.clear();
        tokenizer.tokenize_into(doc, buf);
        buf.len()
    })
    .collect();
```

The built-in [`par_*` batch APIs](parallelism.md) allocate fresh output per
item — they are thin fan-outs over the sequential primitive, not buffer-reusing
implementations. `map_init` is what you reach for when you want reuse *and*
parallelism together.

## There is no scratch-buffer API for distance

`verbora-distance` has no `levenshtein_with_scratch`. Weighted plain Levenshtein
allocates one `Vec<f64>` row, restricted Damerau three, and the full-matrix
search modes a cost vector plus a parent vector; unit-cost paths use bit-vectors
and allocate no row at all. Those `O(m)` and `O(nm)` allocations cannot be
hoisted out of a loop today.

Jaro–Winkler *is* allocation-free for inputs up to 128 code units: its two
match-flag arrays live on the stack below that threshold, and only above it does
it allocate two `Vec<bool>`. Words are short by nature, so the stack path is the
common one.

## Related

- [Iterator vs reusable buffer](iterator-vs-into.md) — when to skip the buffer
  altogether.
- [Allocation behaviour](allocation.md) — the per-API reference.
- [Batch corpora](../recipes/batch.md) — this pattern in a complete program.
- [Parallelism](parallelism.md) — combining this with `rayon`, and the
  built-in `par_*` APIs that exist alongside it.
