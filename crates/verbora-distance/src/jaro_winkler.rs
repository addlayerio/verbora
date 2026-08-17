//! Jaro and Jaro–Winkler similarity, ported from
//! The reference `jaro-winkler_distance`.
//!
//! Both are *similarities*: 1.0 means identical, 0.0 means nothing in common.

use crate::units::{BitPeq, Operands, dispatch};

/// Options for [`jaro_winkler`].
///
/// Mirrors the reference `options` object.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Options {
    /// Lower-case both inputs before comparing.
    pub ignore_case: bool,
    /// A precomputed Jaro similarity, short-circuiting the Jaro step.
    ///
    /// Corresponds to `options.dj`. Supplying it skips the `O(nm)` matching pass,
    /// which is worthwhile when the caller already has the Jaro score.
    pub dj: Option<f64>,
}

/// Jaro similarity between two strings.
pub fn jaro(s1: &str, s2: &str) -> f64 {
    dispatch(s1, s2, |ops| match ops {
        Operands::Bytes(a, b) => jaro_generic(a, b),
        Operands::Units(a, b) => jaro_generic(a, b),
    })
}

/// Sizes at or below this stay on the classical scalar loop — measured to
/// already beat every bit-parallel formulation (and `rapidfuzz`) there,
/// because a 2 KB Peq table build costs more than the handful of
/// window-scan compares it would replace.
const JARO_SCALAR_MAX: usize = 16;

fn jaro_generic<T: BitPeq>(s1: &[T], s2: &[T]) -> f64 {
    let len1 = s1.len();
    let len2 = s2.len();
    if len1 == 0 || len2 == 0 {
        return 0.0;
    }

    let max_len = len1.max(len2);
    // `floor(max/2) - 1` is **signed** in the reference: for two
    // one-character strings it is -1, which makes the match window empty
    // and the similarity 0. `max_len < 2` is exactly that case (any longer
    // operand makes the window non-negative), so the fast paths can use an
    // unsigned window below it.
    let (m, t_raw) = if max_len < 2 {
        (0, 0)
    } else if max_len <= JARO_SCALAR_MAX {
        jaro_scalar(s1, s2)
    } else {
        // Positions of the longer operand past `shorter + w` can never
        // fall inside any match window (a match needs `|i - j| <= w`; the
        // trim boundary is where the clamped window empties), so the bit
        // kernels never look at them. The *original* lengths still feed
        // the final formula below — the trim changes which cells are
        // scanned, never the denominators.
        let w = max_len / 2 - 1;
        let len1t = len1.min(len2 + w);
        let len2t = len2.min(len1 + w);
        if len1t <= 64 && len2t <= 64 {
            jaro_bit_word(&s1[..len1t], &s2[..len2t], w)
        } else {
            jaro_bit_block(&s1[..len1t], &s2[..len2t], w)
        }
    };

    if m == 0 {
        return 0.0;
    }

    let t = t_raw as f64 / 2.0;
    let m = m as f64;
    // Kept as three separate divisions summed in this order to match the
    // the reference's floating-point accumulation exactly. `t` stays
    // fractional (`raw / 2.0`, observable as x.5 for odd raw counts) — the
    // one place a tempting `rapidfuzz`-style integer halving would
    // silently change results.
    ((m / len1 as f64) + (m / len2 as f64) + ((m - t) / m)) / 3.0
}

/// The classical windowed greedy loop, returning `(matches,
/// raw_transpositions)`. The definition every bit-parallel kernel below is
/// differentially pinned against — and still the production path for
/// small inputs, where it wins outright.
fn jaro_scalar<T: Copy + PartialEq>(s1: &[T], s2: &[T]) -> (usize, usize) {
    let len1 = s1.len();
    let len2 = s2.len();
    let match_window = (len1.max(len2) as isize) / 2 - 1;

    // Match flags live on the stack for short inputs. Measured: two `vec![]`
    // allocations per call made 4-character Jaro–Winkler *slower* than the
    // the reference (0.6×), because the reference engine's `new Array(4)` is nearly free while
    // `malloc` is not. Words are short by nature, so this is the common path,
    // not a micro-optimisation for a rare case.
    const STACK_CAP: usize = 128;
    let mut stack1 = [false; STACK_CAP];
    let mut stack2 = [false; STACK_CAP];
    let mut heap1;
    let mut heap2;

    let matches1: &mut [bool] = if len1 <= STACK_CAP {
        &mut stack1[..len1]
    } else {
        heap1 = vec![false; len1];
        &mut heap1
    };
    let matches2: &mut [bool] = if len2 <= STACK_CAP {
        &mut stack2[..len2]
    } else {
        heap2 = vec![false; len2];
        &mut heap2
    };

    let mut m = 0usize;

    for (i, &c1) in s1.iter().enumerate() {
        let start = (i as isize - match_window).max(0) as usize;
        let end = ((i as isize + match_window + 1).max(0) as usize).min(len2);

        for k in start..end {
            if matches2[k] || c1 != s2[k] {
                continue;
            }
            matches1[i] = true;
            matches2[k] = true;
            m += 1;
            break;
        }
    }

    if m == 0 {
        return (0, 0);
    }

    // Count transpositions by walking both match sequences in lockstep.
    let mut t = 0usize;
    let mut k = 0usize;
    for (i, &c1) in s1.iter().enumerate() {
        if !matches1[i] {
            continue;
        }
        while !matches2[k] {
            k += 1;
        }
        if c1 != s2[k] {
            t += 1;
        }
        k += 1;
    }

    (m, t)
}

