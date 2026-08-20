//! Topological ordering of an [`EdgeWeightedDigraph`].

use std::collections::VecDeque;
use std::fmt;

use crate::graph::{EdgeWeightedDigraph, VertexId};

/// The error [`Topological::new`] reports for a graph that is not acyclic.
///
/// [`Cycle::vertices`] holds every vertex that could not be ordered, in
/// ascending [`VertexId`] order. That set is precisely the vertices lying on a
/// cycle together with those reachable only through one — Kahn's algorithm
/// emits a vertex exactly when its in-degree reaches zero, and no vertex in that
/// set ever does. It is deliberately not a single "the" vertex: a graph can
/// contain several disjoint cycles, and naming one of them would suggest the
/// others are not there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cycle {
    unordered: Vec<VertexId>,
    total: usize,
}

impl Cycle {
    /// The vertices that could not be ordered, ascending by id.
    pub fn vertices(&self) -> &[VertexId] {
        &self.unordered
    }
}

impl fmt::Display for Cycle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "graph is cyclic: {} of {} vertices lie on or behind a cycle",
            self.unordered.len(),
            self.total
        )
    }
}

impl std::error::Error for Cycle {}

/// A topological ordering of an [`EdgeWeightedDigraph`].
///
/// # The order, and why it is exactly this one
///
/// A directed acyclic graph usually admits many topological orders. Verbora
/// specifies one, so that the ordering — and therefore every distance a
/// [`crate::PathTree`] derives from it — is reproducible rather than incidental.
///
/// The algorithm is Kahn's (A. B. Kahn, "Topological sorting of large
/// networks", *Communications of the ACM* 5(11):558–562, 1962), with these two
/// tie-breaking rules making it deterministic:
///
/// 1. the queue is seeded with every in-degree-zero vertex in **ascending
///    [`VertexId`] order**, which is first-seen order;
/// 2. it is a **FIFO** queue, and a vertex is enqueued at the moment its
///    in-degree reaches zero.
///
/// Nothing consults a hash map's iteration order, so the result is identical
/// across runs, platforms and compiler versions for the same sequence of
/// [`EdgeWeightedDigraph::add`] calls.
///
/// Building an ordering is O(V + E) in time and O(V) in scratch space, and it
/// recurses nowhere — a chain of a million vertices costs a million queue
/// operations, not a million stack frames.
///
/// # Choosing the right API
///
/// | Want | Call | Cost |
/// |---|---|---|
/// | Feed the order to something else in this crate | [`Topological::order`] | free — a borrowed slice |
/// | Print or match on the caller's own labels | [`Topological::labels`] | one indexed lookup per vertex, lazily |
///
/// `order` is the primitive and allocates nothing; `labels` is a lazy adapter
/// over it, so neither materialises a second vector. Prefer `order` when the
/// ids are what the next step wants — [`crate::PathTree::from_id`] takes one
/// directly — and `labels` at the edge of the program, where a human reads the
/// result.
///
/// ```
/// use verbora_util::{EdgeWeightedDigraph, Topological};
///
/// let mut g: EdgeWeightedDigraph<&str> = EdgeWeightedDigraph::new();
/// g.add(&"parse", &"typecheck", 1.0).unwrap();
/// g.add(&"typecheck", &"codegen", 1.0).unwrap();
/// g.add(&"parse", &"codegen", 1.0).unwrap();
///
/// let order = Topological::new(&g).unwrap();
/// assert_eq!(
///     order.labels().copied().collect::<Vec<_>>(),
///     ["parse", "typecheck", "codegen"]
/// );
/// ```
#[derive(Debug, Clone)]
pub struct Topological<'g, V> {
    graph: &'g EdgeWeightedDigraph<V>,
    order: Vec<VertexId>,
}

