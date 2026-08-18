//! Hamming distance, ported from the reference `hamming_distance`.
//!
//! The number of positions at which two equal-length strings differ.
//!
//! # Performance shape
//!
//! The reference semantics need two things per call: the UTF-16-length
//! equality check and a per-position comparison. A measured decomposition
//! (see `docs/PERFORMANCE_GAPS.md`'s hamming entry) attributed almost the
//! whole runtime to redundant work around those two things: four separate
//! `is_ascii` scans per call (two inside `utf16_len`, two inside
//! `dispatch`) dominated short inputs, and the scalar
//! `Some(x) != b.get(i)` comparison loop — whose `Option` wrapping blocks
//! autovectorization — dominated long ones (92% of the call at 1024
//! bytes).
//!
//! [`hamming`] therefore fronts the reference pipeline with a tiered
//! ASCII fast lane keyed on *byte*-length equality (for ASCII operands,
//! byte length **is** UTF-16 length, so the contract check collapses to a
//! pointer-width compare). Tier boundaries are measured crossovers, not
//! guesses: below 8 bytes a plain scalar zip wins (any setup work costs
//! more than the loop); 8–15 bytes a SWAR word kernel wins; from 16 up a
//! fused 16-lane kernel that counts differences *and* detects non-ASCII
//! in the same pass wins — fusing matters because a separate `is_ascii`
//! pre-pass re-reads both operands and measured ~10 ns extra at 1024
//! bytes. Anything the fast lane cannot prove ASCII falls back to the
//! original pipeline ([`hamming_slow`]) unchanged.

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
    // Case-sensitive fast lane, keyed on *byte*-length equality. This is
    // deliberately not the UTF-16 contract check yet: for ASCII operands the
    // two are the same thing, and every kernel below either proves both
    // operands ASCII or bails to [`hamming_slow`], which applies the real
    // contract. Unequal byte lengths with equal UTF-16 lengths (possible only
    // for non-ASCII input) skip the lane and hit the slow path's `utf16_len`
    // check directly.
    if !ignore_case && s1.len() == s2.len() {
        let a = s1.as_bytes();
        let b = s2.as_bytes();
        let n = a.len();
        if n < 16 {
            // Too short for the fused kernel to pay off; a vectorised
            // `is_ascii` over <16 bytes is one comparison's worth of work.
            if s1.is_ascii() && s2.is_ascii() {
                if n < 8 {
                    return a.iter().zip(b).filter(|(x, y)| x != y).count() as i64;
                }
                return swar_diffs(a, b) as i64;
            }
        } else if let Some(d) = fused_ascii_diffs(a, b) {
            return d as i64;
        }
        return hamming_slow(s1, s2, ignore_case);
    }

    // `ignore_case`, or differing byte lengths. ASCII operands still avoid
    // the slow path's two `String` allocations: ASCII is closed under
    // Unicode lowercasing (`to_lowercase` maps each ASCII byte exactly as
    // `to_ascii_lowercase` does, never changing length), so an allocation-free
    // per-byte folded compare is byte-identical to fold-then-count.
    if s1.is_ascii() && s2.is_ascii() {
        if s1.len() != s2.len() {
            return INCOMPARABLE;
        }
        return s1
            .as_bytes()
            .iter()
            .zip(s2.as_bytes())
            .filter(|(x, y)| !x.eq_ignore_ascii_case(y))
            .count() as i64;
    }
    hamming_slow(s1, s2, ignore_case)
}

