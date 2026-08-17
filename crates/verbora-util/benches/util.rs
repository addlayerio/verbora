// `criterion_group!` expands to an undocumented function, and a benchmark
// harness is not public API, so the workspace-wide `missing_docs` lint is noise
// here.
#![allow(missing_docs)]

//! Criterion benchmarks for `verbora-util`.
//!
//! Four things are measured, chosen because they are what a caller actually
//! spends time in:
//!
//! * **graph construction and traversal** — `add`, `edges`, `Topological`, and
//!   both path trees, over chains, layered DAGs and complete DAGs, so the shape
//!   of the graph (long-and-thin versus short-and-wide) is visible;
//! * **the numeric primitives** `snap_2` and `format_s`, which the relaxation
//!   loop and `toString` respectively call once per edge;
//! * **stop-word and abbreviation membership**, where the port replaces a linear
//!   `indexOf` over a 428-entry array with a binary search;
//! * nothing from `storage` — a file round trip measures the filesystem, not
//!   this crate, and would leave tens of thousands of files behind.
//!
//! Graphs are generated from the same LCG every other harness uses, so all
//! numbers describe byte-identical inputs without needing a shared data file.
//! `benches/data/words.json` — shared with the other crates — supplies the
//! membership inputs.

use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use verbora_util::numfmt::{format_s, snap_2};
use verbora_util::{
    AbbreviationLanguage, DirectedEdge, EdgeWeightedDigraph, Language, LongestPathTree,
    ShortestPathTree, Topological, Vertex,
};

/// The deterministic 64-bit LCG shared with `tools/bench-data/generate.py`.
struct Lcg(u64);

impl Lcg {
    fn new(seed: u64) -> Self {
        Self(seed)
    }

    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }

    /// A weight in `0.00..=9.99`, two decimals — the shape real edge weights
    /// have, and the shape `snap_2` is applied to.
    fn weight(&mut self) -> f64 {
        (self.next() % 1000) as f64 / 100.0
    }
}

/// `0 -> 1 -> … -> n`: the deepest possible traversal for its edge count.
fn chain(n: u32) -> Vec<(u32, u32, f64)> {
    let mut rng = Lcg::new(1);
    (0..n).map(|i| (i, i + 1, rng.weight())).collect()
}

/// `layers` ranks of `width` vertices, fully connected between adjacent ranks.
fn layered(layers: u32, width: u32) -> Vec<(u32, u32, f64)> {
    let mut rng = Lcg::new(2);
    let mut edges = Vec::new();
    for l in 0..layers - 1 {
        for a in 0..width {
            for b in 0..width {
                edges.push((l * width + a, (l + 1) * width + b, rng.weight()));
            }
        }
    }
    edges
}

/// Every `i -> j` with `i < j`: the densest DAG on `n` vertices.
fn complete_dag(n: u32) -> Vec<(u32, u32, f64)> {
    let mut rng = Lcg::new(3);
    let mut edges = Vec::new();
    for i in 0..n {
        for j in i + 1..n {
            edges.push((i, j, rng.weight()));
        }
    }
    edges
}