impl<'g, V> Topological<'g, V> {
    /// Orders `graph`.
    ///
    /// # Errors
    ///
    /// [`Cycle`] if the graph is not acyclic. A self-loop is a cycle.
    pub fn new(graph: &'g EdgeWeightedDigraph<V>) -> Result<Self, Cycle> {
        let n = graph.vertex_count();
        // `usize` rather than `u32`: the edge count is bounded by memory, not by
        // the vertex-id width, so a `u32` counter could in principle wrap on a
        // graph with more than `u32::MAX` edges into one vertex. A wrapped
        // counter would emit a vertex early and silently produce a non-order.
        let mut in_degree = vec![0usize; n];
        for edge in graph.edges() {
            in_degree[edge.to().index()] += 1;
        }

        // Rule 1: seed in ascending id order.
        let mut queue: VecDeque<VertexId> = (0..n)
            .map(VertexId::from_index)
            .filter(|id| in_degree[id.index()] == 0)
            .collect();

        let mut order = Vec::with_capacity(n);
        // Rule 2: FIFO, enqueued the moment the in-degree reaches zero.
        while let Some(id) = queue.pop_front() {
            order.push(id);
            for edge in graph.edges_from(id) {
                let head = edge.to().index();
                in_degree[head] -= 1;
                if in_degree[head] == 0 {
                    queue.push_back(edge.to());
                }
            }
        }

        if order.len() != n {
            let unordered = (0..n)
                .map(VertexId::from_index)
                .filter(|id| in_degree[id.index()] != 0)
                .collect();
            return Err(Cycle {
                unordered,
                total: n,
            });
        }
        Ok(Self { graph, order })
    }

    /// The vertices in topological order, as ids.
    ///
    /// This is the primitive: a borrowed slice, no allocation, and the form
    /// [`crate::PathTree`] consumes. Use [`Topological::labels`] when you want
    /// the caller's own labels back.
    pub fn order(&self) -> &[VertexId] {
        &self.order
    }

