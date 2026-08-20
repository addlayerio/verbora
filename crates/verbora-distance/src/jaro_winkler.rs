//! Jaro and Jaro–Winkler: the windowed greedy loop, the bit-parallel matching
//! pass, and the reductions that feed them.
//!
//! **Internal notes.** The definitions and citations, the guarantees, the
//! match window and its clamp, the fixed `p`, and the choice between the two
//! functions are published on [`jaro`], [`fn@jaro_winkler`] and the crate
//! root. What follows is why the matching pass is shaped the way it is.
//!
//! `docs/design/distance-contract.md` §4.5 records the decision to keep `p`
//! unconfigurable, and §7 the measurement debt below.
//!
//! # Benchmarks
//!
//! `UNMEASURED` for the current code. The crossover tables this module used to
//! publish were measured against a different signature and before the scalar
//! unit landed; they are removed rather than adjusted, and restored only from
//! a fresh full-precision run.
//!
//! # Performance shape
//!
//! Short operands (`max(n1, n2) <= 16`) run the classical windowed greedy
//! loop, which beats every bit-parallel formulation there because building a
//! pattern-match table costs more than the handful of window compares it
//! would replace. Longer operands run a bit-parallel matching pass — one
//! 64-bit word when the trimmed operands fit, a multi-word kernel otherwise —
//! after two reductions that never change the answer: positions of the longer
//! operand beyond `shorter + w` are dropped (no window can reach them), and
//! the common prefix is counted rather than scanned (the proof that the
//! greedy pass matches a common prefix in place is on `jaro_generic`, and it
//! is pinned by `jaro_common_prefix_is_matched_in_place`).

use crate::units::{BitPeq, Operands, Unit, common_prefix_len, dispatch};

/// Jaro similarity between two strings: `1.0` identical, `0.0` nothing in
/// common.
///
/// Jaro, M. A. (1989), *Advances in record linkage methodology as applied to
/// matching the 1985 census of Tampa, Florida*, JASA 84(406), 414–420. This is
/// a **similarity**, not a distance: higher is closer.
///
/// With `n1` and `n2` the operands' scalar counts, `m` the number of matched
/// units and `t` half the number of matched units appearing in a different
/// relative order, the score is
///
/// ```text
/// ((m / n1) + (m / n2) + ((m - t) / m)) / 3
/// ```
///
/// and `0.0` when `m == 0`. Two units match when they are equal and no more
/// than `w = max(0, ⌊max(n1, n2) / 2⌋ − 1)` positions apart.
///
/// The result is always finite and in `0.0..=1.0`, always symmetric, and
/// exactly `1.0` if and only if the two operands are equal — see
/// [*What the similarities guarantee*](crate#what-the-similarities-guarantee)
/// for the shared frame and the degenerate cases.
///
/// # The unit is the Unicode scalar value
///
/// Both operand lengths, the match window and (in
/// [`jaro_winkler`](fn@crate::jaro_winkler)) the prefix length are counted in
/// `char`s, so `"a😀b"` is three units. One consequence worth stating: `"😀"`
/// is a *single* unit, so it lands in the same degenerate class as `"a"` —
/// which is why the window clamp below is load-bearing for real text rather
/// than a curiosity about single letters.
///
/// # The match window, and why it is clamped at zero
///
/// Jaro's window is `w = max(0, ⌊max(n1, n2) / 2⌋ − 1)` units: `s1[i]` and
/// `s2[j]` may match only when they are equal *and* `|i − j| <= w`. The
/// unclamped expression is negative exactly when `max(n1, n2) <= 1`, and there
/// it prunes the one candidate pair at displacement `0`. The window is a
/// **pruning device** — Jaro introduces `|i − j| <= w` so that units too far
/// apart are not treated as matches — not a definition of matching, so
/// evaluating it outside its intended domain is not a result to preserve.
///
/// Clamping therefore changes exactly one input class, both operands one unit
/// long, and there it is the difference between the identity guarantee holding
/// and failing: `jaro("a", "a")` is `1.0` and `jaro("a", "b")` is still `0.0`.
/// Every input with `max(n1, n2) >= 2` is untouched, since `⌊max/2⌋ − 1 >= 0`
/// already.
///
/// # No input rewriting
///
/// Neither this function nor [`jaro_winkler`](fn@crate::jaro_winkler) folds
/// case, trims or normalises anything, so `"A"` and `"a"` are different units.
/// Caseless matching is the caller's, folded once at ingestion rather than
/// re-folded against every candidate:
/// `jaro(&a.to_lowercase(), &b.to_lowercase())`.
///
/// # Examples
///
/// ```
/// use verbora_distance::jaro;
///
/// // Jaro's own worked example: m = 6, t = 1, so (6/6 + 6/6 + 5/6) / 3.
/// assert!((jaro("MARTHA", "MARHTA") - 17.0 / 18.0).abs() < 1e-12);
///
/// assert_eq!(jaro("abc", "abc"), 1.0);
/// assert_eq!(jaro("", ""), 1.0);
/// assert_eq!(jaro("a", ""), 0.0);
/// assert_eq!(jaro("a", "b"), 0.0);
///
/// // One scalar is one unit, so an astral character is a one-unit operand.
/// assert_eq!(jaro("😀", "😀"), 1.0);
/// assert_eq!(jaro("😀", "😁"), 0.0);
///
/// // Case is significant — nothing is rewritten.
/// assert_eq!(jaro("ABC", "abc"), 0.0);
/// ```
///
/// # Allocation
///
/// For ASCII operands: none at all when `max(n1, n2) <= 16` — the greedy
/// loop's match flags are stack arrays — and none when both trimmed operands
/// fit one 64-bit word, since the byte pattern-match table is a fixed
/// 256-entry stack array. Beyond that, one `Vec<u64>` for the packed
/// pattern-match table; the match-flag bitsets stay on the stack up to 1024
/// units per side. Non-ASCII operands additionally cost one `Vec<char>` per
/// side for the scalar decode, and their pattern-match tables are hash maps
/// rather than flat arrays.
#[must_use]
pub fn jaro(s1: &str, s2: &str) -> f64 {
    dispatch(s1, s2, |ops| match ops {
        Operands::Bytes(a, b) => jaro_generic(a, b),
        Operands::Units(a, b) => jaro_generic(a, b),
    })
}

/// Sizes at or below this stay on the classical scalar loop — measured to
/// already beat every bit-parallel formulation there, because a 2 KB Peq
/// table build costs more than the handful of window-scan compares it would
/// replace.
const JARO_SCALAR_MAX: usize = 16;

