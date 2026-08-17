//! Single-source path trees, ported from `shortest_path_tree` and
//! `longest_path_tree`.
//!
//! Both files are the same 90 lines with three edits, so they are implemented
//! here as one traversal parameterised by a [`Relaxation`]. The three edits are
//! precisely the trait's three members, which keeps the differences visible
//! instead of buried in two near-identical copies.
//!
//! # Every accepted distance is rounded to two decimals
//!
//! `distTo[w] = parseFloat((distTo[vertex] + edge.weight).toFixed(2))`. The
//! source comment says "in case of the result of 0.28+0.34 is 0.62000001", so it
//! was meant as cosmetic — but it is applied to the stored value, compounds
//! along a path, and is therefore part of the numeric contract. See
//! [`crate::numfmt::snap_2`].
//!
//! # The sentinels are stranger than they look
//!
//! `ShortestPathTree` initialises an unvisited head with
//! `/\d/.test(distTo[w]) ? distTo[w] : Number.MAX_VALUE` — a **regular
//! expression run over the stringified number**. Every finite number stringifies
//! with at least one digit, so the test is "is this a finite number"; `NaN`,
//! `Infinity` and `undefined` all stringify without digits and fall through to
//! `MAX_VALUE`. `LongestPathTree` instead uses `distTo[w] || 0.0`, which is
//! The reference falsiness: `undefined`, `0`, `-0` and `NaN` all become `0`.

use std::marker::PhantomData;

use crate::graph::{DirectedEdge, EdgeWeightedDigraph};
use crate::numfmt::snap_2;
use crate::sparse::SparseArray;
use crate::topological::{CyclicDependency, Topological};
use crate::vertex::Vertex;

mod sealed {
    pub trait Sealed {}
}

/// The three lines that differ between `ShortestPathTree` and
/// `LongestPathTree`.
///
/// Sealed: the two implementations correspond to two files in the reference, and a
/// third would not correspond to anything.
pub trait Relaxation: sealed::Sealed {
    /// The value written to the head vertex before the comparison, given
    /// whatever was there (`None` for an untouched vertex, i.e. `undefined`).
    fn head_sentinel(current: Option<f64>) -> f64;

    /// Whether `candidate` replaces `incumbent`.
    ///
    /// `>` for shortest paths, `<` for longest. Both are false when either side
    /// is `NaN`, which is how a `NaN` weight leaves the head untouched.
    fn accepts(incumbent: f64, candidate: f64) -> bool;

    /// `hasPathTo(v)`, given the stored distance and whether `v` is the source.
    fn has_path_to(dist: Option<f64>, is_start: bool) -> bool;

    /// Whether `pathTo` short-circuits on the source in addition to consulting
    /// `hasPathTo`.
    ///
    /// `ShortestPathTree` guards on both; `LongestPathTree` guards only on
    /// `hasPathTo`, which happens to reject the source anyway because its
    /// distance is 0 and `!!0` is `false`.
    const PATH_TO_GUARDS_START: bool;
}

/// Minimises the total weight along a path.
#[derive(Debug, Clone, Copy)]
pub struct Shortest;

impl sealed::Sealed for Shortest {}

impl Relaxation for Shortest {
    fn head_sentinel(current: Option<f64>) -> f64 {
        // `/\d/.test(x)`: true for every finite number, false for NaN, ±Infinity
        // and undefined.
        match current {
            Some(d) if d.is_finite() => d,
            _ => f64::MAX,
        }
    }

    fn accepts(incumbent: f64, candidate: f64) -> bool {
        incumbent > candidate
    }

    fn has_path_to(dist: Option<f64>, is_start: bool) -> bool {
        if is_start {
            return false;
        }
        matches!(dist, Some(d) if d.is_finite() && d != f64::MAX)
    }

    const PATH_TO_GUARDS_START: bool = true;
}

/// Maximises the total weight along a path.
///
/// Because the baseline is `0.0` rather than a sentinel, a negative-weight
/// relaxation can never win: no distance ever drops below zero.
#[derive(Debug, Clone, Copy)]
pub struct Longest;

impl sealed::Sealed for Longest {}