/// The lowest `n` bits set, saturating at all 64.
#[inline]
fn mask_lsb(n: usize) -> u64 {
    if n >= 64 { !0u64 } else { (1u64 << n) - 1 }
}

/// Bit-parallel Jaro matching for trimmed operands that both fit one
/// 64-bit word, in *Verbora's own orientation*: Peq is built over `s2`,
/// the loop walks `s1`, and each step takes the **lowest** available `s2`
/// bit inside the window — making every greedy choice structurally
/// identical to [`jaro_scalar`]'s inner `break`, so parity is by
/// construction, not argument (and pinned exhaustively by this module's
/// tests regardless). The window mask reproduces `|i - j| <= w`
/// incrementally: full-width `w + 1` bits at `i = 0`, growing by one bit
/// per step while `i < w`, then sliding.
///
/// Transpositions walk the two match masks' set bits in lockstep with a
/// direct slice compare per pair — no Peq lookup, no iterator `nth` (the
/// two overheads `rapidfuzz`'s equivalent pays per matched character).
fn jaro_bit_word<T: BitPeq>(s1t: &[T], s2t: &[T], w: usize) -> (usize, usize) {
    debug_assert!(s1t.len() <= 64 && s2t.len() <= 64);
    let peq = T::peq1(s2t);

    let mut matched1: u64 = 0;
    let mut matched2: u64 = 0;
    let mut window = mask_lsb(w + 1);
    for (i, &c1) in s1t.iter().enumerate() {
        let avail = T::peq1_get(&peq, c1) & window & !matched2;
        let lowest = avail & avail.wrapping_neg();
        matched2 |= lowest;
        matched1 |= u64::from(avail != 0) << i;
        if i < w {
            window = (window << 1) | 1;
        } else {
            window <<= 1;
        }
    }

    let m = matched2.count_ones() as usize;
    if m == 0 {
        return (0, 0);
    }

    let mut t = 0usize;
    let mut f1 = matched1;
    let mut f2 = matched2;
    while f1 != 0 {
        let i = f1.trailing_zeros() as usize;
        let j = f2.trailing_zeros() as usize;
        t += usize::from(s1t[i] != s2t[j]);
        f1 &= f1 - 1;
        f2 &= f2 - 1;
    }
    (m, t)
}

/// [`jaro_bit_word`] generalised across multiple 64-bit words. The window
/// `[i - w, i + w + 1)` is computed directly per step — first and last
/// words partially masked, interior words whole — and the scan breaks at
/// the first word holding an available match, exactly the scalar loop's
/// leftmost-first choice. (This replaces `rapidfuzz`'s incremental
/// `SearchBoundMask` state machine with a handful of ALU ops per step —
/// simpler to verify, same cost.)
fn jaro_bit_block<T: BitPeq>(s1t: &[T], s2t: &[T], w: usize) -> (usize, usize) {
    const WORD: usize = 64;
    let len2 = s2t.len();
    let words1 = s1t.len().div_ceil(WORD);
    let words2 = len2.div_ceil(WORD);

    let peq = T::peqn(s2t, words2);
    let zeros = vec![0u64; words2];

    let mut matched1 = vec![0u64; words1];
    let mut matched2 = vec![0u64; words2];
    let mut m = 0usize;

    for (i, &c1) in s1t.iter().enumerate() {
        let lo = i.saturating_sub(w);
        let hi = (i + w + 1).min(len2);
        if lo >= hi {
            continue;
        }
        let row = T::peqn_row(&peq, c1).unwrap_or(&zeros);
        let lo_w = lo / WORD;
        let hi_w = (hi - 1) / WORD;
        for wi in lo_w..=hi_w {
            let mut word = row[wi] & !matched2[wi];
            if wi == lo_w {
                word &= !mask_lsb(lo % WORD);
            }
            if wi == hi_w {
                word &= mask_lsb(hi - wi * WORD);
            }
            if word != 0 {
                matched2[wi] |= word & word.wrapping_neg();
                matched1[i / WORD] |= 1u64 << (i % WORD);
                m += 1;
                break;
            }
        }
    }

    if m == 0 {
        return (0, 0);
    }

    let mut t = 0usize;
    let mut word2 = 0usize;
    let mut f2 = matched2[0];
    for (b1, &mw1) in matched1.iter().enumerate() {
        let mut f1 = mw1;
        while f1 != 0 {
            let i = b1 * WORD + f1.trailing_zeros() as usize;
            while f2 == 0 {
                word2 += 1;
                f2 = matched2[word2];
            }
            let j = word2 * WORD + f2.trailing_zeros() as usize;
            t += usize::from(s1t[i] != s2t[j]);
            f1 &= f1 - 1;
            f2 &= f2 - 1;
        }
    }
    (m, t)
}

