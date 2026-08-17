# Streaming

Input larger than you want resident, or output needed before the input ends.
Log processing, tailing a feed, a file you would rather not load.

**Priorities:** bounded memory, lazy processing, early output.
**Non-priority:** total throughput. You are trading some of it for the ability to
start at all.

## The rule

Never call anything that collects. Verbora's lazy entry points:

| Subsystem | Lazy API | Yields |
|---|---|---|
| Tokenizers | `tokens(text)` | one token at a time |
| N-grams | `ngrams_iter(&tokens, n, …)` | one window at a time |
| Trie | `iter_keys_with_prefix(p)`, `keys()`, `iter_matches_on_path(s)` | one key at a time |
| Phonetics | `phoneticize_tokens*` | takes `IntoIterator` — chains without materialising |
| Normalizers | all the `Cow` returns | no collection at all |

## A bounded-memory pipeline

```rust
use verbora_normalizers::remove_diacritics;
use verbora_tokenizers::{AggressiveTokenizer, Tokenize};

/// Count tokens over a document of any size. Peak memory is one token.
fn count_tokens(document: &str) -> usize {
    let tokenizer = AggressiveTokenizer::new();
    let folded = remove_diacritics(document);   // one Cow; borrowed if unaccented

    tokenizer.tokens(&folded).count()
}

assert_eq!(count_tokens("un café très fort"), 4);
```

The only thing that scales with document size here is `folded`, and only when the
document actually contains diacritics. Everything downstream is one token wide.

## Early output

The reason streaming is not just "slower batch": you can answer before you have
read everything.

```rust
use verbora_tokenizers::{AggressiveTokenizer, Tokenize};

/// Returns the position of the first token matching `needle`, without
/// tokenizing the rest of the input.
fn find_position(document: &str, needle: &str) -> Option<usize> {
    AggressiveTokenizer::new()
        .tokens(document)
        .position(|t| t == needle)
}

assert_eq!(find_position("alpha beta gamma delta", "gamma"), Some(2));
```

On a long document that is the difference between splitting three tokens and
splitting all of them.

## Chaining across stage boundaries

The trick to keeping a pipeline lazy is to make sure no stage collects. Verbora's
APIs that take `IntoIterator` are built for this:

```rust
use verbora_core::StopWords;
use verbora_phonetics::{Metaphone, phoneticize_tokens_with};
use verbora_tokenizers::{AggressiveTokenizer, Tokenize};

let tokenizer = AggressiveTokenizer::new();
let metaphone = Metaphone::new();
let stops = StopWords::english();

// tokens ──▶ stop-word filter ──▶ phonetic key
// No intermediate Vec between the tokenizer and the encoder.
let keys = phoneticize_tokens_with(
    tokenizer.tokens("the quick brown fox"),
    &stops,
    false,
    |t| metaphone.process(t),
);

assert_eq!(keys, ["KK", "BRN", "FKS"]);
```

`phoneticize_tokens_with` *does* collect at the end — it returns a `Vec` of
whatever your closure produced — but the tokens themselves never accumulate.

<div class="callout callout-note">
<strong>Prefer the <code>_with</code> variants in streaming code.</strong>
<code>phoneticize_tokens</code> reads the reference's process-global stop-word list;
<code>phoneticize_tokens_with</code> takes an explicit <code>&amp;StopWords</code>.
The same applies to <code>ngrams_str_with</code> against
<code>ngrams_str</code>. Explicit state is reproducible, testable and thread-safe.
</div>

## Reading line by line

The usual shape for a file or socket:

```rust  ignore
use std::io::{BufRead, BufReader};
use std::fs::File;

use verbora_tokenizers::{AggressiveTokenizer, Tokenize};

let tokenizer = AggressiveTokenizer::new();
let reader = BufReader::new(File::open("corpus.txt")?);

let mut total = 0usize;
for line in reader.lines() {
    let line = line?;
    // Tokens borrow `line`, so they must be consumed before it is dropped.
    total += tokenizer.tokens(&line).filter(|t| t.len() > 3).count();
}
```

<div class="callout callout-warn">
<strong>The borrow is the constraint.</strong> Tokens point into
<code>line</code>, which is dropped at the end of each iteration. You cannot
push them into a <code>Vec</code> that outlives the loop without copying. If you
need to retain them, <code>.map(str::to_owned)</code> at that boundary — and note
that you have just left streaming behind.
</div>

## N-grams over a stream

`ngrams_iter` is lazy over an already-tokenized slice, which means the *tokens*
must be materialised even though the *windows* are not:

```rust
use verbora_ngrams::ngrams_iter;

let tokens = ["the", "quick", "brown", "fox", "jumps"];

// Only two windows are ever built.
let first_two: Vec<_> = ngrams_iter(&tokens, 3, None, None).take(2).collect();

assert_eq!(first_two.len(), 2);
assert_eq!(&*first_two[0], &["the", "quick", "brown"]);
```

For a genuinely unbounded stream, window the tokens yourself with a small ring
buffer of length `n`; Verbora has no streaming n-gram entry point that reads from
an iterator.

## What streaming costs you

**No second pass.** An iterator is consumed. If you need the tokens twice you
must either re-tokenize or collect — at which point consider
[batch](batch.md) instead.

**No length up front.** `tokens().count()` re-runs the scan.

**Lifetimes get louder.** Everything borrows, so the compiler will be involved in
your design. That is the price of not copying.

**Not necessarily faster.** Streaming optimises *peak memory* and
*time-to-first-result*. Total throughput may be slightly worse than a tight batch
loop with a reused buffer, which keeps the same pages hot.

## Checklist

- [ ] No `collect()` in the middle of the pipeline
- [ ] `_with` variants used instead of the process-global ones
- [ ] Tokens consumed before their source buffer is dropped
- [ ] Early-exit combinators (`find`, `position`, `any`, `take_while`) used where
      the answer allows it
- [ ] Peak memory actually measured, not assumed

## Related

- [Iterator vs reusable buffer](../performance/iterator-vs-into.md)
- [Batch vs streaming](../performance/batch-vs-streaming.md)
- [Batch corpora](batch.md)
