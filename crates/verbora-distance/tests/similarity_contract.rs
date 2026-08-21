//! The cross-function similarity contract (`docs/design/distance-contract.md`
//! §6.6): the properties every similarity in this crate shares, asserted
//! across all three of them at once rather than once per module.
//!
//! Each metric's own suite pins its arithmetic. What this file pins is that
//! [`jaro`], [`jaro_winkler`] and [`dice_coefficient`] agree about the
//! degenerate cases and about totality — the two places where three
//! independently-derived formulas could each be internally consistent and
//! still contradict one another at the seams. Both were genuinely violated
//! before the Rust-native migration: `jaro("a", "a")` was `0.0` while
//! `jaro_winkler("a", "a")` was `1.0`, and `dice_coefficient("", "")` was
//! `NaN`.

use verbora_distance::{dice_coefficient, jaro, jaro_winkler};

/// A similarity, named so a failure says which one it was.
type Similarity = (&'static str, fn(&str, &str) -> f64);

/// Every similarity this crate publishes, so a new one added here inherits
/// the whole file.
const SIMILARITIES: &[Similarity] = &[
    ("jaro", jaro),
    ("jaro_winkler", jaro_winkler),
    ("dice_coefficient", dice_coefficient),
];

/// One sentence governs all three: **identical inputs score 1.0; inputs
/// sharing no feature score 0.0.** The `0/0` cases in Jaro's `m == 0` clause
/// and in Dice's `|A| + |B| == 0` clause are removable singularities, and
/// which value to insert is forced by consistency — two empty strings are
/// *identical*, not *disjoint*.
#[test]
fn the_degenerate_table_is_shared_by_every_similarity() {
    const TABLE: &[(&str, &str, f64)] = &[
        ("", "", 1.0),
        ("", "a", 0.0),
        ("a", "", 0.0),
        ("a", "a", 1.0),
        ("a", "b", 0.0),
    ];

    for &(name, f) in SIMILARITIES {
        for &(a, b, want) in TABLE {
            let got = f(a, b);
            assert_eq!(
                got.to_bits(),
                want.to_bits(),
                "{name}({a:?}, {b:?}) = {got}, expected {want}"
            );
        }
    }
}

/// No similarity ever returns a non-finite value, over the union of every
/// degenerate corpus the per-metric suites use: empty and single-scalar
/// operands, the whitespace and format scalars Dice's deleted preprocessing
/// step used to erase, astral scalars (one unit each, so they land in the
/// same degenerate class as `"a"`), and repeated-scalar operands.
///
/// A `NaN` escaping here would be invisible: it is not orderable, so
/// `max_by` over a candidate list would give order-dependent results and a
/// single poisoned score would silently corrupt a ranking.
#[test]
fn no_similarity_returns_a_non_finite_value() {
    const CORPUS: &[&str] = &[
        "",
        " ",
        "  ",
        "\t",
        "\n",
        "\u{FEFF}",
        "\u{0085}",
        "a",
        "A",
        "ab",
        "aaaa",
        "😀",
        "😀😁",
        "a😀b",
        "क्षि",
        "Москва",
        "Hello  World",
        "hello world",
    ];

    for &(name, f) in SIMILARITIES {
        for a in CORPUS {
            for b in CORPUS {
                let got = f(a, b);
                assert!(
                    got.is_finite(),
                    "{name}({a:?}, {b:?}) = {got} is not finite"
                );
                assert!(
                    (0.0..=1.0).contains(&got),
                    "{name}({a:?}, {b:?}) = {got} is outside [0, 1]"
                );
                assert_eq!(
                    got.to_bits(),
                    f(b, a).to_bits(),
                    "{name} is not symmetric on ({a:?}, {b:?})"
                );
                if a == b {
                    assert_eq!(
                        got.to_bits(),
                        1.0f64.to_bits(),
                        "{name}({a:?}, {a:?}) is not exactly 1.0"
                    );
                }
            }
        }
    }
}

/// None of the three folds case, so the caller-side recipe is the one that
/// recovers a caseless match — and it is the same recipe for all three.
#[test]
fn no_similarity_folds_case() {
    for &(name, f) in SIMILARITIES {
        for (a, b) in [("ABC", "abc"), ("MARTHA", "martha"), ("Ä", "ä")] {
            assert!(
                f(a, b) < 1.0,
                "{name}({a:?}, {b:?}) folded case, but no metric here rewrites its inputs"
            );
            assert_eq!(
                f(&a.to_lowercase(), &b.to_lowercase()),
                1.0,
                "{name} on the caller-folded pair ({a:?}, {b:?})"
            );
        }
    }
}
