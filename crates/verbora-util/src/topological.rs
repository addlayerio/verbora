//! Topological ordering, ported from the reference `topological`.

use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::fmt;

use crate::graph::{DirectedEdge, EdgeWeightedDigraph};
use crate::vertex::{Vertex, VertexKey};

/// The error `Topological` throws when the graph contains a cycle.
///
/// The message is reproduced byte for byte, including the missing space after
/// the colon and the `JSON.stringify` rendering of the vertex — so a cycle
/// through the number `2` reports `Cyclic dependency:2` and one through the
/// string `'b'` reports `Cyclic dependency:"b"`.
#[derive(Debug, Clone, PartialEq)]
pub struct CyclicDependency {
    /// The vertex the traversal met twice on one path.
    pub vertex: Vertex,
}

impl fmt::Display for CyclicDependency {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Cyclic dependency:{}", self.vertex.to_json())
    }
}

impl Error for CyclicDependency {}

/// A topological ordering of an [`EdgeWeightedDigraph`].
///
/// # `is_dag` is always true
///
/// The reference sets `this.isDag = true` in the constructor and never assigns
/// it again, so `isDAG()` cannot return `false`. A cyclic graph is reported by
/// the constructor *throwing* instead, which here is the [`CyclicDependency`]
/// error from [`Topological::new`]. The accessor is kept so the surface matches.
///
/// # The traversal, and why its exact shape matters
///
/// The reference walks vertex indices from last to first, and for each one
/// recurses over the edges whose **head** is that vertex — that is, over its
/// *incoming* edges, despite the local variable being named `outgoing`. Each
/// vertex is appended on the way out, filling a result array from the back,
/// which is then reversed. The result is a valid topological order, but *which*
/// valid order depends on:
///
/// * the order [`EdgeWeightedDigraph::edges`] yields edges (grouped by tail),
///   since that is the order `uniqueVertexs` discovers vertices in and the order
///   the incoming edges of a vertex are visited in;
/// * the back-to-front outer loop.
///
/// Both are observable through `order()`, and every path tree relaxes vertices
/// in exactly this sequence, so any deviation changes the reported distances.
#[derive(Debug, Clone)]
pub struct Topological {
    sorted: Vec<Vertex>,
    is_dag: bool,
}

impl Topological {
    /// Sorts `graph`.
    ///
    /// # Errors
    ///
    /// Returns [`CyclicDependency`] if the graph contains a cycle, mirroring the
    /// reference's `throw`.
    pub fn new(graph: &EdgeWeightedDigraph) -> Result<Self, CyclicDependency> {
        let edges: Vec<&DirectedEdge> = graph.edges().collect();
        let vertices = unique_vertices(&edges);
        let sorted = topo_sort(&vertices, &edges)?;
        Ok(Self {
            sorted,
            is_dag: true,
        })
    }

    /// Always `true`; see the type-level note.
    pub fn is_dag(&self) -> bool {
        self.is_dag
    }

    /// The vertices in topological order.
    ///
    /// The reference returns `this.sorted.slice()`, a fresh copy per call. A
    /// borrowed slice is observationally identical, since nothing can mutate a
    /// `Topological` after construction.
    pub fn order(&self) -> &[Vertex] {
        &self.sorted
    }
}

/// Every vertex mentioned by `edges`, in first-seen order: for each edge its
/// tail then its head, skipping any already present.
///
/// The reference's membership test is `indexOf(...) < 0`, i.e. strict equality,
/// which is why the number `1` and the string `'1'` both appear, and why a `NaN`
/// vertex would be appended once per occurrence. The hash map below reproduces
/// that exactly: [`Vertex::key`] returns `None` for `NaN`, so `NaN` never
/// matches anything, including itself.
fn unique_vertices(edges: &[&DirectedEdge]) -> Vec<Vertex> {
    let mut out: Vec<Vertex> = Vec::new();
    let mut seen: HashMap<VertexKey<'_>, ()> = HashMap::new();
    // Two passes over the borrow checker's expectations are avoided by keying on
    // the *edge's* vertices, which outlive `out`.
    for edge in edges {
        for vertex in [&edge.start, &edge.end] {
            match vertex.key() {
                Some(k) => {
                    if seen.insert(k, ()).is_none() {
                        out.push(vertex.clone());
                    }
                }
                // NaN: never found, so always pushed.
                None => out.push(vertex.clone()),
            }
        }
    }
    out
}

