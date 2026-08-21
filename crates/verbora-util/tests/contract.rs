//! The public contract, asserted from outside the crate.
//!
//! Every test here corresponds to a defect that was reproduced against the
//! previous implementation before it was changed — each of these failed then and
//! passes now. They live in an integration test rather than beside the code so
//! that they exercise only what the crate root actually exports: a regression
//! that re-privatises or renames part of the surface fails here even if the unit
//! tests still compile.

use verbora_util::{
    AbbreviationLanguage, EdgeWeightedDigraph, GraphError, LANGUAGES, Language, LongestPathTree,
    PathError, ShortestPathTree, Topological,
};

fn chain(edges: &[(u32, u32, f64)]) -> EdgeWeightedDigraph<u32> {
    let mut g = EdgeWeightedDigraph::new();
    for &(a, b, w) in edges {
        g.add(&a, &b, w).unwrap();
    }
    g
}

/// A tree's reported distance equals the sum of the weights along the path it
/// itself reports.
///
/// Previously `0.26`: every accepted distance was rounded to two decimals as it
/// was stored, so the rounding compounded along the path and the tree disagreed
/// with its own edges.
#[test]
fn a_distance_equals_the_sum_along_its_own_path() {
    let g = chain(&[(0, 1, 0.125), (1, 2, 0.125)]);
    let tree = ShortestPathTree::new(&g, &0).unwrap();
    assert_eq!(tree.distance_of(&2), Some(0.25));
}

/// No path is invented across components.
///
/// Previously `path_to(3)` returned `[0, 3]` — the source prepended to a chain
/// that never reached it — on a graph where no such path exists.
#[test]
fn no_path_is_invented_on_a_disconnected_graph() {
    let g = chain(&[(0, 1, 1.0), (2, 3, 1.0)]);
    let tree = ShortestPathTree::new(&g, &0).unwrap();
    assert_eq!(tree.path_of(&3), None);
    assert_eq!(tree.distance_of(&3), None);
    assert_eq!(tree.distance_of(&2), None);
}

/// The source is reachable from itself by the path of no edges.
///
/// Previously `has_path_to(source)` was `false` and `path_to(source)` empty.
#[test]
fn the_source_is_reachable_from_itself() {
    let g = chain(&[(0, 1, 1.0)]);
    let tree = ShortestPathTree::new(&g, &0).unwrap();
    assert_eq!(tree.distance_of(&0), Some(0.0));
    assert_eq!(tree.path_labels_of(&0), Some(vec![&0]));
}

/// No sentinel escapes as a distance.
///
/// Previously a vertex reached only through a `NaN` edge kept the initialisation
/// sentinel and `dist_to` returned `Some(f64::MAX)`.
#[test]
fn no_sentinel_or_non_finite_distance_can_escape() {
    let mut g: EdgeWeightedDigraph<u32> = EdgeWeightedDigraph::new();
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert_eq!(g.add(&0, &1, bad), Err(GraphError::NonFiniteWeight));
    }
    assert!(g.is_empty());

    // The only remaining route to a non-finite total is an overflowing sum, and
    // that is an error rather than a distance.
    let g = chain(&[(0, 1, f64::MAX), (1, 2, f64::MAX)]);
    assert!(matches!(
        LongestPathTree::new(&g, &0),
        Err(PathError::Overflow(_))
    ));
}

/// A vertex count counts vertices.
///
/// Previously `v()` reported one past the largest integer vertex label, so a
/// single edge `10 -> 3` claimed eleven vertices.
#[test]
fn a_vertex_count_counts_vertices() {
    let g = chain(&[(10, 3, 1.0)]);
    assert_eq!(g.vertex_count(), 2);
    assert_eq!(g.edge_count(), 1);
}

/// `is_empty` means empty.
///
/// The adjacency-list type this replaced shipped an `is_empty` that returned
/// `true` exactly when the collection was *not* empty.
#[test]
fn is_empty_means_empty() {
    let mut g: EdgeWeightedDigraph<u32> = EdgeWeightedDigraph::new();
    assert!(g.is_empty());
    g.add(&0, &1, 1.0).unwrap();
    assert!(!g.is_empty());
}

/// A cyclic graph is refused, and the report names every trapped vertex.
#[test]
fn cycles_are_refused_by_everything_that_needs_a_dag() {
    let g = chain(&[(0, 1, 1.0), (1, 2, 1.0), (2, 0, 1.0)]);
    let cycle = Topological::new(&g).unwrap_err();
    assert_eq!(cycle.vertices().len(), 3);
    assert!(matches!(
        ShortestPathTree::new(&g, &0),
        Err(PathError::Cyclic(_))
    ));
    assert!(matches!(
        LongestPathTree::new(&g, &0),
        Err(PathError::Cyclic(_))
    ));
}

/// Membership is a pure, case-sensitive comparison of scalar sequences.
#[test]
fn table_membership_is_exact_and_pure() {
    assert!(Language::En.is_stopword("the"));
    assert!(!Language::En.is_stopword("The"));
    assert!(!Language::Sv.is_stopword(""));
    assert!(AbbreviationLanguage::En.contains("Dr."));
    assert!(!AbbreviationLanguage::En.contains("dr."));

    // English membership does not move when the process-global list does.
    verbora_util::add_global_stopword("verbora");
    assert!(verbora_util::is_global_stopword("verbora"));
    assert!(!Language::En.is_stopword("verbora"));
    verbora_util::reset_global_stopwords();
}

/// Every stop-word entry of every language is already NFC.
///
/// `verbora-core` owns the tables and deliberately carries no Unicode
/// dependency, so it checks this property by rejecting the combining marks a
/// decomposed spelling would need. This is the same claim checked from the
/// other side, with a real UAX #15 normalizer, over every entry of all sixteen
/// lists rather than a sample: an entry in NFD would be a silent dead entry for
/// every caller who normalises input to NFC, which is the same failure mode as
/// a stray trailing space one layer down.
#[test]
fn every_stopword_entry_is_already_nfc() {
    use unicode_normalization::UnicodeNormalization;

    let mut checked = 0usize;
    for &lang in LANGUAGES {
        for &entry in lang.stopwords() {
            let nfc: String = entry.nfc().collect();
            assert_eq!(nfc, entry, "{}: {entry:?} is not NFC", lang.code());
            checked += 1;
        }
    }
    assert_eq!(checked, 3_707, "the total entry count changed");

    // The consequence a caller can observe.
    assert!(Language::Fr.is_stopword("été"));
    let decomposed: String = "été".nfd().collect();
    assert_ne!(decomposed, "été");
    assert!(!Language::Fr.is_stopword(&decomposed));
}

/// Nothing in the public surface panics on a hostile argument.
#[test]
fn no_public_call_panics_on_awkward_input() {
    let g = chain(&[(0, 1, 1.0)]);
    let tree = ShortestPathTree::new(&g, &0).unwrap();

    assert_eq!(tree.distance_of(&u32::MAX), None);
    assert_eq!(tree.path_of(&u32::MAX), None);
    assert_eq!(g.vertex_id(&u32::MAX), None);
    assert_eq!(g.out_degree(g.vertex_id(&1).unwrap()), 0);
    assert_eq!(
        ShortestPathTree::new(&g, &7).unwrap_err(),
        PathError::UnknownSource
    );

    for probe in ["", " ", "\u{0}", "😀", "e\u{301}", &"x".repeat(4096)] {
        assert!(!Language::Fr.is_stopword(probe));
        assert!(!AbbreviationLanguage::Es.contains(probe));
    }
    assert_eq!(Language::from_code(""), None);
}
