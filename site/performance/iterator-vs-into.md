# Iterator vs reusable buffer

These are the two shapes people most often assume are alternatives. They are
not. They optimise different things, and each is wrong in the other's situation.

## The two problems

**An iterator removes the container.** There is no `Vec`, so there is nothing to
allocate, nothing to fill, and nothing to keep in memory. You get one item at a
time and you can stop whenever you like.

**`_into` reuses the container.** There is still a `Vec`, still filled
completely, still holding every item at once — but its allocation is paid for
once and then borrowed by every subsequent call.

```text
tokens()                          tokenize_into()

input                             allocate buffer once
  │                                        │
  ├─ token ─▶ consumer            document 1 ─▶ fill ─▶ consume ─▶ clear
  ├─ token ─▶ consumer            document 2 ─▶ fill ─▶ consume ─▶ clear
  ├─ token ─▶ consumer            document 3 ─▶ fill ─▶ consume ─▶ clear
  └─ …                            document 4 ─▶ fill ─▶ consume ─▶ clear

no container exists               one container, reused
peak memory: one token            peak memory: one document's tokens
can stop early                    always produces everything
single pass                       result is re-readable, indexable, sortable
```

## When the iterator wins

**You consume once, in order.**

```rust
use verbora_tokenizers::{AggressiveTokenizer, Tokenize};

let t = AggressiveTokenizer::new();
let letters: usize = t.tokens("counting letters in every word").map(str::len).sum();

assert_eq!(letters, 26);
```

Materialising here would allocate a `Vec` that exists for one traversal.

**You might not need all of it.**

```rust
use verbora_tokenizers::{AggressiveTokenizer, Tokenize};

let t = AggressiveTokenizer::new();

// Scanning stops at the first hit; the rest of the string is never split.
let found = t.tokens("alpha beta gamma delta").any(|w| w == "beta");

assert!(found);
```

With a materialised result you have already done all the work before you can
look at any of it. On a 9.7 kB document that is the difference between splitting
two tokens and splitting fifteen hundred.

**The input is big and you do not want it all in memory at once.** Peak memory
for a lazy pass is one token. For a materialised pass it is every token in the
document, simultaneously.

**You are feeding another API that takes `IntoIterator`.**

```rust
use verbora_core::StopWords;
use verbora_phonetics::{SoundEx, phoneticize_tokens_with};
use verbora_tokenizers::{AggressiveTokenizer, Tokenize};

let tokenizer = AggressiveTokenizer::new();
let soundex = SoundEx::new();
let stops = StopWords::english();

let keys = phoneticize_tokens_with(tokenizer.tokens("the quick fox"), &stops, false, |t| {
    soundex.process(t)
});

assert_eq!(keys, ["Q200", "F200"]);
```

No `Vec<String>` is built between the tokenizer and the encoder.

## When `_into` wins

**You need the whole result, and you need it again and again.**

```rust
use verbora_tokenizers::{AggressiveTokenizer, Tokenize};

fn index(corpus: &[&str]) -> usize {
    let t = AggressiveTokenizer::new();
    let mut buf = Vec::new();
    let mut hits = 0;

    for document in corpus {
        buf.clear();
        t.tokenize_into(document, &mut buf);

        // Two passes over the same tokens. An iterator would have to re-scan.
        let longest = buf.iter().map(|w| w.len()).max().unwrap_or(0);
        hits += buf.iter().filter(|w| w.len() == longest).count();
    }

    hits
}

assert_eq!(index(&["a bb ccc", "dddd ee"]), 2);
```

Two passes over an iterator means tokenizing twice. Two passes over a buffer is
free.

**You need random access, a length, or sorting.** All of these want a slice.

**You are calling the operation an enormous number of times with output of
similar size each time.** This is the case the API was added for: the buffer
reaches its high-water mark within the first few documents and then never calls
the allocator again.

## When neither wins

Calling something once. Use `tokenize()`. Both of the shapes on this page cost
you something in exchange for a saving you will not measure.

## The decision

```text
Do I need every item at once?
│
├── No — I consume them one at a time
│      │
│      ├── Might I stop early?         → tokens()          (iterator wins twice)
│      └── No, I always consume all     → tokens()          (still no container)
│
└── Yes — I need the whole collection
       │
       ├── Once?                        → tokenize()
       └── Repeatedly, in a loop?        → tokenize_into() with one buffer
```

## What Verbora actually offers

Not every subsystem has both. Reading a name is not enough — check the page.

| Subsystem | Lazy | `_into` |
|---|---|---|
| Tokenizers | `tokens()` on every tokenizer | `tokenize_into`, `tokenize_borrowed_into` |
| N-grams | `ngrams_iter()` → `NGramIter` | — |
| Trie | `iter_keys_with_prefix()`, `keys()`, `iter_matches_on_path()` | — |
| Inflectors | — | `pluralize_into`, `singularize_into` |
| Core traits | — | `stem_into` (clears first) |
| Distance | — | — |
| Phonetics | — | — |
| Normalizers | — | — (they return `Cow`, which is the analogous saving) |

<div class="callout callout-note">
<strong><code>_into</code> is not the same as allocation-free.</strong> It removes
the <em>container</em> allocation. Whether the elements allocate depends on the
API: tokenizers put borrowed <code>&amp;str</code> in the buffer, so nothing else
allocates — but <code>NounInflector::pluralize_into</code> still allocates inside
the matching rule, so it saves one <code>String</code> per call rather than all
of them. The feature pages state this per API.
</div>

## Related

- [Buffer reuse](buffer-reuse.md) — the mechanics of `clear()`, capacity, and
  sizing.
- [Batch vs streaming](batch-vs-streaming.md) — the same trade-off one level up.
- [Choosing the right API: tokenization](../choosing/tokenization.md) — this
  reasoning applied to the flagship subsystem.
