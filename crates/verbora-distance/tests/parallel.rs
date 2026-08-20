//! Sequential-vs-parallel parity for the `par_*_batch` functions (`parallel`
//! feature only).
//!
//! Each `par_*_batch` function is architecturally required to be
//! `pairs.par_iter().map(<the sequential fn>).collect()` — a thin fan-out over
//! the existing sequential primitive, nothing more. This suite proves that by
//! comparing its output, item for item and in order, against a plain
//! `pairs.iter().map(<the sequential fn>).collect()` loop it must be
//! indistinguishable from.
//!
//! Inputs reuse the edge cases each metric's own unit-test suite already knows
//! to exercise (`src/levenshtein.rs`, `src/jaro_winkler.rs`, `src/dice.rs`,
//! `src/hamming.rs`) rather than inventing new ones: empty strings, the
//! Damerau-vs-OSA divergence, astral-plane and BMP Unicode, the
//! single-unit Jaro window clamp, bigram-set collapsing, and scalar-count
//! mismatches for Hamming.

#![cfg(feature = "parallel")]

use verbora_distance::{
    DamerauCosts, damerau_levenshtein, hamming, jaro_winkler, levenshtein, osa,
    par_damerau_levenshtein_batch, par_dice_coefficient_batch, par_hamming_batch,
    par_jaro_winkler_batch, par_levenshtein_batch, par_osa_batch,
};

/// ASCII pairs already exercised by this crate's own unit-test suites:
/// `levenshtein::tests::{classic_distances, transposition_only_counts_for_damerau,
/// damerau_and_osa_are_different_functions, asymmetric_costs_are_respected,
/// every_fast_path_agrees_with_the_full_matrix}`,
/// `jaro_winkler::tests::published_worked_examples`,
/// `dice::tests::{identity_is_not_injective, operands_are_not_rewritten,
/// partial_overlap_is_between_zero_and_one}`,
/// `hamming::tests::counts_differing_positions`.
const PATHOLOGICAL: &[(&str, &str)] = &[
    ("kitten", "sitting"),
    ("saturday", "sunday"),
    ("", ""),
    ("abc", ""),
    ("", "abc"),
    ("same", "same"),
    ("ab", "ba"),  // transposition: 2 under Levenshtein, 1 under Damerau
    ("ca", "abc"), // Damerau vs. OSA diverge (2 vs 3)
    ("abc", "ab"), // one deletion
    ("ab", "abc"), // one insertion
    ("flaw", "lawn"),
    ("a", "abcdef"),
    ("abcdef", "fedcba"),
    ("MARTHA", "MARHTA"),
    ("DIXON", "DICKSONX"),
    ("DWAYNE", "DUANE"),
    ("a", "b"),     // single-unit Jaro: the window clamps to 0, the units differ
    ("aaaa", "aa"), // dice: repeated bigrams collapse to a set
    ("Hello  World", "hello world"),
    ("  padded  ", "padded"),
    ("night", "nacht"),
    ("karolin", "kathrin"),
    ("1011101", "1001001"),
    ("abc", "abc"),
];

/// Unicode pairs already exercised by this crate's own unit-test suites:
/// `levenshtein::tests::{scalar_semantics_count_astral_characters_once,
/// bmp_non_ascii_is_one_unit_per_char}`, `dice::tests::astral_characters_use_scalar_pairs`,
/// `hamming::tests::{length_is_measured_in_scalars, bmp_non_ascii_compares_per_character}`.
const UNICODE: &[(&str, &str)] = &[
    ("a😀b", "ab"), // 3 scalars vs 2: one deletion for Levenshtein
    ("😀", ""),
    ("😀", "😀"),
    ("café", "cafe"),
    ("Москва", "Москва"),
    ("a😀b", "abc"),  // 3 scalars each: comparable for Hamming
    ("a😀b", "abcd"), // 3 vs 4 scalars: incomparable for Hamming
    ("😀", "𝕳"),      // one scalar each, both astral
];

fn all_pairs() -> Vec<(&'static str, &'static str)> {
    PATHOLOGICAL.iter().chain(UNICODE.iter()).copied().collect()
}

/// `pairs` cycled out to a batch large enough to span several `rayon` tasks.
fn many_pairs() -> Vec<(&'static str, &'static str)> {
    all_pairs().into_iter().cycle().take(4096).collect()
}

