# Spellcheck and fuzzy indexes

`verbora-spellcheck` provides frequency-ranked spelling correction and two
indexes for repeated fuzzy lookup over a fixed dictionary.

## Quick example

```rust
use verbora_spellcheck::Spellcheck;

fn main() {
    let checker = Spellcheck::new(["the", "the", "the", "he", "he", "she"]);
    assert!(checker.is_correct("the"));
    assert_eq!(checker.get_corrections("he", 1), ["the", "he", "she"]);
}
```

Repeated words in the input corpus define frequency. Corrections are grouped by
edit distance and then ranked by frequency; a candidate one edit away always
precedes a candidate two edits away.

## Choosing an API

| Need | Use |
|---|---|
| Correct against corpus frequencies | `Spellcheck` |
| Inspect generated edits lazily | `Edits` / `edits_utf16` |
| Query arbitrary distances against a fixed dictionary | `FuzzyIndex` (BK-tree) |
| Optimize repeated queries with a known maximum distance | `DeletionIndex` |
| Correct many independent words | `Spellcheck::par_get_corrections_batch`, with `parallel` enabled |

`Spellcheck` uses a deletion index internally for distances up to two and falls
back to exact lazy edit generation for larger distances. `FuzzyIndex` and
`DeletionIndex` solve dictionary-neighbor lookup; they do not apply corpus
frequency ranking.

## Unicode and cost

Edit generation follows UTF-16 code-unit semantics. The number of possible
edits grows rapidly with distance, so avoid large distances on unrestricted
input. Build one of the indexes once when the same dictionary serves many
queries.

## Related

- [String distance](distance.md)
- [Trie](trie.md)
- [Fuzzy matching recipe](../recipes/fuzzy-matching.md)

