//! Edge-weighted directed graphs.

use std::borrow::Borrow;
use std::fmt;
use std::hash::Hash;

use rustc_hash::FxHashMap;

/// A vertex's position in the graph that minted it.
///
/// Ids are assigned densely from `0`, in the order vertices are first seen as
/// an endpoint of an added edge. They are stable for the lifetime of the graph:
/// nothing removes a vertex, so an id never changes meaning and never dangles.
///
/// An id is meaningful **only** for the graph that minted it. Passing one to a
/// different graph is not detected — the two graphs cannot tell each other's ids
/// apart — so it reads whatever vertex sits at that index there, or `None` when
/// the other graph is smaller.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VertexId(u32);

impl VertexId {
    /// This id as a `usize` index.
    pub fn index(self) -> usize {
        self.0 as usize
    }

    /// Rebuilds an id from a dense index.
    ///
    /// Crate-private: ids are only ever minted by a graph, and this is the
    /// internal inverse of [`VertexId::index`] used while walking
    /// `0..vertex_count`.
    pub(crate) fn from_index(i: usize) -> Self {
        Self(i as u32)
    }
}

impl fmt::Display for VertexId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{}", self.0)
    }
}

/// A weighted edge from one vertex to another.
///
/// Endpoints are [`VertexId`]s rather than labels, so an edge is 16 bytes and
/// `Copy` whatever the label type is: a graph over `String` labels stores each
/// label once, not twice per incident edge. Recover a label with
/// [`EdgeWeightedDigraph::label`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DirectedEdge {
    from: VertexId,
    to: VertexId,
    weight: f64,
}

impl DirectedEdge {
    /// The tail vertex — the end the edge leaves.
    pub fn from(&self) -> VertexId {
        self.from
    }

    /// The head vertex — the end the edge arrives at.
    pub fn to(&self) -> VertexId {
        self.to
    }

    /// The edge weight. Always finite: [`EdgeWeightedDigraph::add`] rejects
    /// anything else.
    pub fn weight(&self) -> f64 {
        self.weight
    }
}

/// Why an edge could not be added.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum GraphError {
    /// The weight was `NaN` or an infinity.
    ///
    /// Finite weights are a precondition of the whole subsystem rather than a
    /// stylistic preference: `NaN` makes every comparison in a relaxation false,
    /// so a `NaN` edge would silently leave a vertex at whatever it happened to
    /// hold, and an infinity makes a distance that no arithmetic can undo.
    /// Rejecting both at the door is what lets [`crate::PathTree`] promise that
    /// every distance it reports is finite.
    NonFiniteWeight,
    /// The graph already holds `u32::MAX` vertices and cannot mint another id.
    ///
    /// Reachable only by a graph with more than four billion distinct vertices.
    /// It is an error rather than a panic because the alternative is a panic in
    /// a safe API on input the caller controls.
    VertexLimit,
}

impl fmt::Display for GraphError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonFiniteWeight => f.write_str("edge weight must be finite"),
            Self::VertexLimit => f.write_str("graph cannot hold more than u32::MAX vertices"),
        }
    }
}

impl std::error::Error for GraphError {}