/// Bitwise equality. Every `f64` this crate returns is finite
/// (`docs/design/distance-contract.md` §1), so there is no `NaN` case to
/// excuse and parity is exact: the parallel and sequential paths call the
/// same pure function on the same operands and must agree bit for bit, not
/// merely within a tolerance.
fn f64_eq(a: f64, b: f64) -> bool {
    a.to_bits() == b.to_bits()
}

fn assert_f64_parity(pairs: &[(&str, &str)], seq: impl Fn(&str, &str) -> f64, got: &[f64]) {
    assert_eq!(
        got.len(),
        pairs.len(),
        "batch of {} pairs produced {} results",
        pairs.len(),
        got.len()
    );
    for (i, (a, b)) in pairs.iter().enumerate() {
        let want = seq(a, b);
        assert!(
            f64_eq(got[i], want),
            "pair {i} ({a:?}, {b:?}): parallel={:?} sequential={want:?}",
            got[i]
        );
    }
}

/// Hamming reports `Option<usize>`: `None` is the ordinary answer for a
/// scalar-count mismatch, so parity has to compare the `Option`s themselves
/// rather than unwrapping them.
fn assert_option_parity(
    pairs: &[(&str, &str)],
    seq: impl Fn(&str, &str) -> Option<usize>,
    got: &[Option<usize>],
) {
    assert_eq!(
        got.len(),
        pairs.len(),
        "batch of {} pairs produced {} results",
        pairs.len(),
        got.len()
    );
    for (i, (a, b)) in pairs.iter().enumerate() {
        assert_eq!(got[i], seq(a, b), "pair {i} ({a:?}, {b:?}) diverged");
    }
}

fn assert_usize_parity(pairs: &[(&str, &str)], seq: impl Fn(&str, &str) -> usize, got: &[usize]) {
    assert_eq!(
        got.len(),
        pairs.len(),
        "batch of {} pairs produced {} results",
        pairs.len(),
        got.len()
    );
    for (i, (a, b)) in pairs.iter().enumerate() {
        assert_eq!(got[i], seq(a, b), "pair {i} ({a:?}, {b:?}) diverged");
    }
}

// ---------------------------------------------------------------------------
// levenshtein
// ---------------------------------------------------------------------------

#[test]
fn levenshtein_batch_empty_input_produces_empty_output() {
    let got = par_levenshtein_batch(&[]);
    assert!(got.is_empty());
}

#[test]
fn levenshtein_batch_a_single_item_matches_the_sequential_call() {
    let pairs = &all_pairs()[..1];
    let got = par_levenshtein_batch(pairs);
    assert_usize_parity(pairs, levenshtein, &got);
}

#[test]
fn levenshtein_batch_matches_sequential_on_pathological_and_unicode_pairs() {
    let pairs = all_pairs();
    let got = par_levenshtein_batch(&pairs);
    assert_usize_parity(&pairs, levenshtein, &got);
}

#[test]
fn levenshtein_batch_many_items_preserve_order_and_match_the_sequential_loop() {
    let pairs = many_pairs();
    let got = par_levenshtein_batch(&pairs);
    assert_usize_parity(&pairs, levenshtein, &got);
}

// ---------------------------------------------------------------------------
// damerau_levenshtein
// ---------------------------------------------------------------------------

#[test]
fn damerau_batch_empty_input_produces_empty_output() {
    let got = par_damerau_levenshtein_batch(&[]);
    assert!(got.is_empty());
}

#[test]
fn damerau_batch_a_single_item_matches_the_sequential_call() {
    let pairs = &all_pairs()[..1];
    let got = par_damerau_levenshtein_batch(pairs);
    assert_usize_parity(pairs, damerau_levenshtein, &got);
}

#[test]
fn damerau_batch_many_items_preserve_order_and_match_the_sequential_loop() {
    let pairs = many_pairs();
    let got = par_damerau_levenshtein_batch(&pairs);
    assert_usize_parity(&pairs, damerau_levenshtein, &got);
}

#[test]
fn damerau_batch_never_panics_because_there_is_no_cost_set_to_reject() {
    // This replaces `damerau_batch_rejects_inadmissible_costs_before_fanning_out`,
    // whose whole subject — a cost set below Lowrance & Wagner's threshold
    // reaching a metric — is now unconstructable: `DamerauCosts::new`
    // returns `Err` instead, and the batch takes no cost argument at all.
    assert!(DamerauCosts::new(1.0, 1.0, 1.0, 0.5).is_err());
    assert!(par_damerau_levenshtein_batch(&[]).is_empty());
    assert_eq!(par_damerau_levenshtein_batch(&[("ab", "ba")]), vec![1]);
}