/// Jaro–Winkler similarity: Jaro, boosted for a shared prefix of up to 4.
pub fn jaro_winkler(s1: &str, s2: &str, opts: &Options) -> f64 {
    // The identity check runs on the *original* strings, before any case folding.
    if s1 == s2 {
        return 1.0;
    }

    let (a, b);
    let (s1, s2) = if opts.ignore_case {
        a = s1.to_lowercase();
        b = s2.to_lowercase();
        (a.as_str(), b.as_str())
    } else {
        (s1, s2)
    };

    let dj = opts.dj.unwrap_or_else(|| jaro(s1, s2));

    const P: f64 = 0.1;
    let l = common_prefix_len(s1, s2);
    dj + (l as f64) * P * (1.0 - dj)
}

/// Length of the common prefix, capped at 4.
///
/// # A reference subtlety
///
/// The reference loop is `while (s1[l] === s2[l] && l < 4) l++`, which tests the
/// characters *before* the bound. Once both strings are exhausted, `s1[l]` and
/// `s2[l]` are both `undefined`, and `undefined === undefined` is `true` — so the
/// counter keeps rising to 4 even for a two-character input. Comparing
/// `Option<u16>` reproduces that exactly, since `None == None`.
///
/// This is **observable**, not a harmless curiosity. It requires the two
/// (possibly case-folded) strings to be equal, which normally means a Jaro score
/// of 1.0 and a boost multiplied by `1 - 1 == 0`. But for single-character
/// inputs the match window is `floor(1/2) - 1 == -1`, so nothing can match and
/// Jaro is **0** — leaving the boost fully exposed:
///
/// ```text
/// JaroWinklerDistance("A", "a", { ignoreCase: true })
///   folds to ("a", "a") -> jaro = 0.0, l saturates at 4
///   result = 0.0 + 4 * 0.1 * (1 - 0.0) = 0.4      (not 1.0)
/// ```
fn common_prefix_len(s1: &str, s2: &str) -> usize {
    dispatch(s1, s2, |ops| match ops {
        Operands::Bytes(a, b) => prefix_generic(a, b),
        Operands::Units(a, b) => prefix_generic(a, b),
    })
}

fn prefix_generic<T: Copy + PartialEq>(s1: &[T], s2: &[T]) -> usize {
    let mut l = 0usize;
    while s1.get(l) == s2.get(l) && l < 4 {
        l += 1;
    }
    l
}

