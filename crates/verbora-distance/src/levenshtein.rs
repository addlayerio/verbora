//! Levenshtein and Damerau–Levenshtein distance, ported from
//! The reference `levenshtein_distance`.
//!
//! Four public entry points mirror the four the reference exports: plain and
//! Damerau, each in "distance" and "search" flavours.
//!
//! # Performance shape
//!
//! The reference always materialises a full `(n+1) × (m+1)` matrix of heap-allocated
//! cell objects, each holding a cost and a parent coordinate — even when only the
//! final scalar is wanted. That is `O(nm)` allocations of `O(nm)` pointer-chased
//! objects.
//!
//! This port picks the cheapest structure that can answer the question asked:
//!
//! | Mode                              | Working set | Why |
//! |-----------------------------------|-------------|-----|
//! | distance, no Damerau, unit cost, 1–64-unit shorter operand | one-word Myers state plus Peq lookup | Myers' (1999) bit-parallel algorithm computes the same answer in O(n) bitwise ops instead of O(n·m) scalar cells — see `plain_levenshtein`'s own doc comment |
//! | distance, no Damerau, unit cost, 65+ unit shorter operand | multi-word Myers state plus packed Peq rows | Myers' state is carried across contiguous 64-bit blocks |
//! | distance, no Damerau (weighted fallback) | 1 row | each cell needs only `up`, `left`, `diag` |
//! | distance, restricted Damerau      | 3 rows      | transposition reaches back to row − 2 |
//! | distance, unrestricted Damerau    | full matrix | transposition reaches an arbitrary earlier row |
//! | search, no Damerau, unit cost     | per-column bit-vector deltas | the parent of every cell is a pure function of its neighbours' costs, and unit-cost cell costs are recoverable from Myers' `Pv`/`Mv` words — see `search_bits` |
//! | search (fallback)                 | full matrix | weighted costs have no bit-vector form, and unrestricted-Damerau parents depend on `last_row_map` state that cell costs alone cannot recover |
//!
//! The one- and three-row paths turn an `O(nm)` allocation into an `O(m)` one and
//! keep the whole working set in cache. The bit-vector path goes further for the
//! common case it covers — no per-cell allocation, and 64 dynamic-programming
//! rows represented by each machine word — closing most of the gap
//! `docs/PERFORMANCE_GAPS.md` entry 26
//! documents against `triple_accel`'s SIMD without needing `unsafe`, which this
//! workspace's `unsafe_code = "deny"` policy rules out by default.
//!
//! # Tie-breaking is observable
//!
//! The reference picks the cheapest predecessor with underscore's `_.min`, which
//! keeps the **first** candidate on ties (its comparison is a strict `<`). The
//! candidate order is: insert, delete, substitute, unrestricted-transpose,
//! restricted-transpose. Cost totals are unaffected by tie-breaking, but the
//! recorded *parent* is — and the parent chain determines the `offset` and
//! `substring` that search mode returns. The order is therefore reproduced
//! exactly.

use crate::units::{BitPeq, Operands, Unit, UnitMap, dispatch};
use rustc_hash::FxHashMap;

/// Options for the Levenshtein family.
///
/// Costs are `f64` because the reference accepts arbitrary the reference numbers here,
/// including fractions and zero, and callers do pass them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Options {
    /// Cost of inserting a symbol. Default 1.
    pub insertion_cost: f64,
    /// Cost of deleting a symbol. Default 1.
    pub deletion_cost: f64,
    /// Cost of substituting one symbol for another. Default 1.
    pub substitution_cost: f64,
    /// Cost of transposing two adjacent symbols. Default 1. Damerau variants only.
    pub transposition_cost: f64,
    /// When `true`, use the restricted (optimal string alignment) transposition
    /// rule, which forbids editing a substring between two transpositions.
    pub restricted: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            insertion_cost: 1.0,
            deletion_cost: 1.0,
            substitution_cost: 1.0,
            transposition_cost: 1.0,
            restricted: false,
        }
    }
}

/// The result of a search-mode call.
///
/// Mirrors the reference object `{ substring, distance, offset }`.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchResult {
    /// The best-matching substring of the target.
    pub substring: String,
    /// The edit distance to that substring.
    pub distance: f64,
    /// The substring's start offset in the target, in UTF-16 code units.
    ///
    /// Signed, and genuinely negative for some inputs: when the backtrace exits
    /// through column 0 the reference implementation reports `-1`, which callers
    /// can observe. It is reported verbatim rather than clamped.
    pub offset: isize,
}

/// Levenshtein distance between `source` and `target`.
pub fn levenshtein(source: &str, target: &str, opts: &Options) -> f64 {
    distance_impl(source, target, opts, false)
}

/// Damerau–Levenshtein distance between `source` and `target`.
pub fn damerau_levenshtein(source: &str, target: &str, opts: &Options) -> f64 {
    distance_impl(source, target, opts, true)
}

/// Finds the substring of `target` closest to `source` under Levenshtein.
pub fn levenshtein_search(source: &str, target: &str, opts: &Options) -> SearchResult {
    search_impl(source, target, opts, false)
}

/// Finds the substring of `target` closest to `source` under Damerau–Levenshtein.
pub fn damerau_levenshtein_search(source: &str, target: &str, opts: &Options) -> SearchResult {
    search_impl(source, target, opts, true)
}

// ---------------------------------------------------------------------------
// Parallel batch (feature = "parallel")
// ---------------------------------------------------------------------------

/// [`levenshtein`], fanned out across a `rayon` thread pool. Requires the
/// `parallel` feature.
///
/// # Why this exists
///
/// `levenshtein` is a pure function over two borrowed `&str`s with no shared
/// state, so scoring many independent pairs is embarrassingly parallel with
/// zero coordination cost between pairs. This function is exactly
/// `pairs.par_iter().map(|(a, b)| levenshtein(a, b, opts)).collect()` — a thin
/// fan-out over the existing sequential primitive, not a second
/// implementation of it. The rolling-row/three-row/full-matrix dispatch inside
/// `levenshtein` itself is untouched; if you need `levenshtein_search` or
/// `damerau_levenshtein_search` in parallel, apply the same
/// `par_iter().map(...)` pattern at your own call site (see
/// `site/performance/parallelism.md`).
///
/// # When to reach for it vs. the sequential loop
///
/// Per-pair cost varies with length and shape: equal or near-equal inputs can
/// be much cheaper than unrelated inputs because the sequential primitive
/// trims common affixes. Rayon also has a fixed scheduling cost, so there is
/// no input-independent crossover point. Benchmark the actual workload with
/// `cargo bench -p verbora-distance --features parallel --
/// par_levenshtein`; for very small batches or short strings, prefer the plain
/// `pairs.iter().map(|(a, b)| levenshtein(a, b, opts)).collect()` loop.
///
/// # Allocation behaviour
///
/// One `Vec<f64>` sized to `pairs.len()` for the output, plus whatever
/// `levenshtein` itself allocates per pair. Weighted plain distance uses one
/// `Vec<f64>` row. Unit-cost ASCII uses integer bit-vectors and a Peq table;
/// multi-word Peq rows and long UTF-16 operands may allocate, while short
/// Unicode operands use fixed stack buffers. No additional buffering, no
/// locking, no per-call
/// thread-pool construction — this uses whichever global `rayon` pool is
/// already installed (or `rayon`'s default one), so pool configuration
/// remains the caller's responsibility, not this crate's.
///
/// # Order and errors
///
/// Output order matches input order — `results[i] == levenshtein(pairs[i].0,
/// pairs[i].1, opts)` — via `rayon`'s order-preserving `map` + `collect`.
/// `levenshtein` never errors, so every element is a plain `f64`.
#[cfg(feature = "parallel")]
pub fn par_levenshtein_batch(pairs: &[(&str, &str)], opts: &Options) -> Vec<f64> {
    use rayon::prelude::*;
    pairs
        .par_iter()
        .map(|(a, b)| levenshtein(a, b, opts))
        .collect()
}

/// [`damerau_levenshtein`], fanned out across a `rayon` thread pool. Requires
/// the `parallel` feature.
///
/// See [`par_levenshtein_batch`] for the full rationale, cost model and
/// allocation behaviour — identical here, since `damerau_levenshtein` differs
/// from `levenshtein` only in which internal dispatch path it takes
/// (restricted three-row vs. unrestricted full matrix), not in its
/// statelessness or thread-safety. This function is exactly
/// `pairs.par_iter().map(|(a, b)| damerau_levenshtein(a, b, opts)).collect()`.
#[cfg(feature = "parallel")]
pub fn par_damerau_levenshtein_batch(pairs: &[(&str, &str)], opts: &Options) -> Vec<f64> {
    use rayon::prelude::*;
    pairs
        .par_iter()
        .map(|(a, b)| damerau_levenshtein(a, b, opts))
        .collect()
}

// ---------------------------------------------------------------------------
// Distance mode
// ---------------------------------------------------------------------------

fn distance_impl(source: &str, target: &str, opts: &Options, damerau: bool) -> f64 {
    if !damerau {
        // Preserve the scalar recurrence's repeated floating-point additions
        // exactly while avoiding both its row allocation and, for non-ASCII
        // input, UTF-16 materialization when one side is empty.
        if source.is_empty() {
            let units = utf16_len(target);
            return if opts.insertion_cost == 1.0 {
                units as f64
            } else {
                repeated_cost(units, opts.insertion_cost)
            };
        }
        if target.is_empty() {
            let units = utf16_len(source);
            return if opts.deletion_cost == 1.0 {
                units as f64
            } else {
                repeated_cost(units, opts.deletion_cost)
            };
        }

        if source.is_ascii() && target.is_ascii() {
            return plain_levenshtein(source.as_bytes(), target.as_bytes(), opts);
        }

        // Large near-identical Unicode strings should not allocate and encode
        // their shared surroundings. Trim only at UTF-8 scalar boundaries;
        // `plain_levenshtein` performs the finer UTF-16-unit trim afterward.
        let (source, target) = if is_unit_cost(opts) && source.len().min(target.len()) > 64 {
            trim_common_utf8_affixes(source, target)
        } else {
            (source, target)
        };
        if source.is_empty() {
            return utf16_len(target) as f64;
        }
        if target.is_empty() {
            return utf16_len(source) as f64;
        }
        if source.is_ascii() && target.is_ascii() {
            return plain_levenshtein(source.as_bytes(), target.as_bytes(), opts);
        }

        // A UTF-16 sequence can never contain more units than its UTF-8 byte
        // representation. Small non-ASCII operands therefore fit in these
        // fixed buffers without a counting pass or a heap allocation.
        const STACK_UNITS: usize = 64;
        if source.len() <= STACK_UNITS && target.len() <= STACK_UNITS {
            let mut source_units = [0u16; STACK_UNITS];
            let mut target_units = [0u16; STACK_UNITS];
            let source_len = encode_utf16_into(source, &mut source_units);
            let target_len = encode_utf16_into(target, &mut target_units);
            return plain_levenshtein(
                &source_units[..source_len],
                &target_units[..target_len],
                opts,
            );
        }

        let source_units: Vec<u16> = source.encode_utf16().collect();
        let target_units: Vec<u16> = target.encode_utf16().collect();
        return plain_levenshtein(&source_units, &target_units, opts);
    }

    dispatch(source, target, |ops| match ops {
        Operands::Bytes(s, t) => distance_generic(s, t, opts, damerau),
        Operands::Units(s, t) => distance_generic(s, t, opts, damerau),
    })
}

#[inline]
fn repeated_cost(count: usize, cost: f64) -> f64 {
    (0..count).fold(0.0, |total, _| total + cost)
}

#[inline]
fn utf16_len(input: &str) -> usize {
    if input.is_ascii() {
        input.len()
    } else {
        input.encode_utf16().count()
    }
}

fn trim_common_utf8_affixes<'a>(mut source: &'a str, mut target: &'a str) -> (&'a str, &'a str) {
    let mut prefix = source
        .as_bytes()
        .iter()
        .zip(target.as_bytes())
        .take_while(|(a, b)| a == b)
        .count();
    while prefix != 0 && (!source.is_char_boundary(prefix) || !target.is_char_boundary(prefix)) {
        prefix -= 1;
    }
    source = &source[prefix..];
    target = &target[prefix..];

    let mut suffix = source
        .as_bytes()
        .iter()
        .rev()
        .zip(target.as_bytes().iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    while suffix != 0
        && (!source.is_char_boundary(source.len() - suffix)
            || !target.is_char_boundary(target.len() - suffix))
    {
        suffix -= 1;
    }
    if suffix != 0 {
        source = &source[..source.len() - suffix];
        target = &target[..target.len() - suffix];
    }
    (source, target)
}

#[inline]
fn encode_utf16_into(input: &str, out: &mut [u16]) -> usize {
    let mut len = 0usize;
    for (slot, unit) in out.iter_mut().zip(input.encode_utf16()) {
        *slot = unit;
        len += 1;
    }
    len
}

fn distance_generic<T: BitPeq + DamerauScratch>(
    source: &[T],
    target: &[T],
    opts: &Options,
    damerau: bool,
) -> f64 {
    match (damerau, opts.restricted) {
        (true, false) => unrestricted_damerau(source, target, opts),
        (true, true) => restricted_damerau(source, target, opts),
        (false, _) => plain_levenshtein(source, target, opts),
    }
}

/// Unrestricted Damerau–Levenshtein distance, choosing the fastest correct
/// evaluation of *Verbora's pinned recurrence* for the input.
///
/// The pinned recurrence is deliberately **not** textbook
/// Damerau–Levenshtein: the reference updates its last-occurrence map
/// within the row and evaluates the transposition candidate at match cells
/// with a genuinely negative row gap (see [`full_matrix`]'s own comment),
/// which diverges from the Lowrance–Wagner/Zhao–Sahni algorithm
/// `strsim`/`rapidfuzz` implement on a measurable fraction of inputs
/// (e.g. `"bb"` → `"abbb"` is 1 here, 2 there) and is not even symmetric.
/// Consequently the linear-space algorithms those crates use — and
/// common-affix trimming, which the `"bb"`/`"abbb"` pair also breaks —
/// are all off the table; the fast path below evaluates the exact pinned
/// recurrence, just on a cheaper structure.
fn unrestricted_damerau<T: BitPeq + DamerauScratch>(
    source: &[T],
    target: &[T],
    opts: &Options,
) -> f64 {
    if is_unit_cost(opts)
        && opts.transposition_cost == 1.0
        && source.len().saturating_add(target.len()) < u32::MAX as usize
    {
        return T::damerau_unit_dispatch(source, target);
    }
    full_matrix(source, target, opts, true, false).final_cost()
}

/// Restricted (OSA) Damerau–Levenshtein distance, choosing the fastest
/// correct algorithm for the input rather than always running
/// [`restricted_rows`]'s scalar three-row DP.
///
/// The same decision [`plain_levenshtein`] already makes for plain
/// Levenshtein, extended to the OSA variant: Hyyrö's 2003 transposition
/// extension of Myers' bit-vector algorithm computes the identical
/// unit-cost OSA distance in `O(n·m/64)` bitwise operations. The extension
/// over the plain kernels is exactly one extra register pair — the previous
/// column's `D0` word and the previous scanned character's pattern-match
/// mask — combined into a transposition mask `tr` that is OR-ed into `D0`
/// (verified line-by-line against `rapidfuzz-0.5.0/src/distance/osa.rs`'s
/// `hyrroe2003`/`hyrroe2003_block`, an independently-published
/// implementation of the same paper, before this was trusted).
///
/// The gate requires *all four* costs to be exactly 1.0 —
/// [`is_unit_cost`] covers insertion/deletion/substitution and the
/// transposition cost is checked separately here, since the bit-vector
/// formulation has no notion of a weighted transposition (a NaN cost fails
/// `== 1.0` and safely falls back to the scalar path). OSA under unit
/// costs is symmetric in its operands, so the shorter operand is always
/// the bit-packed pattern — the same operand-swap [`plain_levenshtein`]
/// performs, pinned by this module's own symmetry tests.
fn restricted_damerau<T: BitPeq>(source: &[T], target: &[T], opts: &Options) -> f64 {
    if is_unit_cost(opts) && opts.transposition_cost == 1.0 {
        let (shorter, longer) = if source.len() <= target.len() {
            (source, target)
        } else {
            (target, source)
        };
        if (2..=64).contains(&shorter.len()) {
            return osa_bit_vector(shorter, longer);
        }
        if shorter.len() > 64 {
            return osa_bit_vector_blocks(shorter, longer);
        }
    }
    restricted_rows(source, target, opts)
}

/// Whether every cost in `opts` is the unit-cost default — the only case
/// [`bit_vector_distance`] (Myers' algorithm) applies to; it has no notion of
/// a weighted insertion/deletion/substitution cost.
#[inline]
fn is_unit_cost(opts: &Options) -> bool {
    opts.insertion_cost == 1.0 && opts.deletion_cost == 1.0 && opts.substitution_cost == 1.0
}

/// Plain Levenshtein distance, choosing the fastest correct algorithm for the
/// input rather than always running [`plain_rows`]'s scalar DP.
///
/// `docs/PERFORMANCE_GAPS.md` entry 26 measured Verbora's scalar rolling-row DP
/// losing to `triple_accel`'s genuinely SIMD-accelerated Levenshtein by a
/// widening margin from 16 characters up (5.7× at 1024) — closing that gap
/// with literal SIMD intrinsics would require `unsafe`, which this
/// workspace's `unsafe_code = "deny"` policy rules out by default. Myers'
/// (1999) bit-vector algorithm gets a comparable *algorithmic* win in plain
/// safe Rust instead: it computes the same unit-cost edit distance in O(n)
/// work for a one-word pattern, or O(n·ceil(m/64)) across multiple words,
/// rather than O(n·m) scalar cell updates. It applies only when every cost is
/// exactly 1.0 (see [`is_unit_cost`] — weighted costs use the rolling-row
/// fallback). The shorter operand is always the bit-packed pattern.
///
/// The lower bound was originally 8 because the then-`HashMap` Peq's setup
/// cost made `n = 4` a wash against the scalar cells it replaced. The
/// [`BitPeq`] flat table removed that setup cost — the sibling OSA fast
/// path, gated at 2 from the start with the identical table shape,
/// measures ~16 ns at n = 4 against the scalar path's ~40 ns — so the
/// gate now starts at 1 (a 1-unit pattern has no Myers subtlety and the
/// kernel handles it; 0-length operands short-circuit before dispatch).
fn plain_levenshtein<T: BitPeq>(source: &[T], target: &[T], opts: &Options) -> f64 {
    if is_unit_cost(opts) {
        let (source, target) = if source.len().min(target.len()) > 16 {
            trim_common_affixes(source, target)
        } else {
            (source, target)
        };
        if source.is_empty() {
            return target.len() as f64;
        }
        if target.is_empty() {
            return source.len() as f64;
        }
        let (shorter, longer) = if source.len() <= target.len() {
            (source, target)
        } else {
            (target, source)
        };
        // Unit costs are symmetric (insertion_cost == deletion_cost), so
        // treating whichever operand is shorter as Myers' "pattern" changes
        // nothing about the result -- only which one gets the compact
        // bitmask representation.
        if (1..=4).contains(&shorter.len()) {
            return bit_vector_distance_tiny(shorter, longer);
        }
        if (5..=64).contains(&shorter.len()) {
            return bit_vector_distance(shorter, longer);
        }
        if shorter.len() > 64 {
            return bit_vector_distance_blocks(shorter, longer);
        }
    }
    plain_rows(source, target, opts)
}

/// Removes equal prefixes and suffixes before unit-cost plain Levenshtein.
/// Those aligned runs cannot participate in a cheaper edit script, so this
/// preserves the exact distance while shrinking the bit-vector pattern and
/// scan. It is deliberately never used by weighted or Damerau variants.
fn trim_common_affixes<'a, T: Unit>(
    mut source: &'a [T],
    mut target: &'a [T],
) -> (&'a [T], &'a [T]) {
    let shared = source.len().min(target.len());
    let mut prefix = 0usize;
    while prefix < shared && source[prefix] == target[prefix] {
        prefix += 1;
    }
    source = &source[prefix..];
    target = &target[prefix..];

    let shared = source.len().min(target.len());
    let mut suffix = 0usize;
    while suffix < shared && source[source.len() - 1 - suffix] == target[target.len() - 1 - suffix]
    {
        suffix += 1;
    }
    if suffix != 0 {
        source = &source[..source.len() - suffix];
        target = &target[..target.len() - suffix];
    }
    (source, target)
}

/// Single-word Myers kernel for patterns of at most four units. Building the
/// normal 256-entry byte Peq table costs more than deriving these few match
/// bits directly from the pattern on every scanned unit.
fn bit_vector_distance_tiny<T: BitPeq>(shorter: &[T], longer: &[T]) -> f64 {
    let m = shorter.len();
    debug_assert!((1..=4).contains(&m));

    if m == 1 {
        return (longer.len() - usize::from(longer.contains(&shorter[0]))) as f64;
    }

    let last_bit = 1u64 << (m - 1);
    let mut pv = (1u64 << m) - 1;
    let mut mv = 0u64;
    let mut score = m as i64;

    for &c in longer {
        let mut eq = u64::from(shorter[0] == c);
        eq |= u64::from(shorter[1] == c) << 1;
        if m > 2 {
            eq |= u64::from(shorter[2] == c) << 2;
        }
        if m > 3 {
            eq |= u64::from(shorter[3] == c) << 3;
        }

        let xv = eq | mv;
        let xh = (((eq & pv).wrapping_add(pv)) ^ pv) | eq;
        let mut ph = mv | !(xh | pv);
        let mut mh = pv & xh;

        score += i64::from(ph & last_bit != 0) - i64::from(mh & last_bit != 0);
        ph = (ph << 1) | 1;
        mh <<= 1;
        pv = mh | !(xv | ph);
        mv = ph & xv;
    }

    score as f64
}

