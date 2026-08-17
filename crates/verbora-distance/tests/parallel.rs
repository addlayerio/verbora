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
//! restricted-vs-unrestricted Damerau divergence, astral-plane and BMP
//! Unicode, the single-character Jaro window quirk, bigram-set collapsing,
//! and UTF-16-length mismatches for Hamming.

#![cfg(feature = "parallel")]

use verbora_distance::{
    hamming, jaro_winkler,
    jaro_winkler::Options as JwOptions,
    levenshtein::{Options as LevOptions, damerau_levenshtein, levenshtein},
    par_damerau_levenshtein_batch, par_dice_coefficient_batch, par_hamming_batch,
    par_jaro_winkler_batch, par_levenshtein_batch,
};

/// ASCII pairs already exercised by this crate's own unit-test suites:
/// `levenshtein::tests::{classic_distances, transposition_only_counts_for_damerau,
/// restricted_and_unrestricted_damerau_differ, asymmetric_costs_are_respected,
/// two_row_and_full_matrix_agree}`, `jaro_winkler::tests::classic_reference_values`,
/// `dice::tests::{bigrams_are_a_set_so_repeats_collapse,
/// sanitize_folds_case_and_collapses_space, partial_overlap_is_between_zero_and_one}`,
/// `hamming::tests::counts_differing_positions`.
const PATHOLOGICAL: &[(&str, &str)] = &[
    ("kitten", "sitting"),
    ("saturday", "sunday"),
    ("", ""),
    ("abc", ""),
    ("", "abc"),
    ("same", "same"),
    ("ab", "ba"),  // transposition: 2 under Levenshtein, 1 under Damerau
    ("ca", "abc"), // restricted vs. unrestricted Damerau diverge (2 vs 3)
    ("abc", "ab"), // one deletion
    ("ab", "abc"), // one insertion
    ("flaw", "lawn"),
    ("a", "abcdef"),
    ("abcdef", "fedcba"),
    ("MARTHA", "MARHTA"),
    ("DIXON", "DICKSONX"),
    ("DWAYNE", "DUANE"),
    ("a", "b"),     // single-char Jaro match window is empty (floor(1/2)-1 = -1)
    ("aaaa", "aa"), // dice: repeated bigrams collapse to a set
    ("Hello  World", "hello world"),
    ("  padded  ", "padded"),
    ("night", "nacht"),
    ("karolin", "kathrin"),
    ("1011101", "1001001"),
    ("abc", "abc"),
];

/// Unicode pairs already exercised by this crate's own unit-test suites:
/// `levenshtein::tests::{utf16_semantics_match_the_reference,
/// bmp_non_ascii_is_one_unit_per_char}`, `dice::tests::astral_characters_use_code_unit_pairs`,
/// `hamming::tests::{length_is_measured_in_utf16_units, bmp_non_ascii_compares_per_character}`.
const UNICODE: &[(&str, &str)] = &[
    ("a😀b", "ab"), // UTF-16 length 4 vs 2 for Levenshtein
    ("😀", ""),
    ("😀", "😀"),
    ("café", "cafe"),
    ("Москва", "Москва"),
    ("a😀b", "abcd"), // both 4 UTF-16 units: comparable for Hamming
    ("a😀b", "ab"),   // 4 vs 2 units: incomparable for Hamming
];

fn all_pairs() -> Vec<(&'static str, &'static str)> {
    PATHOLOGICAL.iter().chain(UNICODE.iter()).copied().collect()
}

/// `pairs` cycled out to a batch large enough to span several `rayon` tasks.
fn many_pairs() -> Vec<(&'static str, &'static str)> {
    all_pairs().into_iter().cycle().take(4096).collect()
}

