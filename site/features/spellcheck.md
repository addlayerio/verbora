# Spellcheck and fuzzy indexes

`verbora-spellcheck` provides frequency-ranked spelling correction and two
indexes for repeated fuzzy lookup over a fixed dictionary.

## Quick example

```rust
use verbora_spellcheck::Spellcheck;

fn main() {
    let checker = Spellcheck::new(["the", "the", "the", "he", "he", "she", "th"]);
    assert!(checker.is_correct("the"));
    assert_eq!(checker.correction_words("he", 1), ["he", "the", "she"]);
}
```

Repeated words in the input corpus define frequency. Corrections come back in
ascending edit distance first — a word reachable in `k` edits precedes any word
needing `k + 1`, however frequent — then descending corpus frequency, then
ascending word. That third key makes the order total, so the sequence is fully
determined by the corpus and the query.

## One metric, one unit

Every distance in this crate is
[`damerau_levenshtein`](distance.md) — unrestricted Damerau–Levenshtein, unit
cost, counted in **Unicode scalar values**. Three consequences follow:

- **A transposition costs one edit**, so `"hte"` corrects to `"the"` at
  `max_distance = 1`.
- **There is no alphabet.** A correction is not restricted to some fixed letter
  set: `"cafe"` corrects to `"café"` and `"Мсква"` to `"Москва"`, in any script,
  with no configuration.
- **An astral scalar is one unit.** Deleting an emoji is one edit, not two.

`corrections(query, k)` is defined by that metric rather than by an enumeration
procedure: it returns every corpus word within `k` edits and nothing else, which
is the same set a brute-force scan would produce.

## Choosing an API

| Need | Use |
|---|---|
| Membership, nothing more | `Spellcheck::is_correct` |
| One suggestion | `Spellcheck::best_correction` |
| A ranked list with the distance and frequency behind the ranking | `Spellcheck::corrections` |
| The ranked words alone, owned | `Spellcheck::correction_words` |
| Query arbitrary distances against a fixed dictionary | `FuzzyIndex` (BK-tree) |
| Repeated queries at one small, known maximum distance | `DeletionIndex` |
| Correct many independent words | `Spellcheck::par_corrections_batch`, with `parallel` enabled |

`Spellcheck` is the one that ranks. The two indexes are candidate generation —
a blocking step, not a search engine: they do not rank, do not pick a best
match, and apply no corpus frequency. Each neighbour arrives with its exact
distance already computed, so ranking at the call site is one sort and no
recomputation.

```rust
use verbora_spellcheck::{DeletionIndexBuilder, FuzzyIndexBuilder};

fn main() {
    let mut fuzzy = FuzzyIndexBuilder::new();
    fuzzy.insert_all(["kitten", "sitting", "mitten"]);
    let fuzzy = fuzzy.build();

    let mut deletion = DeletionIndexBuilder::new(2);
    deletion.insert_all(["kitten", "sitting", "mitten"]);
    let deletion = deletion.build();

    // The same set, reached two ways.
    let mut a: Vec<&str> = fuzzy.neighbors("kitten", 2).map(|n| n.word).collect();
    let mut b: Vec<&str> = deletion
        .neighbors("kitten", 2)
        .expect("2 is the index's ceiling")
        .map(|n| n.word)
        .collect();
    a.sort_unstable();
    b.sort_unstable();
    assert_eq!(a, b);

    // Only the BK-tree can be asked for more after the fact.
    assert_eq!(fuzzy.neighbors("kitten", 3).count(), 3);
    assert!(deletion.neighbors("kitten", 3).is_err());
}
```

`DeletionIndex` fixes its ceiling at build time, so a query beyond it is a
`DistanceBeyondIndex` error rather than a silently short answer.

## Cost

`Spellcheck` builds a symmetric-delete index lazily, on the first query at
`max_distance <= 2`, and only the handful of corpus words sharing a deletion
sequence with the query have their distance computed. Above 2 the query falls
back to a scan that skips any word whose scalar length already differs by more
than `max_distance` and computes the distance for the rest. Depth 3 is not
indexed because the structure would be larger than the corpus it indexes.

The number of candidate edits grows rapidly with distance, so avoid large
distances on unrestricted input, and build one of the indexes once when the same
dictionary serves many queries.

**No speed figures are published for this crate.** No measurement describes the
code as it now stands, so what is documented instead is the work each path does
and what it allocates.

## Related

- [String distance](distance.md)
- [Trie](trie.md)
- [Fuzzy matching recipe](../recipes/fuzzy-matching.md)