/// One step of the iterative traversal.
enum Frame {
    /// The body of `visit(vertex, i, predecessors)`.
    Enter {
        vertex: Vertex,
        /// `vertexs.indexOf(vertex)`, or -1 — see [`VertexPositions`].
        index: i64,
    },
    /// The trailing `sorted[--cursor] = vertex`, plus the path bookkeeping the
    /// reference gets for free by returning from a recursive call.
    Exit {
        vertex: Vertex,
        index: i64,
        /// Whether this vertex was pushed onto the ancestor set on the way in.
        leaves_path: bool,
    },
}

/// The reference's `topoSort`, with the recursion turned into an explicit stack.
///
/// # Two changes, neither of them observable
///
/// **The recursion is gone.** Its depth is the length of the longest chain, so
/// The reference overflows the reference stack on deep graphs. The explicit
/// stack removes that ceiling without changing an ordering decision: children
/// are pushed in reverse so they pop in source order, and the `Exit` frame is
/// pushed first so it pops last — exactly where the recursive call would have
/// returned.
///
/// **The cycle check is a set, not a list.** The reference carries a
/// `predecessors` array, copies it at every level with `concat`, and scans it
/// with `indexOf` — quadratic in the path length, in both time and allocation.
/// Between a vertex's `Enter` and its `Exit` the explicit stack processes
/// exactly that vertex's subtree, so the set of vertices currently "entered but
/// not exited" *is* `predecessors`. Membership is then O(1).
///
/// A `NaN` vertex is never in the set, because `indexOf` can never find one:
/// `NaN !== NaN`. Such a vertex has no position, which is what `index < 0` marks.
fn topo_sort(
    vertices: &[Vertex],
    edges: &[&DirectedEdge],
) -> Result<Vec<Vertex>, CyclicDependency> {
    // `sorted` is filled from the back by a decrementing cursor.
    let mut sorted: Vec<Option<Vertex>> = vec![None; vertices.len()];
    let mut cursor = vertices.len();
    // The reference keys `visited` by the vertex *index*, not the vertex.
    let mut visited: HashSet<i64> = HashSet::with_capacity(vertices.len());
    // The `predecessors` chain of the frame currently being processed.
    let mut on_path: HashSet<i64> = HashSet::new();

    let positions = VertexPositions::build(vertices);
    let incoming = IncomingEdges::build(edges);

    let mut stack: Vec<Frame> = Vec::new();

    // `let i = cursor; while (i--)` walks indices n-1 down to 0.
    for i in (0..vertices.len()).rev() {
        if visited.contains(&(i as i64)) {
            continue;
        }
        stack.push(Frame::Enter {
            vertex: vertices[i].clone(),
            index: i as i64,
        });

        while let Some(frame) = stack.pop() {
            match frame {
                Frame::Exit {
                    vertex,
                    index,
                    leaves_path,
                } => {
                    if leaves_path {
                        on_path.remove(&index);
                    }
                    if cursor > 0 {
                        cursor -= 1;
                        sorted[cursor] = Some(vertex);
                    }
                    // A cursor underflow writes to `sorted[-1]` in the reference,
                    // which is an ordinary property that `reverse()` and
                    // `slice()` both ignore. Dropping the value matches. It is
                    // reachable only through a NaN vertex, which inflates the
                    // vertex list without inflating the cursor.
                }
                Frame::Enter { vertex, index } => {
                    // The cycle check precedes the visited check in the
                    // reference, so a vertex already visited in THIS path still
                    // throws.
                    if index >= 0 && on_path.contains(&index) {
                        return Err(CyclicDependency { vertex });
                    }
                    if !visited.insert(index) {
                        continue;
                    }

                    let into = incoming.of(&vertex);
                    // The reference extends `predecessors` only when there is
                    // something to recurse into, and resets it to `[]`
                    // otherwise — unobservably, since nothing reads it then.
                    let leaves_path = index >= 0 && !into.is_empty();
                    if leaves_path {
                        on_path.insert(index);
                    }

                    stack.push(Frame::Exit {
                        vertex,
                        index,
                        leaves_path,
                    });
                    for &edge_index in into.iter().rev() {
                        let from = &edges[edge_index].start;
                        stack.push(Frame::Enter {
                            vertex: from.clone(),
                            index: positions.position_of(from),
                        });
                    }
                }
            }
        }
    }

    // `sorted.reverse()`, holes dropped.
    sorted.reverse();
    Ok(sorted.into_iter().flatten().collect())
}

