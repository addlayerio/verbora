# Utility data and graph algorithms

`verbora-util` collects the shared utility APIs that are useful outside a
single NLP pipeline: stop words, abbreviation tables, edge-weighted directed
graphs, topological ordering, and shortest- and longest-path trees.

## Graph example

```rust
use verbora_util::{EdgeWeightedDigraph, ShortestPathTree, Topological};

fn main() {
    let mut graph: EdgeWeightedDigraph<&str> = EdgeWeightedDigraph::new();
    graph.add(&"fetch", &"build", 2.0).unwrap();
    graph.add(&"build", &"test", 5.0).unwrap();
    graph.add(&"fetch", &"test", 9.0).unwrap();

    assert_eq!(
        Topological::new(&graph).unwrap().labels().copied().collect::<Vec<_>>(),
        ["fetch", "build", "test"]
    );

    let tree = ShortestPathTree::new(&graph, &"fetch").unwrap();
    assert_eq!(tree.distance_of(&"test"), Some(7.0));
    assert_eq!(
        tree.path_labels_of(&"test"),
        Some(vec![&"fetch", &"build", &"test"])
    );
}
```

## What to use

| Need | API |
|---|---|
| English or language-specific stop words | `Language`, `StopWords` |
| English or Spanish abbreviations | `AbbreviationLanguage`, `ABBREVIATIONS_EN`, `ABBREVIATIONS_ES` |
| Weighted directed graph | `EdgeWeightedDigraph`, `DirectedEdge`, `VertexId` |
| DAG ordering | `Topological` |
| Shortest or longest paths | `ShortestPathTree`, `LongestPathTree` |

## The vertex model

A vertex is whatever label you name it by — `&str`, `String`, `u32`, any
`Eq + Hash + Clone` type. `add` mints a `VertexId` for a label the first time it
sees one and reuses it afterwards, so identity is the label's own equality and
nothing else: `5` and `"5"` are different vertices because they are different
values, not because the graph applies a rule of its own. Every result comes back
addressable both ways — `distance` / `path` take a `VertexId`, `distance_of` /
`path_of` / `path_labels_of` take a label.

## What the numbers mean

- **Nothing is rounded.** A distance is the exact `f64` sum of the weights along
  the path the tree reports.
- **Absence is `None`.** There is no unreachable-distance sentinel, no `-1`
  index and no magic `f64::MAX`.
- **No `NaN` and no infinities escape.** `add` refuses a non-finite weight with
  `GraphError::NonFiniteWeight`, and a path total that would leave the finite
  range fails the build with `PathError::Overflow` naming the vertex. Nothing
  this crate returns can poison a comparison or a sort.
- **Building a tree over a cyclic graph is an error**, not a partial answer:
  `PathError::Cyclic` carries the `Cycle` that was found.

## Abbreviations and stop words

Both are data, and both are compared as Unicode scalar sequences with no case
folding, no normalisation and no trimming. `AbbreviationLanguage::contains` is
an exact-membership test over the list; it is deliberately *not* the suffix rule
`verbora-tokenizers`' sentence tokenizer applies when it consumes these lists.
The stop-word tables are re-exported from `verbora-core`, which owns them, so
the list has exactly one home.

For exact signatures, see the [Rust API reference](../reference/api.md).
