//! Single-source shortest- and longest-path trees over a directed acyclic
//! graph.

use std::borrow::Borrow;
use std::fmt;
use std::hash::Hash;
use std::marker::PhantomData;

use crate::graph::{DirectedEdge, EdgeWeightedDigraph, VertexId};
use crate::topological::{Cycle, Topological};

mod sealed {
    pub trait Sealed {}
}

/// Which extremum a [`PathTree`] optimises for.
///
/// Sealed. The two implementations are [`Shortest`] and [`Longest`]; a third
/// would have to define what "better" means for a tree, and every guarantee in
/// this module is stated in terms of these two.
pub trait Relaxation: sealed::Sealed {
    /// Whether `candidate` is a better distance than the `incumbent` a vertex
    /// already holds.
    ///
    /// Only ever called with two finite values: a vertex with no distance yet is
    /// handled by the caller, and non-finite candidates are rejected before the
    /// comparison. There is therefore no `NaN` case to reason about.
    fn is_better(incumbent: f64, candidate: f64) -> bool;
}

/// Minimises the total weight along a path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shortest;

impl sealed::Sealed for Shortest {}

impl Relaxation for Shortest {
    fn is_better(incumbent: f64, candidate: f64) -> bool {
        candidate < incumbent
    }
}

/// Maximises the total weight along a path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Longest;

impl sealed::Sealed for Longest {}

impl Relaxation for Longest {
    fn is_better(incumbent: f64, candidate: f64) -> bool {
        candidate > incumbent
    }
}

/// Why a [`PathTree`] could not be built.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PathError {
    /// The source label is not a vertex of the graph.
    ///
    /// Distinguished from "the source has no outgoing edges", which is not an
    /// error: that tree is simply `{source: 0.0}`.
    UnknownSource,
    /// The graph is not acyclic, so no relaxation order exists.
    Cyclic(Cycle),
    /// A path's running total left the finite `f64` range.
    ///
    /// Every edge weight is finite by construction, but a sum of finite values
    /// need not be: a two-edge path of weight `f64::MAX` each overflows to
    /// `+inf`. Rather than let an infinity escape as a distance — where it would
    /// poison every later comparison exactly as a `NaN` would — the build fails
    /// and names the vertex whose distance could not be represented.
    Overflow(VertexId),
}

impl fmt::Display for PathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownSource => f.write_str("source vertex is not in the graph"),
            Self::Cyclic(cycle) => write!(f, "{cycle}"),
            Self::Overflow(id) => {
                write!(f, "distance to vertex {id} left the finite range of f64",)
            }
        }
    }
}

impl std::error::Error for PathError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Cyclic(cycle) => Some(cycle),
            _ => None,
        }
    }
}

impl From<Cycle> for PathError {
    fn from(cycle: Cycle) -> Self {
        Self::Cyclic(cycle)
    }
}

/// The shortest-path tree of a directed acyclic graph.
///
/// ```
/// use verbora_util::{EdgeWeightedDigraph, ShortestPathTree};
///
/// let mut g: EdgeWeightedDigraph<u32> = EdgeWeightedDigraph::new();
/// g.add(&5, &4, 0.35).unwrap();
/// g.add(&4, &0, 0.38).unwrap();
/// g.add(&5, &1, 0.32).unwrap();
///
/// let tree = ShortestPathTree::new(&g, &5).unwrap();
/// // 0.35 + 0.38, reported exactly as f64 addition produces it.
/// assert_eq!(tree.distance_of(&0), Some(0.35 + 0.38));
/// assert_eq!(tree.path_labels_of(&0), Some(vec![&5, &4, &0]));
///
/// // The source is reachable from itself by the empty path.
/// assert_eq!(tree.distance_of(&5), Some(0.0));
/// assert_eq!(tree.path_labels_of(&5), Some(vec![&5]));
///
/// // A vertex the source cannot reach has no distance and no path.
/// assert_eq!(tree.distance_of(&1), Some(0.32));
/// assert_eq!(tree.distance_of(&99), None);
/// ```
pub type ShortestPathTree<'g, V> = PathTree<'g, V, Shortest>;

