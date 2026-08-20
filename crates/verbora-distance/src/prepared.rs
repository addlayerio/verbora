//! Prepared patterns: the build-once, query-many shape for the Levenshtein
//! family.
//!
//! **Internal notes.** The choice between this shape and the per-call
//! functions is published on the crate root; what a caller must know about
//! [`PreparedPattern`] itself — what construction freezes, which metrics take
//! part, and what a query that cannot use the table does instead — is on the
//! type. What follows is how the query path is put together.
//!
//! The type owns the pattern verbatim so that every fallback can call the
//! per-call function with exactly the arguments the caller would have passed.
//! That is what makes "the prepared answer equals the per-call answer" true by
//! construction rather than by parallel maintenance, and it is why no fallback
//! here may grow a second implementation of anything.
//!
//! The state this type holds is *prepared immutable state*, never a scratch
//! buffer, and the crate root says so publicly. The distinction in code: the
//! dynamic-programming working set the kernels want (Myers' `Pv`/`Mv` words,
//! the weighted paths' rolling rows) depends on both operands, so it is built
//! per query here exactly as it is in the per-call functions, and no API takes
//! one from the caller.
//!
//! # What has not been measured
//!
//! `UNMEASURED` for the current code. `benches/distance.rs`'s
//! `prepared_pattern` group is the measurement that would settle the
//! amortization claim — one query against 64 candidates at pattern lengths 3
//! to 48, `per_call` against `prepared`, with the table build deliberately
//! inside the timed closure so the amortization is paid for rather than
//! assumed — but no number from it may be published until it is re-run on this
//! code.

use crate::levenshtein::{
    TRIM_MIN_LEN, bit_vector_distance_blocks_with, bit_vector_distance_with, fill_scalars,
    levenshtein, osa, osa_bit_vector_blocks_with, osa_bit_vector_with,
};
use crate::units::{BitPeq, Unit, common_prefix_len, common_suffix_len};
use std::fmt;

/// A pattern with its bit-parallel match table already built, for comparing
/// one string against many.
///
/// Construct it from the operand that stays fixed — the query term, the
/// dictionary headword, the name being screened — and call
/// [`levenshtein`](Self::levenshtein) or [`osa`](Self::osa) once per
/// candidate. Both return exactly what the free function of the same name
/// returns for `(pattern, target)`, in that argument order; the fallbacks that
/// guarantee it are described below.
///
/// [*One comparison, or one pattern against many*](crate#one-comparison-or-one-pattern-against-many)
/// is the decision this type takes part in, with the comparison table and the
/// crossover.
///
/// ```
/// use verbora_distance::PreparedPattern;
///
/// let query = PreparedPattern::new("Jonathan");
///
/// let closest = ["Jonathon", "Nathan", "Johnson"]
///     .into_iter()
///     .map(|name| (name, query.levenshtein(name)))
///     .min_by_key(|&(_, distance)| distance);
///
/// assert_eq!(closest, Some(("Jonathon", 1)));
/// ```
///
/// # Which metrics take part
///
/// [`levenshtein`](Self::levenshtein) and [`osa`](Self::osa) share one table,
/// bit for bit: Hyyrö's transposition extension of Myers' algorithm adds two
/// registers, not table entries, so the match table means the same thing to
/// both — see [`new`](Self::new) for what that means for construction.
///
/// Unrestricted Damerau–Levenshtein has no method here, and the absence is
/// deliberate rather than an omission: its unit-cost kernel is Zhao–Sahni's
/// linear-space recurrence, which uses no pattern-match table at all. Its one
/// table is a *last-occurrence* map filled during the scan, keyed by the
/// symbols of the operand being walked and rewritten as the walk advances —
/// nothing in it is a function of the pattern alone, so there is nothing to
/// hoist. Call [`damerau_levenshtein`](crate::damerau_levenshtein) directly.
///
/// # Unit costs only
///
/// Both methods are the unit-cost metric. There is no weighted form here, and
/// that absence is structural rather than an omission: the prepared state *is*
/// a bit-parallel pattern-match table, and the bit-parallel kernels have no
/// notion of a weighted operation. A caller with weighted costs calls
/// [`levenshtein_weighted`](crate::levenshtein_weighted) or
/// [`osa_weighted`](crate::osa_weighted) per pair; there is nothing a prepared
/// type could hoist for them.
///
/// # What is frozen at construction
///
/// The per-call functions decide two things per *pair* that a prepared pattern
/// must decide once, in advance:
///
/// * **Element type.** An internal dispatch compares two ASCII operands as
///   bytes and promotes any other pair to Unicode scalars. This type freezes
///   the choice on the pattern, so an ASCII pattern's byte table cannot serve
///   a non-ASCII target. A non-ASCII pattern has no such limit: its table is
///   built over scalars and serves every target.
/// * **Operand role.** The per-call kernels bit-pack whichever operand is
///   *shorter*; a prepared pattern is always the packed one, even against a
///   shorter target. Myers' recurrence does not require the scanned operand to
///   be the longer one, so this costs nothing in correctness, and a short
///   target is genuinely cheaper this way — the sweep is `n × ceil(m / 64)`
///   either way round, and swapping would only trade a shorter sweep for a
///   table this type already owns.
///
/// # When a query cannot use the table
///
/// A non-ASCII target against an ASCII pattern, an empty pattern, or a pair
/// whose common affix makes trimming worth more than the table saves: in each
/// case the call falls through to the per-call function verbatim. That is the
/// property worth stating plainly — **every fallback is literally the call you
/// would otherwise have written** — so preparing a pattern is never the slower
/// choice, and no answer ever depends on which path ran.
///
/// # Cost of holding one
///
/// About 2 KB for an ASCII pattern of up to 64 units, because the table is a
/// flat `[u64; 256]` stored inline rather than behind a pointer: a query's
/// match lookup is then one indexed load from `&self` with no indirection to
/// chase. That is the *whole* structure for the common case — one value to
/// keep alive per fixed pattern, not one per comparison — and only patterns
/// past 64 units, or non-ASCII ones, put anything on the heap. Sharing one
/// across threads is a plain `&`: queries never mutate it.
///
/// # When not to prepare
///
/// If each pattern is used for one or two comparisons, the free functions are
/// the better call: the table this type builds up front is precisely what
/// they avoid building for short patterns, where
/// [`levenshtein`](fn@crate::levenshtein) uses a table-free kernel outright.
/// Preparing pays back over a candidate set, not over a pair.
#[derive(Clone)]
pub struct PreparedPattern {
    /// The pattern verbatim, so every fallback can call the per-call function
    /// with the same arguments the caller would have passed.
    pattern: String,
    /// The pattern as Unicode scalars — empty when the pattern is ASCII,
    /// where `pattern.as_bytes()` already *is* the unit sequence and copying
    /// it would buy nothing.
    units: Vec<char>,
    /// Pattern length in Unicode scalars: the length this crate's metrics
    /// count in, and the `m` the kernels need (a table alone cannot report
    /// it — a pattern whose last unit recurs earlier leaves the high bits of
    /// every row unset).
    unit_len: usize,
    peq: Peq,
}

