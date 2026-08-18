# Utility data and graph algorithms

`verbora-util` collects the shared utility APIs that are useful outside a
single NLP pipeline: stop words, abbreviation tables, edge-weighted directed
graphs, path trees, topological ordering, and pluggable storage backends.

## Graph example

```rust
use verbora_util::{EdgeWeightedDigraph, ShortestPathTree, Topological, Vertex};

fn main() {
    let mut graph = EdgeWeightedDigraph::new();
    graph.add(5, 4, 0.35);
    graph.add(4, 7, 0.37);
    graph.add(5, 7, 0.28);

    assert_eq!(Topological::new(&graph).unwrap().order().len(), 3);
    let paths = ShortestPathTree::new(&graph, 5).unwrap();
    assert_eq!(paths.dist_to(&Vertex::from(7)), Some(0.28));
}
```

## What to use

| Need | API |
|---|---|
| English or language-specific stop words | `stopwords` and `Language` |
| English or Spanish abbreviations | `ABBREVIATIONS_EN`, `ABBREVIATIONS_ES` |
| Weighted directed graph | `EdgeWeightedDigraph`, `DirectedEdge` |
| DAG ordering | `Topological` |
| Shortest or longest paths | `ShortestPathTree`, `LongestPathTree` |
| Replaceable persistence | `StorageBackend`, `StoragePlugin`, `FileBackend` |

Graph vertices preserve both numeric and string identity where the public
contract distinguishes them. Stored path distances are rounded to two decimal
places using the crate's documented formatting semantics.

For exact signatures and storage lifecycle behavior, use the
[Rust API reference](../reference/api.md).
