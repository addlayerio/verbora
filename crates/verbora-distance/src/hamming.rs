//! Hamming distance: the tiered ASCII lane and the fused scalar walk behind
//! it.
//!
//! **Internal notes.** The metric's domain, its `Option` return, the unit it
//! counts in and its case behaviour are published on [`hamming`]. What follows
//! is why the kernels are tiered the way they are, and what about them has not
//! been measured. `docs/design/distance-contract.md` §3.3 and §4.9 record the
//! `Option`-versus-`Result` decision in full.
//!
//! # Performance shape
//!
//! The contract needs two things per call: the scalar-count equality check
//! and a per-position comparison. A measured decomposition (see
//! `docs/PERFORMANCE_GAPS.md`'s hamming entry) attributed almost the whole
//! runtime to redundant work around those two things: four separate
//! `is_ascii` scans per call (two inside the length check, two inside the
//! unit dispatch) dominated short inputs, and a scalar `Some(x) != b.get(i)`
//! comparison loop — whose `Option` wrapping blocks autovectorization —
//! dominated long ones (92% of the call at 1024 bytes).
//!
//! [`hamming`] therefore fronts the general path with a tiered ASCII fast
//! lane keyed on *byte*-length equality (for ASCII operands, byte length
//! **is** scalar count, so the contract check collapses to a pointer-width
//! compare). Tier boundaries are measured crossovers, not guesses: below 8
//! bytes a plain scalar zip wins (any setup work costs more than the loop);
//! 8–15 bytes a SWAR word kernel wins; from 16 up a fused 16-lane kernel that
//! counts differences *and* detects non-ASCII in the same pass wins — fusing
//! matters because a separate `is_ascii` pre-pass re-reads both operands and
//! measured ~10 ns extra at 1024 bytes. Anything the fast lane cannot prove
//! ASCII falls through to `hamming_slow`, one fused `chars()` walk that
//! decides comparability and counts differences together.
//!
//! [`hamming`] **never allocates**, for any input.
//!
//! `UNMEASURED` (`docs/design/distance-contract.md` §7, item 3): returning
//! `Option<usize>` rather than a bare integer returns in two registers, which
//! costs the SWAR tier the tail call the compiler previously emitted. The
//! designated mitigation is `#[inline]` on [`hamming`], which should fold the
//! discriminant into the caller's `match` under the workspace's thin LTO. It
//! is deliberately **not** applied here: it would be an unmeasured guess on
//! the crate's cheapest metric, and it belongs in the next measured batch.
//!
//! # The remaining gap to `triple_accel` is a structural floor
//!
//! `docs/PERFORMANCE_GAPS.md` entry 27(b) records Hamming as the widest
//! competitive margin anywhere in this crate: `triple_accel` 0.4.0's
//! `hamming()` dispatches to genuine AVX2/SSE4.1 intrinsics, and Hamming
//! distance is the most SIMD-friendly problem in the whole distance family
//! — a vectorised XOR-and-popcount over the entire string with no
//! data-dependent branching and nothing serial to get in the way. The
//! kernels above narrow that to a small constant factor by getting as
//! close to the same shape as safe Rust allows (a 16-lane fused
//! difference-count/ASCII-check the compiler autovectorises), but they
//! cannot close it: matching hand-written intrinsics means `unsafe`, which
//! this workspace's `unsafe_code = "deny"` policy rules out. The residual
//! is therefore recorded as a floor, not as unfinished tuning — no
//! algorithmic reduction is available (unlike the Levenshtein family's
//! common-affix trimming, Hamming already reads every position exactly
//! once by definition, so there is no work left to *remove*, only work to
//! do more lanes at a time).