/// The benchmark graphs, by label.
/// A named benchmark graph: edges as (from, to, weight).
type BenchGraph = (&'static str, Vec<(u32, u32, f64)>);

fn graphs() -> Vec<BenchGraph> {
    vec![
        ("chain-1000", chain(1000)),
        ("layered-20x20", layered(20, 20)),
        ("complete-64", complete_dag(64)),
    ]
}

fn build(edges: &[(u32, u32, f64)]) -> EdgeWeightedDigraph {
    let mut g = EdgeWeightedDigraph::new();
    for &(a, b, w) in edges {
        g.add(a, b, w);
    }
    g
}

fn bench_digraph_build(c: &mut Criterion) {
    let mut g = c.benchmark_group("digraph_build");
    for (label, edges) in graphs() {
        g.throughput(Throughput::Elements(edges.len() as u64));
        g.bench_with_input(BenchmarkId::from_parameter(label), &edges, |b, edges| {
            b.iter(|| build(black_box(edges)));
        });
    }
    // The pre-built-edge path, which skips one `Vertex` construction per edge.
    let edges: Vec<DirectedEdge> = chain(1000)
        .into_iter()
        .map(|(a, b, w)| DirectedEdge::new(a, b, w))
        .collect();
    g.throughput(Throughput::Elements(edges.len() as u64));
    g.bench_with_input(
        BenchmarkId::from_parameter("add_edge/1000"),
        &edges,
        |b, edges| {
            b.iter(|| {
                let mut graph = EdgeWeightedDigraph::new();
                for e in edges {
                    graph.add_edge(e.clone());
                }
                graph
            });
        },
    );

    // String-vertex edges: the shape that pays for `add_edge`'s tail clone —
    // a `Vertex::Str` clone allocates and copies, where a numeric one is a
    // stack copy. Chained words from the shared corpus, `word[i] -> word[i+1]`,
    // almost all distinct, so nearly every `add_edge` takes the
    // new-vertex path. 20,000 edges rather than 1,000 so the allocation this
    // measures is a large enough share of the total to clear this machine's
    // scheduling noise.
    let words = words();
    let string_edges: Vec<DirectedEdge> = words
        .windows(2)
        .map(|w| DirectedEdge::new(w[0].as_str(), w[1].as_str(), 1.0))
        .collect();
    g.throughput(Throughput::Elements(string_edges.len() as u64));
    g.bench_with_input(
        BenchmarkId::from_parameter("add_edge_string/20000"),
        &string_edges,
        |b, edges| {
            b.iter(|| {
                let mut graph = EdgeWeightedDigraph::new();
                for e in edges {
                    graph.add_edge(e.clone());
                }
                graph
            });
        },
    );
    g.finish();
}

fn bench_digraph_traversal(c: &mut Criterion) {
    let mut g = c.benchmark_group("digraph_edges");
    for (label, edges) in graphs() {
        let graph = build(&edges);
        g.throughput(Throughput::Elements(edges.len() as u64));
        g.bench_with_input(BenchmarkId::new("iterate", label), &graph, |b, graph| {
            // The lazy primitive: no intermediate Vec, no Bag copies.
            b.iter(|| black_box(graph).edges().map(|e| e.weight()).sum::<f64>());
        });
        g.bench_with_input(BenchmarkId::new("to_string", label), &graph, |b, graph| {
            b.iter(|| black_box(graph).to_string());
        });
    }
    g.finish();
}

fn bench_topological(c: &mut Criterion) {
    let mut g = c.benchmark_group("topological");
    for (label, edges) in graphs() {
        let graph = build(&edges);
        g.throughput(Throughput::Elements(edges.len() as u64));
        g.bench_with_input(BenchmarkId::from_parameter(label), &graph, |b, graph| {
            b.iter(|| Topological::new(black_box(graph)).unwrap());
        });
    }
    g.finish();
}

fn bench_path_trees(c: &mut Criterion) {
    let mut g = c.benchmark_group("path_trees");
    for (label, edges) in graphs() {
        let graph = build(&edges);
        g.throughput(Throughput::Elements(edges.len() as u64));
        g.bench_with_input(BenchmarkId::new("shortest", label), &graph, |b, graph| {
            b.iter(|| ShortestPathTree::new(black_box(graph), 0).unwrap());
        });
        g.bench_with_input(BenchmarkId::new("longest", label), &graph, |b, graph| {
            b.iter(|| LongestPathTree::new(black_box(graph), 0).unwrap());
        });
    }

    // Path extraction on a built tree: the chain's last vertex is 1000 edges
    // from the source, so this is the worst case the walk can face.
    let graph = build(&chain(1000));
    let tree = ShortestPathTree::new(&graph, 0).unwrap();
    let target = Vertex::from(1000);
    g.throughput(Throughput::Elements(1000));
    g.bench_function("path_to/chain-1000", |b| {
        b.iter(|| black_box(&tree).path_to(black_box(&target)));
    });
    g.finish();
}

fn bench_refnum(c: &mut Criterion) {
    let mut g = c.benchmark_group("numfmt");

    // The values the relaxation loop actually snaps: a running distance plus an
    // edge weight, so mostly two-decimal sums with binary noise.
    let mut rng = Lcg::new(7);
    let values: Vec<f64> = (0..1024)
        .map(|_| rng.weight() + rng.weight() / 3.0)
        .collect();

    g.throughput(Throughput::Elements(values.len() as u64));
    g.bench_with_input("snap_2", &values, |b, values| {
        b.iter(|| black_box(values).iter().map(|v| snap_2(*v)).sum::<f64>());
    });
    g.bench_with_input("format_s", &values, |b, values| {
        b.iter(|| {
            black_box(values)
                .iter()
                .map(|v| format_s(*v).len())
                .sum::<usize>()
        });
    });
    g.finish();
}

/// Words shared with the other crates' benchmarks.
fn words() -> Vec<String> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap()
        .join("benches/data/words.json");
    let body = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "cannot read {}: {e}\nGenerate it with: python3 tools/bench-data/generate.py",
            path.display()
        )
    });
    let json: serde_json::Value = serde_json::from_str(&body).expect("valid bench data");
    json["words"]
        .as_array()
        .expect("words array")
        .iter()
        .map(|w| w.as_str().unwrap().to_owned())
        .collect()
}

