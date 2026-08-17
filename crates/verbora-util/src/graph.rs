//! Edge-weighted digraphs, ported from `directed_edge` and
//! `edge_weighted_digraph`.

use std::fmt;

use crate::bag::Bag;
use crate::sparse::SparseArray;
use crate::vertex::Vertex;

/// A weighted edge from one vertex to another.
///
/// # The `weight()` method in the reference is unreachable
///
/// `DirectedEdge` declares a `weight()` method on its prototype, but the
/// constructor's own property `this.weight = weight` shadows it, so
/// `edge.weight` is always the number and `edge.weight()` throws
/// `TypeError: e.weight is not a function`. Every call site in the library uses
/// the property form. Here [`DirectedEdge::weight`] is the accessor, which is
/// what the property form does.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DirectedEdge {
    /// The tail vertex. Named `start` to match `JSON.stringify(edge)`.
    pub start: Vertex,
    /// The head vertex. Named `end` to match `JSON.stringify(edge)`.
    pub end: Vertex,
    /// The edge weight.
    ///
    /// The reference accepts any value here, `undefined` included, which makes
    /// every later sum `NaN`. `f64` cannot express `undefined`, so callers who
    /// want that behaviour pass [`f64::NAN`]: the two are numerically
    /// indistinguishable everywhere in the graph code, and differ only in how
    /// `toString` renders them (`undefined` versus `NaN`).
    pub weight: f64,
}

impl DirectedEdge {
    /// Creates an edge.
    ///
    /// ```
    /// use verbora_util::DirectedEdge;
    /// let e = DirectedEdge::new(5, 4, 0.35);
    /// assert_eq!(e.to_string(), "5 -> 4, 0.35");
    /// ```
    pub fn new(start: impl Into<Vertex>, end: impl Into<Vertex>, weight: f64) -> Self {
        Self {
            start: start.into(),
            end: end.into(),
            weight,
        }
    }

    /// The tail vertex. Mirrors `from()`.
    #[doc(alias = "from")]
    pub fn tail(&self) -> &Vertex {
        &self.start
    }

    /// The head vertex. Mirrors `to()`.
    #[doc(alias = "to")]
    pub fn head(&self) -> &Vertex {
        &self.end
    }

    /// The edge weight. Mirrors reading the `weight` property.
    pub fn weight(&self) -> f64 {
        self.weight
    }
}

impl fmt::Display for DirectedEdge {
    /// Renders the edge as `util.format('%s -> %s, %s', start, end, weight)`.
    ///
    /// Number rendering follows `Number::toString`, so a weight of `0.40` prints
    /// as `0.4` and `1e21` as `1e+21`. See [`crate::numfmt`].
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} -> {}, {}",
            self.start,
            self.end,
            crate::numfmt::format_s(self.weight)
        )
    }
}

/// A directed graph whose edges carry weights.
///
/// # `v()` is not the vertex count
///
/// [`EdgeWeightedDigraph::v`] returns `adj.length`. Only *tail* vertices are
/// ever written to `adj`, so the value is one past the largest integer tail —
/// heads and string vertices do not count. A graph consisting of the single edge
/// `10 -> 3` therefore reports `v() == 11` and `e() == 1`, and a graph whose
/// vertices are all strings reports `v() == 0`. The name is the reference's; the
/// behaviour is reproduced rather than corrected.
#[derive(Debug, Clone, Default)]
pub struct EdgeWeightedDigraph {
    edges_num: usize,
    adj: SparseArray<Bag<DirectedEdge>>,
}

impl EdgeWeightedDigraph {
    /// Creates an empty graph.
    pub fn new() -> Self {
        Self {
            edges_num: 0,
            adj: SparseArray::new(),
        }
    }

    /// Adds an edge `start -> end` with the given weight.
    pub fn add(&mut self, start: impl Into<Vertex>, end: impl Into<Vertex>, weight: f64) {
        self.add_edge(DirectedEdge::new(start, end, weight));
    }

