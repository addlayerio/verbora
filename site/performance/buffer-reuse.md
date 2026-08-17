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

**Appending — you clear.**

```rust  ignore
Tokenize::tokenize_into(&self, text, &mut out)          // appends
verbora_core::Tokenizer::tokenize_into(&self, text, &mut out)  // appends
NounInflector::pluralize_into(&self, token, &mut out)   // appends
verbora_inflectors::CaseMode::apply_into(self, s, &mut out)    // appends
```

The doc comment on `Tokenize::tokenize_into` says it plainly: *"`out` is **not**
cleared, so a caller can accumulate across inputs or — the intended use — reuse
one buffer's capacity across a corpus."* Both readings are supported on purpose.
`CaseMode::apply_into`'s own doc comment agrees: *"Applies the restoration,
**appending** to `out`."*

**Clearing — it clears for you.**

```rust  ignore
verbora_core::Stemmer::stem_into(&self, token, &mut out)  // clears first
```

Why the difference? Accumulating tokens across documents is a real use case;
accumulating stem fragments into one `String` is not — it would produce
gibberish. The convention follows what the output type is actually for.

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

**When the work dwarfs the allocation.** `levenshtein/ascii/1024` measures 3.24 ms
per call. Nothing about container reuse is visible next to that.

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

None of Verbora's thirteen built-in `par_*` batch APIs (see
[Parallelism](parallelism.md)) reuse a buffer internally — each is a thin
`par_iter().map(...)` wrapper over the existing sequential primitive,
deliberately, so it allocates fresh output per item same as the sequential
call would. This `map_init` pattern is still what you reach for whenever you
want buffer reuse *and* parallelism together, whether or not a built-in
`par_*` exists for the underlying operation.

## Scratch buffers that do not exist

There is no `levenshtein_with_scratch`. The Levenshtein family allocates its own
working rows on every call — two `Vec<f64>` for the plain path, three for
restricted Damerau, a cost vector plus a parent vector for the full-matrix modes.
Those are `O(m)` or `O(nm)` allocations you cannot currently hoist out of a loop.

Jaro–Winkler *is* allocation-free for inputs up to 128 code units, because its
two match-flag arrays live on the stack below that threshold. Above it, it
allocates two `Vec<bool>`. That threshold was chosen because words are short by
nature — see [the regression story](../benchmarks/distance.md#a-measured-regression-and-its-fix).

## Related

- [Iterator vs reusable buffer](iterator-vs-into.md) — when to skip the buffer
  altogether.
- [Allocation behaviour](allocation.md) — the per-API reference.
- [Batch corpora](../recipes/batch.md) — this pattern in a complete program.
- [Parallelism](parallelism.md) — combining this with `rayon`, and the
  built-in `par_*` APIs that exist alongside it.