/// The frozen match table, in the shape the pattern's own element type and
/// length select.
///
/// One enum rather than a boxed trait object: the query path has to branch on
/// the element type anyway (to decide whether the target fits it), so the
/// branch may as well be the one that hands the kernel a concrete table.
#[expect(
    clippy::large_enum_variant,
    reason = "the 2 KB byte table is stored inline on purpose: this type is \
              built once per pattern and read from thousands of queries, so a \
              pointer chase per lookup costs more than the unused bytes of a \
              smaller variant ever save"
)]
#[derive(Clone)]
enum Peq {
    /// The pattern is empty. Every query is a length shortcut, so there is
    /// nothing worth precomputing and every call delegates.
    Empty,
    /// ASCII pattern of 1..=64 units: one flat 256-entry table of masks.
    ByteWord([u64; 256]),
    /// ASCII pattern past 64 units: packed multi-block rows.
    ByteBlocks(<u8 as BitPeq>::TableN),
    /// Non-ASCII pattern of 1..=64 units: hashed scalar → mask.
    UnitWord(<char as BitPeq>::Table1),
    /// Non-ASCII pattern past 64 units: hashed scalar → packed row.
    UnitBlocks(<char as BitPeq>::TableN),
}

/// Which of the two metrics a query is asking for.
///
/// The pair share a table but not a kernel, so the choice is threaded through
/// the shared query path rather than duplicating it per metric.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Metric {
    Levenshtein,
    Osa,
}