    /// Adds a pre-built edge.
    pub fn add_edge(&mut self, edge: DirectedEdge) {
        // The reference's guard is `if (!this.adj[e.from()])`, which tests the
        // Bag object rather than the index — so vertex 0 works, where an
        // index-truthiness test would have skipped it.
        //
        // `get_or_insert_with` only needs the tail vertex for the duration of
        // this call, so borrowing `edge.start` and moving `edge` in the same
        // statement avoids cloning the vertex — a real cost for `Vertex::Str`,
        // which owns a `String`.
        self.adj.get_or_insert_with(&edge.start, Bag::new).add(edge);
        self.edges_num += 1;
    }

    /// `adj.length` — see the type-level note; this is **not** the vertex count.
    pub fn v(&self) -> u64 {
        self.adj.length()
    }

    /// The number of edges added.
    pub fn e(&self) -> usize {
        self.edges_num
    }

    /// The edges leaving `vertex`, in the order they were added.
    ///
    /// Returns an empty slice for a vertex with no outgoing edges. The reference
    /// returns a fresh array copy (`bag.unpack()`); a borrowed slice is
    /// observationally identical and allocates nothing.
    pub fn get_adj(&self, vertex: &Vertex) -> &[DirectedEdge] {
        self.adj.get(vertex).map_or(&[], Bag::items)
    }

    /// Iterates every edge in `for…in` order over the adjacency array: grouped
    /// by tail vertex, integer tails ascending, then string tails in insertion
    /// order, and within a tail in insertion order.
    ///
    /// This is the lazy primitive the rest of the module is built on —
    /// [`EdgeWeightedDigraph::edges_vec`], [`fmt::Display`], and
    /// [`crate::Topological`] all consume it. The grouping is not incidental: it
    /// is why adding the canonical thirteen edges in any order yields the same
    /// `toString`, and it fixes the traversal order of every path tree.
    pub fn edges(&self) -> impl Iterator<Item = &DirectedEdge> {
        self.adj.iter_for_in().flat_map(Bag::iter)
    }

    /// Collects [`EdgeWeightedDigraph::edges`], mirroring the reference's
    /// `edges()` return of a fresh array.
    pub fn edges_vec(&self) -> Vec<&DirectedEdge> {
        self.edges().collect()
    }
}

impl fmt::Display for EdgeWeightedDigraph {
    /// One edge per line, in [`EdgeWeightedDigraph::edges`] order, joined with
    /// `\n` — and with no trailing newline, since the reference uses `join`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, edge) in self.edges().enumerate() {
            if i > 0 {
                f.write_str("\n")?;
            }
            write!(f, "{edge}")?;
        }
        Ok(())
    }
}

impl Extend<DirectedEdge> for EdgeWeightedDigraph {
    fn extend<I: IntoIterator<Item = DirectedEdge>>(&mut self, iter: I) {
        for edge in iter {
            self.add_edge(edge);
        }
    }
}

