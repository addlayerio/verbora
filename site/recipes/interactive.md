# Interactive request/response

One input, an answer now. A search box, an API endpoint, an editor plugin.

**Priorities:** latency, ergonomics, predictability.
**Non-priority:** allocation count. You are about to do a syscall.

## The shape

```rust
use verbora_normalizers::remove_diacritics;
use verbora_tokenizers::{BorrowingTokenizer, WordTokenizer};

/// Prepare a user's query for lookup: fold accents, split, lowercase.
fn prepare_query(raw: &str) -> Vec<String> {
    let folded = remove_diacritics(raw);            // Cow: free if unaccented

    WordTokenizer
        .tokens(&folded)                            // zero-sized tokenizer
        .map(str::to_lowercase)
        .collect()
}

assert_eq!(prepare_query("Café  RESTAURANT"), ["cafe", "restaurant"]);
```

Nothing clever. `tokenize_borrowed()` would have been just as good;
`tokens().map().collect()` is here because the `map` needs to happen anyway, so
there is no reason to materialise twice.

## Why not the performance APIs

A request handler runs once per request. `tokenize_borrowed_into` would need a
buffer that outlives the request — which means either a `thread_local!`, a pool, or passing
it down your call stack. All three are real complexity, and the thing you save is
one allocation against a request that already cost a TCP round trip.

<div class="callout callout-note">
<strong>Where startup state <em>is</em> worth it:</strong> anything expensive to
construct. A <code>SentenceTokenizer::with_abbreviations</code> list, a stemmed
<code>SentimentAnalyzer</code>, and above all a <code>Trie</code> — build those
once at startup, share them, and keep them out of the hot path.
</div>

## Startup vs per-request

```rust
use verbora_trie::Trie;

/// Built once. `Trie` is `Send + Sync`, so an `Arc<Trie>` serves every request.
fn build_index(words: &[&str]) -> Trie {
    let mut trie = Trie::new();
    trie.reserve(words.len());        // one growth instead of several
    trie.insert_all(words.iter().copied());
    trie
}

let index = build_index(&["rust", "rustic", "rusty", "ruse"]);

// Per request: a query against a shared, immutable structure.
assert_eq!(index.keys_with_prefix("rust"), ["rust", "rustic", "rusty"]);
```

If the index never changes after startup, `trie.freeze()` is the shape to hand
to the request path: a `FrozenTrie` answers `contains` and every prefix
enumeration from a precomputed key table, and `keys_slice` returns the matching
words as a borrowed `&[String]` with no per-key allocation at all.

| Build once, at startup | Build per request |
|---|---|
| `Trie`, or the `FrozenTrie` you freeze it into | `WordTokenizer` / `SegmentTokenizer` (zero-sized values) |
| `SentimentAnalyzer` with a stemmer | `SoundEx`, `Metaphone`, `DoubleMetaphone`, `DaitchMokotoff` |
| `SentenceTokenizer::with_abbreviations(…)` | `LevenshteinCosts` / `OsaCosts` / `DamerauCosts` (`Copy` structs, `const` constructors) |
| `StopWords::for_language(…)` | `NounInflector::new()` if you add no rules |
| `PhoneticIndex`, `FuzzyIndex`, `DeletionIndex` | `PreparedPattern`, if the pattern is the request |

## Bounding the work, not the allocations

The latency risk in an interactive path is almost never allocation. It is doing
`O(n)` work against a corpus of size `n`:

```rust
use verbora_distance::levenshtein;

/// Rank candidates by edit distance — but only after something else has
/// reduced `candidates` to a sane size.
fn best_match<'a>(query: &str, candidates: &[&'a str]) -> Option<&'a str> {
    candidates
        .iter()
        .map(|c| (*c, levenshtein(query, c)))
        // An exact `usize` count of edits, so ranking needs no float
        // comparator; ties keep the first candidate.
        .min_by_key(|&(_, d)| d)
        .map(|(c, _)| c)
}

assert_eq!(best_match("kitten", &["sitting", "mitten", "bitten"]), Some("mitten"));
```

If `candidates` is your whole dictionary, this is the wrong shape no matter how
fast `levenshtein` is. Narrow it first — see
[Fuzzy name matching](fuzzy-matching.md).

## Where the fallible APIs are

Interactive input is arbitrary, and the good news is that almost nothing on the
per-request path can reject it. **Fallibility in Verbora lives at construction
time, not per call.** Validate once at startup and the request path has no error
branch left to write.

```rust
use verbora_phonetics::SoundEx;
use verbora_inflectors::NounInflector;

let soundex = SoundEx::new();
let inflector = NounInflector::new();

// Every `process` is total: no `Result`, no panic, on any `&str`.
assert_eq!(soundex.process("hello"), "H400");

// An input with no letter the algorithm recognises yields an empty key —
// the absence of a key, not a value standing in for one.
assert_eq!(soundex.process("(#*"), "");

// Every inflector method is total too. A word with no matching rule comes
// back unchanged, and the empty token has no inflected form.
assert_eq!(inflector.pluralize("cactus"), "cacti");
assert_eq!(inflector.pluralize(""), "");
```

That means an unusable query token is a value you branch on — an empty key,
an unchanged word — rather than an error you have to map to a status code. Test
for it explicitly if an empty bucket key means something to your endpoint;
nothing will raise it for you.

The `Result`s you do have to handle are the ones you hit while building the
state above, all of them before the first request:

| Fallible call | Error | When |
|---|---|---|
| `SentenceTokenizer::with_abbreviations` | `AbbreviationError` | an abbreviation is the empty string, which would suppress every boundary |
| `Rule::new` | `RuleError` | a caller-supplied inflection pattern does not compile |
| `Corpus::build_lexicon` / `RuleSet::parse_lines` | `LexiconError` / `RuleSetParseError` | a tagger corpus or rule source is malformed |
| `TfIdf::from_json` | `RestoreError` | a persisted corpus does not match the schema |

`hamming` is the one per-call exception, and it is an `Option` rather than an
error: `None` when the two operands' scalar counts differ, so an incomparable
candidate drops out of a `filter_map` instead of scoring.

## Predictability

Two things can make a latency profile spiky:

**Input size you do not control.** `levenshtein` is `O(nm)`. A 100 kB paste into
a search box is 10¹⁰ cells — even at 64 cells per bitwise word, that is hundreds
of millions of operations. Cap the input length at the boundary.

**Non-ASCII promotion.** ASCII operands are compared as borrowed `&[u8]`, since
one ASCII byte is exactly one Unicode scalar; non-ASCII ones are decoded to
scalars first, into a `Vec<char>` — one allocation per operand. At 256 units,
`levenshtein/cyrillic` measures 3.97 µs † against ASCII's
2.13 µs † — both fast, but the difference is real and data-dependent.

† Pending re-measurement, and left as recorded rather than replaced with a
guess. See [Zero-copy](../performance/zero-copy.md#what-the-fast-path-is-worth).

## Checklist

- [ ] Expensive structures built at startup, shared behind `Arc`
- [ ] `StopWords` and compiled patterns hoisted out of loops
- [ ] Input length capped before it reaches an `O(nm)` metric
- [ ] Candidate sets narrowed before ranking
- [ ] Construction-time `Result`s handled at startup, not `unwrap()`ed there
- [ ] Empty phonetic keys and unchanged inflections treated as answers, not errors
- [ ] No `thread_local!` buffer pools until a profiler asks for one

## Related

- [Your first program](../getting-started/first-program.md)
- [Ergonomics vs throughput](../performance/ergonomics-vs-throughput.md)
- [Fuzzy name matching](fuzzy-matching.md)