impl PreparedPattern {
    /// Builds the match table for `pattern`.
    ///
    /// The work here is proportional to the pattern's length plus the table's
    /// fixed size, and it is the work every subsequent query no longer does.
    /// The table is built for both metrics at once because they share it
    /// exactly; there is no per-metric constructor to choose between and no
    /// second table to pay for if a caller uses both.
    ///
    /// ```
    /// use verbora_distance::PreparedPattern;
    ///
    /// let empty = PreparedPattern::new("");
    /// assert_eq!(empty.levenshtein("abc"), 3);
    /// ```
    #[must_use]
    pub fn new(pattern: &str) -> Self {
        if pattern.is_ascii() {
            // ASCII bytes *are* Unicode scalars, so the pattern needs no
            // second representation and no length scan.
            let bytes = pattern.as_bytes();
            let unit_len = bytes.len();
            let peq = match unit_len {
                0 => Peq::Empty,
                1..=64 => Peq::ByteWord(<u8 as BitPeq>::peq1(bytes)),
                _ => Peq::ByteBlocks(<u8 as BitPeq>::peqn(bytes, unit_len.div_ceil(64))),
            };
            return Self {
                pattern: pattern.to_owned(),
                units: Vec::new(),
                unit_len,
                peq,
            };
        }

        let units: Vec<char> = pattern.chars().collect();
        let unit_len = units.len();
        // A non-ASCII `&str` has at least one scalar, so `Peq::Empty` is
        // unreachable here.
        let peq = if unit_len <= 64 {
            Peq::UnitWord(<char as BitPeq>::peq1(&units))
        } else {
            Peq::UnitBlocks(<char as BitPeq>::peqn(&units, unit_len.div_ceil(64)))
        };
        Self {
            pattern: pattern.to_owned(),
            units,
            unit_len,
            peq,
        }
    }

    /// The pattern this was built from.
    #[must_use]
    pub fn pattern(&self) -> &str {
        &self.pattern
    }

    /// Levenshtein distance from this pattern to `target`, in edits.
    ///
    /// Equal to
    /// [`levenshtein(self.pattern(), target)`](fn@crate::levenshtein) for
    /// every input; see [*When a query cannot use the
    /// table*](PreparedPattern#when-a-query-cannot-use-the-table) for the
    /// fallbacks that guarantee it. Unit costs are symmetric, so the argument
    /// order does not affect the answer.
    ///
    /// ```
    /// use verbora_distance::PreparedPattern;
    ///
    /// let query = PreparedPattern::new("kitten");
    /// assert_eq!(query.levenshtein("sitting"), 3);
    /// ```
    #[must_use]
    pub fn levenshtein(&self, target: &str) -> usize {
        self.distance(target, Metric::Levenshtein)
    }

    /// Optimal string alignment (restricted Damerau–Levenshtein) distance
    /// from this pattern to `target`, in edits.
    ///
    /// Equal to [`osa(self.pattern(), target)`](crate::osa) for every input,
    /// on the same terms as [`levenshtein`](Self::levenshtein) — and off the
    /// same table, since Hyyrö's transposition extension reads the identical
    /// match table.
    ///
    /// ```
    /// use verbora_distance::PreparedPattern;
    ///
    /// let query = PreparedPattern::new("kitten");
    /// // One transposition, not two substitutions.
    /// assert_eq!(query.osa("kitetn"), 1);
    /// ```
    #[must_use]
    pub fn osa(&self, target: &str) -> usize {
        self.distance(target, Metric::Osa)
    }

    /// The shared query path: decide whether the prepared table can serve
    /// this pair, and either run the kernel over it or hand the pair back to
    /// the per-call function.
    fn distance(&self, target: &str, metric: Metric) -> usize {
        // There is no cost gate left to take: the bit-parallel kernels have no
        // notion of a weighted operation, and the weighted metrics are now
        // different functions with no prepared form at all. An empty pattern
        // is the one shape with nothing to precompute.
        if matches!(self.peq, Peq::Empty) {
            return self.per_call(target, metric);
        }

        match &self.peq {
            // Dead, and deliberately written as the delegation it would perform
            // rather than as `unreachable!`. The early return above is the only
            // path an empty pattern takes — `peq` is read twice from the same
            // `&self` with no interior mutability in between — but this crate's
            // contract is that *no function panics, on any input, under any
            // cost set* (`docs/design/distance-contract.md`), and an
            // `unreachable!` is a `panic!` expansion that any grep-level audit
            // of that claim has to re-litigate. This arm costs nothing and
            // makes the claim checkable.
            Peq::Empty => self.per_call(target, metric),
            Peq::ByteWord(table) => {
                // An ASCII pattern's byte table cannot see a target's
                // non-ASCII scalars, and comparing raw UTF-8 bytes would
                // answer a different question, so a non-ASCII target goes
                // back to `dispatch`, which promotes both operands together.
                let Some(bytes) = ascii_units(target) else {
                    return self.per_call(target, metric);
                };
                if trim_pays(self.pattern.as_bytes(), bytes) {
                    return self.per_call(target, metric);
                }
                self.word_kernel(table, bytes, metric)
            }
            Peq::ByteBlocks(table) => {
                let Some(bytes) = ascii_units(target) else {
                    return self.per_call(target, metric);
                };
                if trim_pays(self.pattern.as_bytes(), bytes) {
                    return self.per_call(target, metric);
                }
                self.blocks_kernel(table, bytes, metric)
            }
            Peq::UnitWord(table) => with_scalars(target, |units| {
                if trim_pays(&self.units, units) {
                    return self.per_call(target, metric);
                }
                self.word_kernel(table, units, metric)
            }),
            Peq::UnitBlocks(table) => with_scalars(target, |units| {
                if trim_pays(&self.units, units) {
                    return self.per_call(target, metric);
                }
                self.blocks_kernel(table, units, metric)
            }),
        }
    }

