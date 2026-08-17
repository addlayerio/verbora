//! Utilities from the reference `util` module: stop-word and abbreviation data,
//! edge-weighted digraphs with their path trees, and pluggable object storage.
//!
//! Nine names are exported from `util/index` and hoisted to the top-level
//! reference namespace; all nine are here, plus `Bag`, which the index does not
//! re-export but `EdgeWeightedDigraph` is built out of.
//!
//! | the reference | here |
//! |---|---|
//! | the reference's `stopwords` array | [`stopwords`] (English via `verbora-core`) |
//! | the reference `abbreviations` | [`ABBREVIATIONS_EN`] |
//! | the reference `abbreviations_es` | [`ABBREVIATIONS_ES`] |
//! | the reference `DirectedEdge` | [`DirectedEdge`] |
//! | the reference `EdgeWeightedDigraph` | [`EdgeWeightedDigraph`] |
//! | the reference `Topological` | [`Topological`] |
//! | the reference `ShortestPathTree` | [`ShortestPathTree`] |
//! | the reference `LongestPathTree` | [`LongestPathTree`] |
//! | the reference `StorageBackend…` | [`storage`] |
//! | (`Bag`, internal) | [`Bag`] |
//!
//! # The graphs are more the reference-shaped than they look
//!
//! Three things in this corner of the reference are load-bearing and easy to
//! "correct" by accident:
//!
//! 1. **A vertex is a reference value used as a property key.** `1` and `'1'`
//!    address the same adjacency slot but are different vertices to
//!    `Topological`. [`Vertex`] keeps both halves.
//! 2. **`EdgeWeightedDigraph::v` is `adj.length`, not the vertex count.** One
//!    edge `10 -> 3` reports eleven vertices.
//! 3. **Distances are rounded to two decimals as they are stored**, via
//!    The reference's `toFixed`, which rounds ties away from zero where Rust rounds
//!    to even. [`numfmt`] reproduces that and the rest of `Number::toString`.
//!
//! Each is documented where it lives, together with what a naive port would have
//! produced instead.
//!
//! ```
//! use verbora_util::{EdgeWeightedDigraph, ShortestPathTree, Topological, Vertex};
//!
//! let mut g = EdgeWeightedDigraph::new();
//! g.add(5, 4, 0.35);
//! g.add(4, 7, 0.37);
//! g.add(5, 7, 0.28);
//!
//! assert_eq!(g.to_string(), "4 -> 7, 0.37\n5 -> 4, 0.35\n5 -> 7, 0.28");
//! assert_eq!(Topological::new(&g).unwrap().order().len(), 3);
//!
//! let tree = ShortestPathTree::new(&g, 5).unwrap();
//! assert_eq!(tree.dist_to(&Vertex::from(7)), Some(0.28));
//! ```

mod abbreviations;
mod bag;
mod data;
mod graph;
pub mod numfmt;
mod paths;
mod sparse;
pub mod stopwords;
pub mod storage;
mod topological;
mod vertex;

pub use abbreviations::{ABBREVIATIONS_EN, ABBREVIATIONS_ES, AbbreviationLanguage};
pub use bag::Bag;
pub use graph::{DirectedEdge, EdgeWeightedDigraph};
pub use paths::{Longest, LongestPathTree, PathTree, Relaxation, Shortest, ShortestPathTree};
pub use stopwords::Language;
pub use storage::{FileBackend, StorageBackend, StoragePlugin, StorageType};
pub use topological::{CyclicDependency, Topological};
pub use vertex::{Vertex, VertexKey};
