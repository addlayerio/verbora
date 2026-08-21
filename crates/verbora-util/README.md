# verbora-util

A grab-bag, and there is no unifying theme to claim: two unrelated things live
here because both are small, both are wanted by more than one part of
[Verbora](https://verbora.dev), and neither belongs to a single NLP subsystem.
The first is **linguistic data tables** — English and Spanish abbreviation lists
(`AbbreviationLanguage`), plus the sixteen stop-word lists re-exported from
`verbora-core` under this crate's older `Language` name — for suppressing
sentence breaks after `Dr.` and for filtering function words out of a token
stream. The second is **edge-weighted directed graphs**
(`EdgeWeightedDigraph`) with topological ordering (`Topological`) and
single-source shortest- and longest-path trees (`ShortestPathTree`,
`LongestPathTree`), which is what a dependency order, a critical path or a
lattice decoding needs.

## Contract

The graph algorithms are the classical ones for acyclic graphs — Kahn's
topological sort, and relaxation in topological order — cited on the types that
implement them, and each guarantee below is pinned by a test in the module that
makes it. **No sentinels:** absence is `Option::None`, never an "unreachable"
distance, a `-1` index or a magic `f64::MAX`. **No `NaN` and no infinities:**
`add` refuses a non-finite weight and a path total that would leave the finite
range fails with `PathError::Overflow`, so nothing returned here can poison a
comparison or a sort; a distance is the exact `f64` sum along the reported path,
rounded nowhere. **No panics** through the public API on any input a caller can
construct — every failure is an `Option` or a `Result`. The data tables do no
silent rewriting either: lookup is exact scalar-sequence comparison, so case
folding, trimming and normalization are the caller's explicit choice, and note
that `AbbreviationLanguage::contains` is a membership test over the list rather
than the tokenizer's suffix rule — `contains("casino.")` is `false` even though
the entry `"no."` would suppress a sentence boundary after it.

## Example

```rust
use verbora_util::{AbbreviationLanguage, EdgeWeightedDigraph, ShortestPathTree, Topological};

let mut g: EdgeWeightedDigraph<&str> = EdgeWeightedDigraph::new();
g.add(&"fetch", &"build", 2.0).unwrap();
g.add(&"build", &"test", 5.0).unwrap();
g.add(&"fetch", &"test", 9.0).unwrap();

assert_eq!(
    Topological::new(&g).unwrap().labels().copied().collect::<Vec<_>>(),
    ["fetch", "build", "test"],
);

let tree = ShortestPathTree::new(&g, &"fetch").unwrap();
assert_eq!(tree.distance_of(&"test"), Some(7.0));
assert_eq!(tree.path_labels_of(&"test"), Some(vec![&"fetch", &"build", &"test"]));

// Unreachable is `None`, never a sentinel distance.
assert_eq!(tree.distance_of(&"deploy"), None);

// The tables: exact comparison, nothing folded for you.
assert!(AbbreviationLanguage::En.contains("Dr."));
assert!(!AbbreviationLanguage::En.contains("dr."));
```

## See also

Full documentation: <https://verbora.dev/features/util>.

The stop-word lists themselves live in
[`verbora-core`](https://crates.io/crates/verbora-core) — one home, because data
with two homes drifts — and this crate only re-exports them, so depend on
`verbora-core` directly if the lists are all you want. The abbreviation lists
exist to be handed to `SentenceTokenizer::with_abbreviations` in
[`verbora-tokenizers`](https://crates.io/crates/verbora-tokenizers), which is
where the suffix rule they feed is specified. For a prefix-keyed structure
rather than a graph, see [`verbora-trie`](https://crates.io/crates/verbora-trie).