/// The longest-path tree of a directed acyclic graph.
///
/// Longest paths are tractable here precisely because the graph is acyclic —
/// the same relaxation in topological order that solves the shortest-path
/// problem solves this one with the comparison reversed. Negative weights are
/// as welcome as positive ones in both directions.
///
/// ```
/// use verbora_util::{EdgeWeightedDigraph, LongestPathTree};
///
/// let mut g: EdgeWeightedDigraph<u32> = EdgeWeightedDigraph::new();
/// g.add(&0, &1, 0.5).unwrap();
/// g.add(&1, &2, 0.25).unwrap();
/// g.add(&0, &2, 0.1).unwrap();
///
/// let tree = LongestPathTree::new(&g, &0).unwrap();
/// assert_eq!(tree.distance_of(&2), Some(0.75));
/// assert_eq!(tree.path_labels_of(&2), Some(vec![&0, &1, &2]));
/// ```
pub type LongestPathTree<'g, V> = PathTree<'g, V, Longest>;

/// A single-source path tree over a directed acyclic graph.
///
/// Use the aliases [`ShortestPathTree`] and [`LongestPathTree`] rather than
/// naming this type directly.
///
/// # The contract
///
/// * **A distance is present exactly when a path exists.**
///   [`PathTree::distance`] is `Some` for the source and for every vertex
///   reachable from it, and `None` for every other vertex — including one that
///   is in the graph but in another component. No sentinel value stands in for
///   absence.
/// * **Every reported distance is finite.** Weights are finite by construction
///   ([`crate::GraphError::NonFiniteWeight`]) and a running total that would
///   leave the finite range fails the build with [`PathError::Overflow`], so
///   neither `NaN` nor an infinity can be observed through this type.
/// * **A distance is the exact `f64` sum along the reported path**, accumulated
///   in relaxation order and rounded nowhere. `0.28 + 0.34` is reported as
///   `0.6200000000000001`, because that is what the arithmetic yields; rounding
///   a stored distance to a display precision would compound along the path and
///   make the tree disagree with its own edges.
/// * **A reported path exists.** [`PathTree::path`] returns `None` when the
///   target is unreachable and never manufactures a prefix. For the source it is
///   `Some(vec![source])` — the empty path, of length zero.
/// * **Ties keep the edge added first.** When two paths to a vertex tie
///   exactly, the tree arrives by whichever of their final edges was added to
///   the graph first — the lower index in [`EdgeWeightedDigraph::edges`]. This
///   is a property of the graph alone, not of the traversal: it does *not*
///   mean "relaxed first". Relaxation visits tails in topological order, so an
///   edge added first is often relaxed after one added later, and the rule is
///   enforced by comparing edge indices on an exact tie rather than left to
///   fall out of the visit order. Applied at every vertex, it also pins the
///   whole path, since each step of [`PathTree::path`] is one such choice.
///
/// Building is O(V + E) after the topological ordering it delegates to, and
/// holds two `Vec`s of length V.
///
/// # Choosing the right API
///
/// Every query comes in an id form and a label form. The id form is the
/// primitive — one indexed read; the label form is the id form preceded by one
/// hash lookup. Neither is "the fast one" in a way that should decide a design:
/// use whichever matches the value you are holding, and only reach for ids
/// deliberately when a hot loop already has them.
///
/// | Question | With a [`VertexId`] | With a label | Allocates |
/// |---|---|---|---|
/// | Build the tree | [`PathTree::from_id`] | [`PathTree::new`] | two `Vec`s of length V |
/// | How far is it? | [`PathTree::distance`] | [`PathTree::distance_of`] | no |
/// | By which edge did the tree arrive? | [`PathTree::edge_to`] | — | no |
/// | What is the whole path? | [`PathTree::path`] | [`PathTree::path_of`] | one `Vec` per call |
/// | The whole path, as my labels | [`PathTree::path_labels`] | [`PathTree::path_labels_of`] | one `Vec` per call |
///
/// A decision tree, for the common cases:
///
/// * **You have a label and want one number.** [`PathTree::distance_of`]. This
///   is the ordinary case and the right default.
/// * **You are walking [`crate::Topological::order`] or
///   [`EdgeWeightedDigraph::edges`].** You already hold ids; use
///   [`PathTree::distance`] and [`PathTree::path`] and skip the hashing.
/// * **You want to show a route to a human.** [`PathTree::path_labels_of`].
/// * **You want one step, not a route** — "which edge feeds this vertex?" —
///   [`PathTree::edge_to`]. Building a whole path to read its last edge is the
///   mistake this method exists to prevent, and on a long chain it is the
///   difference between O(1) and O(path length) plus an allocation.
///
/// There is deliberately no `path_into(&mut Vec<_>)`. The path length is bounded
/// by the vertex count, one allocation per call is the whole cost, and no
/// benchmark in this crate shows a workload that repeats the call often enough
/// for a reusable buffer to pay for the `clear()` a caller must remember.
#[derive(Debug, Clone)]
pub struct PathTree<'g, V, R: Relaxation> {
    graph: &'g EdgeWeightedDigraph<V>,
    /// Best known distance per vertex id; `None` until a path is found.
    dist: Vec<Option<f64>>,
    /// Index into `graph.edges()` of the edge the tree arrives by.
    edge_to: Vec<Option<u32>>,
    source: VertexId,
    _mode: PhantomData<fn() -> R>,
}