/// Myers' (1999) bit-vector algorithm for unit-cost edit distance: the
/// "pattern" `shorter` must be non-empty and fit in one 64-bit word (`len()
/// <= 64`). [`plain_levenshtein`] selects the multi-word sibling
/// [`bit_vector_distance_blocks`] past that bound. Computes exactly what
/// [`plain_rows`] computes for
/// [`Options::default()`]-equivalent (unit) costs, verified exhaustively
/// against it in this module's own tests before being trusted for anything.
fn bit_vector_distance<T: BitPeq>(shorter: &[T], longer: &[T]) -> f64 {
    let m = shorter.len();
    debug_assert!(m > 0 && m <= 64);

    // Peq[c]: bit i set iff `shorter[i] == c`, via [`BitPeq`]'s per-type
    // tables. This was originally a `std::collections::HashMap`, and a
    // controlled decomposition experiment (see `docs/PERFORMANCE_GAPS.md`
    // entry 26's second update) measured that map -- one SipHash probe per
    // scanned character -- at roughly three quarters of the whole kernel's
    // runtime; the flat byte table turns each probe into one indexed load.
    let peq = T::peq1(shorter);

    let last_bit = 1u64 << (m - 1);
    let mut pv: u64 = if m == 64 { u64::MAX } else { (1u64 << m) - 1 };
    let mut mv: u64 = 0;
    let mut score = m as i64;

    for &c in longer {
        let eq = T::peq1_get(&peq, c);
        let xv = eq | mv;
        let xh = (((eq & pv).wrapping_add(pv)) ^ pv) | eq;
        let mut ph = mv | !(xh | pv);
        let mut mh = pv & xh;

        score += i64::from(ph & last_bit != 0) - i64::from(mh & last_bit != 0);

        ph = (ph << 1) | 1;
        mh <<= 1;
        pv = mh | !(xv | ph);
        mv = ph & xv;
    }

    score as f64
}

/// Myers' (1999) bit-vector algorithm generalised across multiple 64-bit
/// words, for patterns past [`bit_vector_distance`]'s single-word bound
/// (`shorter.len() > 64`) -- closes the remaining, previously-unattempted
/// half of `docs/PERFORMANCE_GAPS.md` entry 26's "multi-word block
/// extension of Myers' algorithm" opportunity. Still `O(n·m/64)` bitwise
/// work rather than `O(n·m)` scalar cells, and still no `unsafe` -- the same
/// trade this workspace already made for the single-word case, just applied
/// per-block.
///
/// Follows Hyyrö's 2003 block formulation of the same paper
/// [`bit_vector_distance`] already cites (confirmed against
/// `rapidfuzz-0.5.0/src/distance/levenshtein.rs`'s own `hyrroe2003_block`,
/// an independently-published, widely-used implementation of the identical
/// algorithm, read directly rather than re-derived from memory): `shorter`
/// is split into `ceil(m / 64)` blocks, each carrying its own `Pv`/`Mv`
/// state, and a single left-to-right sweep over each row of `longer`
/// threads a horizontal-delta carry bit from each block into the next --
/// the generalisation of the single-word algorithm's constant `| 1` /
/// `<< 1` carry-in, which only works there because a lone word has no
/// "next block" to receive an overflow from. Deliberately does **not**
/// carry over `rapidfuzz`'s additional Ukkonen-band block-skipping layered
/// on top of this same core -- that optimisation exists to skip blocks once
/// a distance *threshold* rules them out, and Verbora's `levenshtein`
/// always wants the exact distance, never a bounded/thresholded one, so
/// there is no cutoff to band around.
///
/// Verified exhaustively against [`plain_rows`] (this module's own tests)
/// across randomized pairs spanning many block-boundary lengths (63 through
/// several thousand units), and cross-checked directly against
/// [`bit_vector_distance`] itself at every length where the two functions'
/// domains overlap (`8..=64`) -- block-carry propagation is exactly the
/// part a naive per-block reapplication of the single-word formula would
/// get wrong, so agreement with the already-proven single-word path at the
/// boundary is treated as load-bearing evidence, not a formality.
fn bit_vector_distance_blocks<T: BitPeq>(shorter: &[T], longer: &[T]) -> f64 {
    // Removing the per-column score dependency and reconstructing it from
    // final Pv/Mv popcounts wins through four blocks in A/B benchmarks; past
    // that crossover the extra popcounts lose to the simple last-bit update.
    if shorter.len() <= 256 {
        bit_vector_distance_blocks_impl::<T, true>(shorter, longer)
    } else {
        bit_vector_distance_blocks_impl::<T, false>(shorter, longer)
    }
}

fn bit_vector_distance_blocks_impl<T: BitPeq, const FINAL_POPCOUNT: bool>(
    shorter: &[T],
    longer: &[T],
) -> f64 {
    const WORD: usize = 64;
    let m = shorter.len();
    debug_assert!(m > 0);

    let blocks = m.div_ceil(WORD);
    let last_block = blocks - 1;
    let last_bit = 1u64 << ((m - 1) % WORD);

    // Peq row per unit via [`BitPeq`]: bit i of word b set iff
    // shorter[b * 64 + i] == c. The trailing, never-set bits in the last
    // (possibly partial) block never match any character and are never read
    // past `last_bit`, so leaving them unset is correct, not just
    // convenient -- see this function's own doc comment for why they cannot
    // leak into the meaningful output bit (addition-carry propagation only
    // flows from low bits to high ones, and `last_bit` sits below every
    // such trailing bit). This was one `std::collections::HashMap` per
    // block -- one SipHash probe per block per scanned character, measured
    // (see `docs/PERFORMANCE_GAPS.md` entry 26's second update) at roughly
    // three quarters of the kernel's whole runtime; the packed table costs
    // one lookup per scanned character, then pure indexed loads per block.
    let peq = T::peqn(shorter, blocks);

    // Skip a leading run whose alphabet is absent from the pattern. Processing
    // `k` such units through Myers would only clear the lowest `k` meaningful
    // Pv bits; initializing that state directly avoids O(k·blocks) work. If
    // the whole target is disjoint this reduces the answer to max(m, n). Unlike
    // a separate all-target preflight, the absent prefix is never scanned twice
    // when the first overlap happens late.
    let leading_absent = longer
        .iter()
        .position(|&unit| T::peqn_row(&peq, unit).is_some())
        .unwrap_or(longer.len());
    if leading_absent == longer.len() {
        return m.max(longer.len()) as f64;
    }
    const STACK_BLOCKS: usize = 16;
    let zeros_stack = [0u64; STACK_BLOCKS];
    let zeros_heap;
    let zeros: &[u64] = if blocks <= STACK_BLOCKS {
        &zeros_stack[..blocks]
    } else {
        zeros_heap = vec![0u64; blocks];
        &zeros_heap
    };

    let mut pv_stack = [u64::MAX; STACK_BLOCKS];
    let mut pv_heap;
    let pv: &mut [u64] = if blocks <= STACK_BLOCKS {
        &mut pv_stack[..blocks]
    } else {
        pv_heap = vec![u64::MAX; blocks];
        &mut pv_heap
    };

    for (b, word) in pv.iter_mut().enumerate() {
        let skipped_here = leading_absent.saturating_sub(b * WORD).min(WORD);
        *word = if skipped_here == WORD {
            0
        } else {
            u64::MAX << skipped_here
        };
    }

    let mut mv_stack = [0u64; STACK_BLOCKS];
    let mut mv_heap;
    let mv: &mut [u64] = if blocks <= STACK_BLOCKS {
        &mut mv_stack[..blocks]
    } else {
        mv_heap = vec![0u64; blocks];
        &mut mv_heap
    };
    let mut score = m.max(leading_absent) as i64;

    for &c in &longer[leading_absent..] {
        let row = T::peqn_row(&peq, c).unwrap_or(zeros);
        let mut hp_carry_in = true;
        let mut hn_carry_in = false;

        for (b, &eq) in row.iter().enumerate() {
            let x = eq | u64::from(hn_carry_in);
            let d0 = ((x & pv[b]).wrapping_add(pv[b]) ^ pv[b]) | x | mv[b];

            let mut hp = mv[b] | !(d0 | pv[b]);
            let mut hn = d0 & pv[b];

            let (hp_carry_out, hn_carry_out) = if b == last_block {
                if FINAL_POPCOUNT {
                    (false, false)
                } else {
                    (hp & last_bit != 0, hn & last_bit != 0)
                }
            } else {
                (hp & (1u64 << 63) != 0, hn & (1u64 << 63) != 0)
            };

            if !FINAL_POPCOUNT && b == last_block {
                score += i64::from(hp_carry_out) - i64::from(hn_carry_out);
            }

            hp = (hp << 1) | u64::from(hp_carry_in);
            hn = (hn << 1) | u64::from(hn_carry_in);

            pv[b] = hn | !(d0 | hp);
            mv[b] = hp & d0;

            hp_carry_in = hp_carry_out;
            hn_carry_in = hn_carry_out;
        }
    }

    if FINAL_POPCOUNT {
        let mut distance = longer.len() as i64;
        for b in 0..blocks {
            let mask = if b == last_block {
                if last_bit == 1u64 << 63 {
                    u64::MAX
                } else {
                    last_bit | (last_bit - 1)
                }
            } else {
                u64::MAX
            };
            distance += (pv[b] & mask).count_ones() as i64;
            distance -= (mv[b] & mask).count_ones() as i64;
        }
        distance as f64
    } else {
        score as f64
    }
}

// ---------------------------------------------------------------------------
// Bit-parallel OSA (restricted Damerau–Levenshtein)
// ---------------------------------------------------------------------------

/// Hyyrö's (2003) bit-vector algorithm for unit-cost restricted
/// Damerau–Levenshtein (OSA): the pattern `shorter` must be non-empty and
/// fit in one 64-bit word. Exactly [`bit_vector_distance`]'s Myers kernel
/// plus one extra register pair — `prev_d0` (the previous column's `D0`
/// word) and `prev_pm` (the previous scanned character's pattern-match
/// mask) — whose combination `tr` marks the cells reachable by a free
/// diagonal-2 step, i.e. an adjacent transposition, and is OR-ed into
/// `D0`. Verified against both [`restricted_rows`] (the scalar oracle) and
/// `rapidfuzz-0.5.0`'s `hyrroe2003` in this module's own tests before
/// being trusted for anything.
fn osa_bit_vector<T: BitPeq>(shorter: &[T], longer: &[T]) -> f64 {
    let m = shorter.len();
    debug_assert!((1..=64).contains(&m));
    let table = T::peq1(shorter);

    let last_bit = 1u64 << (m - 1);
    let mut pv: u64 = if m == 64 { u64::MAX } else { (1u64 << m) - 1 };
    let mut mv: u64 = 0;
    let mut prev_d0: u64 = 0;
    let mut prev_pm: u64 = 0;
    let mut score = m as i64;

    for &c in longer {
        let pm_j = T::peq1_get(&table, c);
        let tr = (((!prev_d0) & pm_j) << 1) & prev_pm;
        let d0 = (((pm_j & pv).wrapping_add(pv)) ^ pv) | pm_j | mv | tr;

        let mut hp = mv | !(d0 | pv);
        let mut hn = d0 & pv;

        if hp & last_bit != 0 {
            score += 1;
        }
        if hn & last_bit != 0 {
            score -= 1;
        }

        hp = (hp << 1) | 1;
        hn <<= 1;
        pv = hn | !(d0 | hp);
        mv = hp & d0;

        prev_d0 = d0;
        prev_pm = pm_j;
    }

    score as f64
}

/// Per-word state for [`osa_bit_vector_blocks`]: the plain kernel's
/// `Pv`/`Mv` plus the OSA-specific previous-column `D0` and pattern-match
/// mask this word saw, which the *next* column's transposition mask needs.
struct OsaBlock {
    pv: u64,
    mv: u64,
    d0: u64,
    pm: u64,
}

/// [`osa_bit_vector`] generalised across multiple 64-bit words, for
/// patterns past the single-word bound — the OSA counterpart of
/// [`bit_vector_distance_blocks`], carrying one extra cross-word term: the
/// `<< 1` inside the transposition mask receives bit 63 of
/// `!D0_prev & PM_cur` from the word below, exactly how a transposition
/// straddling a word boundary is seen (verified against
/// `rapidfuzz-0.5.0`'s `hyrroe2003_block`, whose two-buffer `mem::swap`
/// scheme this replaces with one in-place state vector plus two carried
/// scalars — the sentinel row rapidfuzz keeps at index 0 is never written
/// and stays all-zero, which is exactly what resetting the carried scalars
/// to zero at the top of each column reproduces).
fn osa_bit_vector_blocks<T: BitPeq>(shorter: &[T], longer: &[T]) -> f64 {
    const WORD: usize = 64;
    let m = shorter.len();
    // The dispatch gate routes only m > 64 here, but the formula is valid
    // for any m >= 1 (with one block the cross-word carry terms are always
    // zero) — kept callable on the shared domain so the tests can pit this
    // implementation directly against `osa_bit_vector`'s independent shape.
    debug_assert!(m >= 1);
    let blocks = m.div_ceil(WORD);
    let table = T::peqn(shorter, blocks);
    let zeros = vec![0u64; blocks];

    let last_bit = 1u64 << ((m - 1) % WORD);
    let mut state: Vec<OsaBlock> = (0..blocks)
        .map(|_| OsaBlock {
            pv: u64::MAX,
            mv: 0,
            d0: 0,
            pm: 0,
        })
        .collect();
    let mut score = m as i64;

    for &c in longer {
        let row = T::peqn_row(&table, c).unwrap_or(&zeros);
        let mut hp_carry: u64 = 1;
        let mut hn_carry: u64 = 0;
        // Word b−1's previous-column `D0` and current-column `PM`, needed
        // by word b's transposition mask. All-zero at b = 0 (the sentinel).
        let mut below_prev_d0: u64 = 0;
        let mut below_pm: u64 = 0;

        for (b, blk) in state.iter_mut().enumerate() {
            let pm_j = row[b];
            let prev_d0 = blk.d0;
            let prev_pm = blk.pm;

            let tr = ((((!prev_d0) & pm_j) << 1) | (((!below_prev_d0) & below_pm) >> 63)) & prev_pm;

            let x = pm_j | hn_carry;
            let d0 = (((x & blk.pv).wrapping_add(blk.pv)) ^ blk.pv) | x | blk.mv | tr;

            let mut hp = blk.mv | !(d0 | blk.pv);
            let mut hn = d0 & blk.pv;

            if b == blocks - 1 {
                if hp & last_bit != 0 {
                    score += 1;
                }
                if hn & last_bit != 0 {
                    score -= 1;
                }
            }

            let hp_out = hp >> 63;
            let hn_out = hn >> 63;
            hp = (hp << 1) | hp_carry;
            hn = (hn << 1) | hn_carry;

            blk.pv = hn | !(d0 | hp);
            blk.mv = hp & d0;
            blk.d0 = d0;
            blk.pm = pm_j;

            hp_carry = hp_out;
            hn_carry = hn_out;
            below_prev_d0 = prev_d0;
            below_pm = pm_j;
        }
    }

    score as f64
}

// ---------------------------------------------------------------------------
// Unrestricted Damerau, unit-cost fast path
// ---------------------------------------------------------------------------

/// Per-symbol scratch for [`damerau_unrestricted_unit`]: the last row each
/// source symbol appeared in, plus that symbol's snapshot slot in the row
/// arena — specialised per unit type the way [`Unit::Map`] already is
/// (flat arrays for bytes, `FxHashMap` for UTF-16 units).
trait DamerauScratch: Unit {
    type SymTable;
    fn new_table() -> Self::SymTable;
    /// `(last_row, slot)` for `unit`, if it has occurred as a source symbol.
    fn get(table: &Self::SymTable, unit: Self) -> Option<(u32, u32)>;
    /// Records `row` as `unit`'s last occurrence, assigning `next_slot` on
    /// first sight; returns the unit's slot.
    fn set(table: &mut Self::SymTable, unit: Self, row: u32, next_slot: u32) -> u32;
    /// The unit-cost kernel dispatch for this unit type. Bytes get a
    /// measured three-tier dispatcher (stack matrix / register-carried /
    /// memory-carried — see [`damerau_unit_small`] and friends); UTF-16
    /// units use the generic snapshot kernel.
    fn damerau_unit_dispatch(source: &[Self], target: &[Self]) -> f64;
}

impl DamerauScratch for u8 {
    // rows[u] == 0 is the vacant sentinel — rows are 1-based.
    type SymTable = ([u32; 256], [u32; 256]);

    fn new_table() -> Self::SymTable {
        ([0u32; 256], [0u32; 256])
    }

    #[inline]
    fn get(table: &Self::SymTable, unit: Self) -> Option<(u32, u32)> {
        let row = table.0[unit as usize];
        (row != 0).then(|| (row, table.1[unit as usize]))
    }

    #[inline]
    fn set(table: &mut Self::SymTable, unit: Self, row: u32, next_slot: u32) -> u32 {
        let u = unit as usize;
        if table.0[u] == 0 {
            table.1[u] = next_slot;
        }
        table.0[u] = row;
        table.1[u]
    }

    fn damerau_unit_dispatch(source: &[Self], target: &[Self]) -> f64 {
        let n = source.len();
        let m = target.len();
        // The tier thresholds are measured crossovers (see
        // `docs/PERFORMANCE_GAPS.md`'s unrestricted-Damerau entry): the
        // stack matrix wins while its fixed costs beat any table setup;
        // the register-carried mid kernel wins while the snapshot arena is
        // L1-resident; past that, keeping the `cur[c-1]`/`prev[c]` loads
        // memory-carried wins instead (the register chain otherwise puts
        // the arena load's latency onto the loop-carried dependency).
        if n <= 8 && m <= 8 {
            return damerau_unit_small(source, target);
        }
        if n + m <= u16::MAX as usize {
            if n <= 128 && m <= 128 {
                return damerau_unit_mid(source, target);
            }
            return damerau_unit_large(source, target);
        }
        if n + m < u32::MAX as usize {
            return damerau_unrestricted_unit::<u8, u32>(source, target);
        }
        f64::NAN // unreachable: gated at the caller
    }
}

impl DamerauScratch for u16 {
    type SymTable = FxHashMap<u16, (u32, u32)>;

    fn new_table() -> Self::SymTable {
        FxHashMap::default()
    }

    #[inline]
    fn get(table: &Self::SymTable, unit: Self) -> Option<(u32, u32)> {
        table.get(&unit).copied()
    }

    #[inline]
    fn set(table: &mut Self::SymTable, unit: Self, row: u32, next_slot: u32) -> u32 {
        let entry = table.entry(unit).or_insert((0, next_slot));
        entry.0 = row;
        entry.1
    }
    fn damerau_unit_dispatch(source: &[Self], target: &[Self]) -> f64 {
        let total = source.len().saturating_add(target.len());
        if total <= u16::MAX as usize {
            return damerau_unrestricted_unit::<u16, u16>(source, target);
        }
        if total < u32::MAX as usize {
            return damerau_unrestricted_unit::<u16, u32>(source, target);
        }
        f64::NAN // unreachable: gated at the caller
    }
}

/// [`full_matrix`]'s unrestricted-Damerau recurrence, evaluated in
/// `O(distinct_source_symbols × m)` space instead of a full
/// `(n+1) × (m+1)` `f64` cost matrix plus an equally-sized parent matrix
/// that distance mode never reads.
///
/// The full matrix exists in the original because the transposition
/// candidate reads `cost[lrm - 1][lcm - 1]` for an arbitrary earlier row
/// `lrm`. But `lrm` is always `last_row_map[t]` — the most recent row
/// where symbol `t` was the source symbol — so the set of rows reachable
/// through it is at most one per distinct source symbol. Keeping a
/// snapshot of row `last_row[x] − 1` per distinct symbol `x` (copied from
/// `prev` once per row, one `memcpy`, not per cell) makes two rolling rows
/// sufficient.
///
/// Two deliberate re-orderings, both semantics-preserving:
/// * The reference executes `last_row_map.set(s, r)` inside the column
///   loop (m redundant stores); hoisting it to row start is equivalent
///   because the map is only read at cells with `c > 1`, by which point
///   the original has already run the store at `c = 1`.
/// * For a `t == s` read, `lrm == r` and the original reads
///   `cost[r − 1][·]` — exactly the `prev` row this row's snapshot was
///   copied from.
///
/// With every cost 1.0 the candidate arithmetic collapses to `u32`
/// integers: the transposition candidate
/// `before + row_gap·1 + col_gap·1 + 1` becomes
/// `before + (r + c − lrm − lcm − 1)`, provably non-negative
/// (`lrm ≤ r`, `lcm ≤ c − 1`) — the reference's genuinely negative
/// `row_gap = −1` case (see [`full_matrix`]) folds into unsigned
/// arithmetic with no signed math. Candidate *order* is irrelevant here:
/// only the minimum value survives, and parents — the one thing order
/// affects — exist only for search mode, which never takes this path.
/// Every cell value is an integer at most `n + m` (gated `< u32::MAX` at
/// dispatch), exactly representable in `f64`, so outputs are bitwise
/// identical to the `f64` matrix's.
/// A DP cell for [`damerau_unrestricted_unit`]: `u16` for inputs whose
/// distances fit it (halving row and snapshot-arena memory traffic — the
/// dominant cost of this kernel at large sizes), `u32` beyond.
trait DamCell: Copy + Ord {
    fn from_usize(v: usize) -> Self;
    fn to_f64(self) -> f64;
    fn plus(self, d: usize) -> Self;
}

