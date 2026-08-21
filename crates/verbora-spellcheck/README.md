# verbora-spellcheck

Norvig-style spelling correction: give it a corpus, ask it what a misspelling
should have been, and get back a ranked list of candidates with the numbers
behind the ranking. Alongside it are two pure candidate-generation indexes
that answer the related but different question *which stored words are within
`k` edits of this one?* — the blocking step in front of a matcher, a
deduplicator or a search box.

## Three shapes, three questions

| Type | Answers | `max_distance` fixed | Built for |
|---|---|---|---|
| `Spellcheck` | *what should this misspelling have been?* | per query | one corpus with frequencies; **it is the one that ranks** |
| `FuzzyIndex` | *which stored words are within `k` of this one?* | per query | repeated queries at varying `k` (a BK-tree) |
| `DeletionIndex` | the same question | at **build** time | repeated queries at one small, known `k` |

The two indexes do not rank, do not pick a best match and do not accept a
query language; each neighbour comes back with its exact distance already
computed, so ranking at the call site is one sort and no recomputation.
Asking a `DeletionIndex` for more than its build-time ceiling is an error,
not a silently truncated answer.

## The contract

Everything here measures with exactly one metric: **unrestricted
Damerau–Levenshtein at unit cost, counted in Unicode scalar values** (see
`verbora-distance`). Unrestricted rather than optimal-string-alignment
because `FuzzyIndex`'s BK-tree pruning is only correct for a true metric;
Damerau rather than plain Levenshtein because a transposed adjacent pair is
the commonest typo class there is and should cost one edit rather than two.
There is no cost parameter and no configurable alphabet anywhere, so a
correction is never restricted to some fixed letter set — `"cafe"` corrects
to `"café"` and `"Мсква"` to `"Москва"`, in any script, with no
configuration. Candidate generation and verification are pinned to agree on
what one unit is; `tests/completeness.rs` verifies that exhaustively.

## Example

```rust
use verbora_spellcheck::{DeletionIndexBuilder, FuzzyIndexBuilder, Spellcheck};

// The word list is a corpus: repeats are frequencies, and frequency breaks
// ties within an edit distance.
let sc = Spellcheck::new(["the", "the", "the", "he", "he", "she", "th"]);
assert!(sc.is_correct("the"));
assert_eq!(sc.correction_words("he", 1), ["he", "the", "she"]);

// Candidate generation, two ways over the same words.
let mut fuzzy = FuzzyIndexBuilder::new();
fuzzy.insert_all(["kitten", "sitting", "mitten"]);
let fuzzy = fuzzy.build();

let mut deletion = DeletionIndexBuilder::new(2); // ceiling fixed at build
deletion.insert_all(["kitten", "sitting", "mitten"]);
let deletion = deletion.build();

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
```

## See also

Full documentation, including which of the three to reach for and what each
allocates: <https://verbora.dev/features/spellcheck>.

For the raw metrics themselves — including the weighted forms and the
one-pattern-against-many `PreparedPattern` — see `verbora-distance`. For
matching words that *sound* alike rather than that are spelled alike, see
`verbora-phonetics`; for prefix and autocomplete lookups, `verbora-trie`.