    /// Runs the requested metric's single-word kernel over the prepared
    /// table. The two kernels are the same Myers loop plus, for OSA, one
    /// extra register pair — hence one table and two entry points.
    fn word_kernel<T: BitPeq>(&self, table: &T::Table1, target: &[T], metric: Metric) -> usize {
        match metric {
            Metric::Levenshtein => bit_vector_distance_with(table, self.unit_len, target),
            Metric::Osa => osa_bit_vector_with(table, self.unit_len, target),
        }
    }

    /// [`Self::word_kernel`] past the single-word bound, over the packed rows.
    fn blocks_kernel<T: BitPeq>(&self, table: &T::TableN, target: &[T], metric: Metric) -> usize {
        match metric {
            Metric::Levenshtein => bit_vector_distance_blocks_with(table, self.unit_len, target),
            Metric::Osa => osa_bit_vector_blocks_with(table, self.unit_len, target),
        }
    }

    /// The per-call function, called exactly as the caller would have.
    fn per_call(&self, target: &str, metric: Metric) -> usize {
        #[cfg(test)]
        FALLBACKS.with(|count| count.set(count.get() + 1));
        match metric {
            Metric::Levenshtein => levenshtein(&self.pattern, target),
            Metric::Osa => osa(&self.pattern, target),
        }
    }
}

/// Whether this pair is better served by the per-call path's common-affix
/// trim than by the prepared table.
///
/// # Why there is a choice to make
///
/// Trimming a shared prefix shifts every remaining pattern position down
/// by its length, and a `Peq` built over the whole pattern encodes the
/// unshifted positions — so the two optimisations cannot both apply to
/// one table. Which is worth more depends entirely on the pair:
///
/// * No common affix (two distinct names, the dominant case in a
///   screening workload): trimming has nothing to remove, and the table
///   wins outright.
/// * A long common affix (`"Alexander"` vs `"Alexandre"`): trimming
///   collapses the problem to its differing middle, which is worth far
///   more than any table.
///
/// # Why correctness does not depend on the answer
///
/// Both paths compute the same distance. Affix trimming is an
/// optimisation, not a precondition: the untrimmed kernel over the full
/// pattern is exactly what the per-call path runs when the pair has no
/// affix to strip, and Myers' recurrence neither knows nor cares whether
/// a cheaper equivalent pair existed. This predicate therefore trades
/// speed for speed, and a wrong answer costs nanoseconds, never a wrong
/// distance — which is what the differential tests pin, deliberately
/// including affix-heavy pairs on both sides of every threshold below.
///
/// # The estimate
///
/// Cost is counted in column steps — one scanned target unit against one
/// 64-bit block — because that is the unit both paths spend. The prepared
/// path spends `n × ceil(m / 64)` of them and nothing else. The per-call
/// path spends the same over the *trimmed* pair, plus rebuilding the
/// table it threw away, except below [`TRIM_MIN_LEN`] where its kernel
/// needs no table at all.
fn trim_pays<T: Unit>(pattern: &[T], target: &[T]) -> bool {
    let (m, n) = (pattern.len(), target.len());
    debug_assert!(m > 0);
    if n == 0 {
        return false;
    }
    // The whole point of the O(1) screen: an affix scan on a pair that
    // shares neither end can only report zero, and every query in a
    // screening workload would pay for it. Both ends are checked because
    // either one alone is enough to make a trim possible.
    if pattern[0] != target[0] && pattern[m - 1] != target[n - 1] {
        return false;
    }

    // Same order the per-call path trims in — prefix first, then suffix
    // over what is left — so `p + s` is the length it would actually
    // remove, not an over-count from two independent scans.
    let p = common_prefix_len(pattern, target);
    let s = common_suffix_len(&pattern[p..], &target[p..]);
    let trimmed_min = m.min(n) - p - s;
    let trimmed_max = m.max(n) - p - s;

    let rebuild = if trimmed_min <= TRIM_MIN_LEN {
        0
    } else {
        PEQ_REBUILD_COLUMNS
    };
    let per_call = rebuild + trimmed_max * trimmed_min.div_ceil(64);
    let prepared = n * m.div_ceil(64);
    per_call < prepared
}