impl<'g, V, R: Relaxation> PathTree<'g, V, R> {
    /// The source vertex.
    pub fn source(&self) -> VertexId {
        self.source
    }

    /// The graph this tree was built from.
    pub fn graph(&self) -> &'g EdgeWeightedDigraph<V> {
        self.graph
    }

    /// The distance to `id`, or `None` if no path reaches it.
    ///
    /// The primitive: one indexed read, no hashing. `Some(0.0)` for the source.
    pub fn distance(&self, id: VertexId) -> Option<f64> {
        self.dist.get(id.index()).copied().flatten()
    }

    /// The final edge of the tree's path to `id`, or `None` for the source and
    /// for an unreachable vertex.
    ///
    /// This is the one-step primitive [`PathTree::path`] walks. Reach for it
    /// when a single question — "how did the tree get here?" — is all you need,
    /// rather than materialising a whole path to ask it.
    ///
    /// Among the in-edges that achieve `id`'s reported distance, this is
    /// always the one added to the graph first — see the tie-break bullet on
    /// [`PathTree`].
    pub fn edge_to(&self, id: VertexId) -> Option<&'g DirectedEdge> {
        let index = self.edge_to.get(id.index()).copied().flatten()?;
        self.graph.edges().get(index as usize)
    }

    /// The path from the source to `id` as ids, source first, or `None` if no
    /// path reaches it.
    ///
    /// `Some(vec![source])` when `id` is the source. The returned vector is
    /// always non-empty when it is `Some`.
    pub fn path(&self, id: VertexId) -> Option<Vec<VertexId>> {
        self.distance(id)?;
        let mut path = vec![id];
        let mut current = id;
        // Bounded by the vertex count: `edge_to` is a tree over the vertices
        // relaxed in topological order, so the walk strictly decreases in
        // topological position and cannot revisit a vertex. The bound exists so
        // that a future change turning that into a logic error is a wrong
        // answer rather than a hang.
        for _ in 0..self.graph.vertex_count() {
            let Some(edge) = self.edge_to(current) else {
                break;
            };
            current = edge.from();
            path.push(current);
        }
        path.reverse();
        Some(path)
    }

    /// The path from the source to `id` as labels, source first.
    ///
    /// As [`PathTree::path`], with one indexed label lookup per vertex.
    pub fn path_labels(&self, id: VertexId) -> Option<Vec<&'g V>> {
        // The `?` is load-bearing, and is the whole reason the lookup below
        // cannot fail. `path` starts with `self.distance(id)?`, which indexes
        // `dist` — sized `graph.vertex_count()` at construction — so an
        // out-of-range id leaves through the `?` before any label is read, and
        // every other id on the returned path is an `edge.from()` of this same
        // graph. Note what is *not* true: `VertexId` is public and `Copy`, and
        // `graph.rs` documents that an id from another graph resolves silently
        // to whatever sits at that index, so "minted by this graph" is not
        // enforced anywhere. Mapping `label()` over caller-supplied ids without
        // going through `path` would turn this into a live panic in safe public
        // API.
        Some(
            self.path(id)?
                .into_iter()
                .map(|v| {
                    self.graph
                        .label(v)
                        .expect("in range: `path` bounds-checked it")
                })
                .collect(),
        )
    }
}

impl<'g, V: Eq + Hash + Clone, R: Relaxation> PathTree<'g, V, R> {
    /// Builds the tree by relaxing every vertex in topological order.
    ///
    /// # Errors
    ///
    /// [`PathError::UnknownSource`] if `source` is not a vertex of `graph`;
    /// [`PathError::Cyclic`] if the graph is not acyclic;
    /// [`PathError::Overflow`] if a running total leaves the finite `f64` range.
    pub fn new<Q>(graph: &'g EdgeWeightedDigraph<V>, source: &Q) -> Result<Self, PathError>
    where
        V: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        let source = graph.vertex_id(source).ok_or(PathError::UnknownSource)?;
        Self::from_id(graph, source)
    }