/// A directed graph whose edges carry finite `f64` weights.
///
/// # The vertex model
///
/// A vertex is identified by a **label** of the caller's own type `V`, which
/// must be `Eq + Hash + Clone`. Two labels are the same vertex exactly when
/// `V`'s `Eq` says so — there is no coercion between label types, so `5u32` and
/// `"5"` are not merely different vertices, they cannot appear in the same
/// graph at all. Note also what `Eq` excludes: `f64` is not `Eq`, so a `NaN`
/// vertex is unrepresentable rather than an edge case to document.
///
/// Internally each distinct label is interned once into a dense [`VertexId`].
/// Edges store ids, so a label is stored exactly twice — once in the id lookup
/// and once in the label table — regardless of how many edges touch it.
///
/// # Ordering is insertion order, everywhere
///
/// Two orders are observable, and both are simply the order edges were added:
///
/// * [`EdgeWeightedDigraph::edges`] yields every edge in insertion order;
/// * [`EdgeWeightedDigraph::edges_from`] yields one vertex's outgoing edges in
///   insertion order.
///
/// Vertex ids likewise ascend in first-seen order. Nothing here depends on hash
/// iteration order, so a graph built from the same sequence of `add` calls is
/// byte-identical across runs and platforms.
///
/// # Sharing
///
/// A graph is `Send + Sync` whenever `V` is, and holds no interior mutability
/// and no reference counting, so a built graph can be wrapped in an `Arc` and
/// queried from many threads with no lock. [`Topological`](crate::Topological)
/// and [`PathTree`](crate::PathTree) borrow it immutably and add no state of
/// their own that would change that.
///
/// ```
/// use verbora_util::EdgeWeightedDigraph;
///
/// let mut g: EdgeWeightedDigraph<u32> = EdgeWeightedDigraph::new();
/// g.add(&5, &4, 0.35).unwrap();
/// g.add(&4, &7, 0.37).unwrap();
/// g.add(&5, &7, 0.28).unwrap();
///
/// assert_eq!(g.vertex_count(), 3);
/// assert_eq!(g.edge_count(), 3);
/// assert_eq!(g.to_string(), "5 -> 4, 0.35\n4 -> 7, 0.37\n5 -> 7, 0.28");
/// ```
#[derive(Debug, Clone)]
pub struct EdgeWeightedDigraph<V> {
    /// Label per vertex id.
    labels: Vec<V>,
    /// Label to id. Holds the second (and last) copy of each label.
    ids: FxHashMap<V, VertexId>,
    /// Per vertex id, indices into `edges` for its outgoing edges.
    adj: Vec<Vec<u32>>,
    /// Every edge, in insertion order.
    edges: Vec<DirectedEdge>,
}

impl<V> Default for EdgeWeightedDigraph<V> {
    fn default() -> Self {
        Self::new()
    }
}

impl<V> EdgeWeightedDigraph<V> {
    /// Creates an empty graph.
    pub fn new() -> Self {
        Self {
            labels: Vec::new(),
            ids: FxHashMap::default(),
            adj: Vec::new(),
            edges: Vec::new(),
        }
    }

    /// Creates an empty graph sized for `vertices` vertices and `edges` edges.
    ///
    /// Preallocation only; the capacities are not limits.
    pub fn with_capacity(vertices: usize, edges: usize) -> Self {
        Self {
            labels: Vec::with_capacity(vertices),
            ids: FxHashMap::with_capacity_and_hasher(vertices, rustc_hash::FxBuildHasher),
            adj: Vec::with_capacity(vertices),
            edges: Vec::with_capacity(edges),
        }
    }

    /// The number of distinct vertices.
    ///
    /// This counts vertices. Both endpoints of every added edge are counted,
    /// whether or not they have outgoing edges of their own.
    pub fn vertex_count(&self) -> usize {
        self.labels.len()
    }

    /// The number of edges added. Parallel edges are counted separately.
    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }

    /// Whether the graph holds no vertices — equivalently, no edges.
    pub fn is_empty(&self) -> bool {
        self.labels.is_empty()
    }

    /// The label `id` was minted for, or `None` if this graph has no such id.
    pub fn label(&self, id: VertexId) -> Option<&V> {
        self.labels.get(id.index())
    }

    /// Every vertex, as `(id, label)`, in ascending id order.
    pub fn vertices(&self) -> impl ExactSizeIterator<Item = (VertexId, &V)> {
        self.labels
            .iter()
            .enumerate()
            .map(|(i, label)| (VertexId(i as u32), label))
    }

    /// Every edge, in insertion order.
    ///
    /// A slice rather than an iterator: the edges are already contiguous, so
    /// this borrows them with no adapter in between.
    pub fn edges(&self) -> &[DirectedEdge] {
        &self.edges
    }

    /// The edges leaving `id`, in insertion order.
    ///
    /// Empty for a vertex with no outgoing edges, and for an id this graph never
    /// minted.
    pub fn edges_from(&self, id: VertexId) -> impl ExactSizeIterator<Item = &DirectedEdge> {
        self.adj
            .get(id.index())
            .map_or(&[][..], Vec::as_slice)
            .iter()
            .map(|&i| &self.edges[i as usize])
    }

    /// The number of edges leaving `id`; `0` for an unknown id.
    pub fn out_degree(&self, id: VertexId) -> usize {
        self.adj.get(id.index()).map_or(0, Vec::len)
    }

    /// Positions within [`EdgeWeightedDigraph::edges`] of the edges leaving
    /// `id`, in insertion order.
    ///
    /// Crate-private: an index is only meaningful against this graph's own edge
    /// slice. [`crate::PathTree`] stores one per vertex instead of a copy of the
    /// edge, which is what keeps its parent table four bytes per vertex.
    pub(crate) fn adjacent_indices(&self, id: VertexId) -> &[u32] {
        self.adj.get(id.index()).map_or(&[], Vec::as_slice)
    }

    /// The edge at `index` within [`EdgeWeightedDigraph::edges`].
    pub(crate) fn edge_at(&self, index: u32) -> &DirectedEdge {
        &self.edges[index as usize]
    }
}

