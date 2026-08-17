# Interactive request/response

One input, an answer now. A search box, an API endpoint, an editor plugin.

**Priorities:** latency, ergonomics, predictability.
**Non-priority:** allocation count. You are about to do a syscall.

## The shape

```rust
use verbora_normalizers::remove_diacritics;
use verbora_tokenizers::{AggressiveTokenizer, Tokenize};

/// Prepare a user's query for lookup: fold accents, split, lowercase.
fn prepare_query(raw: &str) -> Vec<String> {
    let folded = remove_diacritics(raw);            // Cow: free if unaccented
    let tokenizer = AggressiveTokenizer::new();     // zero-sized

    tokenizer
        .tokens(&folded)
        .map(str::to_lowercase)
        .collect()
}

assert_eq!(prepare_query("Café  RESTAURANT"), ["cafe", "restaurant"]);
```

Nothing clever. `tokenize()` would have been just as good; `tokens().map().collect()`
is here because the `map` needs to happen anyway, so there is no reason to
materialise twice.

## Why not the performance APIs

A request handler runs once per request. `tokenize_into` would need a buffer that
outlives the request — which means either a `thread_local!`, a pool, or passing
it down your call stack. All three are real complexity, and the thing you save is
one allocation against a request that already cost a TCP round trip.

<div class="callout callout-note">
<strong>Where per-request state <em>is</em> worth it:</strong> anything expensive
to construct. Compiled regexes, an <code>OrthographyTokenizer</code>, a
<code>SentenceTokenizer::with_abbreviations</code> list, and above all a
<code>Trie</code> — build those once at startup, share them, and keep them out of
the hot path.
</div>

## Startup vs per-request

```rust
use verbora_trie::Trie;

/// Built once. `Trie` is `Send + Sync`, so an `Arc<Trie>` serves every request.
fn build_index(words: &[&str]) -> Trie {
    let mut trie = Trie::new();
    trie.reserve(words.len());        // one growth instead of several
    trie.add_strings(words.iter().copied());
    trie
}

let index = build_index(&["rust", "rustic", "rusty", "ruse"]);

// Per request: a query against a shared, immutable structure.
assert_eq!(index.keys_with_prefix("rust"), ["rust", "rustic", "rusty"]);
```

| Build once, at startup | Build per request |
|---|---|
| `Trie` | tokenizer values (they are zero-sized) |
| `OrthographyTokenizer::new(lang)` | `SoundEx`, `Metaphone`, `DoubleMetaphone`, `SoundExDM` |
| `RegexpTokenizer` / `Pattern` (compiled regex) | `levenshtein::Options` (a `Copy` struct) |
| `SentenceTokenizer::with_abbreviations(…)` | `NounInflector::new()` if you add no rules |
| `StopWords::english()` | |

## Bounding the work, not the allocations

The latency risk in an interactive path is almost never allocation. It is doing
`O(n)` work against a corpus of size `n`:

```rust
use verbora_distance::{levenshtein, levenshtein::Options};

/// Rank candidates by edit distance — but only after something else has
/// reduced `candidates` to a sane size.
fn best_match<'a>(query: &str, candidates: &[&'a str]) -> Option<&'a str> {
    let opts = Options::default();          // hoisted out of the loop

    candidates
        .iter()
        .map(|c| (*c, levenshtein(query, c, &opts)))
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(c, _)| c)
}

assert_eq!(best_match("kitten", &["sitting", "mitten", "bitten"]), Some("mitten"));
```

If `candidates` is your whole dictionary, this is the wrong shape no matter how
fast `levenshtein` is. Narrow it first — see
[Fuzzy name matching](fuzzy-matching.md).

## Handling the fallible APIs

Interactive input is arbitrary, so the `Result`-returning APIs matter here more
than anywhere else:

```rust
use verbora_phonetics::{PhoneticError, SoundEx};

let soundex = SoundEx::new();

// A token beginning with a regex metacharacter makes the reference throw a
// SyntaxError. `try_process` hands you the condition instead of panicking, so
// you can decide what an unusable query token means for your endpoint.
assert_eq!(
    soundex.try_process("(hello", None),
    Err(PhoneticError::InvalidInitialPattern('(')),
);

// The infallible entry point cannot fail, and is what you want for clean input.
assert_eq!(soundex.process("hello"), "H400");
```

Same for inflectors, where the empty token is an `Err(EmptyToken)` rather than a
panic:

```rust
use verbora_inflectors::NounInflector;

let inflector = NounInflector::new();

assert_eq!(inflector.pluralize("octopus").unwrap(), "octopi");
assert!(inflector.pluralize("").is_err());
```

## Predictability

Two things can make a latency profile spiky:

**Input size you do not control.** `levenshtein` is `O(nm)`. A 100 kB paste into
a search box is 10¹⁰ cell updates. Cap the input length at the boundary.

**Non-ASCII promotion.** ASCII operands are compared as borrowed `&[u8]`;
non-ASCII ones are promoted to `Vec<u16>`. The measured difference is small
(`levenshtein/cyrillic/256` at 193.75 µs against ASCII's 191.34 µs) but it is not
zero, and it is data-dependent.

## Checklist

- [ ] Expensive structures built at startup, shared behind `Arc`
- [ ] `Options` and `StopWords` hoisted out of loops
- [ ] Input length capped before it reaches an `O(nm)` metric
- [ ] Candidate sets narrowed before ranking
- [ ] `try_*` / `Result` paths handled, not `unwrap()`ed on user input
- [ ] No `thread_local!` buffer pools until a profiler asks for one

## Related

- [Your first program](../getting-started/first-program.md)
- [Ergonomics vs throughput](../performance/ergonomics-vs-throughput.md)
- [Fuzzy name matching](fuzzy-matching.md)