    /// As [`PathTree::new`], from an id this graph already minted.
    ///
    /// Skips the label lookup. Use it when the id is already in hand — from
    /// [`crate::Topological::order`], or from an edge — and `new` when you have
    /// a label.
    ///
    /// # Errors
    ///
    /// As [`PathTree::new`]. An id this graph never minted is
    /// [`PathError::UnknownSource`].
    pub fn from_id(graph: &'g EdgeWeightedDigraph<V>, source: VertexId) -> Result<Self, PathError> {
        if source.index() >= graph.vertex_count() {
            return Err(PathError::UnknownSource);
        }
        let n = graph.vertex_count();
        let mut dist: Vec<Option<f64>> = vec![None; n];
        let mut edge_to: Vec<Option<u32>> = vec![None; n];
        dist[source.index()] = Some(0.0);

        let order = Topological::new(graph)?;
        for &tail in order.order() {
            // A vertex with no distance yet has no path from the source, so
            // nothing leaving it can extend one.
            let Some(tail_dist) = dist[tail.index()] else {
                continue;
            };
            for &index in graph.adjacent_indices(tail) {
                let edge = graph.edge_at(index);
                let head = edge.to().index();
                let candidate = tail_dist + edge.weight();
                if !candidate.is_finite() {
                    return Err(PathError::Overflow(edge.to()));
                }
                let better = match dist[head] {
                    None => true,
                    // On an exact tie, the smaller edge index wins — the
                    // edge added first, per the type's tie-break contract.
                    // Relaxation order alone would not give that: it visits
                    // tails in topological order, so an edge added first can
                    // be relaxed after one added later and lose a tie it
                    // should win. The comparison costs nothing on the common
                    // path, since `is_better` short-circuits it.
                    Some(incumbent) => {
                        R::is_better(incumbent, candidate)
                            || (candidate == incumbent
                                && edge_to[head].is_some_and(|kept| index < kept))
                    }
                };
                if better {
                    dist[head] = Some(candidate);
                    edge_to[head] = Some(index);
                }
            }
        }

        Ok(Self {
            graph,
            dist,
            edge_to,
            source,
            _mode: PhantomData,
        })
    }