impl<V: Eq + Hash + Clone> EdgeWeightedDigraph<V> {
    /// Adds an edge `from -> to` with the given weight, interning either
    /// endpoint that is new.
    ///
    /// Parallel edges and repeated `add` calls with the same arguments are both
    /// kept: this is a multigraph, and nothing deduplicates. A self-loop is
    /// accepted here and rejected later, by [`crate::Topological`], because it
    /// is a cycle.
    ///
    /// Labels are taken by reference and cloned only when the vertex is new, so
    /// the common case — an edge between two vertices the graph already knows —
    /// allocates nothing for `V = String`.
    ///
    /// # Errors
    ///
    /// [`GraphError::NonFiniteWeight`] if `weight` is `NaN` or infinite;
    /// [`GraphError::VertexLimit`] if a new vertex would exceed `u32::MAX`.
    ///
    /// ```
    /// use verbora_util::{EdgeWeightedDigraph, GraphError};
    ///
    /// let mut g: EdgeWeightedDigraph<String> = EdgeWeightedDigraph::new();
    /// g.add("build", "test", 1.5).unwrap();
    /// assert_eq!(g.add("build", "test", f64::NAN), Err(GraphError::NonFiniteWeight));
    /// // The failed call added nothing.
    /// assert_eq!(g.edge_count(), 1);
    /// ```
    pub fn add<Q>(&mut self, from: &Q, to: &Q, weight: f64) -> Result<(), GraphError>
    where
        V: Borrow<Q>,
        Q: Hash + Eq + ToOwned<Owned = V> + ?Sized,
    {
        if !weight.is_finite() {
            return Err(GraphError::NonFiniteWeight);
        }
        // Both ids are minted before anything is pushed, so a `VertexLimit`
        // leaves the graph exactly as it was.
        let from_id = self.intern(from)?;
        let to_id = self.intern(to)?;
        let index = self.edges.len() as u32;
        self.edges.push(DirectedEdge {
            from: from_id,
            to: to_id,
            weight,
        });
        self.adj[from_id.index()].push(index);
        Ok(())
    }

    /// The id of `label`, or `None` if this graph has no such vertex.
    pub fn vertex_id<Q>(&self, label: &Q) -> Option<VertexId>
    where
        V: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        self.ids.get(label).copied()
    }

    /// Whether `label` is a vertex of this graph.
    pub fn contains_vertex<Q>(&self, label: &Q) -> bool
    where
        V: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        self.ids.contains_key(label)
    }

    /// Returns `label`'s id, minting one if the vertex is new.
    fn intern<Q>(&mut self, label: &Q) -> Result<VertexId, GraphError>
    where
        V: Borrow<Q>,
        Q: Hash + Eq + ToOwned<Owned = V> + ?Sized,
    {
        if let Some(&id) = self.ids.get(label) {
            return Ok(id);
        }
        let next = u32::try_from(self.labels.len()).map_err(|_| GraphError::VertexLimit)?;
        if next == u32::MAX {
            return Err(GraphError::VertexLimit);
        }
        let id = VertexId(next);
        let owned = label.to_owned();
        self.labels.push(owned.clone());
        self.ids.insert(owned, id);
        self.adj.push(Vec::new());
        Ok(id)
    }
}

impl<V: fmt::Display> fmt::Display for EdgeWeightedDigraph<V> {
    /// One edge per line as `from -> to, weight`, in
    /// [`EdgeWeightedDigraph::edges`] order, with no trailing newline.
    ///
    /// Labels render through `V`'s own `Display` and weights through Rust's
    /// `{}` for `f64`, which prints the shortest decimal that round-trips and
    /// never uses exponential notation.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, edge) in self.edges.iter().enumerate() {
            if i > 0 {
                f.write_str("\n")?;
            }
            match (self.label(edge.from), self.label(edge.to)) {
                (Some(from), Some(to)) => write!(f, "{from} -> {to}, {}", edge.weight)?,
                // Unreachable: every edge is built from ids this graph minted.
                _ => write!(f, "{} -> {}, {}", edge.from, edge.to, edge.weight)?,
            }
        }
        Ok(())
    }
}