    /// The vertices in topological order, as labels.
    ///
    /// One indexed lookup per vertex on top of [`Topological::order`]; prefer
    /// `order` when the ids are what you need.
    pub fn labels(&self) -> impl ExactSizeIterator<Item = &'g V> {
        let graph = self.graph;
        self.order
            .iter()
            .map(move |id| graph.label(*id).expect("id minted by this graph"))
    }

    /// The graph this ordering was built from.
    pub fn graph(&self) -> &'g EdgeWeightedDigraph<V> {
        self.graph
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn graph(edges: &[(u32, u32, f64)]) -> EdgeWeightedDigraph<u32> {
        let mut g = EdgeWeightedDigraph::new();
        for &(a, b, w) in edges {
            g.add(&a, &b, w).unwrap();
        }
        g
    }

    fn labels(t: &Topological<'_, u32>) -> Vec<u32> {
        t.labels().copied().collect()
    }

    /// Checks the defining property directly: every edge points forward.
    fn is_topological(g: &EdgeWeightedDigraph<u32>, order: &[VertexId]) -> bool {
        let mut position = vec![usize::MAX; g.vertex_count()];
        for (i, id) in order.iter().enumerate() {
            position[id.index()] = i;
        }
        g.edges()
            .iter()
            .all(|e| position[e.from().index()] < position[e.to().index()])
    }

    #[test]
    fn the_order_satisfies_the_definition() {
        let g = crate::graph::sample_dag();
        let t = Topological::new(&g).unwrap();
        assert_eq!(t.order().len(), g.vertex_count());
        assert!(is_topological(&g, t.order()));
    }

    /// The documented tie-break, worked through by hand.
    ///
    /// In-degrees of the sample graph: 5 has none, so it alone seeds the queue.
    /// Removing 5 drops 4 to one (6 still points at it), 7 to two and 1 to zero,
    /// so 1 is enqueued; then 1 frees 3, 3 frees 6, 6 frees 4, 4 frees 7 and 0,
    /// and 7 and 0 together free 2.
    #[test]
    fn the_tie_break_is_the_documented_one() {
        let g = crate::graph::sample_dag();
        let t = Topological::new(&g).unwrap();
        assert_eq!(labels(&t), vec![5, 1, 3, 6, 4, 7, 0, 2]);
    }

    /// Two independent sources: the seed order is ascending id, i.e. first-seen.
    #[test]
    fn independent_sources_come_out_in_first_seen_order() {
        // Vertices are first seen as 7, 8, 9, 0 — so ids 0, 1, 2, 3.
        let g = graph(&[(7, 9, 1.0), (8, 9, 1.0), (0, 9, 1.0)]);
        let t = Topological::new(&g).unwrap();
        assert_eq!(labels(&t), vec![7, 8, 0, 9]);
    }

    #[test]
    fn empty_and_single_edge_graphs() {
        let g: EdgeWeightedDigraph<u32> = EdgeWeightedDigraph::new();
        let t = Topological::new(&g).unwrap();
        assert!(t.order().is_empty());
        assert_eq!(t.labels().count(), 0);
        assert_eq!(t.graph().vertex_count(), 0);

        let g = graph(&[(1, 2, 0.5)]);
        let t = Topological::new(&g).unwrap();
        assert_eq!(labels(&t), vec![1, 2]);
    }

    #[test]
    fn a_disconnected_graph_orders_every_component() {
        let g = graph(&[(0, 1, 1.0), (2, 3, 1.0)]);
        let t = Topological::new(&g).unwrap();
        assert_eq!(labels(&t), vec![0, 2, 1, 3]);
        assert!(is_topological(&g, t.order()));
    }

    #[test]
    fn parallel_edges_do_not_disturb_the_order() {
        let g = graph(&[(0, 1, 1.0), (0, 1, 2.0), (1, 2, 3.0)]);
        let t = Topological::new(&g).unwrap();
        assert_eq!(labels(&t), vec![0, 1, 2]);
        assert!(is_topological(&g, t.order()));
    }

    #[test]
    fn a_self_loop_is_a_cycle() {
        let g = graph(&[(0, 0, 1.0)]);
        let err = Topological::new(&g).unwrap_err();
        assert_eq!(err.vertices(), &[VertexId::from_index(0)]);
        assert_eq!(
            err.to_string(),
            "graph is cyclic: 1 of 1 vertices lie on or behind a cycle"
        );
    }

    #[test]
    fn a_cycle_reports_every_vertex_it_traps() {
        // 0 -> 1 -> 2 -> 0, plus 2 -> 3 which is downstream of the cycle, plus
        // 4 -> 5 which is a clean component and must still be ordered out.
        let g = graph(&[
            (0, 1, 1.0),
            (1, 2, 1.0),
            (2, 0, 1.0),
            (2, 3, 1.0),
            (4, 5, 1.0),
        ]);
        let err = Topological::new(&g).unwrap_err();
        // Ids: 0->0, 1->1, 2->2, 3->3, 4->4, 5->5 in first-seen order.
        let trapped: Vec<usize> = err.vertices().iter().map(|v| v.index()).collect();
        assert_eq!(trapped, vec![0, 1, 2, 3]);
        assert_eq!(
            err.to_string(),
            "graph is cyclic: 4 of 6 vertices lie on or behind a cycle"
        );
    }

    #[test]
    fn two_disjoint_cycles_are_both_reported() {
        let g = graph(&[(0, 1, 1.0), (1, 0, 1.0), (2, 3, 1.0), (3, 2, 1.0)]);
        let err = Topological::new(&g).unwrap_err();
        assert_eq!(err.vertices().len(), 4);
    }

    #[test]
    fn a_deep_chain_costs_no_stack() {
        // Recursive depth-first ordering overflows here; Kahn's does not
        // recurse at all.
        let mut g: EdgeWeightedDigraph<u32> = EdgeWeightedDigraph::new();
        for i in 0..200_000u32 {
            g.add(&i, &(i + 1), 0.001).unwrap();
        }
        let t = Topological::new(&g).unwrap();
        assert_eq!(t.order().len(), 200_001);
        assert_eq!(t.labels().next(), Some(&0));
        assert_eq!(t.labels().last(), Some(&200_000));
    }

    #[test]
    fn string_labels() {
        let mut g: EdgeWeightedDigraph<String> = EdgeWeightedDigraph::new();
        g.add("a", "b", 1.0).unwrap();
        g.add("b", "c", 2.0).unwrap();
        let t = Topological::new(&g).unwrap();
        let seen: Vec<&str> = t.labels().map(String::as_str).collect();
        assert_eq!(seen, vec!["a", "b", "c"]);
    }
}
