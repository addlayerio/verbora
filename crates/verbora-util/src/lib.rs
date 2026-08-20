//! Shared data tables and graph algorithms for Verbora.
//!
//! Two things live here, kept together because both are small, both are needed
//! by more than one part of the toolkit, and neither belongs to a single NLP
//! subsystem:
//!
//! * **Linguistic data tables** — abbreviation lists for two languages
//!   ([`AbbreviationLanguage`]), plus the stop-word tables re-exported from
//!   `verbora-core` ([`Language`]). These are data, not algorithms: what
//!   Verbora specifies about them is the comparison used to look a word up and
//!   the shape every entry must have, both of which are enumerated entry by
//!   entry rather than sampled. The stop-word lists themselves live in
//!   `verbora-core` because two other crates need them and data with two homes
//!   drifts.
//! * **Edge-weighted directed graphs** ([`EdgeWeightedDigraph`]) with
//!   topological ordering ([`Topological`]) and single-source shortest- and
//!   longest-path trees ([`ShortestPathTree`], [`LongestPathTree`]). The
//!   algorithms are the classical ones for acyclic graphs — Kahn's topological
//!   sort, and relaxation in topological order — cited on the types that
//!   implement them.
//!
//! The crate root is the entire public surface; every module is private.
//!
//! # What this crate promises about numbers
//!
//! Every guarantee below is pinned by a test in the module that makes it.
//!
//! * **No sentinels.** Absence is [`Option::None`]. There is no "unreachable"
//!   distance value, no `-1` index and no magic `f64::MAX`.
//! * **No `NaN`, and no infinities.** [`EdgeWeightedDigraph::add`] refuses a
//!   non-finite weight, and a path total that would leave the finite range fails
//!   the build with [`PathError::Overflow`]. Nothing this crate returns can
//!   poison a comparison or a sort.
//! * **No silent rewriting.** A distance is the exact `f64` sum along the path
//!   the tree reports, rounded nowhere. Case folding, trimming and normalisation
//!   of a word are the caller's explicit choice, never this crate's.
//! * **No panics** through the public API on any input a caller can construct.
//!   Every failure is an [`Option`] or a [`Result`].
//!
//! ```
//! use verbora_util::{EdgeWeightedDigraph, ShortestPathTree, Topological};
//!
//! let mut g: EdgeWeightedDigraph<&str> = EdgeWeightedDigraph::new();
//! g.add(&"fetch", &"build", 2.0).unwrap();
//! g.add(&"build", &"test", 5.0).unwrap();
//! g.add(&"fetch", &"test", 9.0).unwrap();
//!
//! assert_eq!(
//!     Topological::new(&g).unwrap().labels().copied().collect::<Vec<_>>(),
//!     ["fetch", "build", "test"]
//! );
//!
//! let tree = ShortestPathTree::new(&g, &"fetch").unwrap();
//! assert_eq!(tree.distance_of(&"test"), Some(7.0));
//! assert_eq!(tree.path_labels_of(&"test"), Some(vec![&"fetch", &"build", &"test"]));
//! ```

mod abbreviations;
mod data;
mod graph;
mod paths;
mod topological;

pub use abbreviations::{
    ABBREVIATION_LANGUAGES, ABBREVIATIONS_EN, ABBREVIATIONS_ES, AbbreviationLanguage,
};
pub use graph::{DirectedEdge, EdgeWeightedDigraph, GraphError, VertexId};
pub use paths::{
    Longest, LongestPathTree, PathError, PathTree, Relaxation, Shortest, ShortestPathTree,
};
pub use topological::{Cycle, Topological};

/// The stop-word tables and the process-global English list, both owned by
/// `verbora-core`.
///
/// Re-exported rather than copied. The lists used to exist twice more — once in
/// this crate's `data.rs` and once in `verbora-stemmers` — with nothing but a
/// test holding the copies together; they now have one home, which is the crate
/// both of those depend on. [`Language`] is `verbora_core::StopWordLanguage`
/// under this crate's older name.
///
/// [`Language::En`] and the `*_global_*` functions are **not** the same list.
/// `Language::En` describes the shipped data and is a pure function of it;
/// the functions read and write a process-global list that anything in the
/// process can change. See [`Language`]'s "Choosing the Right API" table for
/// which one a given program wants.
pub use verbora_core::{
    STOPWORD_LANGUAGES as LANGUAGES, StopWordLanguage as Language, StopWords, add_global_stopword,
    add_global_stopwords, global_stopwords, is_global_stopword, remove_global_stopword,
    remove_global_stopwords, reset_global_stopwords,
};