/// An eight-vertex, thirteen-edge DAG shared by this crate's tests.
///
/// It is a worked example rather than a fixture copied from anywhere: every
/// expected value asserted against it is arithmetic shown in the assertion
/// itself.
#[cfg(test)]
pub(crate) fn sample_dag() -> EdgeWeightedDigraph<u32> {
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
        g.add(&a, &b, w).unwrap();
    }
    g
}

#[cfg(test)]
mod tests {
    use super::sample_dag as sample;
    use super::*;

    #[test]
    fn counts_are_counts() {
        let g = sample();
        assert_eq!(g.vertex_count(), 8);
        assert_eq!(g.edge_count(), 13);
        assert!(!g.is_empty());

        // A single edge between two distant labels is two vertices, not eleven.
        let mut sparse: EdgeWeightedDigraph<u32> = EdgeWeightedDigraph::new();
        sparse.add(&10, &3, 1.0).unwrap();
        assert_eq!(sparse.vertex_count(), 2);
        assert_eq!(sparse.edge_count(), 1);
    }

    #[test]
    fn empty_graph() {
        let g: EdgeWeightedDigraph<u32> = EdgeWeightedDigraph::new();
        assert_eq!(g.vertex_count(), 0);
        assert_eq!(g.edge_count(), 0);
        assert!(g.is_empty());
        assert_eq!(g.to_string(), "");
        assert_eq!(g.edges(), &[]);
        assert_eq!(g.vertex_id(&0), None);
        assert_eq!(g.label(VertexId(0)), None);
        assert_eq!(g.out_degree(VertexId(0)), 0);
        assert_eq!(g.edges_from(VertexId(0)).count(), 0);
        assert_eq!(g.vertices().count(), 0);
    }

    #[test]
    fn ids_ascend_in_first_seen_order() {
        let g = sample();
        // The first edge is 5 -> 4, so 5 is id 0 and 4 is id 1.
        assert_eq!(g.vertex_id(&5).unwrap().index(), 0);
        assert_eq!(g.vertex_id(&4).unwrap().index(), 1);
        assert_eq!(g.vertex_id(&7).unwrap().index(), 2);
        assert_eq!(g.vertex_id(&99), None);
        assert!(g.contains_vertex(&6));
        assert!(!g.contains_vertex(&99));

        let seen: Vec<u32> = g.vertices().map(|(_, label)| *label).collect();
        assert_eq!(seen, vec![5, 4, 7, 1, 0, 2, 3, 6]);
        for (id, label) in g.vertices() {
            assert_eq!(g.label(id), Some(label));
            assert_eq!(g.vertex_id(label), Some(id));
        }
    }

    #[test]
    fn edge_order_is_insertion_order() {
        let g = sample();
        let weights: Vec<f64> = g.edges().iter().map(DirectedEdge::weight).collect();
        assert_eq!(
            weights,
            vec![
                0.35, 0.37, 0.28, 0.32, 0.38, 0.26, 0.39, 0.29, 0.34, 0.40, 0.52, 0.58, 0.93
            ]
        );

        let five = g.vertex_id(&5).unwrap();
        let out: Vec<f64> = g.edges_from(five).map(DirectedEdge::weight).collect();
        assert_eq!(out, vec![0.35, 0.28, 0.32]);
        assert_eq!(g.out_degree(five), 3);

        // A head-only vertex exists and has no outgoing edges.
        let two = g.vertex_id(&2).unwrap();
        assert_eq!(g.out_degree(two), 0);
        assert_eq!(g.edges_from(two).count(), 0);
    }

    #[test]
    fn parallel_edges_and_self_loops_are_kept() {
        let mut g: EdgeWeightedDigraph<u32> = EdgeWeightedDigraph::new();
        g.add(&0, &1, 1.0).unwrap();
        g.add(&0, &1, 2.0).unwrap();
        g.add(&0, &0, 3.0).unwrap();
        assert_eq!(g.edge_count(), 3);
        assert_eq!(g.vertex_count(), 2);
        assert_eq!(g.out_degree(g.vertex_id(&0).unwrap()), 3);
    }