/// `a == b`, treating two `NaN`s as equal (as `dice_coefficient("", "")`
/// produces, since `0.0 / 0.0` is `NaN` and genuinely reproduced rather than
/// smoothed away — see `src/dice.rs`).
fn f64_eq(a: f64, b: f64) -> bool {
    a == b || (a.is_nan() && b.is_nan())
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

fn assert_i64_parity(pairs: &[(&str, &str)], seq: impl Fn(&str, &str) -> i64, got: &[i64]) {
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
    let opts = LevOptions::default();
    let got = par_levenshtein_batch(&[], &opts);
    assert!(got.is_empty());
}

#[test]
fn levenshtein_batch_a_single_item_matches_the_sequential_call() {
    let opts = LevOptions::default();
    let pairs = &all_pairs()[..1];
    let got = par_levenshtein_batch(pairs, &opts);
    assert_f64_parity(pairs, |a, b| levenshtein(a, b, &opts), &got);
}

#[test]
fn levenshtein_batch_matches_sequential_on_pathological_and_unicode_pairs() {
    let opts = LevOptions::default();
    let pairs = all_pairs();
    let got = par_levenshtein_batch(&pairs, &opts);
    assert_f64_parity(&pairs, |a, b| levenshtein(a, b, &opts), &got);
}

#[test]
fn levenshtein_batch_many_items_preserve_order_and_match_the_sequential_loop() {
    let opts = LevOptions::default();
    let pairs = many_pairs();
    let got = par_levenshtein_batch(&pairs, &opts);
    assert_f64_parity(&pairs, |a, b| levenshtein(a, b, &opts), &got);
}

// ---------------------------------------------------------------------------
// damerau_levenshtein (both restricted and unrestricted, since the sequential
// suite's own `restricted_and_unrestricted_damerau_differ` shows they diverge)
// ---------------------------------------------------------------------------

#[test]
fn damerau_batch_empty_input_produces_empty_output() {
    let opts = LevOptions::default();
    let got = par_damerau_levenshtein_batch(&[], &opts);
    assert!(got.is_empty());
}

#[test]
fn damerau_batch_a_single_item_matches_the_sequential_call() {
    let opts = LevOptions::default();
    let pairs = &all_pairs()[..1];
    let got = par_damerau_levenshtein_batch(pairs, &opts);
    assert_f64_parity(pairs, |a, b| damerau_levenshtein(a, b, &opts), &got);
}

#[test]
fn damerau_batch_matches_sequential_unrestricted() {
    let opts = LevOptions {
        restricted: false,
        ..LevOptions::default()
    };
    let pairs = many_pairs();
    let got = par_damerau_levenshtein_batch(&pairs, &opts);
    assert_f64_parity(&pairs, |a, b| damerau_levenshtein(a, b, &opts), &got);
}

#[test]
fn damerau_batch_matches_sequential_restricted() {
    let opts = LevOptions {
        restricted: true,
        ..LevOptions::default()
    };
    let pairs = many_pairs();
    let got = par_damerau_levenshtein_batch(&pairs, &opts);
    assert_f64_parity(&pairs, |a, b| damerau_levenshtein(a, b, &opts), &got);
}

// ---------------------------------------------------------------------------
// jaro_winkler
// ---------------------------------------------------------------------------

#[test]
fn jaro_winkler_batch_empty_input_produces_empty_output() {
    let opts = JwOptions::default();
    let got = par_jaro_winkler_batch(&[], &opts);
    assert!(got.is_empty());
}

#[test]
fn jaro_winkler_batch_a_single_item_matches_the_sequential_call() {
    let opts = JwOptions::default();
    let pairs = &all_pairs()[..1];
    let got = par_jaro_winkler_batch(pairs, &opts);
    assert_f64_parity(pairs, |a, b| jaro_winkler(a, b, &opts), &got);
}

#[test]
fn jaro_winkler_batch_matches_sequential_on_pathological_and_unicode_pairs() {
    let opts = JwOptions::default();
    let pairs = all_pairs();
    let got = par_jaro_winkler_batch(&pairs, &opts);
    assert_f64_parity(&pairs, |a, b| jaro_winkler(a, b, &opts), &got);
}

#[test]
fn jaro_winkler_batch_many_items_preserve_order_and_match_the_sequential_loop() {
    let opts = JwOptions::default();
    let pairs = many_pairs();
    let got = par_jaro_winkler_batch(&pairs, &opts);
    assert_f64_parity(&pairs, |a, b| jaro_winkler(a, b, &opts), &got);
}

#[test]
fn jaro_winkler_batch_respects_ignore_case() {
    // Exercises the single-character prefix-boost quirk `jaro_winkler::tests::
    // single_char_ignore_case_exposes_the_prefix_quirk` asserts by value.
    let opts = JwOptions {
        ignore_case: true,
        dj: None,
    };
    let pairs = [("A", "a"), ("X", "x"), ("AB", "ab"), ("MARTHA", "martha")];
    let got = par_jaro_winkler_batch(&pairs, &opts);
    assert_f64_parity(&pairs, |a, b| jaro_winkler(a, b, &opts), &got);
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
fn dice_batch_matches_sequential_on_pathological_and_unicode_pairs_including_nan() {
    // `all_pairs()` includes ("", ""), whose sequential result is NaN
    // (`dice::tests::empty_pair_is_nan`) — `f64_eq` treats NaN == NaN as
    // parity here, since that is genuinely what both sides produce.
    let pairs = all_pairs();
    let got = par_dice_coefficient_batch(&pairs);
    assert_f64_parity(&pairs, verbora_distance::dice_coefficient, &got);
}

#[test]
fn dice_batch_many_items_preserve_order_and_match_the_sequential_loop() {
    let pairs = many_pairs();
    let got = par_dice_coefficient_batch(&pairs);
    assert_f64_parity(&pairs, verbora_distance::dice_coefficient, &got);
}

// ---------------------------------------------------------------------------
// hamming
// ---------------------------------------------------------------------------

#[test]
fn hamming_batch_empty_input_produces_empty_output() {
    let got = par_hamming_batch(&[], false);
    assert!(got.is_empty());
}

#[test]
fn hamming_batch_a_single_item_matches_the_sequential_call() {
    let pairs = &all_pairs()[..1];
    let got = par_hamming_batch(pairs, false);
    assert_i64_parity(pairs, |a, b| hamming(a, b, false), &got);
}

#[test]
fn hamming_batch_matches_sequential_on_pathological_and_unicode_pairs_including_mismatches() {
    // `all_pairs()` includes UTF-16-length mismatches (`hamming::tests::
    // length_is_measured_in_utf16_units`), which report as `INCOMPARABLE` on
    // both sides.
    let pairs = all_pairs();
    let got = par_hamming_batch(&pairs, false);
    assert_i64_parity(&pairs, |a, b| hamming(a, b, false), &got);
}

#[test]
fn hamming_batch_many_items_preserve_order_and_match_the_sequential_loop() {
    let pairs = many_pairs();
    let got = par_hamming_batch(&pairs, false);
    assert_i64_parity(&pairs, |a, b| hamming(a, b, false), &got);
}

#[test]
fn hamming_batch_respects_ignore_case() {
    let pairs = [("ABC", "abc"), ("karolin", "KATHRIN")];
    let got = par_hamming_batch(&pairs, true);
    assert_i64_parity(&pairs, |a, b| hamming(a, b, true), &got);
}