/// Isolates the cost `EdgeWeightedDigraph::add_edge` removed a clone of: a
/// `Vertex::Str` clone allocates and copies its `String`, where a
/// `Vertex::Num` clone is a stack copy. The end-to-end `add_edge_string`
/// benchmark above is too small a share of total graph-build work to clear
/// this machine's scheduling noise; this isolates exactly the operation in
/// question.
fn bench_vertex_clone(c: &mut Criterion) {
    let words = words();
    let string_vertices: Vec<Vertex> = words.iter().map(|w| Vertex::from(w.as_str())).collect();
    let num_vertices: Vec<Vertex> = (0..words.len() as u32).map(Vertex::from).collect();

    let mut g = c.benchmark_group("vertex_clone");
    g.throughput(Throughput::Elements(string_vertices.len() as u64));
    g.bench_function("str", |b| {
        b.iter(|| {
            for v in &string_vertices {
                black_box(v.clone());
            }
        });
    });
    g.bench_function("num", |b| {
        b.iter(|| {
            for v in &num_vertices {
                black_box(v.clone());
            }
        });
    });
    g.finish();
}

/// Isolates the cost `SparseArray::set` removed for a newly-seen string key: a
/// `Box<str>` clone (allocate + copy the bytes) versus an `Rc<str>` clone
/// (bump a reference count). `SparseArray` is crate-private, so this measures the
/// two allocation strategies directly on the same corpus rather than through
/// the graph's public API, which — like `add_edge_string` above — is too
/// noisy at this scale to isolate one allocation inside a much larger
/// end-to-end call.
fn bench_named_key_clone(c: &mut Criterion) {
    let words = words();
    let boxed: Vec<Box<str>> = words.iter().map(|w| w.as_str().into()).collect();
    let rc: Vec<std::rc::Rc<str>> = words
        .iter()
        .map(|w| std::rc::Rc::from(w.as_str()))
        .collect();

    let mut g = c.benchmark_group("named_key_clone");
    g.throughput(Throughput::Elements(boxed.len() as u64));
    g.bench_function("box_str", |b| {
        b.iter(|| {
            for s in &boxed {
                black_box(s.clone());
            }
        });
    });
    g.bench_function("rc_str", |b| {
        b.iter(|| {
            for s in &rc {
                black_box(std::rc::Rc::clone(s));
            }
        });
    });
    g.finish();
}

fn bench_membership(c: &mut Criterion) {
    let words = words();
    // Half real stop words, half misses: a lookup that always misses would
    // flatter binary search, since it never has to compare a full string.
    let mut probes: Vec<&str> = Vec::with_capacity(20_000);
    let en = Language::En.stopwords();
    for (i, w) in words.iter().take(10_000).enumerate() {
        probes.push(w.as_str());
        probes.push(en[i % en.len()]);
    }

    let mut g = c.benchmark_group("membership");
    g.throughput(Throughput::Elements(probes.len() as u64));
    g.bench_with_input("stopwords/en", &probes, |b, probes| {
        b.iter(|| {
            black_box(probes)
                .iter()
                .filter(|w| Language::En.is_stopword(w))
                .count()
        });
    });
    // Swedish is the longest list (428 entries), where a linear scan hurts most.
    g.bench_with_input("stopwords/sv", &probes, |b, probes| {
        b.iter(|| {
            black_box(probes)
                .iter()
                .filter(|w| Language::Sv.is_stopword(w))
                .count()
        });
    });
    g.bench_with_input("abbreviations/es", &probes, |b, probes| {
        b.iter(|| {
            black_box(probes)
                .iter()
                .filter(|w| AbbreviationLanguage::Es.contains(w))
                .count()
        });
    });
    g.finish();
}

criterion_group!(
    benches,
    bench_digraph_build,
    bench_digraph_traversal,
    bench_topological,
    bench_path_trees,
    bench_refnum,
    bench_vertex_clone,
    bench_named_key_clone,
    bench_membership
);
criterion_main!(benches);
