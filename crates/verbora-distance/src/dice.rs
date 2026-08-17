//! Sørensen–Dice coefficient, ported from
//! The reference `dice_coefficient`.
//!
//! Compares the *sets* of adjacent character pairs (bigrams) in two strings.
//! Because the bigrams form a set, repeated pairs are counted once — `"aaaa"`
//! and `"aa"` both reduce to the single bigram `"aa"` and score 1.0.

use rustc_hash::FxHashSet;
use verbora_core::whitespace::is_whitespace;

/// Sørensen–Dice coefficient between two strings, in `[0, 1]`.
///
/// # Empty input
///
/// Two empty strings produce no bigrams at all, so the reference computes
/// `0 / 0` and returns `NaN`. That is reproduced rather than smoothed to 0.0 or
/// 1.0: callers comparing against the reference would otherwise see a silent
/// disagreement, and `NaN` is genuinely the reference's answer.
pub fn dice_coefficient(s1: &str, s2: &str) -> f64 {
    let a = sanitize(s1);
    let b = sanitize(s2);

    let bigrams_a = bigrams(&a);
    let bigrams_b = bigrams(&b);

    // Intersect the smaller set against the larger for fewer probes.
    let (small, large) = if bigrams_a.len() <= bigrams_b.len() {
        (&bigrams_a, &bigrams_b)
    } else {
        (&bigrams_b, &bigrams_a)
    };
    let shared = small.iter().filter(|g| large.contains(*g)).count();

    (2 * shared) as f64 / (bigrams_a.len() + bigrams_b.len()) as f64
}

/// Lower-cases, collapses whitespace runs to a single space, and trims.
///
/// Mirrors the reference `sanitize`, which uses `\s` — see
/// [`verbora_core::whitespace::is_whitespace`] for why Rust's own whitespace
/// predicate is not interchangeable here.
fn sanitize(s: &str) -> Vec<u16> {
    let lowered = s.to_lowercase();

    let mut out: Vec<u16> = Vec::with_capacity(lowered.len());
    let mut pending_space = false;
    for c in lowered.chars() {
        if is_whitespace(c) {
            pending_space = true;
            continue;
        }
        if pending_space && !out.is_empty() {
            out.push(u16::from(b' '));
        }
        pending_space = false;
        let mut buf = [0u16; 2];
        out.extend_from_slice(c.encode_utf16(&mut buf));
    }
    out
}

/// The set of adjacent code-unit pairs.
///
/// Bigrams are taken over UTF-16 code units, matching `str.slice(i, i + 2)`. A
/// one-unit string is padded with a trailing space so that it yields one bigram
/// instead of none.
fn bigrams(units: &[u16]) -> FxHashSet<(u16, u16)> {
    // A pair of code units is 4 bytes and `Copy`; keying the set on the tuple
    // avoids allocating a `String` per bigram the way the reference does.
    let mut set = FxHashSet::default();

    if units.len() == 1 {
        set.insert((units[0], u16::from(b' ')));
        return set;
    }
    if units.is_empty() {
        return set;
    }

    set.reserve(units.len() - 1);
    for w in units.windows(2) {
        set.insert((w[0], w[1]));
    }
    set
}