/// The original reference pipeline, kept verbatim as the non-ASCII (and
/// length-changing case-fold) path — and as the oracle the fast lane is
/// differentially tested against.
fn hamming_slow(s1: &str, s2: &str, ignore_case: bool) -> i64 {
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

/// SWAR difference count over `u64` words for the 8–15-byte tier (correct
/// for any equal-length input; the tier bound is a measured crossover, not a
/// validity limit).
///
/// Differing bytes are detected per word — `byte != 0` ⇔ the high bit of
/// `((byte & 0x7f) + 0x7f) | byte` — and accumulated as per-byte counters
/// inside a single `u64`, summed horizontally once per block by the
/// `0x0101…` multiply. Deliberately **no** `count_ones()`: the workspace's
/// default baseline-x86-64 codegen has no POPCNT, so `count_ones` expands
/// to an expensive SWAR sequence per word, measured slower than the counter
/// scheme.
fn swar_diffs(a: &[u8], b: &[u8]) -> u64 {
    const HI: u64 = 0x8080_8080_8080_8080;
    debug_assert_eq!(a.len(), b.len());
    let words_end = a.len() / 8 * 8;
    let mut total = 0u64;
    let mut i = 0usize;
    while i < words_end {
        // ≤255 words per block, so no per-byte counter can overflow.
        let end = (i + 2040).min(words_end);
        let mut acc = 0u64;
        for (ca, cb) in a[i..end].chunks_exact(8).zip(b[i..end].chunks_exact(8)) {
            let x = u64::from_le_bytes(ca.try_into().unwrap())
                ^ u64::from_le_bytes(cb.try_into().unwrap());
            acc += ((((x & !HI).wrapping_add(!HI)) | x) & HI) >> 7;
        }
        // Horizontal sum via byte extraction, not the classic `0x0101…`
        // multiply: the multiply folds the sum into one byte, which
        // overflows once a block holds more than 255 total differences
        // (each of the eight counters can individually reach 255). Runs
        // once per 2040-byte block, so its cost is noise.
        total += acc.to_le_bytes().iter().map(|&x| u64::from(x)).sum::<u64>();
        i = end;
    }
    for (x, y) in a[words_end..].iter().zip(&b[words_end..]) {
        total += u64::from(x != y);
    }
    total
}

/// Fused difference count + ASCII detection for the ≥16-byte tier: one pass
/// that counts differing bytes in sixteen `u8` lane accumulators (the shape
/// LLVM autovectorises to `pcmpeqb`/`psubb` on one SSE2 register) while
/// OR-ing both operands into high-bit detectors. Returns `None` — meaning
/// "run [`hamming_slow`], the ASCII precondition failed" — if any byte of
/// either operand has its high bit set.
///
/// Fusing is the point, not a flourish: the byte count is only valid under
/// the UTF-16-length contract, which for ASCII operands the caller's
/// byte-length check already proved, and a separate `is_ascii` pre-pass
/// would re-read both operands (measured ~10 ns extra at 1024 bytes — a
/// third of the whole kernel). A 32-lane variant was measured too and
/// rejected: its codegen was build-unstable (13 ↔ 25 ns at 1024 for
/// identical source across builds) where this 16-lane shape held steady.
fn fused_ascii_diffs(a: &[u8], b: &[u8]) -> Option<u64> {
    debug_assert_eq!(a.len(), b.len());
    let n = a.len();
    let mut total = 0u64;
    let mut seen = 0u8;
    let mut i = 0usize;
    while i < n {
        // ≤255 chunks of 16 per block, so no u8 lane counter can overflow.
        let end = (i + 4080).min(n);
        let mut acc = [0u8; 16];
        let mut hi = [0u8; 16];
        let mut ai = a[i..end].chunks_exact(16);
        let mut bi = b[i..end].chunks_exact(16);
        for (ca, cb) in ai.by_ref().zip(bi.by_ref()) {
            for k in 0..16 {
                acc[k] += u8::from(ca[k] != cb[k]);
                hi[k] |= ca[k] | cb[k];
            }
        }
        total += acc.iter().map(|&x| u64::from(x)).sum::<u64>();
        seen |= hi.iter().fold(0u8, |m, &x| m | x);
        for (x, y) in ai.remainder().iter().zip(bi.remainder()) {
            total += u64::from(x != y);
            seen |= x | y;
        }
        i = end;
    }
    (seen & 0x80 == 0).then_some(total)
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

    // -----------------------------------------------------------------
    // Fast-lane battery: the tiered ASCII kernels vs. the retained
    // reference pipeline (`hamming_slow`) as oracle.
    // -----------------------------------------------------------------

    /// A tiny, dependency-free xorshift64 PRNG — deterministic (fixed seed,
    /// so failures reproduce) and good enough for adversarial random
    /// strings, not for anything security-sensitive.
    struct Xorshift64(u64);

    impl Xorshift64 {
        fn next_u64(&mut self) -> u64 {
            let mut x = self.0;
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            self.0 = x;
            x
        }

        fn next_range(&mut self, bound: usize) -> usize {
            (self.next_u64() % bound as u64) as usize
        }

        fn chance(&mut self, one_in: u64) -> bool {
            self.next_u64() % one_in == 0
        }
    }

    fn random_ascii(rng: &mut Xorshift64, len: usize, alphabet: usize) -> String {
        (0..len)
            .map(|_| (b'a' + rng.next_range(alphabet) as u8) as char)
            .collect()
    }

    #[test]
    fn fast_lane_agrees_with_the_slow_path_on_random_pairs() {
        // The correctness-defining differential: every tier of the fast lane
        // (scalar zip, SWAR word, fused 16-lane, ignore_case fold) must
        // return exactly what the reference pipeline returns — including
        // mixed case, non-ASCII bytes (which must bail to the slow path),
        // and unequal lengths (the INCOMPARABLE sentinel).
        let mut rng = Xorshift64(0xC0FF_EE00_5EED_0001);
        for _ in 0..6000 {
            let alphabet = [2usize, 4, 26][rng.next_range(3)];
            let l1 = rng.next_range(80);
            // Mostly equal lengths (the fast lane's territory), sometimes not.
            let l2 = if rng.chance(5) {
                rng.next_range(80)
            } else {
                l1
            };
            let mut s1 = random_ascii(&mut rng, l1, alphabet);
            let mut s2 = random_ascii(&mut rng, l2, alphabet);
            if rng.chance(3) {
                s1 = s1.to_uppercase();
            }
            if rng.chance(10) {
                s1.push('é');
            }
            if rng.chance(10) {
                s2.push('😀');
            }
            for ignore_case in [false, true] {
                assert_eq!(
                    hamming(&s1, &s2, ignore_case),
                    hamming_slow(&s1, &s2, ignore_case),
                    "fast lane diverged for {s1:?} vs {s2:?} ignore_case={ignore_case}"
                );
            }
        }
    }

    #[test]
    fn kernels_agree_with_a_scalar_count_across_block_boundaries() {
        // Both kernels are block-structured to bound their u8 counters at
        // 255 additions (2040-byte blocks of u64 words for `swar_diffs`,
        // 4080-byte blocks of 16-lane chunks for `fused_ascii_diffs`), so
        // the lengths straddle every such boundary, and the two-symbol
        // alphabet drives the per-position difference rate toward the
        // counter-overflow worst case.
        let mut rng = Xorshift64(0x5EED_0BAD_F00D_0002);
        let lengths = [
            0usize, 1, 7, 8, 9, 15, 16, 17, 31, 32, 63, 64, 65, 100, 2039, 2040, 2041, 4079, 4080,
            4081, 5000, 8159, 8160, 8161,
        ];
        for &len in &lengths {
            for _ in 0..4 {
                let a = random_ascii(&mut rng, len, 2);
                let b = random_ascii(&mut rng, len, 2);
                let (x, y) = (a.as_bytes(), b.as_bytes());
                let scalar = x.iter().zip(y).filter(|(p, q)| p != q).count() as u64;
                assert_eq!(swar_diffs(x, y), scalar, "swar_diffs at len {len}");
                assert_eq!(
                    fused_ascii_diffs(x, y),
                    Some(scalar),
                    "fused_ascii_diffs at len {len}"
                );
            }
        }
        // All-different pair: the absolute counter worst case.
        let a = "a".repeat(8161);
        let b = "b".repeat(8161);
        assert_eq!(swar_diffs(a.as_bytes(), b.as_bytes()), 8161);
        assert_eq!(fused_ascii_diffs(a.as_bytes(), b.as_bytes()), Some(8161));
    }

    #[test]
    fn fused_kernel_detects_non_ascii_at_every_position_class() {
        // A high bit must be caught wherever it lands — in the 16-lane body,
        // in the sub-16 remainder, in either operand — because a missed one
        // would silently apply byte-length semantics to a non-ASCII string.
        let clean = vec![b'x'; 50];
        for pos in [0usize, 15, 16, 31, 47, 48, 49] {
            for flip_first in [true, false] {
                let mut dirty = clean.clone();
                dirty[pos] = 0xC3;
                let (a, b) = if flip_first {
                    (&dirty[..], &clean[..])
                } else {
                    (&clean[..], &dirty[..])
                };
                assert_eq!(
                    fused_ascii_diffs(a, b),
                    None,
                    "missed high bit at {pos} (flip_first={flip_first})"
                );
            }
        }
        assert_eq!(fused_ascii_diffs(&clean, &clean), Some(0));
    }

    #[test]
    fn equal_byte_length_non_ascii_takes_the_contract_path() {
        // Equal *byte* lengths but unequal UTF-16 lengths: the fast lane's
        // byte-length gate matches, the fused kernel refuses, and the slow
        // path's utf16_len check must still return the sentinel.
        let s1 = "ééééééééé"; // 18 bytes, 9 UTF-16 units
        let s2 = "abcdefghijklmnopqr"; // 18 bytes, 18 units
        assert_eq!(s1.len(), s2.len());
        assert_eq!(hamming(s1, s2, false), INCOMPARABLE);
        // Equal on both counts: per-character comparison through the slow path.
        let s3 = "ééééééééé";
        let s4 = "ééééééééà";
        assert_eq!(s3.len(), s4.len());
        assert_eq!(hamming(s3, s4, false), 1);
        // Short non-ASCII (< 16 bytes) exercises the small-tier bail-out.
        assert_eq!(hamming("éé", "ab", false), hamming_slow("éé", "ab", false));
    }

    #[test]
    fn ignore_case_ascii_path_is_allocation_free_fold_equivalent() {
        // The ASCII ignore_case lane folds per byte instead of allocating
        // two lowercased Strings; the results must be identical, including
        // at case-boundary bytes adjacent to letters ('@' = 'A' - 1,
        // '[' = 'Z' + 1, '`' = 'a' - 1, '{' = 'z' + 1).
        let tricky = ["@AZ[`az{", "ABCdefGH", "zzzZZZzz", "@[`{@[`{"];
        for a in tricky {
            for b in tricky {
                assert_eq!(
                    hamming(a, b, true),
                    hamming_slow(a, b, true),
                    "{a:?} vs {b:?}"
                );
            }
        }
    }
}