/// Hamming distance between two strings, or `None` when their scalar counts
/// differ.
///
/// The number of positions at which `s1` and `s2` hold different Unicode
/// scalar values (Hamming, R. W., 1950, *Error detecting and error correcting
/// codes*, Bell System Technical Journal 29(2), §2).
///
/// The result is **symmetric** — `hamming(a, b) == hamming(b, a)` for every
/// input, as the definition requires — and bounded: `Some(d)` implies
/// `d <= s1.chars().count()`. Nothing is folded, trimmed or normalised, so
/// case is significant.
///
/// # The domain, and why the answer is an `Option`
///
/// Hamming distance is not defined for sequences of unequal length, so this
/// function returns `Option<usize>`: `Some(d)` when the operands have the same
/// number of Unicode scalar values, `None` when they do not. No magic value is
/// carved out of the numeric range to mean "no answer" — a `-1` sentinel sorts
/// *below* every real distance, so a generic "pick the closest" loop silently
/// prefers length-mismatched candidates. It is `Option` rather than `Result`
/// because a length mismatch is not a fault: it is the ordinary answer when
/// screening a candidate list, and the `filter_map` shape is the one callers
/// want.
///
/// # The unit is the scalar
///
/// One Unicode scalar value is one position, so comparability is decided by
/// `chars().count()` and never by byte length: `hamming("😀", "𝕳")` is
/// `Some(1)` — two one-scalar strings differing in one position — while
/// `hamming("a😀b", "abcd")` is `None`, three scalars against four.
///
/// # No case folding
///
/// No metric in this crate rewrites its inputs, so case is significant:
/// `hamming("ABC", "abc")` is `Some(3)`. Caseless matching is the caller's,
/// folded once at ingestion rather than re-folded against every candidate.
/// That is also the only shape in which the operation stays honest —
/// `str::to_lowercase` is lowercasing, not UAX #21 case folding, and it is
/// context-sensitive (Greek Final_Sigma) and length-changing (U+0130), both of
/// which a position-wise metric cannot absorb.
///
/// # Examples
///
/// ```
/// use verbora_distance::hamming;
///
/// assert_eq!(hamming("karolin", "kathrin"), Some(3));
/// assert_eq!(hamming("😀", "𝕳"), Some(1)); // one scalar each
/// assert_eq!(hamming("a😀b", "abc"), Some(2)); // three scalars each
/// assert_eq!(hamming("a😀b", "abcd"), None); // three scalars vs. four
/// assert_eq!(hamming("", ""), Some(0));
/// assert_eq!(hamming("ABC", "abc"), Some(3)); // no folding
/// ```
///
/// Screening a candidate list is the shape `Option` is for — an incomparable
/// candidate drops out instead of scoring better than a perfect match:
///
/// ```
/// use verbora_distance::hamming;
///
/// let best = ["kitten", "mitten", "sitting"]
///     .into_iter()
///     .filter_map(|c| hamming("kitten", c).map(|d| (c, d)))
///     .min_by_key(|&(_, d)| d);
/// assert_eq!(best, Some(("kitten", 0)));
/// ```
///
/// Caseless matching belongs to the caller, folded once at ingestion:
///
/// ```
/// use verbora_distance::hamming;
///
/// let (a, b) = ("KAROLIN", "karolin");
/// assert_eq!(hamming(a, b), Some(7));
/// assert_eq!(hamming(&a.to_lowercase(), &b.to_lowercase()), Some(0));
/// ```
///
/// # Allocation
///
/// None, on any input.
#[must_use]
pub fn hamming(s1: &str, s2: &str) -> Option<usize> {
    // Fast lane, keyed on *byte*-length equality. This is deliberately not
    // the scalar-count contract check yet: for ASCII operands the two are the
    // same thing, and every kernel below either proves both operands ASCII or
    // falls through to `hamming_slow`, which applies the real contract.
    // Unequal byte lengths with equal scalar counts (possible only for
    // non-ASCII input) skip the lane and hit the fused `chars()` walk
    // directly.
    if s1.len() == s2.len() {
        let a = s1.as_bytes();
        let b = s2.as_bytes();
        let n = a.len();
        if n < 16 {
            // Too short for the fused kernel to pay off; a vectorised
            // `is_ascii` over <16 bytes is one comparison's worth of work.
            if s1.is_ascii() && s2.is_ascii() {
                if n < 8 {
                    return Some(a.iter().zip(b).filter(|(x, y)| x != y).count());
                }
                return Some(swar_diffs(a, b) as usize);
            }
        } else if let Some(d) = try_fused_ascii_diffs(a, b) {
            return Some(d as usize);
        }
    }
    hamming_slow(s1, s2)
}