/// [`dice_coefficient`], fanned out across a `rayon` thread pool. Requires
/// the `parallel` feature.
///
/// # Why this exists
///
/// `dice_coefficient` is a pure function over two borrowed `&str`s with no
/// shared state, so scoring many independent pairs is embarrassingly
/// parallel with zero coordination cost between pairs. This function is
/// exactly `pairs.par_iter().map(|(a, b)| dice_coefficient(a, b)).collect()`
/// — a thin fan-out over the existing sequential primitive, not a second
/// implementation of it. The sanitize/bigram/intersect pipeline inside
/// `dice_coefficient` is untouched.
///
/// # When to reach for it vs. the sequential loop
///
/// `dice_coefficient` is dominated by hashing two bigram sets (see
/// `docs/PERFORMANCE.md`'s `dice/*` rows), and a `rayon` task costs on the
/// order of a microsecond to schedule (`site/performance/parallelism.md`).
/// Measured on this crate's own `distance` benchmark (`cargo bench -p
/// verbora-distance --features parallel -- par_dice`; 32-thread machine,
/// default global `rayon` pool), batches of 1000 pairs at each length:
///
/// | Pair length | Sequential (1000 pairs) | Parallel (1000 pairs) | Speedup |
/// |---:|--:|--:|--:|
/// | 4    | 105 µs   | 52.6 µs  | 2.0× |
/// | 16   | 312 µs   | 69.7 µs  | 4.5× |
/// | 64   | 1.10 ms  | 107 µs   | 10.3× |
/// | 256  | 3.73 ms  | 493 µs   | 7.6× |
/// | 1024 | 11.4 ms  | 1.22 ms  | 9.4× |
///
/// Unlike `levenshtein` and `hamming`, `dice_coefficient` wins even at short
/// pairs in a large-enough batch — the per-pair hashing work is heavier than
/// either of those, so 1000 pairs is already past the crossover at every
/// length tested. For a plain
/// `pairs.iter().map(|(a, b)| dice_coefficient(a, b)).collect()` loop to win,
/// the batch would need to be much smaller than that. These are one
/// machine's numbers, not a guarantee — reproduce with the command above
/// before relying on them.
///
/// # Allocation behaviour
///
/// One `Vec<f64>` sized to `pairs.len()` for the output, plus whatever
/// `dice_coefficient` itself allocates per pair (a sanitized `Vec<u16>` and an
/// `FxHashSet` of bigrams for each of the two strings). No additional
/// buffering, no locking, no per-call thread-pool construction — this uses
/// whichever global `rayon` pool is already installed (or `rayon`'s default
/// one), so pool configuration remains the caller's responsibility, not this
/// crate's.
///
/// # Order and errors
///
/// Output order matches input order — `results[i] ==
/// dice_coefficient(pairs[i].0, pairs[i].1)` — via `rayon`'s order-preserving
/// `map` + `collect`. `dice_coefficient` never errors, so every element is a
/// plain `f64` (possibly `NaN`, exactly as the sequential call would produce
/// for a pair of empty strings).
#[cfg(feature = "parallel")]
pub fn par_dice_coefficient_batch(pairs: &[(&str, &str)]) -> Vec<f64> {
    use rayon::prelude::*;
    pairs
        .par_iter()
        .map(|(a, b)| dice_coefficient(a, b))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_strings_score_one() {
        assert_eq!(dice_coefficient("abc", "abc"), 1.0);
        assert_eq!(dice_coefficient("a", "a"), 1.0);
    }

    #[test]
    fn empty_pair_is_nan() {
        assert!(dice_coefficient("", "").is_nan());
    }

    #[test]
    fn empty_against_nonempty_is_zero() {
        assert_eq!(dice_coefficient("", "abc"), 0.0);
        assert_eq!(dice_coefficient("abc", ""), 0.0);
    }

    #[test]
    fn bigrams_are_a_set_so_repeats_collapse() {
        // Both reduce to the single bigram "aa".
        assert_eq!(dice_coefficient("aaaa", "aa"), 1.0);
    }

    #[test]
    fn sanitize_folds_case_and_collapses_space() {
        assert_eq!(dice_coefficient("Hello  World", "hello world"), 1.0);
        assert_eq!(dice_coefficient("  padded  ", "padded"), 1.0);
    }

    #[test]
    fn partial_overlap_is_between_zero_and_one() {
        let d = dice_coefficient("night", "nacht");
        assert!(d > 0.0 && d < 1.0, "got {d}");
    }

    #[test]
    fn single_char_is_padded_not_dropped() {
        // "a" -> {"a "}, "b" -> {"b "} : no shared bigram.
        assert_eq!(dice_coefficient("a", "b"), 0.0);
        assert_eq!(bigrams(&[u16::from(b'a')]).len(), 1);
    }

    #[test]
    fn astral_characters_use_code_unit_pairs() {
        // "😀" is two code units, so it yields exactly one bigram (the pair).
        let units: Vec<u16> = "😀".encode_utf16().collect();
        assert_eq!(units.len(), 2);
        assert_eq!(bigrams(&units).len(), 1);
        assert_eq!(dice_coefficient("😀", "😀"), 1.0);
    }
}