impl Relaxation for Longest {
    fn head_sentinel(current: Option<f64>) -> f64 {
        coerce_or_zero(current)
    }

    fn accepts(incumbent: f64, candidate: f64) -> bool {
        incumbent < candidate
    }

    fn has_path_to(dist: Option<f64>, _is_start: bool) -> bool {
        // `!!this.distTo[v]`: false for undefined, 0, -0 and NaN — so the source,
        // whose distance is exactly 0, reports having no path to itself.
        matches!(dist, Some(d) if d != 0.0 && !d.is_nan())
    }

    const PATH_TO_GUARDS_START: bool = false;
}

/// The reference's `x || 0`, for a slot holding a number or `undefined`.
///
/// `undefined`, `0`, `-0` and `NaN` are all falsy, so all four yield `+0`.
fn coerce_or_zero(current: Option<f64>) -> f64 {
    match current {
        Some(d) if d != 0.0 && !d.is_nan() => d,
        _ => 0.0,
    }
}

/// A single-source path tree over a directed acyclic graph.
///
/// Use the aliases [`ShortestPathTree`] and [`LongestPathTree`] rather than
/// naming this directly.
///
/// # Two quirks worth knowing before you trust the output
///
/// * **`path_to` walks the parent chain blindly.** It follows `edgeTo` until it
///   runs out and then appends the source unconditionally, whether or not the
///   chain ever reached it. On the disconnected graph `0 -> 1`, `2 -> 3` with
///   source `0`, `path_to(3)` reports `[0, 3]` — a path that does not exist.
/// * **A vertex can acquire a distance without being reachable.** Relaxing
///   vertex `2` writes `distTo[2] = distTo[2] || 0` before looking at any edge,
///   so on that same graph `dist_to(2)` is `0` and `has_path_to(2)` is `true`.
///
/// Both are reproduced. They are the reference's answers, and code built on
/// The reference may depend on them.
#[derive(Debug, Clone)]
pub struct PathTree<R: Relaxation> {
    dist_to: SparseArray<f64>,
    edge_to: SparseArray<DirectedEdge>,
    start: Vertex,
    top: Topological,
    _mode: PhantomData<fn() -> R>,
}

/// The shortest-path tree of a DAG: The reference `ShortestPathTree`.
///
/// ```
/// use verbora_util::{EdgeWeightedDigraph, ShortestPathTree, Vertex};
///
/// let mut g = EdgeWeightedDigraph::new();
/// g.add(5, 4, 0.35);
/// g.add(4, 0, 0.38);
/// g.add(5, 1, 0.32);
///
/// let tree = ShortestPathTree::new(&g, 5).unwrap();
/// assert_eq!(tree.dist_to(&Vertex::from(0)), Some(0.73));
/// assert_eq!(
///     tree.path_to(&Vertex::from(0)),
///     vec![Vertex::from(5), Vertex::from(4), Vertex::from(0)]
/// );
/// // The source reports no path to itself.
/// assert!(!tree.has_path_to(&Vertex::from(5)));
/// ```
pub type ShortestPathTree = PathTree<Shortest>;

/// The longest-path tree of a DAG: The reference `LongestPathTree`.
///
/// ```
/// use verbora_util::{EdgeWeightedDigraph, LongestPathTree, Vertex};
///
/// let mut g = EdgeWeightedDigraph::new();
/// g.add(0, 1, 0.5);
/// g.add(1, 2, 0.25);
/// g.add(0, 2, 0.1);
///
/// let tree = LongestPathTree::new(&g, 0).unwrap();
/// assert_eq!(tree.dist_to(&Vertex::from(2)), Some(0.75));
/// ```
pub type LongestPathTree = PathTree<Longest>;

impl<R: Relaxation> PathTree<R> {
    /// Builds the tree by relaxing every vertex in topological order.
    ///
    /// # Errors
    ///
    /// Returns [`CyclicDependency`] when the graph is cyclic — the reference's
    /// constructor throws from `new Topological(digraph)` in the same place.
    pub fn new(
        digraph: &EdgeWeightedDigraph,
        start: impl Into<Vertex>,
    ) -> Result<Self, CyclicDependency> {
        let start = start.into();
        let mut dist_to: SparseArray<f64> = SparseArray::new();
        let mut edge_to: SparseArray<DirectedEdge> = SparseArray::new();
        dist_to.set(&start, 0.0);

        let top = Topological::new(digraph)?;
        for vertex in top.order() {
            Self::relax_vertex(digraph, vertex, &mut dist_to, &mut edge_to);
        }

        Ok(Self {
            dist_to,
            edge_to,
            start,
            top,
            _mode: PhantomData,
        })
    }