    /// The distance to the vertex labelled `label`, or `None` if there is no
    /// such vertex or no path to it.
    ///
    /// One hash lookup on top of [`PathTree::distance`].
    pub fn distance_of<Q>(&self, label: &Q) -> Option<f64>
    where
        V: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        self.distance(self.graph.vertex_id(label)?)
    }

    /// The path to the vertex labelled `label`, as ids.
    ///
    /// One hash lookup on top of [`PathTree::path`].
    pub fn path_of<Q>(&self, label: &Q) -> Option<Vec<VertexId>>
    where
        V: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        self.path(self.graph.vertex_id(label)?)
    }

    /// The path to the vertex labelled `label`, as labels.
    ///
    /// One hash lookup on top of [`PathTree::path_labels`].
    pub fn path_labels_of<Q>(&self, label: &Q) -> Option<Vec<&'g V>>
    where
        V: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        self.path_labels(self.graph.vertex_id(label)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::sample_dag;

    fn graph(edges: &[(u32, u32, f64)]) -> EdgeWeightedDigraph<u32> {
        let mut g = EdgeWeightedDigraph::new();
        for &(a, b, w) in edges {
            g.add(&a, &b, w).unwrap();
        }
        g
    }

    fn id(g: &EdgeWeightedDigraph<u32>, label: u32) -> VertexId {
        g.vertex_id(&label).expect("label is a vertex of the graph")
    }

    fn labels(path: Option<Vec<&u32>>) -> Vec<u32> {
        path.expect("a path").into_iter().copied().collect()
    }

    /// The sum of the weights along a path of labels, in path order.
    fn weight_along(g: &EdgeWeightedDigraph<u32>, path: &[u32]) -> f64 {
        let mut total = 0.0;
        for pair in path.windows(2) {
            let from = g.vertex_id(&pair[0]).unwrap();
            let to = g.vertex_id(&pair[1]).unwrap();
            // The relaxation kept one specific edge; take the one the tree could
            // have used with the extremal weight for the mode under test. For
            // these fixtures every consecutive pair has exactly one edge.
            let edge = g
                .edges_from(from)
                .find(|e| e.to() == to)
                .expect("an edge between consecutive path vertices");
            total += edge.weight();
        }
        total
    }

    /// The defect that motivated removing the two-decimal snap: a stored
    /// distance that had been rounded no longer equalled the sum along the path
    /// the same tree reported.
    #[test]
    fn a_distance_equals_the_sum_along_its_own_path() {
        let g = graph(&[(0, 1, 0.125), (1, 2, 0.125)]);
        let tree = ShortestPathTree::new(&g, &0).unwrap();
        // 0.125 and 0.25 are both exact in binary, so this is not a tolerance.
        assert_eq!(tree.distance_of(&2), Some(0.25));
        assert_eq!(labels(tree.path_labels_of(&2)), vec![0, 1, 2]);
        assert_eq!(weight_along(&g, &[0, 1, 2]), 0.25);

        // ...and the same holds where the arithmetic is *not* exact: the
        // reported value is the f64 sum, not a rounding of it.
        let g = graph(&[(0, 1, 0.28), (1, 2, 0.34)]);
        let tree = ShortestPathTree::new(&g, &0).unwrap();
        assert_eq!(tree.distance_of(&2), Some(0.28 + 0.34));
        assert_ne!(tree.distance_of(&2), Some(0.62));
    }

    /// Enumerated over the whole sample graph, both modes: the tree's own
    /// distance must equal the sum along the tree's own path, for every vertex.
    #[test]
    fn every_vertex_agrees_with_its_own_path_in_both_modes() {
        let g = sample_dag();
        let shortest = ShortestPathTree::new(&g, &5).unwrap();
        let longest = LongestPathTree::new(&g, &5).unwrap();
        let mut checked = 0;
        for (id, _) in g.vertices() {
            for (distance, path) in [
                (shortest.distance(id), shortest.path_labels(id)),
                (longest.distance(id), longest.path_labels(id)),
            ] {
                assert_eq!(
                    distance.is_some(),
                    path.is_some(),
                    "a distance and a path must be present together"
                );
                let (Some(distance), Some(path)) = (distance, path) else {
                    continue;
                };
                let path: Vec<u32> = path.into_iter().copied().collect();
                assert_eq!(*path.first().unwrap(), 5, "path must start at the source");
                assert_eq!(
                    distance,
                    weight_along(&g, &path),
                    "distance vs path {path:?}"
                );
                assert!(distance.is_finite());
                checked += 1;
            }
        }
        assert_eq!(checked, 16, "8 vertices x 2 modes");
    }

    #[test]
    fn shortest_distances_on_the_sample_graph() {
        let g = sample_dag();
        let tree = ShortestPathTree::new(&g, &5).unwrap();
        // Each expectation is the arithmetic of the path named beside it.
        assert_eq!(tree.distance_of(&5), Some(0.0)); // the source
        assert_eq!(tree.distance_of(&1), Some(0.32)); // 5->1
        assert_eq!(tree.distance_of(&4), Some(0.35)); // 5->4
        assert_eq!(tree.distance_of(&7), Some(0.28)); // 5->7
        assert_eq!(tree.distance_of(&3), Some(0.32 + 0.29)); // 5->1->3
        assert_eq!(tree.distance_of(&0), Some(0.35 + 0.38)); // 5->4->0
        assert_eq!(tree.distance_of(&2), Some(0.28 + 0.34)); // 5->7->2
        assert_eq!(tree.distance_of(&6), Some(0.32 + 0.29 + 0.52)); // 5->1->3->6

        assert_eq!(labels(tree.path_labels_of(&0)), vec![5, 4, 0]);
        assert_eq!(labels(tree.path_labels_of(&2)), vec![5, 7, 2]);
        assert_eq!(labels(tree.path_labels_of(&6)), vec![5, 1, 3, 6]);
        assert_eq!(labels(tree.path_labels_of(&5)), vec![5]);
    }

    #[test]
    fn longest_distances_on_the_sample_graph() {
        let g = sample_dag();
        let tree = LongestPathTree::new(&g, &5).unwrap();
        assert_eq!(tree.distance_of(&5), Some(0.0));
        assert_eq!(tree.distance_of(&1), Some(0.32)); // 5->1
        assert_eq!(tree.distance_of(&3), Some(0.32 + 0.29)); // 5->1->3
        assert_eq!(tree.distance_of(&6), Some(0.32 + 0.29 + 0.52)); // 5->1->3->6
        assert_eq!(tree.distance_of(&4), Some(0.32 + 0.29 + 0.52 + 0.93)); // ...->6->4
        assert_eq!(
            tree.distance_of(&7),
            Some(0.32 + 0.29 + 0.52 + 0.93 + 0.37) // ...->4->7
        );
        assert_eq!(
            tree.distance_of(&0),
            Some(0.32 + 0.29 + 0.52 + 0.93 + 0.38) // ...->4->0
        );
        assert_eq!(
            tree.distance_of(&2),
            Some(0.32 + 0.29 + 0.52 + 0.93 + 0.37 + 0.34) // ...->7->2
        );
        assert_eq!(labels(tree.path_labels_of(&2)), vec![5, 1, 3, 6, 4, 7, 2]);
    }

    /// The source is trivially reachable from itself, by the path of no edges.
    #[test]
    fn the_source_is_reachable_from_itself() {
        let g = graph(&[(0, 1, 1.0)]);
        for tree in [
            &ShortestPathTree::new(&g, &0).unwrap() as &dyn ReachesItself,
            &LongestPathTree::new(&g, &0).unwrap(),
        ] {
            assert_eq!(tree.distance_of_source(), Some(0.0));
            assert_eq!(tree.path_of_source(), Some(vec![0]));
            assert!(tree.has_no_incoming_edge());
        }
    }

    /// A tiny object-safe view over the two modes, so the identity above is
    /// asserted for both without repeating it.
    trait ReachesItself {
        fn distance_of_source(&self) -> Option<f64>;
        fn path_of_source(&self) -> Option<Vec<u32>>;
        fn has_no_incoming_edge(&self) -> bool;
    }

    impl<R: Relaxation> ReachesItself for PathTree<'_, u32, R> {
        fn distance_of_source(&self) -> Option<f64> {
            self.distance(self.source())
        }

        fn path_of_source(&self) -> Option<Vec<u32>> {
            Some(
                self.path_labels(self.source())?
                    .into_iter()
                    .copied()
                    .collect(),
            )
        }

        fn has_no_incoming_edge(&self) -> bool {
            self.edge_to(self.source()).is_none()
        }
    }

    /// No path is invented across components, and no distance is invented for a
    /// vertex the source cannot reach.
    #[test]
    fn nothing_is_reported_for_an_unreachable_vertex() {
        let g = graph(&[(0, 1, 1.0), (2, 3, 1.0)]);
        for tree_reaches in [
            ShortestPathTree::new(&g, &0).unwrap().distance_of(&3),
            ShortestPathTree::new(&g, &0).unwrap().distance_of(&2),
        ] {
            assert_eq!(tree_reaches, None);
        }
        let tree = ShortestPathTree::new(&g, &0).unwrap();
        assert_eq!(tree.path_of(&2), None);
        assert_eq!(tree.path_of(&3), None);
        assert_eq!(tree.path_labels_of(&3), None);
        // ...while the reachable half is intact.
        assert_eq!(tree.distance_of(&1), Some(1.0));

        // A label that is not in the graph at all is likewise `None`, not an
        // error and not a panic.
        assert_eq!(tree.distance_of(&99), None);
        assert_eq!(tree.path_of(&99), None);
        assert_eq!(tree.distance(VertexId::from_index(99)), None);
        assert_eq!(tree.path(VertexId::from_index(99)), None);
        assert!(tree.edge_to(VertexId::from_index(99)).is_none());
    }

    /// Every distance the tree can report is finite, enumerated over the sample
    /// graph and both modes.
    #[test]
    fn no_non_finite_distance_can_be_observed() {
        let g = sample_dag();
        for id in g.vertices().map(|(id, _)| id) {
            for distance in [
                ShortestPathTree::new(&g, &5).unwrap().distance(id),
                LongestPathTree::new(&g, &5).unwrap().distance(id),
            ] {
                assert!(distance.is_none_or(|d| d.is_finite()));
            }
        }
        // The weights that would have produced one are unrepresentable: `add`
        // refuses them, so no graph exists on which to observe the escape.
        let mut g: EdgeWeightedDigraph<u32> = EdgeWeightedDigraph::new();
        assert!(g.add(&0, &1, f64::NAN).is_err());
        assert!(g.add(&0, &1, f64::INFINITY).is_err());
    }

    /// The one remaining route to a non-finite value is a sum that overflows,
    /// and it is an error rather than a distance.
    #[test]
    fn an_overflowing_sum_fails_the_build() {
        let g = graph(&[(0, 1, f64::MAX), (1, 2, f64::MAX)]);
        let err = ShortestPathTree::new(&g, &0).unwrap_err();
        assert_eq!(err, PathError::Overflow(g.vertex_id(&2).unwrap()));
        assert_eq!(
            err.to_string(),
            "distance to vertex #2 left the finite range of f64"
        );
        // The negative direction too.
        let g = graph(&[(0, 1, -f64::MAX), (1, 2, -f64::MAX)]);
        assert!(matches!(
            LongestPathTree::new(&g, &0),
            Err(PathError::Overflow(_))
        ));
    }

    #[test]
    fn negative_weights_are_ordinary_in_both_directions() {
        // Longest used to baseline at 0.0, which made every negative path lose.
        let g = graph(&[(0, 1, -1.0), (1, 2, -1.0), (0, 2, -5.0)]);
        let longest = LongestPathTree::new(&g, &0).unwrap();
        assert_eq!(longest.distance_of(&2), Some(-2.0));
        assert_eq!(labels(longest.path_labels_of(&2)), vec![0, 1, 2]);

        let shortest = ShortestPathTree::new(&g, &0).unwrap();
        assert_eq!(shortest.distance_of(&2), Some(-5.0));
        assert_eq!(labels(shortest.path_labels_of(&2)), vec![0, 2]);
    }

    /// A zero-weight alternative must not be mistaken for "no path".
    #[test]
    fn zero_weights_are_distances_like_any_other() {
        let g = graph(&[(1, 3, 0.29), (1, 6, 0.0), (3, 6, 0.0)]);
        let tree = ShortestPathTree::new(&g, &1).unwrap();
        assert_eq!(tree.distance_of(&6), Some(0.0));
        assert_eq!(labels(tree.path_labels_of(&6)), vec![1, 6]);

        let longest = LongestPathTree::new(&g, &1).unwrap();
        assert_eq!(longest.distance_of(&6), Some(0.29));
        assert_eq!(labels(longest.path_labels_of(&6)), vec![1, 3, 6]);
    }

    #[test]
    fn ties_keep_the_edge_added_first_even_when_it_relaxes_last() {
        // "Relaxed first" and "added first" are different rules, and this
        // graph separates them. Both routes to `t` weigh 2.0. The edge
        // `a->t` is added FIRST (index 0), but `b` precedes `a` in the
        // topological order, so `b->t` is RELAXED first. A "relaxed
        // first" tree keeps `b->t`; the documented "added first" rule
        // keeps `a->t`.
        let g = graph(&[
            (10, 99, 1.0), // index 0: a -> t, added first
            (1, 20, 1.0),  // index 1: s -> b
            (20, 99, 1.0), // index 2: b -> t, added third
            (1, 10, 1.0),  // index 3: s -> a
        ]);
        let order: Vec<u32> = crate::Topological::new(&g)
            .unwrap()
            .order()
            .iter()
            .map(|&v| *g.label(v).unwrap())
            .collect();
        let position = |label: u32| order.iter().position(|&v| v == label).unwrap();
        assert!(
            position(20) < position(10),
            "fixture is only a discriminator while b precedes a in the \
             topological order; got {order:?}"
        );

        let shortest = ShortestPathTree::new(&g, &1).unwrap();
        let longest = LongestPathTree::new(&g, &1).unwrap();
        for tree_edge in [shortest.edge_to(id(&g, 99)), longest.edge_to(id(&g, 99))] {
            let edge = tree_edge.expect("t is reachable");
            assert_eq!(
                (*g.label(edge.from()).unwrap(), *g.label(edge.to()).unwrap()),
                (10, 99),
                "the tying edge added first must win, not the one relaxed first"
            );
        }
        // Swapping the kept edge on a tie must swap the stored distance with
        // it, or the tree would report a distance that is not the sum along
        // the path it also reports.
        for (distance, path) in [
            (shortest.distance_of(&99), shortest.path_labels_of(&99)),
            (longest.distance_of(&99), longest.path_labels_of(&99)),
        ] {
            assert_eq!(distance, Some(2.0));
            assert_eq!(labels(path), vec![1, 10, 99]);
        }
        assert_eq!(weight_along(&g, &[1, 10, 99]), 2.0);
    }

    #[test]
    fn ties_keep_the_edge_added_first() {
        // Both paths to 3 weigh 2.0; 1->3 was added before 2->3, so the tree
        // arrives by it. (Here the two rules happen to agree — vertex 1 also
        // precedes vertex 2 in the topological order; the test above is the
        // one that separates them.)
        let g = graph(&[(0, 1, 1.0), (0, 2, 1.0), (1, 3, 1.0), (2, 3, 1.0)]);
        let tree = ShortestPathTree::new(&g, &0).unwrap();
        assert_eq!(tree.distance_of(&3), Some(2.0));
        assert_eq!(labels(tree.path_labels_of(&3)), vec![0, 1, 3]);

        // Parallel edges of equal weight: the first added wins.
        let g = graph(&[(0, 1, 1.0), (0, 1, 1.0)]);
        let tree = ShortestPathTree::new(&g, &0).unwrap();
        assert_eq!(tree.edge_to(g.vertex_id(&1).unwrap()), Some(&g.edges()[0]));
    }

    #[test]
    fn a_cyclic_graph_is_refused_by_both_modes() {
        let g = graph(&[(0, 1, 1.0), (1, 0, 1.0)]);
        let err = ShortestPathTree::new(&g, &0).unwrap_err();
        assert!(matches!(err, PathError::Cyclic(_)));
        assert_eq!(
            err.to_string(),
            "graph is cyclic: 2 of 2 vertices lie on or behind a cycle"
        );
        assert!(std::error::Error::source(&err).is_some());
        assert!(LongestPathTree::new(&g, &0).is_err());
    }

    #[test]
    fn an_unknown_source_is_an_error_not_an_empty_tree() {
        let g = graph(&[(0, 1, 1.0)]);
        assert_eq!(
            ShortestPathTree::new(&g, &99).unwrap_err(),
            PathError::UnknownSource
        );
        assert_eq!(
            ShortestPathTree::from_id(&g, VertexId::from_index(7)).unwrap_err(),
            PathError::UnknownSource
        );
        assert_eq!(
            PathError::UnknownSource.to_string(),
            "source vertex is not in the graph"
        );
        assert!(std::error::Error::source(&PathError::UnknownSource).is_none());

        // The empty graph has no vertices, so every source is unknown.
        let empty: EdgeWeightedDigraph<u32> = EdgeWeightedDigraph::new();
        assert_eq!(
            ShortestPathTree::new(&empty, &0).unwrap_err(),
            PathError::UnknownSource
        );
    }

    #[test]
    fn a_source_with_no_outgoing_edges_is_a_tree_of_one() {
        let g = graph(&[(0, 1, 1.0)]);
        let tree = ShortestPathTree::new(&g, &1).unwrap();
        assert_eq!(tree.distance_of(&1), Some(0.0));
        assert_eq!(tree.distance_of(&0), None);
        assert_eq!(labels(tree.path_labels_of(&1)), vec![1]);
        assert_eq!(tree.source(), g.vertex_id(&1).unwrap());
        assert_eq!(tree.graph().edge_count(), 1);
    }

    #[test]
    fn from_id_and_new_agree() {
        let g = sample_dag();
        let by_label = ShortestPathTree::new(&g, &5).unwrap();
        let by_id = ShortestPathTree::from_id(&g, g.vertex_id(&5).unwrap()).unwrap();
        for (id, _) in g.vertices() {
            assert_eq!(by_label.distance(id), by_id.distance(id));
            assert_eq!(by_label.path(id), by_id.path(id));
        }
    }

    #[test]
    fn string_labels() {
        let mut g: EdgeWeightedDigraph<String> = EdgeWeightedDigraph::new();
        g.add("a", "b", 1.0).unwrap();
        g.add("b", "c", 2.0).unwrap();
        let tree = ShortestPathTree::new(&g, "a").unwrap();
        assert_eq!(tree.distance_of("b"), Some(1.0));
        assert_eq!(tree.distance_of("c"), Some(3.0));
        let path: Vec<&str> = tree
            .path_labels_of("c")
            .unwrap()
            .into_iter()
            .map(String::as_str)
            .collect();
        assert_eq!(path, vec!["a", "b", "c"]);
    }

    #[test]
    fn a_long_chain_walks_without_recursion() {
        let mut g: EdgeWeightedDigraph<u32> = EdgeWeightedDigraph::new();
        for i in 0..100_000u32 {
            g.add(&i, &(i + 1), 1.0).unwrap();
        }
        let tree = ShortestPathTree::new(&g, &0).unwrap();
        assert_eq!(tree.distance_of(&100_000), Some(100_000.0));
        assert_eq!(tree.path_of(&100_000).unwrap().len(), 100_001);
    }
}
