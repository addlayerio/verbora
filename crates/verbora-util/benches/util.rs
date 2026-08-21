// `criterion_group!` expands to an undocumented function, and a benchmark
// harness is not public API, so the workspace-wide `missing_docs` lint is noise
// here.
#![allow(missing_docs)]

//! Criterion benchmarks for `verbora-util`.
//!
//! Three things are measured, chosen because they are what a caller actually
//! spends time in:
//!
//! * **graph construction and traversal** — `add`, `edges`, `Topological` and
//!   both path trees, over chains, layered DAGs and complete DAGs, so the shape
//!   of the graph (long-and-thin versus short-and-wide) is visible;
//! * **string-labelled construction**, which is the shape that pays for
//!   interning: a label is cloned only when its vertex is new, so a graph whose
//!   vertices repeat allocates far less than one whose vertices are all
//!   distinct. Both shapes are measured;
//! * **stop-word and abbreviation membership**, a binary search over a
//!   de-duplicated view built on first use.
//!
//! Graphs are generated from the same LCG every other harness uses, so all
//! numbers describe byte-identical inputs without needing a shared data file.
//! `benches/data/words.json` — shared with the other crates — supplies the
//! membership and string-label inputs.
//!
//! Nothing here has been run since the crate's Rust-native migration; the
//! figures any previous run produced described a different implementation and a
//! different API, and none of them is carried forward.

use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use verbora_util::{
    AbbreviationLanguage, EdgeWeightedDigraph, Language, LongestPathTree, ShortestPathTree,
    Topological,
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
    /// have.
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

/// A named benchmark graph: edges as (from, to, weight).
type BenchGraph = (&'static str, Vec<(u32, u32, f64)>);

fn graphs() -> Vec<BenchGraph> {
    vec![
        ("chain-1000", chain(1000)),
        ("layered-20x20", layered(20, 20)),
        ("complete-64", complete_dag(64)),
    ]
}

fn build(edges: &[(u32, u32, f64)]) -> EdgeWeightedDigraph<u32> {
    let mut g = EdgeWeightedDigraph::new();
    for &(a, b, w) in edges {
        g.add(&a, &b, w).expect("finite weights");
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

    // Preallocated: the same edges into a graph told how big it will be, which
    // is the only knob `with_capacity` turns.
    let edges = complete_dag(64);
    let vertices = 64;
    g.throughput(Throughput::Elements(edges.len() as u64));
    g.bench_with_input(
        BenchmarkId::from_parameter("complete-64/with_capacity"),
        &edges,
        |b, edges| {
            b.iter(|| {
                let mut graph = EdgeWeightedDigraph::with_capacity(vertices, edges.len());
                for &(a, bb, w) in black_box(edges) {
                    graph.add(&a, &bb, w).expect("finite weights");
                }
                graph
            });
        },
    );
    g.finish();
}

/// String labels, in the two shapes that differ in allocation behaviour.
///
/// Interning clones a label only when its vertex is new, so:
///
/// * `distinct` chains the corpus word by word — `word[i] -> word[i+1]`, almost
///   all distinct — so nearly every `add` mints a vertex and pays two `String`
///   clones;
/// * `repeated` draws both endpoints from a small pool, so almost every `add`
///   finds an existing vertex and clones nothing.
///
/// The gap between the two is the cost interning avoids on the common shape; a
/// design that stored a label per edge endpoint would pay the `distinct` price
/// on both.
fn bench_string_labels(c: &mut Criterion) {
    let words = words();
    let distinct: Vec<(String, String)> = words
        .windows(2)
        .map(|w| (w[0].clone(), w[1].clone()))
        .collect();
    let pool: Vec<String> = words.iter().take(64).cloned().collect();
    let repeated: Vec<(String, String)> = (0..distinct.len())
        .map(|i| (pool[i % 63].clone(), pool[(i % 63) + 1].clone()))
        .collect();

    let mut g = c.benchmark_group("digraph_build_string");
    for (label, pairs) in [("distinct", &distinct), ("repeated", &repeated)] {
        g.throughput(Throughput::Elements(pairs.len() as u64));
        g.bench_with_input(BenchmarkId::from_parameter(label), pairs, |b, pairs| {
            b.iter(|| {
                let mut graph: EdgeWeightedDigraph<String> = EdgeWeightedDigraph::new();
                for (from, to) in black_box(pairs) {
                    // Cyclic input is fine: only construction is timed.
                    graph.add(from.as_str(), to.as_str(), 1.0).expect("finite");
                }
                graph
            });
        });
    }
    g.finish();
}

fn bench_digraph_traversal(c: &mut Criterion) {
    let mut g = c.benchmark_group("digraph_edges");
    for (label, edges) in graphs() {
        let graph = build(&edges);
        g.throughput(Throughput::Elements(edges.len() as u64));
        g.bench_with_input(BenchmarkId::new("iterate", label), &graph, |b, graph| {
            b.iter(|| {
                black_box(graph)
                    .edges()
                    .iter()
                    .map(verbora_util::DirectedEdge::weight)
                    .sum::<f64>()
            });
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
            b.iter(|| Topological::new(black_box(graph)).expect("acyclic"));
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
            b.iter(|| ShortestPathTree::new(black_box(graph), &0).expect("acyclic"));
        });
        g.bench_with_input(BenchmarkId::new("longest", label), &graph, |b, graph| {
            b.iter(|| LongestPathTree::new(black_box(graph), &0).expect("acyclic"));
        });
    }

    // Path extraction on a built tree: the chain's last vertex is 1000 edges
    // from the source, so this is the worst case the walk can face. Both the id
    // form and the label form are timed, since the pair is a documented choice.
    let graph = build(&chain(1000));
    let tree = ShortestPathTree::new(&graph, &0).expect("acyclic");
    let target = graph.vertex_id(&1000).expect("built above");
    g.throughput(Throughput::Elements(1000));
    g.bench_function("path/chain-1000", |b| {
        b.iter(|| black_box(&tree).path(black_box(target)));
    });
    g.bench_function("path_labels/chain-1000", |b| {
        b.iter(|| black_box(&tree).path_labels(black_box(target)));
    });
    g.bench_function("distance_of/chain-1000", |b| {
        b.iter(|| black_box(&tree).distance_of(&1000u32));
    });
    g.bench_function("distance/chain-1000", |b| {
        b.iter(|| black_box(&tree).distance(black_box(target)));
    });
    g.finish();
}

/// Words shared with the other crates' benchmarks.
fn words() -> Vec<String> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crate is two levels below the workspace root")
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
        .map(|w| w.as_str().expect("a string").to_owned())
        .collect()
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
    // Indonesian is the longest list (809 entries), where a linear scan would
    // hurt most.
    g.bench_with_input("stopwords/id", &probes, |b, probes| {
        b.iter(|| {
            black_box(probes)
                .iter()
                .filter(|w| Language::Id.is_stopword(w))
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
    bench_string_labels,
    bench_digraph_traversal,
    bench_topological,
    bench_path_trees,
    bench_membership
);
criterion_main!(benches);