/// Column steps the per-call path's `Peq` rebuild is worth, for
/// [`trim_pays`]'s estimate.
///
/// A rebuild zeroes a 2 KB table (or clears and refills a hash map) and then
/// writes one bit per pattern unit; a column step is a handful of register
/// operations on data already in cache. The two are orders of magnitude
/// apart, and this constant is a deliberately round stand-in for that ratio
/// rather than a measured crossover — it decides only which of two paths
/// computing the same number runs, so precision here buys nothing that a
/// benchmark of the real workload would not buy better.
const PEQ_REBUILD_COLUMNS: usize = 64;

#[cfg(test)]
thread_local! {
    /// Fallbacks to the per-call functions on this thread.
    ///
    /// Parity against the per-call functions is necessary but not sufficient
    /// evidence: a prepared type that fell back on *every* query would pass
    /// every differential test while doing nothing. The tests read this
    /// counter to pin which path each shape of query actually took, so a
    /// regression that quietly disables the table fails loudly instead of
    /// passing silently. One counter per thread, because the test harness
    /// gives each test its own.
    static FALLBACKS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Runs `f` and reports how many queries inside it fell back.
#[cfg(test)]
fn count_fallbacks(f: impl FnOnce()) -> usize {
    FALLBACKS.with(|count| count.set(0));
    f();
    FALLBACKS.with(std::cell::Cell::get)
}

/// `target`'s bytes when they are also its Unicode scalars, `None` when the
/// two differ.
#[inline]
fn ascii_units(target: &str) -> Option<&[u8]> {
    target.is_ascii().then_some(target.as_bytes())
}

/// Runs `f` on `target`'s Unicode scalars.
///
/// A UTF-8 string can never hold more scalars than it holds bytes, so a
/// length check against the byte length is enough to know a fixed stack
/// buffer will fit — no counting pass, and no allocation for the word-sized
/// targets a screening workload is made of.
fn with_scalars<R>(target: &str, f: impl FnOnce(&[char]) -> R) -> R {
    const STACK_UNITS: usize = 64;
    if target.len() <= STACK_UNITS {
        let mut units = ['\0'; STACK_UNITS];
        let len = fill_scalars(target, &mut units);
        return f(&units[..len]);
    }
    let units: Vec<char> = target.chars().collect();
    f(&units)
}

impl fmt::Debug for PreparedPattern {
    /// Prints the pattern and which table shape it selected, not the table:
    /// 256 mask words are noise in a debug log, and the shape is the part
    /// that explains a cost.
    ///
    /// The length is keyed `scalars` rather than after the private field it
    /// reads: there is no `unit_len` accessor on this type — the contract
    /// deleted it in favour of `p.pattern().chars().count()` — and a debug
    /// key naming a method that does not exist sends a reader looking for it.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PreparedPattern")
            .field("pattern", &self.pattern)
            .field("scalars", &self.unit_len)
            .field(
                "table",
                &match self.peq {
                    Peq::Empty => "none (empty pattern)",
                    Peq::ByteWord(_) => "byte, one word",
                    Peq::ByteBlocks(_) => "byte, multi-block",
                    Peq::UnitWord(_) => "scalar, one word",
                    Peq::UnitBlocks(_) => "scalar, multi-block",
                },
            )
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Deterministic PRNG, the same shape as the sibling modules' helpers:
    /// fixed seed so a failure reproduces, adversarial enough for random
    /// strings, not for anything security-sensitive.
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

    /// Both metrics, both directions of the claim: a prepared pattern answers
    /// exactly what the per-call function answers.
    #[track_caller]
    fn assert_parity(pattern: &str, target: &str) {
        let prepared = PreparedPattern::new(pattern);
        assert_eq!(
            prepared.levenshtein(target),
            levenshtein(pattern, target),
            "levenshtein({pattern:?}, {target:?})"
        );
        assert_eq!(
            prepared.osa(target),
            osa(pattern, target),
            "osa({pattern:?}, {target:?})"
        );
    }

    #[test]
    fn classic_pairs_match_the_per_call_functions() {
        for (a, b) in [
            ("kitten", "sitting"),
            ("saturday", "sunday"),
            ("flaw", "lawn"),
            ("ab", "ba"),
            ("ca", "abc"),
            ("same", "same"),
        ] {
            assert_parity(a, b);
            assert_parity(b, a);
        }
    }

    #[test]
    fn empty_operands_on_both_sides() {
        for (a, b) in [("", ""), ("", "abc"), ("abc", ""), ("", "😀"), ("😀", "")] {
            assert_parity(a, b);
        }
        // The empty pattern keeps no table, so this exercises the delegating
        // branch specifically.
        assert_eq!(PreparedPattern::new("").levenshtein("abcd"), 4);
        // The crate ships exactly one way to spell a pattern's length.
        assert_eq!(PreparedPattern::new("").pattern().chars().count(), 0);
    }

    #[test]
    fn targets_shorter_than_the_pattern() {
        // The per-call path bit-packs the shorter operand; a prepared pattern
        // is always the packed one, so every one of these runs the kernel
        // with the roles the per-call path would have swapped.
        let pattern = "abcdefghijklmnop";
        for target in ["", "a", "ap", "xyz", "abcd", "ponmlkjihgfedcba"] {
            assert_parity(pattern, target);
        }
        // Same again with a multi-block pattern against one-unit targets.
        let long: String = std::iter::repeat_n("abcdefgh", 20).collect();
        for target in ["a", "z", "ab"] {
            assert_parity(&long, target);
        }
    }

    #[test]
    fn non_ascii_target_against_an_ascii_pattern_falls_back() {
        // The element type is frozen on the pattern, so these must route
        // through `dispatch` rather than compare UTF-8 bytes.
        for (a, b) in [
            ("cafe", "café"),
            ("abc", "abç"),
            ("kitten", "kittén"),
            ("ab", "a😀b"),
            ("x", "😀"),
        ] {
            assert_parity(a, b);
            // And the mirrored direction, where the *pattern* is non-ASCII
            // and the target is not.
            assert_parity(b, a);
        }
    }

    #[test]
    fn astral_characters_count_as_one_unit() {
        let prepared = PreparedPattern::new("a😀b");
        // Three scalars, so deleting the emoji is one edit
        // (`docs/design/distance-contract.md` §2.5).
        assert_eq!(prepared.pattern().chars().count(), 3);
        assert_eq!(prepared.levenshtein("ab"), 1);
        assert_parity("a😀b", "ab");
        assert_parity("😀😀", "😀");
        assert_parity("𝕳𝖊𝖑𝖑𝖔", "hello");
    }

    #[test]
    fn weighted_costs_have_no_prepared_form_at_all() {
        // There is nothing left to delegate: the weighted metrics are
        // different functions, they take a cost type this type's methods do
        // not accept, and the argument-order caveat that used to matter
        // (`insertion != deletion` made the prepared pattern's role as
        // *source* observable) cannot arise, because unit costs are
        // symmetric. What a weighted caller does instead is call the free
        // function per pair — which is exactly what the prepared path would
        // have fallen back to.
        let costs = crate::levenshtein::LevenshteinCosts::new(1.0, 3.0, 1.0).expect("admissible");
        assert_eq!(
            crate::levenshtein::levenshtein_weighted("abc", "ab", &costs),
            3.0
        );
        assert_eq!(
            crate::levenshtein::levenshtein_weighted("ab", "abc", &costs),
            1.0
        );
        // The prepared pattern answers the unit-cost question, symmetrically.
        assert_eq!(PreparedPattern::new("abc").levenshtein("ab"), 1);
        assert_eq!(PreparedPattern::new("ab").levenshtein("abc"), 1);
    }

    #[test]
    fn affix_heavy_pairs_agree_on_both_sides_of_the_heuristic() {
        // Every shape `trim_pays` can classify: shared prefix only, shared
        // suffix only, both, one differing unit in the middle, and identical
        // operands — at lengths spanning the table-free bound, the one-word
        // bound and the multi-block case.
        for len in [1usize, 4, 5, 8, 63, 64, 65, 100, 130] {
            let base: String = std::iter::repeat_n('a', len).collect();
            let mut prefixed = base.clone();
            prefixed.push_str("xyz");
            let mut suffixed = String::from("xyz");
            suffixed.push_str(&base);
            let mut middle = base.clone();
            if len > 1 {
                middle.replace_range(len / 2..len / 2 + 1, "q");
            }

            for target in [&base, &prefixed, &suffixed, &middle] {
                assert_parity(&base, target);
                assert_parity(target, &base);
            }
        }

        // Real-world affix shapes, where the trim is worth most.
        for (a, b) in [
            ("Alexander", "Alexandre"),
            ("Jonathan", "Jonathon"),
            ("internationalization", "internationalisation"),
            ("prefix-common-suffix", "prefix-XXXXXX-suffix"),
        ] {
            assert_parity(a, b);
            assert_parity(b, a);
        }
    }

    #[test]
    fn randomized_corpus_matches_the_per_call_functions() {
        // The differential battery: small alphabets so shared affixes and
        // transpositions arise by chance at every length, and lengths that
        // straddle the table-free, one-word and multi-block boundaries on
        // both the pattern and the target side.
        let mut rng = SplitMix64(0x9E37_1234_ABCD_0007);
        for round in 0..6000 {
            let alphabet: &[u8] = [&b"ab"[..], b"abc", b"abcdefgh"][round % 3];
            let m = rng.next_range(90);
            let n = rng.next_range(90);
            let pattern: String = (0..m)
                .map(|_| alphabet[rng.next_range(alphabet.len())] as char)
                .collect();
            let mut target: String = (0..n)
                .map(|_| alphabet[rng.next_range(alphabet.len())] as char)
                .collect();
            // Plant a shared affix in a third of the rounds so the heuristic's
            // measuring branch is exercised as heavily as its screening one.
            if round % 3 == 0 && !pattern.is_empty() {
                let take = 1 + rng.next_range(pattern.len());
                target = format!("{}{target}", &pattern[..take]);
            }
            assert_parity(&pattern, &target);
        }
    }

    #[test]
    fn randomized_unicode_corpus_matches_the_per_call_functions() {
        // Non-ASCII patterns take the hashed tables and the scalar kernels;
        // mixing ASCII and non-ASCII operands exercises the frozen element
        // type in both directions, and the astral characters make some
        // operands two units per character.
        let alphabet = ['a', 'é', 'ß', 'Ж', '😀', 'z'];
        let mut rng = SplitMix64(0x5EED_0000_C0FF_EE11);
        for _ in 0..3000 {
            let m = rng.next_range(40);
            let n = rng.next_range(40);
            let pattern: String = (0..m).map(|_| alphabet[rng.next_range(6)]).collect();
            let target: String = (0..n).map(|_| alphabet[rng.next_range(6)]).collect();
            assert_parity(&pattern, &target);
        }
    }

    #[test]
    fn multi_block_patterns_match_the_per_call_functions() {
        // Past 64 units the packed rows and the block-carry kernels take
        // over, on both metrics, and the popcount/last-bit variants of the
        // plain kernel split at 256 units.
        let mut rng = SplitMix64(0xB10C_0000_0000_0042);
        for m in [65usize, 100, 128, 255, 256, 257, 400] {
            let pattern: String = (0..m)
                .map(|_| b"abcde"[rng.next_range(5)] as char)
                .collect();
            for n in [0usize, 1, 64, 65, 300] {
                let target: String = (0..n)
                    .map(|_| b"abcde"[rng.next_range(5)] as char)
                    .collect();
                assert_parity(&pattern, &target);
            }
        }
    }

    #[test]
    fn one_prepared_pattern_serves_a_whole_candidate_set() {
        // The workload the type exists for: reusing one instance across many
        // queries must not accumulate state, so the same target answers the
        // same distance whenever it is asked, in any order, interleaved
        // between the two metrics.
        let prepared = PreparedPattern::new("Jonathan");
        let candidates = [
            "Jonathon",
            "Nathan",
            "Johnson",
            "Jonathan",
            "",
            "Jónathan",
            "nahtaJ",
        ];
        let first: Vec<(usize, usize)> = candidates
            .iter()
            .map(|c| (prepared.levenshtein(c), prepared.osa(c)))
            .collect();
        for _ in 0..3 {
            for (c, expected) in candidates.iter().zip(&first) {
                assert_eq!(prepared.osa(c), expected.1, "osa({c:?})");
                assert_eq!(prepared.levenshtein(c), expected.0, "lev({c:?})");
            }
        }
        assert_eq!(prepared.pattern(), "Jonathan");
    }

    #[test]
    fn damerau_has_no_prepared_form() {
        // The module claims unrestricted Damerau–Levenshtein shares nothing
        // with the prepared table; this pins the consequence a caller sees —
        // it stays a free function, and the prepared type offers no method
        // that would silently answer a different metric.
        assert_eq!(crate::damerau_levenshtein("ca", "abc"), 2);
        assert_eq!(PreparedPattern::new("ca").osa("abc"), 3);
    }

    #[test]
    fn the_prepared_table_is_the_path_actually_taken() {
        // Parity proves the answers agree; this proves the table is what
        // produced them. Every shape that *should* use it, and every
        // documented reason it cannot.
        let ascii = PreparedPattern::new("kitten");
        let unicode = PreparedPattern::new("Ж😀café");
        let long: String = std::iter::repeat_n("abcdefgh", 12).collect();
        let blocks = PreparedPattern::new(&long);

        // Shares neither first nor last unit with the pattern: the table runs.
        for (label, used) in [
            (
                "ascii word",
                count_fallbacks(|| {
                    let _ = ascii.levenshtein("sitting");
                    let _ = ascii.osa("sitting");
                }),
            ),
            (
                "utf-16 word",
                count_fallbacks(|| {
                    let _ = unicode.levenshtein("dolor sit");
                    let _ = unicode.osa("dolor sit");
                }),
            ),
            (
                "ascii blocks",
                count_fallbacks(|| {
                    let _ = blocks.levenshtein("zyxwvutsrq");
                    let _ = blocks.osa("zyxwvutsrq");
                }),
            ),
            (
                "short target",
                count_fallbacks(|| {
                    // Shorter than the pattern — the per-call path would swap the
                    // operands here; the prepared one must not need to.
                    let _ = ascii.levenshtein("s");
                    let _ = ascii.osa("s");
                }),
            ),
        ] {
            assert_eq!(used, 0, "{label} should have used the prepared table");
        }

        // The two documented reasons a query cannot: element type and an
        // empty pattern. Weighted costs are no longer among them — they are
        // not a query this type accepts at all, so there is no cost-shaped
        // way to fall off the table.
        assert_eq!(
            count_fallbacks(|| {
                let _ = ascii.levenshtein("kittén");
            }),
            1
        );
        let empty = PreparedPattern::new("");
        assert_eq!(
            count_fallbacks(|| {
                let _ = empty.levenshtein("sitting");
            }),
            1
        );

        // A near-identical pair: trimming leaves almost nothing, which is
        // worth more than any table, so this one is meant to fall back.
        let mut near = long;
        near.push_str("zz");
        assert_eq!(
            count_fallbacks(|| {
                let _ = blocks.levenshtein(&near);
            }),
            1
        );
        // A single shared unit is not, on a pattern this long: rebuilding the
        // table would cost far more than the one column the trim removes.
        assert_eq!(
            count_fallbacks(|| {
                let _ = blocks.levenshtein("axxxxxxxxxx");
            }),
            0
        );
    }

    #[test]
    fn the_randomized_corpus_is_not_all_fallbacks() {
        // Guards the batteries above from becoming vacuous: over a corpus
        // built the same way `randomized_corpus_matches_the_per_call_functions`
        // builds one, the prepared table must carry the clear majority of
        // queries. The exact ratio is not the point — "most of them" is.
        let mut rng = SplitMix64(0x9E37_1234_ABCD_0007);
        let rounds = 2000;
        let fallbacks = count_fallbacks(|| {
            for _ in 0..rounds {
                let m = 1 + rng.next_range(20);
                let n = 1 + rng.next_range(20);
                let pattern: String = (0..m)
                    .map(|_| b"abcdefgh"[rng.next_range(8)] as char)
                    .collect();
                let target: String = (0..n)
                    .map(|_| b"abcdefgh"[rng.next_range(8)] as char)
                    .collect();
                let _ = PreparedPattern::new(&pattern).levenshtein(&target);
            }
        });
        assert!(
            fallbacks * 2 < rounds,
            "{fallbacks} of {rounds} queries fell back to the per-call path"
        );
    }

    #[test]
    fn debug_reports_the_table_shape_not_the_table() {
        let short = format!("{:?}", PreparedPattern::new("abc"));
        assert!(short.contains("byte, one word"), "{short}");
        assert!(short.contains("scalars: 3"), "{short}");
        let long: String = std::iter::repeat_n('a', 70).collect();
        assert!(format!("{:?}", PreparedPattern::new(&long)).contains("multi-block"));
        assert!(format!("{:?}", PreparedPattern::new("café")).contains("scalar, one word"));
        assert!(format!("{:?}", PreparedPattern::new("")).contains("empty pattern"));
    }

    #[test]
    fn is_shareable_across_threads() {
        // The documented usage is one instance behind a `&`, screening a
        // candidate set in parallel. Queries take `&self` and write nothing,
        // so this holds structurally — pinned here because it is a promise the
        // docs make, and adding an interior-mutable field later would break it
        // silently.
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<PreparedPattern>();

        let prepared = PreparedPattern::new("Jonathan");
        let shared = &prepared;
        std::thread::scope(|scope| {
            for _ in 0..4 {
                scope.spawn(move || {
                    assert_eq!(shared.levenshtein("Jonathon"), 1);
                    assert_eq!(shared.osa("Jonahtan"), 1);
                });
            }
        });
    }

    #[test]
    fn clone_answers_the_same_distances() {
        let prepared = PreparedPattern::new("Jonathan");
        let cloned = prepared.clone();
        for c in ["Jonathon", "x", "Jónathan"] {
            assert_eq!(cloned.levenshtein(c), prepared.levenshtein(c));
            assert_eq!(cloned.osa(c), prepared.osa(c));
        }
    }
}