/// Jaro similarity over one consistent unit representation.
///
/// # Why the common prefix can be counted instead of scanned
///
/// Jaro's match window is derived from the *original* operand lengths, so
/// the affix trimming [`crate::levenshtein()`] applies is not available here
/// as-is — cutting characters off would change `max_len`, hence `w`, hence
/// the answer. What *is* available is a strictly narrower reduction that
/// keeps `w` fixed and only skips work the greedy pass would do anyway.
///
/// Let `p` be the length of the longest common prefix and `w >= 0` (which
/// the clamp below guarantees for every input). Claim: the
/// greedy scan matches position `i` of `s1` to position `i` of `s2` for
/// every `i < p`. By induction — at step `i`, positions `0..i` of `s2` are
/// exactly the ones already taken, the window `[max(0, i − w), …)` contains
/// `i` because `w >= 0`, and the scan takes the leftmost free position that
/// matches, which is `i` itself since `s1[i] == s2[i]`. So the prefix
/// contributes `p` matches at aligned positions, and — being aligned and
/// equal — zero transpositions, while leaving the *rest* of the scan
/// identical to what it would do untrimmed (the positions the trimmed
/// window drops below index `p` are precisely the ones already consumed).
/// `m` is therefore `p + m_rest` and `t` is `t_rest`, with the original
/// `len1`/`len2` still feeding the similarity formula.
///
/// The mirrored reduction on the common *suffix* is **not** sound: the scan
/// is left-to-right and greedy, so a trailing character can legitimately be
/// consumed by an earlier position. `jaro("ab", "baba")` has two matches
/// and two raw transpositions; trimming the shared `"b"`…`"ab"` tail keeps
/// the match count but loses both transpositions. This module's
/// `jaro_common_suffix_trimming_is_invalid` pins that counterexample so the
/// asymmetry cannot be "tidied up" later.
fn jaro_generic<T: BitPeq>(s1: &[T], s2: &[T]) -> f64 {
    let len1 = s1.len();
    let len2 = s2.len();
    // The two empty cases are different questions and get different answers.
    // `("", "")` is the *identity* case — two identical operands — and the
    // identity axiom fixes it at 1.0; the formula's `0/0` there is a
    // removable singularity, and which value to insert is settled by
    // consistency rather than by the formula. Exactly one empty operand is
    // the *disjoint* case: there is nothing to match, `m == 0`, and the
    // standard's own `m = 0` clause gives 0.0.
    if len1 == 0 && len2 == 0 {
        return 1.0;
    }
    if len1 == 0 || len2 == 0 {
        return 0.0;
    }

    let max_len = len1.max(len2);
    // The match window `floor(max/2) - 1`, **clamped at 0**.
    //
    // The unclamped expression is negative only when `max(n1, n2) <= 1`, and
    // there evaluating it prunes the one candidate pair at displacement 0 —
    // the window is a *pruning* device (Jaro introduces `|i - j| <= w` so
    // that units too far apart are not treated as matches), not a definition
    // of matching, so evaluating it outside its intended domain is not a
    // result to preserve. Clamping changes exactly one input class, both
    // operands one unit long, and there it is the difference between the
    // identity axiom holding and failing: `jaro("a","a")` is `1.0` and
    // `jaro("a","b")` stays `0.0`. Every input with `max >= 2` is untouched,
    // since `floor(max/2) - 1 >= 0` there.
    //
    // Under the scalar unit this is load-bearing rather than cosmetic:
    // `"😀"` is one unit, so without the clamp `jaro("😀","😀")` would fall
    // into the degenerate branch and silently return `0.0`.
    let (m, t_raw) = if max_len <= JARO_SCALAR_MAX {
        jaro_scalar(s1, s2)
    } else {
        // Positions of the longer operand past `shorter + w` can never
        // fall inside any match window (a match needs `|i - j| <= w`; the
        // trim boundary is where the clamped window empties), so the bit
        // kernels never look at them. The *original* lengths still feed
        // the final formula below — the trim changes which cells are
        // scanned, never the denominators.
        let w = (max_len / 2).saturating_sub(1);
        let len1t = len1.min(len2 + w);
        let len2t = len2.min(len1 + w);
        let s1t = &s1[..len1t];
        let s2t = &s2[..len2t];
        // Common *prefix* only — see `jaro_common_prefix_is_matched_in
        // _place` for why the greedy pass provably matches it position for
        // position, and `jaro_common_suffix_trimming_is_invalid` for the
        // counterexample showing the mirrored reduction is unsound.
        let prefix = common_prefix_len(s1t, s2t);
        let (s1t, s2t) = (&s1t[prefix..], &s2t[prefix..]);
        let (m_rest, t_raw) = if s1t.is_empty() || s2t.is_empty() {
            (0, 0)
        } else if s1t.len() <= 64 && s2t.len() <= 64 {
            jaro_bit_word(s1t, s2t, w)
        } else {
            jaro_bit_block(s1t, s2t, w)
        };
        (m_rest + prefix, t_raw)
    };

    if m == 0 {
        return 0.0;
    }

    let t = t_raw as f64 / 2.0;
    let m = m as f64;
    // Three separate divisions summed left to right in exactly this
    // grouping, and `t` kept fractional as `raw / 2.0` (observable as x.5
    // for an odd raw count). Both are specified rather than incidental
    // (`docs/design/distance-contract.md` §3.4): under IEEE-754 a different
    // grouping — or an integer `raw / 2` halving — is a different number,
    // and §6.4 pins `jaro("abc", "bcaaaa")` bitwise against this expression.
    ((m / len1 as f64) + (m / len2 as f64) + ((m - t) / m)) / 3.0
}