    /// Relaxes every edge leaving `vertex`.
    ///
    /// The four steps are in the reference's order, and the order matters: the
    /// head's sentinel is installed *before* the tail's `|| 0` write-back, and
    /// the comparison then re-reads the head — which is only distinguishable
    /// when head and tail are the same vertex, i.e. for a self-loop, which
    /// `Topological` rejects before we ever get here.
    fn relax_vertex(
        digraph: &EdgeWeightedDigraph,
        vertex: &Vertex,
        dist_to: &mut SparseArray<f64>,
        edge_to: &mut SparseArray<DirectedEdge>,
    ) {
        for edge in digraph.get_adj(vertex) {
            let head = &edge.end;

            dist_to.set(head, R::head_sentinel(dist_to.get(head).copied()));
            let tail_dist = coerce_or_zero(dist_to.get(vertex).copied());
            dist_to.set(vertex, tail_dist);

            let incumbent = dist_to.get(head).copied().unwrap_or(f64::NAN);
            let candidate = tail_dist + edge.weight;
            if R::accepts(incumbent, candidate) {
                dist_to.set(head, snap_2(candidate));
                edge_to.set(head, edge.clone());
            }
        }
    }

    /// The distance recorded for `v`, or `None` if nothing ever wrote one.
    ///
    /// `None` corresponds to the reference returning `undefined`, which is
    /// distinct from a recorded `0`.
    #[doc(alias = "getDistTo")]
    pub fn dist_to(&self, v: &Vertex) -> Option<f64> {
        self.dist_to.get(v).copied()
    }

    /// Whether the tree claims a path to `v`.
    ///
    /// See the type-level note: "claims" is doing real work here.
    pub fn has_path_to(&self, v: &Vertex) -> bool {
        R::has_path_to(self.dist_to(v), *v == self.start)
    }

    /// The path from the source to `v`, source first.
    ///
    /// Empty when [`PathTree::has_path_to`] is false, and — for shortest paths —
    /// when `v` is the source.
    pub fn path_to(&self, v: &Vertex) -> Vec<Vertex> {
        if !self.has_path_to(v) || (R::PATH_TO_GUARDS_START && *v == self.start) {
            return Vec::new();
        }
        let mut path = Vec::new();
        let mut current = self.edge_to.get(v);
        // The reference's loop is unbounded; a DAG guarantees it terminates. The
        // bound here can only be hit by a parent chain that is not a tree, which
        // `Topological` has already ruled out — it exists so a future change
        // cannot turn a logic error into a hang.
        for _ in 0..=self.edge_to.entry_count() {
            let Some(edge) = current else { break };
            path.push(edge.end.clone());
            current = self.edge_to.get(&edge.start);
        }
        // Appended unconditionally, reached or not.
        path.push(self.start.clone());
        path.reverse();
        path
    }

    /// The source vertex.
    pub fn start(&self) -> &Vertex {
        &self.start
    }

    /// The topological ordering the relaxation followed.
    ///
    /// Public because the reference stores it as `this.top` and callers can
    /// reach it.
    pub fn topological(&self) -> &Topological {
        &self.top
    }

    /// The edge by which the tree reaches `v`, if any.
    ///
    /// No counterpart in the reference — `edgeTo` is a bare property there — but it
    /// is what `path_to` walks, and exposing it lets callers avoid rebuilding a
    /// whole path to ask one question.
    pub fn edge_to(&self, v: &Vertex) -> Option<&DirectedEdge> {
        self.edge_to.get(v)
    }