impl FromIterator<DirectedEdge> for EdgeWeightedDigraph {
    fn from_iter<I: IntoIterator<Item = DirectedEdge>>(iter: I) -> Self {
        let mut g = Self::new();
        g.extend(iter);
        g
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The 13-edge digraph both the reference graph specs are written against.
    pub(crate) fn spec_graph() -> EdgeWeightedDigraph {
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

    #[test]
    fn spec_graph_summary() {
        let g = spec_graph();
        assert_eq!(g.v(), 8);
        assert_eq!(g.e(), 13);
        assert_eq!(
            g.to_string(),
            "0 -> 2, 0.26\n1 -> 3, 0.29\n3 -> 7, 0.39\n3 -> 6, 0.52\n4 -> 7, 0.37\n\
             4 -> 0, 0.38\n5 -> 4, 0.35\n5 -> 7, 0.28\n5 -> 1, 0.32\n6 -> 2, 0.4\n\
             6 -> 0, 0.58\n6 -> 4, 0.93\n7 -> 2, 0.34"
        );
    }

    #[test]
    fn edges_regroup_regardless_of_insertion_order() {
        let mut reversed = EdgeWeightedDigraph::new();
        let edges: Vec<DirectedEdge> = spec_graph().edges().cloned().collect();
        for e in edges.iter().rev() {
            reversed.add_edge(e.clone());
        }
        // Tails regroup identically; only the order *within* a tail differs.
        let tails: Vec<String> = reversed.edges().map(|e| e.start.to_string()).collect();
        let expected: Vec<String> = spec_graph().edges().map(|e| e.start.to_string()).collect();
        assert_eq!(tails, expected);
    }

    #[test]
    fn v_counts_slots_not_vertices() {
        let mut g = EdgeWeightedDigraph::new();
        g.add(10, 3, 1.0);
        assert_eq!(g.v(), 11);
        assert_eq!(g.e(), 1);
    }

    #[test]
    fn string_vertices_leave_length_at_zero() {
        let mut g = EdgeWeightedDigraph::new();
        g.add("a", "b", 1.0);
        g.add("b", "c", 2.0);
        assert_eq!(g.v(), 0);
        assert_eq!(g.e(), 2);
        assert_eq!(g.to_string(), "a -> b, 1\nb -> c, 2");
    }

    #[test]
    fn vertex_zero_gets_an_adjacency_list() {
        // The reference's guard tests the Bag, not the index, so 0 is fine.
        let mut g = EdgeWeightedDigraph::new();
        g.add(0, 1, 0.5);
        assert_eq!(g.get_adj(&Vertex::from(0)).len(), 1);
    }

    #[test]
    fn number_and_numeric_string_tails_share_a_bag() {
        let mut g = EdgeWeightedDigraph::new();
        g.add(1, 2, 0.5);
        g.add("1", 3, 0.25);
        assert_eq!(g.get_adj(&Vertex::from(1)).len(), 2);
        assert_eq!(g.get_adj(&Vertex::from("1")).len(), 2);
        assert_eq!(g.to_string(), "1 -> 2, 0.5\n1 -> 3, 0.25");
    }

    #[test]
    fn empty_graph() {
        let g = EdgeWeightedDigraph::new();
        assert_eq!(g.v(), 0);
        assert_eq!(g.e(), 0);
        assert_eq!(g.to_string(), "");
        assert_eq!(g.get_adj(&Vertex::from(0)), &[]);
        assert_eq!(g.edges().count(), 0);
    }

    #[test]
    fn weight_rendering_follows_the_reference() {
        assert_eq!(DirectedEdge::new(6, 2, 0.40).to_string(), "6 -> 2, 0.4");
        assert_eq!(DirectedEdge::new(0, 1, -0.0).to_string(), "0 -> 1, -0");
        assert_eq!(DirectedEdge::new(0, 1, 1e21).to_string(), "0 -> 1, 1e+21");
        assert_eq!(DirectedEdge::new(0, 1, f64::NAN).to_string(), "0 -> 1, NaN");
        assert_eq!(
            DirectedEdge::new("日本", "中文", 0.5).to_string(),
            "日本 -> 中文, 0.5"
        );
    }

    #[test]
    fn json_field_order_matches_the_reference() {
        // `JSON.stringify(edge)` is '{"start":5,"end":4,"weight":0.35}'. The
        // field names and their order match; serde renders an integral f64 as
        // `5.0` where the reference, having only one number type, writes `5`.
        let e = DirectedEdge::new(5, 4, 0.35);
        assert_eq!(
            serde_json::to_string(&e).unwrap(),
            r#"{"start":5.0,"end":4.0,"weight":0.35}"#
        );
        let round: DirectedEdge =
            serde_json::from_str(r#"{"start":5,"end":4,"weight":0.35}"#).unwrap();
        assert_eq!(round, e);
    }
}