#[test]
fn there_is_no_weighted_batch_variant() {
    // Deliberate: the weighted path is strictly heavier per pair, so the
    // crossover at which parallelism wins is *earlier* than the unit form's
    // and this file's guidance is conservative for it. A caller with
    // weighted costs writes the one-line `par_iter().map(...)` themselves —
    // which is what this test does, standing in for the API that is not
    // shipped.
    use rayon::prelude::*;
    use verbora_distance::{LevenshteinCosts, levenshtein_weighted};
    let costs = LevenshteinCosts::new(1.0, 3.0, 1.0).expect("admissible");
    let pairs = all_pairs();
    let got: Vec<f64> = pairs
        .par_iter()
        .map(|(a, b)| levenshtein_weighted(a, b, &costs))
        .collect();
    assert_f64_parity(&pairs, |a, b| levenshtein_weighted(a, b, &costs), &got);
}

// ---------------------------------------------------------------------------
// osa
// ---------------------------------------------------------------------------

#[test]
fn osa_batch_empty_input_produces_empty_output() {
    let got = par_osa_batch(&[]);
    assert!(got.is_empty());
}

#[test]
fn osa_batch_a_single_item_matches_the_sequential_call() {
    let pairs = &all_pairs()[..1];
    let got = par_osa_batch(pairs);
    assert_usize_parity(pairs, osa, &got);
}

#[test]
fn osa_batch_many_items_preserve_order_and_match_the_sequential_loop() {
    let pairs = many_pairs();
    let got = par_osa_batch(&pairs);
    assert_usize_parity(&pairs, osa, &got);
}

#[test]
fn damerau_and_osa_batches_genuinely_differ() {
    // The two fan-outs must not be aliases of one another: at least one
    // pinned pair has to come back with different answers, or the split into
    // two entry points is decorative.
    let pairs = [("ca", "abc")];
    assert_eq!(par_damerau_levenshtein_batch(&pairs), vec![2]);
    assert_eq!(par_osa_batch(&pairs), vec![3]);
}

// ---------------------------------------------------------------------------
// jaro_winkler
// ---------------------------------------------------------------------------

#[test]
fn jaro_winkler_batch_empty_input_produces_empty_output() {
    let got = par_jaro_winkler_batch(&[]);
    assert!(got.is_empty());
}

#[test]
fn jaro_winkler_batch_a_single_item_matches_the_sequential_call() {
    let pairs = &all_pairs()[..1];
    let got = par_jaro_winkler_batch(pairs);
    assert_f64_parity(pairs, jaro_winkler, &got);
}

#[test]
fn jaro_winkler_batch_matches_sequential_on_pathological_and_unicode_pairs() {
    let pairs = all_pairs();
    let got = par_jaro_winkler_batch(&pairs);
    assert_f64_parity(&pairs, jaro_winkler, &got);
}

#[test]
fn jaro_winkler_batch_many_items_preserve_order_and_match_the_sequential_loop() {
    let pairs = many_pairs();
    let got = par_jaro_winkler_batch(&pairs);
    assert_f64_parity(&pairs, jaro_winkler, &got);
}

#[test]
fn jaro_winkler_batch_is_case_sensitive_like_the_sequential_call() {
    // No metric in this crate rewrites its inputs, so the batch cannot
    // either: `("A", "a")` scores 0.0 per element, and the folded pair scores
    // 1.0, exactly as `jaro_winkler::tests::
    // case_is_significant_and_folding_is_the_callers` asserts by value.
    let pairs = [("A", "a"), ("X", "x"), ("AB", "ab"), ("MARTHA", "martha")];
    let got = par_jaro_winkler_batch(&pairs);
    assert_f64_parity(&pairs, jaro_winkler, &got);
    assert_eq!(got[0], 0.0);

    let folded: Vec<(String, String)> = pairs
        .iter()
        .map(|(a, b)| (a.to_lowercase(), b.to_lowercase()))
        .collect();
    let folded_refs: Vec<(&str, &str)> = folded
        .iter()
        .map(|(a, b)| (a.as_str(), b.as_str()))
        .collect();
    let got = par_jaro_winkler_batch(&folded_refs);
    assert_f64_parity(&folded_refs, jaro_winkler, &got);
    assert!(got.iter().all(|&v| v == 1.0));
}