    /// Relaxes a single edge against the current tree.
    ///
    /// # This method is dead code in the reference
    ///
    /// `relaxEdge` is on both prototypes and is never invoked: the constructor
    /// drives everything through `relaxVertex`. It is ported because it is
    /// reachable, and it behaves differently from the relaxation the constructor
    /// uses — there is no sentinel handling, so a comparison against an
    /// untouched vertex is `undefined > x`, which is `false`; and the accepted
    /// value is **not** rounded to two decimals.
    pub fn relax_edge(&mut self, edge: &DirectedEdge) {
        // `undefined` participates in a relational comparison as NaN, and every
        // comparison with NaN is false.
        let incumbent = self.dist_to(&edge.end).unwrap_or(f64::NAN);
        let candidate = self.dist_to(&edge.start).unwrap_or(f64::NAN) + edge.weight;
        if R::accepts(incumbent, candidate) {
            self.dist_to.set(&edge.end, candidate);
            self.edge_to.set(&edge.end, edge.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec_graph() -> EdgeWeightedDigraph {
        let mut g = EdgeWeightedDigraph::new();
        for (a, b, w) in [
            (5, 4, 0.35),
            (4, 7, 0.37),
            (5, 7, 0.28),
            (5, 1, 0.32),
            (4, 0, 0.38),
            (0, 2, 0.26),
            (3, 7, 0.39),
            (1, 3, 0.29),
            (7, 2, 0.34),
            (6, 2, 0.40),
            (3, 6, 0.52),
            (6, 0, 0.58),
            (6, 4, 0.93),
        ] {
            g.add(a, b, w);
        }
        g
    }

    fn v(n: i32) -> Vertex {
        Vertex::from(n)
    }

    fn path_nums(path: &[Vertex]) -> Vec<i32> {
        path.iter()
            .map(|x| match x {
                Vertex::Num(n) => *n as i32,
                Vertex::Str(s) => panic!("expected a numeric vertex, got {s:?}"),
            })
            .collect()
    }

    #[test]
    fn shortest_matches_the_reference_spec() {
        let tree = ShortestPathTree::new(&spec_graph(), 5).unwrap();
        let expected = [0.73, 0.32, 0.62, 0.61, 0.35, 0.0, 1.13, 0.28];
        for (i, want) in expected.iter().enumerate() {
            assert_eq!(tree.dist_to(&v(i as i32)), Some(*want), "distTo({i})");
        }
        assert_eq!(path_nums(&tree.path_to(&v(0))), vec![5, 4, 0]);
        assert_eq!(path_nums(&tree.path_to(&v(2))), vec![5, 7, 2]);
        assert_eq!(path_nums(&tree.path_to(&v(6))), vec![5, 1, 3, 6]);
        assert!(tree.path_to(&v(5)).is_empty());
        for i in [0, 1, 2, 3, 4, 6, 7] {
            assert!(tree.has_path_to(&v(i)), "hasPathTo({i})");
        }
        assert!(!tree.has_path_to(&v(5)));
    }

    #[test]
    fn longest_matches_the_reference_spec() {
        let tree = LongestPathTree::new(&spec_graph(), 5).unwrap();
        let expected = [2.44, 0.32, 2.77, 0.61, 2.06, 0.0, 1.13, 2.43];
        for (i, want) in expected.iter().enumerate() {
            assert_eq!(tree.dist_to(&v(i as i32)), Some(*want), "distTo({i})");
        }
        assert_eq!(path_nums(&tree.path_to(&v(0))), vec![5, 1, 3, 6, 4, 0]);
        assert_eq!(path_nums(&tree.path_to(&v(2))), vec![5, 1, 3, 6, 4, 7, 2]);
        assert!(!tree.has_path_to(&v(5)));
        assert!(tree.path_to(&v(5)).is_empty());
    }

    #[test]
    fn issue_150_lightest_weight_path() {
        let mut g = EdgeWeightedDigraph::new();
        g.add(1, 3, 0.29);
        g.add(1, 6, 0.0);
        g.add(3, 6, 0.0);
        let tree = ShortestPathTree::new(&g, 1).unwrap();
        assert_eq!(path_nums(&tree.path_to(&v(6))), vec![1, 6]);

        let mut g = EdgeWeightedDigraph::new();
        g.add(1, 3, -1.0);
        g.add(1, 6, 0.0);
        g.add(3, 6, 0.0);
        let tree = ShortestPathTree::new(&g, 1).unwrap();
        assert!(tree.has_path_to(&v(6)));
        assert_eq!(path_nums(&tree.path_to(&v(6))), vec![1, 3, 6]);
    }

    #[test]
    fn disconnected_graph_invents_a_path() {
        // Reproducing the reference exactly: `pathTo(3)` prefixes the source
        // even though the chain never reaches it, and vertex 2 gets a distance
        // just by being relaxed.
        let mut g = EdgeWeightedDigraph::new();
        g.add(0, 1, 1.0);
        g.add(2, 3, 1.0);
        let tree = ShortestPathTree::new(&g, 0).unwrap();
        assert_eq!(path_nums(&tree.path_to(&v(3))), vec![0, 3]);
        assert_eq!(tree.dist_to(&v(2)), Some(0.0));
        assert!(tree.has_path_to(&v(2)));
        assert_eq!(path_nums(&tree.path_to(&v(2))), vec![0]);
    }

    #[test]
    fn nan_weight_leaves_the_sentinel_in_place() {
        let mut g = EdgeWeightedDigraph::new();
        g.add(0, 1, f64::NAN);
        g.add(1, 2, 1.0);
        g.add(0, 2, 0.5);
        let s = ShortestPathTree::new(&g, 0).unwrap();
        assert_eq!(s.dist_to(&v(1)), Some(f64::MAX));
        assert!(!s.has_path_to(&v(1)));
        assert_eq!(s.dist_to(&v(2)), Some(0.5));

        let l = LongestPathTree::new(&g, 0).unwrap();
        assert_eq!(l.dist_to(&v(1)), Some(0.0));
        assert!(!l.has_path_to(&v(1)));
    }

    #[test]
    fn untouched_vertices_have_no_distance() {
        let tree = ShortestPathTree::new(&spec_graph(), 5).unwrap();
        assert_eq!(tree.dist_to(&v(99)), None);
        assert!(!tree.has_path_to(&v(99)));
        assert!(tree.path_to(&v(99)).is_empty());
    }

    #[test]
    fn string_vertices() {
        let mut g = EdgeWeightedDigraph::new();
        g.add("a", "b", 1.0);
        g.add("b", "c", 2.0);
        let tree = ShortestPathTree::new(&g, "a").unwrap();
        assert_eq!(tree.dist_to(&Vertex::from("b")), Some(1.0));
        assert_eq!(
            tree.path_to(&Vertex::from("c")),
            vec![Vertex::from("a"), Vertex::from("b"), Vertex::from("c")]
        );
    }

    #[test]
    fn cyclic_graphs_are_rejected_at_construction() {
        let mut g = EdgeWeightedDigraph::new();
        g.add(0, 1, 1.0);
        g.add(1, 0, 1.0);
        assert_eq!(
            ShortestPathTree::new(&g, 0).unwrap_err().to_string(),
            "Cyclic dependency:1"
        );
        assert!(LongestPathTree::new(&g, 0).is_err());
    }

    #[test]
    fn relax_edge_is_unrounded_and_sentinel_free() {
        let mut tree = ShortestPathTree::new(&spec_graph(), 5).unwrap();
        // 0.35 is already better than 0.0 + 0.01 is not — but the *unrounded*
        // value is what would be stored, unlike the constructor's path.
        tree.relax_edge(&DirectedEdge::new(5, 4, 0.01));
        assert_eq!(tree.dist_to(&v(4)), Some(0.01));

        // Against untouched vertices every comparison is false.
        let mut empty = ShortestPathTree::new(&EdgeWeightedDigraph::new(), 0).unwrap();
        empty.relax_edge(&DirectedEdge::new(0, 1, 1.0));
        assert_eq!(empty.dist_to(&v(1)), None);
    }

    #[test]
    fn empty_graph() {
        let tree = ShortestPathTree::new(&EdgeWeightedDigraph::new(), 0).unwrap();
        assert_eq!(tree.dist_to(&v(0)), Some(0.0));
        assert!(!tree.has_path_to(&v(0)));
        assert!(tree.path_to(&v(0)).is_empty());
        assert!(tree.topological().order().is_empty());
        assert_eq!(*tree.start(), v(0));
    }
}