    #[test]
    fn non_finite_weights_are_refused_without_side_effects() {
        let mut g: EdgeWeightedDigraph<u32> = EdgeWeightedDigraph::new();
        for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(g.add(&0, &1, bad), Err(GraphError::NonFiniteWeight));
        }
        assert_eq!(g.vertex_count(), 0);
        assert_eq!(g.edge_count(), 0);
        assert_eq!(
            GraphError::NonFiniteWeight.to_string(),
            "edge weight must be finite"
        );
        assert_eq!(
            GraphError::VertexLimit.to_string(),
            "graph cannot hold more than u32::MAX vertices"
        );

        // Signed zero is finite and therefore accepted, with its sign intact.
        g.add(&0, &1, -0.0).unwrap();
        assert!(g.edges()[0].weight().is_sign_negative());
    }

    #[test]
    fn string_labels_are_stored_twice_and_not_per_edge() {
        let mut g: EdgeWeightedDigraph<String> = EdgeWeightedDigraph::new();
        g.add("compile", "link", 2.0).unwrap();
        g.add("link", "package", 1.0).unwrap();
        g.add("compile", "package", 9.0).unwrap();
        assert_eq!(g.vertex_count(), 3);
        assert_eq!(g.edge_count(), 3);
        assert_eq!(g.vertex_id("compile").unwrap().index(), 0);
        assert_eq!(g.label(VertexId(2)).map(String::as_str), Some("package"));
        assert_eq!(
            g.to_string(),
            "compile -> link, 2\nlink -> package, 1\ncompile -> package, 9"
        );
    }

    #[test]
    fn non_ascii_labels_round_trip() {
        let mut g: EdgeWeightedDigraph<String> = EdgeWeightedDigraph::new();
        g.add("日本", "中文", 0.5).unwrap();
        g.add("café", "😀", 0.25).unwrap();
        assert_eq!(g.vertex_count(), 4);
        assert!(g.contains_vertex("😀"));
        // NFC "café" and NFD "cafe\u{301}" are different labels: `Eq` on `str`
        // compares scalar sequences and nothing normalises.
        assert!(!g.contains_vertex("cafe\u{301}"));
        assert_eq!(g.to_string(), "日本 -> 中文, 0.5\ncafé -> 😀, 0.25");
    }

    #[test]
    fn display_uses_rust_float_formatting() {
        let mut g: EdgeWeightedDigraph<u32> = EdgeWeightedDigraph::new();
        g.add(&6, &2, 0.40).unwrap();
        g.add(&0, &1, 1e21).unwrap();
        g.add(&0, &2, -0.0).unwrap();
        assert_eq!(
            g.to_string(),
            "6 -> 2, 0.4\n0 -> 1, 1000000000000000000000\n0 -> 2, -0"
        );
    }

    /// The predecessor of this type was neither: its sparse adjacency store
    /// keyed string vertices by `Rc<str>`, so a graph could not cross a thread
    /// boundary at all.
    #[test]
    fn a_graph_can_be_shared_across_threads() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<EdgeWeightedDigraph<u32>>();
        assert_send_sync::<EdgeWeightedDigraph<String>>();
        assert_send_sync::<DirectedEdge>();
        assert_send_sync::<VertexId>();

        let graph = std::sync::Arc::new(sample());
        let handles: Vec<_> = (0..4)
            .map(|_| {
                let graph = std::sync::Arc::clone(&graph);
                std::thread::spawn(move || graph.vertex_count())
            })
            .collect();
        for handle in handles {
            assert_eq!(handle.join().unwrap(), 8);
        }
    }

    #[test]
    fn vertex_id_display_and_index() {
        assert_eq!(VertexId(7).to_string(), "#7");
        assert_eq!(VertexId(7).index(), 7);
        assert!(VertexId(1) < VertexId(2));
    }

    #[test]
    fn with_capacity_matches_new() {
        let mut a: EdgeWeightedDigraph<u32> = EdgeWeightedDigraph::with_capacity(4, 8);
        let mut b: EdgeWeightedDigraph<u32> = EdgeWeightedDigraph::new();
        for (x, y, w) in [(0, 1, 1.0), (1, 2, 2.0)] {
            a.add(&x, &y, w).unwrap();
            b.add(&x, &y, w).unwrap();
        }
        assert_eq!(a.to_string(), b.to_string());
        assert_eq!(a.vertex_count(), b.vertex_count());
    }
}