/// The general path: everything the ASCII fast lane could not prove ASCII,
/// plus every pair whose byte lengths differ.
///
/// One fused `chars()` walk that decides comparability and counts differences
/// together. Walking both operands in lockstep discovers a length mismatch
/// exactly when one iterator ends before the other, so no separate counting
/// pass is needed and no intermediate sequence is materialised — which is
/// what makes [`hamming`] allocation-free on non-ASCII input too.
///
/// A length mismatch returns `None` rather than charging the surplus
/// positions as differences: counting them would make the result depend on
/// argument order (the loop runs over whichever operand it was given first)
/// and would not be Hamming distance.
fn hamming_slow(s1: &str, s2: &str) -> Option<usize> {
    let mut a = s1.chars();
    let mut b = s2.chars();
    let mut diffs = 0usize;
    loop {
        match (a.next(), b.next()) {
            (Some(x), Some(y)) => diffs += usize::from(x != y),
            (None, None) => return Some(diffs),
            _ => return None,
        }
    }
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
        // Both `unwrap`s below are `chunks_exact`'s own contract, not an
        // assumption about the block arithmetic: it yields *only*
        // exactly-8-byte slices and routes any short tail to `remainder()`,
        // which this loop never calls. `TryInto<[u8; 8]>` has no other failure
        // mode, so neither conversion can fail even if `end - i` were not a
        // multiple of 8. The `b[i..end]` slicing is the line's real
        // precondition, and it holds because `swar_diffs`' only caller is
        // inside an `s1.len() == s2.len()` branch — asserted here by the
        // `debug_assert_eq!` above, which a second caller must keep true.
        for (ca, cb) in a[i..end].chunks_exact(8).zip(b[i..end].chunks_exact(8)) {
            // `chunks_exact(8)` yields slices of exactly 8 bytes and drops any
            // shorter remainder, so both conversions to `[u8; 8]` are
            // infallible by construction. Change the chunk width and these
            // become fallible together.
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
/// OR-ing both operands into high-bit detectors.
///
/// Returns `None` when any byte of either operand has its high bit set,
/// meaning "the ASCII precondition failed, run [`hamming_slow`]". That is a
/// *retry* signal about this kernel's applicability and has nothing to do
/// with [`hamming`]'s `None`, which is the metric's own "these operands are
/// incomparable" — hence the `try_` prefix.
///
/// Fusing is the point, not a flourish: the byte count is only valid under
/// the scalar-count contract, which for ASCII operands the caller's
/// byte-length check already proved, and a separate `is_ascii` pre-pass
/// would re-read both operands (measured ~10 ns extra at 1024 bytes — a
/// third of the whole kernel). A 32-lane variant was measured too and
/// rejected: its codegen was build-unstable (13 ↔ 25 ns at 1024 for
/// identical source across builds) where this 16-lane shape held steady.
fn try_fused_ascii_diffs(a: &[u8], b: &[u8]) -> Option<u64> {
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

/// [`hamming`], fanned out across a `rayon` thread pool. Requires the
/// `parallel` feature.
///
/// # Why this exists
///
/// `hamming` is a pure function over two borrowed `&str`s with no shared
/// state, so scoring many independent pairs is embarrassingly parallel with
/// zero coordination cost between pairs. This function is exactly
/// `pairs.par_iter().map(|(a, b)| hamming(a, b)).collect()` — a thin fan-out
/// over the existing sequential primitive, not a second implementation of
/// it. The length check and the comparison kernels inside `hamming` are
/// untouched.
///
/// # When to reach for it vs. the sequential loop
///
/// `hamming` is the cheapest metric in this crate, while a `rayon` task costs
/// on the order of a microsecond to schedule
/// (`site/performance/parallelism.md`) — so this is the function in this
/// crate least likely to pay off. A plain
/// `pairs.iter().map(|(a, b)| hamming(a, b)).collect()` loop is the right
/// default; reach for this only once the pairs are long enough that the
/// per-pair work dominates scheduling, and confirm it on your own data.
///
/// `UNMEASURED`: the crossover table this documentation used to carry was
/// measured against a different signature (an `i64` return and an
/// `ignore_case` parameter that no longer exists), so it is removed rather
/// than adjusted. It is restored only from a fresh full-precision run of
/// `cargo bench -p verbora-distance --features parallel -- par_hamming`
/// (`docs/design/distance-contract.md` §7, item 8).
///
/// # Allocation behaviour
///
/// One `Vec<Option<usize>>` sized to `pairs.len()` for the output, and
/// nothing else: `hamming` itself allocates nothing on any input. No
/// additional buffering, no locking, no per-call thread-pool construction —
/// this uses whichever global `rayon` pool is already installed (or `rayon`'s
/// default one), so pool configuration remains the caller's responsibility,
/// not this crate's.
///
/// # Order and errors
///
/// Output order matches input order — `results[i] == hamming(pairs[i].0,
/// pairs[i].1)` — via `rayon`'s order-preserving `map` + `collect`. `hamming`
/// never errors; a scalar-count mismatch reports as `None` per element,
/// exactly as the sequential call would.
#[cfg(feature = "parallel")]
#[cfg_attr(docsrs, doc(cfg(feature = "parallel")))]
#[must_use]
pub fn par_hamming_batch(pairs: &[(&str, &str)]) -> Vec<Option<usize>> {
    use rayon::prelude::*;
    pairs.par_iter().map(|(a, b)| hamming(a, b)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::levenshtein::levenshtein;

    /// The definition applied literally: materialise both scalar sequences,
    /// require equal lengths, count positions that differ. Deliberately
    /// allocating where [`hamming`] and [`hamming_slow`] are not, so that a
    /// bug in the fused lockstep walk cannot hide inside the oracle too.
    fn naive_hamming(a: &str, b: &str) -> Option<usize> {
        let x: Vec<char> = a.chars().collect();
        let y: Vec<char> = b.chars().collect();
        if x.len() != y.len() {
            return None;
        }
        let mut d = 0usize;
        for (p, q) in x.iter().zip(y.iter()) {
            if p != q {
                d += 1;
            }
        }
        Some(d)
    }

    #[test]
    fn counts_differing_positions() {
        // Every value counted from `d = |{i : x_i != y_i}|` by hand, not by
        // running the code (`docs/design/distance-contract.md` §6.3).
        assert_eq!(hamming("karolin", "kathrin"), Some(3)); // positions 2, 3, 4
        assert_eq!(hamming("karolin", "kerstin"), Some(3)); // positions 1, 3, 4
        assert_eq!(hamming("1011101", "1001001"), Some(2)); // positions 2, 4
        assert_eq!(hamming("2173896", "2233796"), Some(3)); // positions 1, 2, 4
        assert_eq!(hamming("abc", "abc"), Some(0));
        assert_eq!(hamming("", ""), Some(0));
        // Every position differs: the bound `d <= n` attained.
        assert_eq!(hamming("aaaa", "bbbb"), Some(4));
    }

    #[test]
    fn length_mismatch_is_none() {
        // Not a sentinel, not an error: the distance is undefined for
        // unequal-length sequences, so the value is absent from the type.
        assert_eq!(hamming("abc", "ab"), None);
        assert_eq!(hamming("ab", "abc"), None);
        assert_eq!(hamming("", "a"), None);
        assert_eq!(hamming("a", ""), None);
    }

    #[test]
    fn case_is_significant_and_folding_is_the_callers_job() {
        // No metric in this crate rewrites its inputs.
        assert_eq!(hamming("ABC", "abc"), Some(3));
        assert_eq!(hamming("Karolin", "karolin"), Some(1));
        // The caller-side equivalence the removed parameter used to hide.
        let (a, b) = ("KAROLIN", "karolin");
        assert_eq!(hamming(&a.to_lowercase(), &b.to_lowercase()), Some(0));
        // ...and where that equivalence stops: `to_lowercase` is lowercasing,
        // not UAX #21 case folding, so "ß" and "SS" — which Default Caseless
        // Matching equates — remain one scalar against two, hence
        // incomparable. Visible to the caller now, instead of buried in a
        // parameter.
        assert_eq!(hamming(&"ß".to_lowercase(), &"SS".to_lowercase()), None);
        // U+0130 is the one code point whose lowercasing changes length; the
        // caller sees that in the operands they pass, not in the metric.
        assert_eq!(hamming("İ", "i"), Some(1)); // one scalar each, unfolded
        assert_eq!(hamming(&"İ".to_lowercase(), "i"), None); // "i" + U+0307
    }

    #[test]
    fn length_is_measured_in_scalars() {
        // `docs/design/distance-contract.md` §2.5: comparability is decided
        // by scalar count, so "a😀b" is three units, not four.
        assert_eq!(hamming("a😀b", "abc"), Some(2)); // positions 1 and 2 differ
        assert_eq!(hamming("a😀b", "abcd"), None); // 3 vs 4
        assert_eq!(hamming("a😀b", "ab"), None); // 3 vs 2
        // Two one-character strings: exactly one differing position, a value
        // the UTF-16 unit could not represent (it reported 2, which exceeds
        // the operand length the definition bounds it by).
        assert_eq!(hamming("😀", "𝕳"), Some(1));
        assert_eq!(hamming("😀", "ab"), None);
        // One scalar each, unequal byte counts: the ASCII fast lane's
        // byte-length gate must fall through rather than reject.
        assert_eq!(hamming("é", "a"), Some(1));
        assert_eq!(hamming("a", "é"), Some(1));
    }

    #[test]
    fn bmp_non_ascii_compares_per_character() {
        assert_eq!(hamming("café", "cafe"), Some(1));
        assert_eq!(hamming("Москва", "Москва"), Some(0));
        assert_eq!(hamming("北京", "南京"), Some(1));
    }

    // -----------------------------------------------------------------
    // Fast-lane battery: the tiered ASCII kernels and the fused `chars()`
    // path against an independent materialising oracle.
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

    /// ASCII alphabet, sized by the caller so the per-position difference
    /// rate can be driven toward the counter-overflow worst case.
    fn random_ascii(rng: &mut Xorshift64, len: usize, alphabet: usize) -> String {
        (0..len)
            .map(|_| (b'a' + rng.next_range(alphabet) as u8) as char)
            .collect()
    }

    /// Non-ASCII scalars at every UTF-8 width: 2 bytes (Latin-1, Cyrillic),
    /// 3 bytes (CJK), 4 bytes (astral).
    const WIDE_ALPHABET: &[char] = &['é', 'я', '漢', '😀', 'ü', '𝕳'];

    fn random_wide(rng: &mut Xorshift64, len: usize, alphabet: usize) -> String {
        (0..len)
            .map(|_| WIDE_ALPHABET[rng.next_range(alphabet)])
            .collect()
    }

    #[test]
    fn fast_lane_agrees_with_the_naive_oracle_on_random_pairs() {
        // The correctness-defining differential: every tier of the fast lane
        // (scalar zip, SWAR word, fused 16-lane) and the general `chars()`
        // walk must return exactly what a materialising, independently
        // written oracle returns — including non-ASCII bytes (which must bail
        // out of the fast lane) and unequal lengths (which must be `None`).
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
            assert_eq!(
                hamming(&s1, &s2),
                naive_hamming(&s1, &s2),
                "fast lane diverged for {s1:?} vs {s2:?}"
            );
        }
    }

    #[test]
    fn every_tier_and_block_boundary_agrees_with_the_naive_oracle() {
        // The tier crossovers (8, 16) and both kernels' counter-block
        // boundaries (2040 bytes of `u64` words for `swar_diffs`, 4080 bytes
        // of 16-lane chunks for `try_fused_ascii_diffs`, plus their doubles),
        // with both operands non-ASCII on half the lengths so the fused
        // `chars()` walk is exercised at the same scales.
        let mut rng = Xorshift64(0x5EED_0BAD_F00D_0002);
        let lengths = [
            0usize, 1, 7, 8, 9, 15, 16, 17, 31, 63, 64, 65, 2039, 2040, 2041, 4079, 4080, 4081,
            8159, 8160, 8161,
        ];
        for (i, &len) in lengths.iter().enumerate() {
            let wide = i % 2 == 1;
            for _ in 0..2 {
                let make = |rng: &mut Xorshift64, n: usize| {
                    if wide {
                        random_wide(rng, n, WIDE_ALPHABET.len())
                    } else {
                        random_ascii(rng, n, 2)
                    }
                };
                let a = make(&mut rng, len);
                let b = make(&mut rng, len);
                assert_eq!(
                    hamming(&a, &b),
                    naive_hamming(&a, &b),
                    "diverged at len {len} (wide={wide})"
                );
                // One scalar longer: comparability must fail at every scale,
                // including where the byte lengths happen to line up.
                let c = make(&mut rng, len + 1);
                assert_eq!(hamming(&a, &c), None, "len {len} vs {}", len + 1);
                assert_eq!(hamming(&c, &a), None, "len {} vs {len}", len + 1);
            }
        }
    }

    #[test]
    fn kernels_agree_with_a_scalar_count_across_block_boundaries() {
        // Both kernels are block-structured to bound their u8 counters at
        // 255 additions (2040-byte blocks of u64 words for `swar_diffs`,
        // 4080-byte blocks of 16-lane chunks for `try_fused_ascii_diffs`), so
        // the lengths straddle every such boundary, and the two-symbol
        // alphabet drives the per-position difference rate toward the
        // counter-overflow worst case.
        let mut rng = Xorshift64(0x5EED_0BAD_F00D_0003);
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
                    try_fused_ascii_diffs(x, y),
                    Some(scalar),
                    "try_fused_ascii_diffs at len {len}"
                );
            }
        }
        // All-different pair: the absolute counter worst case.
        let a = "a".repeat(8161);
        let b = "b".repeat(8161);
        assert_eq!(swar_diffs(a.as_bytes(), b.as_bytes()), 8161);
        assert_eq!(
            try_fused_ascii_diffs(a.as_bytes(), b.as_bytes()),
            Some(8161)
        );
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
                    try_fused_ascii_diffs(a, b),
                    None,
                    "missed high bit at {pos} (flip_first={flip_first})"
                );
            }
        }
        assert_eq!(try_fused_ascii_diffs(&clean, &clean), Some(0));
    }

    #[test]
    fn equal_byte_length_non_ascii_takes_the_contract_path() {
        // Equal *byte* lengths but unequal scalar counts: the fast lane's
        // byte-length gate matches, the fused kernel refuses, and the general
        // path's scalar-count check must still report `None`.
        let s1 = "ééééééééé"; // 18 bytes, 9 scalars
        let s2 = "abcdefghijklmnopqr"; // 18 bytes, 18 scalars
        assert_eq!(s1.len(), s2.len());
        assert_eq!(hamming(s1, s2), None);
        // Equal on both counts: per-character comparison through the general
        // path.
        let s3 = "ééééééééé";
        let s4 = "ééééééééà";
        assert_eq!(s3.len(), s4.len());
        assert_eq!(hamming(s3, s4), Some(1));
        // Short non-ASCII (< 16 bytes) exercises the small-tier bail-out:
        // four bytes against two, but two scalars each, so the pair is
        // comparable and both positions differ.
        assert_eq!(hamming("éé", "ab"), naive_hamming("éé", "ab"));
        assert_eq!(hamming("éé", "ab"), Some(2));
    }

    // -----------------------------------------------------------------
    // Algebraic properties (`docs/design/distance-contract.md` §6.3).
    // -----------------------------------------------------------------

    /// Operands spanning every dispatch branch and every UTF-8 width:
    /// ASCII at each fast-lane tier, Latin-1, Greek (including the
    /// context-sensitive sigmas), Cyrillic, Hebrew, Arabic, Devanagari,
    /// Thai, Hangul, CJK, and astral scalars — plus the case-mapping shapes
    /// that a folding parameter used to make asymmetric.
    const CORPUS: &[&str] = &[
        "",
        "a",
        "b",
        "ab",
        "ba",
        "abc",
        "ABC",
        "abcd",
        "abcdefgh",
        "abcdefghijklmnopqr",
        "karolin",
        "kathrin",
        "kerstin",
        "café",
        "cafe",
        "é",
        "à",
        "Σ",
        "σ",
        "ς",
        "ΑΣ",
        "Ας",
        "İ",
        "i",
        "i\u{307}",
        "ß",
        "ẞ",
        "ﬁ",
        "I",
        "ı",
        "Москва",
        "москва",
        "עברית",
        "مرحبا",
        "क्षि",
        "ไทย",
        "한국",
        "北京",
        "南京",
        "😀",
        "😁",
        "𝕳",
        "a😀b",
        "😀😁😀",
    ];

    #[test]
    fn identity_discernibility_symmetry_and_the_length_bound() {
        for &a in CORPUS {
            // Identity, and its converse: zero distance iff equal strings.
            assert_eq!(hamming(a, a), Some(0), "identity failed for {a:?}");
            for &b in CORPUS {
                let d = hamming(a, b);
                assert_eq!(d, hamming(b, a), "asymmetric for {a:?} vs {b:?}");
                assert_eq!(
                    d.is_some(),
                    a.chars().count() == b.chars().count(),
                    "comparability disagrees with scalar count for {a:?} vs {b:?}"
                );
                if let Some(d) = d {
                    assert_eq!(
                        d == 0,
                        a == b,
                        "discernibility failed for {a:?} vs {b:?} (d = {d})"
                    );
                    assert!(
                        d <= a.chars().count(),
                        "{d} exceeds the operand length for {a:?} vs {b:?}"
                    );
                    // Substitutions alone realise the Hamming distance, and
                    // Levenshtein minimises over all edit scripts.
                    assert!(
                        levenshtein(a, b) <= d,
                        "levenshtein({a:?}, {b:?}) exceeds the Hamming distance {d}"
                    );
                }
            }
        }
    }

    #[test]
    fn triangle_inequality_holds_over_comparable_triples() {
        for &a in CORPUS {
            for &b in CORPUS {
                let Some(ab) = hamming(a, b) else { continue };
                for &c in CORPUS {
                    let (Some(bc), Some(ac)) = (hamming(b, c), hamming(a, c)) else {
                        continue;
                    };
                    assert!(
                        ac <= ab + bc,
                        "triangle inequality violated: {a:?}/{b:?}/{c:?} \
                         ({ac} > {ab} + {bc})"
                    );
                }
            }
        }
    }

    #[test]
    fn hamming_is_symmetric_on_random_mixed_inputs() {
        // The property over a randomized corpus that mixes ASCII (every fast
        // -lane tier), Latin-1, astral characters and the case-mapping traps
        // above, at equal and unequal lengths. Symmetry must hold through
        // every branch, not just the ones the fixtures reach.
        let mut rng = Xorshift64(0x5EED_5111_1E7E_0004);
        for _ in 0..8000 {
            let build = |rng: &mut Xorshift64| {
                let len = rng.next_range(40);
                let alphabet = [2usize, 4, 26][rng.next_range(3)];
                let mut s = random_ascii(rng, len, alphabet);
                if rng.chance(3) {
                    s = s.to_uppercase();
                }
                for _ in 0..rng.next_range(3) {
                    s.push_str(CORPUS[rng.next_range(CORPUS.len())]);
                }
                s
            };
            let s1 = build(&mut rng);
            let s2 = build(&mut rng);
            assert_eq!(
                hamming(&s1, &s2),
                hamming(&s2, &s1),
                "asymmetric for {s1:?} vs {s2:?}"
            );
            assert_eq!(
                hamming(&s1, &s2),
                naive_hamming(&s1, &s2),
                "diverged from the oracle for {s1:?} vs {s2:?}"
            );
        }
    }
}