/// A lookup table keyed the way the reference's `===` compares vertices.
///
/// Split into a numeric and a string half rather than keyed by a single
/// [`VertexKey`]: a borrowed key type forces the borrow checker to tie every
/// lookup result to the *query*'s lifetime, and these tables outlive their
/// queries. Numbers are keyed by bit pattern with `-0` normalised to `+0`;
/// `NaN` is never inserted and never found, which is what makes `indexOf`
/// return -1 for it.
struct VertexTable<'a, T> {
    by_num: HashMap<u64, T>,
    by_str: HashMap<&'a str, T>,
}

impl<'a, T> VertexTable<'a, T> {
    fn with_capacity(n: usize) -> Self {
        Self {
            by_num: HashMap::with_capacity(n),
            by_str: HashMap::with_capacity(n),
        }
    }

    fn get(&self, vertex: &Vertex) -> Option<&T> {
        match vertex {
            Vertex::Num(n) if n.is_nan() => None,
            Vertex::Num(n) => self.by_num.get(&normalised_bits(*n)),
            Vertex::Str(s) => self.by_str.get(s.as_str()),
        }
    }

    /// Inserts `value` unless the vertex already has one, mirroring the fact
    /// that `indexOf` reports the FIRST match.
    fn insert_first(&mut self, vertex: &'a Vertex, value: T) {
        match vertex {
            Vertex::Num(n) if n.is_nan() => {}
            Vertex::Num(n) => {
                self.by_num.entry(normalised_bits(*n)).or_insert(value);
            }
            Vertex::Str(s) => {
                self.by_str.entry(s.as_str()).or_insert(value);
            }
        }
    }

    fn entry_or_default(&mut self, vertex: &'a Vertex) -> Option<&mut T>
    where
        T: Default,
    {
        match vertex {
            Vertex::Num(n) if n.is_nan() => None,
            Vertex::Num(n) => Some(self.by_num.entry(normalised_bits(*n)).or_default()),
            Vertex::Str(s) => Some(self.by_str.entry(s.as_str()).or_default()),
        }
    }
}

/// `f64::to_bits` with `-0` folded onto `+0`, so the two hash alike as `0 === -0`
/// requires.
fn normalised_bits(n: f64) -> u64 {
    if n == 0.0 { 0.0f64 } else { n }.to_bits()
}

/// `vertexs.indexOf(v)`, precomputed.
struct VertexPositions<'a> {
    positions: VertexTable<'a, usize>,
}

impl<'a> VertexPositions<'a> {
    fn build(vertices: &'a [Vertex]) -> Self {
        let mut positions = VertexTable::with_capacity(vertices.len());
        for (i, v) in vertices.iter().enumerate() {
            positions.insert_first(v, i);
        }
        Self { positions }
    }

    /// Returns the index, or `-1` — the value `indexOf` reports for a miss, and
    /// the value the reference then uses as a `visited` key.
    fn position_of(&self, v: &Vertex) -> i64 {
        self.positions.get(v).map_or(-1, |&i| i as i64)
    }
}

/// `edges.filter(e => e.to() === vertex)`, precomputed.
///
/// The reference re-filters the whole edge list for every vertex, which is
/// O(V*E). Bucketing by head vertex is O(E) once and yields the same edges in
/// the same order, because the buckets are filled in edge order.
struct IncomingEdges<'a> {
    buckets: VertexTable<'a, Vec<usize>>,
}

