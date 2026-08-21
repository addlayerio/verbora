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

| Buffer element type | Which call fills it | `clear()` frees | Reuse saves |
|---|---|---|---|
| `&'a str` | `tokenize_borrowed_into` | nothing (just pointers) | the whole allocation cost |
| `String` | `Tokenizer::tokenize_into` | every element | the vector only |

Every tokenizer in `verbora-tokenizers` can fill either, because every token is a
substring of the input. Take the borrowed buffer unless the tokens must outlive
the document.

## The two conventions

<div class="callout callout-warn">
<strong>Verbora has two, deliberately.</strong> Check which one you are calling —
this is the single easiest mistake to make with these APIs.
</div>

| Method | Convention |
|---|---|
| `BorrowingTokenizer::tokenize_borrowed_into` | **appends** — you call `clear()` |
| `Tokenizer::tokenize_into` | **appends** |
| `NounInflector::pluralize_into` / `singularize_into` | **appends** |
| `OrdinalInflector::nth_into` | **appends** |
| `verbora_inflectors::CaseMode::apply_into` | **appends** |
| `SoundEx::process_into` / `Metaphone::process_into` | **appends** |
| `verbora_transliterators::transliterate_ja_into` | **appends** |
| `BrillTagger::tag_into` / `annotate_into` | **appends** — and the rules apply only to the newly appended range, so a forgotten `clear()` grows the buffer rather than re-transforming what was already there |
| `verbora_core::Stemmer::stem_into` | **clears first** |

Appending is the default because accumulating tokens across documents is a real
use case. Accumulating stem fragments into one `String` is not, so `stem_into`
clears. Each method states its convention in its own rustdoc.

## Accumulating on purpose

Because `tokenize_borrowed_into` appends, gathering a whole corpus into one
buffer needs no special API:

```rust
use verbora_tokenizers::{BorrowingTokenizer, WordTokenizer};

let mut all: Vec<&str> = Vec::new();

for document in ["one two", "three four"] {
    WordTokenizer.tokenize_borrowed_into(document, &mut all);      // no clear
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
use verbora_tokenizers::{BorrowingTokenizer, WordTokenizer};

// English averages ~5 characters plus a separator per token; over-estimating
// slightly is much cheaper than three doublings.
let text = "the quick brown fox jumps over the lazy dog";
let mut buf: Vec<&str> = Vec::with_capacity(text.len() / 5);

WordTokenizer.tokenize_borrowed_into(text, &mut buf);
assert_eq!(buf.len(), 9);
```

Estimate from the input, not from the token count you do not have yet: one
token needs at least one byte plus a separator between any two, so
`text.len() / average_token_length` is a workable upper bound and
over-estimating slightly is far cheaper than three doublings.

`Trie::reserve(additional)` does the equivalent for a bulk load, growing the node
arena once instead of as the words go in.

## Where reuse does not help

**One-shot calls.** You have introduced mutable state and a manual `clear()` to
save a single allocation.

**When the elements dominate.** If the buffer holds `String`s that are freed on
every `clear()`, reuse saves the vector and nothing else. Prefer a tokenizer that
yields `&str` if you have the choice.

**When the work dwarfs the allocation.** A `levenshtein/ascii/1024` call takes
29.08 µs † — pending re-measurement, but tens of microseconds either way — and a
weighted call evaluates the full scalar recurrence on top of that. Container
reuse is not the dominant question at that scale; see the
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
allocates one `Vec<f64>` row, weighted `osa` three, weighted
`damerau_levenshtein` a full cost plus parent matrix, and the search modes a
cost vector plus a parent vector; unit-cost `levenshtein` and `osa` use
bit-vectors and allocate no row at all, while unit-cost `damerau_levenshtein`
allocates at most one `Vec<i64>` holding its three rolling rows (and nothing at
all for short operands). Those `O(m)` and `O(nm)` allocations cannot be hoisted
out of a loop today.

`jaro` and `jaro_winkler` are the crate's allocation-free case, on ASCII input:
nothing at all when the longer operand is at most 16 units — the greedy scalar
loop keeps its match flags in `[bool; 128]` stack arrays — and nothing when both
trimmed operands fit one 64-bit word, because the byte pattern-match table is a
fixed 256-entry stack array. Past that, the packed pattern-match table becomes
one `Vec<u64>`, while the match-flag bitsets stay on the stack up to 1024 units
per side. A non-ASCII pair adds one `Vec<char>` per operand for the scalar decode
and hashes the table instead. Words are short by nature, so the allocation-free
paths are the common ones.

## Related

- [Iterator vs reusable buffer](iterator-vs-into.md) — when to skip the buffer
  altogether.
- [Allocation behaviour](allocation.md) — the per-API reference.
- [Batch corpora](../recipes/batch.md) — this pattern in a complete program.
- [Parallelism](parallelism.md) — combining this with `rayon`, and the
  built-in `par_*` APIs that exist alongside it.