impl DamCell for u16 {
    #[inline]
    fn from_usize(v: usize) -> Self {
        v as u16
    }
    #[inline]
    fn to_f64(self) -> f64 {
        f64::from(self)
    }
    #[inline]
    fn plus(self, d: usize) -> Self {
        self + d as u16
    }
}

impl DamCell for u32 {
    #[inline]
    fn from_usize(v: usize) -> Self {
        v as u32
    }
    #[inline]
    fn to_f64(self) -> f64 {
        f64::from(self)
    }
    #[inline]
    fn plus(self, d: usize) -> Self {
        self + d as u32
    }
}

/// The pinned unrestricted-Damerau recurrence on a fixed stack matrix, for
/// operands of at most 8 bytes each: no tables, no heap — the
/// last-occurrence lookup is a plain `rposition` scan over the few source
/// bytes seen so far, and the full (tiny) matrix makes the transposition
/// read a direct index. Below this size the scratch-table zeroing the
/// larger kernels pay costs more than the whole DP.
fn damerau_unit_small(source: &[u8], target: &[u8]) -> f64 {
    const CAP: usize = 9;
    let n = source.len();
    let m = target.len();
    debug_assert!(n < CAP && m < CAP);
    if n == 0 {
        return m as f64;
    }
    if m == 0 {
        return n as f64;
    }
    let w = m + 1;
    let mut mat = [0u16; CAP * CAP];
    for (c, cell) in mat[..=m].iter_mut().enumerate() {
        *cell = c as u16;
    }
    for r in 1..=n {
        mat[r * w] = r as u16;
    }
    for r in 1..=n {
        let s = source[r - 1];
        let base = r * w;
        let pbase = base - w;
        let mut lcm: usize = 0;
        for c in 1..=m {
            let t = target[c - 1];
            let insert = mat[base + c - 1] + 1;
            let delete = mat[pbase + c] + 1;
            let sub = mat[pbase + c - 1] + u16::from(s != t);
            let mut best = insert.min(delete).min(sub);
            if r > 1 && c > 1 && lcm != 0 {
                // `source[..r]` includes this row's own symbol, matching the
                // reference's mid-row `last_row_map.set(s, r)`.
                if let Some(p) = source[..r].iter().rposition(|&x| x == t) {
                    let lrm = p + 1;
                    let before = mat[(lrm - 1) * w + (lcm - 1)];
                    let gaps = r + c - lrm - lcm - 1;
                    let transpose = before + gaps as u16;
                    if transpose < best {
                        best = transpose;
                    }
                }
            }
            mat[base + c] = best;
            if s == t {
                lcm = c;
            }
        }
    }
    f64::from(mat[n * w + m])
}