/// [`jaro_winkler`], fanned out across a `rayon` thread pool. Requires the
/// `parallel` feature.
///
/// # Why this exists
///
/// `jaro_winkler` is a pure function over two borrowed `&str`s with no shared
/// state, so scoring many independent pairs is embarrassingly parallel with
/// zero coordination cost between pairs. This function is exactly
/// `pairs.par_iter().map(|(a, b)| jaro_winkler(a, b, opts)).collect()` — a
/// thin fan-out over the existing sequential primitive, not a second
/// implementation of it. The windowed-match pass, the transposition pass and
/// the stack-vs-heap match-flag decision inside `jaro`/`jaro_winkler` are
/// untouched; if you need plain [`jaro`] in parallel, apply the same
/// `par_iter().map(...)` pattern at your own call site (see
/// `site/performance/parallelism.md`).
///
/// # When to reach for it vs. the sequential loop
///
/// `jaro_winkler` is cheap even at 1024 characters (see
/// `docs/PERFORMANCE.md`'s `jaro_winkler/*` rows), and a `rayon` task costs on
/// the order of a microsecond to schedule (`site/performance/parallelism.md`).
/// Measured on this crate's own `distance` benchmark (`cargo bench -p
/// verbora-distance --features parallel -- par_jaro_winkler`; 32-thread
/// machine, default global `rayon` pool), batches of 1000 pairs at each
/// length:
///
/// | Pair length | Sequential (1000 pairs) | Parallel (1000 pairs) | Speedup |
/// |---:|--:|--:|--:|
/// | 4    | 17.4 µs  | 106 µs   | 0.16× (parallel *loses*, badly) |
/// | 16   | 160 µs   | 41.1 µs  | 3.9× |
/// | 64   | 883 µs   | 112 µs   | 7.9× |
/// | 256  | 13.4 ms  | 974 µs   | 13.7× |
/// | 1024 | 167 ms   | 10.7 ms  | 15.6× |
///
/// For short pairs, or small batches, a plain `pairs.iter().map(|(a, b)|
/// jaro_winkler(a, b, opts)).collect()` loop wins outright — at 4 characters
/// even a 1000-pair batch is a net loss; this function pays off once pairs
/// are at least a few dozen characters or the batch is large. These are one
/// machine's numbers, not a guarantee — reproduce with the command above
/// before relying on them.
///
/// # Allocation behaviour
///
/// One `Vec<f64>` sized to `pairs.len()` for the output, plus whatever
/// `jaro_winkler` itself allocates per pair (a stack buffer for inputs up to
/// 128 units each, or a heap `Vec<bool>` beyond that — see the module-level
/// comment on `jaro_generic`). No additional buffering, no locking, no
/// per-call thread-pool construction — this uses whichever global `rayon`
/// pool is already installed (or `rayon`'s default one), so pool
/// configuration remains the caller's responsibility, not this crate's.
///
/// # Order and errors
///
/// Output order matches input order — `results[i] == jaro_winkler(pairs[i].0,
/// pairs[i].1, opts)` — via `rayon`'s order-preserving `map` + `collect`.
/// `jaro_winkler` never errors, so every element is a plain `f64`.
#[cfg(feature = "parallel")]
pub fn par_jaro_winkler_batch(pairs: &[(&str, &str)], opts: &Options) -> Vec<f64> {
    use rayon::prelude::*;
    pairs
        .par_iter()
        .map(|(a, b)| jaro_winkler(a, b, opts))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn jw(a: &str, b: &str) -> f64 {
        jaro_winkler(a, b, &Options::default())
    }

    #[test]
    fn identical_strings_score_one() {
        assert_eq!(jw("", ""), 1.0);
        assert_eq!(jw("abc", "abc"), 1.0);
        assert_eq!(jw("😀", "😀"), 1.0);
    }

    #[test]
    fn classic_reference_values() {
        assert!((jw("MARTHA", "MARHTA") - 0.9611111111111111).abs() < 1e-12);
        assert!((jw("DIXON", "DICKSONX") - 0.8133333333333332).abs() < 1e-12);
        assert!((jw("DWAYNE", "DUANE") - 0.84).abs() < 1e-12);
    }

    #[test]
    fn empty_against_nonempty_is_zero() {
        assert_eq!(jw("", "abc"), 0.0);
        assert_eq!(jw("abc", ""), 0.0);
    }

    #[test]
    fn single_char_window_is_negative_and_yields_zero() {
        // match_window = floor(1/2) - 1 = -1, so nothing can match.
        assert_eq!(jaro("a", "b"), 0.0);
        assert_eq!(jw("a", "b"), 0.0);
    }

    #[test]
    fn ignore_case_folds_before_comparing() {
        assert!(jw("MARTHA", "martha") < 1.0);
        let folded = jaro_winkler(
            "MARTHA",
            "martha",
            &Options {
                ignore_case: true,
                dj: None,
            },
        );
        assert_eq!(folded, 1.0);
    }

    #[test]
    fn supplied_dj_short_circuits() {
        let r = jaro_winkler(
            "abcd",
            "abcz",
            &Options {
                ignore_case: false,
                dj: Some(0.0),
            },
        );
        // dj = 0, prefix = 3 => 0 + 3 * 0.1 * 1 = 0.3
        assert!((r - 0.3).abs() < 1e-12);
    }

    #[test]
    fn single_char_ignore_case_exposes_the_prefix_quirk() {
        // Folding makes the strings equal, but a length-1 match window of -1
        // leaves Jaro at 0, so the saturated prefix boost shows through:
        // 0 + 4 * 0.1 * (1 - 0) = 0.4. Verified against the reference.
        let o = Options {
            ignore_case: true,
            dj: None,
        };
        assert_eq!(jaro_winkler("A", "a", &o), 0.4);
        assert_eq!(jaro_winkler("X", "x", &o), 0.4);
        // Two characters is enough for the window to open up again.
        assert_eq!(jaro_winkler("AB", "ab", &o), 1.0);
    }

    #[test]
    fn prefix_counter_saturates_at_four() {
        assert_eq!(prefix_generic(b"abcdefg", b"abcdefg"), 4);
        assert_eq!(prefix_generic(b"abz", b"abx"), 2);
        // Both exhausted: None == None keeps the counter climbing to the cap.
        assert_eq!(prefix_generic(b"ab", b"ab"), 4);
    }

    // -- Bit-parallel Jaro battery ------------------------------------------

    /// Deterministic PRNG, same shape as `levenshtein.rs`'s test helper.
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
    }

    /// A forced matching kernel: `(s1_trimmed, s2_trimmed, window)` to
    /// `(matches, raw_transpositions)`.
    type Kernel<T> = fn(&[T], &[T], usize) -> (usize, usize);

    /// The full Jaro similarity computed through a FORCED kernel, so tests
    /// can exercise `jaro_bit_word`/`jaro_bit_block` on inputs the
    /// size-dispatch in `jaro_generic` would route to the scalar loop.
    fn jaro_forced<T: BitPeq>(s1: &[T], s2: &[T], kernel: Kernel<T>) -> f64 {
        let len1 = s1.len();
        let len2 = s2.len();
        if len1 == 0 || len2 == 0 {
            return 0.0;
        }
        let max_len = len1.max(len2);
        if max_len < 2 {
            return 0.0;
        }
        let w = max_len / 2 - 1;
        let len1t = len1.min(len2 + w);
        let len2t = len2.min(len1 + w);
        let (m, t_raw) = kernel(&s1[..len1t], &s2[..len2t], w);
        if m == 0 {
            return 0.0;
        }
        let t = t_raw as f64 / 2.0;
        let m = m as f64;
        ((m / len1 as f64) + (m / len2 as f64) + ((m - t) / m)) / 3.0
    }

    /// Scalar reference through the same wrapper (no trim -- the scalar
    /// loop's own windowing already ignores out-of-window cells).
    fn jaro_scalar_full<T: Copy + PartialEq>(s1: &[T], s2: &[T]) -> f64 {
        let len1 = s1.len();
        let len2 = s2.len();
        if len1 == 0 || len2 == 0 {
            return 0.0;
        }
        let (m, t_raw) = jaro_scalar(s1, s2);
        if m == 0 {
            return 0.0;
        }
        let t = t_raw as f64 / 2.0;
        let m = m as f64;
        ((m / len1 as f64) + (m / len2 as f64) + ((m - t) / m)) / 3.0
    }

    #[test]
    fn jaro_bit_kernels_agree_with_the_scalar_loop_exhaustively() {
        // Every pair over {a,b} with lengths <= 8 (65,535+ pairs) and over
        // {a,b,c} with lengths <= 6 -- the same exhaustive spaces the
        // design-phase experiment used, where repeated characters stress
        // the greedy lowest-bit choice hardest. Bitwise f64 equality,
        // scalar vs word vs block.
        fn enumerate(alphabet: &[u8], max_len: usize) -> Vec<Vec<u8>> {
            let mut out: Vec<Vec<u8>> = vec![Vec::new()];
            let mut frontier: Vec<Vec<u8>> = vec![Vec::new()];
            for _ in 0..max_len {
                let mut next = Vec::new();
                for s in &frontier {
                    for &c in alphabet {
                        let mut t = s.clone();
                        t.push(c);
                        next.push(t);
                    }
                }
                out.extend(next.iter().cloned());
                frontier = next;
            }
            out
        }

        for (alphabet, max_len) in [(&b"ab"[..], 8usize), (&b"abc"[..], 6usize)] {
            let all = enumerate(alphabet, max_len);
            for s1 in &all {
                for s2 in &all {
                    let scalar = jaro_scalar_full(s1, s2);
                    let word = jaro_forced(s1, s2, jaro_bit_word::<u8>);
                    let block = jaro_forced(s1, s2, jaro_bit_block::<u8>);
                    assert_eq!(
                        scalar.to_bits(),
                        word.to_bits(),
                        "word mismatch for {s1:?} vs {s2:?}"
                    );
                    assert_eq!(
                        scalar.to_bits(),
                        block.to_bits(),
                        "block mismatch for {s1:?} vs {s2:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn jaro_bit_kernels_agree_on_random_long_pairs() {
        // Randomized differential across the sizes the dispatch actually
        // sends to each kernel, including asymmetric pairs that exercise
        // the length trim, and the public `jaro()` on top (which also
        // covers the ASCII/UTF-16 dispatch).
        let mut rng = Xorshift64(0x1A80_1A80_1A80);
        const ALPHABETS: [&[u8]; 3] = [b"ab", b"abcd", b"abcdefghijklmnopqrstuvwxyz"];
        for round in 0..600 {
            let alphabet = ALPHABETS[round % ALPHABETS.len()];
            let len1 = rng.next_range(300);
            let len2 = if round % 3 == 0 {
                rng.next_range(30)
            } else {
                rng.next_range(300)
            };
            let s1: Vec<u8> = (0..len1)
                .map(|_| alphabet[rng.next_range(alphabet.len())])
                .collect();
            let s2: Vec<u8> = (0..len2)
                .map(|_| alphabet[rng.next_range(alphabet.len())])
                .collect();

            let scalar = jaro_scalar_full(&s1, &s2);
            let block = jaro_forced(&s1, &s2, jaro_bit_block::<u8>);
            assert_eq!(
                scalar.to_bits(),
                block.to_bits(),
                "block mismatch at round {round} ({len1}x{len2})"
            );

            let a = String::from_utf8(s1).expect("ascii");
            let b = String::from_utf8(s2).expect("ascii");
            assert_eq!(
                jaro(&a, &b).to_bits(),
                scalar.to_bits(),
                "public dispatch mismatch at round {round}"
            );
        }
    }

    #[test]
    fn jaro_bit_kernels_agree_at_boundaries_and_trim_edges() {
        let mut rng = Xorshift64(0xED9E_0EDD);
        for &len1 in &[17usize, 63, 64, 65, 127, 128, 129, 200] {
            for _ in 0..10 {
                let s1: Vec<u8> = (0..len1)
                    .map(|_| b'a' + (rng.next_range(4) as u8))
                    .collect();
                // longer = shorter + w, +/- 1 around the trim boundary, plus
                // equal lengths and 1-vs-n.
                let w = len1.max(2) / 2 - 1;
                for &len2 in &[1usize, len1, len1 + w - 1, len1 + w, len1 + w + 1, len1 * 2] {
                    let s2: Vec<u8> = (0..len2)
                        .map(|_| b'a' + (rng.next_range(4) as u8))
                        .collect();
                    let scalar = jaro_scalar_full(&s1, &s2);
                    let block = jaro_forced(&s1, &s2, jaro_bit_block::<u8>);
                    assert_eq!(
                        scalar.to_bits(),
                        block.to_bits(),
                        "mismatch at {len1}x{len2}"
                    );
                    let a = String::from_utf8(s1.clone()).expect("ascii");
                    let b = String::from_utf8(s2.clone()).expect("ascii");
                    assert_eq!(jaro(&a, &b).to_bits(), scalar.to_bits());
                }
            }
        }
    }

    #[test]
    fn jaro_fractional_transpositions_are_preserved() {
        // m = 3, raw transpositions = 3 (odd) => t = 1.5, jaro = 2/3
        // exactly. An integer `raw / 2` halving -- what rapidfuzz does --
        // would produce a different value here; this pins Verbora's
        // fractional semantics through every kernel.
        let expected = ((3.0f64 / 3.0) + (3.0 / 6.0) + ((3.0 - 1.5) / 3.0)) / 3.0;
        assert_eq!(jaro("abc", "bcaaaa").to_bits(), expected.to_bits());
        let s1 = b"abc";
        let s2 = b"bcaaaa";
        assert_eq!(
            jaro_forced(&s1[..], &s2[..], jaro_bit_word::<u8>).to_bits(),
            expected.to_bits()
        );
        assert_eq!(
            jaro_forced(&s1[..], &s2[..], jaro_bit_block::<u8>).to_bits(),
            expected.to_bits()
        );
    }

    #[test]
    fn jaro_bit_kernels_agree_on_utf16_input() {
        // The u16 monomorphization (FxHashMap-backed Peq), including astral
        // input where one char is two units.
        let mut rng = Xorshift64(0x0016_0016_0016);
        const CYRILLIC: &[char] = &['\u{430}', '\u{431}', '\u{432}', '\u{433}', '\u{434}'];
        for &(len1, len2) in &[
            (20usize, 25usize),
            (60, 64),
            (65, 70),
            (128, 200),
            (300, 40),
        ] {
            let a: String = (0..len1)
                .map(|_| CYRILLIC[rng.next_range(CYRILLIC.len())])
                .collect();
            let b: String = (0..len2)
                .map(|_| CYRILLIC[rng.next_range(CYRILLIC.len())])
                .collect();
            let ua: Vec<u16> = a.encode_utf16().collect();
            let ub: Vec<u16> = b.encode_utf16().collect();
            let scalar = jaro_scalar_full(&ua[..], &ub[..]);
            assert_eq!(
                jaro(&a, &b).to_bits(),
                scalar.to_bits(),
                "utf16 dispatch mismatch {len1}x{len2}"
            );
            let block = jaro_forced(&ua[..], &ub[..], jaro_bit_block::<u16>);
            assert_eq!(scalar.to_bits(), block.to_bits(), "utf16 block mismatch");
        }
        let a = "\u{1F600}\u{1F601}".repeat(20);
        let b = "\u{1F601}\u{1F600}".repeat(20);
        let ua: Vec<u16> = a.encode_utf16().collect();
        let ub: Vec<u16> = b.encode_utf16().collect();
        assert_eq!(
            jaro(&a, &b).to_bits(),
            jaro_scalar_full(&ua[..], &ub[..]).to_bits()
        );
    }

    // -- Adversarial battery (audit) ----------------------------------------

    /// A SplitMix64 PRNG — a different algorithm (not just a different
    /// seed) from the Xorshift64 used by every other randomized test in
    /// this module, so its coverage does not share blind spots with them.
    struct SplitMix64(u64);

    impl SplitMix64 {
        fn next_u64(&mut self) -> u64 {
            self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = self.0;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z ^ (z >> 31)
        }

        fn next_range(&mut self, bound: usize) -> usize {
            (self.next_u64() % bound as u64) as usize
        }
    }

    /// Asserts scalar == public dispatch == block kernel (always), and ==
    /// word kernel wherever the trimmed operands actually fit one word —
    /// the same gate `jaro_generic` applies, so the word kernel is never
    /// called outside its domain.
    fn assert_all_paths_agree(s1: &[u8], s2: &[u8], context: &str) {
        let scalar = jaro_scalar_full(s1, s2);
        let block = jaro_forced(s1, s2, jaro_bit_block::<u8>);
        assert_eq!(
            scalar.to_bits(),
            block.to_bits(),
            "block mismatch: {context}"
        );

        let max_len = s1.len().max(s2.len());
        if max_len >= 2 {
            let w = max_len / 2 - 1;
            let len1t = s1.len().min(s2.len() + w);
            let len2t = s2.len().min(s1.len() + w);
            if len1t <= 64 && len2t <= 64 {
                let word = jaro_forced(s1, s2, jaro_bit_word::<u8>);
                assert_eq!(scalar.to_bits(), word.to_bits(), "word mismatch: {context}");
            }
        }

        let a = String::from_utf8(s1.to_vec()).expect("ascii");
        let b = String::from_utf8(s2.to_vec()).expect("ascii");
        assert_eq!(
            jaro(&a, &b).to_bits(),
            scalar.to_bits(),
            "public dispatch mismatch: {context}"
        );
    }

    #[test]
    fn jaro_word_kernel_window_saturation_agrees() {
        // The word kernel's `mask_lsb(w + 1)` saturates when `w + 1 >= 64`,
        // which the dispatch reaches only for a very short s1 against an
        // s2 of length ~128 (e.g. len1 = 1, len2 = 128 gives w = 63 with
        // both trimmed operands still <= 64). Sweep the whole neighbourhood
        // in both argument orders — no existing test hits this shape except
        // by luck.
        let mut rng = SplitMix64(0x5A70_0001_A0D1_7000);
        for len1 in 1usize..=4 {
            for len2 in 110usize..=140 {
                let s1: Vec<u8> = (0..len1).map(|_| b"ab"[rng.next_range(2)]).collect();
                let s2: Vec<u8> = (0..len2).map(|_| b"ab"[rng.next_range(2)]).collect();
                assert_all_paths_agree(&s1, &s2, &format!("saturation {len1}x{len2}"));
                assert_all_paths_agree(&s2, &s1, &format!("saturation {len2}x{len1}"));
            }
        }
        // The exact saturation edge, deterministically: a lone matching /
        // non-matching character against uniform seas.
        for len2 in [126usize, 127, 128, 129, 130] {
            let s2: Vec<u8> = vec![b'a'; len2];
            assert_all_paths_agree(b"a", &s2, &format!("lone match vs {len2}"));
            assert_all_paths_agree(b"z", &s2, &format!("lone miss vs {len2}"));
        }
    }

    #[test]
    fn jaro_trim_boundary_matches_with_repeated_chars() {
        // The length trim cuts s2 at `len1 + w`. A match sitting exactly at
        // column `len1 + w - 1` (the last kept position, reachable only
        // from i = len1 - 1 at full window stretch) must survive; one at
        // `len1 + w` must not exist for the scalar loop either. Repeated
        // 'a's straddling that cut stress the greedy lowest-bit choice at
        // the exact edge. An off-by-one trim changes m and shows up as a
        // different similarity.
        for &(len1, len2) in &[
            (20usize, 60usize),
            (33, 80),
            (65, 160),
            (100, 260),
            (17, 40),
        ] {
            let w = len1.max(len2) / 2 - 1;
            let edge = len1 - 1 + w; // farthest reachable column for i = len1 - 1
            if edge >= len2 {
                panic!("test shape broken: edge {edge} >= len2 {len2}");
            }
            // Variant 1: the only match is exactly at the edge column.
            let mut s1 = vec![b'z'; len1];
            s1[len1 - 1] = b'a';
            let mut s2 = vec![b'y'; len2];
            s2[edge] = b'a';
            assert_all_paths_agree(&s1, &s2, &format!("edge match {len1}x{len2}"));

            // Variant 2: the would-be match is one past the edge — out of
            // every window, so m = 0 on all paths.
            let mut s2_out = vec![b'y'; len2];
            if edge + 1 < len2 {
                s2_out[edge + 1] = b'a';
                assert_all_paths_agree(&s1, &s2_out, &format!("edge miss {len1}x{len2}"));
            }

            // Variant 3: a run of repeated 'a's straddling the cut — the
            // greedy scan must take the in-window ones leftmost-first and
            // never see the trimmed tail.
            let mut s2_run = vec![b'y'; len2];
            let run_start = edge.saturating_sub(2);
            for slot in s2_run.iter_mut().take((edge + 3).min(len2)).skip(run_start) {
                *slot = b'a';
            }
            let mut s1_run = vec![b'z'; len1];
            s1_run[len1 - 1] = b'a';
            s1_run[len1 - 2] = b'a';
            assert_all_paths_agree(&s1_run, &s2_run, &format!("edge run {len1}x{len2}"));
        }
    }

    #[test]
    fn jaro_sparse_matches_across_many_words_agree() {
        // Matches clustered at the two ends of multi-word operands with
        // nothing but mismatches between them: the transposition walk must
        // skip whole all-zero words of `matched2` while pairing set bits in
        // lockstep. The end clusters are internally swapped ('ab'/'ba',
        // 'cd'/'dc') so the walk counts genuine transpositions on both
        // sides of the empty gap.
        for &n in &[124usize, 128, 200, 300, 383] {
            let mut s1 = vec![b'x'; n + 4];
            let mut s2 = vec![b'y'; n + 4];
            s1[0] = b'a';
            s1[1] = b'b';
            s2[0] = b'b';
            s2[1] = b'a';
            s1[n + 2] = b'c';
            s1[n + 3] = b'd';
            s2[n + 2] = b'd';
            s2[n + 3] = b'c';
            assert_all_paths_agree(&s1, &s2, &format!("sparse ends n={n}"));

            // Shifted variant: the tail cluster of s2 sits w-ish positions
            // earlier, so the match pairs are asymmetric across a word
            // boundary rather than aligned.
            let mut s2_shift = vec![b'y'; n + 4];
            s2_shift[0] = b'b';
            s2_shift[1] = b'a';
            let off = 30.min(n / 2);
            s2_shift[n + 2 - off] = b'd';
            s2_shift[n + 3 - off] = b'c';
            assert_all_paths_agree(&s1, &s2_shift, &format!("sparse shifted n={n}"));
        }
    }

    #[test]
    fn jaro_bit_word_u16_direct_agrees() {
        // `jaro_bit_word::<u16>` was only reachable through the public
        // dispatch before; pin the monomorphization directly, including
        // surrogate halves (astral chars split into two units).
        let mut rng = SplitMix64(0x0016_D1EC_7000_0001);
        const UNITS: &[u16] = &[0x430, 0x431, 0x432, 0xD83D, 0xDE00, 0x4E2D];
        for &(len1, len2) in &[
            (17usize, 20usize),
            (25, 25),
            (33, 60),
            (40, 64),
            (64, 64),
            (60, 17),
        ] {
            let s1: Vec<u16> = (0..len1)
                .map(|_| UNITS[rng.next_range(UNITS.len())])
                .collect();
            let s2: Vec<u16> = (0..len2)
                .map(|_| UNITS[rng.next_range(UNITS.len())])
                .collect();
            let scalar = jaro_scalar_full(&s1[..], &s2[..]);
            let max_len = len1.max(len2);
            let w = max_len / 2 - 1;
            let len1t = len1.min(len2 + w);
            let len2t = len2.min(len1 + w);
            assert!(len1t <= 64 && len2t <= 64, "test shape must fit one word");
            let word = jaro_forced(&s1[..], &s2[..], jaro_bit_word::<u16>);
            let block = jaro_forced(&s1[..], &s2[..], jaro_bit_block::<u16>);
            assert_eq!(scalar.to_bits(), word.to_bits(), "u16 word {len1}x{len2}");
            assert_eq!(scalar.to_bits(), block.to_bits(), "u16 block {len1}x{len2}");
        }
    }

    #[test]
    fn jaro_large_randomized_differential_splitmix() {
        // Independent large-scale differential: SplitMix64 (different PRNG
        // algorithm from every pre-audit test here), wider length range,
        // heavily asymmetric pairs in a third of the rounds, and a
        // repeated-character-rich alphabet to stress greedy tie-breaking.
        let mut rng = SplitMix64(0x1A20_AD17_2026_0816);
        const ALPHABETS: [&[u8]; 3] = [b"ab", b"aab", b"abcdef"];
        for round in 0..400 {
            let alphabet = ALPHABETS[round % ALPHABETS.len()];
            let len1 = 1 + rng.next_range(500);
            let len2 = if round % 3 == 0 {
                1 + rng.next_range(12)
            } else {
                1 + rng.next_range(500)
            };
            let s1: Vec<u8> = (0..len1)
                .map(|_| alphabet[rng.next_range(alphabet.len())])
                .collect();
            let s2: Vec<u8> = (0..len2)
                .map(|_| alphabet[rng.next_range(alphabet.len())])
                .collect();
            assert_all_paths_agree(&s1, &s2, &format!("round {round} ({len1}x{len2})"));
        }

        // The u16 monomorphization at the same scale, via public dispatch
        // (bitwise) and the forced block kernel.
        const CYRILLIC: &[char] = &['\u{430}', '\u{431}', '\u{432}'];
        for round in 0..60 {
            let len1 = 1 + rng.next_range(400);
            let len2 = 1 + rng.next_range(400);
            let a: String = (0..len1)
                .map(|_| CYRILLIC[rng.next_range(CYRILLIC.len())])
                .collect();
            let b: String = (0..len2)
                .map(|_| CYRILLIC[rng.next_range(CYRILLIC.len())])
                .collect();
            let ua: Vec<u16> = a.encode_utf16().collect();
            let ub: Vec<u16> = b.encode_utf16().collect();
            let scalar = jaro_scalar_full(&ua[..], &ub[..]);
            let block = jaro_forced(&ua[..], &ub[..], jaro_bit_block::<u16>);
            assert_eq!(
                scalar.to_bits(),
                block.to_bits(),
                "u16 round {round} ({len1}x{len2})"
            );
            assert_eq!(jaro(&a, &b).to_bits(), scalar.to_bits());
        }
    }
}