impl<'a> IncomingEdges<'a> {
    fn build(edges: &'a [&'a DirectedEdge]) -> Self {
        let mut buckets: VertexTable<'a, Vec<usize>> = VertexTable::with_capacity(edges.len());
        for (i, edge) in edges.iter().enumerate() {
            // A NaN head is `!==` everything, itself included, so it gets no
            // bucket and no vertex ever finds it.
            if let Some(bucket) = buckets.entry_or_default(&edge.end) {
                bucket.push(i);
            }
        }
        Self { buckets }
    }

    fn of(&self, vertex: &Vertex) -> &[usize] {
        self.buckets.get(vertex).map_or(&[], Vec::as_slice)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn graph(edges: &[(i32, i32, f64)]) -> EdgeWeightedDigraph {
        let mut g = EdgeWeightedDigraph::new();
        for &(a, b, w) in edges {
            g.add(a, b, w);
        }
        g
    }

    fn nums(order: &[Vertex]) -> Vec<f64> {
        order
            .iter()
            .map(|v| match v {
                Vertex::Num(n) => *n,
                Vertex::Str(s) => panic!("expected a numeric vertex, got {s:?}"),
            })
            .collect()
    }

    #[test]
    fn canonical_order() {
        let g = graph(&[
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
        ]);
        let t = Topological::new(&g).unwrap();
        assert!(t.is_dag());
        assert_eq!(
            nums(t.order()),
            vec![5.0, 1.0, 3.0, 6.0, 4.0, 7.0, 0.0, 2.0]
        );
    }

    #[test]
    fn empty_graph_sorts_to_nothing() {
        let t = Topological::new(&EdgeWeightedDigraph::new()).unwrap();
        assert!(t.order().is_empty());
        assert!(t.is_dag());
    }

    #[test]
    fn string_vertices() {
        let mut g = EdgeWeightedDigraph::new();
        g.add("a", "b", 1.0);
        g.add("b", "c", 2.0);
        let t = Topological::new(&g).unwrap();
        assert_eq!(
            t.order(),
            &[Vertex::from("a"), Vertex::from("b"), Vertex::from("c")]
        );
    }

    #[test]
    fn cycles_report_the_vertex_met_twice() {
        let e = Topological::new(&graph(&[(0, 0, 1.0)])).unwrap_err();
        assert_eq!(e.to_string(), "Cyclic dependency:0");

        let e = Topological::new(&graph(&[(0, 1, 1.0), (1, 2, 1.0), (2, 0, 1.0)])).unwrap_err();
        assert_eq!(e.to_string(), "Cyclic dependency:2");

        let mut g = EdgeWeightedDigraph::new();
        g.add("a", "b", 1.0);
        g.add("b", "a", 1.0);
        assert_eq!(
            Topological::new(&g).unwrap_err().to_string(),
            "Cyclic dependency:\"b\""
        );
    }

    #[test]
    fn number_and_numeric_string_are_distinct_vertices() {
        let mut g = EdgeWeightedDigraph::new();
        g.add(1, 2, 0.5);
        g.add("1", 3, 0.25);
        let t = Topological::new(&g).unwrap();
        assert_eq!(
            t.order(),
            &[
                Vertex::from("1"),
                Vertex::Num(3.0),
                Vertex::Num(1.0),
                Vertex::Num(2.0)
            ]
        );
    }

    #[test]
    fn deep_chain_does_not_overflow_the_stack() {
        // The reference recurses once per link and dies somewhere in the tens of
        // thousands; the iterative traversal here has no such ceiling.
        let mut g = EdgeWeightedDigraph::new();
        for i in 0..50_000 {
            g.add(i, i + 1, 0.001);
        }
        let t = Topological::new(&g).unwrap();
        assert_eq!(t.order().len(), 50_001);
        assert_eq!(t.order()[0], Vertex::Num(0.0));
        assert_eq!(t.order()[50_000], Vertex::Num(50_000.0));
    }
}
