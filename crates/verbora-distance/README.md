# verbora-distance

String distance and similarity metrics: the Levenshtein family (Levenshtein,
unrestricted Damerau–Levenshtein and optimal string alignment, each in a
unit-cost and a weighted form, each with a *search* flavour that reports where
in a larger text the best match sits), Hamming, Jaro, Jaro–Winkler and
Sørensen–Dice. For anyone matching names, deduplicating records, ranking
fuzzy candidates or scoring how far a typo is from a word.

## The contract

**One Unicode scalar value (`char`) is one unit**, in every metric, for every
count — so `"a😀b"` is three units, and no metric folds case, trims, collapses
whitespace or normalises its input. Unit-cost distances return `usize`
(exact, `Ord`, `Hash`, usable as a map key); the weighted forms take a cost
set built through a `new` that returns `Result`, so an inadmissible cost set
is rejected before any metric sees it. `hamming` returns `Option<usize>` —
`None` when the operands hold different numbers of scalars, rather than a
sentinel carved out of the numeric range. **No function here panics on any
input under any cost set, and no `NaN` escapes**: `dice_coefficient`, `jaro`
and `jaro_winkler` decide their degenerate cases before the division that
would otherwise evaluate `0 / 0`, and each returns a finite value in
`0.0..=1.0` that is symmetric and exactly `1.0` for identical operands.

The algorithms are the published ones: Levenshtein (1966); Damerau (1964),
in the unrestricted form of Lowrance & Wagner (1975) — whose cost
precondition `DamerauCosts::new` enforces — and in the restricted
optimal-string-alignment form; Jaro, M. A. (1989), *Advances in record
linkage methodology as applied to matching the 1985 census of Tampa,
Florida*, JASA 84(406), 414–420; Winkler, W. E. (1990), *String comparator
metrics and enhanced decision rules in the Fellegi–Sunter model of record
linkage*, ASA Proceedings of the Section on Survey Research Methods,
354–359; and Dice, L. R. (1945), *Measures of the amount of ecologic
association between species*, Ecology 26(3), 297–302.

`PreparedPattern` is the shape for screening one fixed pattern against many
candidates: it builds the bit-parallel match table once instead of once per
call, and returns exactly what the free functions return.

## Example

```rust
use verbora_distance::{
    LevenshteinCosts, PreparedPattern, dice_coefficient, hamming, jaro_winkler, levenshtein,
    levenshtein_weighted,
};

assert_eq!(levenshtein("kitten", "sitting"), 3);
assert_eq!(dice_coefficient("abc", "abc"), 1.0);

// Hamming is undefined for operands of different lengths, and says so.
assert_eq!(hamming("karolin", "kathrin"), Some(3));
assert_eq!(hamming("karolin", "kathrine"), None);

// Winkler's boost rewards a shared prefix, not a shared suffix.
assert!(jaro_winkler("abcde", "abcdz") > jaro_winkler("abcde", "zbcde"));

// Weighted: a deletion priced at three insertions. The cost set is validated
// at construction, so the metric itself cannot be handed something invalid.
let costs = LevenshteinCosts::new(1.0, 3.0, 1.0).expect("admissible costs");
assert_eq!(levenshtein_weighted("abc", "ab", &costs), 3.0);

// One pattern, many candidates: build the match table once, then screen.
let query = PreparedPattern::new("Jonathan");
let within_two: Vec<&str> = ["Jonathon", "Nathan", "Johnson", "Jon"]
    .into_iter()
    .filter(|c| query.levenshtein(c) <= 2)
    .collect();
assert_eq!(within_two, ["Jonathon"]);
```

## See also

Full documentation, including which metric to reach for and what each one
costs: <https://verbora.dev/features/distance>. Measured results:
<https://verbora.dev/benchmarks/>.

If what you actually wanted was a *dictionary* rather than a pair of strings,
`verbora-spellcheck` is the crate that ranks corrections and retrieves
neighbours within an edit distance without scanning the corpus; for matching
words that *sound* alike rather than that are spelled alike, see
`verbora-phonetics`; for prefix and autocomplete lookups, `verbora-trie`.