/// The mid-range byte kernel (both operands ≤ 128 units): the generic
/// snapshot kernel's recurrence with three measured restructurings —
/// row `r = 1` and column `c = 1` peeled so the steady-state loop drops
/// its `r > 1`/`c > 1` guards; the column loop split into a
/// no-candidate-possible phase A (before the row's first match) and a
/// candidate phase B; one packed `[u32; 256]` symbol table
/// (`slot << 16 | row`, half the zeroing of the two-array form); and the
/// `cur[c - 1]` operand carried in a register. Cells are `u16` — the
/// dispatch gate guarantees `n + m` fits.
fn damerau_unit_mid(source: &[u8], target: &[u8]) -> f64 {
    let n = source.len();
    let m = target.len();
    if n == 0 {
        return m as f64;
    }
    if m == 0 {
        return n as f64;
    }
    let w = m + 1;
    let mut prev: Vec<u16> = (0..=m).map(|v| v as u16).collect();
    let mut cur: Vec<u16> = vec![0u16; w];
    let mut table = [0u32; 256]; // low 16 bits: row (0 vacant); high 16: slot
    let mut arena: Vec<u16> = Vec::new();
    let mut next_slot: u32;

    // Row r = 1: the candidate needs r > 1, so this is a plain DP row.
    {
        let s = source[0];
        table[s as usize] = 1; // slot 0, row 1
        arena.resize(w, 0);
        next_slot = 1;
        arena[..w].copy_from_slice(&prev);

        cur[0] = 1;
        let mut left: u16 = 1;
        let mut diag = prev[0];
        for c in 1..=m {
            let t = target[c - 1];
            let up = prev[c];
            let insert = left + 1;
            let delete = up + 1;
            let sub = diag + u16::from(s != t);
            let best = insert.min(delete).min(sub);
            cur[c] = best;
            left = best;
            diag = up;
        }
        std::mem::swap(&mut prev, &mut cur);
    }

    for r in 2..=n {
        let s = source[r - 1];
        let su = s as usize;
        let old = table[su];
        let slot = if old == 0 {
            let sl = next_slot;
            next_slot += 1;
            arena.resize(arena.len() + w, 0);
            sl
        } else {
            old >> 16
        };
        table[su] = (slot << 16) | r as u32;
        arena[slot as usize * w..][..w].copy_from_slice(&prev);

        let rw = r as u16;
        cur[0] = rw;
        // c = 1 peeled: the candidate needs c > 1.
        let t0 = target[0];
        let mut diag;
        let mut left;
        {
            let up = prev[1];
            let insert = rw + 1;
            let delete = up + 1;
            let sub = prev[0] + u16::from(s != t0);
            let best = insert.min(delete).min(sub);
            cur[1] = best;
            left = best;
            diag = up;
        }
        let mut lcm: usize = if s == t0 { 1 } else { 0 };
        let mut c = 2usize;

        // Phase A: no match seen this row yet, so no candidate is possible.
        if lcm == 0 {
            while c <= m {
                let t = target[c - 1];
                let up = prev[c];
                let insert = left + 1;
                let delete = up + 1;
                let sub = diag + u16::from(s != t);
                let best = insert.min(delete).min(sub);
                cur[c] = best;
                left = best;
                diag = up;
                if s == t {
                    lcm = c;
                    c += 1;
                    break;
                }
                c += 1;
            }
        }

        // Phase B: `lcm != 0` guaranteed.
        while c <= m {
            let t = target[c - 1];
            let up = prev[c];
            let insert = left + 1;
            let delete = up + 1;
            let sub = diag + u16::from(s != t);
            let mut best = insert.min(delete).min(sub);

            let e = table[t as usize];
            let lrm = e & 0xFFFF;
            if lrm != 0 {
                let before = arena[(e >> 16) as usize * w + (lcm - 1)];
                let gaps = r + c - lrm as usize - lcm - 1;
                let transpose = before + gaps as u16;
                if transpose < best {
                    best = transpose;
                }
            }
            cur[c] = best;
            left = best;
            diag = up;
            if s == t {
                lcm = c;
            }
            c += 1;
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    f64::from(prev[m])
}

/// The large byte kernel (either operand > 128 units): identical to
/// [`damerau_unit_mid`] except the `cur[c - 1]`/`prev[c]` operands stay
/// memory-carried — measured as the winning shape once the snapshot arena
/// outgrows L1, where the register-carried chain instead puts the arena
/// load's miss latency onto the loop-carried dependency.
fn damerau_unit_large(source: &[u8], target: &[u8]) -> f64 {
    let n = source.len();
    let m = target.len();
    if n == 0 {
        return m as f64;
    }
    if m == 0 {
        return n as f64;
    }
    let w = m + 1;
    let mut prev: Vec<u16> = (0..=m).map(|v| v as u16).collect();
    let mut cur: Vec<u16> = vec![0u16; w];
    let mut table = [0u32; 256];
    let mut arena: Vec<u16> = Vec::new();
    let mut next_slot: u32;

    {
        let s = source[0];
        table[s as usize] = 1;
        arena.resize(w, 0);
        next_slot = 1;
        arena[..w].copy_from_slice(&prev);
        cur[0] = 1;
        let mut diag = prev[0];
        for c in 1..=m {
            let t = target[c - 1];
            let insert = cur[c - 1] + 1;
            let delete = prev[c] + 1;
            let sub = diag + u16::from(s != t);
            cur[c] = insert.min(delete).min(sub);
            diag = prev[c];
        }
        std::mem::swap(&mut prev, &mut cur);
    }

    for r in 2..=n {
        let s = source[r - 1];
        let su = s as usize;
        let old = table[su];
        let slot = if old == 0 {
            let sl = next_slot;
            next_slot += 1;
            arena.resize(arena.len() + w, 0);
            sl
        } else {
            old >> 16
        };
        table[su] = (slot << 16) | r as u32;
        arena[slot as usize * w..][..w].copy_from_slice(&prev);

        cur[0] = r as u16;
        let t0 = target[0];
        {
            let insert = cur[0] + 1;
            let delete = prev[1] + 1;
            let sub = prev[0] + u16::from(s != t0);
            cur[1] = insert.min(delete).min(sub);
        }
        let mut lcm: usize = if s == t0 { 1 } else { 0 };
        let mut diag = prev[1];
        let mut c = 2usize;

        if lcm == 0 {
            while c <= m {
                let t = target[c - 1];
                let insert = cur[c - 1] + 1;
                let delete = prev[c] + 1;
                let sub = diag + u16::from(s != t);
                cur[c] = insert.min(delete).min(sub);
                diag = prev[c];
                if s == t {
                    lcm = c;
                    c += 1;
                    break;
                }
                c += 1;
            }
        }

        while c <= m {
            let t = target[c - 1];
            let insert = cur[c - 1] + 1;
            let delete = prev[c] + 1;
            let sub = diag + u16::from(s != t);
            let mut best = insert.min(delete).min(sub);
            let e = table[t as usize];
            let lrm = e & 0xFFFF;
            if lrm != 0 {
                let before = arena[(e >> 16) as usize * w + (lcm - 1)];
                let gaps = r + c - lrm as usize - lcm - 1;
                let transpose = before + gaps as u16;
                if transpose < best {
                    best = transpose;
                }
            }
            diag = prev[c];
            cur[c] = best;
            if s == t {
                lcm = c;
            }
            c += 1;
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    f64::from(prev[m])
}

fn damerau_unrestricted_unit<T: BitPeq + DamerauScratch, C: DamCell>(
    source: &[T],
    target: &[T],
) -> f64 {
    let n = source.len();
    let m = target.len();
    if n == 0 {
        return m as f64;
    }
    if m == 0 {
        return n as f64;
    }

    let w = m + 1;
    let mut prev: Vec<C> = (0..=m).map(C::from_usize).collect();
    let mut cur: Vec<C> = vec![C::from_usize(0); w];
    let mut table = T::new_table();
    let mut arena: Vec<C> = Vec::new();
    let mut next_slot: u32 = 0;

    for r in 1..=n {
        let s = source[r - 1];
        let slot = T::set(&mut table, s, r as u32, next_slot);
        if slot == next_slot {
            arena.resize(arena.len() + w, C::from_usize(0));
            next_slot += 1;
        }
        arena[slot as usize * w..][..w].copy_from_slice(&prev);

        cur[0] = C::from_usize(r);
        let mut lcm: usize = 0; // last column where s == t this row; 0 = none
        let mut diag = prev[0];
        for c in 1..=m {
            let t = target[c - 1];
            let insert = cur[c - 1].plus(1);
            let delete = prev[c].plus(1);
            let sub = diag.plus(usize::from(s != t));
            let mut best = insert.min(delete).min(sub);

            if r > 1 && c > 1 && lcm != 0 {
                if let Some((lrm, tslot)) = <T as DamerauScratch>::get(&table, t) {
                    let before = arena[tslot as usize * w + (lcm - 1)];
                    let gaps = r + c - lrm as usize - lcm - 1;
                    let transpose = before.plus(gaps);
                    if transpose < best {
                        best = transpose;
                    }
                }
            }

            diag = prev[c];
            cur[c] = best;
            if s == t {
                lcm = c;
            }
        }
        std::mem::swap(&mut prev, &mut cur);
    }

    prev[m].to_f64()
}

/// One-row Levenshtein. No parent tracking, so no tie-breaking concerns.
fn plain_rows<T: Unit>(source: &[T], target: &[T], opts: &Options) -> f64 {
    let m = target.len();

    let mut row: Vec<f64> = Vec::with_capacity(m + 1);
    row.push(0.0);
    for c in 1..=m {
        row.push(row[c - 1] + opts.insertion_cost);
    }

    for &s in source {
        let mut diag = row[0];
        row[0] += opts.deletion_cost;
        let mut left = row[0];
        for c in 1..=m {
            let up = row[c];
            let insert = left + opts.insertion_cost;
            let delete = up + opts.deletion_cost;
            let mut sub = diag;
            if s != target[c - 1] {
                sub += opts.substitution_cost;
            }
            let best = min3(insert, delete, sub);
            row[c] = best;
            diag = up;
            left = best;
        }
    }
    row[m]
}

#[cfg(test)]
fn plain_rows_two_oracle<T: Unit>(source: &[T], target: &[T], opts: &Options) -> f64 {
    let n = source.len();
    let m = target.len();
    let mut prev: Vec<f64> = Vec::with_capacity(m + 1);
    prev.push(0.0);
    for c in 1..=m {
        prev.push(prev[c - 1] + opts.insertion_cost);
    }
    let mut cur = vec![0.0f64; m + 1];
    for r in 1..=n {
        cur[0] = prev[0] + opts.deletion_cost;
        let s = source[r - 1];
        for c in 1..=m {
            let insert = cur[c - 1] + opts.insertion_cost;
            let delete = prev[c] + opts.deletion_cost;
            let mut sub = prev[c - 1];
            if s != target[c - 1] {
                sub += opts.substitution_cost;
            }
            cur[c] = min3(insert, delete, sub);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[m]
}

/// Three-row restricted Damerau (optimal string alignment).
fn restricted_rows<T: Unit>(source: &[T], target: &[T], opts: &Options) -> f64 {
    let n = source.len();
    let m = target.len();

    // rows[0] = r-2, rows[1] = r-1, rows[2] = r
    let mut prev2: Vec<f64> = vec![0.0; m + 1];
    let mut prev: Vec<f64> = Vec::with_capacity(m + 1);
    prev.push(0.0);
    for c in 1..=m {
        prev.push(prev[c - 1] + opts.insertion_cost);
    }
    let mut cur = vec![0.0f64; m + 1];

    for r in 1..=n {
        cur[0] = prev[0] + opts.deletion_cost;
        let s = source[r - 1];
        for c in 1..=m {
            let t = target[c - 1];
            let insert = cur[c - 1] + opts.insertion_cost;
            let delete = prev[c] + opts.deletion_cost;
            let mut sub = prev[c - 1];
            if s != t {
                sub += opts.substitution_cost;
            }
            let mut best = min3(insert, delete, sub);

            if r > 1 && c > 1 && s == target[c - 2] && source[r - 2] == t {
                let transpose = prev2[c - 2] + opts.transposition_cost;
                if transpose < best {
                    best = transpose;
                }
            }
            cur[c] = best;
        }
        // Rotate: prev2 <- prev <- cur, and reuse prev2's buffer as the next cur.
        std::mem::swap(&mut prev2, &mut prev);
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[m]
}

#[inline]
fn min3(a: f64, b: f64, c: f64) -> f64 {
    // Strict comparisons preserve the first candidate on ties and reproduce
    // the existing NaN behavior; `f64::min` has different NaN semantics.
    let mut best = a;
    if b < best {
        best = b;
    }
    if c < best {
        best = c;
    }
    best
}

// ---------------------------------------------------------------------------
// Full matrix (search mode, and unrestricted Damerau)
// ---------------------------------------------------------------------------

/// A dense cost matrix with parent back-pointers, laid out as two flat arrays.
///
/// Struct-of-arrays rather than array-of-structs: the cost sweep is contiguous
/// and the parents are only touched during backtracking, so keeping them apart
/// avoids polluting the cache line during the hot inner loop.
struct Matrix {
    cols: usize,
    cost: Vec<f64>,
    parent: Vec<(u32, u32)>,
    rows: usize,
}

impl Matrix {
    #[inline]
    fn idx(&self, r: usize, c: usize) -> usize {
        r * self.cols + c
    }

    #[inline]
    fn cost_at(&self, r: usize, c: usize) -> f64 {
        self.cost[self.idx(r, c)]
    }

    fn final_cost(&self) -> f64 {
        self.cost_at(self.rows - 1, self.cols - 1)
    }
}

fn full_matrix<T: Unit>(
    source: &[T],
    target: &[T],
    opts: &Options,
    damerau: bool,
    search: bool,
) -> Matrix {
    let n = source.len();
    let m = target.len();
    let cols = m + 1;

    let mut mat = Matrix {
        cols,
        cost: vec![0.0; (n + 1) * cols],
        parent: vec![(0, 0); (n + 1) * cols],
        rows: n + 1,
    };

    // Column 0: deletions all the way down.
    for r in 1..=n {
        let i = mat.idx(r, 0);
        mat.cost[i] = mat.cost[mat.idx(r - 1, 0)] + opts.deletion_cost;
        mat.parent[i] = ((r - 1) as u32, 0);
    }
    // Row 0: insertions across — except in search mode, where every prefix of the
    // target is a free starting point.
    for c in 1..=m {
        let i = mat.idx(0, c);
        if search {
            mat.cost[i] = 0.0;
        } else {
            mat.cost[i] = mat.cost[mat.idx(0, c - 1)] + opts.insertion_cost;
            mat.parent[i] = (0, (c - 1) as u32);
        }
    }

    let unrestricted = damerau && !opts.restricted;
    let restricted = damerau && opts.restricted;

    let mut last_row_map = T::new_map();
    let mut last_col_match: Option<usize> = None;

    for r in 1..=n {
        if unrestricted {
            last_col_match = None;
        }
        let s = source[r - 1];
        for c in 1..=m {
            let t = target[c - 1];

            let insert = mat.cost_at(r, c - 1) + opts.insertion_cost;
            let delete = mat.cost_at(r - 1, c) + opts.deletion_cost;
            let mut sub = mat.cost_at(r - 1, c - 1);
            if s != t {
                sub += opts.substitution_cost;
            }

            // Candidate order is load-bearing: the first strict minimum wins, and
            // the winner's coordinates become this cell's parent.
            let mut best_cost = insert;
            let mut best_parent = (r as u32, (c - 1) as u32);
            if delete < best_cost {
                best_cost = delete;
                best_parent = ((r - 1) as u32, c as u32);
            }
            if sub < best_cost {
                best_cost = sub;
                best_parent = ((r - 1) as u32, (c - 1) as u32);
            }

            if unrestricted && r > 1 && c > 1 {
                if let (Some(lcm), Some(lrm)) = (last_col_match, last_row_map.get(t)) {
                    let before = mat.cost_at(lrm - 1, lcm - 1);
                    // These gaps are genuinely signed. `last_row_map` is written
                    // as the current row is scanned, so `lrm` can equal `r`,
                    // making `r - lrm - 1` equal to -1. The reference multiplies the
                    // deletion cost by that negative gap; computing it in `usize`
                    // would underflow and panic, and clamping it at zero would
                    // change the resulting distance.
                    let row_gap = r as isize - lrm as isize - 1;
                    let col_gap = c as isize - lcm as isize - 1;
                    let transpose = before
                        + (row_gap as f64) * opts.deletion_cost
                        + (col_gap as f64) * opts.insertion_cost
                        + opts.transposition_cost;
                    if transpose < best_cost {
                        best_cost = transpose;
                        best_parent = ((lrm - 1) as u32, (lcm - 1) as u32);
                    }
                }
            }

            if restricted && r > 1 && c > 1 && s == target[c - 2] && source[r - 2] == t {
                let transpose = mat.cost_at(r - 2, c - 2) + opts.transposition_cost;
                if transpose < best_cost {
                    best_cost = transpose;
                    best_parent = ((r - 2) as u32, (c - 2) as u32);
                }
            }

            let i = mat.idx(r, c);
            mat.cost[i] = best_cost;
            mat.parent[i] = best_parent;

            if unrestricted {
                last_row_map.set(s, r);
                if s == t {
                    last_col_match = Some(c);
                }
            }
        }
    }

    mat
}

// ---------------------------------------------------------------------------
// Search mode
// ---------------------------------------------------------------------------

fn search_impl(source: &str, target: &str, opts: &Options, damerau: bool) -> SearchResult {
    dispatch(source, target, |ops| match ops {
        Operands::Bytes(s, t) => {
            let (start, end, dist) = search_generic(s, t, opts, damerau);
            SearchResult {
                // Safe: the operands are ASCII, so any byte range is valid UTF-8.
                substring: String::from_utf8_lossy(slice_units(t, start, end)).into_owned(),
                distance: dist,
                offset: start,
            }
        }
        Operands::Units(s, t) => {
            let (start, end, dist) = search_generic(s, t, opts, damerau);
            SearchResult {
                // The slice may split a surrogate pair, exactly as the reference's
                // `slice` does; lossy decoding maps a lone surrogate to U+FFFD,
                // which is the closest well-formed Rust representation.
                substring: String::from_utf16_lossy(slice_units(t, start, end)),
                distance: dist,
                offset: start,
            }
        }
    })
}

/// Returns `(match_start, match_end, distance)` in units of the operand slice.
///
/// `match_start` is signed; see [`SearchResult::offset`].
///
/// Dispatch mirrors [`plain_levenshtein`]'s: unit-cost plain Levenshtein —
/// the overwhelmingly common shape ([`Options::default()`]) — takes the
/// bit-parallel [`search_bits`] path; everything else keeps the original
/// full-matrix walk. Damerau search must **never** take the fast path even
/// under unit costs: an unrestricted transposition parent depends on
/// `last_row_map` state at the moment the cell was filled, which cell costs
/// alone cannot recover — a structural blocker, not an unimplemented case.
/// Empty operands are excluded so the kernels can assume a non-empty
/// pattern; [`search_full_matrix`] handles them at no measurable cost.
fn search_generic<T: BitPeq>(
    source: &[T],
    target: &[T],
    opts: &Options,
    damerau: bool,
) -> (isize, usize, f64) {
    if !damerau && is_unit_cost(opts) && !source.is_empty() && !target.is_empty() {
        return search_bits(source, target);
    }
    search_full_matrix(source, target, opts, damerau)
}

/// The original full-matrix search: [`full_matrix`] with a free row 0, the
/// reference's first-minimum scan of the last row, and a parent-chain
/// backtrack. The only search evaluation for weighted costs and both
/// Damerau variants — and the oracle [`search_bits`] is differentially
/// tested against.
fn search_full_matrix<T: Unit>(
    source: &[T],
    target: &[T],
    opts: &Options,
    damerau: bool,
) -> (isize, usize, f64) {
    let n = source.len();
    let m = target.len();
    let mat = full_matrix(source, target, opts, damerau, true);

    // Minimum over the last row. `>` keeps the FIRST minimum, matching the reference.
    let mut min_distance = (n + m) as f64;
    let mut match_end = m;
    for c in 0..=m {
        let cost = mat.cost_at(n, c);
        if min_distance > cost {
            min_distance = cost;
            match_end = c;
        }
    }

    let match_start: isize = if match_end == 0 {
        0
    } else {
        // Walk parents back until we reach the first row or column.
        let mut row = n;
        let mut col = match_end;
        while row > 1 && col > 1 {
            let (pr, pc) = mat.parent[mat.idx(row, col)];
            row = pr as usize;
            col = pc as usize;
        }
        // The walk can terminate on column 0 (an insertion parent from column 1),
        // making the start -1. The reference keeps that negative value: it is
        // returned verbatim as `offset`, and `slice(-1, end)` then counts from
        // the end of the target rather than clamping to 0.
        col as isize - 1
    };

    (match_start, match_end, min_distance)
}

// ---------------------------------------------------------------------------
// Bit-parallel search (unit-cost plain Levenshtein)
// ---------------------------------------------------------------------------

/// What the search forward pass stores per target column: the Myers/Hyyrö
/// vertical-delta words `Pv`/`Mv`, from which any cell cost of the search
/// DP is recoverable (see [`search_cell_cost`]).
///
/// This is the structure that replaces [`full_matrix`]'s cost + parent
/// matrices for the fast path: `2 × ⌈n/64⌉` words per column instead of
/// `24 (n+1)` bytes per column — 96× less memory at 1024×1024 — because
/// under unit costs the full cost column is redundant with its own delta
/// bits, and the parent matrix is redundant with the costs (see
/// [`search_bits`]'s backtrack).
struct SearchColumns {
    /// Concatenated per-column `Pv` words, laid out `[(c − 1) · blocks + b]`
    /// for `c` in `1..=m` (column 0 needs no storage: its costs are the
    /// boundary `D[r][0] = r`).
    col_pv: Vec<u64>,
    /// Same layout for `Mv`.
    col_mv: Vec<u64>,
    /// Words per column: `⌈n/64⌉`.
    blocks: usize,
    /// First column attaining the minimum of the last row — the reference's
    /// first-minimum scan (`>`, so ties keep the earliest column), with
    /// column 0's cost `n` included ahead of the scanned columns.
    match_end: usize,
    /// That minimum. An integer (unit costs), converted to `f64` at the
    /// boundary — lossless, values are bounded by `n + m`.
    min_distance: i64,
}

/// Bit-parallel evaluation of unit-cost plain-Levenshtein search:
/// [`search_full_matrix`]'s exact `(match_start, match_end, distance)`
/// tuple — same first-minimum tie-breaking, same backtrack parents — from a
/// Myers/Hyyrö forward pass plus a cost-recomputing backtrack, with **no**
/// parent matrix.
///
/// Two observations make exact parity possible without one:
///
/// 1. The stored parent of every cell is a *pure function* of the three
///    neighbour costs: candidate order insert → delete → substitute, first
///    strict `<` wins ([`full_matrix`]'s pinned tie-break). So the
///    backtrack can recompute each parent choice from cell costs alone.
/// 2. Under unit costs any cell cost is recoverable from the per-column
///    vertical deltas the forward kernel already produces:
///    `D[r][c] = Σ_{i<r} (Pv[c] bit i) − (Mv[c] bit i)` (row 0 is 0
///    everywhere in search mode; column 0 is `r`). Storing `Pv`/`Mv` per
///    column is enough.
///
/// The forward pass is the crate's existing Hyyrö block kernel
/// ([`bit_vector_distance_blocks`]) with exactly one change: the
/// per-column horizontal carry-in is 0 instead of 1, because search mode's
/// row 0 is free (`D[0][j] − D[0][j−1] = 0`, Sellers' boundary) where
/// distance mode's costs `+1` per column. A single-word specialisation
/// covers `n ≤ 64` the way [`bit_vector_distance`] does for distance mode.
///
/// Requires non-empty operands (gated in [`search_generic`]). Verified
/// against the full-matrix oracle on full-`SearchResult` equality —
/// substring, `f64` distance bits, signed offset — across randomized
/// corpora with embedded near-matches forcing real ties, both unit types.
fn search_bits<T: BitPeq>(source: &[T], target: &[T]) -> (isize, usize, f64) {
    let n = source.len();
    let m = target.len();
    debug_assert!(n >= 1 && m >= 1);

    let fw = if n <= 64 {
        search_forward_word(source, target)
    } else {
        search_forward_blocks(source, target)
    };
    let match_end = fw.match_end;

    let match_start: isize = if match_end == 0 {
        0
    } else {
        // The parent walk, with each parent recomputed instead of loaded.
        // Stops exactly where the full-matrix walk stops (`row > 1 &&
        // col > 1`), so parents of row-1/column-1 cells — which the matrix
        // records but never reads — need no recomputation either.
        let mut row = n;
        let mut col = match_end;
        while row > 1 && col > 1 {
            let insert = search_cell_cost(&fw, row, col - 1) + 1;
            let delete = search_cell_cost(&fw, row - 1, col) + 1;
            let substitute = search_cell_cost(&fw, row - 1, col - 1)
                + i64::from(source[row - 1] != target[col - 1]);

            // Candidate order insert → delete → substitute; first strict
            // minimum wins — byte-for-byte the comparison sequence
            // `full_matrix` used to pick the parent it stored.
            let mut best = insert;
            let mut parent = (row, col - 1);
            if delete < best {
                best = delete;
                parent = (row - 1, col);
            }
            if substitute < best {
                parent = (row - 1, col - 1);
            }
            (row, col) = parent;
        }
        col as isize - 1
    };

    (match_start, match_end, fw.min_distance as f64)
}

/// Single-word (`n ≤ 64`) search forward pass: [`bit_vector_distance`]'s
/// Myers kernel with the search boundary (horizontal carry-in 0 — the
/// `| 1` after the `ph` shift is the one deliberate omission) and a
/// per-column store of `Pv`/`Mv`.
///
/// `pv` starts at `u64::MAX` rather than the distance kernel's masked
/// `(1 << n) − 1`: bits at positions ≥ n are junk, but harmlessly so — the
/// kernel itself only inspects `last_bit` (position `n − 1`), carries in
/// the `wrapping_add` only propagate upward, and [`search_cell_cost`] reads
/// masked prefixes of at most `n` bits. Pinned by the exhaustive
/// cell-cost test against the scalar DP.
fn search_forward_word<T: BitPeq>(source: &[T], target: &[T]) -> SearchColumns {
    let n = source.len();
    let m = target.len();
    debug_assert!((1..=64).contains(&n));

    let peq = T::peq1(source);
    let last_bit = 1u64 << (n - 1);
    let mut pv: u64 = u64::MAX;
    let mut mv: u64 = 0;
    let mut score = n as i64;

    let mut col_pv = vec![0u64; m];
    let mut col_mv = vec![0u64; m];

    // First-minimum scan, replicated exactly: min starts at n + m with
    // match_end = m, then column 0 (cost n, always an improvement) is
    // considered before any scanned column.
    let mut min_distance = (n + m) as i64;
    let mut match_end = m;
    if min_distance > n as i64 {
        min_distance = n as i64;
        match_end = 0;
    }

    for j in 1..=m {
        let eq = T::peq1_get(&peq, target[j - 1]);
        let xv = eq | mv;
        let xh = (((eq & pv).wrapping_add(pv)) ^ pv) | eq;
        let mut ph = mv | !(xh | pv);
        let mut mh = pv & xh;

        if ph & last_bit != 0 {
            score += 1;
        }
        if mh & last_bit != 0 {
            score -= 1;
        }

        // Search boundary: carry-in 0 (row 0 free), vs `| 1` in distance mode.
        ph <<= 1;
        mh <<= 1;
        pv = mh | !(xv | ph);
        mv = ph & xv;

        col_pv[j - 1] = pv;
        col_mv[j - 1] = mv;

        if min_distance > score {
            min_distance = score;
            match_end = j;
        }
    }

    SearchColumns {
        col_pv,
        col_mv,
        blocks: 1,
        match_end,
        min_distance,
    }
}

/// Multi-word (`n > 64`) search forward pass: [`bit_vector_distance_blocks`]'s
/// Hyyrö block kernel with the search boundary (`hp_carry_in` starts each
/// column at `false` instead of `true` — row 0 is free) and a per-column
/// store of the `Pv`/`Mv` block vectors.
///
/// Callable for any `n ≥ 1` (with one block the formulation degenerates to
/// the single-word one) — kept so the tests can pit the two shapes against
/// each other on the shared domain, exactly as the OSA kernels do.
fn search_forward_blocks<T: BitPeq>(source: &[T], target: &[T]) -> SearchColumns {
    const WORD: usize = 64;
    let n = source.len();
    let m = target.len();
    debug_assert!(n >= 1);

    let blocks = n.div_ceil(WORD);
    let last_block = blocks - 1;
    let last_bit = 1u64 << ((n - 1) % WORD);

    let peq = T::peqn(source, blocks);
    let zeros = vec![0u64; blocks];

    let mut pv = vec![u64::MAX; blocks];
    let mut mv = vec![0u64; blocks];
    let mut score = n as i64;

    let mut col_pv = vec![0u64; m * blocks];
    let mut col_mv = vec![0u64; m * blocks];

    let mut min_distance = (n + m) as i64;
    let mut match_end = m;
    if min_distance > n as i64 {
        min_distance = n as i64;
        match_end = 0;
    }

    for j in 1..=m {
        let row = T::peqn_row(&peq, target[j - 1]).unwrap_or(&zeros);

        // Search boundary: D[0][j] − D[0][j−1] = 0 (row 0 free), so the
        // per-column horizontal carry-in is 0, not distance mode's +1.
        let mut hp_carry_in = false;
        let mut hn_carry_in = false;

        for (b, &eq) in row.iter().enumerate() {
            let x = eq | u64::from(hn_carry_in);
            let d0 = ((x & pv[b]).wrapping_add(pv[b]) ^ pv[b]) | x | mv[b];

            let mut hp = mv[b] | !(d0 | pv[b]);
            let mut hn = d0 & pv[b];

            let (hp_carry_out, hn_carry_out) = if b == last_block {
                (hp & last_bit != 0, hn & last_bit != 0)
            } else {
                (hp & (1u64 << 63) != 0, hn & (1u64 << 63) != 0)
            };
            if b == last_block {
                score += i64::from(hp_carry_out) - i64::from(hn_carry_out);
            }

            hp = (hp << 1) | u64::from(hp_carry_in);
            hn = (hn << 1) | u64::from(hn_carry_in);

            pv[b] = hn | !(d0 | hp);
            mv[b] = hp & d0;

            hp_carry_in = hp_carry_out;
            hn_carry_in = hn_carry_out;
        }

        let base = (j - 1) * blocks;
        col_pv[base..base + blocks].copy_from_slice(&pv);
        col_mv[base..base + blocks].copy_from_slice(&mv);

        if min_distance > score {
            min_distance = score;
            match_end = j;
        }
    }

    SearchColumns {
        col_pv,
        col_mv,
        blocks,
        match_end,
        min_distance,
    }
}

/// Cell cost `D[r][c]` of the unit-cost search DP, recovered from the
/// stored column deltas: a prefix popcount over the first `r` bits of
/// column `c`'s `Pv` minus the same over `Mv` — the definition of Myers'
/// vertical deltas (`D[i][c] − D[i−1][c] = Pv bit − Mv bit`) telescoped
/// from `D[0][c] = 0` (search mode's free row 0). Column 0 is the plain
/// deletion boundary `r`.
///
/// `count_ones` here, unlike the kernels' deliberate POPCNT avoidance
/// elsewhere: the backtrack runs `O(n + m)` of these against the forward
/// pass's `O(n·m/64)` column work, so even baseline-x86-64's expanded
/// `count_ones` sequence is off the critical path.
fn search_cell_cost(fw: &SearchColumns, r: usize, c: usize) -> i64 {
    if r == 0 {
        return 0;
    }
    if c == 0 {
        return r as i64;
    }
    let base = (c - 1) * fw.blocks;
    let pv = &fw.col_pv[base..base + fw.blocks];
    let mv = &fw.col_mv[base..base + fw.blocks];
    let full = r / 64;
    let mut d = 0i64;
    for (&p, &m_word) in pv[..full].iter().zip(&mv[..full]) {
        d += i64::from(p.count_ones()) - i64::from(m_word.count_ones());
    }
    let rem = r % 64;
    if rem > 0 {
        let mask = (1u64 << rem) - 1;
        d += i64::from((pv[full] & mask).count_ones()) - i64::from((mv[full] & mask).count_ones());
    }
    d
}

/// `String.prototype.slice` semantics for a unit slice.
///
/// A negative `start` is taken relative to the end and clamped at 0; an
/// exhausted or inverted range yields an empty slice.
fn slice_units<T>(units: &[T], start: isize, end: usize) -> &[T] {
    let len = units.len();
    let s = if start < 0 {
        (len as isize + start).max(0) as usize
    } else {
        (start as usize).min(len)
    };
    let e = end.min(len);
    if s >= e { &[] } else { &units[s..e] }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lev(a: &str, b: &str) -> f64 {
        levenshtein(a, b, &Options::default())
    }

    #[test]
    fn classic_distances() {
        assert_eq!(lev("kitten", "sitting"), 3.0);
        assert_eq!(lev("saturday", "sunday"), 3.0);
        assert_eq!(lev("", ""), 0.0);
        assert_eq!(lev("abc", ""), 3.0);
        assert_eq!(lev("", "abc"), 3.0);
        assert_eq!(lev("same", "same"), 0.0);
    }

    #[test]
    fn transposition_only_counts_for_damerau() {
        let o = Options::default();
        assert_eq!(levenshtein("ab", "ba", &o), 2.0);
        assert_eq!(damerau_levenshtein("ab", "ba", &o), 1.0);
    }

    #[test]
    fn restricted_and_unrestricted_damerau_differ() {
        let unrestricted = Options {
            restricted: false,
            ..Options::default()
        };
        let restricted = Options {
            restricted: true,
            ..Options::default()
        };
        // "ca" -> "abc" is 2 under unrestricted Damerau but 3 under the
        // restricted (optimal string alignment) rule.
        assert_eq!(damerau_levenshtein("ca", "abc", &unrestricted), 2.0);
        assert_eq!(damerau_levenshtein("ca", "abc", &restricted), 3.0);
    }

    #[test]
    fn asymmetric_costs_are_respected() {
        let o = Options {
            deletion_cost: 3.0,
            ..Options::default()
        };
        // "abc" -> "ab" is one deletion.
        assert_eq!(levenshtein("abc", "ab", &o), 3.0);
        // "ab" -> "abc" is one insertion, still cost 1.
        assert_eq!(levenshtein("ab", "abc", &o), 1.0);
    }

    #[test]
    fn fractional_and_zero_costs() {
        let frac = Options {
            insertion_cost: 0.5,
            deletion_cost: 1.5,
            substitution_cost: 0.75,
            ..Options::default()
        };
        assert_eq!(levenshtein("ab", "abc", &frac), 0.5);

        let zero = Options {
            insertion_cost: 0.0,
            deletion_cost: 0.0,
            substitution_cost: 0.0,
            ..Options::default()
        };
        assert_eq!(levenshtein("kitten", "sitting", &zero), 0.0);
    }

    #[test]
    fn utf16_semantics_match_the_reference() {
        // The headline case: the reference sees lengths 4 and 2, so the answer is 2, not 1.
        assert_eq!(lev("a😀b", "ab"), 2.0);
        assert_eq!(lev("😀", ""), 2.0);
        assert_eq!(lev("😀", "😀"), 0.0);
    }

    #[test]
    fn bmp_non_ascii_is_one_unit_per_char() {
        assert_eq!(lev("café", "cafe"), 1.0);
        assert_eq!(lev("Москва", "Москва"), 0.0);
    }

    #[test]
    fn search_finds_best_substring() {
        let r = levenshtein_search("ca", "abc", &Options::default());
        assert_eq!(r.substring, "a");
        assert_eq!(r.distance, 1.0);
        assert_eq!(r.offset, 0);
    }

    #[test]
    fn two_row_and_full_matrix_agree() {
        // The row-based fast paths must not diverge from the matrix they replace.
        let words = [
            "kitten", "sitting", "flaw", "lawn", "", "a", "abcdef", "fedcba",
        ];
        for a in words {
            for b in words {
                for restricted in [false, true] {
                    let o = Options {
                        restricted,
                        ..Options::default()
                    };
                    let fast = distance_impl(a, b, &o, restricted);
                    let slow = dispatch(a, b, |ops| match ops {
                        Operands::Bytes(s, t) => {
                            full_matrix(s, t, &o, restricted, false).final_cost()
                        }
                        Operands::Units(s, t) => {
                            full_matrix(s, t, &o, restricted, false).final_cost()
                        }
                    });
                    assert_eq!(fast, slow, "{a:?} vs {b:?} restricted={restricted}");
                }
            }
        }
    }

    /// A tiny, dependency-free xorshift64 PRNG -- deterministic (fixed seed,
    /// so failures reproduce) and good enough for generating adversarial
    /// random strings, not for anything security-sensitive.
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

    /// A random ASCII string of `len` characters drawn from a small alphabet
    /// (deliberately narrow -- a handful of repeated symbols stresses Peq's
    /// hashmap-collision handling in [`bit_vector_distance`] far harder than
    /// a wide, mostly-distinct alphabet would).
    fn random_string(rng: &mut Xorshift64, len: usize) -> String {
        const ALPHABET: &[u8] = b"abcde";
        (0..len)
            .map(|_| ALPHABET[rng.next_range(ALPHABET.len())] as char)
            .collect()
    }

    #[test]
    fn bit_vector_agrees_with_plain_rows_on_random_pairs() {
        // The correctness-defining property test for `bit_vector_distance`
        // (Myers' algorithm): it must return exactly what the already-
        // trusted scalar DP (`plain_rows`) returns, for every input it
        // actually handles -- not just a handful of hand-picked examples.
        // Lengths deliberately straddle every branch boundary the
        // implementation has: 0 and 1 (the smallest cases), 63/64/65 (the
        // single-word/needs-fallback boundary), and a size well past it.
        let mut rng = Xorshift64(0x5EED_F00D_C0FF_EE42);
        let lengths = [0usize, 1, 2, 5, 30, 63, 64, 65, 100, 200];
        let opts = Options::default();

        for &a_len in &lengths {
            for &b_len in &lengths {
                for _ in 0..20 {
                    let a = random_string(&mut rng, a_len);
                    let b = random_string(&mut rng, b_len);

                    let via_fast_path = distance_impl(&a, &b, &opts, false);
                    // `plain_rows` directly, bypassing `plain_levenshtein`'s
                    // dispatch entirely -- the independent baseline.
                    let via_plain_rows = dispatch(&a, &b, |ops| match ops {
                        Operands::Bytes(s, t) => plain_rows(s, t, &opts),
                        Operands::Units(s, t) => plain_rows(s, t, &opts),
                    });

                    assert_eq!(
                        via_fast_path, via_plain_rows,
                        "mismatch for {a:?} (len {a_len}) vs {b:?} (len {b_len})"
                    );
                }
            }
        }
    }

    #[test]
    fn bit_vector_agrees_on_utf16_input() {
        // Same property, forced through the `Operands::Units` (u16) path --
        // `bit_vector_distance`'s `T: Unit + Hash` bound is exercised for
        // both monomorphizations, not just u8.
        let mut rng = Xorshift64(0x1234_5678_9ABC_DEF0);
        let opts = Options::default();
        let pairs = [
            ("café", "cafe"),
            ("Москва", "Масква"),
            ("😀😀😀", "😀"),
            ("a😀b😀c", "abc"),
        ];
        for (a, b) in pairs {
            let via_fast_path = levenshtein(a, b, &opts);
            let via_plain_rows = dispatch(a, b, |ops| match ops {
                Operands::Bytes(s, t) => plain_rows(s, t, &opts),
                Operands::Units(s, t) => plain_rows(s, t, &opts),
            });
            assert_eq!(via_fast_path, via_plain_rows, "mismatch for {a:?} vs {b:?}");
        }
        // A batch of random Cyrillic (non-ASCII, forces the u16 path) pairs.
        const CYRILLIC: &[char] = &['а', 'б', 'в', 'г', 'д'];
        for _ in 0..50 {
            let a_len = rng.next_range(70);
            let b_len = rng.next_range(70);
            let a: String = (0..a_len)
                .map(|_| CYRILLIC[rng.next_range(CYRILLIC.len())])
                .collect();
            let b: String = (0..b_len)
                .map(|_| CYRILLIC[rng.next_range(CYRILLIC.len())])
                .collect();
            let via_fast_path = levenshtein(&a, &b, &opts);
            let via_plain_rows = dispatch(&a, &b, |ops| match ops {
                Operands::Bytes(s, t) => plain_rows(s, t, &opts),
                Operands::Units(s, t) => plain_rows(s, t, &opts),
            });
            assert_eq!(via_fast_path, via_plain_rows, "mismatch for {a:?} vs {b:?}");
        }
    }

    #[test]
    fn utf8_affix_pretrim_matches_the_utf16_oracle() {
        let opts = Options::default();
        let mut cases = Vec::new();

        let base = "аб😀中".repeat(100);
        let mut changed: Vec<char> = base.chars().collect();
        changed[200] = 'ж';
        cases.push((base.clone(), changed.into_iter().collect::<String>()));
        cases.push((base.clone(), base));

        // These pairs share UTF-8 continuation bytes inside their differing
        // final scalar. A byte-only trim would create invalid slices; the
        // pretrim must retreat to a char boundary before UTF-16 encoding.
        cases.push((
            format!("{}é", "x".repeat(65)),
            format!("{}©", "x".repeat(65)),
        ));
        cases.push((
            format!("{}😀", "д".repeat(40)),
            format!("{}😁", "д".repeat(40)),
        ));

        for (source, target) in cases {
            let actual = levenshtein(&source, &target, &opts);
            let expected = dispatch(&source, &target, |ops| match ops {
                Operands::Bytes(s, t) => plain_rows(s, t, &opts),
                Operands::Units(s, t) => plain_rows(s, t, &opts),
            });
            assert_eq!(actual, expected, "{source:?} -> {target:?}");
        }
    }

    #[test]
    fn bit_vector_fast_path_only_applies_to_unit_cost() {
        // Weighted costs must never take the bit-vector path -- it has no
        // formulation for them. Confirms the two dispatch paths still agree
        // where `is_unit_cost` is false, i.e. that nothing was silently
        // broken in `plain_rows` itself by this change.
        let weighted = Options {
            insertion_cost: 2.0,
            ..Options::default()
        };
        assert_eq!(levenshtein("abc", "ab", &weighted), 1.0); // one deletion, cost 1
        assert_eq!(levenshtein("ab", "abc", &weighted), 2.0); // one insertion, cost 2
    }

    #[test]
    fn one_row_weighted_matches_two_row_oracle_bit_for_bit() {
        // The rolling-row rewrite is an allocation/layout optimization only.
        // Keep the retired two-row recurrence as an independent test oracle,
        // including rectangular, empty and UTF-16 inputs and unusual costs
        // whose IEEE-754 evaluation order is observable.
        let options = [
            Options::default(),
            Options {
                insertion_cost: 0.5,
                deletion_cost: 1.5,
                substitution_cost: 0.75,
                ..Options::default()
            },
            Options {
                insertion_cost: 0.0,
                deletion_cost: 0.0,
                substitution_cost: 0.0,
                ..Options::default()
            },
            Options {
                insertion_cost: -1.0,
                deletion_cost: -0.5,
                substitution_cost: 2.0,
                ..Options::default()
            },
            Options {
                insertion_cost: f64::INFINITY,
                deletion_cost: 1.0,
                substitution_cost: 0.25,
                ..Options::default()
            },
            Options {
                insertion_cost: f64::NAN,
                deletion_cost: 1.0,
                substitution_cost: 0.25,
                ..Options::default()
            },
        ];
        let pairs = [
            ("", ""),
            ("", "a😀b"),
            ("a😀b", ""),
            ("a", "abcdefghijklmnopqrstuvwxyz"),
            ("abcdefghijklmnopqrstuvwxyz", "a"),
            ("kitten", "sitting"),
            ("Москва", "Масква"),
            ("😀😃😄", "😃😄😁"),
        ];

        for opts in options {
            for (source, target) in pairs {
                let (actual, expected) = dispatch(source, target, |ops| match ops {
                    Operands::Bytes(s, t) => {
                        (plain_rows(s, t, &opts), plain_rows_two_oracle(s, t, &opts))
                    }
                    Operands::Units(s, t) => {
                        (plain_rows(s, t, &opts), plain_rows_two_oracle(s, t, &opts))
                    }
                });
                if expected.is_nan() {
                    assert!(actual.is_nan(), "{source:?} -> {target:?}, {opts:?}");
                } else {
                    assert_eq!(
                        actual.to_bits(),
                        expected.to_bits(),
                        "{source:?} -> {target:?}, {opts:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn empty_plain_distance_matches_the_row_recurrence_bit_for_bit() {
        // `distance_impl` handles these before UTF-16 materialization. The
        // repeated additions (rather than `len as f64 * cost`) intentionally
        // retain the scalar recurrence's rounding and special-value behavior.
        for cost in [0.0, -0.0, 0.1, -0.5, f64::INFINITY, f64::NAN] {
            let insert = Options {
                insertion_cost: cost,
                ..Options::default()
            };
            let expected_insert = dispatch("", "a😀b", |ops| match ops {
                Operands::Bytes(s, t) => plain_rows_two_oracle(s, t, &insert),
                Operands::Units(s, t) => plain_rows_two_oracle(s, t, &insert),
            });
            let actual_insert = levenshtein("", "a😀b", &insert);

            let delete = Options {
                deletion_cost: cost,
                ..Options::default()
            };
            let expected_delete = dispatch("a😀b", "", |ops| match ops {
                Operands::Bytes(s, t) => plain_rows_two_oracle(s, t, &delete),
                Operands::Units(s, t) => plain_rows_two_oracle(s, t, &delete),
            });
            let actual_delete = levenshtein("a😀b", "", &delete);

            for (actual, expected) in [
                (actual_insert, expected_insert),
                (actual_delete, expected_delete),
            ] {
                if expected.is_nan() {
                    assert!(actual.is_nan());
                } else {
                    assert_eq!(actual.to_bits(), expected.to_bits());
                }
            }
        }
    }

    /// Random ASCII bytes drawn from a small alphabet, as a direct `Vec<u8>`
    /// rather than a `String` -- lets tests call `bit_vector_distance`/
    /// `bit_vector_distance_blocks` directly on unit slices instead of
    /// going through `dispatch`.
    fn random_units(rng: &mut Xorshift64, len: usize) -> Vec<u8> {
        const ALPHABET: &[u8] = b"abcde";
        (0..len)
            .map(|_| ALPHABET[rng.next_range(ALPHABET.len())])
            .collect()
    }

    #[test]
    fn bit_vector_blocks_agrees_with_plain_rows_on_random_pairs() {
        // The correctness-defining property test for
        // `bit_vector_distance_blocks`, mirroring
        // `bit_vector_agrees_with_plain_rows_on_random_pairs` above but with
        // lengths chosen to straddle every *block* boundary (multiples of
        // 64, +/- 1) rather than the single-word one -- exactly the
        // territory a naive per-block reapplication of the single-word
        // formula would get wrong.
        let mut rng = Xorshift64(0xB10C_5EED_1234_5678);
        let lengths = [
            65usize, 127, 128, 129, 191, 192, 193, 255, 256, 257, 500, 1000,
        ];
        let opts = Options::default();

        for &a_len in &lengths {
            for &b_len in &lengths {
                for _ in 0..3 {
                    let a = random_string(&mut rng, a_len);
                    let b = random_string(&mut rng, b_len);

                    let via_fast_path = distance_impl(&a, &b, &opts, false);
                    let via_plain_rows = dispatch(&a, &b, |ops| match ops {
                        Operands::Bytes(s, t) => plain_rows(s, t, &opts),
                        Operands::Units(s, t) => plain_rows(s, t, &opts),
                    });

                    assert_eq!(
                        via_fast_path, via_plain_rows,
                        "mismatch for len {a_len} vs len {b_len}"
                    );
                }
            }
        }
    }

    #[test]
    fn bit_vector_blocks_skips_absent_prefix_without_changing_state() {
        let opts = Options::default();
        for pattern_len in [65usize, 129, 257] {
            let shorter = vec![b'z'; pattern_len];
            for prefix_len in [0usize, 1, 63, 64, 65, 127, 128, 300, 1_000] {
                let mut longer = vec![b'a'; prefix_len];
                longer.push(b'z');
                longer.extend_from_slice(b"bbb");
                assert_eq!(
                    bit_vector_distance_blocks(&shorter, &longer),
                    plain_rows(&shorter, &longer, &opts),
                    "pattern={pattern_len}, absent prefix={prefix_len}"
                );
            }

            // Exercise the all-disjoint return with both operand length
            // orders; the public dispatcher normally supplies the longer
            // target, while the kernel itself remains robust in direct tests.
            for target_len in [1usize, pattern_len, pattern_len * 2] {
                let longer = vec![b'a'; target_len];
                assert_eq!(
                    bit_vector_distance_blocks(&shorter, &longer),
                    plain_rows(&shorter, &longer, &opts),
                    "disjoint pattern={pattern_len}, target={target_len}"
                );
            }
        }
    }

    #[test]
    fn bit_vector_blocks_agrees_with_plain_rows_when_longer_is_much_bigger() {
        // A shorter, `plain_rows`-only-reasonable check that the pattern
        // (shorter operand) can sit at a handful of block counts while the
        // scanned operand (`longer`) is far larger than any single test
        // above -- exercises many rows of block-carry propagation, not just
        // many blocks.
        let mut rng = Xorshift64(0xFEED_0BAD_C0FF_EE99);
        let opts = Options::default();
        for &shorter_len in &[65usize, 130, 260] {
            for _ in 0..2 {
                let a = random_string(&mut rng, shorter_len);
                let b = random_string(&mut rng, 5000);
                let via_fast_path = distance_impl(&a, &b, &opts, false);
                let via_plain_rows = dispatch(&a, &b, |ops| match ops {
                    Operands::Bytes(s, t) => plain_rows(s, t, &opts),
                    Operands::Units(s, t) => plain_rows(s, t, &opts),
                });
                assert_eq!(
                    via_fast_path, via_plain_rows,
                    "mismatch for shorter_len {shorter_len} vs longer_len 5000"
                );
            }
        }
    }

    #[test]
    fn bit_vector_blocks_agrees_with_bit_vector_distance_at_the_boundary() {
        // `bit_vector_distance` (single-word) and `bit_vector_distance_blocks`
        // (multi-word) are two independently-shaped implementations of the
        // same algorithm. Wherever both *could* apply (shorter operand
        // 8..=64 units), they must agree exactly, called directly against
        // each other -- not just each independently against `plain_rows`,
        // since two different bugs could each independently happen to still
        // agree with `plain_rows` on the specific random inputs a fuzz test
        // draws. This closes that gap at essentially zero extra cost.
        let mut rng = Xorshift64(0xC0DE_FACE_0BAD_F00D);
        for shorter_len in 8usize..=64 {
            for _ in 0..10 {
                let longer_len = rng.next_range(300).max(1);
                let shorter = random_units(&mut rng, shorter_len);
                let longer = random_units(&mut rng, longer_len);

                let via_single = bit_vector_distance(&shorter, &longer);
                let via_blocks = bit_vector_distance_blocks(&shorter, &longer);

                assert_eq!(
                    via_single, via_blocks,
                    "mismatch at shorter_len={shorter_len} longer_len={longer_len}"
                );
            }
        }
    }

    #[test]
    fn bit_vector_blocks_agrees_on_utf16_input() {
        // Same property as `bit_vector_agrees_on_utf16_input`, but for
        // lengths that force the multi-block path -- `T: Unit + Hash` is
        // exercised for the `u16` monomorphization here too, not just `u8`.
        let mut rng = Xorshift64(0x9E37_79B9_7F4A_7C15);
        let opts = Options::default();
        const CYRILLIC: &[char] = &['а', 'б', 'в', 'г', 'д'];
        let lengths = [65usize, 128, 200, 500];
        for &a_len in &lengths {
            for &b_len in &lengths {
                let a: String = (0..a_len)
                    .map(|_| CYRILLIC[rng.next_range(CYRILLIC.len())])
                    .collect();
                let b: String = (0..b_len)
                    .map(|_| CYRILLIC[rng.next_range(CYRILLIC.len())])
                    .collect();
                let via_fast_path = levenshtein(&a, &b, &opts);
                let via_plain_rows = dispatch(&a, &b, |ops| match ops {
                    Operands::Bytes(s, t) => plain_rows(s, t, &opts),
                    Operands::Units(s, t) => plain_rows(s, t, &opts),
                });
                assert_eq!(via_fast_path, via_plain_rows, "mismatch for {a:?} vs {b:?}");
            }
        }
        // Astral (non-BMP) input, forcing surrogate-pair-width UTF-16 units.
        let pairs = [
            ("😀".repeat(80), "😀".repeat(79)),
            ("a😀".repeat(70), "b😀".repeat(70)),
        ];
        for (a, b) in pairs {
            let via_fast_path = levenshtein(&a, &b, &opts);
            let via_plain_rows = dispatch(&a, &b, |ops| match ops {
                Operands::Bytes(s, t) => plain_rows(s, t, &opts),
                Operands::Units(s, t) => plain_rows(s, t, &opts),
            });
            assert_eq!(via_fast_path, via_plain_rows, "mismatch for {a:?} vs {b:?}");
        }
    }

    #[test]
    fn bit_vector_blocks_matches_hand_computed_edge_cases() {
        // Hand-verifiable cases at exact block boundaries, independent of
        // the randomized tests above.
        let opts = Options::default();
        // Exactly 65 units: one full block plus a single-bit second block.
        // "a" * 65 vs "a" * 64 + "b": one substitution.
        let a = "a".repeat(65);
        let b = format!("{}b", "a".repeat(64));
        assert_eq!(levenshtein(&a, &b, &opts), 1.0);
        // Identical long strings: distance 0.
        let c = "abcde".repeat(50); // 250 units
        assert_eq!(levenshtein(&c, &c, &opts), 0.0);
        // Completely disjoint alphabets, equal length: distance == length.
        let d = "x".repeat(200);
        let e = "y".repeat(200);
        assert_eq!(levenshtein(&d, &e, &opts), 200.0);
        // One empty operand past the single-word bound: distance == length.
        let f = "z".repeat(150);
        assert_eq!(levenshtein(&f, "", &opts), 150.0);
    }

    // -----------------------------------------------------------------
    // Adversarial coverage for `bit_vector_distance_blocks`.
    //
    // Everything below computes the oracle via `plain_rows` directly
    // (bypassing `plain_levenshtein`'s dispatch entirely, exactly like the
    // property tests above), never by hand-derivation, per the review
    // brief: "compute the correct answer independently ... and assert
    // equality."
    // -----------------------------------------------------------------

    /// `plain_rows`, called directly through `dispatch` -- the independent
    /// oracle every adversarial test below checks the fast path against.
    fn oracle_plain_rows(a: &str, b: &str, opts: &Options) -> f64 {
        dispatch(a, b, |ops| match ops {
            Operands::Bytes(s, t) => plain_rows(s, t, opts),
            Operands::Units(s, t) => plain_rows(s, t, opts),
        })
    }

    #[test]
    fn bit_vector_blocks_full_vs_partial_last_block_boundaries() {
        // Exact multiples of the 64-bit block width (128, 192, ...)
        // exercise the "last block is exactly full" branch of
        // `last_bit`/carry handling; length+1 sits immediately next to it
        // in the "general, partial last block" case. A single substitution
        // is placed at every position that touches a block boundary (0,
        // 62/63/64/65, 126/127/128/129, ..., last) so any off-by-one in
        // `last_bit` or in the inter-block carry wiring shows up as a wrong
        // distance instead of staying hidden in the middle of a block.
        let mut rng = Xorshift64(0xB0BA_FACE_5EED_0001);
        let opts = Options::default();
        for &len in &[128usize, 129, 192, 193, 256, 257, 320, 321, 384, 385] {
            let base = random_string(&mut rng, len);
            let base_bytes = base.as_bytes();

            assert_eq!(levenshtein(&base, &base, &opts), 0.0, "identical len {len}");

            let mut positions: Vec<usize> = vec![0, len - 1];
            for boundary in (63..len).step_by(64) {
                positions.push(boundary);
                if boundary + 1 < len {
                    positions.push(boundary + 1);
                }
                if boundary > 0 {
                    positions.push(boundary - 1);
                }
            }
            positions.sort_unstable();
            positions.dedup();

            for pos in positions {
                let mut mutated = base_bytes.to_vec();
                // `+1 mod 5` on the `abcde` alphabet always yields a
                // different character.
                mutated[pos] = b'a' + ((mutated[pos] - b'a' + 1) % 5);
                let mutated_str = String::from_utf8(mutated).unwrap();

                let via_fast_path = levenshtein(&base, &mutated_str, &opts);
                let via_plain_rows = oracle_plain_rows(&base, &mutated_str, &opts);
                assert_eq!(
                    via_fast_path, via_plain_rows,
                    "single substitution at pos {pos} of len {len} mismatch \
                     (fast={via_fast_path}, oracle={via_plain_rows})"
                );
            }
        }
    }

    #[test]
    fn bit_vector_blocks_all_one_character_repetition() {
        // The narrowest possible alphabet (one symbol): Peq has a single
        // entry whose bitmask is either all-1s (within a block) or wholly
        // absent for every other character, which is the most extreme
        // input the addition-based D0 formula ever sees. Same-character
        // pairs of unequal length spanning several block counts (pure
        // insertion/deletion, no substitution possible) plus same-length
        // identical pairs (distance 0).
        let opts = Options::default();
        for &(len_a, len_b) in &[
            (128usize, 128usize),
            (128, 129),
            (129, 128),
            (192, 64),
            (64, 192),
            (200, 400),
            (400, 200),
            (321, 321),
            (500, 503),
        ] {
            let a = "a".repeat(len_a);
            let b = "a".repeat(len_b);
            let via_fast_path = levenshtein(&a, &b, &opts);
            let via_plain_rows = oracle_plain_rows(&a, &b, &opts);
            assert_eq!(
                via_fast_path, via_plain_rows,
                "all-'a' mismatch len_a={len_a} len_b={len_b}"
            );
        }

        // Same idea but with two different single characters throughout --
        // every position is a mismatch, and Peq for 'b' is entirely absent
        // from `a`'s pattern (falls back to `unwrap_or(0)` on every probe).
        for &(len_a, len_b) in &[(128usize, 128usize), (200, 200), (321, 321), (256, 300)] {
            let a = "a".repeat(len_a);
            let b = "b".repeat(len_b);
            let via_fast_path = levenshtein(&a, &b, &opts);
            let via_plain_rows = oracle_plain_rows(&a, &b, &opts);
            assert_eq!(
                via_fast_path, via_plain_rows,
                "all-'a' vs all-'b' mismatch len_a={len_a} len_b={len_b}"
            );
        }
    }

    #[test]
    fn bit_vector_blocks_alternating_two_characters() {
        // "abab..." vs "baba..." and shifted-by-one variants: every Peq bit
        // alternates, so pv/mv alternate too -- a classic worst case for
        // bit-parallel edit distance, and a harder stress on the
        // hashmap-backed Peq than random text (only 2 distinct keys, each
        // touched on every single probe).
        let opts = Options::default();
        for &len in &[128usize, 129, 192, 200, 256, 257, 320, 400] {
            let a: String = (0..len)
                .map(|i| if i % 2 == 0 { 'a' } else { 'b' })
                .collect();
            let b: String = (0..len)
                .map(|i| if i % 2 == 0 { 'b' } else { 'a' })
                .collect();
            let via_fast_path = levenshtein(&a, &b, &opts);
            let via_plain_rows = oracle_plain_rows(&a, &b, &opts);
            assert_eq!(
                via_fast_path, via_plain_rows,
                "alternating mismatch len {len}"
            );

            // Same alternating pattern, but `b` is one unit longer -- forces
            // the phase shift to interact with a length difference too.
            let b_longer = format!("{b}a");
            let via_fast_path2 = levenshtein(&a, &b_longer, &opts);
            let via_plain_rows2 = oracle_plain_rows(&a, &b_longer, &opts);
            assert_eq!(
                via_fast_path2, via_plain_rows2,
                "alternating + 1 mismatch len {len}"
            );
        }

        // A 3-symbol cycle against a 2-symbol cycle: more Peq entries,
        // still highly repetitive, unequal lengths.
        let a: String = (0..500).map(|i| ['a', 'b', 'c'][i % 3]).collect();
        let b: String = (0..480).map(|i| ['b', 'a'][i % 2]).collect();
        let via_fast_path = levenshtein(&a, &b, &opts);
        let via_plain_rows = oracle_plain_rows(&a, &b, &opts);
        assert_eq!(via_fast_path, via_plain_rows, "3-cycle vs 2-cycle mismatch");
    }

    #[test]
    fn bit_vector_blocks_disjoint_and_near_identical_multiblock() {
        let mut rng = Xorshift64(0xD15C_A5ED_9999_0001);
        let opts = Options::default();

        // Completely disjoint alphabets: every position is a mandatory
        // substitution (or a pure length-difference insert/delete once one
        // operand runs out).
        for &(len_a, len_b) in &[
            (128usize, 128usize),
            (200, 200),
            (321, 321),
            (256, 300),
            (300, 256),
        ] {
            let a = "x".repeat(len_a);
            let b = "y".repeat(len_b);
            let via_fast_path = levenshtein(&a, &b, &opts);
            let via_plain_rows = oracle_plain_rows(&a, &b, &opts);
            assert_eq!(
                via_fast_path, via_plain_rows,
                "disjoint mismatch {len_a} vs {len_b}"
            );
        }

        // Nearly identical: a shared random base with a small, scattered
        // handful of positions flipped -- the opposite stress case from
        // fully disjoint, and closest to "real" near-duplicate text.
        for &len in &[128usize, 192, 256, 320, 400] {
            let base = random_string(&mut rng, len);
            let mut mutated = base.clone().into_bytes();
            for _ in 0..5 {
                let pos = rng.next_range(len);
                let delta = 1 + rng.next_range(4) as u8;
                mutated[pos] = b'a' + ((mutated[pos] - b'a' + delta) % 5);
            }
            let mutated_str = String::from_utf8(mutated).unwrap();
            let via_fast_path = levenshtein(&base, &mutated_str, &opts);
            let via_plain_rows = oracle_plain_rows(&base, &mutated_str, &opts);
            assert_eq!(
                via_fast_path, via_plain_rows,
                "near-identical mismatch len {len}"
            );
        }
    }

    #[test]
    fn bit_vector_blocks_boundary_pattern_lengths_against_huge_targets() {
        // Pattern lengths just past every block boundary (65, 129, 193,
        // ...) combined with a `longer` operand that is itself huge --
        // exercises many rows of block-carry propagation for a pattern
        // shape the smaller tests above never reach.
        let mut rng = Xorshift64(0x8000_0001_DEAD_10CC);
        let opts = Options::default();
        for &shorter_len in &[65usize, 129, 193, 257, 321, 385] {
            for &longer_len in &[4000usize, 10_007] {
                let a = random_string(&mut rng, shorter_len);
                let b = random_string(&mut rng, longer_len);
                let via_fast_path = levenshtein(&a, &b, &opts);
                let via_plain_rows = oracle_plain_rows(&a, &b, &opts);
                assert_eq!(
                    via_fast_path, via_plain_rows,
                    "boundary pattern len {shorter_len} vs huge target len {longer_len}"
                );
            }
        }

        // Degenerate content at the same shape: an all-one-character
        // pattern against a huge, mostly-matching target with a handful of
        // scattered mismatches.
        let pattern = "q".repeat(193);
        let mut target = "q".repeat(9001).into_bytes();
        for i in (100..9000).step_by(777) {
            target[i] = b'r';
        }
        let target_str = String::from_utf8(target).unwrap();
        let via_fast_path = levenshtein(&pattern, &target_str, &opts);
        let via_plain_rows = oracle_plain_rows(&pattern, &target_str, &opts);
        assert_eq!(
            via_fast_path, via_plain_rows,
            "degenerate huge-target mismatch"
        );
    }

    #[test]
    fn bit_vector_blocks_direct_call_empty_longer() {
        // `bit_vector_distance_blocks` is never reached through the public
        // `levenshtein` entry point when either operand is empty (the empty
        // operand is always the "shorter" one, so `shorter.len() > 64`
        // never holds for it) -- but the function itself places no such
        // restriction on `longer`, so call it directly to confirm the
        // degenerate "longer is empty" case is still handled correctly:
        // `longer`'s loop body never runs, so the whole answer comes from
        // the initial `score = m`, i.e. pure deletions.
        let mut rng = Xorshift64(0xE3E4_1234_0000_AAAA);
        for &len in &[65usize, 128, 129, 300] {
            let shorter = random_units(&mut rng, len);
            let longer: Vec<u8> = vec![];
            let via_blocks = bit_vector_distance_blocks(&shorter, &longer);
            let via_plain_rows = plain_rows(&shorter, &longer, &Options::default());
            assert_eq!(
                via_blocks, via_plain_rows,
                "empty-longer mismatch len {len}"
            );
            assert_eq!(via_blocks, len as f64);
        }

        // Same edge case through the public API, at a size well past the
        // single-word bound and (for the empty operand's counterpart) well
        // into 5-digit territory.
        assert_eq!(
            levenshtein("", &"m".repeat(12_000), &Options::default()),
            12_000.0
        );
    }

    #[test]
    fn bit_vector_blocks_edit_distance_at_block_boundary() {
        // Construct a pattern/target pair whose *true* edit distance lands
        // exactly on a block-count multiple (64, 128, 192) by replacing
        // whole 64-unit spans with a disjoint alphabet -- the shape most
        // likely to walk hp/hn carry chains through several consecutive
        // blocks in a single row, since a whole block flips from "all
        // matches" to "all mismatches" at once. Self-checked: flipping k
        // whole 64-unit blocks to a disjoint character forces exactly k*64
        // substitutions for same-length operands (any insert+delete pair
        // costs 2, strictly more than the 1 a substitution costs here), so
        // the oracle itself is expected to equal `k * 64` -- checked
        // explicitly, not just trusted.
        let opts = Options::default();
        for &blocks_to_flip in &[1usize, 2, 3] {
            let total_len = 400;
            let mut longer = "a".repeat(total_len).into_bytes();
            for b in 0..blocks_to_flip {
                let start = 64 * b;
                let end = (start + 64).min(total_len);
                for byte in &mut longer[start..end] {
                    *byte = b'z';
                }
            }
            let shorter = "a".repeat(total_len);
            let longer_str = String::from_utf8(longer).unwrap();

            let via_fast_path = levenshtein(&shorter, &longer_str, &opts);
            let via_plain_rows = oracle_plain_rows(&shorter, &longer_str, &opts);
            assert_eq!(
                via_fast_path, via_plain_rows,
                "block-boundary distance mismatch, blocks_to_flip={blocks_to_flip}"
            );
            assert_eq!(via_plain_rows, (blocks_to_flip * 64) as f64);
        }
    }

    /// A SplitMix64 PRNG -- an algorithm independent of `Xorshift64` above
    /// (not just a different seed), so this test's random coverage doesn't
    /// share blind spots with the existing property tests.
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

    /// A wider ASCII alphabet than `random_string`'s deliberately-narrow
    /// `abcde` -- complements it rather than replacing it, for a
    /// differential test that wants realistic text too.
    fn random_ascii_wide(rng: &mut SplitMix64, len: usize) -> String {
        const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
        (0..len)
            .map(|_| ALPHABET[rng.next_range(ALPHABET.len())] as char)
            .collect()
    }

    /// A mix of BMP (1 UTF-16 unit) and astral (2 UTF-16 units) characters,
    /// built up to *exactly* `units` UTF-16 code units, so length-boundary
    /// arithmetic (block counts, `last_bit`) is exercised precisely rather
    /// than approximately.
    fn random_unicode_wide(rng: &mut SplitMix64, units: usize) -> String {
        const BMP: &[char] = &['а', 'б', 'в', 'ñ', 'ü', '中', '字'];
        const ASTRAL: &[char] = &['😀', '𝔘', '𝕏', '🎉'];
        let mut s = String::new();
        let mut remaining = units;
        while remaining > 0 {
            if remaining >= 2 && rng.next_range(3) == 0 {
                s.push(ASTRAL[rng.next_range(ASTRAL.len())]);
                remaining -= 2;
            } else {
                s.push(BMP[rng.next_range(BMP.len())]);
                remaining -= 1;
            }
        }
        s
    }

    #[test]
    fn bit_vector_blocks_large_scale_differential_ascii_and_utf16() {
        // A second, independent differential test: a different PRNG
        // algorithm (SplitMix64, not Xorshift64) from every other
        // randomized test in this module, a wider alphabet than the
        // deliberately-narrow `abcde` used elsewhere, explicit length pairs
        // reaching beyond 10,000 units, and both the ASCII/`u8` dispatch
        // path and the UTF-16/`u16` path (including astral characters, so
        // surrogate-pair-width units are covered at large scale too, not
        // just in the smaller dedicated astral test). Lengths are an
        // explicit fixed list rather than a random cross product so the
        // total cost of the `O(n*m)` `plain_rows` oracle stays bounded and
        // predictable even at these sizes.
        let mut rng = SplitMix64(0x243F_6A88_85A3_08D3);
        let opts = Options::default();

        let length_pairs = [
            (65usize, 70usize),
            (66, 4096),
            (127, 130),
            (200, 4096),
            (383, 500),
            (512, 10_001),
            (1000, 1050),
            (2049, 2100),
            (65, 12_345),
            (300, 10_007),
        ];

        for &(shorter_len, longer_len) in &length_pairs {
            let a = random_ascii_wide(&mut rng, shorter_len);
            let b = random_ascii_wide(&mut rng, longer_len);
            let via_fast_path = levenshtein(&a, &b, &opts);
            let via_plain_rows = oracle_plain_rows(&a, &b, &opts);
            assert_eq!(
                via_fast_path, via_plain_rows,
                "ascii mismatch shorter_len={shorter_len} longer_len={longer_len}"
            );

            let ua = random_unicode_wide(&mut rng, shorter_len);
            let ub = random_unicode_wide(&mut rng, longer_len);
            let via_fast_path_u = levenshtein(&ua, &ub, &opts);
            let via_plain_rows_u = oracle_plain_rows(&ua, &ub, &opts);
            assert_eq!(
                via_fast_path_u, via_plain_rows_u,
                "utf16 mismatch shorter_len={shorter_len} longer_len={longer_len}"
            );
        }
    }

    // -- OSA (restricted Damerau) bit-vector battery ------------------------

    /// Unit-cost restricted (OSA) options — the fast path's gate.
    fn osa_opts() -> Options {
        Options {
            restricted: true,
            ..Options::default()
        }
    }

    /// `restricted_rows` called directly, bypassing `restricted_damerau`'s
    /// fast-path dispatch entirely — the independent scalar oracle for
    /// every OSA bit-vector test below.
    fn oracle_osa(a: &str, b: &str) -> f64 {
        let opts = osa_opts();
        dispatch(a, b, |ops| match ops {
            Operands::Bytes(s, t) => restricted_rows(s, t, &opts),
            Operands::Units(s, t) => restricted_rows(s, t, &opts),
        })
    }

    #[test]
    fn osa_bit_vector_agrees_with_restricted_rows_on_random_pairs() {
        // The correctness-defining differential test for the OSA fast
        // paths, mirroring the plain-Levenshtein batteries above: lengths
        // straddle the scalar/single-word boundary (0-3), the single-word/
        // block boundary (63/64/65), and several block boundaries. Both
        // argument orders are asserted, because the fast path swaps
        // operands (OSA under unit costs is symmetric; the oracle computes
        // whichever order it is given).
        let mut rng = Xorshift64(0x05A0_5A05_A05A);
        let opts = osa_opts();
        let lengths = [
            0usize, 1, 2, 3, 7, 8, 9, 63, 64, 65, 127, 128, 129, 191, 192, 193, 256, 500,
        ];

        for &a_len in &lengths {
            for &b_len in &lengths {
                for _ in 0..4 {
                    let a = random_string(&mut rng, a_len);
                    let b = random_string(&mut rng, b_len);
                    let expected = oracle_osa(&a, &b);
                    assert_eq!(
                        damerau_levenshtein(&a, &b, &opts),
                        expected,
                        "mismatch for len {a_len} vs len {b_len}"
                    );
                    assert_eq!(
                        damerau_levenshtein(&b, &a, &opts),
                        expected,
                        "symmetry mismatch for len {b_len} vs len {a_len}"
                    );
                }
            }
        }
    }

    #[test]
    fn osa_transposition_heavy_inputs_agree() {
        // Inputs built from adjacent swaps specifically stress the `tr`
        // register pair -- the one part of the kernels plain Levenshtein
        // never exercises.
        let mut rng = Xorshift64(0x7A57_A57A_57A5);
        let opts = osa_opts();
        for &len in &[10usize, 30, 64, 65, 100, 200, 300] {
            for _ in 0..10 {
                let a = random_string(&mut rng, len);
                let mut b: Vec<char> = a.chars().collect();
                let swaps = 1 + rng.next_range(len / 2);
                for _ in 0..swaps {
                    let i = rng.next_range(len - 1);
                    b.swap(i, i + 1);
                }
                let b: String = b.into_iter().collect();
                assert_eq!(
                    damerau_levenshtein(&a, &b, &opts),
                    oracle_osa(&a, &b),
                    "mismatch for len {len} after {swaps} adjacent swaps"
                );
            }
        }
    }

    #[test]
    fn osa_block_boundary_transpositions_agree() {
        // A transposition straddling a 64-unit word boundary is carried by
        // the `(((!below_prev_d0) & below_pm) >> 63)` term and by nothing
        // else -- a bug there is invisible to interior-swap tests, so the
        // straddle positions are pinned explicitly.
        let mut rng = Xorshift64(0xB0B0_B0B0_B0B0);
        let opts = osa_opts();
        for &len in &[130usize, 200, 260] {
            for &boundary in &[63usize, 127, 191] {
                if boundary + 1 >= len {
                    continue;
                }
                for _ in 0..5 {
                    let a = random_string(&mut rng, len);
                    let mut b: Vec<char> = a.chars().collect();
                    b.swap(boundary, boundary + 1);
                    let b: String = b.into_iter().collect();
                    assert_eq!(
                        damerau_levenshtein(&a, &b, &opts),
                        oracle_osa(&a, &b),
                        "mismatch for len {len}, swap at ({boundary},{})",
                        boundary + 1
                    );
                }
            }
        }
    }

    #[test]
    fn osa_alternating_and_degenerate_inputs_agree() {
        let opts = osa_opts();
        // Alternating two-symbol strings are the all-transposition worst
        // case; single-symbol repetition degenerates Peq to one dense row;
        // disjoint alphabets never match at all.
        for &len in &[63usize, 64, 65, 128, 129, 200] {
            let ab: String = "ab".chars().cycle().take(len).collect();
            let ba: String = "ba".chars().cycle().take(len).collect();
            assert_eq!(damerau_levenshtein(&ab, &ba, &opts), oracle_osa(&ab, &ba));

            let aa = "a".repeat(len);
            let bb = "b".repeat(len);
            assert_eq!(damerau_levenshtein(&aa, &bb, &opts), oracle_osa(&aa, &bb));
            assert_eq!(damerau_levenshtein(&aa, &ab, &opts), oracle_osa(&aa, &ab));
        }
        // Empty and one-unit operands stay on the scalar path (gate starts
        // at 2) but must agree regardless.
        assert_eq!(damerau_levenshtein("", "abc", &opts), 3.0);
        assert_eq!(damerau_levenshtein("a", "abc", &opts), 2.0);
    }

    #[test]
    fn osa_classic_fixtures() {
        let opts = osa_opts();
        // The OSA-vs-unrestricted discriminator: restricted forbids
        // editing between the transposed pair, so "CA" -> "ABC" costs 3
        // (unrestricted Damerau reaches it in 2).
        assert_eq!(damerau_levenshtein("CA", "ABC", &opts), 3.0);
        assert_eq!(damerau_levenshtein("CA", "AC", &opts), 1.0);
        assert_eq!(damerau_levenshtein("ab", "ba", &opts), 1.0);
        assert_eq!(damerau_levenshtein("kitten", "sitting", &opts), 3.0);
        // rapidfuzz's own 131-character block-boundary fixture
        // (`osa.rs`, `tests::simple`), reproduced verbatim: the CA/AC
        // transposition sits exactly astride the first word boundary.
        let filler = "a".repeat(64);
        let s1 = format!("a{filler}CA{filler}a");
        let s2 = format!("b{filler}AC{filler}b");
        assert_eq!(damerau_levenshtein(&s1, &s2, &opts), 3.0);
        assert_eq!(oracle_osa(&s1, &s2), 3.0);
    }

    #[test]
    fn osa_single_word_and_blocks_agree_on_the_shared_domain() {
        // Two independently-shaped implementations of the same algorithm
        // must agree wherever both apply -- direct calls, so two different
        // bugs that each happen to agree with the scalar oracle on the
        // random inputs above cannot slip through together.
        let mut rng = Xorshift64(0xC0DE_0511_0511);
        for shorter_len in 2usize..=64 {
            for _ in 0..6 {
                let longer_len = rng.next_range(300).max(1);
                let shorter = random_units(&mut rng, shorter_len);
                let longer = random_units(&mut rng, longer_len);
                assert_eq!(
                    osa_bit_vector(&shorter, &longer),
                    osa_bit_vector_blocks(&shorter, &longer),
                    "mismatch at shorter_len={shorter_len} longer_len={longer_len}"
                );
            }
        }
    }

    #[test]
    fn osa_utf16_and_astral_inputs_agree() {
        // The `u16` monomorphization (FxHashMap-backed Peq), including
        // astral input where one char is two units.
        let mut rng = Xorshift64(0x0111_0111_0111);
        let opts = osa_opts();
        const CYRILLIC: &[char] = &['\u{430}', '\u{431}', '\u{432}', '\u{433}', '\u{434}'];
        for &(a_len, b_len) in &[
            (10usize, 12usize),
            (40, 40),
            (64, 70),
            (65, 130),
            (200, 210),
        ] {
            let a: String = (0..a_len)
                .map(|_| CYRILLIC[rng.next_range(CYRILLIC.len())])
                .collect();
            let b: String = (0..b_len)
                .map(|_| CYRILLIC[rng.next_range(CYRILLIC.len())])
                .collect();
            assert_eq!(
                damerau_levenshtein(&a, &b, &opts),
                oracle_osa(&a, &b),
                "cyrillic mismatch {a_len}x{b_len}"
            );
        }
        assert_eq!(
            damerau_levenshtein(
                "\u{418}\u{432}\u{430}\u{43d}\u{43a}\u{43e}",
                "\u{41f}\u{435}\u{442}\u{440}\u{443}\u{43d}\u{43a}\u{43e}",
                &opts
            ),
            5.0
        );
        let a = "\u{1F600}".repeat(40);
        let b = format!("a{}", "\u{1F600}".repeat(39));
        assert_eq!(damerau_levenshtein(&a, &b, &opts), oracle_osa(&a, &b));
    }

    #[test]
    fn osa_weighted_costs_never_take_the_fast_path() {
        // Any non-1.0 cost (including a weighted transposition, which
        // `is_unit_cost` alone does not cover, and NaN, which fails
        // `== 1.0`) must fall back to the scalar path.
        for transposition_cost in [0.5, 2.0, 0.0, f64::NAN] {
            let opts = Options {
                restricted: true,
                transposition_cost,
                ..Options::default()
            };
            let got = damerau_levenshtein("abcd", "abdc", &opts);
            let want = dispatch("abcd", "abdc", |ops| match ops {
                Operands::Bytes(s, t) => restricted_rows(s, t, &opts),
                Operands::Units(s, t) => restricted_rows(s, t, &opts),
            });
            assert_eq!(got.to_bits(), want.to_bits());
        }
        let weighted = Options {
            restricted: true,
            insertion_cost: 2.0,
            ..Options::default()
        };
        assert_eq!(damerau_levenshtein("ab", "abc", &weighted), 2.0);
        assert_eq!(damerau_levenshtein("abc", "ab", &weighted), 1.0);
    }

    // -- Unrestricted-Damerau fast-path battery -----------------------------

    /// `full_matrix(...).final_cost()` called directly — the pinned oracle
    /// for the unit-cost unrestricted-Damerau fast path.
    fn oracle_unrestricted(a: &str, b: &str) -> f64 {
        let opts = Options::default();
        dispatch(a, b, |ops| match ops {
            Operands::Bytes(s, t) => full_matrix(s, t, &opts, true, false).final_cost(),
            Operands::Units(s, t) => full_matrix(s, t, &opts, true, false).final_cost(),
        })
    }

    #[test]
    fn damerau_unit_fast_path_matches_the_pinned_quirk_fixtures() {
        // These are the fixtures where Verbora's pinned recurrence diverges
        // from textbook Damerau-Levenshtein (strsim/rapidfuzz answer 2, 3
        // and 5 for the first three) -- the exact cases any "cleanup"
        // toward the textbook algorithm would silently break. Asymmetry is
        // pinned too: the reference recurrence is genuinely not symmetric.
        let opts = Options::default();
        assert_eq!(damerau_levenshtein("bb", "abbb", &opts), 1.0);
        assert_eq!(damerau_levenshtein("abbb", "bb", &opts), 2.0);
        assert_eq!(damerau_levenshtein("dfcb", "bdffc", &opts), 2.0);
        assert_eq!(damerau_levenshtein("aabcbbb", "cabbccaab", &opts), 3.0);
        assert_eq!(damerau_levenshtein("ca", "abc", &opts), 2.0);
        for (a, b) in [
            ("bb", "abbb"),
            ("abbb", "bb"),
            ("dfcb", "bdffc"),
            ("aabcbbb", "cabbccaab"),
            ("ca", "abc"),
        ] {
            assert_eq!(damerau_levenshtein(a, b, &opts), oracle_unrestricted(a, b));
        }
    }

    #[test]
    fn damerau_unit_fast_path_agrees_with_full_matrix_on_random_pairs() {
        // Small alphabets maximise last-row/last-column interactions and
        // match-cell transpositions -- the parts of the pinned recurrence
        // the textbook algorithm gets wrong.
        let mut rng = Xorshift64(0xDA3E_DA3E_DA3E);
        let opts = Options::default();
        let lengths = [0usize, 1, 2, 3, 5, 8, 13, 21, 34, 55, 80];
        for &a_len in &lengths {
            for &b_len in &lengths {
                for _ in 0..6 {
                    let a = random_string(&mut rng, a_len);
                    let b = random_string(&mut rng, b_len);
                    assert_eq!(
                        damerau_levenshtein(&a, &b, &opts),
                        oracle_unrestricted(&a, &b),
                        "mismatch for {a:?} vs {b:?}"
                    );
                }
            }
        }
        // A few larger sizes against the (slow) full-matrix oracle.
        for &(a_len, b_len) in &[(200usize, 210usize), (300, 40), (129, 500)] {
            let a = random_string(&mut rng, a_len);
            let b = random_string(&mut rng, b_len);
            assert_eq!(
                damerau_levenshtein(&a, &b, &opts),
                oracle_unrestricted(&a, &b),
                "mismatch at {a_len}x{b_len}"
            );
        }
    }

    #[test]
    fn damerau_unit_fast_path_agrees_on_utf16_input() {
        let mut rng = Xorshift64(0xDA3E_0016_0016);
        let opts = Options::default();
        const CYRILLIC: &[char] = &['\u{430}', '\u{431}', '\u{432}'];
        for &(a_len, b_len) in &[(5usize, 7usize), (20, 20), (40, 60), (80, 30)] {
            let a: String = (0..a_len)
                .map(|_| CYRILLIC[rng.next_range(CYRILLIC.len())])
                .collect();
            let b: String = (0..b_len)
                .map(|_| CYRILLIC[rng.next_range(CYRILLIC.len())])
                .collect();
            assert_eq!(
                damerau_levenshtein(&a, &b, &opts),
                oracle_unrestricted(&a, &b),
                "cyrillic mismatch {a_len}x{b_len}"
            );
        }
        let a = "\u{1F600}\u{1F601}\u{1F600}\u{1F601}";
        let b = "\u{1F601}\u{1F600}\u{1F601}";
        assert_eq!(damerau_levenshtein(a, b, &opts), oracle_unrestricted(a, b));
    }

    #[test]
    fn damerau_weighted_costs_never_take_the_unit_fast_path() {
        // A weighted transposition (not covered by `is_unit_cost`) and each
        // weighted base cost must all still route to `full_matrix`.
        for opts in [
            Options {
                transposition_cost: 0.5,
                ..Options::default()
            },
            Options {
                insertion_cost: 2.0,
                ..Options::default()
            },
            Options {
                transposition_cost: f64::NAN,
                ..Options::default()
            },
        ] {
            let got = damerau_levenshtein("ca", "abc", &opts);
            let want = dispatch("ca", "abc", |ops| match ops {
                Operands::Bytes(s, t) => full_matrix(s, t, &opts, true, false).final_cost(),
                Operands::Units(s, t) => full_matrix(s, t, &opts, true, false).final_cost(),
            });
            assert_eq!(got.to_bits(), want.to_bits());
        }
        // Half-cost transposition observably differs from unit cost.
        let half = Options {
            transposition_cost: 0.5,
            ..Options::default()
        };
        assert_eq!(damerau_levenshtein("ab", "ba", &half), 0.5);
    }

    // =======================================================================
    // Adversarial audit battery
    // =======================================================================

    /// Random bytes over `abcde` drawn from a SplitMix64 (the audit tests
    /// use a PRNG algorithm distinct from the pre-audit Xorshift64 ones).
    fn sm_units(rng: &mut SplitMix64, len: usize) -> Vec<u8> {
        const ALPHABET: &[u8] = b"abcde";
        (0..len)
            .map(|_| ALPHABET[rng.next_range(ALPHABET.len())])
            .collect()
    }

    fn ascii_string(units: &[u8]) -> String {
        String::from_utf8(units.to_vec()).expect("ascii")
    }

    // -- Area 1: bit-parallel OSA -------------------------------------------

    #[test]
    fn osa_offset_straddle_transpositions_agree() {
        // Transpositions placed at every word boundary of the PATTERN
        // (positions 62..65, 126..129) while a 0-2 unit prefix insertion
        // shifts the TARGET columns, so the swapped pair straddles the
        // pattern's 64-bit words at a *different* alignment than the
        // column stream — the exact shape the cross-word `tr` carry and
        // the per-word `prev_pm` register disagree on if either is wired
        // to the wrong column. The pre-audit boundary test only swapped at
        // identical positions in equal-length operands.
        let mut rng = SplitMix64(0x05A0_0FF5_E7B0_0001);
        let opts = osa_opts();
        for &len in &[130usize, 200, 300] {
            for &p in &[0usize, 1, 62, 63, 64, 65, 126, 127, 128, 129] {
                if p + 1 >= len {
                    continue;
                }
                for prefix_len in 0usize..=2 {
                    let a = sm_units(&mut rng, len);
                    let mut b = sm_units(&mut rng, prefix_len);
                    b.extend_from_slice(&a);
                    b.swap(prefix_len + p, prefix_len + p + 1);

                    // Direct kernel call against the direct scalar oracle
                    // (bypasses the operand-swapping dispatch entirely).
                    assert_eq!(
                        osa_bit_vector_blocks(&a, &b),
                        restricted_rows(&a, &b, &opts),
                        "direct blocks mismatch len={len} p={p} prefix={prefix_len}"
                    );

                    // And through the public entry, both argument orders.
                    let sa = ascii_string(&a);
                    let sb = ascii_string(&b);
                    let expected = oracle_osa(&sa, &sb);
                    assert_eq!(
                        damerau_levenshtein(&sa, &sb, &opts),
                        expected,
                        "public mismatch len={len} p={p} prefix={prefix_len}"
                    );
                    assert_eq!(
                        damerau_levenshtein(&sb, &sa, &opts),
                        expected,
                        "public reversed mismatch len={len} p={p} prefix={prefix_len}"
                    );
                }
            }
            // Swap at the very end of the pattern: the transposed pair's
            // second bit is the score bit (`last_bit`) itself.
            let a = sm_units(&mut rng, len);
            let mut b = a.clone();
            b.swap(len - 2, len - 1);
            assert_eq!(
                osa_bit_vector_blocks(&a, &b),
                restricted_rows(&a, &b, &opts),
                "tail swap mismatch len={len}"
            );
        }
    }

    #[test]
    fn osa_single_symbol_seas_with_boundary_swaps() {
        // Degenerate single-symbol strings at block scale, with a lone
        // disturbance forming an adjacent transposition exactly astride
        // each word boundary: `Peq['a']` is all-ones (dense addition
        // carries through every word), and the transposition is the only
        // structure present.
        let opts = osa_opts();
        for &len in &[65usize, 129, 193, 260] {
            for &p in &[0usize, 62, 63, 64, 127, 128, 191, 192] {
                if p + 1 >= len {
                    continue;
                }
                // s1 = ...a b a... / s2 = ...a a b...: swapping "ba" <-> "ab".
                let mut s1 = vec![b'a'; len];
                let mut s2 = vec![b'a'; len];
                s1[p] = b'b';
                s2[p + 1] = b'b';
                assert_eq!(
                    osa_bit_vector_blocks(&s1, &s2),
                    restricted_rows(&s1, &s2, &opts),
                    "sea swap mismatch len={len} p={p}"
                );

                // Two distinct symbols transposed inside the sea.
                let mut s3 = vec![b'a'; len];
                let mut s4 = vec![b'a'; len];
                s3[p] = b'b';
                s3[p + 1] = b'c';
                s4[p] = b'c';
                s4[p + 1] = b'b';
                let sa = ascii_string(&s3);
                let sb = ascii_string(&s4);
                let expected = oracle_osa(&sa, &sb);
                assert_eq!(
                    damerau_levenshtein(&sa, &sb, &opts),
                    expected,
                    "bc-sea mismatch len={len} p={p}"
                );
                assert_eq!(expected, 1.0, "a bc<->cb swap must cost exactly 1");
            }
            // Tail swap in the sea.
            let mut s1 = vec![b'a'; len];
            let mut s2 = vec![b'a'; len];
            s1[len - 2] = b'b';
            s2[len - 1] = b'b';
            assert_eq!(
                osa_bit_vector_blocks(&s1, &s2),
                restricted_rows(&s1, &s2, &opts),
                "sea tail swap mismatch len={len}"
            );
        }
    }

    #[test]
    fn osa_tiny_and_empty_operands_all_entries() {
        let opts = osa_opts();
        // Every tiny pair through the public entry against the direct
        // scalar oracle (these all route to `restricted_rows` or the
        // single-word kernel's lower gate — pinned regardless).
        let tiny = ["", "a", "b", "ab", "ba", "aa", "abc", "cba", "aab"];
        for a in tiny {
            for b in tiny {
                assert_eq!(
                    damerau_levenshtein(a, b, &opts),
                    oracle_osa(a, b),
                    "tiny mismatch {a:?} vs {b:?}"
                );
            }
        }
        // Direct kernel calls at the domain edges the dispatch never
        // exercises: an empty text against a multi-block pattern (score
        // must be exactly m, from initialisation alone), a one-unit text,
        // and the documented-callable m = 1 blocks case.
        let long = vec![b'q'; 65];
        assert_eq!(osa_bit_vector_blocks(&long, b""), 65.0);
        assert_eq!(
            osa_bit_vector_blocks(&long, b"q"),
            restricted_rows(&long, b"q", &opts)
        );
        assert_eq!(
            osa_bit_vector_blocks(b"x", b"xy"),
            restricted_rows(b"x", b"xy", &opts)
        );
        assert_eq!(
            osa_bit_vector(b"xy", b"yx"),
            restricted_rows(b"xy", b"yx", &opts)
        );
    }

    #[test]
    fn osa_large_randomized_differential_splitmix() {
        // Independent large-scale OSA differential: SplitMix64 with a
        // fresh seed, swap-and-edit mutation of a shared base (rather than
        // two independent random strings — keeps the distance small, so
        // the transposition machinery, not the substitution floor,
        // decides the answer), both argument orders, ASCII and UTF-16.
        let mut rng = SplitMix64(0x05A0_2026_0816_AAAA);
        let opts = osa_opts();
        for round in 0..120 {
            let len = 65 + rng.next_range(350);
            let a = sm_units(&mut rng, len);
            let mut b = a.clone();
            let swaps = 1 + rng.next_range(10);
            for _ in 0..swaps {
                let i = rng.next_range(b.len() - 1);
                b.swap(i, i + 1);
            }
            for _ in 0..rng.next_range(5) {
                let i = rng.next_range(b.len());
                b[i] = b"abcde"[rng.next_range(5)];
            }
            if rng.next_range(3) == 0 {
                let cut = 1 + rng.next_range(4);
                b.drain(..cut);
            }
            let sa = ascii_string(&a);
            let sb = ascii_string(&b);
            let expected = oracle_osa(&sa, &sb);
            assert_eq!(
                damerau_levenshtein(&sa, &sb, &opts),
                expected,
                "round {round} ({len})"
            );
            assert_eq!(
                damerau_levenshtein(&sb, &sa, &opts),
                expected,
                "round {round} reversed ({len})"
            );
        }

        // UTF-16 rounds with boundary-adjacent swaps on one-unit (BMP)
        // characters, so swap positions are exact unit positions.
        const BMP: &[char] = &['\u{430}', '\u{431}', '\u{432}', '\u{4E2D}'];
        for &p in &[62usize, 63, 64, 65, 127, 128] {
            let chars: Vec<char> = (0..160).map(|_| BMP[rng.next_range(BMP.len())]).collect();
            let mut swapped = chars.clone();
            swapped.swap(p, p + 1);
            let a: String = chars.into_iter().collect();
            let b: String = swapped.into_iter().collect();
            assert_eq!(
                damerau_levenshtein(&a, &b, &opts),
                oracle_osa(&a, &b),
                "utf16 swap at {p}"
            );
        }
        // Astral rounds: surrogate pairs at multi-block scale.
        let a = "\u{1F600}\u{1F601}".repeat(40); // 160 units
        let b = format!("\u{1F601}\u{1F600}{}", "\u{1F600}\u{1F601}".repeat(39));
        assert_eq!(damerau_levenshtein(&a, &b, &opts), oracle_osa(&a, &b));
    }

    // -- Area 2: BitPeq flat tables under the plain kernels -----------------

    #[test]
    fn bitpeq_full_byte_alphabet_kernels_direct() {
        // Bytes outside ASCII can never arrive through `dispatch`, but the
        // kernels are generic over `&[u8]` — call them directly with all
        // 256 byte values so the flat 256-entry index tables are exercised
        // end to end (including entries 0 and 255), not just the 5-26
        // letters every string-based test is limited to.
        let mut rng = SplitMix64(0xB17E_0000_FFFF_0001);
        let opts = Options::default();
        for &m in &[8usize, 64, 65, 200, 256, 300] {
            for _ in 0..3 {
                // Pattern: cycling + random full-range bytes, guaranteeing
                // many distinct values and re-occurrences across blocks.
                let shorter: Vec<u8> = (0..m)
                    .map(|i| {
                        if i % 2 == 0 {
                            (i % 256) as u8
                        } else {
                            (rng.next_u64() % 256) as u8
                        }
                    })
                    .collect();
                let longer: Vec<u8> = (0..600).map(|_| (rng.next_u64() % 256) as u8).collect();

                let expected = plain_rows(&shorter, &longer, &opts);
                if (8..=64).contains(&m) {
                    assert_eq!(
                        bit_vector_distance(&shorter, &longer),
                        expected,
                        "single-word full-alphabet mismatch m={m}"
                    );
                }
                assert_eq!(
                    bit_vector_distance_blocks(&shorter, &longer),
                    expected,
                    "blocks full-alphabet mismatch m={m}"
                );
            }
        }
    }

    #[test]
    fn bitpeq_u16_wide_alphabet_kernels_direct() {
        // The u16 monomorphization with far more than 256 distinct units
        // (impossible to reach via u8), so the FxHashMap-backed TableN
        // grows through hundreds of slot allocations, with recurrences
        // scattered across blocks.
        let mut rng = SplitMix64(0x0016_31DE_A1FA_0001);
        let opts = Options::default();
        for &m in &[65usize, 200, 400] {
            let shorter: Vec<u16> = (0..m)
                .map(|i| {
                    if i % 3 == 0 {
                        (i % 1000) as u16
                    } else {
                        (rng.next_u64() % 1000) as u16
                    }
                })
                .collect();
            let longer: Vec<u16> = (0..700).map(|_| (rng.next_u64() % 1000) as u16).collect();
            assert_eq!(
                bit_vector_distance_blocks(&shorter, &longer),
                plain_rows(&shorter, &longer, &opts),
                "u16 wide-alphabet blocks mismatch m={m}"
            );
        }
        // Single-word u16 kernel on the same alphabet.
        let shorter: Vec<u16> = (0..60).map(|_| (rng.next_u64() % 1000) as u16).collect();
        let longer: Vec<u16> = (0..500).map(|_| (rng.next_u64() % 1000) as u16).collect();
        assert_eq!(
            bit_vector_distance(&shorter, &longer),
            plain_rows(&shorter, &longer, &opts)
        );
    }

    #[test]
    fn bitpeq_randomized_differential_splitmix() {
        // Fresh-seed randomized differential for the BitPeq-backed plain
        // kernels: full-range bytes, lengths sweeping every word boundary
        // neighbourhood, both kernels wherever their domains apply, plus
        // the two kernels pitted directly against each other.
        let mut rng = SplitMix64(0xB17E_2026_0816_BBBB);
        let opts = Options::default();
        let lengths = [8usize, 63, 64, 65, 66, 127, 128, 129, 130, 192, 250];
        for &m in &lengths {
            for _ in 0..4 {
                let narrow = rng.next_range(2) == 0;
                let gen_byte = |rng: &mut SplitMix64| -> u8 {
                    if narrow {
                        b'a' + (rng.next_u64() % 3) as u8
                    } else {
                        (rng.next_u64() % 256) as u8
                    }
                };
                let shorter: Vec<u8> = (0..m).map(|_| gen_byte(&mut rng)).collect();
                let longer_len = 1 + rng.next_range(400);
                let longer: Vec<u8> = (0..longer_len).map(|_| gen_byte(&mut rng)).collect();
                let expected = plain_rows(&shorter, &longer, &opts);
                if (8..=64).contains(&m) {
                    assert_eq!(
                        bit_vector_distance(&shorter, &longer),
                        expected,
                        "word mismatch m={m} n={longer_len} narrow={narrow}"
                    );
                }
                assert_eq!(
                    bit_vector_distance_blocks(&shorter, &longer),
                    expected,
                    "blocks mismatch m={m} n={longer_len} narrow={narrow}"
                );
            }
        }
    }

    // -- Area 4: unrestricted-Damerau unit-cost fast path -------------------

    #[test]
    fn damerau_unit_snapshot_overwrite_stress() {
        // Structured symbol-recurrence patterns: every source symbol
        // reappears many times with varying gaps, so each row overwrites
        // some symbol's arena snapshot that later transposition candidates
        // would have read — any snapshot kept too fresh (copied after the
        // current row is computed) or too stale (first-occurrence kept
        // forever) diverges from the full-matrix oracle here.
        let opts = Options::default();
        for &k in &[2usize, 3, 5, 8, 40, 100] {
            let pairs = [
                ("ab".repeat(k), "ba".repeat(k)),
                ("aab".repeat(k), "aba".repeat(k)),
                ("abc".repeat(k), "cab".repeat(k)),
                ("ab".repeat(k), format!("b{}", "ab".repeat(k))),
                ("aabb".repeat(k), "bbaa".repeat(k)),
            ];
            for (a, b) in &pairs {
                let expected = oracle_unrestricted(a, b);
                assert_eq!(
                    damerau_levenshtein(a, b, &opts),
                    expected,
                    "structured mismatch k={k} {a:?} vs {b:?}"
                );
                let expected_rev = oracle_unrestricted(b, a);
                assert_eq!(
                    damerau_levenshtein(b, a, &opts),
                    expected_rev,
                    "structured reversed mismatch k={k}"
                );
            }
        }

        // Two-symbol random battery: an alphabet of exactly {a, b}
        // maximises last-row/last-column churn (every row overwrites one
        // of only two snapshots, and `lcm` is set on nearly every row).
        let mut rng = SplitMix64(0xDA3E_2026_0816_CCCC);
        for round in 0..200 {
            let len_a = 1 + rng.next_range(120);
            let len_b = 1 + rng.next_range(120);
            let a: String = (0..len_a)
                .map(|_| if rng.next_range(2) == 0 { 'a' } else { 'b' })
                .collect();
            let b: String = (0..len_b)
                .map(|_| if rng.next_range(2) == 0 { 'a' } else { 'b' })
                .collect();
            assert_eq!(
                damerau_levenshtein(&a, &b, &opts),
                oracle_unrestricted(&a, &b),
                "ab-random mismatch round {round} ({len_a}x{len_b})"
            );
        }
    }

    #[test]
    fn damerau_unit_degenerate_tiny_and_nul() {
        let opts = Options::default();
        // Every tiny pair through the public entry, including empties.
        let tiny = ["", "a", "b", "ab", "ba", "aa", "aba", "bab"];
        for a in tiny {
            for b in tiny {
                assert_eq!(
                    damerau_levenshtein(a, b, &opts),
                    oracle_unrestricted(a, b),
                    "tiny mismatch {a:?} vs {b:?}"
                );
            }
        }
        // Degenerate single-symbol strings at block scale.
        for (a, b) in [
            ("a".repeat(500), "a".repeat(497)),
            ("a".repeat(300), "b".repeat(300)),
            ("ab".repeat(150), "ba".repeat(150)),
            ("a".repeat(400), format!("{}b", "a".repeat(399))),
        ] {
            assert_eq!(
                damerau_levenshtein(&a, &b, &opts),
                oracle_unrestricted(&a, &b),
                "degenerate mismatch {}x{}",
                a.len(),
                b.len()
            );
        }
        // NUL bytes are valid ASCII: byte value 0 indexes the first entry
        // of the flat tables, and the 1-based row sentinel must not
        // confuse a legitimate row for "vacant".
        for (a, b) in [
            ("\0ab\0", "b\0a"),
            ("\0\0", "\0"),
            ("a\0b", "ab\0"),
            ("\0a", "a\0"),
        ] {
            assert_eq!(
                damerau_levenshtein(a, b, &opts),
                oracle_unrestricted(a, b),
                "nul mismatch {a:?} vs {b:?}"
            );
        }
    }

    #[test]
    fn damerau_unit_many_distinct_symbols() {
        let opts = Options::default();
        // u8, direct: all 256 byte values as source symbols, so every slot
        // of the flat scratch tables is allocated and the arena spans 256
        // rows. Compared against the full matrix called directly on the
        // same units.
        let mut rng = SplitMix64(0xDA3E_A1FA_BE7A_0001);
        let source: Vec<u8> = (0..300).map(|i| (i % 256) as u8).collect();
        let target: Vec<u8> = (0..310).map(|_| (rng.next_u64() % 256) as u8).collect();
        assert_eq!(
            damerau_unrestricted_unit::<_, u16>(&source, &target),
            full_matrix(&source, &target, &opts, true, false).final_cost()
        );
        assert_eq!(
            damerau_unrestricted_unit::<_, u16>(&target, &source),
            full_matrix(&target, &source, &opts, true, false).final_cost()
        );

        // u16 via the public entry: >300 distinct BMP characters, so the
        // FxHashMap scratch allocates hundreds of slots (impossible for
        // u8) and next_slot bookkeeping is stressed well past 256.
        let wide_char = |i: usize| char::from_u32(0x400 + (i % 400) as u32).unwrap();
        let a: String = (0..350).map(wide_char).collect();
        let b: String = (0..350).map(|i| wide_char(i + 7)).collect();
        assert_eq!(
            damerau_levenshtein(&a, &b, &opts),
            oracle_unrestricted(&a, &b),
            "u16 wide-alphabet mismatch"
        );
        // Astral-heavy pair: surrogate halves as recurring symbols.
        let a = "\u{1F600}\u{1F601}\u{1F602}".repeat(30);
        let b = format!(
            "\u{1F601}\u{1F600}{}",
            "\u{1F602}\u{1F601}\u{1F600}".repeat(29)
        );
        assert_eq!(
            damerau_levenshtein(&a, &b, &opts),
            oracle_unrestricted(&a, &b),
            "astral mismatch"
        );
    }

    #[test]
    fn damerau_unit_large_randomized_differential_splitmix() {
        // Fresh-seed large randomized differential for the unit-cost
        // unrestricted-Damerau fast path, both argument orders (the pinned
        // recurrence is NOT symmetric, so each order is its own case).
        let mut rng = SplitMix64(0xDA3E_2026_0816_DDDD);
        let opts = Options::default();
        const ALPHABETS: [&[u8]; 2] = [b"ab", b"abcde"];
        for round in 0..80 {
            let alphabet = ALPHABETS[round % 2];
            let len_a = 1 + rng.next_range(250);
            let len_b = 1 + rng.next_range(250);
            let a: String = (0..len_a)
                .map(|_| alphabet[rng.next_range(alphabet.len())] as char)
                .collect();
            let b: String = (0..len_b)
                .map(|_| alphabet[rng.next_range(alphabet.len())] as char)
                .collect();
            let fwd = oracle_unrestricted(&a, &b);
            let rev = oracle_unrestricted(&b, &a);
            assert_eq!(
                damerau_levenshtein(&a, &b, &opts),
                fwd,
                "round {round} ({len_a}x{len_b})"
            );
            assert_eq!(
                damerau_levenshtein(&b, &a, &opts),
                rev,
                "round {round} reversed ({len_b}x{len_a})"
            );
        }
        // A handful of u16 rounds at the same scale.
        const CYR: &[char] = &['\u{430}', '\u{431}', '\u{432}'];
        for round in 0..15 {
            let len_a = 1 + rng.next_range(150);
            let len_b = 1 + rng.next_range(150);
            let a: String = (0..len_a).map(|_| CYR[rng.next_range(CYR.len())]).collect();
            let b: String = (0..len_b).map(|_| CYR[rng.next_range(CYR.len())]).collect();
            assert_eq!(
                damerau_levenshtein(&a, &b, &opts),
                oracle_unrestricted(&a, &b),
                "u16 round {round}"
            );
        }
    }

    #[test]
    fn damerau_unit_u16_and_u32_cells_agree() {
        // The u32-cell monomorphization is only dispatched past 65,535
        // total units -- unreachable for a differential test against the
        // full-matrix oracle -- so the two cell widths are pinned directly
        // against each other (and u16 against the oracle elsewhere).
        let mut rng = Xorshift64(0xCE11_CE11_CE11);
        for &(a_len, b_len) in &[(0usize, 5usize), (7, 7), (40, 60), (128, 200), (300, 41)] {
            for _ in 0..8 {
                let a = random_units(&mut rng, a_len);
                let b = random_units(&mut rng, b_len);
                assert_eq!(
                    damerau_unrestricted_unit::<u8, u16>(&a, &b),
                    damerau_unrestricted_unit::<u8, u32>(&a, &b),
                    "cell-width mismatch at {a_len}x{b_len}"
                );
            }
        }
    }

    #[test]
    fn damerau_byte_tiers_agree_with_full_matrix_at_boundaries() {
        // The three byte kernels and their dispatch thresholds (8, 128),
        // exercised right at and across each boundary against the pinned
        // full-matrix oracle, in both argument orders (the recurrence is
        // asymmetric, so both orders are load-bearing).
        let mut rng = Xorshift64(0x71E5_71E5_71E5);
        let opts = Options::default();
        let sizes = [
            (7usize, 7usize),
            (8, 8),
            (8, 9),
            (9, 8),
            (9, 9),
            (8, 200),
            (127, 128),
            (128, 128),
            (128, 129),
            (129, 129),
            (129, 40),
            (160, 160),
            (200, 130),
        ];
        for &(a_len, b_len) in &sizes {
            for _ in 0..6 {
                let a = random_string(&mut rng, a_len);
                let b = random_string(&mut rng, b_len);
                let expected = oracle_unrestricted(&a, &b);
                assert_eq!(
                    damerau_levenshtein(&a, &b, &opts),
                    expected,
                    "mismatch at {a_len}x{b_len}"
                );
                assert_eq!(
                    damerau_levenshtein(&b, &a, &opts),
                    oracle_unrestricted(&b, &a),
                    "reverse mismatch at {b_len}x{a_len}"
                );
            }
        }
    }

    #[test]
    fn damerau_byte_tiers_agree_with_each_other_on_shared_domains() {
        // The three tier kernels are independently-shaped implementations
        // of one recurrence; wherever their domains overlap they must agree
        // exactly, called directly (not through dispatch).
        let mut rng = Xorshift64(0x3B1D_3B1D_3B1D);
        for _ in 0..300 {
            let a_len = 1 + rng.next_range(8);
            let b_len = 1 + rng.next_range(8);
            let a = random_units(&mut rng, a_len);
            let b = random_units(&mut rng, b_len);
            let small = damerau_unit_small(&a, &b);
            let mid = damerau_unit_mid(&a, &b);
            let large = damerau_unit_large(&a, &b);
            let generic = damerau_unrestricted_unit::<u8, u16>(&a, &b);
            assert_eq!(small, mid, "small/mid at {a_len}x{b_len}");
            assert_eq!(mid, large, "mid/large at {a_len}x{b_len}");
            assert_eq!(large, generic, "large/generic at {a_len}x{b_len}");
        }
        for _ in 0..100 {
            let a_len = 9 + rng.next_range(120);
            let b_len = 9 + rng.next_range(120);
            let a = random_units(&mut rng, a_len);
            let b = random_units(&mut rng, b_len);
            assert_eq!(
                damerau_unit_mid(&a, &b),
                damerau_unit_large(&a, &b),
                "mid/large at {a_len}x{b_len}"
            );
            assert_eq!(
                damerau_unit_large(&a, &b),
                damerau_unrestricted_unit::<u8, u16>(&a, &b),
                "large/generic at {a_len}x{b_len}"
            );
        }
    }

    #[test]
    fn damerau_byte_tiers_handle_quirks_and_degenerate_shapes() {
        // The pinned-recurrence quirk fixtures forced through each tier by
        // direct call (they are all small, so dispatch alone would only
        // exercise the stack kernel), plus degenerate shapes.
        for (a, b, want) in [
            ("bb", "abbb", 1.0),
            ("abbb", "bb", 2.0),
            ("dfcb", "bdffc", 2.0),
            ("aabcbbb", "cabbccaab", 3.0),
            ("ca", "abc", 2.0),
        ] {
            let ab = a.as_bytes();
            let bb = b.as_bytes();
            // The stack kernel's contract is both operands <= 8 units; the
            // seven-by-nine fixture is out of its dispatch domain.
            if ab.len() <= 8 && bb.len() <= 8 {
                assert_eq!(damerau_unit_small(ab, bb), want, "small {a:?}");
            }
            assert_eq!(damerau_unit_mid(ab, bb), want, "mid {a:?}");
            assert_eq!(damerau_unit_large(ab, bb), want, "large {a:?}");
        }
        // Single-symbol seas and disjoint alphabets across tiers.
        let opts = Options::default();
        for len in [8usize, 9, 60, 129, 200] {
            let aa = "a".repeat(len);
            let ab: String = "ab".chars().cycle().take(len).collect();
            let zz = "z".repeat(len + 3);
            assert_eq!(
                damerau_levenshtein(&aa, &ab, &opts),
                oracle_unrestricted(&aa, &ab)
            );
            assert_eq!(
                damerau_levenshtein(&aa, &zz, &opts),
                oracle_unrestricted(&aa, &zz)
            );
        }
    }

    // -- Bit-parallel search battery ----------------------------------------

    /// The full-matrix search forced end-to-end — `search_full_matrix`
    /// through `dispatch`, assembled into a `SearchResult` exactly as
    /// `search_impl` does — bypassing `search_generic`'s fast-path gate
    /// entirely. The oracle every `search_bits` test below compares full
    /// `SearchResult`s (substring string, `f64` distance bits, signed
    /// offset) against.
    fn oracle_search(a: &str, b: &str, opts: &Options, damerau: bool) -> SearchResult {
        dispatch(a, b, |ops| match ops {
            Operands::Bytes(s, t) => {
                let (start, end, dist) = search_full_matrix(s, t, opts, damerau);
                SearchResult {
                    substring: String::from_utf8_lossy(slice_units(t, start, end)).into_owned(),
                    distance: dist,
                    offset: start,
                }
            }
            Operands::Units(s, t) => {
                let (start, end, dist) = search_full_matrix(s, t, opts, damerau);
                SearchResult {
                    substring: String::from_utf16_lossy(slice_units(t, start, end)),
                    distance: dist,
                    offset: start,
                }
            }
        })
    }

    /// A narrow-alphabet random ASCII string: small alphabets force dense
    /// match structure — many equally-cheap alignments — which is exactly
    /// what stresses the pinned first-minimum and backtrack tie-breaking.
    fn search_rand(rng: &mut SplitMix64, len: usize, alphabet: usize) -> String {
        (0..len)
            .map(|_| (b'a' + rng.next_range(alphabet) as u8) as char)
            .collect()
    }

    /// Embeds a lightly-mutated copy of `needle` into `haystack` at a random
    /// position, forcing a real near-match (and, with narrow alphabets,
    /// frequent exact ties between competing end positions).
    fn embed_near_match(
        rng: &mut SplitMix64,
        needle: &str,
        haystack: &mut String,
        alphabet: usize,
    ) {
        let n = needle.len();
        let m = haystack.len();
        if m <= n {
            return;
        }
        let pos = rng.next_range(m - n);
        let mut copy = needle.to_owned().into_bytes();
        for _ in 0..rng.next_range(3) {
            let i = rng.next_range(copy.len());
            copy[i] = b'a' + rng.next_range(alphabet) as u8;
        }
        haystack.replace_range(pos..pos + n, std::str::from_utf8(&copy).unwrap());
    }

    #[test]
    fn search_bits_agrees_with_full_matrix_on_random_ascii() {
        // The correctness-defining differential for the search fast path:
        // full-`SearchResult` equality against the full-matrix oracle across
        // randomized corpora. Half the haystacks carry an embedded mutated
        // copy of the needle so real matches and ties occur constantly
        // rather than by luck; needle lengths cross both the single-word
        // boundary (63..=66) and the two-block boundary (127..=130).
        let mut rng = SplitMix64(0x5EA2_C4B1_D00D_0001);
        let opts = Options::default();
        for case in 0..3000usize {
            let alphabet = [2usize, 3, 4, 26][rng.next_range(4)];
            let n = 1 + rng.next_range(if case % 5 == 0 { 200 } else { 90 });
            let m = 1 + rng.next_range(220);
            let s = search_rand(&mut rng, n, alphabet);
            let mut t = search_rand(&mut rng, m, alphabet);
            if rng.next_range(2) == 0 {
                embed_near_match(&mut rng, &s, &mut t, alphabet);
            }
            let got = levenshtein_search(&s, &t, &opts);
            let want = oracle_search(&s, &t, &opts, false);
            assert_eq!(got, want, "search mismatch: s={s:?} t={t:?}");
        }
    }

    #[test]
    fn search_bits_boundary_needle_lengths_agree() {
        // The word/blocks dispatch boundary (64) and the one/two-block
        // boundary (128) swept explicitly rather than left to the random
        // corpus, with embedded near-matches at every combination.
        let mut rng = SplitMix64(0x5EA2_C4B1_D00D_0002);
        let opts = Options::default();
        for &n in &[1usize, 2, 63, 64, 65, 66, 127, 128, 129, 130] {
            for &m in &[1usize, 64, 65, 129, 200] {
                for _ in 0..6 {
                    let s = search_rand(&mut rng, n, 3);
                    let mut t = search_rand(&mut rng, m, 3);
                    embed_near_match(&mut rng, &s, &mut t, 3);
                    let got = levenshtein_search(&s, &t, &opts);
                    let want = oracle_search(&s, &t, &opts, false);
                    assert_eq!(got, want, "boundary mismatch n={n} m={m}");
                }
            }
        }
    }

    #[test]
    fn search_bits_agrees_on_utf16_input() {
        // The same differential through the `Operands::Units` (u16) path:
        // Cyrillic (BMP, one unit per char) and astral characters (two
        // units, exercising surrogate-width slicing in the assembled
        // substring). The u16 kernels share every line with the u8 ones
        // except the FxHashMap Peq, so this pins the table plumbing.
        let mut rng = SplitMix64(0x5EA2_C4B1_D00D_0003);
        let opts = Options::default();
        const CYRILLIC: &[char] = &['а', 'б', 'в', 'г'];
        for _ in 0..600 {
            let n = 1 + rng.next_range(120);
            let m = 1 + rng.next_range(140);
            let s: String = (0..n)
                .map(|_| CYRILLIC[rng.next_range(CYRILLIC.len())])
                .collect();
            let t: String = (0..m)
                .map(|_| CYRILLIC[rng.next_range(CYRILLIC.len())])
                .collect();
            let got = levenshtein_search(&s, &t, &opts);
            let want = oracle_search(&s, &t, &opts, false);
            assert_eq!(got, want, "u16 search mismatch: s={s:?} t={t:?}");
        }
        // Astral needles and haystacks: unit lengths cross the word
        // boundary at half the character count.
        for _ in 0..200 {
            let s_units = 2 + rng.next_range(100);
            let t_units = 2 + rng.next_range(140);
            let s = random_unicode_wide(&mut rng, s_units);
            let t = random_unicode_wide(&mut rng, t_units);
            let got = levenshtein_search(&s, &t, &opts);
            let want = oracle_search(&s, &t, &opts, false);
            assert_eq!(got, want, "astral search mismatch: s={s:?} t={t:?}");
        }
    }

    #[test]
    fn search_cell_costs_match_the_full_matrix() {
        // The recovery identity `D[r][c] = prefix-popcount(Pv) −
        // prefix-popcount(Mv)` checked cell-by-cell against the full search
        // matrix — exhaustively for single-word patterns (which also pins
        // the `pv = u64::MAX` junk-bit claim), at random cells for
        // multi-word ones, and always including the r = 64 / r = 65 prefix
        // rows where the mask arithmetic changes words.
        let mut rng = SplitMix64(0x5EA2_C4B1_D00D_0004);
        let opts = Options::default();
        for _ in 0..150 {
            let n = 1 + rng.next_range(150);
            let m = 1 + rng.next_range(150);
            let s = search_rand(&mut rng, n, 3).into_bytes();
            let t = search_rand(&mut rng, m, 3).into_bytes();
            let mat = full_matrix(&s, &t, &opts, false, true);
            let fw = if n <= 64 {
                search_forward_word(&s, &t)
            } else {
                search_forward_blocks(&s, &t)
            };
            if n <= 64 {
                for r in 0..=n {
                    for c in 0..=m {
                        assert_eq!(
                            search_cell_cost(&fw, r, c) as f64,
                            mat.cost_at(r, c),
                            "cell ({r},{c}) n={n} m={m}"
                        );
                    }
                }
            } else {
                for _ in 0..60 {
                    let r = rng.next_range(n + 1);
                    let c = rng.next_range(m + 1);
                    assert_eq!(
                        search_cell_cost(&fw, r, c) as f64,
                        mat.cost_at(r, c),
                        "cell ({r},{c}) n={n} m={m}"
                    );
                }
                for r in [64usize, 65, n] {
                    for c in [1usize, m / 2, m] {
                        assert_eq!(
                            search_cell_cost(&fw, r, c) as f64,
                            mat.cost_at(r, c),
                            "word-boundary cell ({r},{c}) n={n} m={m}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn search_word_and_blocks_agree_on_the_shared_domain() {
        // `search_forward_blocks` is valid for any n >= 1, so pit it
        // directly against the single-word shape everywhere both apply —
        // block-carry threading is exactly what a naive reapplication of
        // the word formula would get wrong, so agreement with the proven
        // single-word path is load-bearing evidence (the same argument
        // `bit_vector_blocks_agrees_with_bit_vector_distance_at_the_boundary`
        // makes for distance mode).
        let mut rng = SplitMix64(0x5EA2_C4B1_D00D_0005);
        for &n in &[1usize, 7, 32, 63, 64] {
            for _ in 0..8 {
                let m = 1 + rng.next_range(120);
                let s = search_rand(&mut rng, n, 3).into_bytes();
                let t = search_rand(&mut rng, m, 3).into_bytes();
                let word = search_forward_word(&s, &t);
                let blocks = search_forward_blocks(&s, &t);
                assert_eq!(word.match_end, blocks.match_end, "match_end n={n} m={m}");
                assert_eq!(
                    word.min_distance, blocks.min_distance,
                    "min_distance n={n} m={m}"
                );
                for r in 0..=n {
                    for c in 0..=m {
                        assert_eq!(
                            search_cell_cost(&word, r, c),
                            search_cell_cost(&blocks, r, c),
                            "cell ({r},{c}) n={n} m={m}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn search_tie_breaking_pinned_examples() {
        // Degenerate repeated-symbol inputs where *every* end position ties:
        // the first-minimum scan and the insert-first backtrack order are
        // the only things determining the answer, so any tie-break drift
        // diverges here first. Expected values pinned from the full-matrix
        // oracle (and stable: they are the reference's own semantics).
        let opts = Options::default();
        for (s, t) in [
            ("aaa", "aaaaaa"),
            ("aa", "aa"),
            ("ab", "ababab"),
            ("aba", "bab"),
            ("ca", "abc"),
            ("b", "aaa"),
        ] {
            let got = levenshtein_search(s, t, &opts);
            let want = oracle_search(s, t, &opts, false);
            assert_eq!(got, want, "tie-break mismatch for {s:?} in {t:?}");
        }
        // The zero-distance prefix tie: "aaa" occurs at offsets 0..=3 in
        // "aaaaaa"; the first minimum keeps the earliest end (column 3),
        // and the backtrack walks pure matches to offset 0.
        let r = levenshtein_search("aaa", "aaaaaa", &opts);
        assert_eq!(
            (r.substring.as_str(), r.distance, r.offset),
            ("aaa", 0.0, 0)
        );
    }

    #[test]
    fn search_weighted_damerau_and_empty_operands_keep_the_matrix_path() {
        // The gate's exclusions, each checked to still produce the original
        // answers. Weighted costs: no bit-vector formulation exists.
        let weighted = Options {
            substitution_cost: 0.5,
            ..Options::default()
        };
        let got = levenshtein_search("kitten", "sitting", &weighted);
        let want = oracle_search("kitten", "sitting", &weighted, false);
        assert_eq!(got, want);

        // Damerau search: transposition parents depend on `last_row_map`
        // state, unrecoverable from cell costs — must never take the fast
        // path even under unit costs. "ca" in "abc" distinguishes the two:
        // the unrestricted transposition changes both distance and
        // backtrace.
        let opts = Options::default();
        for (s, t) in [("ca", "abc"), ("ab", "xxbaxx"), ("abcd", "acbd")] {
            let got = damerau_levenshtein_search(s, t, &opts);
            let want = oracle_search(s, t, &opts, true);
            assert_eq!(got, want, "damerau search mismatch for {s:?} in {t:?}");
        }

        // Empty operands: excluded from the fast path so the kernels can
        // assume a non-empty pattern; answers come from the matrix path.
        for (s, t) in [("", "abc"), ("abc", ""), ("", "")] {
            let got = levenshtein_search(s, t, &opts);
            let want = oracle_search(s, t, &opts, false);
            assert_eq!(got, want, "empty-operand mismatch for {s:?} in {t:?}");
        }
        assert_eq!(levenshtein_search("", "abc", &opts).distance, 0.0);
        assert_eq!(levenshtein_search("abc", "", &opts).distance, 3.0);
    }

    #[test]
    fn search_bench_corpus_pairs_agree() {
        // The pinned benchmark corpus — the exact inputs the competitive
        // numbers are measured on — must produce identical `SearchResult`s
        // through the fast path, up to and including the 1024-unit pairs
        // (16 blocks, the largest column count any in-repo measurement
        // exercises). Skipped silently only if the generated data file is
        // absent (it is checked in, so absence means a partial checkout).
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .unwrap()
            .join("benches/data/distance-pairs.json");
        let Ok(body) = std::fs::read_to_string(&path) else {
            eprintln!("skipping: {} not generated", path.display());
            return;
        };
        let json: serde_json::Value = serde_json::from_str(&body).expect("valid bench data");
        let opts = Options::default();
        for key in ["ascii", "cyrillic"] {
            for (size, pair) in json["pairs"][key].as_object().expect("pair map") {
                let a = pair[0].as_str().unwrap();
                let b = pair[1].as_str().unwrap();
                let got = levenshtein_search(a, b, &opts);
                let want = oracle_search(a, b, &opts, false);
                assert_eq!(got, want, "bench pair {key}/{size}");
            }
        }
    }
}