/// The classical windowed greedy loop, returning `(matches,
/// raw_transpositions)`. The definition every bit-parallel kernel below is
/// differentially pinned against — and still the production path for
/// small inputs, where it wins outright.
fn jaro_scalar<T: Copy + PartialEq>(s1: &[T], s2: &[T]) -> (usize, usize) {
    let len1 = s1.len();
    let len2 = s2.len();
    // `floor(max/2) - 1`, clamped at 0 — see [`jaro_generic`] for why the
    // clamp is the definition rather than a repair.
    let match_window = (len1.max(len2) / 2).saturating_sub(1);

    // Match flags live on the stack for short inputs. The work this path
    // does is `O(len1 * (2w + 1))` window compares — for a four-unit operand
    // that is a handful of them, against two `vec![]` allocations and their
    // frees, which are unbounded calls into the allocator. Words are short by
    // nature, so this is the common path, not a micro-optimisation for a rare
    // case. The bound is a constant rather than a tuned threshold: 128 flags
    // per side is 256 stack bytes, small enough to cost nothing when unused.
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
        let start = i.saturating_sub(match_window);
        let end = (i + match_window + 1).min(len2);

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
/// direct slice compare per pair: the `k`-th set bit of `matched1` and the
/// `k`-th of `matched2` are by construction the `k`-th matched pair, so the
/// comparison needs neither a Peq lookup nor an `nth` walk to find them.
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
/// leftmost-first choice. Recomputing the window per step rather than
/// carrying it as incremental state is deliberate: the bound is a closed
/// form in `i`, so a handful of ALU ops reproduce it exactly and there is no
/// carried state whose drift could go unnoticed.
///
/// # The saturated-head cursor
///
/// The window is `2w + 1` positions wide, so for similar operands it spans
/// many words whose `matched2` bits are already *all* set — the scan reads
/// and discards them on every one of the `len1` steps, which is quadratic
/// work that produces nothing. `head` is the index of the lowest word not
/// yet fully matched. Because `matched2` bits are only ever set, never
/// cleared, `head` is monotonically non-decreasing, so advancing it costs
/// `O(words2)` across the entire call rather than per step. Starting the
/// scan at `max(lo_w, head)` is exactly equivalent, not an approximation:
/// every skipped word `wi < head` has `matched2[wi]` saturated over all of
/// its *valid* positions and `peqn` never sets a bit outside those, so
/// `row[wi] & !matched2[wi]` is provably zero there. Once `head` passes the
/// last word, no later position can match at all and the scan stops
/// outright.
///
/// # Working set
///
/// The two match-flag vectors are the only per-call state, one bit per
/// position rather than the scalar loop's one `bool` byte, so operands up
/// to `STACK_WORDS * 64` units keep them entirely on the stack — the same
/// stack-first trade `jaro_scalar` already makes for its `bool` arrays, and
/// worth making here too now that the affix reduction routes short,
/// near-identical residues through this kernel. A unit absent from the
/// pattern is skipped outright instead of being scanned against a
/// borrowed all-zero row, which removes the third buffer entirely.
fn jaro_bit_block<T: BitPeq>(s1t: &[T], s2t: &[T], w: usize) -> (usize, usize) {
    const WORD: usize = 64;
    const STACK_WORDS: usize = 16;
    let len2 = s2t.len();
    let words1 = s1t.len().div_ceil(WORD);
    let words2 = len2.div_ceil(WORD);

    let peq = T::peqn(s2t, words2);

    let mut stack1 = [0u64; STACK_WORDS];
    let mut heap1;
    let matched1: &mut [u64] = if words1 <= STACK_WORDS {
        &mut stack1[..words1]
    } else {
        heap1 = vec![0u64; words1];
        &mut heap1
    };
    let mut stack2 = [0u64; STACK_WORDS];
    let mut heap2;
    let matched2: &mut [u64] = if words2 <= STACK_WORDS {
        &mut stack2[..words2]
    } else {
        heap2 = vec![0u64; words2];
        &mut heap2
    };
    let mut m = 0usize;

    // The last word is partial whenever `len2` is not a multiple of 64; its
    // out-of-range high bits are never set, so "fully matched" has to be
    // tested against the valid-position mask, not against `u64::MAX`.
    let tail = len2 % WORD;
    let last_full = if tail == 0 { !0u64 } else { mask_lsb(tail) };
    let mut head = 0usize;

    for (i, &c1) in s1t.iter().enumerate() {
        let lo = i.saturating_sub(w);
        let hi = (i + w + 1).min(len2);
        if lo >= hi {
            continue;
        }
        while head < words2 {
            let saturated = if head + 1 == words2 { last_full } else { !0u64 };
            if matched2[head] != saturated {
                break;
            }
            head += 1;
        }
        if head == words2 {
            break;
        }
        // Absent from the pattern entirely: no window can hold a match.
        let Some(row) = T::peqn_row(&peq, c1) else {
            continue;
        };
        let lo_w = lo / WORD;
        let hi_w = (hi - 1) / WORD;
        if head > hi_w {
            continue;
        }
        for wi in lo_w.max(head)..=hi_w {
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

/// Jaro–Winkler similarity: [`jaro`], boosted for a shared prefix of up to
/// four units.
///
/// Winkler, W. E. (1990), *String comparator metrics and enhanced decision
/// rules in the Fellegi–Sunter model of record linkage*, ASA Proceedings of
/// the Section on Survey Research Methods, 354–359:
///
/// ```text
/// sim_w = sim_j + l * p * (1 - sim_j)
/// ```
///
/// where `sim_j` is [`jaro`], `l = min(4, common prefix length in scalars)`
/// and `p = 0.1`. This is a **similarity**, not a distance: higher is closer,
/// on the terms in
/// [*What the similarities guarantee*](crate#what-the-similarities-guarantee).
/// [*Jaro or Jaro–Winkler*](crate#jaro-or-jarowinkler) is the choice between
/// this function and the unboosted one.
///
/// # Winkler's `p` is fixed at 0.1
///
/// The boost uses `p = 0.1`, Winkler's own value, applied
/// **unconditionally** — Winkler's later "only when `sim_j > 0.7`" variant
/// introduces a discontinuity and is not implemented here. `p` is not
/// configurable: `p > 0.25` would make `l · p > 1` and let the boost carry the
/// score above `1.0`, breaking the range guarantee, so exposing it safely
/// would mean shipping a validated newtype whose only job is to exist. If it
/// ever returns it arrives as a separately named function.
///
/// Rearranged, `sim_w = (1 − l·p) · sim_j + l·p`: an affine interpolation
/// between `sim_j` and `1.0` with weight `l·p <= 0.4`. The result therefore
/// never leaves the closed interval `[sim_j, 1]`, and no clamp is applied or
/// needed. `jaro_winkler(x, x) == 1.0` exactly — [`jaro`] returns
/// exactly `1.0`, so the boost term is `l * 0.1 * 0.0` — and it reaches that
/// through the formula, with no equality short-circuit to make the two
/// functions disagree about their own identity element.
///
/// # Examples
///
/// ```
/// use verbora_distance::jaro_winkler;
///
/// // Winkler's worked examples: 17/18 + 3·0.1·(1/18), and so on.
/// assert!((jaro_winkler("MARTHA", "MARHTA") - 0.9611111111111111).abs() < 1e-12);
/// assert!((jaro_winkler("DIXON", "DICKSONX") - 0.8133333333333332).abs() < 1e-12);
/// assert!((jaro_winkler("DWAYNE", "DUANE") - 0.84).abs() < 1e-12);
///
/// assert_eq!(jaro_winkler("abc", "abc"), 1.0);
/// assert_eq!(jaro_winkler("", ""), 1.0);
/// assert_eq!(jaro_winkler("a", "b"), 0.0);
/// ```
///
/// Case is significant, because no metric in this crate rewrites its inputs.
/// Fold once, at the call site:
///
/// ```
/// use verbora_distance::jaro_winkler;
///
/// let (a, b) = ("A", "a");
/// assert_eq!(jaro_winkler(a, b), 0.0); // different units, no shared prefix
/// assert_eq!(jaro_winkler(&a.to_lowercase(), &b.to_lowercase()), 1.0);
/// ```
///
/// # Allocation
///
/// Exactly [`jaro`]'s — the prefix scan is in-place, and both the score and
/// the prefix are computed from a single scalar decode of the operands.
#[must_use]
pub fn jaro_winkler(s1: &str, s2: &str) -> f64 {
    dispatch(s1, s2, |ops| match ops {
        Operands::Bytes(a, b) => jaro_winkler_generic(a, b),
        Operands::Units(a, b) => jaro_winkler_generic(a, b),
    })
}

/// Jaro–Winkler over one consistent unit representation.
///
/// Both halves share the caller's single decode: [`jaro_winkler`] dispatches
/// once and computes the score and the prefix from the same slices, rather
/// than decoding the operands a second time for the prefix scan.
fn jaro_winkler_generic<T: BitPeq>(s1: &[T], s2: &[T]) -> f64 {
    /// Winkler's prefix scale. Fixed, not configurable — see the module
    /// documentation and `docs/design/distance-contract.md` §4.5.
    const P: f64 = 0.1;

    let dj = jaro_generic(s1, s2);
    let l = winkler_prefix_len(s1, s2);
    // Left to right in exactly this grouping: `dj + ((l * P) * (1 - dj))`.
    dj + (l as f64) * P * (1.0 - dj)
}

/// Winkler's `l`: the length of the operands' common prefix, in units,
/// capped at 4.
///
/// It is a *length*, so it is bounded by both operands — `("ab", "ab")` gives
/// `2`, not `4`. The cap is Winkler's; the bound is arithmetic.
fn winkler_prefix_len<T: Unit>(s1: &[T], s2: &[T]) -> usize {
    common_prefix_len(s1, s2).min(4)
}

/// [`jaro_winkler`], fanned out across a `rayon` thread pool. Requires the
/// `parallel` feature.
///
/// # Why this exists
///
/// `jaro_winkler` is a pure function over two borrowed `&str`s with no shared
/// state, so scoring many independent pairs is embarrassingly parallel with
/// zero coordination cost between pairs. This function is exactly
/// `pairs.par_iter().map(|(a, b)| jaro_winkler(a, b)).collect()` — a thin
/// fan-out over the existing sequential primitive, not a second
/// implementation of it. The windowed-match pass, the transposition pass and
/// the stack-vs-heap match-flag decision inside `jaro`/`jaro_winkler` are
/// untouched; if you need plain [`jaro`] in parallel, apply the same
/// `par_iter().map(...)` pattern at your own call site (see
/// `site/performance/parallelism.md`).
///
/// # When to reach for it vs. the sequential loop
///
/// `jaro_winkler` is cheap even at 1024 characters, while a `rayon` task
/// costs on the order of a microsecond to schedule
/// (`site/performance/parallelism.md`) — so a plain
/// `pairs.iter().map(|(a, b)| jaro_winkler(a, b)).collect()` loop wins
/// outright for short pairs or small batches. This function pays off once
/// the pairs are long enough that the per-pair work dominates scheduling, or
/// the batch is large; confirm it on your own data.
///
/// `UNMEASURED`: the crossover table this documentation used to carry was
/// measured against a different signature (an `Options` argument that no
/// longer exists) and before the scalar unit landed, so it is removed rather
/// than adjusted. It is restored only from a fresh full-precision run of
/// `cargo bench -p verbora-distance --features parallel -- par_jaro_winkler`
/// (`docs/design/distance-contract.md` §7, item 8).
///
/// # Allocation behaviour
///
/// One `Vec<f64>` sized to `pairs.len()` for the output, plus whatever
/// [`jaro_winkler`] itself allocates per pair (see its own `Allocation`
/// section). No additional buffering, no locking, no per-call thread-pool
/// construction — this uses whichever global `rayon` pool is already
/// installed (or `rayon`'s default one), so pool configuration remains the
/// caller's responsibility, not this crate's.
///
/// # Order and errors
///
/// Output order matches input order — `results[i] == jaro_winkler(pairs[i].0,
/// pairs[i].1)` — via `rayon`'s order-preserving `map` + `collect`.
/// `jaro_winkler` never errors, so every element is a plain `f64`.
#[cfg(feature = "parallel")]
#[must_use]
pub fn par_jaro_winkler_batch(pairs: &[(&str, &str)]) -> Vec<f64> {
    use rayon::prelude::*;
    pairs.par_iter().map(|(a, b)| jaro_winkler(a, b)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Jaro's published worked examples, each with the arithmetic of the
    /// definition spelled out rather than a recorded constant.
    ///
    /// - `("MARTHA", "MARHTA")`: `n1 = n2 = 6`, `w = ⌊6/2⌋ − 1 = 2`. M, A, R
    ///   match in place, T and H cross inside the window, the trailing A
    ///   matches — `m = 6` with two matched units in a different relative
    ///   order, so raw transpositions `= 2` and `t = 1`.
    ///   `(6/6 + 6/6 + 5/6)/3 = 17/18`.
    /// - `("DIXON", "DICKSONX")`: `n1 = 5`, `n2 = 8`, `w = ⌊8/2⌋ − 1 = 3`.
    ///   D, I, O, N match; the trailing X of `DIXON` sits at `i = 2` and its
    ///   only partner at `j = 7`, `|2 − 7| = 5 > 3`, so it is unreachable.
    ///   `m = 4`, `t = 0`. `(4/5 + 4/8 + 4/4)/3`.
    /// - `("DWAYNE", "DUANE")`: `m = 4` (D, A, N, E), `t = 0`.
    ///   `(4/6 + 4/5 + 4/4)/3`.
    ///
    /// Winkler's boost on top is `sim_j + l · 0.1 · (1 − sim_j)` with
    /// `l = min(4, common prefix)`: 3 for `MAR`, 2 for `DI`, 1 for `D`.
    #[test]
    fn published_worked_examples() {
        let martha = ((6.0f64 / 6.0) + (6.0 / 6.0) + ((6.0 - 1.0) / 6.0)) / 3.0;
        assert_eq!(jaro("MARTHA", "MARHTA").to_bits(), martha.to_bits());
        assert!((martha - 17.0 / 18.0).abs() < 1e-15);
        assert_eq!(
            jaro_winkler("MARTHA", "MARHTA").to_bits(),
            (martha + 3.0 * 0.1 * (1.0 - martha)).to_bits()
        );
        assert!((jaro_winkler("MARTHA", "MARHTA") - 0.9611111111111111).abs() < 1e-12);

        let dixon = ((4.0f64 / 5.0) + (4.0 / 8.0) + ((4.0 - 0.0) / 4.0)) / 3.0;
        assert_eq!(jaro("DIXON", "DICKSONX").to_bits(), dixon.to_bits());
        assert!((dixon - 2.3 / 3.0).abs() < 1e-15);
        assert_eq!(
            jaro_winkler("DIXON", "DICKSONX").to_bits(),
            (dixon + 2.0 * 0.1 * (1.0 - dixon)).to_bits()
        );
        assert!((jaro_winkler("DIXON", "DICKSONX") - 0.8133333333333332).abs() < 1e-12);

        let dwayne = ((4.0f64 / 6.0) + (4.0 / 5.0) + ((4.0 - 0.0) / 4.0)) / 3.0;
        assert_eq!(jaro("DWAYNE", "DUANE").to_bits(), dwayne.to_bits());
        assert_eq!(
            jaro_winkler("DWAYNE", "DUANE").to_bits(),
            (dwayne + 1.0 * 0.1 * (1.0 - dwayne)).to_bits()
        );
        assert!((jaro_winkler("DWAYNE", "DUANE") - 0.84).abs() < 1e-12);

        // `("aaaa", "aa")`: `w = ⌊4/2⌋ − 1 = 1`. The greedy pass claims
        // `j = 0` for `i = 0` and `j = 1` for `i = 1`; `i = 2` sees only
        // `j ∈ {1, 2, 3} ∩ [0, 2)` = `{1}`, already taken, and `i = 3` sees
        // nothing in range at all. `m = 2`, `t = 0`.
        let aaaa = ((2.0f64 / 4.0) + (2.0 / 2.0) + ((2.0 - 0.0) / 2.0)) / 3.0;
        assert_eq!(jaro("aaaa", "aa").to_bits(), aaaa.to_bits());
    }

    /// Guarantee 3 of the module contract, bitwise, across every operand
    /// class: empty, one unit, the short greedy loop, an astral scalar, a
    /// multi-scalar grapheme cluster, and — in both an ASCII and a non-ASCII
    /// spelling — an operand past `JARO_SCALAR_MAX`, where the common-prefix
    /// reduction consumes both operands entirely and the bit kernels are
    /// never entered. That last row is what makes deleting the equality
    /// short-circuit free: the fast exit it used to provide is structural.
    /// `jaro_winkler` must reach `1.0` through the formula.
    #[test]
    fn identity_is_exactly_one() {
        for x in [
            "",
            "a",
            "ab",
            "abcd",
            "abcdefgh",
            "aaaa",
            "😀",
            "क्षि",
            "abcdefghijklmnopqrst", // 20 units: past JARO_SCALAR_MAX
            "Ααββγγδδεεζζηηθθιικκ", // the same, non-ASCII
        ] {
            assert_eq!(jaro(x, x).to_bits(), 1.0f64.to_bits(), "jaro({x:?}, {x:?})");
            assert_eq!(
                jaro_winkler(x, x).to_bits(),
                1.0f64.to_bits(),
                "jaro_winkler({x:?}, {x:?})"
            );
        }
    }

    /// The degenerate table published on the crate root, asserted directly.
    #[test]
    fn degenerate_table() {
        for (a, b, want) in [
            ("", "", 1.0f64),
            ("", "a", 0.0),
            ("a", "", 0.0),
            ("a", "a", 1.0),
            ("a", "b", 0.0),
        ] {
            assert_eq!(jaro(a, b).to_bits(), want.to_bits(), "jaro({a:?}, {b:?})");
            assert_eq!(
                jaro_winkler(a, b).to_bits(),
                want.to_bits(),
                "jaro_winkler({a:?}, {b:?})"
            );
        }
    }

    #[test]
    fn empty_operands() {
        // Both empty is the identity case, not the disjoint one.
        assert_eq!(jaro("", "").to_bits(), 1.0f64.to_bits());
        assert_eq!(jaro_winkler("", "").to_bits(), 1.0f64.to_bits());
        // Exactly one empty: nothing to match, so `m == 0`.
        assert_eq!(jaro("", "a"), 0.0);
        assert_eq!(jaro("a", ""), 0.0);
        assert_eq!(jaro("", "abcdef"), 0.0);
        assert_eq!(jaro("abcdef", ""), 0.0);
        assert_eq!(jaro_winkler("", "abc"), 0.0);
        assert_eq!(jaro_winkler("abc", ""), 0.0);
    }

    #[test]
    fn single_char_window_is_clamped_at_zero() {
        // `floor(1/2) - 1` is -1; the window is clamped at 0, so the one
        // candidate pair at displacement 0 is still considered.
        // Distinct units: no match, m = 0, similarity 0.
        assert_eq!(jaro("a", "b"), 0.0);
        assert_eq!(jaro_winkler("a", "b"), 0.0);
        // Equal units: m = 1, t = 0, (1/1 + 1/1 + 1/1)/3 = 1.0 exactly.
        assert_eq!(jaro("x", "x").to_bits(), 1.0f64.to_bits());
        // Under the scalar unit an astral character is *one* unit, so this
        // is the same class — and the clamp is what keeps it at 1.0.
        assert_eq!(jaro("😀", "😀").to_bits(), 1.0f64.to_bits());
        assert_eq!(jaro("😀", "😁"), 0.0);
        // The contrast that makes the unit change worth the churn
        // (`docs/design/distance-contract.md` §2.5): under the UTF-16 unit
        // these two calls returned the *same* number, because two emoji
        // sharing a high surrogate were indistinguishable from two CJK
        // words sharing a real character. Now the emoji share nothing
        // (0.0) while the CJK pair shares one of two characters:
        // n1 = n2 = 2, w = 0, m = 1, t = 0 -> (1/2 + 1/2 + 1/1)/3.
        let cjk = ((1.0f64 / 2.0) + (1.0 / 2.0) + ((1.0 - 0.0) / 1.0)) / 3.0;
        assert_eq!(jaro("北京", "南京").to_bits(), cjk.to_bits());
        assert!((cjk - 0.666_666_666_666_666_6).abs() < 1e-12);
        // Invisible for `max >= 2`, where `floor(max/2) - 1 >= 0` already.
        // `jaro("aaaa","aa")`: w = 1, the greedy pass claims j = 0 then
        // j = 1 and the remaining i have no free j in window, so m = 2,
        // t = 0 and the score is (2/4 + 2/2 + 2/2)/3.
        let expected = ((2.0f64 / 4.0) + (2.0 / 2.0) + ((2.0 - 0.0) / 2.0)) / 3.0;
        assert_eq!(jaro("aaaa", "aa").to_bits(), expected.to_bits());
    }

    #[test]
    fn case_is_significant_and_folding_is_the_callers() {
        // No metric in this crate rewrites its inputs, so `"A"` and `"a"` are
        // simply different units: no match, no shared prefix, no boost.
        assert_eq!(jaro_winkler("A", "a"), 0.0);
        assert_eq!(jaro("MARTHA", "martha"), 0.0);
        assert!(jaro_winkler("MARTHA", "martha") < 1.0);
        // Folded at the call site, the pair is identical and scores exactly
        // 1.0 through the formula.
        for (a, b) in [("A", "a"), ("X", "x"), ("AB", "ab"), ("MARTHA", "martha")] {
            let (fa, fb) = (a.to_lowercase(), b.to_lowercase());
            assert_eq!(jaro(&fa, &fb).to_bits(), 1.0f64.to_bits());
            assert_eq!(jaro_winkler(&fa, &fb).to_bits(), 1.0f64.to_bits());
        }
    }

    #[test]
    fn winkler_prefix_length_is_a_capped_length() {
        // `l` is the common prefix length, bounded by both operands and
        // capped at 4. It is *not* a counter that keeps climbing once both
        // operands are exhausted: `("ab", "ab")` shares two units, not four.
        assert_eq!(winkler_prefix_len(b"", b""), 0);
        assert_eq!(winkler_prefix_len(b"a", b"a"), 1);
        assert_eq!(winkler_prefix_len(b"ab", b"ab"), 2);
        assert_eq!(winkler_prefix_len(b"abc", b"abc"), 3);
        assert_eq!(winkler_prefix_len(b"abcd", b"abcd"), 4);
        assert_eq!(winkler_prefix_len(b"abcdefg", b"abcdefg"), 4); // the cap
        assert_eq!(winkler_prefix_len(b"abz", b"abx"), 2);
        assert_eq!(winkler_prefix_len(b"a", b"ab"), 1);
        assert_eq!(winkler_prefix_len(b"", b"abc"), 0);
        // Counted in scalars, not bytes: one astral character is one unit.
        let astral: Vec<char> = "😀😁x".chars().collect();
        let other: Vec<char> = "😀😁y".chars().collect();
        assert_eq!(winkler_prefix_len(&astral, &other), 2);
    }

    #[test]
    fn boost_is_zero_when_prefixes_differ() {
        // `l == 0` makes the boost term `0.0 * (1 - dj)`, so Jaro–Winkler is
        // Jaro bit for bit — no drift introduced by the multiply-add.
        assert_eq!(
            jaro_winkler("abcd", "zbcd").to_bits(),
            jaro("abcd", "zbcd").to_bits()
        );
        assert_eq!(
            jaro_winkler("MARTHA", "XARTHA").to_bits(),
            jaro("MARTHA", "XARTHA").to_bits()
        );
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
        if len1 == 0 && len2 == 0 {
            return 1.0;
        }
        if len1 == 0 || len2 == 0 {
            return 0.0;
        }
        let max_len = len1.max(len2);
        let w = (max_len / 2).saturating_sub(1);
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
        if len1 == 0 && len2 == 0 {
            return 1.0;
        }
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
        // covers the ASCII/scalar dispatch).
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
        // `docs/design/distance-contract.md` §6.4's evaluation-order
        // fixture: m = 3, raw transpositions = 3 (odd) => t = 1.5. An
        // integer `raw / 2` halving would drop the .5 and produce a
        // different value here, so this pins the fractional halving --
        // and the three-division grouping -- through every kernel.
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
    fn jaro_bit_kernels_agree_on_scalar_input() {
        // The `char` monomorphization (FxHashMap-backed Peq), including
        // astral input, where one scalar is one unit.
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
            let ua: Vec<char> = a.chars().collect();
            let ub: Vec<char> = b.chars().collect();
            let scalar = jaro_scalar_full(&ua[..], &ub[..]);
            assert_eq!(
                jaro(&a, &b).to_bits(),
                scalar.to_bits(),
                "scalar dispatch mismatch {len1}x{len2}"
            );
            let block = jaro_forced(&ua[..], &ub[..], jaro_bit_block::<char>);
            assert_eq!(scalar.to_bits(), block.to_bits(), "scalar block mismatch");
        }
        let a = "\u{1F600}\u{1F601}".repeat(20);
        let b = "\u{1F601}\u{1F600}".repeat(20);
        let ua: Vec<char> = a.chars().collect();
        let ub: Vec<char> = b.chars().collect();
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
        let w = (max_len / 2).saturating_sub(1);
        let len1t = s1.len().min(s2.len() + w);
        let len2t = s2.len().min(s1.len() + w);
        if len1t <= 64 && len2t <= 64 {
            let word = jaro_forced(s1, s2, jaro_bit_word::<u8>);
            assert_eq!(scalar.to_bits(), word.to_bits(), "word mismatch: {context}");
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
    fn jaro_bit_word_char_direct_agrees() {
        // `jaro_bit_word::<char>` is only reachable through the public
        // dispatch otherwise; pin the monomorphization directly, over an
        // alphabet mixing BMP and astral scalars.
        let mut rng = SplitMix64(0x0016_D1EC_7000_0001);
        const UNITS: &[char] = &['\u{430}', '\u{431}', '\u{432}', '😀', '𝕳', '\u{4E2D}'];
        for &(len1, len2) in &[
            (17usize, 20usize),
            (25, 25),
            (33, 60),
            (40, 64),
            (64, 64),
            (60, 17),
        ] {
            let s1: Vec<char> = (0..len1)
                .map(|_| UNITS[rng.next_range(UNITS.len())])
                .collect();
            let s2: Vec<char> = (0..len2)
                .map(|_| UNITS[rng.next_range(UNITS.len())])
                .collect();
            let scalar = jaro_scalar_full(&s1[..], &s2[..]);
            let max_len = len1.max(len2);
            let w = (max_len / 2).saturating_sub(1);
            let len1t = len1.min(len2 + w);
            let len2t = len2.min(len1 + w);
            assert!(len1t <= 64 && len2t <= 64, "test shape must fit one word");
            let word = jaro_forced(&s1[..], &s2[..], jaro_bit_word::<char>);
            let block = jaro_forced(&s1[..], &s2[..], jaro_bit_block::<char>);
            assert_eq!(scalar.to_bits(), word.to_bits(), "char word {len1}x{len2}");
            assert_eq!(
                scalar.to_bits(),
                block.to_bits(),
                "char block {len1}x{len2}"
            );
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

        // The `char` monomorphization at the same scale, via public dispatch
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
            let ua: Vec<char> = a.chars().collect();
            let ub: Vec<char> = b.chars().collect();
            let scalar = jaro_scalar_full(&ua[..], &ub[..]);
            let block = jaro_forced(&ua[..], &ub[..], jaro_bit_block::<char>);
            assert_eq!(
                scalar.to_bits(),
                block.to_bits(),
                "char round {round} ({len1}x{len2})"
            );
            assert_eq!(jaro(&a, &b).to_bits(), scalar.to_bits());
        }
    }

    // -- Common-prefix reduction --------------------------------------------

    /// `jaro_generic`'s non-scalar branch reproduced exactly — length trim,
    /// prefix reduction, kernel, similarity formula — but FORCED, so the
    /// reduction can be exercised on inputs the production size dispatch
    /// would hand to `jaro_scalar` instead. Everything here mirrors the
    /// real code; only the `max_len <= JARO_SCALAR_MAX` gate is bypassed.
    fn jaro_prefix_reduced<T: BitPeq>(s1: &[T], s2: &[T], kernel: Kernel<T>) -> f64 {
        let len1 = s1.len();
        let len2 = s2.len();
        if len1 == 0 && len2 == 0 {
            return 1.0;
        }
        if len1 == 0 || len2 == 0 {
            return 0.0;
        }
        let max_len = len1.max(len2);
        let w = (max_len / 2).saturating_sub(1);
        let len1t = len1.min(len2 + w);
        let len2t = len2.min(len1 + w);
        let s1t = &s1[..len1t];
        let s2t = &s2[..len2t];
        let prefix = common_prefix_len(s1t, s2t);
        let (a, b) = (&s1t[prefix..], &s2t[prefix..]);
        let (m_rest, t_raw) = if a.is_empty() || b.is_empty() {
            (0, 0)
        } else {
            kernel(a, b, w)
        };
        let m = m_rest + prefix;
        if m == 0 {
            return 0.0;
        }
        let t = t_raw as f64 / 2.0;
        let m = m as f64;
        ((m / len1 as f64) + (m / len2 as f64) + ((m - t) / m)) / 3.0
    }

    #[test]
    fn jaro_prefix_reduction_agrees_with_the_scalar_loop_exhaustively() {
        // The reduction's correctness-defining test: every pair over a
        // two-letter alphabet up to length 8 and a three-letter alphabet up
        // to length 6, scalar loop versus prefix-reduced word and block
        // kernels, compared bitwise. Long shared prefixes made of repeated
        // characters are dense in these spaces, which is exactly where the
        // "the greedy pass matches the prefix in place" claim would fail if
        // it were wrong.
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
                    assert_eq!(
                        scalar.to_bits(),
                        jaro_prefix_reduced(s1, s2, jaro_bit_word::<u8>).to_bits(),
                        "word mismatch for {s1:?} vs {s2:?}"
                    );
                    assert_eq!(
                        scalar.to_bits(),
                        jaro_prefix_reduced(s1, s2, jaro_bit_block::<u8>).to_bits(),
                        "block mismatch for {s1:?} vs {s2:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn jaro_common_prefix_is_matched_in_place() {
        // The induction the reduction rests on, checked directly against
        // the scalar loop's own match flags rather than through the
        // similarity value: for every `i` below the common prefix length,
        // the greedy pass must have taken position `i` of `s2` for position
        // `i` of `s1` — never an earlier or later one. Repeated characters
        // are what would break this if the "leftmost free" reasoning were
        // wrong, so the operands are built from tiny alphabets.
        fn matched_pairs(s1: &[u8], s2: &[u8]) -> Vec<(usize, usize)> {
            // The contract's window, written out independently:
            // `max(0, floor(max(n1, n2) / 2) - 1)`.
            let w = (s1.len().max(s2.len()) / 2).saturating_sub(1);
            let mut taken = vec![false; s2.len()];
            let mut pairs = Vec::new();
            for (i, &c1) in s1.iter().enumerate() {
                let start = i.saturating_sub(w);
                let end = (i + w + 1).min(s2.len());
                for k in start..end {
                    if taken[k] || c1 != s2[k] {
                        continue;
                    }
                    taken[k] = true;
                    pairs.push((i, k));
                    break;
                }
            }
            pairs
        }

        let mut rng = SplitMix64(0x9111_0000_5EED_1234);
        const ALPHABETS: [&[u8]; 3] = [b"a", b"ab", b"aab"];
        for round in 0..500 {
            let alphabet = ALPHABETS[round % ALPHABETS.len()];
            let shared = rng.next_range(40);
            let prefix: Vec<u8> = (0..shared)
                .map(|_| alphabet[rng.next_range(alphabet.len())])
                .collect();
            let mut s1 = prefix.clone();
            let mut s2 = prefix.clone();
            for _ in 0..1 + rng.next_range(30) {
                s1.push(alphabet[rng.next_range(alphabet.len())]);
            }
            for _ in 0..1 + rng.next_range(30) {
                s2.push(alphabet[rng.next_range(alphabet.len())]);
            }
            // `common_prefix_len` may exceed `shared` when the tails happen
            // to continue agreeing; use the real value, not the seed.
            let p = common_prefix_len(&s1, &s2);
            let pairs = matched_pairs(&s1, &s2);
            for i in 0..p {
                assert_eq!(
                    pairs.get(i),
                    Some(&(i, i)),
                    "prefix position {i} of {s1:?} vs {s2:?} was not matched in place"
                );
            }
        }
    }

    #[test]
    fn jaro_common_suffix_trimming_is_invalid() {
        // The counterexample that keeps the mirrored reduction out. `"ab"`
        // and `"baba"` share the suffix `"b"`…`"ab"` under a naive
        // right-to-left scan; the untrimmed greedy pass finds 2 matches and
        // 2 raw transpositions, while trimming the shared tail finds the
        // same 2 matches with 0 transpositions — a different similarity.
        assert_eq!(jaro_scalar(b"ab", b"baba"), (2, 2));
        // The mirrored reduction, spelled out: cut the one shared trailing
        // unit off each side, run the kernel on the residue with the
        // ORIGINAL window (exactly what the prefix reduction does), and add
        // the cut unit back as a match. The match count survives; both
        // transpositions do not.
        let w = 4 / 2 - 1;
        let (m_rest, t_rest) = jaro_bit_word(b"a", b"bab", w);
        assert_eq!((m_rest + 1, t_rest), (2, 0));
        // The production path answers the untrimmed value, bitwise.
        assert_eq!(
            jaro("ab", "baba").to_bits(),
            jaro_scalar_full(b"ab", b"baba").to_bits()
        );

        // The same asymmetry at a size that actually routes through the bit
        // kernels: a shared tail whose characters are also available
        // earlier, so the greedy pass consumes them out of the tail.
        let s1 = format!("{}ab", "z".repeat(40));
        let s2 = format!("{}baba", "z".repeat(40));
        assert_eq!(
            jaro(&s1, &s2).to_bits(),
            jaro_scalar_full(s1.as_bytes(), s2.as_bytes()).to_bits()
        );
    }

    #[test]
    fn jaro_prefix_reduction_agrees_on_near_identical_corpora_at_scale() {
        // The competitive grid's own near-identical shape at the sizes
        // where the loss was measured, plus deletions (which make the
        // operands differ in length, so the length trim and the prefix
        // reduction interact) and Cyrillic/astral operands for the `char`
        // monomorphization. `jaro_winkler` is checked alongside `jaro`
        // because its boost multiplies `1 - dj`, so any drift in `dj`
        // shows up amplified there.
        fn substitute_one(s: &str, at: usize) -> String {
            let chars: Vec<char> = s.chars().collect();
            let i = at % chars.len();
            let replacement = if chars[i] == 'Z' { 'Q' } else { 'Z' };
            chars
                .iter()
                .enumerate()
                .map(|(k, &c)| if k == i { replacement } else { c })
                .collect()
        }
        fn delete_one(s: &str, at: usize) -> String {
            let chars: Vec<char> = s.chars().collect();
            let i = at % chars.len();
            chars
                .iter()
                .enumerate()
                .filter_map(|(k, &c)| (k != i).then_some(c))
                .collect()
        }

        let mut rng = SplitMix64(0x3A20_0BAD_0F00_D001);
        const ASCII: &[u8] = b"abcdefghijklmnopqrstuvwxyz";
        const BMP: &[char] = &['\u{430}', '\u{431}', '\u{432}', '\u{4e2d}', '\u{f1}'];
        const ASTRAL: &[char] = &['\u{1F600}', '\u{1D518}', '\u{1F389}'];
        for &len in &[17usize, 33, 64, 65, 128, 129, 256, 1024] {
            let ascii: String = (0..len)
                .map(|_| ASCII[rng.next_range(ASCII.len())] as char)
                .collect();
            let bmp: String = (0..len).map(|_| BMP[rng.next_range(BMP.len())]).collect();
            let astral: String = (0..len / 2)
                .map(|_| ASTRAL[rng.next_range(ASTRAL.len())])
                .collect();
            for base in [ascii, bmp, astral] {
                let units = base.chars().count();
                for &at in &[0usize, 1, units / 2, units - 1] {
                    for variant in [substitute_one(&base, at), delete_one(&base, at)] {
                        let ua: Vec<char> = base.chars().collect();
                        let ub: Vec<char> = variant.chars().collect();
                        let want = jaro_scalar_full(&ua, &ub);
                        assert_eq!(
                            jaro(&base, &variant).to_bits(),
                            want.to_bits(),
                            "len={len} at={at}"
                        );
                        let want_rev = jaro_scalar_full(&ub, &ua);
                        assert_eq!(
                            jaro(&variant, &base).to_bits(),
                            want_rev.to_bits(),
                            "reversed len={len} at={at}"
                        );
                        // Jaro-Winkler on top of the same pair.
                        let boost = winkler_prefix_len(&ua, &ub) as f64;
                        let expected = want + boost * 0.1 * (1.0 - want);
                        assert_eq!(
                            jaro_winkler(&base, &variant).to_bits(),
                            expected.to_bits(),
                            "winkler len={len} at={at}"
                        );
                    }
                }
                // Identical operands: the reduction consumes everything and
                // the kernels never run.
                assert_eq!(jaro(&base, &base), 1.0);
            }
        }
    }

    #[test]
    fn jaro_block_head_cursor_saturation_and_partial_last_word() {
        // The saturated-head cursor's own edge cases. (a) `s2` fully
        // matched well before `s1` is exhausted, which is the `head ==
        // words2` early stop; (b) a partial last word, where "fully
        // matched" is the valid-position mask rather than `u64::MAX` —
        // getting that wrong would stop the scan a word early and silently
        // drop matches; (c) a permanently unmatchable hole low in `s2` that
        // must NOT let the cursor advance past it. All against the scalar
        // loop, bitwise.
        for &len2 in &[65usize, 100, 128, 129, 192, 200, 257] {
            // (a)+(b): s2 is a short run of 'a's that s1 saturates early.
            let s2: Vec<u8> = vec![b'a'; len2];
            for &len1 in &[len2, len2 * 2, len2 * 3] {
                let s1: Vec<u8> = vec![b'a'; len1];
                assert_all_paths_agree(&s1, &s2, &format!("saturating {len1}x{len2}"));
            }
            // (c): position 0 of s2 holds a unit that appears nowhere in
            // s1, so word 0 can never saturate and the cursor must stay put.
            let mut holed = vec![b'a'; len2];
            holed[0] = b'q';
            let s1: Vec<u8> = vec![b'a'; len2];
            assert_all_paths_agree(&s1, &holed, &format!("hole at 0, len {len2}"));
            // A hole in the middle of an otherwise saturated prefix.
            let mut mid_hole = vec![b'a'; len2];
            mid_hole[len2 / 2] = b'q';
            assert_all_paths_agree(&s1, &mid_hole, &format!("hole at mid, len {len2}"));
        }
        // A unit absent from the pattern entirely (the `peqn_row` `None`
        // arm, which no longer falls back to a borrowed all-zero row).
        let all_a = [b'a'; 200];
        assert_all_paths_agree(&[b'z'; 200], &all_a, "wholly disjoint alphabets");
        let mut sparse = [b'z'; 200];
        sparse[199] = b'a';
        assert_all_paths_agree(&sparse, &all_a, "single late overlap");
    }

    // -- The four guarantees, as properties ---------------------------------

    /// Asserts the four published guarantees on one pair: totality (finite),
    /// range (`0..=1`), symmetry (bitwise under argument swap) and strict
    /// identity (`== 1.0` exactly when the operands are equal), for both
    /// [`jaro`] and [`jaro_winkler`].
    fn assert_guarantees(a: &str, b: &str) {
        for (name, f) in [
            ("jaro", jaro as fn(&str, &str) -> f64),
            ("jaro_winkler", jaro_winkler as fn(&str, &str) -> f64),
        ] {
            let v = f(a, b);
            assert!(v.is_finite(), "{name}({a:?}, {b:?}) = {v} is not finite");
            assert!(
                (0.0..=1.0).contains(&v),
                "{name}({a:?}, {b:?}) = {v} is outside [0, 1]"
            );
            assert_eq!(
                v.to_bits(),
                f(b, a).to_bits(),
                "{name} is not symmetric on ({a:?}, {b:?})"
            );
            assert_eq!(
                v == 1.0,
                a == b,
                "{name}({a:?}, {b:?}) = {v}, but the operands are {}equal",
                if a == b { "" } else { "not " }
            );
        }
    }

    #[test]
    fn guarantees_hold_over_a_large_randomized_corpus() {
        // Range, totality, symmetry and strict identity over 51,000+
        // randomized pairs, lengths 0..=300, across six alphabets: a
        // single repeated unit (which makes equal-length pairs *equal*, so
        // the identity half of the property is exercised constantly), two
        // and three letters (dense repeats, hardest on the greedy
        // tie-breaking), the Latin alphabet, a BMP alphabet and an astral
        // one — the last two also driving the `char` monomorphization.
        const ASCII_ALPHABETS: [&[u8]; 4] = [b"a", b"ab", b"abc", b"abcdefghijklmnopqrstuvwxyz"];
        const BMP: &[char] = &['\u{430}', '\u{431}', '\u{4e2d}', '\u{f1}', '\u{3b1}'];
        const ASTRAL: &[char] = &['\u{1F600}', '\u{1D518}', '\u{1F389}', '\u{2070E}'];

        let mut rng = SplitMix64(0x0AC1_0AC1_2026_0819);
        let draw = |rng: &mut SplitMix64, alphabet: usize, len: usize| -> String {
            match alphabet {
                0..=3 => {
                    let a = ASCII_ALPHABETS[alphabet];
                    (0..len)
                        .map(|_| a[rng.next_range(a.len())] as char)
                        .collect()
                }
                4 => (0..len).map(|_| BMP[rng.next_range(BMP.len())]).collect(),
                _ => (0..len)
                    .map(|_| ASTRAL[rng.next_range(ASTRAL.len())])
                    .collect(),
            }
        };

        for round in 0..51_000u32 {
            let alphabet = (round % 6) as usize;
            // Lengths 0..=300, with both zero lengths drawn often enough
            // that the empty and half-empty classes are genuinely covered.
            let len1 = if round % 401 == 0 {
                0
            } else {
                rng.next_range(301)
            };
            let len2 = if round % 401 == 200 {
                0
            } else if round % 7 == 0 {
                // Heavily asymmetric pairs in a seventh of the rounds.
                rng.next_range(20)
            } else {
                rng.next_range(301)
            };
            let a = draw(&mut rng, alphabet, len1);
            let b = draw(&mut rng, alphabet, len2);
            assert_guarantees(&a, &b);
        }
    }

    #[test]
    fn identity_holds_only_at_equality_exhaustively() {
        // Every pair over `{a, b}` up to length 8 — 511 strings, 261,121
        // ordered pairs — checked for all four guarantees. This is the
        // clause `jaro(a, b) == 1.0 ⟹ a == b` at full coverage over a space
        // dense in near-misses: strings that differ in one position, in
        // length only, or by a transposition all appear here many times.
        fn enumerate(alphabet: &[u8], max_len: usize) -> Vec<String> {
            let mut out: Vec<String> = vec![String::new()];
            let mut frontier: Vec<String> = vec![String::new()];
            for _ in 0..max_len {
                let mut next = Vec::new();
                for s in &frontier {
                    for &c in alphabet {
                        let mut t = s.clone();
                        t.push(c as char);
                        next.push(t);
                    }
                }
                out.extend(next.iter().cloned());
                frontier = next;
            }
            out
        }

        let all = enumerate(b"ab", 8);
        assert_eq!(all.len(), 511, "2^9 - 1 strings of length 0..=8");
        for a in &all {
            for b in &all {
                assert_guarantees(a, b);
            }
        }
    }
}