// ---------------------------------------------------------------------------
// dice_coefficient
// ---------------------------------------------------------------------------

#[test]
fn dice_batch_empty_input_produces_empty_output() {
    let got = par_dice_coefficient_batch(&[]);
    assert!(got.is_empty());
}

#[test]
fn dice_batch_a_single_item_matches_the_sequential_call() {
    let pairs = &all_pairs()[..1];
    let got = par_dice_coefficient_batch(pairs);
    assert_f64_parity(pairs, verbora_distance::dice_coefficient, &got);
}

#[test]
fn dice_batch_matches_sequential_on_pathological_and_unicode_pairs() {
    // `all_pairs()` includes ("", ""), which used to make this the one batch
    // that produced a `NaN`. It is now `1.0` — two empty strings are
    // identical, not disjoint (`dice::tests::degenerate_pairs_are_total`) —
    // so every element here is finite and parity is bit-exact.
    let pairs = all_pairs();
    let got = par_dice_coefficient_batch(&pairs);
    assert_f64_parity(&pairs, verbora_distance::dice_coefficient, &got);
    assert!(got.iter().all(|v| v.is_finite()));
}

#[test]
fn dice_batch_many_items_preserve_order_and_match_the_sequential_loop() {
    let pairs = many_pairs();
    let got = par_dice_coefficient_batch(&pairs);
    assert_f64_parity(&pairs, verbora_distance::dice_coefficient, &got);
}

#[test]
fn dice_batch_is_case_sensitive_like_the_sequential_call() {
    // No metric in this crate rewrites its inputs, so the batch cannot
    // either: each of these pairs has disjoint bigram sets and scores 0.0,
    // and the folded pair scores 1.0, exactly as
    // `dice::tests::operands_are_not_rewritten` asserts by value.
    let pairs = [("AB", "ab"), ("ABC", "abc"), ("MARTHA", "martha")];
    let got = par_dice_coefficient_batch(&pairs);
    assert_f64_parity(&pairs, verbora_distance::dice_coefficient, &got);
    assert!(got.iter().all(|&v| v == 0.0));

    let folded: Vec<(String, String)> = pairs
        .iter()
        .map(|(a, b)| (a.to_lowercase(), b.to_lowercase()))
        .collect();
    let folded_refs: Vec<(&str, &str)> = folded
        .iter()
        .map(|(a, b)| (a.as_str(), b.as_str()))
        .collect();
    let got = par_dice_coefficient_batch(&folded_refs);
    assert_f64_parity(&folded_refs, verbora_distance::dice_coefficient, &got);
    assert!(got.iter().all(|&v| v == 1.0));
}

// ---------------------------------------------------------------------------
// hamming
// ---------------------------------------------------------------------------

#[test]
fn hamming_batch_empty_input_produces_empty_output() {
    let got = par_hamming_batch(&[]);
    assert!(got.is_empty());
}

#[test]
fn hamming_batch_a_single_item_matches_the_sequential_call() {
    let pairs = &all_pairs()[..1];
    let got = par_hamming_batch(pairs);
    assert_option_parity(pairs, hamming, &got);
}

#[test]
fn hamming_batch_matches_sequential_on_pathological_and_unicode_pairs_including_mismatches() {
    // `all_pairs()` includes scalar-count mismatches (`hamming::tests::
    // length_is_measured_in_scalars`), which report as `None` on both sides.
    let pairs = all_pairs();
    let got = par_hamming_batch(&pairs);
    assert_option_parity(&pairs, hamming, &got);
}

#[test]
fn hamming_batch_many_items_preserve_order_and_match_the_sequential_loop() {
    let pairs = many_pairs();
    let got = par_hamming_batch(&pairs);
    assert_option_parity(&pairs, hamming, &got);
}

#[test]
fn hamming_batch_reports_case_sensitively() {
    // No metric folds case, so a batch cannot either: the parallel fan-out is
    // the sequential function and nothing more.
    let pairs = [("ABC", "abc"), ("karolin", "KATHRIN")];
    let got = par_hamming_batch(&pairs);
    assert_eq!(got, vec![Some(3), Some(7)]);
    assert_option_parity(&pairs, hamming, &got);
}
