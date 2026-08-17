//! Hamming distance, ported from the reference `hamming_distance`.
//!
//! The number of positions at which two equal-length strings differ.

use crate::units::{Operands, dispatch, utf16_len};

/// Sentinel returned when the inputs cannot be compared.
///
/// The reference returns `-1` for length-mismatched input rather than raising, so
/// the sentinel is part of the contract. [`hamming_checked`] offers the same
/// computation with an `Option` if you would rather not thread a magic number
/// through your own code.
pub const INCOMPARABLE: i64 = -1;

/// Hamming distance between two strings, or [`INCOMPARABLE`] if their lengths
/// differ.
///
/// Lengths are compared in UTF-16 code units, matching the reference's
/// `String#length`.
pub fn hamming(s1: &str, s2: &str, ignore_case: bool) -> i64 {
    // The length check runs on the *original* strings, before any case folding —
    // and case folding can change length (`ß` uppercases to `SS`, `İ`
    // lowercases to two code units), so the order matters.
    if utf16_len(s1) != utf16_len(s2) {
        return INCOMPARABLE;
    }

    if ignore_case {
        let a = s1.to_lowercase();
        let b = s2.to_lowercase();
        return count_diffs(&a, &b);
    }
    count_diffs(s1, s2)
}

/// Hamming distance as an `Option`, `None` when the lengths differ.
pub fn hamming_checked(s1: &str, s2: &str, ignore_case: bool) -> Option<u64> {
    match hamming(s1, s2, ignore_case) {
        INCOMPARABLE => None,
        d => Some(d as u64),
    }
}

/// Counts differing positions over `s1.len()` positions.
///
/// After case folding the two strings may no longer be the same length; the
/// the reference loop is bounded by the (possibly folded) *first* string and reads
/// past the end of the second as `undefined`, which never equals a character.
/// Comparing `Option`s reproduces that.
fn count_diffs(s1: &str, s2: &str) -> i64 {
    dispatch(s1, s2, |ops| match ops {
        Operands::Bytes(a, b) => diffs_generic(a, b),
        Operands::Units(a, b) => diffs_generic(a, b),
    })
}

fn diffs_generic<T: Copy + PartialEq>(a: &[T], b: &[T]) -> i64 {
    let mut diffs = 0i64;
    for (i, x) in a.iter().enumerate() {
        if Some(x) != b.get(i) {
            diffs += 1;
        }
    }
    diffs
}

/// [`hamming`], fanned out across a `rayon` thread pool. Requires the
/// `parallel` feature.
///
/// # Why this exists
///
/// `hamming` is a pure function over two borrowed `&str`s with no shared
/// state, so scoring many independent pairs is embarrassingly parallel with
/// zero coordination cost between pairs. This function is exactly
/// `pairs.par_iter().map(|(a, b)| hamming(a, b, ignore_case)).collect()` — a
/// thin fan-out over the existing sequential primitive, not a second
/// implementation of it. The length check and code-unit comparison inside
/// `hamming` are untouched; if you need [`hamming_checked`]'s `Option` shape
/// in parallel, apply the same `par_iter().map(...)` pattern at your own call
/// site (see `site/performance/parallelism.md`).
///
/// # When to reach for it vs. the sequential loop
///
/// `hamming` is the cheapest metric in this crate (see `docs/PERFORMANCE.md`'s
/// `hamming/*` rows), while a `rayon` task costs on the order of a
/// microsecond to schedule (`site/performance/parallelism.md`) — so this is
/// the function in this crate least likely to pay off. Measured on this
/// crate's own `distance` benchmark (`cargo bench -p verbora-distance
/// --features parallel -- par_hamming`; 32-thread machine, default global
/// `rayon` pool, `ignore_case: false`), batches of 1000 pairs at each length:
///
/// | Pair length | Sequential (1000 pairs) | Parallel (1000 pairs) | Speedup |
/// |---:|--:|--:|--:|
/// | 4    | 6.65 µs | 53.5 µs | 0.12× (parallel *loses*, badly) |
/// | 16   | 10.0 µs | 64.4 µs | 0.16× (parallel *loses*) |
/// | 64   | 23.1 µs | 152 µs  | 0.15× (parallel *loses*) |
/// | 256  | 121 µs  | 70.0 µs | 1.7× |
/// | 1024 | 336 µs  | 129 µs  | 2.6× |
///
/// A plain `pairs.iter().map(|(a, b)| hamming(a, b, ignore_case)).collect()`
/// loop wins outright for anything shorter than roughly 256 characters, even
/// at a 1000-pair batch; reach for this only once pairs are that long, the
/// batch is far larger than 1000, or `ignore_case`'s extra lowercasing pass
/// makes the per-pair work heavy enough that scheduling overhead stops
/// dominating. These are one machine's numbers, not a guarantee — reproduce
/// with the command above before relying on them.
///
/// # Allocation behaviour
///
/// One `Vec<i64>` sized to `pairs.len()` for the output, plus whatever
/// `hamming` itself allocates per pair (two owned `String`s when
/// `ignore_case` is `true`, nothing otherwise). No additional buffering, no
/// locking, no per-call thread-pool construction — this uses whichever global
/// `rayon` pool is already installed (or `rayon`'s default one), so pool
/// configuration remains the caller's responsibility, not this crate's.
///
/// # Order and errors
///
/// Output order matches input order — `results[i] == hamming(pairs[i].0,
/// pairs[i].1, ignore_case)` — via `rayon`'s order-preserving `map` +
/// `collect`. `hamming` never errors; a length mismatch reports as
/// [`INCOMPARABLE`] per element, exactly as the sequential call would.
#[cfg(feature = "parallel")]
pub fn par_hamming_batch(pairs: &[(&str, &str)], ignore_case: bool) -> Vec<i64> {
    use rayon::prelude::*;
    pairs
        .par_iter()
        .map(|(a, b)| hamming(a, b, ignore_case))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_differing_positions() {
        assert_eq!(hamming("karolin", "kathrin", false), 3);
        assert_eq!(hamming("1011101", "1001001", false), 2);
        assert_eq!(hamming("abc", "abc", false), 0);
        assert_eq!(hamming("", "", false), 0);
    }

    #[test]
    fn length_mismatch_returns_the_sentinel() {
        assert_eq!(hamming("abc", "ab", false), INCOMPARABLE);
        assert_eq!(hamming_checked("abc", "ab", false), None);
    }

    #[test]
    fn ignore_case_folds_both_sides() {
        assert_eq!(hamming("ABC", "abc", false), 3);
        assert_eq!(hamming("ABC", "abc", true), 0);
    }

    #[test]
    fn length_is_measured_in_utf16_units() {
        // "a😀b" is 4 code units, "abcd" is 4: comparable in the reference terms.
        assert_ne!(hamming("a😀b", "abcd", false), INCOMPARABLE);
        // ...but "a😀b" (4) against "ab" (2) is not.
        assert_eq!(hamming("a😀b", "ab", false), INCOMPARABLE);
    }

    #[test]
    fn bmp_non_ascii_compares_per_character() {
        assert_eq!(hamming("café", "cafe", false), 1);
        assert_eq!(hamming("Москва", "Москва", false), 0);
    }
}
