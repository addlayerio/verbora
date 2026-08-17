//! String distance and similarity metrics for Rust.
//!
//! String distance and similarity — seven public metrics across four algorithms.
//!
//! ```
//! use verbora_distance::{levenshtein, jaro_winkler, dice_coefficient, hamming};
//!
//! assert_eq!(levenshtein("kitten", "sitting", &Default::default()), 3.0);
//! assert_eq!(dice_coefficient("abc", "abc"), 1.0);
//! assert_eq!(hamming("karolin", "kathrin", false), 3);
//! assert_eq!(jaro_winkler("abc", "abc", &Default::default()), 1.0);
//! ```
//!
//! # Conventions differ between metrics
//!
//! The metrics deliberately do not share a single direction convention, and this crate does
//! not "fix" that, since doing so would change every caller's results:
//!
//! | Metric | Range | Direction |
//! |--------|-------|-----------|
//! | [`fn@levenshtein`], [`fn@damerau_levenshtein`] | `0..` | distance — lower is closer |
//! | [`fn@hamming`] | `-1`, `0..` | distance — lower is closer; `-1` means incomparable |
//! | [`fn@jaro`], [`fn@jaro_winkler`] | `0..=1` | similarity — higher is closer |
//! | [`dice_coefficient`] | `0..=1`, or `NaN` | similarity — higher is closer |
//!
//! The [`verbora_core::StringMetric`] implementations below record which
//! direction each one uses, so generic code can adapt without any metric
//! changing its output.
//!
//! # Unicode
//!
//! Every metric here indexes text by UTF-16 code unit —
//! because that is observable in the results. See [`units`] for the mechanism
//! and for the ASCII fast path that keeps it free on ordinary input.
//!
//! # Batch computation (feature = `parallel`)
//!
//! Every metric above is a pure, stateless free function, so scoring many
//! independent pairs is embarrassingly parallel with zero coordination cost.
//! With the `parallel` feature enabled, [`par_levenshtein_batch`],
//! [`par_damerau_levenshtein_batch`], [`par_jaro_winkler_batch`],
//! [`par_dice_coefficient_batch`] and [`par_hamming_batch`] fan a batch of
//! pairs out across a `rayon` thread pool. Each is exactly
//! `pairs.par_iter().map(<the sequential function>).collect()` — see the
//! individual function docs for cost trade-offs and when a plain sequential
//! loop is the better choice (usually: for small batches or short strings).

pub mod dice;
pub mod hamming;
pub mod jaro_winkler;
pub mod levenshtein;
pub mod units;

pub use dice::dice_coefficient;
#[cfg(feature = "parallel")]
pub use dice::par_dice_coefficient_batch;
#[cfg(feature = "parallel")]
pub use hamming::par_hamming_batch;
pub use hamming::{INCOMPARABLE, hamming, hamming_checked};
#[cfg(feature = "parallel")]
pub use jaro_winkler::par_jaro_winkler_batch;
pub use jaro_winkler::{jaro, jaro_winkler};
pub use levenshtein::{
    SearchResult, damerau_levenshtein, damerau_levenshtein_search, levenshtein, levenshtein_search,
};
#[cfg(feature = "parallel")]
pub use levenshtein::{par_damerau_levenshtein_batch, par_levenshtein_batch};

use verbora_core::StringMetric;

/// Levenshtein distance as a [`StringMetric`].
#[derive(Debug, Clone, Copy, Default)]
pub struct Levenshtein(pub levenshtein::Options);

impl StringMetric for Levenshtein {
    const IS_SIMILARITY: bool = false;
    fn measure(&self, a: &str, b: &str) -> f64 {
        levenshtein(a, b, &self.0)
    }
}

/// Damerau–Levenshtein distance as a [`StringMetric`].
#[derive(Debug, Clone, Copy, Default)]
pub struct DamerauLevenshtein(pub levenshtein::Options);

impl StringMetric for DamerauLevenshtein {
    const IS_SIMILARITY: bool = false;
    fn measure(&self, a: &str, b: &str) -> f64 {
        damerau_levenshtein(a, b, &self.0)
    }
}

/// Jaro–Winkler similarity as a [`StringMetric`].
#[derive(Debug, Clone, Copy, Default)]
pub struct JaroWinkler(pub jaro_winkler::Options);

impl StringMetric for JaroWinkler {
    const IS_SIMILARITY: bool = true;
    fn measure(&self, a: &str, b: &str) -> f64 {
        jaro_winkler(a, b, &self.0)
    }
}

/// Sørensen–Dice coefficient as a [`StringMetric`].
#[derive(Debug, Clone, Copy, Default)]
pub struct Dice;

impl StringMetric for Dice {
    const IS_SIMILARITY: bool = true;
    fn measure(&self, a: &str, b: &str) -> f64 {
        dice_coefficient(a, b)
    }
}

/// Hamming distance as a [`StringMetric`].
///
/// Incomparable inputs measure as `-1.0`, matching the scalar API.
#[derive(Debug, Clone, Copy, Default)]
pub struct Hamming {
    /// Fold case before comparing.
    pub ignore_case: bool,
}

impl StringMetric for Hamming {
    const IS_SIMILARITY: bool = false;
    fn measure(&self, a: &str, b: &str) -> f64 {
        hamming(a, b, self.ignore_case) as f64
    }
}
