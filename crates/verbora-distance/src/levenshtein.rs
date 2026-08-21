//! Levenshtein, Damerau–Levenshtein and optimal string alignment: kernels,
//! recurrences and the search matrix.
//!
//! **Internal notes.** Everything a caller needs is published where a caller
//! finds it: the algorithm table and the unit-versus-weighted choice are on
//! the crate root, the search guarantees and the observable tie-breaking are
//! on [`SearchResult`], the cost preconditions are on [`LevenshteinCosts`],
//! [`OsaCosts`] and [`DamerauCosts`], and the per-metric semantics are on the
//! functions themselves. What follows is why *this code* is shaped the way it
//! is — kernel selection, working sets, and the reductions that feed them.
//!
//! Both Damerau variants are symmetric under unit costs, and unrestricted
//! Damerau–Levenshtein additionally satisfies the triangle inequality; each is
//! pinned by this module's own differential tests against a from-scratch
//! reference implementation transcribed from the published recurrence, plus
//! property tests for symmetry and the triangle inequality.
//!
//! # What has not been measured
//!
//! `UNMEASURED`, on the unit/weighted split: the unit tier's kernels are the
//! ones this module has always run for unit costs, and the weighted tier's are
//! the ones it has always run for weighted costs, so the *algorithms* are
//! unchanged — but removing the per-call cost comparison that used to choose
//! between them has never been timed, and no number may be published for it
//! until it is.
//!
//! # Performance shape
//!
//! A naive implementation materialises a full `(n+1) × (m+1)` matrix of cost
//! and parent cells even when only the final scalar is wanted: `O(nm)` memory
//! for an `O(nm)` computation. This module picks the cheapest structure that
//! can answer the question actually asked:
//!
//! | Mode                              | Working set | Why |
//! |-----------------------------------|-------------|-----|
//! | distance, Levenshtein, unit cost, 1–64-unit shorter operand | one-word Myers state plus Peq lookup | Myers' (1999) bit-parallel algorithm computes the same answer in O(n) bitwise ops instead of O(n·m) scalar cells — see `plain_levenshtein_unit` |
//! | distance, Levenshtein, unit cost, 65+ unit shorter operand | multi-word Myers state plus packed Peq rows | Myers' state is carried across contiguous 64-bit blocks |
//! | distance, Levenshtein, weighted   | 1 row | each cell needs only `up`, `left`, `diag` |
//! | distance, OSA, unit cost          | Hyyrö bit-vector state | one extra register pair over Myers — see `osa_bit_vector` |
//! | distance, OSA, weighted           | 3 rows      | transposition reaches back to row − 2 |
//! | distance, unrestricted Damerau, unit cost | 3 rows + one saved-cell row | Zhao–Sahni's linear-space algorithm — see `damerau_zhao_sahni` |
//! | distance, unrestricted Damerau, weighted | full matrix | a weighted transposition reaches an arbitrary earlier row |
//! | search, Levenshtein, unit cost    | per-column bit-vector deltas | the parent of every cell is a pure function of its neighbours' costs, and unit-cost cell costs are recoverable from Myers' `Pv`/`Mv` words — see `search_bits` |
//! | search (every other combination)  | full matrix | weighted costs have no bit-vector form, and transposition parents depend on `last_row_map` state that cell costs alone cannot recover |
//!
//! All three unit-cost distance paths first strip the operands' common prefix
//! and suffix (`trim_common_affixes`), the reduction that makes
//! near-identical pairs almost free.
//!
//! The row-based paths turn an `O(nm)` allocation into an `O(m)` one and keep
//! the whole working set in cache. The bit-vector paths go further for the
//! cases they cover — no per-cell allocation, and 64 dynamic-programming rows
//! represented by each machine word — closing most of the gap
//! `docs/PERFORMANCE_GAPS.md` entry 26 documents against `triple_accel`'s SIMD
//! without needing `unsafe`, which this workspace's `unsafe_code = "deny"`
//! policy rules out by default.
//!
//! Each of those bit-vector paths builds its pattern-match table (`Peq`)
//! inside the call and drops it again, which is right for a single pair and
//! wasteful for a fixed query compared against a whole candidate set. For
//! that shape, [`crate::prepared::PreparedPattern`] builds the table once and
//! drives [`levenshtein`] and [`osa`] from it; it shares these kernels rather
//! than reimplementing them, so the two shapes cannot answer differently.
//!
//! The tie-breaking those search paths implement is **observable**, so it is
//! specified for callers on [`SearchResult`] rather than here. The rule the
//! code follows: every "cheapest predecessor" comparison is a strict `<` over
//! the candidate order insert, delete, substitute, transpose, and the last
//! row's minimum is scanned from column 0 with the same strict comparison.
//! Changing either would change which span a search reports.

use crate::units::{
    BitPeq, Operands, Unit, UnitMap, common_prefix_len, common_suffix_len, dispatch,
};
use core::fmt;
use rustc_hash::FxHashMap;

/// The edit operation a [`CostError`] is about.
///
/// Sealed for the same reason [`CostError`] is: it is that error's payload, so
/// leaving it open would hand back at the inner layer the freedom the outer one
/// reserves. A metric with a fourth edit operation is a metric this crate could
/// plausibly grow.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operation {
    /// A unit of the target not matched from the source.
    Insertion,
    /// A unit of the source not matched into the target.
    Deletion,
    /// An aligned pair of differing units.
    Substitution,
    /// A swap of two adjacent units.
    Transposition,
}

impl fmt::Display for Operation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Insertion => "insertion",
            Self::Deletion => "deletion",
            Self::Substitution => "substitution",
            Self::Transposition => "transposition",
        })
    }
}

/// Why a cost set was rejected by one of the three cost constructors.
///
/// Every variant carries the offending value, so a caller can log what it
/// actually passed rather than what it meant to pass.
///
/// # Equality is bitwise, so it is reflexive
///
/// [`PartialEq`] is written by hand rather than derived, and compares the
/// `f64` payloads by [`f64::to_bits`]. A derived impl would compare them
/// with `==`, under which `NaN != NaN` — and `NaN` is the canonical way to
/// reach [`NotFinite`](Self::NotFinite), so the derived impl made the error
/// unequal to *itself* in its most common case, breaking `assert_eq!` on
/// errors and any `Result` comparison in a test:
///
/// ```
/// use verbora_distance::{CostError, LevenshteinCosts, Operation};
///
/// let rejected = LevenshteinCosts::new(f64::NAN, 1.0, 1.0);
/// assert_eq!(rejected, rejected);
/// assert_eq!(
///     rejected,
///     Err(CostError::NotFinite { operation: Operation::Insertion, value: f64::NAN })
/// );
/// ```
///
/// Bit equality is a genuine equivalence relation — reflexive, symmetric and
/// transitive over every `f64` — so [`Eq`] is implemented too. It is *finer*
/// than `==` at one point only, `-0.0` versus `0.0`, which is reachable in
/// exactly one place: a `transposition` supplied as `-0.0` and rejected by
/// the threshold. Distinguishing it is the right answer there, because the
/// variant's whole job is to report the value as supplied. (`NotFinite`
/// carries only non-finite values; `Negative` only values satisfying
/// `value < 0.0`, which `-0.0` does not; and a rejection's `minimum` is
/// always strictly positive, since a rejection requires
/// `insertion + deletion` to be strictly positive.)
///
/// The three cost *types* keep their derived `PartialEq`: they can only hold
/// finite values, so a derived comparison is already reflexive there.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub enum CostError {
    /// The cost is `NaN` or infinite. A distance built from either is not a
    /// number a caller can threshold, rank or normalise.
    NotFinite {
        /// Which operation the cost priced.
        operation: Operation,
        /// The value as supplied.
        value: f64,
    },
    /// The cost is negative. A "distance" of `-4.0` between a string and
    /// itself is not a distance, so Verbora does not ship one. Zero *is*
    /// admissible.
    Negative {
        /// Which operation the cost priced.
        operation: Operation,
        /// The value as supplied.
        value: f64,
    },
    /// [`DamerauCosts::new`] only: the transposition cost is below Lowrance
    /// & Wagner's threshold. See that constructor.
    TranspositionBelowThreshold {
        /// The transposition cost as supplied.
        transposition: f64,
        /// Lowrance & Wagner's threshold for the supplied insertion and
        /// deletion costs — the real number `(insertion + deletion) / 2`,
        /// rounded to the nearest `f64`.
        ///
        /// **Always finite**, for every pair of admissible insertion and
        /// deletion costs. Both are finite, so their true mean is at most
        /// `f64::MAX` even when their *sum* is not representable; the
        /// constructor halves each operand in that case rather than
        /// reporting the sum's `+inf`, which is not a threshold any cost
        /// could meet.
        ///
        /// It is a **diagnostic**, not the comparison performed. The
        /// admission test is [`DamerauCosts::new`]'s normative predicate
        /// `2 * transposition >= insertion + deletion`, which is not routed
        /// through this value. The threshold frequently has no exact `f64` —
        /// at the subnormal end, where halving loses the low bit, and equally
        /// at the top of the range, where `insertion + deletion` itself
        /// rounds — so the value here can land on the rejected
        /// `transposition` itself, and this field is not to be read as "any
        /// value at least this large is accepted". The predicate is the
        /// authority; this is the number to put in a log line.
        minimum: f64,
    },
}

impl PartialEq for CostError {
    fn eq(&self, other: &Self) -> bool {
        /// Bit equality: `NaN` equals itself, which `==` does not give.
        fn same(a: f64, b: f64) -> bool {
            a.to_bits() == b.to_bits()
        }
        match (self, other) {
            (
                Self::NotFinite {
                    operation: a,
                    value: x,
                },
                Self::NotFinite {
                    operation: b,
                    value: y,
                },
            )
            | (
                Self::Negative {
                    operation: a,
                    value: x,
                },
                Self::Negative {
                    operation: b,
                    value: y,
                },
            ) => a == b && same(*x, *y),
            (
                Self::TranspositionBelowThreshold {
                    transposition: t1,
                    minimum: m1,
                },
                Self::TranspositionBelowThreshold {
                    transposition: t2,
                    minimum: m2,
                },
            ) => same(*t1, *t2) && same(*m1, *m2),
            _ => false,
        }
    }
}

impl Eq for CostError {}

impl fmt::Display for CostError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFinite { operation, value } => {
                write!(f, "{operation} cost must be finite; got {value}")
            }
            Self::Negative { operation, value } => {
                write!(f, "{operation} cost must be non-negative; got {value}")
            }
            Self::TranspositionBelowThreshold {
                transposition,
                minimum,
            } => write!(
                f,
                "unrestricted Damerau–Levenshtein requires 2 * transposition >= \
                 insertion + deletion (Lowrance & Wagner 1975); got transposition \
                 = {transposition} against a threshold (insertion + deletion) / 2 \
                 of {minimum}. Below that threshold the Lowrance–Wagner recurrence \
                 does not return the minimum-cost edit script. Use `OsaCosts`, \
                 which is defined for every admissible cost set, or raise the \
                 transposition cost"
            ),
        }
    }
}

impl std::error::Error for CostError {}

/// Rejects a single cost that is not finite and non-negative.
///
/// Returns the error rather than a `Result<(), _>` so the three constructors
/// can stay `const fn` without `?`, which is not available in a `const`
/// context.
const fn check(operation: Operation, value: f64) -> Option<CostError> {
    // `-0.0 < 0.0` is false, so a negative zero is accepted: it *is* zero,
    // and zero is admissible.
    if !value.is_finite() {
        return Some(CostError::NotFinite { operation, value });
    }
    if value < 0.0 {
        return Some(CostError::Negative { operation, value });
    }
    None
}

/// Edit costs for [`levenshtein_weighted`] and [`levenshtein_search_weighted`].
///
/// Three fields, because plain Levenshtein has no transposition operation:
/// a transposition cost cannot be handed to a function that would discard it.
/// Construct with [`new`](Self::new), which rejects every cost a distance
/// cannot be built from.
///
/// There is no `Default`, and that is deliberate: the one cost set everybody
/// wants is reachable *without* a cost type at all, by calling
/// [`levenshtein`]. A `Default` would exist only to let callers write the
/// slow path for the fast case.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LevenshteinCosts {
    insertion: f64,
    deletion: f64,
    substitution: f64,
}

impl LevenshteinCosts {
    /// A validated Levenshtein cost set.
    ///
    /// # Errors
    ///
    /// [`CostError::NotFinite`] if any cost is `NaN` or infinite;
    /// [`CostError::Negative`] if any cost is negative. Zero is admissible —
    /// `LevenshteinCosts::new(0.0, 0.0, 0.0)` prices every edit script at
    /// `0.0` by construction, making the result a pseudometric rather than a
    /// metric.
    ///
    /// ```
    /// use verbora_distance::{CostError, LevenshteinCosts, Operation};
    ///
    /// assert!(LevenshteinCosts::new(1.0, 2.0, 0.0).is_ok());
    /// assert_eq!(
    ///     LevenshteinCosts::new(1.0, -1.0, 1.0),
    ///     Err(CostError::Negative { operation: Operation::Deletion, value: -1.0 })
    /// );
    /// assert!(LevenshteinCosts::new(f64::INFINITY, 1.0, 1.0).is_err());
    /// ```
    pub const fn new(insertion: f64, deletion: f64, substitution: f64) -> Result<Self, CostError> {
        if let Some(e) = check(Operation::Insertion, insertion) {
            return Err(e);
        }
        if let Some(e) = check(Operation::Deletion, deletion) {
            return Err(e);
        }
        if let Some(e) = check(Operation::Substitution, substitution) {
            return Err(e);
        }
        Ok(Self {
            insertion,
            deletion,
            substitution,
        })
    }

    /// The price of inserting one unit.
    #[must_use]
    pub const fn insertion(&self) -> f64 {
        self.insertion
    }

    /// The price of deleting one unit.
    #[must_use]
    pub const fn deletion(&self) -> f64 {
        self.deletion
    }

    /// The price of substituting one unit for another.
    #[must_use]
    pub const fn substitution(&self) -> f64 {
        self.substitution
    }

    /// The internal four-field form. Plain Levenshtein never evaluates a
    /// transposition candidate, so the fourth field is `INFINITY`: the value
    /// that would still be *correct* if some future path did evaluate one,
    /// since a candidate priced at infinity can never win a minimum.
    const fn costs(&self) -> Costs {
        Costs {
            insertion: self.insertion,
            deletion: self.deletion,
            substitution: self.substitution,
            transposition: f64::INFINITY,
        }
    }
}

/// Edit costs for [`osa_weighted`] and [`osa_search_weighted`].
///
/// Four fields. Optimal string alignment's recurrence *defines* its answer as
/// the minimum over alignments editing no position twice, so every admissible
/// cost set is sound here — including a transposition cheaper than the
/// insertion/deletion pair it stands in for, which
/// [`DamerauCosts`] must reject.
///
/// There is no `Default` and no conversion to or from the sibling cost types;
/// see [`LevenshteinCosts`] for why.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OsaCosts {
    insertion: f64,
    deletion: f64,
    substitution: f64,
    transposition: f64,
}

impl OsaCosts {
    /// A validated OSA cost set.
    ///
    /// # Errors
    ///
    /// [`CostError::NotFinite`] if any cost is `NaN` or infinite;
    /// [`CostError::Negative`] if any cost is negative. Zero is admissible,
    /// and so is any finite non-negative transposition cost.
    ///
    /// ```
    /// use verbora_distance::{OsaCosts, osa_weighted};
    ///
    /// // A cheap transposition is sound for OSA, and rejected by DamerauCosts.
    /// let costs = OsaCosts::new(1.0, 1.0, 1.0, 0.25)?;
    /// assert_eq!(osa_weighted("ab", "ba", &costs), 0.25);
    /// # Ok::<(), verbora_distance::CostError>(())
    /// ```
    pub const fn new(
        insertion: f64,
        deletion: f64,
        substitution: f64,
        transposition: f64,
    ) -> Result<Self, CostError> {
        if let Some(e) = check(Operation::Insertion, insertion) {
            return Err(e);
        }
        if let Some(e) = check(Operation::Deletion, deletion) {
            return Err(e);
        }
        if let Some(e) = check(Operation::Substitution, substitution) {
            return Err(e);
        }
        if let Some(e) = check(Operation::Transposition, transposition) {
            return Err(e);
        }
        Ok(Self {
            insertion,
            deletion,
            substitution,
            transposition,
        })
    }

    /// The price of inserting one unit.
    #[must_use]
    pub const fn insertion(&self) -> f64 {
        self.insertion
    }

    /// The price of deleting one unit.
    #[must_use]
    pub const fn deletion(&self) -> f64 {
        self.deletion
    }

    /// The price of substituting one unit for another.
    #[must_use]
    pub const fn substitution(&self) -> f64 {
        self.substitution
    }

    /// The price of swapping two adjacent units.
    #[must_use]
    pub const fn transposition(&self) -> f64 {
        self.transposition
    }

    /// The internal four-field form.
    const fn costs(&self) -> Costs {
        Costs {
            insertion: self.insertion,
            deletion: self.deletion,
            substitution: self.substitution,
            transposition: self.transposition,
        }
    }
}

/// Edit costs for [`damerau_levenshtein_weighted`] and
/// [`damerau_levenshtein_search_weighted`].
///
/// The same four costs [`OsaCosts`] carries, plus one discharged
/// precondition: this type cannot be constructed with a transposition cost
/// below Lowrance & Wagner's threshold, so no unrestricted
/// Damerau–Levenshtein call can be made with a cost set the recurrence does
/// not answer for. There is no runtime check downstream and no panic.
///
/// There is no `Default` and no conversion to or from the sibling cost types;
/// see [`LevenshteinCosts`] for why.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DamerauCosts {
    insertion: f64,
    deletion: f64,
    substitution: f64,
    transposition: f64,
}

impl DamerauCosts {
    /// A validated unrestricted Damerau–Levenshtein cost set.
    ///
    /// # Errors
    ///
    /// [`CostError::NotFinite`] if any cost is `NaN` or infinite;
    /// [`CostError::Negative`] if any cost is negative; and
    /// [`CostError::TranspositionBelowThreshold`] unless
    ///
    /// ```text
    /// 2 * transposition >= insertion + deletion
    /// ```
    ///
    /// which is Lowrance & Wagner's (1975) precondition, not a Verbora
    /// restriction. Below that threshold a chain of adjacent swaps is a
    /// cheaper way to move a unit than deleting and re-inserting it, and
    /// their recurrence — which credits at most one transposition per
    /// matching row/column pair — stops ranging over every edit script, so
    /// what it returns is *a* script's cost rather than the minimum. Measured
    /// with a Dijkstra search over edit scripts as the reference: at
    /// `insertion = 1, deletion = 1, substitution = 5, transposition = 0.999`
    /// the recurrence reports `d("aab", "baa") = 2` where two transpositions
    /// achieve `1.998`.
    ///
    /// # How the predicate is evaluated
    ///
    /// **The predicate above is evaluated as written**, in `f64`:
    /// `2.0 * transposition < insertion + deletion` is the rejection test.
    /// It is *not* rearranged into `transposition < (insertion + deletion) /
    /// 2.0`. The two agree wherever `insertion + deletion` is a normal
    /// `f64` — halving and doubling are both exact there, and rounding
    /// commutes with scaling by a power of two — but they part company at
    /// the ends of the range, and the rearranged form is wrong there:
    ///
    /// * **The sum overflows.** With all four costs at `f64::MAX`, `2 *
    ///   transposition` and `insertion + deletion` are both `+inf`, so
    ///   `inf >= inf` admits the set — as it must, since `2 * f64::MAX >=
    ///   f64::MAX + f64::MAX` holds in the real numbers. The rearranged form
    ///   computes a threshold of `inf / 2.0 == inf` and rejects a cost set
    ///   that satisfies Lowrance & Wagner exactly.
    /// * **The sum is subnormal.** Halving a subnormal loses its low bit, so
    ///   the rearranged form's threshold can round *down* below the true
    ///   mean and admit a transposition the predicate excludes.
    ///
    /// Against the real-number predicate, the `f64` form never over-rejects:
    /// `2 * transposition` is exact unless it overflows, and no `f64` lies
    /// between `insertion + deletion` and its rounded sum. It over-accepts
    /// in exactly one regime — `transposition` above `f64::MAX / 2`, where
    /// the doubling saturates to `+inf` and the test admits unconditionally.
    /// That regime is not observable: every edit script that could
    /// distinguish a chain of such transpositions from a delete-and-reinsert
    /// costs more than `f64::MAX` and saturates to `+inf` too, so the
    /// recurrence still returns the minimum of the values `f64` can hold
    /// (see `docs/design/distance-contract.md` §3.1, "Numeric limits").
    ///
    /// [`osa_weighted`] imposes no such condition, and the unit-cost
    /// [`damerau_levenshtein`] evaluates `(1, 1, 1, 1)`, which satisfies the
    /// threshold with equality (`2 ≥ 2`).
    ///
    /// ```
    /// use verbora_distance::DamerauCosts;
    ///
    /// assert!(DamerauCosts::new(1.0, 1.0, 1.0, 1.0).is_ok());
    /// assert!(DamerauCosts::new(0.5, 0.5, 1.0, 0.5).is_ok()); // 1.0 >= 1.0
    /// assert!(DamerauCosts::new(1.0, 1.0, 5.0, 0.999).is_err()); // 1.998 < 2.0
    ///
    /// // 2 * f64::MAX >= f64::MAX + f64::MAX, so this is admissible.
    /// assert!(DamerauCosts::new(f64::MAX, f64::MAX, f64::MAX, f64::MAX).is_ok());
    /// ```
    pub const fn new(
        insertion: f64,
        deletion: f64,
        substitution: f64,
        transposition: f64,
    ) -> Result<Self, CostError> {
        if let Some(e) = check(Operation::Insertion, insertion) {
            return Err(e);
        }
        if let Some(e) = check(Operation::Deletion, deletion) {
            return Err(e);
        }
        if let Some(e) = check(Operation::Substitution, substitution) {
            return Err(e);
        }
        if let Some(e) = check(Operation::Transposition, transposition) {
            return Err(e);
        }
        // Lowrance & Wagner's predicate, spelled exactly as the contract
        // states it. See this constructor's "How the predicate is evaluated"
        // section for why it is not rearranged into a comparison against
        // `minimum`.
        let sum = insertion + deletion;
        if 2.0 * transposition < sum {
            // `minimum` is a diagnostic, and must be a number the caller can
            // read as a threshold — so it is never `+inf`. The true mean of
            // two finite costs is at most `f64::MAX`; halving each operand
            // keeps it in range when their sum is not representable, and the
            // sum is used wherever it is, since halving *it* is exact for
            // every normal result and correct for subnormal ones where
            // halving the operands separately is not.
            let minimum = if sum.is_finite() {
                sum / 2.0
            } else {
                insertion / 2.0 + deletion / 2.0
            };
            return Err(CostError::TranspositionBelowThreshold {
                transposition,
                minimum,
            });
        }
        Ok(Self {
            insertion,
            deletion,
            substitution,
            transposition,
        })
    }

    /// The price of inserting one unit.
    #[must_use]
    pub const fn insertion(&self) -> f64 {
        self.insertion
    }

    /// The price of deleting one unit.
    #[must_use]
    pub const fn deletion(&self) -> f64 {
        self.deletion
    }

    /// The price of substituting one unit for another.
    #[must_use]
    pub const fn substitution(&self) -> f64 {
        self.substitution
    }

    /// The price of swapping two adjacent units.
    #[must_use]
    pub const fn transposition(&self) -> f64 {
        self.transposition
    }

    /// The internal four-field form.
    const fn costs(&self) -> Costs {
        Costs {
            insertion: self.insertion,
            deletion: self.deletion,
            substitution: self.substitution,
            transposition: self.transposition,
        }
    }
}

/// The four costs the weighted recurrences read, in one record.
///
/// Private, and deliberately unvalidated: validation is a property of the
/// (cost set, algorithm) pair and has already happened at the public
/// boundary, where the caller's choice of function fixed the algorithm. Once
/// inside the crate the shared kernels want one shape, not three.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Costs {
    insertion: f64,
    deletion: f64,
    substitution: f64,
    transposition: f64,
}

impl Costs {
    /// The cost set the unit-cost entry points evaluate.
    ///
    /// Only two paths need it — unit-cost unrestricted-Damerau and OSA
    /// *search*, which have no bit-parallel form and therefore share the
    /// weighted tier's full matrix. Every other unit-cost path runs a kernel
    /// with no cost parameter at all.
    const UNIT: Self = Self {
        insertion: 1.0,
        deletion: 1.0,
        substitution: 1.0,
        transposition: 1.0,
    };
}

/// The result of a search-mode call: a slice of the **target**, where it sits,
/// and how far it is from the source.
///
/// `D` is the distance type of the tier that produced it: `usize` for the
/// unit-cost searches, `f64` for the weighted ones.
///
/// The lifetime is named rather than elided, which usefully documents that
/// this borrows the *target* and never the source.
///
/// # Guarantees
///
/// For every cost set, every variant and every input:
///
/// 1. `&target[r.range()] == r.substring()`. The matched text genuinely
///    occurs in the target, at the reported position. This holds *by
///    construction*: the substring is produced by slicing the target at the
///    reported range, so there is nothing to assert.
/// 2. `metric(source, r.substring()) == r.distance()`, where `metric` is the
///    distance function matching the search function called — exactly,
///    including under weighted costs.
/// 3. Ties resolve to the first candidate in insert → delete → substitute →
///    transpose order, at the earliest end column, with the empty substring
///    ahead of all of them.
///
/// # Tie-breaking is observable
///
/// A search picks the cheapest predecessor of each cell with a strict `<`,
/// which keeps the **first** candidate on ties; the candidate order is
/// insert, delete, substitute, transpose. The final row's minimum is scanned
/// with the same strict comparison, starting from column 0, so the earliest
/// end column — and the empty substring ahead of all of them — wins a tie.
///
/// Cost totals are unaffected by tie-breaking, but the alignment chosen is,
/// and the alignment is what fixes the span [`substring`](Self::substring) and
/// [`range`](Self::range) report. The order is therefore part of the
/// specification rather than an incidental property of the implementation: two
/// equally-cheap spans always resolve the same way, on every platform and in
/// every release.
///
/// What tie-breaking may **not** do is change the answer's meaning. Whichever
/// alignment wins, it is followed all the way back to its start, so
/// `r.range()` spans exactly where that alignment starts and ends,
/// `&target[r.range()] == r.substring()`, and
/// `distance(source, r.substring()) == r.distance()` for every cost set. That
/// is pinned against a brute force over every substring of the target which
/// shares no code with the search routines.
///
/// # The cost of borrowing
///
/// This removes one allocation per call for the *search → read → discard*
/// shape and makes the allocation opt-in (`r.substring().to_owned()`) rather
/// than mandatory. For the *search a corpus → keep the good hits* shape it is
/// a memory regression: a `Vec<SearchResult<'t, _>>` pins every target alive.
/// A caller who filters and keeps should copy out `(range, distance)` or own
/// the substring at the filter point.
///
/// ```
/// use verbora_distance::levenshtein_search;
///
/// let target = "Zürich, Berlin, Wien";
/// let found = levenshtein_search("Berlin", target);
///
/// assert_eq!(found.substring(), "Berlin");
/// assert_eq!(found.distance(), 0);
/// // A byte range, so it slices the target directly — even though "Zürich"
/// // contains a two-byte character.
/// assert_eq!(found.range(), 9..15);
/// assert_eq!(&target[found.range()], found.substring());
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SearchResult<'t, D> {
    /// The best-matching substring of the target, borrowed from it.
    substring: &'t str,
    /// That substring's **byte** offset in the target.
    start: usize,
    /// The edit distance to that substring.
    distance: D,
}

impl<'t, D: Copy> SearchResult<'t, D> {
    /// The matched text, borrowed from `target`.
    #[must_use]
    pub fn substring(&self) -> &'t str {
        self.substring
    }

    /// The match's **byte** range in `target`: `&target[r.range()] ==
    /// r.substring()`.
    ///
    /// Derived from [`substring`](Self::substring), never stored, so the two
    /// cannot disagree. A scalar boundary is always a byte boundary, so the
    /// range is always sliceable.
    ///
    /// A caller who wants a *scalar* offset instead writes
    /// `target[..r.range().start].chars().count()`.
    #[must_use]
    pub fn range(&self) -> core::ops::Range<usize> {
        self.start..self.start + self.substring.len()
    }

    /// The distance from `source` to [`substring`](Self::substring), under the
    /// metric that produced this result.
    #[must_use]
    pub fn distance(&self) -> D {
        self.distance
    }
}

/// Levenshtein distance between `source` and `target`, in edits.
///
/// Insertions, deletions and substitutions only — an adjacent swap costs two
/// edits. There is no cost argument: this *is* the unit-cost metric, and
/// [`levenshtein_weighted`] is the function that prices operations
/// differently. See
/// [*Unit costs or weighted costs*](crate#unit-costs-or-weighted-costs) for
/// which to reach for, and [`PreparedPattern`](crate::PreparedPattern) when
/// one operand is fixed across many comparisons.
///
/// ```
/// use verbora_distance::levenshtein;
///
/// assert_eq!(levenshtein("kitten", "sitting"), 3);
/// assert_eq!(levenshtein("ab", "ba"), 2);
/// assert_eq!(levenshtein("", "abc"), 3);
/// ```
#[must_use]
pub fn levenshtein(source: &str, target: &str) -> usize {
    levenshtein_unit_impl(source, target)
}

/// Levenshtein distance between `source` and `target`, priced by `costs`.
///
/// The minimum total cost of an edit script over insertions, deletions and
/// substitutions. Evaluated by a scalar dynamic program: the bit-parallel
/// kernel [`levenshtein`] uses has no notion of a weighted operation.
///
/// Equal to `levenshtein(source, target) as f64` when every cost is `1.0`,
/// bit for bit — pinned by this module's own tests.
///
/// ```
/// use verbora_distance::{LevenshteinCosts, levenshtein_weighted};
///
/// let costs = LevenshteinCosts::new(1.0, 3.0, 1.0)?;
/// // Deleting the "c" costs 3; substituting is not available against a
/// // shorter target.
/// assert_eq!(levenshtein_weighted("abc", "ab", &costs), 3.0);
/// # Ok::<(), verbora_distance::CostError>(())
/// ```
#[must_use]
pub fn levenshtein_weighted(source: &str, target: &str, costs: &LevenshteinCosts) -> f64 {
    levenshtein_weighted_impl(source, target, &costs.costs())
}

/// Unrestricted Damerau–Levenshtein distance between `source` and `target`,
/// in edits.
///
/// The canonical Lowrance–Wagner algorithm: insertions, deletions,
/// substitutions and transpositions of two characters that are adjacent *in
/// the source*, with the substring between a transposed pair free to be edited
/// as well. This is a true metric — symmetric, and satisfying the triangle
/// inequality — both pinned by this module's own property tests.
///
/// Use [`osa`] instead when a swapped pair must never be edited again; it is
/// the cheaper, more restrictive rule, and the two genuinely differ.
///
/// ```
/// use verbora_distance::{damerau_levenshtein, osa};
///
/// assert_eq!(damerau_levenshtein("ab", "ba"), 1);
/// // Unrestricted Damerau reaches "ABC" from "CA" in two edits; OSA needs three.
/// assert_eq!(damerau_levenshtein("CA", "ABC"), 2);
/// assert_eq!(osa("CA", "ABC"), 3);
/// // Symmetric, unlike an optimal-alignment-style recurrence.
/// assert_eq!(damerau_levenshtein("bb", "abbb"), 2);
/// assert_eq!(damerau_levenshtein("abbb", "bb"), 2);
/// ```
#[must_use]
pub fn damerau_levenshtein(source: &str, target: &str) -> usize {
    damerau_unit_impl(source, target)
}

/// Unrestricted Damerau–Levenshtein distance between `source` and `target`,
/// priced by `costs`.
///
/// Lowrance & Wagner's cost precondition is discharged by
/// [`DamerauCosts::new`], so this function cannot be reached with a cost set
/// its recurrence does not answer for. It does not panic, for any input.
///
/// ```
/// use verbora_distance::{DamerauCosts, damerau_levenshtein_weighted};
///
/// let costs = DamerauCosts::new(1.0, 1.0, 1.5, 1.0)?;
/// assert_eq!(damerau_levenshtein_weighted("ab", "ba", &costs), 1.0);
/// assert_eq!(damerau_levenshtein_weighted("ab", "ax", &costs), 1.5);
/// # Ok::<(), verbora_distance::CostError>(())
/// ```
#[must_use]
pub fn damerau_levenshtein_weighted(source: &str, target: &str, costs: &DamerauCosts) -> f64 {
    damerau_weighted_impl(source, target, &costs.costs())
}

/// Optimal string alignment (restricted Damerau–Levenshtein) distance between
/// `source` and `target`, in edits.
///
/// A transposition of two adjacent characters costs one operation, but neither
/// character may take part in any further edit — equivalently, no substring
/// between two swapped characters may itself be edited. That restriction makes
/// OSA cheaper to compute than [`damerau_levenshtein`] (a bit-parallel kernel
/// applies) at the price of not satisfying the triangle inequality. It is
/// symmetric.
///
/// ```
/// use verbora_distance::osa;
///
/// assert_eq!(osa("ab", "ba"), 1);
/// assert_eq!(osa("CA", "ABC"), 3);
/// ```
#[must_use]
pub fn osa(source: &str, target: &str) -> usize {
    osa_unit_impl(source, target)
}

/// Optimal string alignment distance between `source` and `target`, priced by
/// `costs`.
///
/// Every admissible cost set is sound here, including a transposition cheaper
/// than the delete/insert pair it replaces: OSA's recurrence *defines* its
/// answer as the minimum over alignments editing no position twice.
///
/// ```
/// use verbora_distance::{OsaCosts, osa_weighted};
///
/// let costs = OsaCosts::new(1.0, 1.0, 1.0, 0.25)?;
/// assert_eq!(osa_weighted("ab", "ba", &costs), 0.25);
/// # Ok::<(), verbora_distance::CostError>(())
/// ```
#[must_use]
pub fn osa_weighted(source: &str, target: &str, costs: &OsaCosts) -> f64 {
    osa_weighted_impl(source, target, &costs.costs())
}

/// Finds the substring of `target` closest to `source` under Levenshtein, in
/// edits.
///
/// The result borrows the target and reports a **byte** range into it, so it
/// can be used as a highlight span directly. The search is total: the empty
/// substring is always a candidate, so there is always a best match and no
/// `Option` to unwrap. "Close enough" is the caller's threshold.
///
/// ```
/// use verbora_distance::levenshtein_search;
///
/// let target = "the quick brown fox";
/// let found = levenshtein_search("brwn", target);
///
/// assert_eq!(found.substring(), "brown");
/// assert_eq!(found.distance(), 1);
/// assert_eq!(found.range(), 10..15);
///
/// // The range slices the target, so highlighting is a split, not a search.
/// let r = found.range();
/// let (before, rest) = target.split_at(r.start);
/// let (hit, after) = rest.split_at(r.len());
/// assert_eq!((before, hit, after), ("the quick ", "brown", " fox"));
/// ```
#[must_use]
pub fn levenshtein_search<'t>(source: &str, target: &'t str) -> SearchResult<'t, usize> {
    search_unit_impl(source, target, Variant::Plain)
}

/// Finds the substring of `target` closest to `source` under Levenshtein,
/// priced by `costs`.
///
/// ```
/// use verbora_distance::{LevenshteinCosts, levenshtein_search_weighted};
///
/// // Deletions are cheap, insertions are not: the match may drop characters
/// // of the source rather than pad the window.
/// let costs = LevenshteinCosts::new(3.0, 0.5, 1.0)?;
/// let found = levenshtein_search_weighted("brown", "the quick brwn fox", &costs);
///
/// assert_eq!(found.substring(), "brwn");
/// assert_eq!(found.distance(), 0.5);
/// # Ok::<(), verbora_distance::CostError>(())
/// ```
#[must_use]
pub fn levenshtein_search_weighted<'t>(
    source: &str,
    target: &'t str,
    costs: &LevenshteinCosts,
) -> SearchResult<'t, f64> {
    search_weighted_impl(source, target, &costs.costs(), Variant::Plain)
}

/// Finds the substring of `target` closest to `source` under unrestricted
/// Damerau–Levenshtein, in edits.
///
/// ```
/// use verbora_distance::damerau_levenshtein_search;
///
/// // One transposition of the adjacent "ro", not two substitutions.
/// let found = damerau_levenshtein_search("brown", "the qucik borwn fox");
/// assert_eq!(found.substring(), "borwn");
/// assert_eq!(found.distance(), 1);
/// assert_eq!(found.range(), 10..15);
/// ```
#[must_use]
pub fn damerau_levenshtein_search<'t>(source: &str, target: &'t str) -> SearchResult<'t, usize> {
    search_unit_impl(source, target, Variant::Damerau)
}

/// Finds the substring of `target` closest to `source` under unrestricted
/// Damerau–Levenshtein, priced by `costs`.
///
/// Cannot panic: [`DamerauCosts::new`] has already discharged Lowrance &
/// Wagner's precondition.
///
/// ```
/// use verbora_distance::{DamerauCosts, damerau_levenshtein_search_weighted};
///
/// let costs = DamerauCosts::new(1.0, 1.0, 1.5, 1.0)?;
/// let found = damerau_levenshtein_search_weighted("brown", "a borwn dog", &costs);
/// assert_eq!(found.substring(), "borwn");
/// assert_eq!(found.distance(), 1.0);
/// # Ok::<(), verbora_distance::CostError>(())
/// ```
#[must_use]
pub fn damerau_levenshtein_search_weighted<'t>(
    source: &str,
    target: &'t str,
    costs: &DamerauCosts,
) -> SearchResult<'t, f64> {
    search_weighted_impl(source, target, &costs.costs(), Variant::Damerau)
}

/// Finds the substring of `target` closest to `source` under optimal string
/// alignment (restricted Damerau–Levenshtein), in edits.
///
/// ```
/// use verbora_distance::osa_search;
///
/// // Non-ASCII text ahead of the match: the reported range is in bytes, so
/// // it indexes the target correctly where a scalar offset would not.
/// // "Zürich, " is 8 characters but 9 bytes, because "ü" takes two.
/// let target = "Zürich, Belrin, Wien";
/// let found = osa_search("Berlin", target);
///
/// assert_eq!(found.substring(), "Belrin"); // one transposition away
/// assert_eq!(found.distance(), 1);
/// assert_eq!(found.range(), 9..15);
/// assert_eq!(&target[found.range()], found.substring());
/// ```
#[must_use]
pub fn osa_search<'t>(source: &str, target: &'t str) -> SearchResult<'t, usize> {
    search_unit_impl(source, target, Variant::Osa)
}

/// Finds the substring of `target` closest to `source` under optimal string
/// alignment, priced by `costs`.
///
/// ```
/// use verbora_distance::{OsaCosts, osa_search_weighted};
///
/// let costs = OsaCosts::new(1.0, 1.0, 1.0, 0.25)?;
/// let found = osa_search_weighted("ab", "xxbaxx", &costs);
/// assert_eq!(found.substring(), "ba");
/// assert_eq!(found.distance(), 0.25);
/// # Ok::<(), verbora_distance::CostError>(())
/// ```
#[must_use]
pub fn osa_search_weighted<'t>(
    source: &str,
    target: &'t str,
    costs: &OsaCosts,
) -> SearchResult<'t, f64> {
    search_weighted_impl(source, target, &costs.costs(), Variant::Osa)
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
/// `pairs.par_iter().map(|(a, b)| levenshtein(a, b)).collect()` — a thin
/// fan-out over the existing sequential primitive, not a second
/// implementation of it. The kernel dispatch inside `levenshtein` itself is
/// untouched; if you need `levenshtein_search`, a weighted variant, or
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
/// `pairs.iter().map(|(a, b)| levenshtein(a, b)).collect()` loop.
///
/// # Allocation behaviour
///
/// One `Vec<usize>` sized to `pairs.len()` for the output, plus whatever
/// `levenshtein` itself allocates per pair. Unit-cost ASCII uses integer
/// bit-vectors and a Peq table; multi-word Peq rows and long non-ASCII operands
/// may allocate, while short Unicode operands use fixed stack buffers. No
/// additional buffering, no locking, no per-call thread-pool construction —
/// this uses whichever global `rayon` pool is already installed (or `rayon`'s
/// default one), so pool configuration remains the caller's responsibility,
/// not this crate's.
///
/// # Order and errors
///
/// Output order matches input order — `results[i] == levenshtein(pairs[i].0,
/// pairs[i].1)` — via `rayon`'s order-preserving `map` + `collect`.
/// `levenshtein` never errors and never panics, so every element is a plain
/// `usize`.
#[cfg(feature = "parallel")]
#[cfg_attr(docsrs, doc(cfg(feature = "parallel")))]
#[must_use]
pub fn par_levenshtein_batch(pairs: &[(&str, &str)]) -> Vec<usize> {
    use rayon::prelude::*;
    pairs.par_iter().map(|(a, b)| levenshtein(a, b)).collect()
}

/// [`damerau_levenshtein`], fanned out across a `rayon` thread pool. Requires
/// the `parallel` feature.
///
/// See [`par_levenshtein_batch`] for the full rationale, cost model and
/// allocation behaviour — identical here, since `damerau_levenshtein` differs
/// from `levenshtein` only in which internal dispatch path it takes
/// (Zhao–Sahni's linear-space rows vs. Myers' bit vectors), not in its
/// statelessness or thread-safety. This function is exactly
/// `pairs.par_iter().map(|(a, b)| damerau_levenshtein(a, b)).collect()`.
///
/// It cannot panic, for any input including an empty `pairs`: there is no
/// cost set to reject.
#[cfg(feature = "parallel")]
#[cfg_attr(docsrs, doc(cfg(feature = "parallel")))]
#[must_use]
pub fn par_damerau_levenshtein_batch(pairs: &[(&str, &str)]) -> Vec<usize> {
    use rayon::prelude::*;
    pairs
        .par_iter()
        .map(|(a, b)| damerau_levenshtein(a, b))
        .collect()
}

/// [`osa`], fanned out across a `rayon` thread pool. Requires the `parallel`
/// feature.
///
/// See [`par_levenshtein_batch`] for the full rationale, cost model and
/// allocation behaviour — identical here, since `osa` is the same kind of pure,
/// stateless function over two borrowed `&str`s. This function is exactly
/// `pairs.par_iter().map(|(a, b)| osa(a, b)).collect()`.
///
/// Kept as its own entry point rather than folded into
/// [`par_damerau_levenshtein_batch`] because the two compute different
/// distances; the crate's convention is one `par_*_batch` per public
/// sequential metric.
///
/// There is deliberately no weighted batch variant, here or for the siblings:
/// the weighted path is strictly heavier per pair, so the crossover at which
/// parallelism wins is *earlier* than the unit form's and the guidance above
/// is conservative for it. A caller with weighted costs writes the one-line
/// `par_iter().map(...)` themselves.
#[cfg(feature = "parallel")]
#[cfg_attr(docsrs, doc(cfg(feature = "parallel")))]
#[must_use]
pub fn par_osa_batch(pairs: &[(&str, &str)]) -> Vec<usize> {
    use rayon::prelude::*;
    pairs.par_iter().map(|(a, b)| osa(a, b)).collect()
}

// ---------------------------------------------------------------------------
// Distance mode
// ---------------------------------------------------------------------------

/// [`levenshtein`]'s body: the unit-cost tier, which is a count of edits and
/// never touches a cost value.
fn levenshtein_unit_impl(source: &str, target: &str) -> usize {
    // Avoid the row allocation and, for non-ASCII input, scalar
    // materialization entirely when one side is empty.
    if source.is_empty() {
        return target.chars().count();
    }
    if target.is_empty() {
        return source.chars().count();
    }

    if source.is_ascii() && target.is_ascii() {
        return plain_levenshtein_unit(source.as_bytes(), target.as_bytes());
    }

    // Large near-identical Unicode strings should not allocate and decode
    // their shared surroundings. The UTF-8 trim already backs off to scalar
    // boundaries, so it is now exactly the trim `plain_levenshtein_unit`
    // performs afterward over `&[char]` — the two can no longer disagree
    // about where a unit begins.
    let (source, target) = if source.len().min(target.len()) > 64 {
        trim_common_utf8_affixes(source, target)
    } else {
        (source, target)
    };
    if source.is_empty() {
        return target.chars().count();
    }
    if target.is_empty() {
        return source.chars().count();
    }
    if source.is_ascii() && target.is_ascii() {
        return plain_levenshtein_unit(source.as_bytes(), target.as_bytes());
    }

    with_scalars(source, target, plain_levenshtein_unit)
}

/// [`levenshtein_weighted`]'s body: the scalar recurrence, verbatim.
///
/// No affix trimming: the reduction's proof assumes a unit-cost matrix (see
/// [`trim_common_affixes`]), and there is no longer any runtime gate that
/// could apply it here by accident — the trim lives in
/// [`levenshtein_unit_impl`], which has no cost argument at all.
fn levenshtein_weighted_impl(source: &str, target: &str, costs: &Costs) -> f64 {
    // Preserve the scalar recurrence's repeated floating-point additions
    // exactly while avoiding both its row allocation and, for non-ASCII
    // input, scalar materialization when one side is empty. A fold of
    // repeated additions, not a multiplication: under IEEE-754 the two
    // differ, and the fold is what the general recurrence itself accumulates.
    if source.is_empty() {
        return repeated_cost(target.chars().count(), costs.insertion);
    }
    if target.is_empty() {
        return repeated_cost(source.chars().count(), costs.deletion);
    }

    if source.is_ascii() && target.is_ascii() {
        return plain_rows(source.as_bytes(), target.as_bytes(), costs);
    }
    with_scalars(source, target, |s, t| plain_rows(s, t, costs))
}

/// [`damerau_levenshtein`]'s body.
fn damerau_unit_impl(source: &str, target: &str) -> usize {
    if source.is_ascii() && target.is_ascii() {
        return damerau_unrestricted(source.as_bytes(), target.as_bytes());
    }

    // The same UTF-8 pre-trim [`levenshtein_unit_impl`] applies, and now for
    // the same reason: a near-identical Unicode pair should not pay to
    // encode its shared surroundings before the per-unit trim inside
    // [`damerau_unrestricted`] discards them anyway. This became legal
    // here only when the recurrence became canonical — see
    // [`trim_common_affixes`].
    let (source, target) = if source.len().min(target.len()) > 64 {
        trim_common_utf8_affixes(source, target)
    } else {
        (source, target)
    };
    if source.is_empty() {
        return target.chars().count();
    }
    if target.is_empty() {
        return source.chars().count();
    }
    if source.is_ascii() && target.is_ascii() {
        return damerau_unrestricted(source.as_bytes(), target.as_bytes());
    }
    with_scalars(source, target, damerau_unrestricted)
}

/// [`damerau_levenshtein_weighted`]'s body: the Lowrance–Wagner recurrence on
/// a full matrix, since a weighted transposition reaches an arbitrary earlier
/// row.
fn damerau_weighted_impl(source: &str, target: &str, costs: &Costs) -> f64 {
    dispatch(source, target, |ops| match ops {
        Operands::Bytes(s, t) => full_matrix(s, t, costs, Variant::Damerau, false).final_cost(),
        Operands::Units(s, t) => full_matrix(s, t, costs, Variant::Damerau, false).final_cost(),
    })
}

/// [`osa`]'s body.
fn osa_unit_impl(source: &str, target: &str) -> usize {
    dispatch(source, target, |ops| match ops {
        Operands::Bytes(s, t) => osa_unit(s, t),
        Operands::Units(s, t) => osa_unit(s, t),
    })
}

/// [`osa_weighted`]'s body: the three-row scalar recurrence.
fn osa_weighted_impl(source: &str, target: &str, costs: &Costs) -> f64 {
    dispatch(source, target, |ops| match ops {
        Operands::Bytes(s, t) => osa_rows(s, t, costs),
        Operands::Units(s, t) => osa_rows(s, t, costs),
    })
}

/// Runs `f` on `source` and `target` as Unicode scalar slices.
///
/// A UTF-8 string of *n* bytes holds at most *n* scalars (every scalar is at
/// least one byte), so a byte-length check is enough to know a fixed stack
/// buffer will fit — no counting pass, and no heap allocation for the
/// word-sized operands the common case is made of.
fn with_scalars<R>(source: &str, target: &str, f: impl FnOnce(&[char], &[char]) -> R) -> R {
    const STACK_UNITS: usize = 64;
    if source.len() <= STACK_UNITS && target.len() <= STACK_UNITS {
        let mut source_units = ['\0'; STACK_UNITS];
        let mut target_units = ['\0'; STACK_UNITS];
        let source_len = fill_scalars(source, &mut source_units);
        let target_len = fill_scalars(target, &mut target_units);
        return f(&source_units[..source_len], &target_units[..target_len]);
    }

    let source_units: Vec<char> = source.chars().collect();
    let target_units: Vec<char> = target.chars().collect();
    f(&source_units, &target_units)
}

/// `cost` added to itself `count` times, left to right.
///
/// A fold of repeated additions, **not** a multiplication: under IEEE-754 the
/// two differ, and the fold is what the general recurrence accumulates when it
/// walks a row of pure insertions. The weighted empty-operand answer is
/// specified as this fold (`docs/design/distance-contract.md` §3.1), so the
/// shortcut and the general path cannot disagree.
///
/// The unit tier has no use for it: there the same quantity is a count, and
/// `count` is already the answer.
#[inline]
fn repeated_cost(count: usize, cost: f64) -> f64 {
    (0..count).fold(0.0, |total, _| total + cost)
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

/// Fills `out` with `input`'s scalars, returning how many were written.
///
/// The caller has already established that `out` is long enough (a UTF-8
/// string of *n* bytes has at most *n* scalars), so the `zip` never truncates
/// in practice.
#[inline]
pub(crate) fn fill_scalars(input: &str, out: &mut [char]) -> usize {
    let mut len = 0usize;
    for (slot, unit) in out.iter_mut().zip(input.chars()) {
        *slot = unit;
        len += 1;
    }
    len
}

/// Canonical unrestricted Damerau–Levenshtein distance at unit costs.
///
/// Strips the operands' maximal common affixes ([`trim_common_affixes`]) and
/// then runs the cheapest correct kernel for what is left: a fixed stack
/// matrix for byte operands of at most [`DAMERAU_STACK_MAX`] units each
/// ([`damerau_unit_small`]), otherwise Zhao–Sahni's linear-space algorithm
/// ([`damerau_zhao_sahni`]).
///
/// The distance is symmetric, so the shorter operand becomes the *column*
/// operand — the one whose length decides the row width, and therefore the
/// whole kernel's memory footprint. Pinned by this module's symmetry tests.
///
/// Weighted costs never reach here — they are a different function
/// ([`damerau_levenshtein_weighted`]), routed to [`full_matrix`], which
/// evaluates the same Lowrance–Wagner recurrence with per-operation weights.
fn damerau_unrestricted<T: DamerauUnit>(source: &[T], target: &[T]) -> usize {
    let (source, target) = if source.len().min(target.len()) > TRIM_MIN_LEN {
        trim_common_affixes(source, target)
    } else {
        (source, target)
    };
    if source.is_empty() {
        return target.len();
    }
    if target.is_empty() {
        return source.len();
    }
    let (longer, shorter) = if source.len() >= target.len() {
        (source, target)
    } else {
        (target, source)
    };
    T::damerau_unit(longer, shorter)
}

/// Optimal string alignment (restricted Damerau) distance, choosing the
/// fastest correct algorithm for the input rather than always running
/// [`osa_rows`]'s scalar three-row DP.
///
/// The same decision [`plain_levenshtein_unit`] already makes for plain
/// Levenshtein, extended to the OSA variant: Hyyrö's 2003 transposition
/// extension of Myers' bit-vector algorithm computes the identical
/// unit-cost OSA distance in `O(n·m/64)` bitwise operations. The extension
/// over the plain kernels is exactly one extra register pair — the previous
/// column's `D0` word and the previous scanned character's pattern-match
/// mask — combined into a transposition mask `tr` that is OR-ed into `D0`.
/// Trusted only after this module's own differential tests pinned it against
/// [`osa_rows`], the scalar recurrence transcribed from the OSA definition.
///
/// There is no cost gate here any more, and that is the point: which kernel
/// runs is a structural property of which public function the caller called.
/// [`osa`] reaches this; [`osa_weighted`] reaches [`osa_rows`] instead, and no
/// cost value can move a call between the two. OSA under unit costs is
/// symmetric in its operands, so the shorter operand is always the bit-packed
/// pattern — the same operand-swap [`plain_levenshtein_unit`] performs, pinned
/// by this module's own symmetry tests.
///
/// The operands are first stripped of their maximal common prefix and suffix.
/// That is a genuine algorithmic reduction, not a micro-optimisation — a pair
/// differing in one interior position collapses from `O(n·m/64)` to `O(1)`
/// bit-vector work — and it is valid for OSA for a reason the
/// plain-Levenshtein case does not supply on its own; see
/// [`trim_common_affixes`]' own doc comment for the straddling-transposition
/// argument that makes it hold.
fn osa_unit<T: BitPeq>(source: &[T], target: &[T]) -> usize {
    let (source, target) = if source.len().min(target.len()) > TRIM_MIN_LEN {
        trim_common_affixes(source, target)
    } else {
        (source, target)
    };
    // Unit-cost OSA against an empty operand is the other operand's
    // length: there is nothing to transpose or substitute against.
    if source.is_empty() {
        return target.len();
    }
    if target.is_empty() {
        return source.len();
    }
    let (shorter, longer) = if source.len() <= target.len() {
        (source, target)
    } else {
        (target, source)
    };
    // The lower bound is 1, not the 2 this path started at. A one-unit
    // pattern makes the transposition mask identically zero — `tr`'s
    // `<< 1` shifts the only meaningful bit out of range, and the
    // `& prev_pm` that follows can never restore it — so the kernel
    // degenerates to plain Myers, which is exactly right (no adjacent
    // pair exists to transpose). Keeping 1 out cost nothing while
    // one-unit operands were rare, but the affix trim above produces
    // them constantly from near-identical pairs, and the scalar
    // fallback's three row allocations then dominate the whole call.
    if (1..=64).contains(&shorter.len()) {
        return osa_bit_vector(shorter, longer);
    }
    osa_bit_vector_blocks(shorter, longer)
}

/// A weighted cell cost that is known to be a non-negative integer, as a
/// `usize`.
///
/// Two unit-cost paths have no bit-parallel form and therefore borrow the
/// weighted tier's matrix: unrestricted-Damerau search and OSA search. Under
/// [`Costs::UNIT`] every cell of that matrix is a sum of `0.0` and `1.0`
/// terms bounded by `n + m`, so it is an integer exactly representable in
/// `f64` and the conversion is lossless rather than a rounding.
#[inline]
fn exact_usize(value: f64) -> usize {
    debug_assert!(value >= 0.0 && value.fract() == 0.0 && value <= (1u64 << 53) as f64);
    value as usize
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
/// rather than O(n·m) scalar cell updates. It applies to the unit-cost tier
/// only — [`levenshtein_weighted`] is a different function with a rolling-row
/// kernel — and needs no runtime cost check to say so. The shorter operand is
/// always the bit-packed pattern.
///
/// The lower bound was originally 8 because the then-`HashMap` Peq's setup
/// cost made `n = 4` a wash against the scalar cells it replaced. The
/// [`BitPeq`] flat table removed that setup cost — the sibling OSA fast
/// path, gated at 2 from the start with the identical table shape,
/// measures ~16 ns at n = 4 against the scalar path's ~40 ns — so the
/// gate now starts at 1 (a 1-unit pattern has no Myers subtlety and the
/// kernel handles it; 0-length operands short-circuit before dispatch).
///
/// Before any of that, unit-cost operands are stripped of their common
/// affixes ([`trim_common_affixes`], gated by [`TRIM_MIN_LEN`]) — the
/// reduction that decides the near-identical-pair shape outright: a pair
/// differing in a single interior position stops being an `O(n·m/64)`
/// bit-vector sweep and becomes a `memcmp` plus a one-unit kernel call.
fn plain_levenshtein_unit<T: BitPeq>(source: &[T], target: &[T]) -> usize {
    let (source, target) = if source.len().min(target.len()) > TRIM_MIN_LEN {
        trim_common_affixes(source, target)
    } else {
        (source, target)
    };
    if source.is_empty() {
        return target.len();
    }
    if target.is_empty() {
        return source.len();
    }
    let (shorter, longer) = if source.len() <= target.len() {
        (source, target)
    } else {
        (target, source)
    };
    // Unit costs are symmetric (insertion == deletion), so treating
    // whichever operand is shorter as Myers' "pattern" changes nothing
    // about the result -- only which one gets the compact bitmask
    // representation.
    if (1..=4).contains(&shorter.len()) {
        return bit_vector_distance_tiny(shorter, longer);
    }
    if (5..=64).contains(&shorter.len()) {
        return bit_vector_distance(shorter, longer);
    }
    bit_vector_distance_blocks(shorter, longer)
}

/// Shortest operand length at which [`trim_common_affixes`] is worth
/// running, for both the plain and the OSA unit-cost fast paths.
///
/// The trim is *cheap* but not free: two chunk-wide `memcmp`s for
/// unrelated operands, which is what the whole call costs at
/// the smallest benchmarked size. Every unit-cost kernel from 5 units up
/// builds a `Peq` table first (a flat `[u64; 256]`, 2 KB to zero), so the
/// trim is already amortized there many times over; below that,
/// [`bit_vector_distance_tiny`] derives its match bits directly from the
/// pattern with no table at all and the whole call is a handful of
/// nanoseconds, so the scan is left off rather than charged to inputs that
/// cannot benefit much from it. `4` is therefore the largest length still
/// on the table-free path, and the gate is `> 4`.
pub(crate) const TRIM_MIN_LEN: usize = 4;

/// Removes the maximal equal prefix and the maximal equal suffix before any
/// of the three unit-cost distance kernels. Those aligned runs cannot
/// participate in a cheaper edit script, so this preserves the exact
/// distance while shrinking the bit-vector pattern and the scan.
///
/// # Where this is and is not valid
///
/// * **Plain Levenshtein** — the textbook reduction: an optimal edit script
///   can always be rearranged to leave a common prefix/suffix untouched, so
///   `d(P·a·S, P·b·S) == d(a, b)`.
/// * **OSA / restricted Damerau, unit costs** — also valid, but *not* by
///   inheritance from the plain case, and this is the one that needed
///   checking rather than assuming: OSA's extra candidate reaches back two
///   rows and two columns, so a cut can in principle sever an alignment the
///   plain recurrence never had. Write the pair as `P·a'` and `P·b'` with
///   `|P| = p`. Inside the prefix square the matrix is `D[i][j] = |i − j|`
///   (both axes carry the *same* string there), and for the cells at
///   `(p+i', p+j')` with `i', j' >= 2` the transposition guard and the cell
///   it reads translate one-for-one into the trimmed matrix. The only cells
///   that differ are the ones flush against the cut — `(p+1, p+j')` and
///   `(p+i', p+1)` — where the untrimmed matrix can evaluate a
///   transposition candidate that the trimmed one has no row `−1` / column
///   `−1` to reach. Those candidates are worth exactly `j'` and `i'`,
///   because the cell they read is inside the prefix square and is
///   therefore a pure length difference; meanwhile the guard that enables
///   them forces a character match that already puts the trimmed cell at
///   `j' − 1` / `i' − 1` (or at `0`, when the guard collapses to
///   `a'[1] == b'[1]`). A candidate that is always strictly greater than
///   the value it competes with cannot change a minimum, so no cell moves
///   and nothing propagates. Suffix trimming then follows from OSA's
///   reversal symmetry. Note the argument never needs the affix to be
///   *maximal* — any common prefix is a sound cut — the implementation
///   simply takes the longest one because that is the most work saved.
///   Pinned exhaustively by
///   `osa_affix_trim_matches_the_untrimmed_oracle_exhaustively`.
/// * **Unrestricted Damerau–Levenshtein, unit costs** — valid, and for the
///   strongest reason of the three: unrestricted Damerau–Levenshtein is a
///   true metric, so `d(P·a·S, P·b·S) ≤ d(a, b)` by applying `a`'s optimal
///   script inside the surrounding context, and `d(a, b) ≤ d(P·a·S, P·b·S)`
///   because every operation of an optimal script for the padded pair either
///   lies wholly inside the middle (keep it) or touches the affix (drop it,
///   which cannot make the residual middles disagree — the affixes are
///   equal). Pinned exhaustively by
///   `damerau_affix_trim_matches_the_untrimmed_oracle_exhaustively`.
/// * **Weighted costs** — not attempted for any of the three; the
///   prefix-region argument above assumes a unit-cost matrix. Nothing gates
///   on a cost comparison any more: the weighted entry points reach the
///   scalar recurrences directly and never call this function, so the
///   reduction is applied exactly where it is sound by construction.
fn trim_common_affixes<'a, T: Unit>(
    mut source: &'a [T],
    mut target: &'a [T],
) -> (&'a [T], &'a [T]) {
    let prefix = common_prefix_len(source, target);
    source = &source[prefix..];
    target = &target[prefix..];

    let suffix = common_suffix_len(source, target);
    if suffix != 0 {
        source = &source[..source.len() - suffix];
        target = &target[..target.len() - suffix];
    }
    (source, target)
}

/// Single-word Myers kernel for patterns of at most four units. Building the
/// normal 256-entry byte Peq table costs more than deriving these few match
/// bits directly from the pattern on every scanned unit.
fn bit_vector_distance_tiny<T: BitPeq>(shorter: &[T], longer: &[T]) -> usize {
    let m = shorter.len();
    debug_assert!((1..=4).contains(&m));

    if m == 1 {
        return longer.len() - usize::from(longer.contains(&shorter[0]));
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

    score as usize
}

/// Myers' (1999) bit-vector algorithm for unit-cost edit distance: the
/// "pattern" `shorter` must be non-empty and fit in one 64-bit word (`len()
/// <= 64`). [`plain_levenshtein_unit`] selects the multi-word sibling
/// [`bit_vector_distance_blocks`] past that bound. Computes exactly what
/// [`plain_rows`] computes for
/// unit costs, verified exhaustively
/// against it in this module's own tests before being trusted for anything.
fn bit_vector_distance<T: BitPeq>(shorter: &[T], longer: &[T]) -> usize {
    debug_assert!(!shorter.is_empty() && shorter.len() <= 64);

    // Peq[c]: bit i set iff `shorter[i] == c`, via [`BitPeq`]'s per-type
    // tables. This was originally a `std::collections::HashMap`, and a
    // controlled decomposition experiment (see `docs/PERFORMANCE_GAPS.md`
    // entry 26's second update) measured that map -- one SipHash probe per
    // scanned character -- at roughly three quarters of the whole kernel's
    // runtime; the flat byte table turns each probe into one indexed load.
    bit_vector_distance_with(&T::peq1(shorter), shorter.len(), longer)
}

/// [`bit_vector_distance`]'s kernel proper, over a `Peq` table the caller
/// already holds.
///
/// Split out so the table's construction and its *use* can have different
/// lifetimes: the per-call entry point above builds one and drops it, while
/// [`crate::prepared::PreparedPattern`] builds one at construction and drives
/// this from every query. Both reach the identical loop, so the 1-vs-N shape
/// cannot drift from the 1-vs-1 one — the reason it is a shared function
/// rather than a copied loop.
///
/// `m` is the pattern length in units, which the table alone cannot report
/// (a pattern whose last unit recurs earlier leaves the high bits of every
/// row unset). Requires `1 <= m <= 64`; `longer` may be any length,
/// including shorter than the pattern or empty — Myers' recurrence carries
/// no assumption that the scanned operand is the longer one, only that the
/// bit-packed one fits a word.
pub(crate) fn bit_vector_distance_with<T: BitPeq>(
    peq: &T::Table1,
    m: usize,
    longer: &[T],
) -> usize {
    debug_assert!(m > 0 && m <= 64);

    let last_bit = 1u64 << (m - 1);
    let mut pv: u64 = if m == 64 { u64::MAX } else { (1u64 << m) - 1 };
    let mut mv: u64 = 0;
    let mut score = m as i64;

    for &c in longer {
        let eq = T::peq1_get(peq, c);
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

    score as usize
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
/// [`bit_vector_distance`] already cites: `shorter`
/// is split into `ceil(m / 64)` blocks, each carrying its own `Pv`/`Mv`
/// state, and a single left-to-right sweep over each row of `longer`
/// threads a horizontal-delta carry bit from each block into the next --
/// the generalisation of the single-word algorithm's constant `| 1` /
/// `<< 1` carry-in, which only works there because a lone word has no
/// "next block" to receive an overflow from. Deliberately does **not**
/// layer Ukkonen-band block skipping on top of this same core -- that
/// optimisation exists to skip blocks once a distance *threshold* rules
/// them out, and [`levenshtein`] always wants the exact distance, never a
/// bounded or thresholded one, so there is no cutoff to band around.
///
/// Verified exhaustively against [`plain_rows`] (this module's own tests)
/// across randomized pairs spanning many block-boundary lengths (63 through
/// several thousand units), and cross-checked directly against
/// [`bit_vector_distance`] itself at every length where the two functions'
/// domains overlap (`8..=64`) -- block-carry propagation is exactly the
/// part a naive per-block reapplication of the single-word formula would
/// get wrong, so agreement with the already-proven single-word path at the
/// boundary is treated as load-bearing evidence, not a formality.
fn bit_vector_distance_blocks<T: BitPeq>(shorter: &[T], longer: &[T]) -> usize {
    let m = shorter.len();
    debug_assert!(m > 0);
    // Peq row per unit via [`BitPeq`]: bit i of word b set iff
    // shorter[b * 64 + i] == c. The trailing, never-set bits in the last
    // (possibly partial) block never match any character and are never read
    // past `last_bit`, so leaving them unset is correct, not just
    // convenient -- see this function's own doc comment above
    // for why they cannot leak into the meaningful output bit (addition-carry
    // propagation only flows from low bits to high ones, and `last_bit` sits
    // below every such trailing bit). This was one
    // `std::collections::HashMap` per block -- one SipHash probe per block
    // per scanned character, measured (see `docs/PERFORMANCE_GAPS.md` entry
    // 26's second update) at roughly three quarters of the kernel's whole
    // runtime; the packed table costs one lookup per scanned character, then
    // pure indexed loads per block.
    bit_vector_distance_blocks_with(&T::peqn(shorter, m.div_ceil(64)), m, longer)
}

/// [`bit_vector_distance_blocks`]'s kernel proper, over packed `Peq` rows the
/// caller already holds — the multi-block counterpart of
/// [`bit_vector_distance_with`], and shared with
/// [`crate::prepared::PreparedPattern`] for the same reason.
///
/// The table must have been built with `blocks = m.div_ceil(64)` rows per
/// unit; `m` is the pattern length in units, which the rows alone cannot
/// report.
pub(crate) fn bit_vector_distance_blocks_with<T: BitPeq>(
    peq: &T::TableN,
    m: usize,
    longer: &[T],
) -> usize {
    // Removing the per-column score dependency and reconstructing it from
    // final Pv/Mv popcounts wins through four blocks in A/B benchmarks; past
    // that crossover the extra popcounts lose to the simple last-bit update.
    if m <= 256 {
        bit_vector_distance_blocks_impl::<T, true>(peq, m, longer)
    } else {
        bit_vector_distance_blocks_impl::<T, false>(peq, m, longer)
    }
}

fn bit_vector_distance_blocks_impl<T: BitPeq, const FINAL_POPCOUNT: bool>(
    peq: &T::TableN,
    m: usize,
    longer: &[T],
) -> usize {
    const WORD: usize = 64;
    debug_assert!(m > 0);

    let blocks = m.div_ceil(WORD);
    let last_block = blocks - 1;
    let last_bit = 1u64 << ((m - 1) % WORD);

    // Skip a leading run whose alphabet is absent from the pattern. Processing
    // `k` such units through Myers would only clear the lowest `k` meaningful
    // Pv bits; initializing that state directly avoids O(k·blocks) work. If
    // the whole target is disjoint this reduces the answer to max(m, n). Unlike
    // a separate all-target preflight, the absent prefix is never scanned twice
    // when the first overlap happens late.
    let leading_absent = longer
        .iter()
        .position(|&unit| T::peqn_row(peq, unit).is_some())
        .unwrap_or(longer.len());
    if leading_absent == longer.len() {
        return m.max(longer.len());
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
        let row = T::peqn_row(peq, c).unwrap_or(zeros);
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
        distance as usize
    } else {
        score as usize
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
/// `D0`. Verified against [`osa_rows`] — the scalar oracle transcribed
/// from the OSA recurrence — in this module's own tests before being
/// trusted for anything.
fn osa_bit_vector<T: BitPeq>(shorter: &[T], longer: &[T]) -> usize {
    debug_assert!((1..=64).contains(&shorter.len()));
    osa_bit_vector_with(&T::peq1(shorter), shorter.len(), longer)
}

/// [`osa_bit_vector`]'s kernel proper, over a `Peq` table the caller already
/// holds — the OSA counterpart of [`bit_vector_distance_with`].
///
/// The table is bit-for-bit the one plain Levenshtein uses: Hyyrö's
/// transposition extension adds registers, never table entries, so one
/// prepared `Peq` serves both metrics and
/// [`crate::prepared::PreparedPattern`] builds only one. Unrestricted
/// Damerau–Levenshtein is the odd one out and has no share in it — see that
/// type's own doc comment.
pub(crate) fn osa_bit_vector_with<T: BitPeq>(table: &T::Table1, m: usize, longer: &[T]) -> usize {
    debug_assert!((1..=64).contains(&m));

    let last_bit = 1u64 << (m - 1);
    let mut pv: u64 = if m == 64 { u64::MAX } else { (1u64 << m) - 1 };
    let mut mv: u64 = 0;
    let mut prev_d0: u64 = 0;
    let mut prev_pm: u64 = 0;
    let mut score = m as i64;

    for &c in longer {
        let pm_j = T::peq1_get(table, c);
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

    score as usize
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
/// straddling a word boundary is seen.
///
/// The per-column state is one in-place vector plus two carried scalars
/// rather than a pair of buffers swapped each column: a transposition
/// candidate reaches back exactly one column, so a full second copy of the
/// state would hold nothing the two scalars do not already carry. Resetting
/// those scalars to zero at the top of each column is what stands in for the
/// row above the first — the row a two-buffer scheme would keep permanently
/// all-zero.
fn osa_bit_vector_blocks<T: BitPeq>(shorter: &[T], longer: &[T]) -> usize {
    let m = shorter.len();
    // The dispatch gate routes only m > 64 here, but the formula is valid
    // for any m >= 1 (with one block the cross-word carry terms are always
    // zero) — kept callable on the shared domain so the tests can pit this
    // implementation directly against `osa_bit_vector`'s independent shape.
    debug_assert!(m >= 1);
    osa_bit_vector_blocks_with(&T::peqn(shorter, m.div_ceil(64)), m, longer)
}

/// [`osa_bit_vector_blocks`]'s kernel proper, over packed `Peq` rows the
/// caller already holds — the multi-block counterpart of
/// [`osa_bit_vector_with`].
pub(crate) fn osa_bit_vector_blocks_with<T: BitPeq>(
    table: &T::TableN,
    m: usize,
    longer: &[T],
) -> usize {
    const WORD: usize = 64;
    debug_assert!(m >= 1);
    let blocks = m.div_ceil(WORD);
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
        let row = T::peqn_row(table, c).unwrap_or(&zeros);
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

    score as usize
}

// ---------------------------------------------------------------------------
// Unrestricted Damerau, unit-cost fast path
// ---------------------------------------------------------------------------

/// Largest byte-operand length still served by [`damerau_unit_small`]'s
/// table-free stack matrix.
///
/// Below this size the 256-entry last-occurrence table that
/// [`damerau_zhao_sahni`] zeroes costs more than the entire dynamic program
/// it accelerates, and the whole `(n+1) × (m+1)` matrix fits in a single
/// stack array. The affix trim in [`damerau_unrestricted`] makes this the
/// *common* size for near-identical pairs, not a rare one.
const DAMERAU_STACK_MAX: usize = 8;

/// Per-unit-type dispatch and scratch for the unit-cost canonical Damerau
/// kernels.
///
/// Specialised the way [`Unit::Map`] already is: bytes get a flat 256-entry
/// last-occurrence table (a probe is one indexed load) and the table-free
/// stack matrix for tiny operands; scalars get an `FxHashMap` and always
/// take the linear-space kernel (with a hashed table, the stack matrix's
/// `rposition` scan would not be the cheaper shape anyway).
///
/// The table is deliberately `i32` rather than the `usize` [`Unit::Map`]
/// stores: it is memset once per call, so halving it to 1 KB halves a fixed
/// cost that short operands — the common shape once [`trim_common_affixes`]
/// has run — feel directly.
trait DamerauUnit: Unit {
    /// Last source row (1-based) in which each symbol occurred, `-1` if none.
    type LastRow;
    fn new_last_row() -> Self::LastRow;
    fn last_row_get(table: &Self::LastRow, unit: Self) -> i64;
    fn last_row_set(table: &mut Self::LastRow, unit: Self, row: i64);

    /// Canonical unrestricted Damerau–Levenshtein distance between two
    /// non-empty operands, `longer.len() >= shorter.len()`.
    fn damerau_unit(longer: &[Self], shorter: &[Self]) -> usize;
}

impl DamerauUnit for u8 {
    type LastRow = [i32; 256];

    fn new_last_row() -> Self::LastRow {
        [-1; 256]
    }

    #[inline]
    fn last_row_get(table: &Self::LastRow, unit: Self) -> i64 {
        i64::from(table[unit as usize])
    }

    #[inline]
    fn last_row_set(table: &mut Self::LastRow, unit: Self, row: i64) {
        table[unit as usize] = row as i32;
    }

    fn damerau_unit(longer: &[Self], shorter: &[Self]) -> usize {
        if longer.len() <= DAMERAU_STACK_MAX {
            return damerau_unit_small(longer, shorter);
        }
        damerau_zhao_sahni(longer, shorter)
    }
}

impl DamerauUnit for char {
    type LastRow = FxHashMap<char, i32>;

    fn new_last_row() -> Self::LastRow {
        FxHashMap::default()
    }

    #[inline]
    fn last_row_get(table: &Self::LastRow, unit: Self) -> i64 {
        table.get(&unit).map_or(-1, |&row| i64::from(row))
    }

    #[inline]
    fn last_row_set(table: &mut Self::LastRow, unit: Self, row: i64) {
        table.insert(unit, row as i32);
    }

    fn damerau_unit(longer: &[Self], shorter: &[Self]) -> usize {
        damerau_zhao_sahni(longer, shorter)
    }
}

/// The canonical Lowrance–Wagner recurrence on a fixed stack matrix, for
/// byte operands of at most [`DAMERAU_STACK_MAX`] units each: no tables, no
/// heap — the last-occurrence lookup is a plain `rposition` scan over the
/// few source bytes of *strictly earlier* rows, and the full (tiny) matrix
/// makes the transposition candidate a direct index.
///
/// This evaluates the recurrence in its unabridged form — every earlier
/// occurrence `k` of the target symbol is a candidate, with both gap terms
/// live — rather than [`damerau_zhao_sahni`]'s two-case specialisation, so
/// the two kernels are independent shapes of the same distance and are
/// pitted directly against each other in this module's tests.
fn damerau_unit_small(source: &[u8], target: &[u8]) -> usize {
    const CAP: usize = DAMERAU_STACK_MAX + 1;
    let n = source.len();
    let m = target.len();
    debug_assert!(n <= DAMERAU_STACK_MAX && m <= DAMERAU_STACK_MAX);
    if n == 0 {
        return m;
    }
    if m == 0 {
        return n;
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
                // `source[..r - 1]`, not `source[..r]`: the last-occurrence
                // map records a symbol's row only once that row is finished,
                // so row `r` cannot see its own symbol. Looking one row too
                // far ahead is exactly what makes a recurrence asymmetric —
                // it lets a cell claim a transposition with a *negative* row
                // gap, which no edit script can realise.
                if let Some(p) = source[..r - 1].iter().rposition(|&x| x == t) {
                    let lrm = p + 1;
                    let before = mat[(lrm - 1) * w + (lcm - 1)];
                    // Both gaps are non-negative now: `lrm <= r - 1` and
                    // `lcm <= c - 1`.
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
    usize::from(mat[n * w + m])
}

/// Canonical unrestricted Damerau–Levenshtein distance in `O(m)` space,
/// following Zhao and Sahni's *Linear space string correction algorithm
/// using the Damerau-Levenshtein distance* (2020), verified here against a
/// from-scratch transcription of Lowrance & Wagner's published
/// `(n+2) × (m+2)` recurrence.
///
/// # Why linear space is possible at all
///
/// The textbook Lowrance–Wagner recurrence needs a full matrix because its
/// transposition candidate reads `H[k−1][l−1]` for an arbitrary earlier row
/// `k` (the last row carrying the target symbol) and column `l` (the last
/// column carrying the source symbol). Zhao–Sahni's observation is that in
/// an optimal solution only two of those candidates can ever win: the one
/// with no column gap (`j − l == 1`) and the one with no row gap
/// (`i − k == 1`). Each needs a single remembered cell rather than a whole
/// matrix:
///
/// * `fr[j + 1]` holds `H[k−1][j−2]`, written on the last row whose symbol
///   matched `target[j − 1]` — exactly row `k`, since `last_row` is updated
///   at the end of every row;
/// * `t` holds `H[i−2][l−1]`, captured at the current row's most recent
///   match column.
///
/// So the working set is three rolling rows (`r`, `r1`, `fr`) plus two
/// scalars and one last-occurrence table, instead of `O(nm)` cells. All three
/// rows live in one contiguous buffer — a single stack array while the
/// operands are small enough, one heap allocation otherwise, never three.
///
/// # Sentinels
///
/// `max_val = max(n, m) + 1` stands in for the `−1` row and column of the
/// published formulation: it is strictly greater than any achievable
/// distance (which is bounded by `max(n, m)`), so a candidate that reads a
/// sentinel can never win a minimum. Index 0 of every row is that sentinel
/// column and is written exactly once, at construction.
///
/// Requires both operands non-empty; [`damerau_unrestricted`] short-circuits
/// the empty cases before dispatch.
fn damerau_zhao_sahni<T: DamerauUnit>(source: &[T], target: &[T]) -> usize {
    let n = source.len();
    let m = target.len();
    debug_assert!(n > 0 && m > 0);
    // Row indices are stored as `i32` in the last-occurrence table; an input
    // long enough to overflow that would need an `O(nm)` sweep over >2^62
    // cells, which no caller can actually run.
    debug_assert!(n <= i32::MAX as usize);

    let max_val = (n.max(m) + 1) as i64;
    let size = m + 2;

    // One buffer, three rows laid end to end:
    //   `fr` — `fr[j + 1]` = H[k−1][j−2] for the last row k whose symbol
    //          matched target[j − 1]; unwritten entries stay at the sentinel;
    //   `r1` — row i−1;
    //   `r`  — row i under construction, and (before the column loop
    //          overwrites it) still row i−2, which `last_i2l1` harvests.
    //
    // Three rows of up to 34 cells sit on the stack: enough for word-length
    // operands — and for whatever the affix trim leaves of a near-identical
    // pair — to run without touching the allocator, while staying smaller
    // than the byte kernel's own last-occurrence table. Past that, one `Vec`,
    // by which point the `O(nm)` sweep dwarfs a single allocation anyway.
    const STACK_CELLS: usize = 3 * 34;
    let mut stack = [max_val; STACK_CELLS];
    let mut heap;
    let buf: &mut [i64] = if 3 * size <= STACK_CELLS {
        &mut stack[..3 * size]
    } else {
        heap = vec![max_val; 3 * size];
        &mut heap
    };
    let (fr, rest) = buf.split_at_mut(size);
    let (mut r1, mut r) = rest.split_at_mut(size);
    // Row 0: H[0][j] = j, at index j + 1; index 0 stays at the sentinel.
    for (j, cell) in r.iter_mut().enumerate().skip(1) {
        *cell = (j - 1) as i64;
    }

    let mut last_row = T::new_last_row();

    for (i0, &s) in source.iter().enumerate() {
        let i = (i0 + 1) as i64;
        std::mem::swap(&mut r, &mut r1);
        // Last column of this row where source[i − 1] matched, and the
        // saved `H[i−2][l−1]` that goes with it.
        let mut last_col: i64 = -1;
        let mut t = max_val;
        // `r[j + 1]` before it is overwritten is H[i−2][j], so this carries
        // H[i−2][j−1] into iteration j.
        let mut last_i2l1 = r[1];
        r[1] = i;

        for (j0, &c) in target.iter().enumerate() {
            let j = j0 + 1;
            let substitute = r1[j] + i64::from(s != c);
            let insert = r[j] + 1;
            let delete = r1[j + 1] + 1;
            let mut best = substitute.min(insert).min(delete);

            if s == c {
                last_col = j as i64;
                fr[j + 1] = r1[j - 1];
                t = last_i2l1;
            } else {
                let k = T::last_row_get(&last_row, c);
                let l = last_col;
                if j as i64 - l == 1 {
                    // No column gap: H[k−1][j−2] + (i − k − 1) + 1.
                    best = best.min(fr[j + 1] + (i - k));
                } else if i - k == 1 {
                    // No row gap: H[i−2][l−1] + (j − l − 1) + 1.
                    best = best.min(t + (j as i64 - l));
                }
            }

            last_i2l1 = r[j + 1];
            r[j + 1] = best;
        }

        // End of row, not mid-row: a symbol becomes visible to the
        // transposition candidate only from the *next* row onward. This one
        // placement is what makes the recurrence canonical and symmetric.
        T::last_row_set(&mut last_row, s, i);
    }

    r[m + 1] as usize
}

/// One-row Levenshtein. No parent tracking, so no tie-breaking concerns.
fn plain_rows<T: Unit>(source: &[T], target: &[T], costs: &Costs) -> f64 {
    let m = target.len();

    let mut row: Vec<f64> = Vec::with_capacity(m + 1);
    row.push(0.0);
    for c in 1..=m {
        row.push(row[c - 1] + costs.insertion);
    }

    for &s in source {
        let mut diag = row[0];
        row[0] += costs.deletion;
        let mut left = row[0];
        for c in 1..=m {
            let up = row[c];
            let insert = left + costs.insertion;
            let delete = up + costs.deletion;
            let mut sub = diag;
            if s != target[c - 1] {
                sub += costs.substitution;
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
fn plain_rows_two_oracle<T: Unit>(source: &[T], target: &[T], costs: &Costs) -> f64 {
    let n = source.len();
    let m = target.len();
    let mut prev: Vec<f64> = Vec::with_capacity(m + 1);
    prev.push(0.0);
    for c in 1..=m {
        prev.push(prev[c - 1] + costs.insertion);
    }
    let mut cur = vec![0.0f64; m + 1];
    for r in 1..=n {
        cur[0] = prev[0] + costs.deletion;
        let s = source[r - 1];
        for c in 1..=m {
            let insert = cur[c - 1] + costs.insertion;
            let delete = prev[c] + costs.deletion;
            let mut sub = prev[c - 1];
            if s != target[c - 1] {
                sub += costs.substitution;
            }
            cur[c] = min3(insert, delete, sub);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[m]
}

/// Three-row optimal string alignment (restricted Damerau). The recurrence
/// [`osa_weighted_impl`] evaluates, and the scalar oracle the unit tier's
/// bit-parallel kernels are differentially tested against.
fn osa_rows<T: Unit>(source: &[T], target: &[T], costs: &Costs) -> f64 {
    let n = source.len();
    let m = target.len();

    // rows[0] = r-2, rows[1] = r-1, rows[2] = r
    let mut prev2: Vec<f64> = vec![0.0; m + 1];
    let mut prev: Vec<f64> = Vec::with_capacity(m + 1);
    prev.push(0.0);
    for c in 1..=m {
        prev.push(prev[c - 1] + costs.insertion);
    }
    let mut cur = vec![0.0f64; m + 1];

    for r in 1..=n {
        cur[0] = prev[0] + costs.deletion;
        let s = source[r - 1];
        for c in 1..=m {
            let t = target[c - 1];
            let insert = cur[c - 1] + costs.insertion;
            let delete = prev[c] + costs.deletion;
            let mut sub = prev[c - 1];
            if s != t {
                sub += costs.substitution;
            }
            let mut best = min3(insert, delete, sub);

            if r > 1 && c > 1 && s == target[c - 2] && source[r - 2] == t {
                let transpose = prev2[c - 2] + costs.transposition;
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
// Full matrix (search mode, and weighted-cost Damerau)
// ---------------------------------------------------------------------------

/// Which of the module's three distances a shared routine is evaluating.
///
/// This replaces what used to be a `restricted: bool` field on the old
/// public `Options`: the algorithm is chosen by the public function the
/// caller reached for, so it travels as an internal parameter and never as
/// user-visible state. The cost *tier* is now chosen the same way, which is
/// why nothing here inspects a cost value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Variant {
    /// Levenshtein: no transposition candidate at all.
    Plain,
    /// Unrestricted Damerau–Levenshtein (Lowrance–Wagner).
    Damerau,
    /// Optimal string alignment (restricted Damerau).
    Osa,
}

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

/// The `(n+1) × (m+1)` cost-plus-parent matrix, evaluating whichever
/// `variant`'s recurrence is asked for.
///
/// Used for search mode (where the parent chain is the answer) and as the
/// weighted-cost fallback for every distance the unit-cost kernels decline.
///
/// The unrestricted-Damerau branch is the Lowrance–Wagner recurrence exactly
/// as published: `last_row_map` records a source symbol's row only once that
/// row is *finished*, so a cell's transposition candidate can only reach
/// strictly earlier rows and both gap terms are non-negative. Updating the
/// map mid-row instead would let a cell claim a transposition with a
/// negative row gap — an alignment no edit script realises, and the reason
/// such a variant is not symmetric.
///
/// That branch carries Lowrance–Wagner's cost precondition with it:
/// `2 · transposition ≥ insertion + deletion`, discharged by
/// [`DamerauCosts::new`] before any value of that type exists, so this
/// function may assume it and there is no runtime check here to take. The
/// other two variants have no such condition.
fn full_matrix<T: Unit>(
    source: &[T],
    target: &[T],
    costs: &Costs,
    variant: Variant,
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
        mat.cost[i] = mat.cost[mat.idx(r - 1, 0)] + costs.deletion;
        mat.parent[i] = ((r - 1) as u32, 0);
    }
    // Row 0: insertions across — except in search mode, where every prefix of the
    // target is a free starting point.
    for c in 1..=m {
        let i = mat.idx(0, c);
        if search {
            mat.cost[i] = 0.0;
        } else {
            mat.cost[i] = mat.cost[mat.idx(0, c - 1)] + costs.insertion;
            mat.parent[i] = (0, (c - 1) as u32);
        }
    }

    let unrestricted = variant == Variant::Damerau;
    let restricted = variant == Variant::Osa;

    let mut last_row_map = T::new_map();
    let mut last_col_match: Option<usize> = None;

    for r in 1..=n {
        if unrestricted {
            last_col_match = None;
        }
        let s = source[r - 1];
        for c in 1..=m {
            let t = target[c - 1];

            let insert = mat.cost_at(r, c - 1) + costs.insertion;
            let delete = mat.cost_at(r - 1, c) + costs.deletion;
            let mut sub = mat.cost_at(r - 1, c - 1);
            if s != t {
                sub += costs.substitution;
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

            if unrestricted {
                if let (Some(lcm), Some(lrm)) = (last_col_match, last_row_map.get(t)) {
                    // `lrm` is a strictly earlier row and `lcm` a strictly
                    // earlier column, so both gaps are non-negative and both
                    // reads are in bounds. The candidate spends `row_gap`
                    // deletions and `col_gap` insertions to clear the span
                    // between the transposed pair, then one transposition.
                    let before = mat.cost_at(lrm - 1, lcm - 1);
                    let row_gap = (r - lrm - 1) as f64;
                    let col_gap = (c - lcm - 1) as f64;
                    let transpose = before
                        + row_gap * costs.deletion
                        + col_gap * costs.insertion
                        + costs.transposition;
                    if transpose < best_cost {
                        best_cost = transpose;
                        best_parent = ((lrm - 1) as u32, (lcm - 1) as u32);
                    }
                }
            }

            if restricted && r > 1 && c > 1 && s == target[c - 2] && source[r - 2] == t {
                let transpose = mat.cost_at(r - 2, c - 2) + costs.transposition;
                if transpose < best_cost {
                    best_cost = transpose;
                    best_parent = ((r - 2) as u32, (c - 2) as u32);
                }
            }

            let i = mat.idx(r, c);
            mat.cost[i] = best_cost;
            mat.parent[i] = best_parent;

            if unrestricted && s == t {
                last_col_match = Some(c);
            }
        }
        // End of row: `s` becomes visible to the transposition candidate only
        // from row `r + 1` onward. See this function's own doc comment.
        if unrestricted {
            last_row_map.set(s, r);
        }
    }

    mat
}

// ---------------------------------------------------------------------------
// Search mode
// ---------------------------------------------------------------------------

/// The unit-cost searches' body.
fn search_unit_impl<'t>(
    source: &str,
    target: &'t str,
    variant: Variant,
) -> SearchResult<'t, usize> {
    let (start, end, distance) = dispatch(source, target, |ops| match ops {
        Operands::Bytes(s, t) => search_unit(s, t, variant),
        Operands::Units(s, t) => search_unit(s, t, variant),
    });
    borrow_span(target, start, end, distance)
}

/// The weighted searches' body.
fn search_weighted_impl<'t>(
    source: &str,
    target: &'t str,
    costs: &Costs,
    variant: Variant,
) -> SearchResult<'t, f64> {
    let (start, end, distance) = dispatch(source, target, |ops| match ops {
        Operands::Bytes(s, t) => search_full_matrix(s, t, costs, variant),
        Operands::Units(s, t) => search_full_matrix(s, t, costs, variant),
    });
    borrow_span(target, start, end, distance)
}

/// Turns the kernels' `[start, end)` **scalar** span into a borrowed slice of
/// `target` plus its **byte** start.
///
/// This is the one place the two quantities meet, and the reason
/// `SearchResult`'s first guarantee (`docs/design/distance-contract.md` §3.2)
/// needs no assertion: the substring *is* borrowed from the target at the
/// reported range, so it cannot be text the target does not contain. Building
/// an owned `String` from the span instead would make that a claim to check
/// rather than a fact — every scalar boundary is a byte boundary, so borrowing
/// is total here, and nothing is re-encoded on the way out.
///
/// Two arms, keyed on the **target** alone rather than on the arm
/// [`dispatch`] took — which is the wider of the two fast paths, since an
/// ASCII target promoted to `&[char]` by a non-ASCII *source* still gets the
/// identity conversion here:
///
/// * **ASCII target** — a byte index *is* a scalar index, so the conversion is
///   the identity and there is no walk.
/// * **Non-ASCII target** — one `char_indices()` pass recovers both ends.
///   Allocation-free and `O(m)` alongside the kernels' `Θ(n·m)`, which is the
///   same order as the `is_ascii` scan itself.
fn borrow_span<D>(target: &str, start: usize, end: usize, distance: D) -> SearchResult<'_, D> {
    let len = target.len();
    let (byte_start, byte_end) = if target.is_ascii() {
        let s = start.min(len);
        (s, end.min(len).max(s))
    } else {
        // Both bounds come from a backtrack that cannot leave the matrix, so
        // a scalar index past the end is unreachable; falling back to the end
        // of the string keeps the slicing total rather than relying on that.
        let (mut s, mut e) = (len, len);
        for (scalar, (byte, _)) in target.char_indices().enumerate() {
            if scalar == start {
                s = byte;
            }
            if scalar == end {
                e = byte;
            }
        }
        (s, e.max(s))
    };
    let substring = &target[byte_start..byte_end];
    // The new index arithmetic, guarded at debug-only cost. Guarantee (2) —
    // that the metric applied to `substring` reproduces `distance` — is
    // deliberately *not* asserted here: it would cost a second `Θ(n·m)`
    // evaluation on every call. It is pinned by test instead.
    debug_assert_eq!(
        substring.chars().count(),
        end.saturating_sub(start),
        "derived byte span must hold exactly the scalars the kernel matched"
    );
    SearchResult {
        substring,
        start: byte_start,
        distance,
    }
}

/// Returns `(match_start, match_end, distance)` in units of the operand slice,
/// for the unit-cost tier.
///
/// Plain Levenshtein takes the bit-parallel [`search_bits`] path. Neither
/// Damerau variant may, even at unit costs: a transposition parent depends on
/// `last_row_map`/row-gap state at the moment the cell was filled, which cell
/// costs alone cannot recover — a structural blocker, not an unimplemented
/// case. They borrow the weighted tier's matrix under [`Costs::UNIT`] instead,
/// which produces integer cell costs (see [`exact_usize`]). Empty operands are
/// excluded from the bit-parallel path so its kernels can assume a non-empty
/// pattern; [`search_full_matrix`] handles them at no measurable cost.
fn search_unit<T: BitPeq>(source: &[T], target: &[T], variant: Variant) -> (usize, usize, usize) {
    if variant == Variant::Plain && !source.is_empty() && !target.is_empty() {
        return search_bits(source, target);
    }
    let (start, end, distance) = search_full_matrix(source, target, &Costs::UNIT, variant);
    (start, end, exact_usize(distance))
}

/// The full-matrix search: [`full_matrix`] with a free row 0, a
/// first-minimum scan of the last row, and a parent-chain backtrack to that
/// free row. The only search evaluation for weighted costs and for both
/// Damerau variants — and the oracle [`search_bits`] is differentially tested
/// against.
///
/// Both halves are written to hold for *any* cost set rather than only the
/// unit-cost one; see the comments inside for the two places where that used
/// not to be true.
fn search_full_matrix<T: Unit>(
    source: &[T],
    target: &[T],
    costs: &Costs,
    variant: Variant,
) -> (usize, usize, f64) {
    let n = source.len();
    let m = target.len();
    let mat = full_matrix(source, target, costs, variant, true);

    // Minimum over the last row, seeded with column 0 — the empty-substring
    // candidate — rather than with a synthetic bound. `n + m` is an upper
    // bound only when every cost is 1.0: raise any cost above 1 and no cell
    // clears it, so the scan never fires and the function returns a distance
    // that is not a cell of its own matrix, with a `match_end` (and hence
    // substring and offset) that was never chosen. Column 0 always holds the
    // real cost of matching `source` against the empty substring, so seeding
    // from it is a valid bound for *any* cost set and makes the
    // empty-substring candidate a genuine participant.
    //
    // `>` keeps the FIRST minimum, so column 0 also wins every tie — which
    // is what the unit-cost scan already did, since `n + m > n` for `m >= 1`
    // made column 0 its first improvement.
    let mut min_distance = mat.cost_at(n, 0);
    let mut match_end = 0usize;
    for c in 1..=m {
        let cost = mat.cost_at(n, c);
        if min_distance > cost {
            min_distance = cost;
            match_end = c;
        }
    }

    // Walk parents back to the free row 0; the column the path sits in there
    // is where the matched substring starts.
    //
    // The walk must reach row 0, not stop one short of it. Stopping at row 1
    // and reporting `col - 1` assumes the final step into row 0 is a diagonal
    // one, which is only safe when a substitution costs no more than a
    // deletion: under unit costs a leading deletion can always be rewritten
    // as a substitution one column earlier for the same total, so `col - 1`
    // named a substring that still realised the reported distance. Weighted
    // costs break that rewrite — with `deletion = 1, substitution = 2`,
    // `levenshtein_search("adccb", "cdbb")` reports distance 3, achieved by
    // "db", while `col - 1` named "cdb", which is 4 away.
    //
    // Column 0 is the other exit: from `(r, 0)` the parent chain is pure
    // deletions up to `(0, 0)`, so the loop can stop there and report the
    // same 0.
    let mut row = n;
    let mut col = match_end;
    while row > 0 && col > 0 {
        let (pr, pc) = mat.parent[mat.idx(row, col)];
        row = pr as usize;
        col = pc as usize;
    }
    (col, match_end, min_distance)
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
    /// First column attaining the minimum of the last row (`>`, so ties keep
    /// the earliest column), seeded from column 0's cost `n` ahead of the
    /// scanned columns — the same seed [`search_full_matrix`] uses.
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
/// Requires non-empty operands (gated in [`search_unit`]). Verified
/// against the full-matrix oracle on full-`SearchResult` equality —
/// substring, `f64` distance bits, offset — across randomized corpora with
/// embedded near-matches forcing real ties, both unit types; and, through
/// the public entry point, against the substring brute force that shares no
/// code with either.
fn search_bits<T: BitPeq>(source: &[T], target: &[T]) -> (usize, usize, usize) {
    let n = source.len();
    let m = target.len();
    debug_assert!(n >= 1 && m >= 1);

    let fw = if n <= 64 {
        search_forward_word(source, target)
    } else {
        search_forward_blocks(source, target)
    };
    let match_end = fw.match_end;

    // The parent walk, with each parent recomputed instead of loaded. Stops
    // exactly where the full-matrix walk stops — on the free row 0 or on
    // column 0 — and reports the same column. `search_cell_cost` supplies
    // both boundaries (`D[0][c] = 0`, `D[r][0] = r`), so the row-1 and
    // column-1 cells the walk now passes through need no special case.
    let mut row = n;
    let mut col = match_end;
    while row > 0 && col > 0 {
        let insert = search_cell_cost(&fw, row, col - 1) + 1;
        let delete = search_cell_cost(&fw, row - 1, col) + 1;
        let substitute =
            search_cell_cost(&fw, row - 1, col - 1) + i64::from(source[row - 1] != target[col - 1]);

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

    (col, match_end, fw.min_distance as usize)
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

    // First-minimum scan, replicated exactly: seeded from column 0, whose
    // unit-cost value is `n`, then `>` over the scanned columns so ties keep
    // the earliest — the same seed [`search_full_matrix`] uses.
    let mut min_distance = n as i64;
    let mut match_end = 0usize;

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

    // Seeded from column 0 (unit-cost value `n`), as in
    // [`search_forward_word`] and [`search_full_matrix`].
    let mut min_distance = n as i64;
    let mut match_end = 0usize;

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

#[cfg(test)]
mod tests {
    use super::*;

    /// The one-row scalar recurrence at unit costs, as the `usize` the unit
    /// tier returns.
    ///
    /// The row kernels accumulate in `f64` because they serve the weighted
    /// tier; under [`Costs::UNIT`] every cell is a small exact integer, so
    /// narrowing is lossless and the oracle is still an independently written
    /// scalar dynamic program rather than the kernel under test.
    fn unit_rows<T: Unit>(source: &[T], target: &[T]) -> usize {
        exact_usize(plain_rows(source, target, &Costs::UNIT))
    }

    /// [`unit_rows`] for the three-row OSA recurrence.
    fn unit_osa_rows<T: Unit>(source: &[T], target: &[T]) -> usize {
        exact_usize(osa_rows(source, target, &Costs::UNIT))
    }

    /// [`unit_rows`] for the full-matrix evaluation of `variant`.
    fn unit_matrix<T: Unit>(source: &[T], target: &[T], variant: Variant) -> usize {
        exact_usize(full_matrix(source, target, &Costs::UNIT, variant, false).final_cost())
    }

    fn lev(a: &str, b: &str) -> usize {
        levenshtein(a, b)
    }

    /// A `LevenshteinCosts` the test knows is admissible.
    #[track_caller]
    fn lev_costs(insertion: f64, deletion: f64, substitution: f64) -> LevenshteinCosts {
        LevenshteinCosts::new(insertion, deletion, substitution).expect("admissible costs")
    }

    /// An `OsaCosts` the test knows is admissible.
    #[track_caller]
    fn osa_costs(insertion: f64, deletion: f64, substitution: f64, transposition: f64) -> OsaCosts {
        OsaCosts::new(insertion, deletion, substitution, transposition).expect("admissible costs")
    }

    /// A `DamerauCosts` the test knows is admissible.
    #[track_caller]
    fn damerau_costs(
        insertion: f64,
        deletion: f64,
        substitution: f64,
        transposition: f64,
    ) -> DamerauCosts {
        DamerauCosts::new(insertion, deletion, substitution, transposition)
            .expect("admissible costs")
    }

    #[test]
    fn classic_distances() {
        assert_eq!(lev("kitten", "sitting"), 3);
        assert_eq!(lev("saturday", "sunday"), 3);
        assert_eq!(lev("", ""), 0);
        assert_eq!(lev("abc", ""), 3);
        assert_eq!(lev("", "abc"), 3);
        assert_eq!(lev("same", "same"), 0);
    }

    #[test]
    fn transposition_only_counts_for_damerau() {
        assert_eq!(levenshtein("ab", "ba"), 2);
        assert_eq!(damerau_levenshtein("ab", "ba"), 1);
    }

    #[test]
    fn damerau_and_osa_are_different_functions() {
        // "ca" -> "abc" is 2 under unrestricted Damerau but 3 under optimal
        // string alignment: OSA forbids editing between the swapped pair.
        // The choice lives in the function name, not in an option field.
        assert_eq!(damerau_levenshtein("ca", "abc"), 2);
        assert_eq!(osa("ca", "abc"), 3);
        // Both count a bare adjacent swap as one edit.
        assert_eq!(damerau_levenshtein("ab", "ba"), 1);
        assert_eq!(osa("ab", "ba"), 1);
    }

    #[test]
    fn asymmetric_costs_are_respected() {
        // Asymmetric prices are the weighted tier's whole reason to exist,
        // and they are only expressible there: the unit tier has no argument
        // to make asymmetric.
        let costs = lev_costs(1.0, 3.0, 1.0);
        // "abc" -> "ab" is one deletion.
        assert_eq!(levenshtein_weighted("abc", "ab", &costs), 3.0);
        // "ab" -> "abc" is one insertion, still cost 1.
        assert_eq!(levenshtein_weighted("ab", "abc", &costs), 1.0);
    }

    #[test]
    fn fractional_and_zero_costs() {
        let frac = lev_costs(0.5, 1.5, 0.75);
        assert_eq!(levenshtein_weighted("ab", "abc", &frac), 0.5);

        // Zero is admissible, and prices every script at 0.0 by
        // construction: the result is a pseudometric rather than a metric.
        let zero = lev_costs(0.0, 0.0, 0.0);
        assert_eq!(levenshtein_weighted("kitten", "sitting", &zero), 0.0);
    }

    #[test]
    fn scalar_semantics_count_astral_characters_once() {
        // `docs/design/distance-contract.md` §2.5: one scalar is one unit,
        // so deleting "😀" from "a😀b" is a single deletion — the UTF-16 unit
        // charged two, one per surrogate half.
        assert_eq!(lev("a😀b", "ab"), 1);
        assert_eq!(lev("😀", ""), 1);
        assert_eq!(lev("", "😀"), 1);
        assert_eq!(lev("😀", "😀"), 0);
        // Substituting one astral character for another is one edit, not two.
        assert_eq!(lev("😀", "𝕳"), 1);
        // And an adjacent swap of two astral characters is one under OSA.
        assert_eq!(osa("😀😁", "😁😀"), 1);
        // The empty-operand clause: the answer is the other operand's scalar
        // count, for every plane.
        for t in ["", "a", "abc", "café", "Москва", "😀", "a😀b", "𝕳𝖊𝖑𝖑𝖔"]
        {
            assert_eq!(lev("", t), t.chars().count(), "levenshtein(\"\", {t:?})");
            assert_eq!(lev(t, ""), t.chars().count(), "levenshtein({t:?}, \"\")");
            assert_eq!(
                damerau_levenshtein("", t),
                t.chars().count(),
                "damerau_levenshtein(\"\", {t:?})"
            );
            assert_eq!(osa("", t), t.chars().count(), "osa(\"\", {t:?})");
        }
    }

    #[test]
    fn bmp_non_ascii_is_one_unit_per_char() {
        assert_eq!(lev("café", "cafe"), 1);
        assert_eq!(lev("Москва", "Москва"), 0);
    }

    #[test]
    fn search_finds_best_substring() {
        let r = levenshtein_search("ca", "abc");
        assert_eq!(r.substring(), "a");
        assert_eq!(r.distance(), 1);
        assert_eq!(r.range(), 0..1);
    }

    #[test]
    fn every_fast_path_agrees_with_the_full_matrix() {
        // Each of the three public distances must agree with the full matrix
        // evaluating its own variant's recurrence.
        let words = [
            "kitten", "sitting", "flaw", "lawn", "", "a", "abcdef", "fedcba",
        ];
        for a in words {
            for b in words {
                for (variant, fast) in [
                    (Variant::Plain, levenshtein(a, b)),
                    (Variant::Damerau, damerau_levenshtein(a, b)),
                    (Variant::Osa, osa(a, b)),
                ] {
                    let slow = dispatch(a, b, |ops| match ops {
                        Operands::Bytes(s, t) => unit_matrix(s, t, variant),
                        Operands::Units(s, t) => unit_matrix(s, t, variant),
                    });
                    assert_eq!(fast, slow, "{a:?} vs {b:?} {variant:?}");
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

        for &a_len in &lengths {
            for &b_len in &lengths {
                for _ in 0..20 {
                    let a = random_string(&mut rng, a_len);
                    let b = random_string(&mut rng, b_len);

                    let via_fast_path = levenshtein(&a, &b);
                    // `unit_rows` directly, bypassing
                    // `plain_levenshtein_unit`'s dispatch entirely -- the
                    // independent baseline.
                    let via_plain_rows = dispatch(&a, &b, |ops| match ops {
                        Operands::Bytes(s, t) => unit_rows(s, t),
                        Operands::Units(s, t) => unit_rows(s, t),
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
    fn bit_vector_agrees_on_scalar_input() {
        // Same property, forced through the `Operands::Units` (char) path --
        // `bit_vector_distance`'s `T: Unit + Hash` bound is exercised for
        // both monomorphizations, not just u8.
        let mut rng = Xorshift64(0x1234_5678_9ABC_DEF0);
        let pairs = [
            ("café", "cafe"),
            ("Москва", "Масква"),
            ("😀😀😀", "😀"),
            ("a😀b😀c", "abc"),
        ];
        for (a, b) in pairs {
            let via_fast_path = levenshtein(a, b);
            let via_plain_rows = dispatch(a, b, |ops| match ops {
                Operands::Bytes(s, t) => unit_rows(s, t),
                Operands::Units(s, t) => unit_rows(s, t),
            });
            assert_eq!(via_fast_path, via_plain_rows, "mismatch for {a:?} vs {b:?}");
        }
        // A batch of random Cyrillic (non-ASCII, forces the char path) pairs.
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
            let via_fast_path = levenshtein(&a, &b);
            let via_plain_rows = dispatch(&a, &b, |ops| match ops {
                Operands::Bytes(s, t) => unit_rows(s, t),
                Operands::Units(s, t) => unit_rows(s, t),
            });
            assert_eq!(via_fast_path, via_plain_rows, "mismatch for {a:?} vs {b:?}");
        }
    }

    #[test]
    fn utf8_affix_pretrim_matches_the_scalar_oracle() {
        let mut cases = Vec::new();

        let base = "аб😀中".repeat(100);
        let mut changed: Vec<char> = base.chars().collect();
        changed[200] = 'ж';
        cases.push((base.clone(), changed.into_iter().collect::<String>()));
        cases.push((base.clone(), base));

        // These pairs share UTF-8 continuation bytes inside their differing
        // final scalar. A byte-only trim would create invalid slices; the
        // pretrim must retreat to a char boundary before scalar decoding.
        cases.push((
            format!("{}é", "x".repeat(65)),
            format!("{}©", "x".repeat(65)),
        ));
        cases.push((
            format!("{}😀", "д".repeat(40)),
            format!("{}😁", "д".repeat(40)),
        ));

        for (source, target) in cases {
            let actual = levenshtein(&source, &target);
            let expected = dispatch(&source, &target, |ops| match ops {
                Operands::Bytes(s, t) => unit_rows(s, t),
                Operands::Units(s, t) => unit_rows(s, t),
            });
            assert_eq!(actual, expected, "{source:?} -> {target:?}");
        }
    }

    #[test]
    fn the_weighted_tier_prices_each_operation_separately() {
        // The bit-vector kernels have no formulation for a weighted
        // operation, which is why the weighted tier is a different function
        // rather than a different argument: no cost value can move a call
        // onto them. What remains to check here is that the scalar
        // recurrence charges the right price per operation.
        let dearer_inserts = lev_costs(2.0, 1.0, 1.0);
        assert_eq!(levenshtein_weighted("abc", "ab", &dearer_inserts), 1.0); // one deletion, cost 1
        assert_eq!(levenshtein_weighted("ab", "abc", &dearer_inserts), 2.0); // one insertion, cost 2
    }

    #[test]
    fn one_row_weighted_matches_two_row_oracle_bit_for_bit() {
        // The rolling-row rewrite is an allocation/layout optimization only.
        // Keep the retired two-row recurrence as an independent test oracle,
        // including rectangular, empty and non-ASCII inputs and unusual costs
        // whose IEEE-754 evaluation order is observable.
        // The cost grid is exactly the admissible one: every set here can be
        // built through `LevenshteinCosts::new`, so every one of them can
        // actually reach the recurrence from the public surface. The
        // `INFINITY`/`NaN`/negative entries this test used to carry are
        // unconstructable now and are gone; the property they pinned —
        // that the two row shapes associate their additions identically —
        // survives on the remaining domain, and the empty-operand fold is
        // pinned separately by
        // `weighted_empty_operands_are_a_fold_of_repeated_additions`.
        let options = [
            Costs::UNIT,
            Costs {
                insertion: 0.5,
                deletion: 1.5,
                substitution: 0.75,
                ..Costs::UNIT
            },
            Costs {
                insertion: 0.0,
                deletion: 0.0,
                substitution: 0.0,
                ..Costs::UNIT
            },
            Costs {
                insertion: 0.1,
                deletion: 0.2,
                substitution: 0.15,
                ..Costs::UNIT
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
                assert_eq!(
                    actual.to_bits(),
                    expected.to_bits(),
                    "{source:?} -> {target:?}, {opts:?}"
                );
            }
        }
    }

    #[test]
    fn empty_plain_distance_matches_the_row_recurrence_bit_for_bit() {
        // `levenshtein_weighted_impl` handles these before scalar
        // materialization. The repeated additions (rather than
        // `len as f64 * cost`) intentionally retain the scalar recurrence's
        // rounding, so the shortcut and the general path cannot disagree.
        for cost in [0.0, -0.0, 0.1, 0.5, 1.0, 3.25] {
            let insert = lev_costs(cost, 1.0, 1.0);
            let expected_insert = dispatch("", "a😀b", |ops| match ops {
                Operands::Bytes(s, t) => plain_rows_two_oracle(s, t, &insert.costs()),
                Operands::Units(s, t) => plain_rows_two_oracle(s, t, &insert.costs()),
            });
            let actual_insert = levenshtein_weighted("", "a😀b", &insert);

            let delete = lev_costs(1.0, cost, 1.0);
            let expected_delete = dispatch("a😀b", "", |ops| match ops {
                Operands::Bytes(s, t) => plain_rows_two_oracle(s, t, &delete.costs()),
                Operands::Units(s, t) => plain_rows_two_oracle(s, t, &delete.costs()),
            });
            let actual_delete = levenshtein_weighted("a😀b", "", &delete);

            for (actual, expected) in [
                (actual_insert, expected_insert),
                (actual_delete, expected_delete),
            ] {
                assert_eq!(actual.to_bits(), expected.to_bits());
            }
        }
    }

    #[test]
    fn weighted_empty_operands_are_a_fold_of_repeated_additions() {
        // `docs/design/distance-contract.md` §3.1: the weighted
        // empty-operand cost is `c` added to itself `n` times, left to
        // right — not `n as f64 * c`. The two differ under IEEE-754, and the
        // fold is what the general recurrence's own row accumulation
        // produces. The expected value here is an independently written fold,
        // never a multiplication and never a recorded output.
        for (cost, text) in [
            (0.1f64, "abcdefghij"),
            (0.1, "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
            (0.30000000000000004, "Москва"),
            (7.7, "abc"),
            // Astral: the operand whose scalar count differs from its
            // UTF-16 unit count, so the fold length is the one the contract
            // specifies rather than the one the old unit would have used.
            (0.1, "😀😁😂"),
        ] {
            let units = text.chars().count();
            let mut fold = 0.0f64;
            for _ in 0..units {
                fold += cost;
            }

            let insert_only = lev_costs(cost, 1.0, 1.0);
            assert_eq!(
                levenshtein_weighted("", text, &insert_only).to_bits(),
                fold.to_bits(),
                "insertions into {text:?} at {cost}"
            );

            let delete_only = lev_costs(1.0, cost, 1.0);
            assert_eq!(
                levenshtein_weighted(text, "", &delete_only).to_bits(),
                fold.to_bits(),
                "deletions from {text:?} at {cost}"
            );
        }

        // The distinction is observable, not theoretical: for these operands
        // the fold and the product are different `f64`s.
        let cost = 0.1;
        let text = "abcdefghij";
        let units = text.chars().count();
        let mut fold = 0.0f64;
        for _ in 0..units {
            fold += cost;
        }
        assert_ne!(fold.to_bits(), (units as f64 * cost).to_bits());
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

        for &a_len in &lengths {
            for &b_len in &lengths {
                for _ in 0..3 {
                    let a = random_string(&mut rng, a_len);
                    let b = random_string(&mut rng, b_len);

                    let via_fast_path = levenshtein(&a, &b);
                    let via_plain_rows = dispatch(&a, &b, |ops| match ops {
                        Operands::Bytes(s, t) => unit_rows(s, t),
                        Operands::Units(s, t) => unit_rows(s, t),
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
        for pattern_len in [65usize, 129, 257] {
            let shorter = vec![b'z'; pattern_len];
            for prefix_len in [0usize, 1, 63, 64, 65, 127, 128, 300, 1_000] {
                let mut longer = vec![b'a'; prefix_len];
                longer.push(b'z');
                longer.extend_from_slice(b"bbb");
                assert_eq!(
                    bit_vector_distance_blocks(&shorter, &longer),
                    unit_rows(&shorter, &longer),
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
                    unit_rows(&shorter, &longer),
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
        for &shorter_len in &[65usize, 130, 260] {
            for _ in 0..2 {
                let a = random_string(&mut rng, shorter_len);
                let b = random_string(&mut rng, 5000);
                let via_fast_path = levenshtein(&a, &b);
                let via_plain_rows = dispatch(&a, &b, |ops| match ops {
                    Operands::Bytes(s, t) => unit_rows(s, t),
                    Operands::Units(s, t) => unit_rows(s, t),
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
    fn bit_vector_blocks_agrees_on_scalar_input() {
        // Same property as `bit_vector_agrees_on_scalar_input`, but for
        // lengths that force the multi-block path -- `T: Unit + Hash` is
        // exercised for the `char` monomorphization here too, not just `u8`.
        let mut rng = Xorshift64(0x9E37_79B9_7F4A_7C15);
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
                let via_fast_path = levenshtein(&a, &b);
                let via_plain_rows = dispatch(&a, &b, |ops| match ops {
                    Operands::Bytes(s, t) => unit_rows(s, t),
                    Operands::Units(s, t) => unit_rows(s, t),
                });
                assert_eq!(via_fast_path, via_plain_rows, "mismatch for {a:?} vs {b:?}");
            }
        }
        // Astral (non-BMP) input: one unit each, four bytes each.
        let pairs = [
            ("😀".repeat(80), "😀".repeat(79)),
            ("a😀".repeat(70), "b😀".repeat(70)),
        ];
        for (a, b) in pairs {
            let via_fast_path = levenshtein(&a, &b);
            let via_plain_rows = dispatch(&a, &b, |ops| match ops {
                Operands::Bytes(s, t) => unit_rows(s, t),
                Operands::Units(s, t) => unit_rows(s, t),
            });
            assert_eq!(via_fast_path, via_plain_rows, "mismatch for {a:?} vs {b:?}");
        }
    }

    #[test]
    fn bit_vector_blocks_matches_hand_computed_edge_cases() {
        // Hand-verifiable cases at exact block boundaries, independent of
        // the randomized tests above.
        // Exactly 65 units: one full block plus a single-bit second block.
        // "a" * 65 vs "a" * 64 + "b": one substitution.
        let a = "a".repeat(65);
        let b = format!("{}b", "a".repeat(64));
        assert_eq!(levenshtein(&a, &b), 1);
        // Identical long strings: distance 0.
        let c = "abcde".repeat(50); // 250 units
        assert_eq!(levenshtein(&c, &c), 0);
        // Completely disjoint alphabets, equal length: distance == length.
        let d = "x".repeat(200);
        let e = "y".repeat(200);
        assert_eq!(levenshtein(&d, &e), 200);
        // One empty operand past the single-word bound: distance == length.
        let f = "z".repeat(150);
        assert_eq!(levenshtein(&f, ""), 150);
    }

    // -----------------------------------------------------------------
    // Adversarial coverage for `bit_vector_distance_blocks`.
    //
    // Everything below computes the oracle via `plain_rows` directly
    // (bypassing `plain_levenshtein_unit`'s dispatch entirely, exactly like
    // the property tests above), never by hand-derivation, per the review
    // brief: "compute the correct answer independently ... and assert
    // equality."
    // -----------------------------------------------------------------

    /// `plain_rows`, called directly through `dispatch` -- the independent
    /// oracle every adversarial test below checks the fast path against.
    fn oracle_plain_rows(a: &str, b: &str, costs: &Costs) -> f64 {
        dispatch(a, b, |ops| match ops {
            Operands::Bytes(s, t) => plain_rows(s, t, costs),
            Operands::Units(s, t) => plain_rows(s, t, costs),
        })
    }

    /// [`oracle_plain_rows`] at unit costs, as the `usize` the unit tier
    /// returns — see [`unit_rows`].
    fn oracle_plain_rows_unit(a: &str, b: &str) -> usize {
        exact_usize(oracle_plain_rows(a, b, &Costs::UNIT))
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
        for &len in &[128usize, 129, 192, 193, 256, 257, 320, 321, 384, 385] {
            let base = random_string(&mut rng, len);
            let base_bytes = base.as_bytes();

            assert_eq!(levenshtein(&base, &base), 0, "identical len {len}");

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

                let via_fast_path = levenshtein(&base, &mutated_str);
                let via_plain_rows = oracle_plain_rows_unit(&base, &mutated_str);
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
            let via_fast_path = levenshtein(&a, &b);
            let via_plain_rows = oracle_plain_rows_unit(&a, &b);
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
            let via_fast_path = levenshtein(&a, &b);
            let via_plain_rows = oracle_plain_rows_unit(&a, &b);
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
        for &len in &[128usize, 129, 192, 200, 256, 257, 320, 400] {
            let a: String = (0..len)
                .map(|i| if i % 2 == 0 { 'a' } else { 'b' })
                .collect();
            let b: String = (0..len)
                .map(|i| if i % 2 == 0 { 'b' } else { 'a' })
                .collect();
            let via_fast_path = levenshtein(&a, &b);
            let via_plain_rows = oracle_plain_rows_unit(&a, &b);
            assert_eq!(
                via_fast_path, via_plain_rows,
                "alternating mismatch len {len}"
            );

            // Same alternating pattern, but `b` is one unit longer -- forces
            // the phase shift to interact with a length difference too.
            let b_longer = format!("{b}a");
            let via_fast_path2 = levenshtein(&a, &b_longer);
            let via_plain_rows2 = oracle_plain_rows_unit(&a, &b_longer);
            assert_eq!(
                via_fast_path2, via_plain_rows2,
                "alternating + 1 mismatch len {len}"
            );
        }

        // A 3-symbol cycle against a 2-symbol cycle: more Peq entries,
        // still highly repetitive, unequal lengths.
        let a: String = (0..500).map(|i| ['a', 'b', 'c'][i % 3]).collect();
        let b: String = (0..480).map(|i| ['b', 'a'][i % 2]).collect();
        let via_fast_path = levenshtein(&a, &b);
        let via_plain_rows = oracle_plain_rows_unit(&a, &b);
        assert_eq!(via_fast_path, via_plain_rows, "3-cycle vs 2-cycle mismatch");
    }

    #[test]
    fn bit_vector_blocks_disjoint_and_near_identical_multiblock() {
        let mut rng = Xorshift64(0xD15C_A5ED_9999_0001);

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
            let via_fast_path = levenshtein(&a, &b);
            let via_plain_rows = oracle_plain_rows_unit(&a, &b);
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
            let via_fast_path = levenshtein(&base, &mutated_str);
            let via_plain_rows = oracle_plain_rows_unit(&base, &mutated_str);
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
        for &shorter_len in &[65usize, 129, 193, 257, 321, 385] {
            for &longer_len in &[4000usize, 10_007] {
                let a = random_string(&mut rng, shorter_len);
                let b = random_string(&mut rng, longer_len);
                let via_fast_path = levenshtein(&a, &b);
                let via_plain_rows = oracle_plain_rows_unit(&a, &b);
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
        let via_fast_path = levenshtein(&pattern, &target_str);
        let via_plain_rows = oracle_plain_rows_unit(&pattern, &target_str);
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
            let via_plain_rows = unit_rows(&shorter, &longer);
            assert_eq!(
                via_blocks, via_plain_rows,
                "empty-longer mismatch len {len}"
            );
            assert_eq!(via_blocks, len);
        }

        // Same edge case through the public API, at a size well past the
        // single-word bound and (for the empty operand's counterpart) well
        // into 5-digit territory.
        assert_eq!(levenshtein("", &"m".repeat(12_000)), 12_000);
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

            let via_fast_path = levenshtein(&shorter, &longer_str);
            let via_plain_rows = oracle_plain_rows_unit(&shorter, &longer_str);
            assert_eq!(
                via_fast_path, via_plain_rows,
                "block-boundary distance mismatch, blocks_to_flip={blocks_to_flip}"
            );
            assert_eq!(via_plain_rows, blocks_to_flip * 64);
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

    /// A mix of BMP and astral characters, built to *exactly* `units`
    /// scalars, so length-boundary arithmetic (block counts, `last_bit`) is
    /// exercised precisely rather than approximately.
    ///
    /// Under the scalar unit an astral character is one unit like any other,
    /// so the mix varies the operands' *byte* width (2, 3 and 4 bytes per
    /// unit) without varying their length — which is what makes the
    /// byte-vs-unit distinction visible to the kernels' index arithmetic.
    fn random_unicode_wide(rng: &mut SplitMix64, units: usize) -> String {
        const BMP: &[char] = &['а', 'б', 'в', 'ñ', 'ü', '中', '字'];
        const ASTRAL: &[char] = &['😀', '𝔘', '𝕏', '🎉'];
        let mut s = String::new();
        for _ in 0..units {
            if rng.next_range(3) == 0 {
                s.push(ASTRAL[rng.next_range(ASTRAL.len())]);
            } else {
                s.push(BMP[rng.next_range(BMP.len())]);
            }
        }
        debug_assert_eq!(s.chars().count(), units);
        s
    }

    #[test]
    fn bit_vector_blocks_large_scale_differential_ascii_and_scalar() {
        // A second, independent differential test: a different PRNG
        // algorithm (SplitMix64, not Xorshift64) from every other
        // randomized test in this module, a wider alphabet than the
        // deliberately-narrow `abcde` used elsewhere, explicit length pairs
        // reaching beyond 10,000 units, and both the ASCII/`u8` dispatch
        // path and the non-ASCII/`char` path (including astral characters,
        // so four-byte units are covered at large scale too, not just in
        // the smaller dedicated astral test). Lengths are an
        // explicit fixed list rather than a random cross product so the
        // total cost of the `O(n*m)` `plain_rows` oracle stays bounded and
        // predictable even at these sizes.
        let mut rng = SplitMix64(0x243F_6A88_85A3_08D3);

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
            let via_fast_path = levenshtein(&a, &b);
            let via_plain_rows = oracle_plain_rows_unit(&a, &b);
            assert_eq!(
                via_fast_path, via_plain_rows,
                "ascii mismatch shorter_len={shorter_len} longer_len={longer_len}"
            );

            let ua = random_unicode_wide(&mut rng, shorter_len);
            let ub = random_unicode_wide(&mut rng, longer_len);
            let via_fast_path_u = levenshtein(&ua, &ub);
            let via_plain_rows_u = oracle_plain_rows_unit(&ua, &ub);
            assert_eq!(
                via_fast_path_u, via_plain_rows_u,
                "scalar mismatch shorter_len={shorter_len} longer_len={longer_len}"
            );
        }
    }

    // -- OSA (restricted Damerau) bit-vector battery ------------------------

    /// `unit_osa_rows` called directly, bypassing [`osa_unit`]'s fast-path
    /// dispatch entirely — the independent scalar oracle for every OSA
    /// bit-vector test below.
    fn oracle_osa(a: &str, b: &str) -> usize {
        dispatch(a, b, |ops| match ops {
            Operands::Bytes(s, t) => unit_osa_rows(s, t),
            Operands::Units(s, t) => unit_osa_rows(s, t),
        })
    }

    #[test]
    fn osa_bit_vector_agrees_with_osa_rows_on_random_pairs() {
        // The correctness-defining differential test for the OSA fast
        // paths, mirroring the plain-Levenshtein batteries above: lengths
        // straddle the scalar/single-word boundary (0-3), the single-word/
        // block boundary (63/64/65), and several block boundaries. Both
        // argument orders are asserted, because the fast path swaps
        // operands (OSA under unit costs is symmetric; the oracle computes
        // whichever order it is given).
        let mut rng = Xorshift64(0x05A0_5A05_A05A);
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
                        osa(&a, &b),
                        expected,
                        "mismatch for len {a_len} vs len {b_len}"
                    );
                    assert_eq!(
                        osa(&b, &a),
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
                    osa(&a, &b),
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
                        osa(&a, &b),
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
        // Alternating two-symbol strings are the all-transposition worst
        // case; single-symbol repetition degenerates Peq to one dense row;
        // disjoint alphabets never match at all.
        for &len in &[63usize, 64, 65, 128, 129, 200] {
            let ab: String = "ab".chars().cycle().take(len).collect();
            let ba: String = "ba".chars().cycle().take(len).collect();
            assert_eq!(osa(&ab, &ba), oracle_osa(&ab, &ba));

            let aa = "a".repeat(len);
            let bb = "b".repeat(len);
            assert_eq!(osa(&aa, &bb), oracle_osa(&aa, &bb));
            assert_eq!(osa(&aa, &ab), oracle_osa(&aa, &ab));
        }
        // Empty and one-unit operands stay on the scalar path (gate starts
        // at 2) but must agree regardless.
        assert_eq!(osa("", "abc"), 3);
        assert_eq!(osa("a", "abc"), 2);
    }

    #[test]
    fn osa_classic_fixtures() {
        // The OSA-vs-unrestricted discriminator: restricted forbids
        // editing between the transposed pair, so "CA" -> "ABC" costs 3
        // (unrestricted Damerau reaches it in 2).
        assert_eq!(osa("CA", "ABC"), 3);
        assert_eq!(osa("CA", "AC"), 1);
        assert_eq!(osa("ab", "ba"), 1);
        assert_eq!(osa("kitten", "sitting"), 3);
        // A 131-unit pair built so the CA/AC transposition sits exactly
        // astride the first 64-bit word boundary -- the one place the
        // block kernel's cross-word transposition carry is load-bearing.
        let filler = "a".repeat(64);
        let s1 = format!("a{filler}CA{filler}a");
        let s2 = format!("b{filler}AC{filler}b");
        assert_eq!(osa(&s1, &s2), 3);
        assert_eq!(oracle_osa(&s1, &s2), 3);
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
    fn osa_scalar_and_astral_inputs_agree() {
        // The `char` monomorphization (FxHashMap-backed Peq), including
        // astral input where one char is two units.
        let mut rng = Xorshift64(0x0111_0111_0111);
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
                osa(&a, &b),
                oracle_osa(&a, &b),
                "cyrillic mismatch {a_len}x{b_len}"
            );
        }
        assert_eq!(
            osa(
                "\u{418}\u{432}\u{430}\u{43d}\u{43a}\u{43e}",
                "\u{41f}\u{435}\u{442}\u{440}\u{443}\u{43d}\u{43a}\u{43e}"
            ),
            5
        );
        let a = "\u{1F600}".repeat(40);
        let b = format!("a{}", "\u{1F600}".repeat(39));
        assert_eq!(osa(&a, &b), oracle_osa(&a, &b));
    }

    #[test]
    fn osa_weighted_evaluates_the_scalar_recurrence_bit_for_bit() {
        // `osa_weighted` is a different function from `osa`, not the same
        // function with a different argument, so there is no fast path for a
        // cost value to escape from. What it must do is evaluate the scalar
        // three-row recurrence exactly — including for a transposition
        // priced below the delete/insert pair it replaces, which OSA (unlike
        // unrestricted Damerau) is defined for.
        for transposition in [0.5, 2.0, 0.0, 0.25] {
            let costs = osa_costs(1.0, 1.0, 1.0, transposition);
            let got = osa_weighted("abcd", "abdc", &costs);
            let want = dispatch("abcd", "abdc", |ops| match ops {
                Operands::Bytes(s, t) => osa_rows(s, t, &costs.costs()),
                Operands::Units(s, t) => osa_rows(s, t, &costs.costs()),
            });
            assert_eq!(got.to_bits(), want.to_bits());
        }
        let dearer_inserts = osa_costs(2.0, 1.0, 1.0, 1.0);
        assert_eq!(osa_weighted("ab", "abc", &dearer_inserts), 2.0);
        assert_eq!(osa_weighted("abc", "ab", &dearer_inserts), 1.0);
    }

    // -- Unrestricted-Damerau battery ---------------------------------------

    /// A from-scratch transcription of the published Lowrance–Wagner
    /// recurrence for unrestricted Damerau–Levenshtein — the *definition*
    /// this module's kernels are measured against, sharing no code with any
    /// of them.
    ///
    /// Written straight from the textbook statement of the algorithm: an
    /// explicit `(n+2) × (m+2)` matrix whose row and column `−1` hold the
    /// `maxdist` sentinel, a `da` map from symbol to its last source row
    /// (updated only *after* a row completes), a per-row `db` holding the
    /// last column that matched, and the four-way minimum
    ///
    /// ```text
    /// H[i][j] = min( H[i-1][j-1] + cost,        // substitution
    ///                H[i][j-1]   + 1,           // insertion
    ///                H[i-1][j]   + 1,           // deletion
    ///                H[k-1][l-1] + (i-k-1) + 1 + (j-l-1) )  // transposition
    /// ```
    ///
    /// Transcribed from that recurrence and nothing else. No affix
    /// trimming, no linear-space trickery, no operand swapping — the point
    /// is that it is boring enough to be read against the paper and
    /// believed.
    fn reference_damerau_units(a: &[char], b: &[char]) -> usize {
        let (n, m) = (a.len(), b.len());
        if n == 0 {
            return m;
        }
        if m == 0 {
            return n;
        }
        let maxdist = (n + m) as i64;
        let w = m + 2;
        // `h[(i + 1) * w + (j + 1)]` is `H[i][j]` for `i, j` in `-1..`.
        let mut h = vec![0i64; (n + 2) * w];
        h[0] = maxdist;
        for i in 0..=n {
            h[(i + 1) * w] = maxdist;
            h[(i + 1) * w + 1] = i as i64;
        }
        for j in 0..=m {
            h[j + 1] = maxdist;
            h[w + j + 1] = j as i64;
        }

        let mut da: FxHashMap<char, usize> = FxHashMap::default();
        for i in 1..=n {
            let mut db = 0usize;
            for j in 1..=m {
                let k = da.get(&b[j - 1]).copied().unwrap_or(0);
                let l = db;
                let cost = if a[i - 1] == b[j - 1] {
                    db = j;
                    0
                } else {
                    1
                };
                let substitute = h[i * w + j] + cost;
                let insert = h[(i + 1) * w + j] + 1;
                let delete = h[i * w + j + 1] + 1;
                let transpose =
                    h[k * w + l] + (i as i64 - k as i64 - 1) + 1 + (j as i64 - l as i64 - 1);
                h[(i + 1) * w + j + 1] = substitute.min(insert).min(delete).min(transpose);
            }
            da.insert(a[i - 1], i);
        }
        h[(n + 1) * w + m + 1] as usize
    }

    /// [`reference_damerau_units`] over Unicode scalars — the crate's own
    /// text-unit semantics, so `&str` inputs can be compared directly.
    ///
    /// Re-derived over scalars rather than re-recorded: the recurrence is
    /// unchanged, only the sequence it ranges over is, which is exactly what
    /// `docs/design/distance-contract.md` §6 requires of the seven oracles
    /// that were denominated in UTF-16.
    fn oracle_unrestricted(a: &str, b: &str) -> usize {
        let ua: Vec<char> = a.chars().collect();
        let ub: Vec<char> = b.chars().collect();
        reference_damerau_units(&ua, &ub)
    }

    /// `full_matrix`'s own unrestricted-Damerau branch, called directly —
    /// the weighted-cost fallback path, checked against the same reference.
    fn matrix_unrestricted(a: &str, b: &str, costs: &Costs) -> f64 {
        dispatch(a, b, |ops| match ops {
            Operands::Bytes(s, t) => full_matrix(s, t, costs, Variant::Damerau, false).final_cost(),
            Operands::Units(s, t) => full_matrix(s, t, costs, Variant::Damerau, false).final_cost(),
        })
    }

    #[test]
    fn damerau_matches_the_canonical_answers_on_the_former_quirk_fixtures() {
        // These are the fixtures that used to pin a non-canonical,
        // asymmetric recurrence (which answered 1, 2, 2, 3, 2 here). The
        // literals below are the canonical Lowrance-Wagner answers, worked
        // out from the published recurrence, and they are asserted both as
        // literals and against the from-scratch reference above, so neither
        // can drift alone.
        assert_eq!(damerau_levenshtein("bb", "abbb"), 2);
        assert_eq!(damerau_levenshtein("abbb", "bb"), 2);
        assert_eq!(damerau_levenshtein("dfcb", "bdffc"), 3);
        assert_eq!(damerau_levenshtein("bdffc", "dfcb"), 3);
        assert_eq!(damerau_levenshtein("aabcbbb", "cabbccaab"), 5);
        assert_eq!(damerau_levenshtein("cabbccaab", "aabcbbb"), 5);
        assert_eq!(damerau_levenshtein("ca", "abc"), 2);
        assert_eq!(damerau_levenshtein("abc", "ca"), 2);
        for (a, b) in [
            ("bb", "abbb"),
            ("abbb", "bb"),
            ("dfcb", "bdffc"),
            ("bdffc", "dfcb"),
            ("aabcbbb", "cabbccaab"),
            ("cabbccaab", "aabcbbb"),
            ("ca", "abc"),
            ("abc", "ca"),
        ] {
            assert_eq!(
                damerau_levenshtein(a, b),
                oracle_unrestricted(a, b),
                "reference mismatch {a:?} vs {b:?}"
            );
            assert_eq!(
                exact_usize(matrix_unrestricted(a, b, &Costs::UNIT)),
                oracle_unrestricted(a, b),
                "matrix mismatch {a:?} vs {b:?}"
            );
        }
    }

    #[test]
    fn damerau_unit_fast_path_agrees_with_the_reference_on_random_pairs() {
        // Small alphabets maximise last-row/last-column interactions and
        // match-cell transpositions -- the parts of the recurrence a
        // linear-space specialisation is most likely to get wrong.
        let mut rng = Xorshift64(0xDA3E_DA3E_DA3E);
        let lengths = [0usize, 1, 2, 3, 5, 8, 13, 21, 34, 55, 80];
        for &a_len in &lengths {
            for &b_len in &lengths {
                for _ in 0..6 {
                    let a = random_string(&mut rng, a_len);
                    let b = random_string(&mut rng, b_len);
                    assert_eq!(
                        damerau_levenshtein(&a, &b),
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
                damerau_levenshtein(&a, &b),
                oracle_unrestricted(&a, &b),
                "mismatch at {a_len}x{b_len}"
            );
        }
    }

    #[test]
    fn damerau_unit_fast_path_agrees_on_scalar_input() {
        let mut rng = Xorshift64(0xDA3E_0016_0016);
        const CYRILLIC: &[char] = &['\u{430}', '\u{431}', '\u{432}'];
        for &(a_len, b_len) in &[(5usize, 7usize), (20, 20), (40, 60), (80, 30)] {
            let a: String = (0..a_len)
                .map(|_| CYRILLIC[rng.next_range(CYRILLIC.len())])
                .collect();
            let b: String = (0..b_len)
                .map(|_| CYRILLIC[rng.next_range(CYRILLIC.len())])
                .collect();
            assert_eq!(
                damerau_levenshtein(&a, &b),
                oracle_unrestricted(&a, &b),
                "cyrillic mismatch {a_len}x{b_len}"
            );
        }
        let a = "\u{1F600}\u{1F601}\u{1F600}\u{1F601}";
        let b = "\u{1F601}\u{1F600}\u{1F601}";
        assert_eq!(damerau_levenshtein(a, b), oracle_unrestricted(a, b));
    }

    #[test]
    fn damerau_weighted_evaluates_the_full_matrix_bit_for_bit() {
        // Every cost set here satisfies `2 * transposition >= insertion +
        // deletion`, because `DamerauCosts::new` will not build one that does
        // not — see `damerau_costs_reject_below_the_lowrance_wagner_threshold`
        // for the sets it refuses.
        for costs in [
            damerau_costs(1.0, 1.0, 1.0, 2.0),
            damerau_costs(2.0, 1.0, 1.0, 1.5),
            damerau_costs(1.0, 0.25, 1.0, 0.75),
        ] {
            let got = damerau_levenshtein_weighted("ca", "abc", &costs);
            let want = matrix_unrestricted("ca", "abc", &costs.costs());
            assert_eq!(got.to_bits(), want.to_bits());
        }
        // A weighted transposition observably differs from unit cost: a swap
        // at 1.5 still beats the two substitutions it replaces, but no longer
        // costs the 1.0 the unit tier charges.
        let dearer = damerau_costs(1.0, 1.0, 1.0, 1.5);
        assert_eq!(damerau_levenshtein_weighted("ab", "ba", &dearer), 1.5);
        assert_eq!(damerau_levenshtein("ab", "ba"), 1);
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
                        unit_osa_rows(&a, &b),
                        "direct blocks mismatch len={len} p={p} prefix={prefix_len}"
                    );

                    // And through the public entry, both argument orders.
                    let sa = ascii_string(&a);
                    let sb = ascii_string(&b);
                    let expected = oracle_osa(&sa, &sb);
                    assert_eq!(
                        osa(&sa, &sb),
                        expected,
                        "public mismatch len={len} p={p} prefix={prefix_len}"
                    );
                    assert_eq!(
                        osa(&sb, &sa),
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
                unit_osa_rows(&a, &b),
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
                    unit_osa_rows(&s1, &s2),
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
                assert_eq!(osa(&sa, &sb), expected, "bc-sea mismatch len={len} p={p}");
                assert_eq!(expected, 1, "a bc<->cb swap must cost exactly 1");
            }
            // Tail swap in the sea.
            let mut s1 = vec![b'a'; len];
            let mut s2 = vec![b'a'; len];
            s1[len - 2] = b'b';
            s2[len - 1] = b'b';
            assert_eq!(
                osa_bit_vector_blocks(&s1, &s2),
                unit_osa_rows(&s1, &s2),
                "sea tail swap mismatch len={len}"
            );
        }
    }

    #[test]
    fn osa_tiny_and_empty_operands_all_entries() {
        // Every tiny pair through the public entry against the direct
        // scalar oracle (these all route to `osa_rows` or the
        // single-word kernel's lower gate — pinned regardless).
        let tiny = ["", "a", "b", "ab", "ba", "aa", "abc", "cba", "aab"];
        for a in tiny {
            for b in tiny {
                assert_eq!(osa(a, b), oracle_osa(a, b), "tiny mismatch {a:?} vs {b:?}");
            }
        }
        // Direct kernel calls at the domain edges the dispatch never
        // exercises: an empty text against a multi-block pattern (score
        // must be exactly m, from initialisation alone), a one-unit text,
        // and the documented-callable m = 1 blocks case.
        let long = vec![b'q'; 65];
        assert_eq!(osa_bit_vector_blocks(&long, b""), 65);
        assert_eq!(
            osa_bit_vector_blocks(&long, b"q"),
            unit_osa_rows(&long, b"q")
        );
        assert_eq!(
            osa_bit_vector_blocks(b"x", b"xy"),
            unit_osa_rows(b"x", b"xy")
        );
        assert_eq!(osa_bit_vector(b"xy", b"yx"), unit_osa_rows(b"xy", b"yx"));
    }

    #[test]
    fn osa_large_randomized_differential_splitmix() {
        // Independent large-scale OSA differential: SplitMix64 with a
        // fresh seed, swap-and-edit mutation of a shared base (rather than
        // two independent random strings — keeps the distance small, so
        // the transposition machinery, not the substitution floor,
        // decides the answer), both argument orders, ASCII and non-ASCII.
        let mut rng = SplitMix64(0x05A0_2026_0816_AAAA);
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
            assert_eq!(osa(&sa, &sb), expected, "round {round} ({len})");
            assert_eq!(osa(&sb, &sa), expected, "round {round} reversed ({len})");
        }

        // Non-ASCII rounds with boundary-adjacent swaps on BMP
        // characters, so swap positions are exact unit positions.
        const BMP: &[char] = &['\u{430}', '\u{431}', '\u{432}', '\u{4E2D}'];
        for &p in &[62usize, 63, 64, 65, 127, 128] {
            let chars: Vec<char> = (0..160).map(|_| BMP[rng.next_range(BMP.len())]).collect();
            let mut swapped = chars.clone();
            swapped.swap(p, p + 1);
            let a: String = chars.into_iter().collect();
            let b: String = swapped.into_iter().collect();
            assert_eq!(osa(&a, &b), oracle_osa(&a, &b), "scalar swap at {p}");
        }
        // Astral rounds: surrogate pairs at multi-block scale.
        let a = "\u{1F600}\u{1F601}".repeat(40); // 160 units
        let b = format!("\u{1F601}\u{1F600}{}", "\u{1F600}\u{1F601}".repeat(39));
        assert_eq!(osa(&a, &b), oracle_osa(&a, &b));
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

                let expected = unit_rows(&shorter, &longer);
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
    fn bitpeq_char_wide_alphabet_kernels_direct() {
        // The `char` monomorphization with far more than 256 distinct units
        // (impossible to reach via u8), so the FxHashMap-backed TableN
        // grows through hundreds of slot allocations, with recurrences
        // scattered across blocks. The alphabet starts at U+10000, so every
        // unit is astral — a value no UTF-16 code unit could carry.
        let mut rng = SplitMix64(0x0016_31DE_A1FA_0001);
        let scalar = |v: u64| char::from_u32(0x1_0000 + (v % 1000) as u32).expect("astral");
        for &m in &[65usize, 200, 400] {
            let shorter: Vec<char> = (0..m)
                .map(|i| {
                    if i % 3 == 0 {
                        scalar(i as u64)
                    } else {
                        scalar(rng.next_u64())
                    }
                })
                .collect();
            let longer: Vec<char> = (0..700).map(|_| scalar(rng.next_u64())).collect();
            assert_eq!(
                bit_vector_distance_blocks(&shorter, &longer),
                unit_rows(&shorter, &longer),
                "char wide-alphabet blocks mismatch m={m}"
            );
        }
        // Single-word `char` kernel on the same alphabet.
        let shorter: Vec<char> = (0..60).map(|_| scalar(rng.next_u64())).collect();
        let longer: Vec<char> = (0..500).map(|_| scalar(rng.next_u64())).collect();
        assert_eq!(
            bit_vector_distance(&shorter, &longer),
            unit_rows(&shorter, &longer)
        );
    }

    #[test]
    fn bitpeq_randomized_differential_splitmix() {
        // Fresh-seed randomized differential for the BitPeq-backed plain
        // kernels: full-range bytes, lengths sweeping every word boundary
        // neighbourhood, both kernels wherever their domains apply, plus
        // the two kernels pitted directly against each other.
        let mut rng = SplitMix64(0xB17E_2026_0816_BBBB);
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
                let expected = unit_rows(&shorter, &longer);
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
    fn damerau_unit_symbol_recurrence_stress() {
        // Structured symbol-recurrence patterns: every source symbol
        // reappears many times with varying gaps, so each row overwrites the
        // `last_row` entry and the `fr` saved cell that later transposition
        // candidates would have read. A saved cell kept one row too fresh
        // (updated mid-row rather than at row end) or one row too stale
        // (first occurrence kept forever) diverges from the reference here.
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
                    damerau_levenshtein(a, b),
                    expected,
                    "structured mismatch k={k} {a:?} vs {b:?}"
                );
                assert_eq!(
                    damerau_levenshtein(b, a),
                    expected,
                    "structured reversed mismatch k={k}"
                );
            }
        }

        // Two-symbol random battery: an alphabet of exactly {a, b}
        // maximises last-row/last-column churn (every row overwrites one of
        // only two entries, and the row's last match column moves constantly).
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
                damerau_levenshtein(&a, &b),
                oracle_unrestricted(&a, &b),
                "ab-random mismatch round {round} ({len_a}x{len_b})"
            );
        }
    }

    #[test]
    fn damerau_unit_degenerate_tiny_and_nul() {
        // Every tiny pair through the public entry, including empties.
        let tiny = ["", "a", "b", "ab", "ba", "aa", "aba", "bab"];
        for a in tiny {
            for b in tiny {
                assert_eq!(
                    damerau_levenshtein(a, b),
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
                damerau_levenshtein(&a, &b),
                oracle_unrestricted(&a, &b),
                "degenerate mismatch {}x{}",
                a.len(),
                b.len()
            );
        }
        // NUL bytes are valid ASCII: byte value 0 indexes the first entry
        // of the flat last-occurrence table, whose vacant sentinel must not
        // be confused with a legitimate row.
        for (a, b) in [
            ("\0ab\0", "b\0a"),
            ("\0\0", "\0"),
            ("a\0b", "ab\0"),
            ("\0a", "a\0"),
        ] {
            assert_eq!(
                damerau_levenshtein(a, b),
                oracle_unrestricted(a, b),
                "nul mismatch {a:?} vs {b:?}"
            );
        }
    }

    #[test]
    fn damerau_unit_many_distinct_symbols() {
        // u8, direct: all 256 byte values as source symbols, so every entry
        // of the flat last-occurrence table is written. Compared against the
        // from-scratch reference on the same units.
        let mut rng = SplitMix64(0xDA3E_A1FA_BE7A_0001);
        let source: Vec<u8> = (0..300).map(|i| (i % 256) as u8).collect();
        let target: Vec<u8> = (0..310).map(|_| (rng.next_u64() % 256) as u8).collect();
        let widen = |v: &[u8]| -> Vec<char> { v.iter().map(|&b| char::from(b)).collect() };
        assert_eq!(
            damerau_zhao_sahni(&source, &target),
            reference_damerau_units(&widen(&source), &widen(&target))
        );
        assert_eq!(
            damerau_zhao_sahni(&target, &source),
            reference_damerau_units(&widen(&target), &widen(&source))
        );

        // `char` via the public entry: >300 distinct BMP characters, so the
        // FxHashMap-backed last-occurrence map holds hundreds of entries
        // (impossible for u8).
        let wide_char = |i: usize| char::from_u32(0x400 + (i % 400) as u32).unwrap();
        let a: String = (0..350).map(wide_char).collect();
        let b: String = (0..350).map(|i| wide_char(i + 7)).collect();
        assert_eq!(
            damerau_levenshtein(&a, &b),
            oracle_unrestricted(&a, &b),
            "char wide-alphabet mismatch"
        );
        // Astral-heavy pair: whole astral scalars as recurring symbols.
        let a = "\u{1F600}\u{1F601}\u{1F602}".repeat(30);
        let b = format!(
            "\u{1F601}\u{1F600}{}",
            "\u{1F602}\u{1F601}\u{1F600}".repeat(29)
        );
        assert_eq!(
            damerau_levenshtein(&a, &b),
            oracle_unrestricted(&a, &b),
            "astral mismatch"
        );
    }

    #[test]
    fn damerau_unit_large_randomized_differential_splitmix() {
        // Fresh-seed large randomized differential for the unit-cost
        // unrestricted-Damerau fast path against the from-scratch
        // reference. Both argument orders are asserted — not because the
        // distance is asymmetric (it is not) but because the dispatch swaps
        // operands by length, so the two orders take different code paths
        // through the same kernel.
        let mut rng = SplitMix64(0xDA3E_2026_0816_DDDD);
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
            let expected = oracle_unrestricted(&a, &b);
            assert_eq!(
                damerau_levenshtein(&a, &b),
                expected,
                "round {round} ({len_a}x{len_b})"
            );
            assert_eq!(
                damerau_levenshtein(&b, &a),
                expected,
                "round {round} reversed ({len_b}x{len_a})"
            );
        }
        // A handful of `char` rounds at the same scale.
        const CYR: &[char] = &['\u{430}', '\u{431}', '\u{432}'];
        for round in 0..15 {
            let len_a = 1 + rng.next_range(150);
            let len_b = 1 + rng.next_range(150);
            let a: String = (0..len_a).map(|_| CYR[rng.next_range(CYR.len())]).collect();
            let b: String = (0..len_b).map(|_| CYR[rng.next_range(CYR.len())]).collect();
            assert_eq!(
                damerau_levenshtein(&a, &b),
                oracle_unrestricted(&a, &b),
                "char round {round}"
            );
        }
    }

    #[test]
    fn damerau_full_matrix_branch_agrees_with_the_reference() {
        // The weighted-cost fallback shares its recurrence with the unit-cost
        // kernels, so at unit costs it must produce the reference's answer
        // too — otherwise the two halves of the API would disagree the
        // moment a caller set a cost to anything but 1.0.
        let mut rng = SplitMix64(0xDA3E_FA11_BACC_0001);
        let opts = Costs::UNIT;
        for round in 0..300 {
            let alphabet: &[u8] = [&b"ab"[..], b"abc", b"abcde"][round % 3];
            let len_a = rng.next_range(40);
            let len_b = rng.next_range(40);
            let a: String = (0..len_a)
                .map(|_| alphabet[rng.next_range(alphabet.len())] as char)
                .collect();
            let b: String = (0..len_b)
                .map(|_| alphabet[rng.next_range(alphabet.len())] as char)
                .collect();
            assert_eq!(
                exact_usize(matrix_unrestricted(&a, &b, &opts)),
                oracle_unrestricted(&a, &b),
                "matrix mismatch round {round} {a:?} vs {b:?}"
            );
        }
        // Cyrillic, forcing the `char` monomorphization of the matrix branch.
        const CYR: &[char] = &['\u{430}', '\u{431}', '\u{432}'];
        for _ in 0..40 {
            let len_a = rng.next_range(30);
            let len_b = rng.next_range(30);
            let a: String = (0..len_a).map(|_| CYR[rng.next_range(CYR.len())]).collect();
            let b: String = (0..len_b).map(|_| CYR[rng.next_range(CYR.len())]).collect();
            assert_eq!(
                exact_usize(matrix_unrestricted(&a, &b, &opts)),
                oracle_unrestricted(&a, &b)
            );
        }
    }

    #[test]
    fn damerau_kernels_agree_with_the_reference_at_the_dispatch_boundary() {
        // The stack-matrix/Zhao-Sahni threshold (`DAMERAU_STACK_MAX`) swept
        // right at and across the boundary, in both argument orders, against
        // the from-scratch reference.
        let mut rng = Xorshift64(0x71E5_71E5_71E5);
        let sizes = [
            (1usize, 1usize),
            (7, 7),
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
                    damerau_levenshtein(&a, &b),
                    expected,
                    "mismatch at {a_len}x{b_len}"
                );
                assert_eq!(
                    damerau_levenshtein(&b, &a),
                    expected,
                    "reverse mismatch at {b_len}x{a_len}"
                );
            }
        }
    }

    #[test]
    fn damerau_stack_matrix_and_zhao_sahni_agree_on_the_shared_domain() {
        // Two independently-shaped evaluations of one distance: the stack
        // matrix runs the unabridged Lowrance-Wagner recurrence (every
        // earlier occurrence is a candidate, both gap terms live), while
        // Zhao-Sahni keeps only the two candidates the paper proves
        // sufficient. Wherever both apply they must agree exactly, called
        // directly — so two different bugs that each happen to satisfy the
        // reference on some random draw cannot slip through together.
        let mut rng = Xorshift64(0x3B1D_3B1D_3B1D);
        for _ in 0..600 {
            let a_len = 1 + rng.next_range(DAMERAU_STACK_MAX);
            let b_len = 1 + rng.next_range(DAMERAU_STACK_MAX);
            let a = random_units(&mut rng, a_len);
            let b = random_units(&mut rng, b_len);
            let stack = damerau_unit_small(&a, &b);
            let zhao = damerau_zhao_sahni(&a, &b);
            assert_eq!(stack, zhao, "stack/zhao at {a_len}x{b_len}");
            // The stack kernel is the unabridged recurrence, so it doubles as
            // an independent check of the reference at these sizes.
            let widen = |v: &[u8]| -> Vec<char> { v.iter().map(|&x| char::from(x)).collect() };
            assert_eq!(
                stack,
                reference_damerau_units(&widen(&a), &widen(&b)),
                "stack/reference at {a_len}x{b_len}"
            );
        }
        // Exhaustively over a two-letter alphabet up to the stack bound:
        // every string pair, no sampling.
        fn all_strings(alphabet: &[u8], max_len: usize) -> Vec<Vec<u8>> {
            let mut out = vec![Vec::new()];
            let mut frontier = vec![Vec::new()];
            for _ in 0..max_len {
                let mut next = Vec::new();
                for s in &frontier {
                    for &c in alphabet {
                        let mut v: Vec<u8> = s.clone();
                        v.push(c);
                        next.push(v);
                    }
                }
                out.extend(next.iter().cloned());
                frontier = next;
            }
            out
        }
        let all = all_strings(b"ab", 6);
        for a in &all {
            for b in &all {
                if a.is_empty() || b.is_empty() {
                    continue;
                }
                assert_eq!(
                    damerau_unit_small(a, b),
                    damerau_zhao_sahni(a, b),
                    "exhaustive stack/zhao {a:?} vs {b:?}"
                );
            }
        }
    }

    #[test]
    fn damerau_kernels_handle_the_canonical_fixtures_and_degenerate_shapes() {
        // The canonical answers forced through each kernel by direct call
        // (they are all small, so dispatch alone would only exercise the
        // stack kernel), plus degenerate shapes.
        for (a, b, want) in [
            ("bb", "abbb", 2),
            ("abbb", "bb", 2),
            ("dfcb", "bdffc", 3),
            ("aabcbbb", "cabbccaab", 5),
            ("ca", "abc", 2),
        ] {
            let ab = a.as_bytes();
            let bb = b.as_bytes();
            // The stack kernel's contract is both operands <= DAMERAU_STACK_MAX.
            if ab.len() <= DAMERAU_STACK_MAX && bb.len() <= DAMERAU_STACK_MAX {
                assert_eq!(damerau_unit_small(ab, bb), want, "small {a:?}");
            }
            assert_eq!(damerau_zhao_sahni(ab, bb), want, "zhao {a:?}");
        }
        // Single-symbol seas and disjoint alphabets across both kernels.
        for len in [8usize, 9, 60, 129, 200] {
            let aa = "a".repeat(len);
            let ab: String = "ab".chars().cycle().take(len).collect();
            let zz = "z".repeat(len + 3);
            assert_eq!(damerau_levenshtein(&aa, &ab), oracle_unrestricted(&aa, &ab));
            assert_eq!(damerau_levenshtein(&aa, &zz), oracle_unrestricted(&aa, &zz));
        }
    }

    // -- Canonicity: symmetry and from-scratch references -------------------

    /// A from-scratch transcription of the textbook optimal-string-alignment
    /// recurrence on a full `(n+1) × (m+1)` matrix — the definition `osa` is
    /// measured against, sharing no code with `osa_rows` (which rotates three
    /// rows) or with the bit-parallel kernels.
    fn reference_osa_units(a: &[char], b: &[char]) -> usize {
        let (n, m) = (a.len(), b.len());
        let w = m + 1;
        let mut h = vec![0usize; (n + 1) * w];
        for (i, cell) in h.iter_mut().take(w).enumerate() {
            *cell = i;
        }
        for i in 1..=n {
            h[i * w] = i;
        }
        for i in 1..=n {
            for j in 1..=m {
                let cost = usize::from(a[i - 1] != b[j - 1]);
                let mut best = (h[(i - 1) * w + j - 1] + cost)
                    .min(h[i * w + j - 1] + 1)
                    .min(h[(i - 1) * w + j] + 1);
                if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                    best = best.min(h[(i - 2) * w + j - 2] + 1);
                }
                h[i * w + j] = best;
            }
        }
        h[n * w + m]
    }

    /// [`reference_osa_units`] over Unicode scalars — re-derived over the
    /// contract's unit rather than re-recorded, per
    /// `docs/design/distance-contract.md` §6.
    fn oracle_osa_reference(a: &str, b: &str) -> usize {
        let ua: Vec<char> = a.chars().collect();
        let ub: Vec<char> = b.chars().collect();
        reference_osa_units(&ua, &ub)
    }

    /// A randomized corpus generator shared by the property tests below:
    /// deliberately narrow alphabets (dense matches and transpositions),
    /// mixed with Cyrillic and astral text so both monomorphizations are
    /// covered.
    fn property_pair(rng: &mut SplitMix64, round: usize) -> (String, String) {
        const CYR: &[char] = &['\u{430}', '\u{431}', '\u{432}', '\u{433}'];
        const ASTRAL: &[char] = &['\u{1F600}', '\u{1F601}', '\u{1F602}'];
        let len_a = rng.next_range(70);
        let len_b = rng.next_range(70);
        match round % 4 {
            0 | 1 => {
                let alphabet: &[u8] = if round % 2 == 0 { b"ab" } else { b"abcde" };
                let make = |rng: &mut SplitMix64, n: usize| -> String {
                    (0..n)
                        .map(|_| alphabet[rng.next_range(alphabet.len())] as char)
                        .collect()
                };
                let a = make(rng, len_a);
                let b = make(rng, len_b);
                (a, b)
            }
            2 => {
                let make = |rng: &mut SplitMix64, n: usize| -> String {
                    (0..n).map(|_| CYR[rng.next_range(CYR.len())]).collect()
                };
                let a = make(rng, len_a);
                let b = make(rng, len_b);
                (a, b)
            }
            _ => {
                let make = |rng: &mut SplitMix64, n: usize| -> String {
                    (0..n)
                        .map(|_| {
                            if rng.next_range(2) == 0 {
                                ASTRAL[rng.next_range(ASTRAL.len())]
                            } else {
                                CYR[rng.next_range(CYR.len())]
                            }
                        })
                        .collect()
                };
                let a = make(rng, len_a);
                let b = make(rng, len_b);
                (a, b)
            }
        }
    }

    // -- Plain Levenshtein: the algebraic battery ---------------------------
    //
    // `damerau_levenshtein` and `osa` each have one; `levenshtein` did not,
    // which left the crate's most-used function with no pin on the three
    // properties `docs/design/distance-contract.md` §6.1 requires of it —
    // identity, discernibility, symmetry and the triangle inequality.
    //
    // The generator below is deliberately *not* `property_pair`: the unit
    // change is the whole point of the migration, and an alphabet that never
    // leaves the Basic Multilingual Plane cannot observe it. Astral scalars
    // (one unit each, two UTF-16 code units each) and combining marks (one
    // unit each, and the case where a *rendered* character spans several
    // units) are both drawn from, so a kernel that mis-counted either would
    // break symmetry or the triangle inequality here rather than silently
    // agreeing with itself.

    /// The mixed-plane alphabet the plain-Levenshtein property tests draw
    /// from: ASCII, Latin-1, Greek, Cyrillic, CJK, combining marks and
    /// astral scalars.
    ///
    /// The combining marks are listed as ordinary members, so the generator
    /// produces sequences where a mark follows a base letter (a
    /// multi-scalar grapheme cluster), sequences where two marks stack, and
    /// sequences where a mark stands alone. All three are legal `&str`s and
    /// all three are counted the same way: one scalar, one unit
    /// (§2.6 — the unit is a scalar, not a grapheme cluster).
    const MIXED_PLANE_ALPHABET: &[char] = &[
        'a',
        'b',
        'c', // ASCII
        'é',
        'ß', // Latin-1
        'α',
        'ω', // Greek
        'б',
        'ж', // Cyrillic
        '京',
        '語',       // CJK
        '\u{0301}', // COMBINING ACUTE ACCENT
        '\u{0327}', // COMBINING CEDILLA
        '\u{093F}', // DEVANAGARI VOWEL SIGN I
        '\u{1F600}',
        '\u{1F601}', // astral: emoji
        '\u{1D573}', // astral: MATHEMATICAL BOLD FRAKTUR CAPITAL H
        '\u{10437}', // astral: DESERET SMALL LETTER YEE
    ];

    /// A random string of `len` scalars over [`MIXED_PLANE_ALPHABET`].
    fn mixed_plane_string(rng: &mut SplitMix64, len: usize) -> String {
        (0..len)
            .map(|_| MIXED_PLANE_ALPHABET[rng.next_range(MIXED_PLANE_ALPHABET.len())])
            .collect()
    }

    /// Fixtures every plain-Levenshtein property test also runs, chosen so
    /// the classes the randomized draws reach only rarely are always covered:
    /// empty operands, pure-astral operands, a base letter against its
    /// decomposed form, and NFC/NFD pairs that render identically but are
    /// different scalar sequences (§2.6 — the unit is not normalisation).
    const METRIC_FIXTURES: &[&str] = &[
        "",
        "a",
        "😀",
        "😀😁",
        "a😀b",
        "ab",
        "café",              // NFC: c a f é
        "cafe\u{0301}",      // NFD: c a f e ◌́
        "e\u{0301}",         //
        "é",                 //
        "q\u{0327}\u{0301}", // two stacked marks
        "\u{0301}",          // a lone combining mark
        "北京",
        "𝕳𝖊𝖑𝖑𝖔",
        "Москва",
    ];

    #[test]
    fn levenshtein_is_symmetric() {
        // `docs/design/distance-contract.md` §3.1: under unit costs the three
        // edit distances are symmetric, because the operation set is closed
        // under swapping the operands (an insertion into `a` is a deletion
        // from `b` and a substitution is its own inverse). The dispatch
        // relies on it — it puts the shorter operand in the bit-packed
        // pattern regardless of which argument it arrived as.
        let mut rng = SplitMix64(0x1E7A_2026_0819_1001);
        for round in 0..4000 {
            let len_a = rng.next_range(70);
            let len_b = rng.next_range(70);
            let a = mixed_plane_string(&mut rng, len_a);
            let b = mixed_plane_string(&mut rng, len_b);
            assert_eq!(
                levenshtein(&a, &b),
                levenshtein(&b, &a),
                "asymmetric at round {round}: {a:?} vs {b:?}"
            );
        }
        // Operands long enough to leave the single-word bit-parallel kernel
        // and enter the blocked one, in both argument orders.
        let mut rng = SplitMix64(0x1E7A_2026_0819_1002);
        for round in 0..200 {
            let (len_a, len_b) = (60 + rng.next_range(140), 60 + rng.next_range(140));
            let a = mixed_plane_string(&mut rng, len_a);
            let b = mixed_plane_string(&mut rng, len_b);
            assert_eq!(
                levenshtein(&a, &b),
                levenshtein(&b, &a),
                "asymmetric at long round {round}"
            );
        }
        for a in METRIC_FIXTURES {
            for b in METRIC_FIXTURES {
                assert_eq!(
                    levenshtein(a, b),
                    levenshtein(b, a),
                    "asymmetric on the fixture pair {a:?}/{b:?}"
                );
            }
        }
    }

    #[test]
    fn levenshtein_identity_holds_and_only_at_equality() {
        // Identity and discernibility together: `d(a, b) == 0` **iff**
        // `a == b`. Identity alone is satisfied by the constant-zero
        // function, so the interesting half is the converse — no two
        // distinct scalar sequences may collapse to distance 0. This is
        // where a unit that decoded lossily would show: under the old UTF-16
        // unit two *different* astral characters sharing a high surrogate
        // still differed in their low one, but any scheme that folded a
        // scalar onto a shorter representation could tie them.
        let mut rng = SplitMix64(0x1E7A_2026_0819_1003);
        for round in 0..4000 {
            let (len_a, len_b) = (rng.next_range(40), rng.next_range(40));
            let a = mixed_plane_string(&mut rng, len_a);
            let b = mixed_plane_string(&mut rng, len_b);
            let d = levenshtein(&a, &b);
            assert_eq!(
                d == 0,
                a == b,
                "round {round}: levenshtein({a:?}, {b:?}) = {d} but equality is {}",
                a == b
            );
            // Identity, asserted on its own so a failure names which half
            // broke: every string is distance 0 from itself.
            assert_eq!(levenshtein(&a, &a), 0, "identity failed for {a:?}");
            assert_eq!(levenshtein(&b, &b), 0, "identity failed for {b:?}");
        }
        for a in METRIC_FIXTURES {
            assert_eq!(levenshtein(a, a), 0, "identity failed for {a:?}");
            for b in METRIC_FIXTURES {
                assert_eq!(
                    levenshtein(a, b) == 0,
                    a == b,
                    "discernibility failed for {a:?}/{b:?}"
                );
            }
        }
        // The two spellings of "café" render identically and are four and
        // five scalars respectively, so they are one edit apart and
        // emphatically not equal — the clause §2.6 exists to state.
        assert_eq!(levenshtein("café", "cafe\u{0301}"), 2);
        assert_ne!("café", "cafe\u{0301}");
    }

    #[test]
    fn levenshtein_satisfies_the_triangle_inequality() {
        // Levenshtein is a metric: an edit script from `x` to `y` followed by
        // one from `y` to `z` is *an* edit script from `x` to `z`, and
        // `d(x, z)` is the minimum over all of them, so it can only be
        // cheaper. Narrow alphabets make near-collinear triples common,
        // which is where the bound is tight enough for a wrong answer to
        // break it.
        let mut rng = SplitMix64(0x1E7A_2026_0819_1004);
        for round in 0..4000 {
            // A deliberately narrow slice of the mixed alphabet: three
            // scalars of very different UTF-8 widths (1, 3 and 4 bytes),
            // one of which is a combining mark, so short strings collide
            // constantly and the triangle is tight.
            const NARROW: &[char] = &['a', '\u{0301}', '\u{1F600}'];
            let make = |rng: &mut SplitMix64| -> String {
                let n = rng.next_range(12);
                (0..n).map(|_| NARROW[rng.next_range(3)]).collect()
            };
            let (x, y, z) = (make(&mut rng), make(&mut rng), make(&mut rng));
            let xz = levenshtein(&x, &z);
            let xy = levenshtein(&x, &y);
            let yz = levenshtein(&y, &z);
            assert!(
                xz <= xy + yz,
                "triangle violated at round {round}: d({x:?},{z:?})={xz} > {xy} + {yz}"
            );
        }
        // The same over the wide mixed-plane alphabet and longer operands,
        // where the bound is loose but the kernels differ.
        let mut rng = SplitMix64(0x1E7A_2026_0819_1005);
        for round in 0..800 {
            let (lx, ly, lz) = (rng.next_range(50), rng.next_range(50), rng.next_range(50));
            let x = mixed_plane_string(&mut rng, lx);
            let y = mixed_plane_string(&mut rng, ly);
            let z = mixed_plane_string(&mut rng, lz);
            let (xz, xy, yz) = (
                levenshtein(&x, &z),
                levenshtein(&x, &y),
                levenshtein(&y, &z),
            );
            assert!(
                xz <= xy + yz,
                "triangle violated at wide round {round}: {xz} > {xy} + {yz}"
            );
        }
        // Every ordered triple of the fixtures, so the empty operand and the
        // pure-astral ones are always in the middle position too.
        for x in METRIC_FIXTURES {
            for y in METRIC_FIXTURES {
                for z in METRIC_FIXTURES {
                    let (xz, xy, yz) = (levenshtein(x, z), levenshtein(x, y), levenshtein(y, z));
                    assert!(
                        xz <= xy + yz,
                        "triangle violated on fixtures {x:?}/{y:?}/{z:?}: {xz} > {xy} + {yz}"
                    );
                }
            }
        }
    }

    #[test]
    fn damerau_levenshtein_is_symmetric() {
        // Unrestricted Damerau-Levenshtein is a metric, so `d(a, b)` and
        // `d(b, a)` must agree for every input — the property the recurrence
        // this replaced did NOT have (it answered 1 for ("bb", "abbb") and 2
        // for the reverse). Thousands of randomized pairs across both
        // monomorphizations, plus the historical counterexamples.
        let mut rng = SplitMix64(0x5155_2026_0819_0001);
        for round in 0..4000 {
            let (a, b) = property_pair(&mut rng, round);
            assert_eq!(
                damerau_levenshtein(&a, &b),
                damerau_levenshtein(&b, &a),
                "asymmetric at round {round}: {a:?} vs {b:?}"
            );
        }
        // Longer operands, where the dispatch swaps the operands by length
        // and the trim can change which kernel each order reaches.
        let mut rng = SplitMix64(0x5155_2026_0819_0002);
        for round in 0..300 {
            let len_a = 1 + rng.next_range(400);
            let len_b = 1 + rng.next_range(400);
            let a = random_ascii_wide(&mut rng, len_a);
            let b = if rng.next_range(2) == 0 {
                random_ascii_wide(&mut rng, len_b)
            } else {
                // Near-identical, so the affix trim fires in both directions.
                substitute_one(&a, rng.next_range(len_a))
            };
            assert_eq!(
                damerau_levenshtein(&a, &b),
                damerau_levenshtein(&b, &a),
                "asymmetric at long round {round}"
            );
        }
        for (a, b) in [
            ("bb", "abbb"),
            ("dfcb", "bdffc"),
            ("aabcbbb", "cabbccaab"),
            ("ca", "abc"),
        ] {
            assert_eq!(
                damerau_levenshtein(a, b),
                damerau_levenshtein(b, a),
                "asymmetric on the historical fixture {a:?}/{b:?}"
            );
        }
    }

    #[test]
    fn osa_is_symmetric() {
        // OSA is not a metric (it can violate the triangle inequality) but it
        // is symmetric, and the dispatch relies on that when it picks the
        // shorter operand as the bit-packed pattern.
        let mut rng = SplitMix64(0x0A5A_2026_0819_0003);
        for round in 0..4000 {
            let (a, b) = property_pair(&mut rng, round);
            assert_eq!(
                osa(&a, &b),
                osa(&b, &a),
                "asymmetric at round {round}: {a:?} vs {b:?}"
            );
        }
        let mut rng = SplitMix64(0x0A5A_2026_0819_0004);
        for round in 0..300 {
            let len_a = 1 + rng.next_range(400);
            let a = random_ascii_wide(&mut rng, len_a);
            let b = if rng.next_range(2) == 0 {
                let other_len = 1 + rng.next_range(400);
                random_ascii_wide(&mut rng, other_len)
            } else {
                delete_one(&a, rng.next_range(len_a))
            };
            assert_eq!(osa(&a, &b), osa(&b, &a), "asymmetric at long round {round}");
        }
    }

    #[test]
    fn damerau_levenshtein_agrees_with_the_from_scratch_reference_on_random_corpora() {
        // The differential that defines canonicity: the shipped function
        // against a full-matrix transcription of the published Lowrance-Wagner
        // recurrence, over thousands of randomized pairs spanning small
        // alphabets, Cyrillic and astral text.
        let mut rng = SplitMix64(0x0DA3_2026_0819_0005);
        for round in 0..4000 {
            let (a, b) = property_pair(&mut rng, round);
            assert_eq!(
                damerau_levenshtein(&a, &b),
                oracle_unrestricted(&a, &b),
                "reference mismatch at round {round}: {a:?} vs {b:?}"
            );
        }
        // Larger operands, and the near-identical shape the trim targets.
        let mut rng = SplitMix64(0x0DA3_2026_0819_0006);
        for round in 0..60 {
            let len = 100 + rng.next_range(200);
            let a = random_ascii_wide(&mut rng, len);
            let b = match round % 3 {
                0 => {
                    let other_len = len + rng.next_range(50);
                    random_ascii_wide(&mut rng, other_len)
                }
                1 => substitute_one(&a, rng.next_range(len)),
                _ => delete_one(&a, rng.next_range(len)),
            };
            assert_eq!(
                damerau_levenshtein(&a, &b),
                oracle_unrestricted(&a, &b),
                "reference mismatch at large round {round}"
            );
        }
    }

    #[test]
    fn osa_agrees_with_the_from_scratch_reference_on_random_corpora() {
        // The same differential for OSA, against its own from-scratch
        // full-matrix reference (not `osa_rows`, which the bit-parallel
        // kernels are already checked against elsewhere) — so the scalar
        // oracle itself is pinned to the textbook definition rather than
        // trusted.
        let mut rng = SplitMix64(0x0A5A_2026_0819_0007);
        for round in 0..4000 {
            let (a, b) = property_pair(&mut rng, round);
            assert_eq!(
                osa(&a, &b),
                oracle_osa_reference(&a, &b),
                "reference mismatch at round {round}: {a:?} vs {b:?}"
            );
            assert_eq!(
                oracle_osa(&a, &b),
                oracle_osa_reference(&a, &b),
                "three-row oracle mismatch at round {round}"
            );
        }
        let mut rng = SplitMix64(0x0A5A_2026_0819_0008);
        for round in 0..60 {
            let len = 100 + rng.next_range(200);
            let a = random_ascii_wide(&mut rng, len);
            let b = match round % 3 {
                0 => {
                    let other_len = len + rng.next_range(50);
                    random_ascii_wide(&mut rng, other_len)
                }
                1 => substitute_one(&a, rng.next_range(len)),
                _ => delete_one(&a, rng.next_range(len)),
            };
            assert_eq!(
                osa(&a, &b),
                oracle_osa_reference(&a, &b),
                "reference mismatch at large round {round}"
            );
        }
    }

    // -- Independent search oracle ------------------------------------------
    //
    // Search mode's *distance* has a definition that mentions no matrix at
    // all: the minimum, over every substring `t'` of the target, of
    // `distance(source, t')` under the same algorithm. The helpers below
    // evaluate exactly that, using only the crate's own **non-search**
    // distance entry points — which are themselves pinned against
    // from-scratch transcriptions of the published recurrences elsewhere in
    // this module. Nothing here reaches `full_matrix`, `search_full_matrix`
    // or `search_bits`, so a defect shared by every search routine still
    // shows up. (`oracle_search`, further down, is *not* such an oracle: it
    // calls `search_full_matrix` and exists solely to pit `search_bits`
    // against the full-matrix walk.)

    /// The public unit-cost distance function for `variant`.
    fn variant_distance(variant: Variant, a: &str, b: &str) -> usize {
        match variant {
            Variant::Plain => levenshtein(a, b),
            Variant::Damerau => damerau_levenshtein(a, b),
            Variant::Osa => osa(a, b),
        }
    }

    /// The public unit-cost search function for `variant`.
    fn variant_search<'t>(variant: Variant, a: &str, b: &'t str) -> SearchResult<'t, usize> {
        match variant {
            Variant::Plain => levenshtein_search(a, b),
            Variant::Damerau => damerau_levenshtein_search(a, b),
            Variant::Osa => osa_search(a, b),
        }
    }

    /// One admissible cost set, in whichever of the three public shapes a
    /// variant needs.
    ///
    /// The weighted sweeps run every variant over the same four numbers, and
    /// each variant takes a different type — deliberately, so a transposition
    /// cost cannot reach a function that would discard it. The grid therefore
    /// stores the numbers and converts per call.
    #[derive(Debug, Clone, Copy)]
    struct CostSet {
        insertion: f64,
        deletion: f64,
        substitution: f64,
        transposition: f64,
    }

    impl CostSet {
        #[track_caller]
        fn levenshtein(self) -> LevenshteinCosts {
            LevenshteinCosts::new(self.insertion, self.deletion, self.substitution)
                .expect("admissible")
        }

        #[track_caller]
        fn osa(self) -> OsaCosts {
            OsaCosts::new(
                self.insertion,
                self.deletion,
                self.substitution,
                self.transposition,
            )
            .expect("admissible")
        }

        #[track_caller]
        fn damerau(self) -> DamerauCosts {
            DamerauCosts::new(
                self.insertion,
                self.deletion,
                self.substitution,
                self.transposition,
            )
            .expect("admissible, including the Lowrance-Wagner threshold")
        }

        /// The internal four-field form, for the row/matrix oracles.
        fn costs(self) -> Costs {
            Costs {
                insertion: self.insertion,
                deletion: self.deletion,
                substitution: self.substitution,
                transposition: self.transposition,
            }
        }
    }

    /// The public weighted distance function for `variant`.
    fn variant_distance_weighted(variant: Variant, a: &str, b: &str, set: CostSet) -> f64 {
        match variant {
            Variant::Plain => levenshtein_weighted(a, b, &set.levenshtein()),
            Variant::Damerau => damerau_levenshtein_weighted(a, b, &set.damerau()),
            Variant::Osa => osa_weighted(a, b, &set.osa()),
        }
    }

    /// The public weighted search function for `variant`.
    fn variant_search_weighted<'t>(
        variant: Variant,
        a: &str,
        b: &'t str,
        set: CostSet,
    ) -> SearchResult<'t, f64> {
        match variant {
            Variant::Plain => levenshtein_search_weighted(a, b, &set.levenshtein()),
            Variant::Damerau => damerau_levenshtein_search_weighted(a, b, &set.damerau()),
            Variant::Osa => osa_search_weighted(a, b, &set.osa()),
        }
    }

    /// Every substring of `target`, borrowed.
    ///
    /// Enumerated over *byte* boundaries that are also character boundaries,
    /// which under the scalar unit is every unit boundary there is — no
    /// operand is excluded, astral ones included, because a scalar span
    /// always has a byte span. This shares no code with the search routines:
    /// it never sees a matrix, a kernel or `dispatch`.
    fn all_substrings(target: &str) -> Vec<&str> {
        let bounds: Vec<usize> = target
            .char_indices()
            .map(|(b, _)| b)
            .chain(std::iter::once(target.len()))
            .collect();
        let mut out = Vec::new();
        for (i, &start) in bounds.iter().enumerate() {
            for &end in &bounds[i..] {
                out.push(&target[start..end]);
            }
        }
        out
    }

    /// `min over every substring t' of target of distance(source, t')`, unit
    /// costs.
    fn brute_force_search_distance(source: &str, target: &str, variant: Variant) -> usize {
        all_substrings(target)
            .into_iter()
            .map(|sub| variant_distance(variant, source, sub))
            .min()
            .expect("the empty substring is always a candidate")
    }

    /// [`brute_force_search_distance`] under a weighted cost set.
    fn brute_force_search_distance_weighted(
        source: &str,
        target: &str,
        set: CostSet,
        variant: Variant,
    ) -> f64 {
        let mut best = f64::INFINITY;
        for sub in all_substrings(target) {
            best = best.min(variant_distance_weighted(variant, source, sub, set));
        }
        best
    }

    /// The first guarantee of `docs/design/distance-contract.md` §3.2: the
    /// reported range is a **byte** range that slices the target, and slicing
    /// it reproduces the reported text.
    ///
    /// Plus the separately-stated "no fabrication" clause — the substring
    /// occurs in the target *somewhere* — which is implied by the first but
    /// asserted on its own because its violation was invisible for so long:
    /// `String::from_utf16_lossy` over a span starting or ending between the
    /// halves of a surrogate pair returned U+FFFD, a character that does not
    /// occur in the target at all.
    #[track_caller]
    fn assert_range_slices_the_target<D: Copy>(
        target: &str,
        got: &SearchResult<'_, D>,
        variant: Variant,
    ) {
        let range = got.range();
        assert!(
            target.is_char_boundary(range.start) && target.is_char_boundary(range.end),
            "{variant:?} range {range:?} is not a character boundary of {target:?}"
        );
        assert_eq!(
            &target[range.clone()],
            got.substring(),
            "{variant:?} range {range:?} does not slice to {:?} in {target:?}",
            got.substring()
        );
        assert!(
            target.contains(got.substring()),
            "{variant:?} substring {:?} does not occur in {target:?}",
            got.substring()
        );
    }

    /// Checks a unit-cost `_search` call against the brute-force definition,
    /// and additionally that the substring it reports is a real one: it must
    /// sit at the reported offset in the target and, on its own, realise the
    /// reported distance. Both extra checks use only the non-search distance
    /// functions, so they pin `substring`/`offset` without consulting the
    /// matrix that produced them.
    fn assert_search_matches_brute_force(source: &str, target: &str, variant: Variant) {
        let got = variant_search(variant, source, target);
        let want = brute_force_search_distance(source, target, variant);
        assert_eq!(
            got.distance(),
            want,
            "{variant:?} search distance {} != best substring distance {want} \
             for {source:?} in {target:?}",
            got.distance()
        );
        assert_eq!(
            variant_distance(variant, source, got.substring()),
            got.distance(),
            "{variant:?} reported substring {:?} does not realise distance {} \
             for {source:?} in {target:?}",
            got.substring(),
            got.distance()
        );
        assert_range_slices_the_target(target, &got, variant);
    }

    /// [`assert_search_matches_brute_force`] under a weighted cost set.
    fn assert_weighted_search_matches_brute_force(
        source: &str,
        target: &str,
        set: CostSet,
        variant: Variant,
    ) {
        let got = variant_search_weighted(variant, source, target, set);
        let want = brute_force_search_distance_weighted(source, target, set, variant);
        assert_eq!(
            got.distance(),
            want,
            "{variant:?} search distance {} != best substring distance {want} \
             for {source:?} in {target:?} with {set:?}",
            got.distance()
        );
        assert_eq!(
            variant_distance_weighted(variant, source, got.substring(), set),
            got.distance(),
            "{variant:?} reported substring {:?} does not realise distance {} \
             for {source:?} in {target:?} with {set:?}",
            got.substring(),
            got.distance()
        );
        assert_range_slices_the_target(target, &got, variant);
    }

    /// Cost sets the weighted search differential sweeps.
    ///
    /// Every one satisfies `2 * transposition >= insertion + deletion`, since
    /// [`DamerauCosts::new`] refuses to build anything else; the first four
    /// hold it with equality and the last two with slack. `(5, 5, 5, 5)` is
    /// the set that exposed the `(n + m)` seed — every cell of the matrix
    /// exceeds it.
    const SEARCH_COST_GRID: &[CostSet] = &[
        CostSet {
            insertion: 1.0,
            deletion: 1.0,
            substitution: 1.0,
            transposition: 1.0,
        },
        CostSet {
            insertion: 5.0,
            deletion: 5.0,
            substitution: 5.0,
            transposition: 5.0,
        },
        CostSet {
            insertion: 3.0,
            deletion: 1.0,
            substitution: 2.0,
            transposition: 2.0,
        },
        CostSet {
            insertion: 0.5,
            deletion: 2.0,
            substitution: 1.5,
            transposition: 1.25,
        },
        CostSet {
            insertion: 1.0,
            deletion: 2.0,
            substitution: 4.0,
            transposition: 5.0,
        },
        CostSet {
            insertion: 0.0,
            deletion: 1.0,
            substitution: 1.0,
            transposition: 1.0,
        },
    ];

    const ALL_VARIANTS: [Variant; 3] = [Variant::Plain, Variant::Damerau, Variant::Osa];

    // -- The tie-break rule, pinned independently ---------------------------
    //
    // `docs/design/distance-contract.md` §3.2's third guarantee is the one
    // that had no independent pin: `search_tie_breaking_pinned_examples`
    // compares against `oracle_search_unit`, which *calls*
    // `search_full_matrix`, so it can only catch a divergence between the
    // bit-parallel path and the matrix path — never a tie-break that is
    // wrong in both. §6.2 asks for a brute force "that shares no code with
    // the search routines", and this is it.
    //
    // The rule, restated so it can be evaluated without a matrix:
    //
    //   * the reported distance is the minimum, over every substring of the
    //     target, of `metric(source, substring)`;
    //   * the reported **end** is the earliest scalar position at which some
    //     substring attains that minimum — column 0, the empty substring,
    //     being the earliest of all, which is where "the empty substring
    //     ahead of all of them" comes from;
    //   * the reported **start** is fixed by the parent chain, whose
    //     candidate order is insert → delete → substitute → transpose with
    //     the first minimum winning.
    //
    // The first two clauses are properties of the *substring set* and are
    // enumerated directly below. The third is a property of the alignment,
    // and a greedy backtrack that takes the first optimal predecessor in a
    // fixed order is exactly "the lexicographically least reversed operation
    // sequence among optimal alignments" — so it is reproduced here from the
    // published candidate order, over a cost table that is itself built by
    // substring enumeration rather than by any recurrence.

    /// Byte offsets of every scalar boundary of `s`, plus `s.len()`.
    ///
    /// Index `k` is the byte offset of scalar `k`, so `bounds[i]..bounds[j]`
    /// is the byte range of scalars `i..j`. Under the scalar unit every one
    /// of these is a valid `&str` boundary, which is the whole reason
    /// `SearchResult::range` can exist (§2.2).
    fn scalar_bounds(s: &str) -> Vec<usize> {
        s.char_indices()
            .map(|(b, _)| b)
            .chain(std::iter::once(s.len()))
            .collect()
    }

    /// The search cost table `D[r][c]`, evaluated **by definition** rather
    /// than by a recurrence.
    ///
    /// Search mode makes row 0 free, which is precisely the statement that a
    /// match may begin at any scalar of the target. So
    ///
    /// ```text
    /// D[r][c] = min over 0 <= s <= c of metric(source[..r], target[s..c])
    /// ```
    ///
    /// — the same reading [`brute_force_search_distance`] already relies on
    /// at `r == n`, extended to every row so the parent chain can be
    /// evaluated on it. Every entry is produced by enumerating substrings and
    /// calling the crate's public **non-search** distance function, so
    /// nothing here reaches `full_matrix`'s search mode, `search_full_matrix`,
    /// `search_bits`, `search_forward_word` or `search_forward_blocks`.
    fn brute_force_search_table(source: &str, target: &str, variant: Variant) -> Vec<Vec<usize>> {
        let sb = scalar_bounds(source);
        let tb = scalar_bounds(target);
        let n = sb.len() - 1;
        let m = tb.len() - 1;
        let mut table = vec![vec![0usize; m + 1]; n + 1];
        for (r, row) in table.iter_mut().enumerate() {
            let prefix = &source[..sb[r]];
            for (c, cell) in row.iter_mut().enumerate() {
                let mut best = usize::MAX;
                for s in 0..=c {
                    best = best.min(variant_distance(variant, prefix, &target[tb[s]..tb[c]]));
                }
                *cell = best;
            }
        }
        table
    }

    /// [`brute_force_search_table`] under a weighted cost set.
    fn brute_force_search_table_weighted(
        source: &str,
        target: &str,
        set: CostSet,
        variant: Variant,
    ) -> Vec<Vec<f64>> {
        let sb = scalar_bounds(source);
        let tb = scalar_bounds(target);
        let n = sb.len() - 1;
        let m = tb.len() - 1;
        let mut table = vec![vec![0.0f64; m + 1]; n + 1];
        for (r, row) in table.iter_mut().enumerate() {
            let prefix = &source[..sb[r]];
            for (c, cell) in row.iter_mut().enumerate() {
                let mut best = f64::INFINITY;
                for s in 0..=c {
                    let d = variant_distance_weighted(variant, prefix, &target[tb[s]..tb[c]], set);
                    if d < best {
                        best = d;
                    }
                }
                *cell = best;
            }
        }
        table
    }

    /// The `(start, end)` scalar positions §3.2's tie-break rule names,
    /// given the search cost table and the operands as scalar slices.
    ///
    /// `cost(r, c)` reads the table; `edit(k)` is the price of one operation
    /// (`Operation::Insertion` &c.) so the same walk serves both tiers. The
    /// candidate order and the "first minimum wins" rule are transcribed from
    /// the published recurrences — Levenshtein (1966) for insert/delete/
    /// substitute, the optimal-string-alignment recurrence for OSA's
    /// `(r - 2, c - 2)` transposition, and Lowrance & Wagner (1975) for
    /// unrestricted Damerau's `(lrm - 1, lcm - 1)` transposition with its two
    /// gap terms — not copied from the implementation.
    fn tie_break_walk(
        src: &[char],
        tgt: &[char],
        variant: Variant,
        cost: &dyn Fn(usize, usize) -> f64,
        edit: &dyn Fn(Operation) -> f64,
    ) -> (usize, usize) {
        let n = src.len();
        let m = tgt.len();

        // The last row's minimum, scanned from column 0 so the earliest end
        // column — and the empty substring ahead of every other candidate —
        // wins a tie.
        let mut end = 0usize;
        let mut best = cost(n, 0);
        for c in 1..=m {
            if cost(n, c) < best {
                best = cost(n, c);
                end = c;
            }
        }

        // Walk parents back to the free row 0 (or to column 0, from which
        // the chain is pure deletions to the origin).
        let (mut r, mut c) = (n, end);
        while r > 0 && c > 0 {
            let here = cost(r, c);
            // 1. Insertion: the target scalar at `c` is spent unmatched.
            if cost(r, c - 1) + edit(Operation::Insertion) == here {
                c -= 1;
                continue;
            }
            // 2. Deletion: the source scalar at `r` is spent unmatched.
            if cost(r - 1, c) + edit(Operation::Deletion) == here {
                r -= 1;
                continue;
            }
            // 3. Substitution, free when the two scalars are equal.
            let sub = if src[r - 1] == tgt[c - 1] {
                cost(r - 1, c - 1)
            } else {
                cost(r - 1, c - 1) + edit(Operation::Substitution)
            };
            if sub == here {
                r -= 1;
                c -= 1;
                continue;
            }
            // 4. Transposition, in whichever form this variant defines.
            match variant {
                Variant::Plain => {}
                Variant::Osa => {
                    if r > 1 && c > 1 && src[r - 1] == tgt[c - 2] && src[r - 2] == tgt[c - 1] {
                        let t = cost(r - 2, c - 2) + edit(Operation::Transposition);
                        if t == here {
                            r -= 2;
                            c -= 2;
                            continue;
                        }
                    }
                }
                Variant::Damerau => {
                    // Lowrance & Wagner's candidate: the last row above `r`
                    // whose source scalar equals `tgt[c - 1]`, the last
                    // column left of `c` whose target scalar equals
                    // `src[r - 1]`, the two gaps between them cleared by
                    // plain deletions and insertions, and one transposition.
                    let lrm = (1..r).rev().find(|&x| src[x - 1] == tgt[c - 1]);
                    let lcm = (1..c).rev().find(|&y| tgt[y - 1] == src[r - 1]);
                    if let (Some(lrm), Some(lcm)) = (lrm, lcm) {
                        let t = cost(lrm - 1, lcm - 1)
                            + (r - lrm - 1) as f64 * edit(Operation::Deletion)
                            + (c - lcm - 1) as f64 * edit(Operation::Insertion)
                            + edit(Operation::Transposition);
                        if t == here {
                            r = lrm - 1;
                            c = lcm - 1;
                            continue;
                        }
                    }
                }
            }
            panic!(
                "no candidate reproduces D[{r}][{c}] = {here} for {variant:?}: the search \
                 recurrence and this transcription of it disagree"
            );
        }
        (c, end)
    }

    /// Asserts a unit-cost `_search` call returns exactly the substring the
    /// tie-break rule names, both positions derived from the substring
    /// enumeration alone.
    #[track_caller]
    fn assert_search_matches_the_tie_break_rule(source: &str, target: &str, variant: Variant) {
        let table = brute_force_search_table(source, target, variant);
        let src: Vec<char> = source.chars().collect();
        let tgt: Vec<char> = target.chars().collect();
        let (start, end) = tie_break_walk(
            &src,
            &tgt,
            variant,
            &|r, c| table[r][c] as f64,
            // Unit costs: every operation is priced 1.
            &|_| 1.0,
        );
        let tb = scalar_bounds(target);
        let got = variant_search(variant, source, target);
        assert_eq!(
            got.distance(),
            table[src.len()][end],
            "{variant:?} distance for {source:?} in {target:?}"
        );
        assert_eq!(
            got.range(),
            tb[start]..tb[end],
            "{variant:?} tie-break range for {source:?} in {target:?}: expected scalars \
             {start}..{end}, got substring {:?}",
            got.substring()
        );
        assert_eq!(
            got.substring(),
            &target[tb[start]..tb[end]],
            "{variant:?} tie-break substring for {source:?} in {target:?}"
        );
    }

    /// [`assert_search_matches_the_tie_break_rule`] under a weighted cost
    /// set.
    ///
    /// Every cost in [`SEARCH_COST_GRID`] is a dyadic rational and the
    /// operands here are short, so every sum below is exact in `f64` and the
    /// `==` comparisons in [`tie_break_walk`] are the right ones.
    #[track_caller]
    fn assert_weighted_search_matches_the_tie_break_rule(
        source: &str,
        target: &str,
        set: CostSet,
        variant: Variant,
    ) {
        let table = brute_force_search_table_weighted(source, target, set, variant);
        let src: Vec<char> = source.chars().collect();
        let tgt: Vec<char> = target.chars().collect();
        let (start, end) =
            tie_break_walk(&src, &tgt, variant, &|r, c| table[r][c], &|op| match op {
                Operation::Insertion => set.insertion,
                Operation::Deletion => set.deletion,
                Operation::Substitution => set.substitution,
                Operation::Transposition => set.transposition,
            });
        let tb = scalar_bounds(target);
        let got = variant_search_weighted(variant, source, target, set);
        assert_eq!(
            got.distance(),
            table[src.len()][end],
            "{variant:?} distance for {source:?} in {target:?} with {set:?}"
        );
        assert_eq!(
            got.range(),
            tb[start]..tb[end],
            "{variant:?} tie-break range for {source:?} in {target:?} with {set:?}: expected \
             scalars {start}..{end}, got substring {:?}",
            got.substring()
        );
    }

    /// Inputs where *every* end position ties, so the tie-break is the only
    /// thing deciding the answer, plus the non-ASCII and astral shapes where
    /// a scalar position and a byte position are different numbers.
    const TIE_BREAK_CORPUS: &[(&str, &str)] = &[
        // Degenerate repeats: every column of the last row holds the same
        // cost, so the earliest-end rule alone picks the answer.
        ("aaa", "aaaaaa"),
        ("aa", "aa"),
        ("a", "aaaa"),
        ("ab", "ababab"),
        ("aba", "bab"),
        ("ca", "abc"),
        ("b", "aaa"),
        ("abc", "xyz"),
        ("ab", "aab"),
        ("ab", "aXb"),
        ("adccb", "cdbb"),
        // Where the *substitute-after-delete* half of the order is
        // observable at unit costs: at the winning end column the deletion
        // and the substitution parents tie, and preferring the substitution
        // would walk one column further left and report `"ba"` where the
        // published order reports `"a"`.
        ("aa", "ba"),
        ("bb", "ab"),
        ("aa", "bba"),
        ("bb", "aab"),
        ("aa", "bab"),
        // Empty operands, where column 0 is the whole answer.
        ("", ""),
        ("", "abc"),
        ("abc", ""),
        ("a", ""),
        ("", "a"),
        // Non-ASCII: scalar index and byte index diverge.
        ("é", "ééé"),
        ("б", "аббаб"),
        ("京", "北京京南"),
        ("สวัสดี", "สวัสดีสวัสดี"),
        // Astral: two UTF-16 code units, one scalar, four bytes.
        ("😀", "😀😀😀"),
        ("😀", "ab😀cd"),
        ("😀😁", "😀😁😀😁"),
        ("a😀", "😀a😀a"),
        ("𝕳", "𝕳𝕳"),
        ("😀x", "x😀x😀"),
    ];

    #[test]
    fn search_tie_break_matches_an_independent_substring_brute_force() {
        // The pin §6.2 asks for and `search_tie_breaking_pinned_examples`
        // cannot provide: the expected `(substring, range, distance)` comes
        // from enumerating every substring of the target, scoring each with
        // the crate's non-search distance function, and applying §3.2's
        // tie-break rule to the resulting table — no matrix, no kernel, no
        // `dispatch`.
        for &(source, target) in TIE_BREAK_CORPUS {
            for variant in ALL_VARIANTS {
                assert_search_matches_the_tie_break_rule(source, target, variant);
            }
        }

        // Randomized small operands over narrow alphabets, where exact ties
        // between competing end columns are the common case rather than the
        // exception. Deliberately tiny: the brute force is O(n · m²) distance
        // calls and its whole value is that it is written the slow, obvious
        // way.
        let mut rng = SplitMix64(0x71E8_2026_0819_0001);
        for round in 0..500 {
            let (a, b) = tie_break_pair(&mut rng, round);
            for variant in ALL_VARIANTS {
                assert_search_matches_the_tie_break_rule(&a, &b, variant);
            }
        }
    }

    #[test]
    fn search_tie_break_matches_the_brute_force_under_weighted_costs() {
        // The same rule under prices that make delete, insert and substitute
        // genuinely different, including the asymmetric sets where the
        // cheapest script is not the shortest one and the `insertion: 0.0`
        // set, where extending the match leftward is free and the tie-break
        // is all that bounds the answer.
        for &(source, target) in TIE_BREAK_CORPUS {
            for variant in ALL_VARIANTS {
                for &set in SEARCH_COST_GRID {
                    assert_weighted_search_matches_the_tie_break_rule(source, target, set, variant);
                }
            }
        }

        let mut rng = SplitMix64(0x71E8_2026_0819_0002);
        for round in 0..120 {
            let (a, b) = tie_break_pair(&mut rng, round);
            for variant in ALL_VARIANTS {
                for &set in SEARCH_COST_GRID {
                    assert_weighted_search_matches_the_tie_break_rule(&a, &b, set, variant);
                }
            }
        }
    }

    /// Short random pairs for the tie-break sweeps: narrow alphabets so ties
    /// are dense, cycling through ASCII, Cyrillic and astral so the byte
    /// derivation is exercised in all three widths.
    fn tie_break_pair(rng: &mut SplitMix64, round: usize) -> (String, String) {
        const ALPHABETS: [&[char]; 4] = [
            &['a', 'b'],
            &['a', 'b', 'c'],
            &['а', 'б'],
            &['\u{1F600}', '\u{1F601}', 'a'],
        ];
        let alphabet = ALPHABETS[round % ALPHABETS.len()];
        let n = rng.next_range(5);
        let m = rng.next_range(7);
        let make = |k: usize, rng: &mut SplitMix64| -> String {
            (0..k)
                .map(|_| alphabet[rng.next_range(alphabet.len())])
                .collect()
        };
        let a = make(n, rng);
        let b = make(m, rng);
        (a, b)
    }

    // -- The BMP battery: the gap that let the offset defect survive --------

    #[test]
    fn search_reports_a_byte_range_that_slices_the_target() {
        // `docs/design/distance-contract.md` §2.5, tier 1 — the widest
        // defect, and not an astral one. The search used to report a
        // *UTF-16* offset while its own rustdoc documented it as a `&str`
        // index. One umlaut is enough to make the two disagree, and byte 8
        // of this target happens to be a character boundary, so the wrong
        // answer sliced cleanly to `" Berlin, Wien"` instead of panicking.
        //
        // The expected value is counted from the UTF-8 encoding, not read
        // off the implementation: "Zürich, " is Z(1) ü(2) r(1) i(1) c(1)
        // h(1) ,(1) ␠(1) = 9 bytes, and "Berlin" is 6.
        let target = "Zürich, Berlin, Wien";
        assert_eq!(target.char_indices().nth(8).map(|(b, _)| b), Some(9));
        for variant in ALL_VARIANTS {
            let got = variant_search(variant, "Berlin", target);
            assert_eq!(got.range(), 9..15, "{variant:?}");
            assert_eq!(got.substring(), "Berlin", "{variant:?}");
            assert_eq!(got.distance(), 0, "{variant:?}");
            assert_eq!(&target[got.range()], "Berlin", "{variant:?}");
        }

        // Tier 2 — targets where the old UTF-16 offset landed *mid*
        // character, so the documented `&target[offset..]` panicked outright.
        // Every one of these is now a valid slice by construction.
        for (source, target) in [
            ("x", "caféx"),
            ("b", "北京b"),
            ("z", "한국z"),
            ("w", "مرحباw"),
            ("ก", "สวัสดีก"),
            ("β", "αβγδε"),
            ("б", "абвгд"),
        ] {
            for variant in ALL_VARIANTS {
                let got = variant_search(variant, source, target);
                assert_range_slices_the_target(target, &got, variant);
                assert_eq!(
                    got.substring(),
                    source,
                    "{variant:?} exact occurrence of {source:?} in {target:?}"
                );
                assert_eq!(got.distance(), 0, "{variant:?} {source:?} in {target:?}");
            }
        }
    }

    #[test]
    fn search_invariants_hold_across_scripts_and_cost_sets() {
        // The corpus the external-contract test never had: Latin-1 through
        // CJK, plus astral. Both §3.2 invariants — the range slices to the
        // reported text, and the reported text alone realises the reported
        // distance — for every variant, both tiers, and every cost set in
        // the grid, against the substring brute force that shares no code
        // with the search routines.
        const CORPUS: &[(&str, &str)] = &[
            // Latin-1 / accented Latin
            ("Berlin", "Zürich, Berlin, Wien"),
            ("cafe", "un café noir"),
            ("naive", "the naïve approach"),
            // Greek
            ("κόσμε", "γειά σου κόσμε"),
            ("αβγ", "ζαβγη"),
            // Cyrillic
            ("Москва", "город Москва зимой"),
            ("бв", "абвгд"),
            // Thai (three bytes per scalar)
            ("สวัสดี", "พูดว่า สวัสดี ครับ"),
            // Hangul
            ("한국", "대한민국 한국어"),
            // Arabic (right-to-left, two bytes per scalar)
            ("مرحبا", "قال مرحبا بالعالم"),
            // CJK
            ("北京", "从上海到北京的路"),
            ("日本語", "これは日本語です"),
            // Astral: emoji, mathematical alphanumerics, an old script
            ("😀", "ab😀cd"),
            ("😀😁", "x😀😁y"),
            ("𝕳𝖊", "q𝕳𝖊𝖑𝖑𝖔r"),
            ("𐐷", "a𐐷b"),
            // Mixed planes, and near-misses rather than exact occurrences
            ("a😀b", "xxa😁bxx"),
            ("Berlin", "Zürich, Berlim, Wien"),
            ("北京", "南京市"),
            ("😀x", "y😁x"),
            // Degenerate ends
            ("", "über"),
            ("über", ""),
            ("é", "é"),
        ];

        for &(source, target) in CORPUS {
            for variant in ALL_VARIANTS {
                assert_search_matches_brute_force(source, target, variant);
                for &set in SEARCH_COST_GRID {
                    assert_weighted_search_matches_brute_force(source, target, set, variant);
                }
            }
        }
    }

    #[test]
    fn search_never_fabricates_text_absent_from_the_target() {
        // The property whose violation was invisible for so long: with
        // `substitution: 0.25`, searching "X" in "😀ab" used to return
        // `substring: "\u{FFFD}"` at offset 0 — a character that does not
        // occur in the target at all — because the UTF-16 span began or
        // ended between the halves of a surrogate pair and
        // `String::from_utf16_lossy` substituted U+FFFD for the orphan.
        // Unit costs hid it, because the mid-pair alignment tied with one
        // that avoided it.
        //
        // This asserts the *guarantee*, not the old symptom: whatever the
        // search returns must occur in the target at the reported range.
        // Under the scalar unit the whole class is unrepresentable.
        let cheap_substitution = CostSet {
            insertion: 1.0,
            deletion: 1.0,
            substitution: 0.25,
            transposition: 1.0,
        };
        let sources = ["X", "😀", "a", "ab", "😀a", "x😀", "", "𝕳"];
        let targets = [
            "😀ab",
            "a😀b",
            "ab😀",
            "😀",
            "😀😁",
            "𝕳𝖊𝖑𝖑𝖔",
            "",
            "a",
            "é😀",
            "😀é",
        ];
        for source in sources {
            for target in targets {
                for variant in ALL_VARIANTS {
                    let got = variant_search(variant, source, target);
                    assert_range_slices_the_target(target, &got, variant);

                    let got = variant_search_weighted(variant, source, target, cheap_substitution);
                    assert_range_slices_the_target(target, &got, variant);
                    // And the substring genuinely realises the distance, so
                    // "returns a real substring" cannot be met by returning
                    // an arbitrary one.
                    assert_eq!(
                        variant_distance_weighted(
                            variant,
                            source,
                            got.substring(),
                            cheap_substitution
                        ),
                        got.distance(),
                        "{variant:?} {source:?} in {target:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn search_distance_equals_the_best_substring_distance_for_every_variant() {
        // The unit-cost differential, against the independent brute force.
        let mut rng = SplitMix64(0x5EA2_2026_0819_000B);
        for round in 0..600 {
            let alphabet: &[u8] = [&b"ab"[..], b"abc"][round % 2];
            let n = 1 + rng.next_range(6);
            let m = 1 + rng.next_range(9);
            let make = |rng: &mut SplitMix64, k: usize| -> String {
                (0..k)
                    .map(|_| alphabet[rng.next_range(alphabet.len())] as char)
                    .collect()
            };
            let s = make(&mut rng, n);
            let t = make(&mut rng, m);
            for variant in ALL_VARIANTS {
                assert_search_matches_brute_force(&s, &t, variant);
            }
        }
    }

    #[test]
    fn search_distance_matches_the_brute_force_oracle_under_weighted_costs() {
        // The differential that catches D1: `search_full_matrix` used to seed
        // its last-row minimum with `(n + m)`, an upper bound only when every
        // cost is 1.0. Raise any cost above 1 and no cell of the matrix ever
        // clears the seed, so the scan never fires and the returned distance,
        // substring and offset are all fabricated. A weighted cost grid over
        // small operands finds it immediately; before the fix this failed on
        // 406 of 930 cases at the all-5.0 cost set.
        let mut rng = SplitMix64(0x5EA2_2026_0819_0F01);
        for round in 0..300 {
            let alphabet: &[u8] = [&b"ab"[..], b"abc", b"abcd"][round % 3];
            let make = |rng: &mut SplitMix64, k: usize| -> String {
                (0..k)
                    .map(|_| alphabet[rng.next_range(alphabet.len())] as char)
                    .collect()
            };
            let (n, m) = (rng.next_range(6), rng.next_range(8));
            let s = make(&mut rng, n);
            let t = make(&mut rng, m);
            for &set in SEARCH_COST_GRID {
                for variant in ALL_VARIANTS {
                    assert_weighted_search_matches_brute_force(&s, &t, set, variant);
                }
            }
        }
        // The same sweep through the non-ASCII (`Operands::Units`) branch.
        const CYRILLIC: &[char] = &['а', 'б', 'в'];
        let mut rng = SplitMix64(0x5EA2_2026_0819_0F02);
        for _ in 0..60 {
            let make = |rng: &mut SplitMix64, k: usize| -> String {
                (0..k).map(|_| CYRILLIC[rng.next_range(3)]).collect()
            };
            let (n, m) = (rng.next_range(5), rng.next_range(7));
            let s = make(&mut rng, n);
            let t = make(&mut rng, m);
            for &set in SEARCH_COST_GRID {
                for variant in ALL_VARIANTS {
                    assert_weighted_search_matches_brute_force(&s, &t, set, variant);
                }
            }
        }
    }

    #[test]
    fn search_handles_empty_operands_on_either_side() {
        // Empty operands are excluded from the bit-parallel path and were
        // excluded from every search differential too, so the full-matrix
        // walk's handling of them was unpinned. `("a", "")` with
        // `deletion: 3.0` is the measured case: the old seed `(n + m)`
        // was 1, below the true answer 3, so the scan never fired and the
        // function reported 1.
        for &set in SEARCH_COST_GRID {
            for variant in ALL_VARIANTS {
                for (s, t) in [
                    ("", ""),
                    ("", "abc"),
                    ("abc", ""),
                    ("a", ""),
                    ("", "a"),
                    ("ab", ""),
                    ("", "ba"),
                ] {
                    assert_weighted_search_matches_brute_force(s, t, set, variant);
                    assert_search_matches_brute_force(s, t, variant);
                }
            }
        }

        // The degenerate table of `docs/design/distance-contract.md` §3.2,
        // asserted directly for all three unit-cost variants.
        for variant in ALL_VARIANTS {
            for (source, target, distance) in [("abc", "", 3usize), ("", "abc", 0), ("", "", 0)] {
                let got = variant_search(variant, source, target);
                assert_eq!(got.substring(), "", "{variant:?} {source:?} in {target:?}");
                assert_eq!(got.range(), 0..0, "{variant:?} {source:?} in {target:?}");
                assert_eq!(
                    got.distance(),
                    distance,
                    "{variant:?} {source:?} in {target:?}"
                );
            }
        }
    }

    #[test]
    fn search_weighted_cost_regression_fixtures() {
        // The two cases the audit measured, pinned as literals computed from
        // the definition rather than recorded from the implementation.
        let five = lev_costs(5.0, 5.0, 5.0);
        // Every substring of "xyz" is 15.0 away from "abc" under these costs
        // (three deletions, or k substitutions plus 3 - k deletions), so the
        // first minimum — the empty substring at column 0 — wins the tie.
        // Before the fix: substring "z", distance 6, offset 2, none of which
        // is a cell of the matrix.
        let r = levenshtein_search_weighted("abc", "xyz", &five);
        assert_eq!(r.distance(), 15.0);
        assert_eq!(r.substring(), "");
        assert_eq!(r.range(), 0..0);

        // One deletion at cost 3, against an empty target: there is exactly
        // one substring to choose from. Before the fix: 1.0.
        let del3 = lev_costs(1.0, 3.0, 1.0);
        let r = levenshtein_search_weighted("a", "", &del3);
        assert_eq!(r.distance(), 3.0);
        assert_eq!(r.substring(), "");
        assert_eq!(r.range(), 0..0);

        // Weighted costs where the answer is genuinely inside the target, so
        // the fix cannot be "always return column 0".
        let r = levenshtein_search_weighted("bcd", "axbcdxa", &five);
        assert_eq!(r.distance(), 0.0);
        assert_eq!(r.substring(), "bcd");
        assert_eq!(r.range(), 2..5);
    }

    // -- Lowrance-Wagner cost precondition (D2) -----------------------------

    /// The cost set the audit measured `d("aab", "baa") = 2` under, where a
    /// Dijkstra search over edit scripts finds `1.998` (two transpositions).
    /// `2 * 0.999 < 1.0 + 1.0`, so it is below the threshold.
    const SUB_THRESHOLD_CHEAP_SWAP: CostSet = CostSet {
        insertion: 1.0,
        deletion: 1.0,
        substitution: 5.0,
        transposition: 0.999,
    };

    /// The audit's second measured set: `2 * 1.5 < 2.0 + 2.0`, where the
    /// recurrence reported 4 against an achievable 3.
    const SUB_THRESHOLD_DEAR_GAPS: CostSet = CostSet {
        insertion: 2.0,
        deletion: 2.0,
        substitution: 1.0,
        transposition: 1.5,
    };

    /// `DamerauCosts::new(..)` for a [`CostSet`], whatever the verdict.
    fn damerau_costs_result(set: CostSet) -> Result<DamerauCosts, CostError> {
        DamerauCosts::new(
            set.insertion,
            set.deletion,
            set.substitution,
            set.transposition,
        )
    }

    /// What `DamerauCosts::new` must say about a cost set in a sweep's grid.
    ///
    /// Declared per grid entry, by hand, from Lowrance & Wagner's predicate
    /// `2 * transposition >= insertion + deletion`.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Damerau {
        /// The predicate holds, so the constructor must accept.
        Admissible,
        /// The predicate fails, so the constructor must return
        /// [`CostError::TranspositionBelowThreshold`].
        BelowThreshold,
    }

    /// `DamerauCosts::new` for a grid entry, asserted against the verdict the
    /// grid declares.
    ///
    /// The sweeps used to write `if let Ok(damerau) = damerau_costs_result(set)`
    /// and silently `continue` past every rejected entry. That makes a
    /// rejection invisible, including a *wrong* rejection — and it hid one:
    /// the all-`f64::MAX` row of the no-panic grid satisfies the predicate
    /// (`2 * f64::MAX` and `f64::MAX + f64::MAX` are both `+inf`), but the
    /// constructor evaluated a rearranged form that computed a threshold of
    /// `inf` and refused it. The sweep whose whole purpose was "every
    /// constructible cost set is exercised" was quietly not exercising the
    /// one row where the arithmetic could go wrong. A declared verdict makes
    /// the rejection an outcome the test asserts rather than a branch it
    /// skips.
    #[track_caller]
    fn damerau_costs_expecting(set: CostSet, verdict: Damerau) -> Option<DamerauCosts> {
        match (verdict, damerau_costs_result(set)) {
            (Damerau::Admissible, Ok(costs)) => Some(costs),
            (Damerau::BelowThreshold, Err(CostError::TranspositionBelowThreshold { .. })) => None,
            (want, got) => {
                panic!("cost set {set:?}: grid declares {want:?}, constructor said {got:?}")
            }
        }
    }

    #[test]
    fn damerau_costs_reject_below_the_lowrance_wagner_threshold() {
        // Lowrance-Wagner computes the minimum-cost edit script only while
        // `2 * transposition >= insertion + deletion`. Below it the
        // recurrence silently returns a value that is not the minimum, on
        // both of the audit's measured cost sets. The precondition is now
        // discharged by the constructor, so an inadmissible set cannot reach
        // a metric at all — there is nothing left to panic.
        assert!(damerau_costs_result(SUB_THRESHOLD_CHEAP_SWAP).is_err());
        assert!(damerau_costs_result(SUB_THRESHOLD_DEAR_GAPS).is_err());
        assert_eq!(
            damerau_costs_result(SUB_THRESHOLD_CHEAP_SWAP),
            Err(CostError::TranspositionBelowThreshold {
                transposition: 0.999,
                minimum: 1.0,
            })
        );

        // Admissible: the unit set holds it with equality (2 = 1 + 1), and
        // slack in either direction is fine.
        assert!(DamerauCosts::new(1.0, 1.0, 1.0, 1.0).is_ok());
        assert!(DamerauCosts::new(1.0, 1.0, 1.0, 3.0).is_ok());
        assert!(DamerauCosts::new(0.5, 0.5, 9.0, 0.5).is_ok());

        // The boundary is inclusive, evaluated in the constructor's own `f64`
        // arithmetic. `(0.1 + 0.2) / 2.0` is *not* `0.15`: the sum rounds to
        // 0.30000000000000004, so the minimum is 0.15000000000000002 and a
        // transposition of exactly 0.15 is below it. Both halves are pinned,
        // because the near-miss is the case a tolerance would silently
        // paper over.
        let minimum = (0.1f64 + 0.2f64) / 2.0;
        assert!(DamerauCosts::new(0.1, 0.2, 1.0, minimum).is_ok());
        assert_eq!(
            DamerauCosts::new(0.1, 0.2, 1.0, 0.15),
            Err(CostError::TranspositionBelowThreshold {
                transposition: 0.15,
                minimum,
            })
        );
        assert_eq!(minimum, 0.150_000_000_000_000_02);

        // The two sets from the published worked examples, at the boundary
        // and just below it.
        assert!(DamerauCosts::new(0.5, 0.5, 1.0, 0.5).is_ok()); // 1.0 >= 1.0
        assert!(DamerauCosts::new(1.0, 1.0, 5.0, 0.999).is_err()); // 1.998 < 2.0
        assert!(DamerauCosts::new(2.0, 2.0, 1.0, 1.5).is_err()); // 3.0 < 4.0

        // Admissible weighted costs really do compute: no panic, and the
        // answer is the weighted matrix's.
        let boundary = DamerauCosts::new(0.1, 0.2, 1.0, minimum).unwrap();
        assert_eq!(
            damerau_levenshtein_weighted("aab", "baa", &boundary).to_bits(),
            matrix_unrestricted("aab", "baa", &boundary.costs()).to_bits()
        );
    }

    #[test]
    fn damerau_costs_evaluate_the_predicate_as_written_not_a_rearrangement() {
        // `docs/design/distance-contract.md` §3.1 states the predicate as
        // `2 * transposition >= insertion + deletion`. The constructor
        // evaluates exactly that. The rearranged form `transposition >=
        // (insertion + deletion) / 2.0` is equivalent only while
        // `insertion + deletion` is a normal `f64`, and the cases below are
        // the ones where it is not — each of them a cost set the rearranged
        // form gets wrong.

        // The sum overflows and the predicate holds anyway: `2 * f64::MAX`
        // and `f64::MAX + f64::MAX` are both `+inf`, and `inf >= inf`. The
        // rearranged form would compare against `inf / 2.0 == inf` and
        // reject a set that satisfies Lowrance & Wagner in the reals.
        assert_eq!(2.0 * f64::MAX, f64::INFINITY);
        assert_eq!(f64::MAX + f64::MAX, f64::INFINITY);
        assert!(DamerauCosts::new(f64::MAX, f64::MAX, f64::MAX, f64::MAX).is_ok());
        assert!(DamerauCosts::new(f64::MAX, f64::MAX, 1.0, f64::MAX).is_ok());

        // The sum overflows and the predicate genuinely fails: `2 * (MAX/2)`
        // is `MAX` exactly, which is not `>= inf`. The rejection is real, and
        // the reported threshold is a *finite* number — the true mean of two
        // finite costs is at most `f64::MAX`, so there is always one to
        // report, and `+inf` would name a threshold no cost could meet.
        let half_max = f64::MAX / 2.0;
        assert_eq!(2.0 * half_max, f64::MAX);
        let rejected = DamerauCosts::new(f64::MAX, f64::MAX, 1.0, half_max);
        assert_eq!(
            rejected,
            Err(CostError::TranspositionBelowThreshold {
                transposition: half_max,
                minimum: f64::MAX,
            })
        );
        let Err(CostError::TranspositionBelowThreshold { minimum, .. }) = rejected else {
            unreachable!("just asserted the variant")
        };
        assert!(
            minimum.is_finite(),
            "reported threshold {minimum} is not finite"
        );

        // Subnormal costs, where halving is the operation that is inexact
        // rather than doubling. `u` is the smallest positive `f64`.
        // `insertion + deletion = 5u` exactly, so the threshold is `2.5u` and
        // a transposition of `2u` is below it: `4u < 5u` rejects. The
        // rearranged form computes `fl(5u / 2) == 2u` — the tie rounds to
        // even — and would accept.
        let u = f64::from_bits(1);
        assert_eq!(u + 4.0 * u, 5.0 * u);
        assert!(DamerauCosts::new(u, 4.0 * u, 1.0, 2.0 * u).is_err()); // 4u < 5u
        assert!(DamerauCosts::new(u, 4.0 * u, 1.0, 3.0 * u).is_ok()); // 6u >= 5u

        // Every rejection still carries a finite threshold, whatever the
        // magnitudes: sweep the corners of the range against the predicate
        // evaluated independently here.
        let magnitudes = [
            0.0,
            u,
            f64::MIN_POSITIVE,
            0.5,
            1.0,
            half_max,
            f64::MAX / 4.0,
            f64::MAX,
        ];
        for i in magnitudes {
            for d in magnitudes {
                for t in magnitudes {
                    let want_ok = 2.0 * t >= i + d;
                    match DamerauCosts::new(i, d, 1.0, t) {
                        Ok(_) => assert!(want_ok, "accepted i={i} d={d} t={t}"),
                        Err(CostError::TranspositionBelowThreshold {
                            transposition,
                            minimum,
                        }) => {
                            assert!(!want_ok, "rejected i={i} d={d} t={t}");
                            assert_eq!(transposition.to_bits(), t.to_bits());
                            assert!(minimum.is_finite(), "i={i} d={d} t={t} -> {minimum}");
                        }
                        Err(other) => panic!("i={i} d={d} t={t}: {other}"),
                    }
                }
            }
        }
    }

    #[test]
    fn cost_errors_compare_equal_to_themselves_including_the_nan_case() {
        // `CostError`'s `PartialEq` is written by hand and compares the `f64`
        // payloads bitwise. A derived impl compares them with `==`, under
        // which `NaN != NaN` — and `NaN` is the canonical way to reach
        // `NotFinite`, so a derived impl leaves the error unequal to itself
        // in its most common case and `assert_eq!` on a `Result` silently
        // cannot be written.
        // The shape a test wants to write, and could not before: a `Result`
        // compared against the rejection it is supposed to be.
        assert_eq!(
            LevenshteinCosts::new(f64::NAN, 1.0, 1.0),
            Err(CostError::NotFinite {
                operation: Operation::Insertion,
                value: f64::NAN,
            })
        );

        // Nine distinct rejections. Every one equals itself (the `n == m`
        // diagonal) and differs from every other, so the relation is neither
        // broken by `NaN` nor collapsed by the bitwise comparison.
        let errors = [
            LevenshteinCosts::new(f64::NAN, 1.0, 1.0).unwrap_err(),
            LevenshteinCosts::new(1.0, f64::NAN, 1.0).unwrap_err(),
            LevenshteinCosts::new(1.0, 1.0, f64::NAN).unwrap_err(),
            LevenshteinCosts::new(f64::INFINITY, 1.0, 1.0).unwrap_err(),
            LevenshteinCosts::new(f64::NEG_INFINITY, 1.0, 1.0).unwrap_err(),
            LevenshteinCosts::new(-1.0, 1.0, 1.0).unwrap_err(),
            OsaCosts::new(1.0, 1.0, 1.0, f64::NAN).unwrap_err(),
            OsaCosts::new(1.0, 1.0, 1.0, -1.0).unwrap_err(),
            DamerauCosts::new(1.0, 1.0, 1.0, 0.25).unwrap_err(),
        ];
        for (n, e) in errors.iter().enumerate() {
            for (m, other) in errors.iter().enumerate() {
                assert_eq!(e == other, n == m, "errors {n} and {m}: {e:?} / {other:?}");
            }
        }

        // Two errors describing the same rejection compare equal even when
        // they come from different constructors — the error is about the
        // cost, not about which type refused it.
        assert_eq!(
            OsaCosts::new(1.0, 1.0, 1.0, f64::NAN).unwrap_err(),
            DamerauCosts::new(1.0, 1.0, 1.0, f64::NAN).unwrap_err()
        );

        // The one point at which bit equality is finer than `==`: a
        // transposition supplied as `-0.0` is admissible as a *cost* (it is
        // zero) but below any positive threshold, so it is reported as
        // supplied and stays distinct from `0.0`. That is the right answer
        // for a variant whose job is to say what the caller passed.
        let signed_zero = DamerauCosts::new(1.0, 1.0, 1.0, -0.0);
        let plain_zero = DamerauCosts::new(1.0, 1.0, 1.0, 0.0);
        assert_eq!(
            signed_zero,
            Err(CostError::TranspositionBelowThreshold {
                transposition: -0.0,
                minimum: 1.0,
            })
        );
        assert_ne!(signed_zero, plain_zero);

        // The cost types themselves keep a derived `PartialEq`: they cannot
        // hold a `NaN`, so it is already reflexive there.
        let costs = DamerauCosts::new(1.0, 2.0, 3.0, 4.0).unwrap();
        assert_eq!(costs, DamerauCosts::new(1.0, 2.0, 3.0, 4.0).unwrap());
    }

    #[test]
    fn cost_constructors_reject_non_finite_and_negative_costs() {
        // A cost is admissible when it is finite and non-negative. Zero is
        // admissible; negative and non-finite are not, because a "distance"
        // of -4.0 between a string and itself is not a distance and a NaN
        // one cannot be thresholded, ranked or normalised.
        for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert!(
                matches!(
                    LevenshteinCosts::new(bad, 1.0, 1.0),
                    Err(CostError::NotFinite {
                        operation: Operation::Insertion,
                        ..
                    })
                ),
                "insertion {bad}"
            );
        }
        // NaN compares unequal to itself, so the NaN cases are matched
        // structurally rather than by equality.
        assert!(matches!(
            LevenshteinCosts::new(f64::NAN, 1.0, 1.0),
            Err(CostError::NotFinite {
                operation: Operation::Insertion,
                ..
            })
        ));
        assert_eq!(
            LevenshteinCosts::new(1.0, f64::INFINITY, 1.0),
            Err(CostError::NotFinite {
                operation: Operation::Deletion,
                value: f64::INFINITY,
            })
        );
        assert_eq!(
            LevenshteinCosts::new(1.0, 1.0, f64::NEG_INFINITY),
            Err(CostError::NotFinite {
                operation: Operation::Substitution,
                value: f64::NEG_INFINITY,
            })
        );
        assert_eq!(
            LevenshteinCosts::new(-1.0, 1.0, 1.0),
            Err(CostError::Negative {
                operation: Operation::Insertion,
                value: -1.0,
            })
        );
        assert_eq!(
            LevenshteinCosts::new(1.0, -0.5, 1.0),
            Err(CostError::Negative {
                operation: Operation::Deletion,
                value: -0.5,
            })
        );
        assert_eq!(
            LevenshteinCosts::new(1.0, 1.0, -2.0),
            Err(CostError::Negative {
                operation: Operation::Substitution,
                value: -2.0,
            })
        );

        // Zero is accepted, and so is negative zero, which *is* zero.
        assert!(LevenshteinCosts::new(0.0, 0.0, 0.0).is_ok());
        assert!(LevenshteinCosts::new(-0.0, 0.0, 0.0).is_ok());

        // The same rules on the four-field types, plus the transposition
        // field they add.
        assert_eq!(
            OsaCosts::new(1.0, 1.0, 1.0, -1.0),
            Err(CostError::Negative {
                operation: Operation::Transposition,
                value: -1.0,
            })
        );
        assert_eq!(
            DamerauCosts::new(1.0, 1.0, 1.0, f64::INFINITY),
            Err(CostError::NotFinite {
                operation: Operation::Transposition,
                value: f64::INFINITY,
            })
        );
        assert!(matches!(
            DamerauCosts::new(1.0, 1.0, 1.0, f64::NAN),
            Err(CostError::NotFinite {
                operation: Operation::Transposition,
                ..
            })
        ));
        // A negative transposition — a free lunch that would make repeated
        // swapping unboundedly profitable — is rejected before the threshold
        // test ever runs.
        assert_eq!(
            DamerauCosts::new(1.0, 1.0, 1.0, -1.0),
            Err(CostError::Negative {
                operation: Operation::Transposition,
                value: -1.0,
            })
        );

        // The accessors return exactly what was supplied.
        let costs = OsaCosts::new(0.25, 1.5, 2.0, 0.75).unwrap();
        assert_eq!(costs.insertion(), 0.25);
        assert_eq!(costs.deletion(), 1.5);
        assert_eq!(costs.substitution(), 2.0);
        assert_eq!(costs.transposition(), 0.75);
    }

    #[test]
    fn cost_constructors_are_usable_in_const_position() {
        // `new` is a `const fn`, which is the whole reason no
        // `from_integers` constructor exists: a caller who wants a cost set
        // in a `const` can have one.
        const UNIT: Result<LevenshteinCosts, CostError> = LevenshteinCosts::new(1.0, 1.0, 1.0);
        assert!(UNIT.is_ok());
        const BAD: Result<DamerauCosts, CostError> = DamerauCosts::new(1.0, 1.0, 1.0, 0.25);
        assert!(BAD.is_err());
    }

    #[test]
    fn cost_error_displays_the_offending_value() {
        let err = LevenshteinCosts::new(1.0, -3.5, 1.0).unwrap_err();
        let text = err.to_string();
        assert!(text.contains("deletion"), "{text}");
        assert!(text.contains("-3.5"), "{text}");
        let err = DamerauCosts::new(1.0, 1.0, 1.0, 0.25).unwrap_err();
        let text = err.to_string();
        assert!(text.contains("0.25"), "{text}");
        assert!(text.contains("Lowrance & Wagner"), "{text}");
        // It is a `std::error::Error`, so it composes with `?` and `Box<dyn>`.
        let boxed: Box<dyn std::error::Error> = Box::new(err);
        assert!(!boxed.to_string().is_empty());
    }

    #[test]
    fn osa_accepts_every_cost_set_including_sub_threshold_ones() {
        // OSA's recurrence *defines* its answer — the minimum over alignments
        // that edit no position twice — so no cost set can make it return
        // something other than what it means. Both of the sets unrestricted
        // Damerau rejects must therefore still build as `OsaCosts` and
        // compute, and must agree with the three-row scalar DP that is OSA's
        // own definition in code.
        for set in [SUB_THRESHOLD_CHEAP_SWAP, SUB_THRESHOLD_DEAR_GAPS] {
            let costs = set.osa();
            for (a, b) in [("aab", "baa"), ("ca", "abc"), ("abcd", "acbd"), ("", "a")] {
                let want = dispatch(a, b, |ops| match ops {
                    Operands::Bytes(s, t) => osa_rows(s, t, &costs.costs()),
                    Operands::Units(s, t) => osa_rows(s, t, &costs.costs()),
                });
                assert_eq!(
                    osa_weighted(a, b, &costs).to_bits(),
                    want.to_bits(),
                    "{a:?} {b:?}"
                );
                // ...and so must `osa_search_weighted`, which shares
                // `full_matrix` with the Damerau variant whose cost set is
                // rejected at construction.
                let _ = osa_search_weighted(a, b, &costs);
            }
        }
        // A transposition of 0.25 is deliberately published for OSA.
        assert!(OsaCosts::new(1.0, 1.0, 1.0, 0.25).is_ok());
        // The witness that a sub-threshold cost set is genuinely meaningful
        // for OSA and genuinely undefined for unrestricted Damerau. The two
        // swaps that take "aab" to "baa" for 1.998 overlap, so OSA's rule
        // forbids them and its answer is a deletion plus an insertion, 2.0 —
        // which is exactly what its recurrence *means*, not an under-counted
        // minimum. Unrestricted Damerau, whose answer would be 1.998, cannot
        // reach it with the Lowrance-Wagner recurrence, so the cost set is
        // refused at construction instead of reporting the same 2.0 as if it
        // were the minimum.
        assert_eq!(
            osa_weighted("aab", "baa", &SUB_THRESHOLD_CHEAP_SWAP.osa()),
            2.0
        );
        assert!(damerau_costs_result(SUB_THRESHOLD_CHEAP_SWAP).is_err());
    }

    #[test]
    fn empty_operands_cost_the_other_operand_s_length() {
        // `docs/design/distance-contract.md` §3.1's empty-operand table, for
        // the unit tier: the answer is the other operand's scalar count.
        // The corpus spans ASCII, BMP and astral — the astral rows are the
        // ones that moved when the unit did.
        for text in [
            "",
            "a",
            "abc",
            "kitten",
            "café",
            "Москва",
            "日本語",
            "ﬀ",
            "  ",
            "😀",
            "a😀b",
            "𝕳𝖊𝖑𝖑𝖔",
            "😀😁😂",
        ] {
            let units = text.chars().count();
            assert_eq!(levenshtein("", text), units, "insertions into {text:?}");
            assert_eq!(levenshtein(text, ""), units, "deletions from {text:?}");
            assert_eq!(osa("", text), units, "osa insertions into {text:?}");
            assert_eq!(osa(text, ""), units, "osa deletions from {text:?}");
            assert_eq!(
                damerau_levenshtein("", text),
                units,
                "damerau insertions into {text:?}"
            );
            assert_eq!(
                damerau_levenshtein(text, ""),
                units,
                "damerau deletions from {text:?}"
            );
        }
        assert_eq!(levenshtein("", ""), 0);
    }

    #[test]
    fn the_length_lemma_holds() {
        // `docs/design/distance-contract.md` §3.1: each insertion or deletion
        // changes the unit count by exactly one and each substitution by
        // zero, so the count difference is a lower bound on the distance.
        // This is published as contract because callers build screening gates
        // on it, and a gate built on a length this crate does not count in
        // silently discards true matches: `"ab"` versus `"ab😀"` differs by
        // one scalar but by two UTF-16 code units, so a `k = 1` gate keyed
        // on the latter would drop a true match. The length is
        // `chars().count()`, and it is the only length this crate ships.
        let mut rng = SplitMix64(0x1E27_2026_0819_0001);
        for round in 0..3000 {
            let (a, b) = property_pair(&mut rng, round);
            let delta = a.chars().count().abs_diff(b.chars().count());
            let d = levenshtein(&a, &b);
            assert!(
                delta <= d,
                "length lemma violated: |{delta}| > levenshtein({a:?}, {b:?}) = {d}"
            );
            // The same bound holds for the two Damerau variants, whose extra
            // operation preserves the unit count.
            assert!(delta <= osa(&a, &b));
            assert!(delta <= damerau_levenshtein(&a, &b));
        }
        // The weighted analogue — `min(insertion, deletion) * |Δ|` — holds in
        // exact arithmetic but NOT bit-for-bit, so it is pinned with the
        // tolerance the contract publishes rather than as a strict bound.
        // The recurrence accumulates costs by repeated addition while the
        // bound is a single multiplication; the two round differently.
        let costs = lev_costs(2.0, 3.0, 0.5);
        for (a, b) in [
            ("abc", "abcdef"),
            ("kitten", ""),
            ("", "ab"),
            ("ab", "ab😀"),
        ] {
            let delta = a.chars().count().abs_diff(b.chars().count());
            let bound = 2.0 * delta as f64;
            let d = levenshtein_weighted(a, b, &costs);
            // 2.0, 3.0 and 0.5 are exactly representable, so these round
            // identically and the bound is met exactly.
            assert!(
                bound <= d,
                "weighted length lemma violated for {a:?} vs {b:?}: {bound} > {d}"
            );
        }
    }

    /// The weighted length lemma is a statement about the exact-real metric,
    /// not about the `f64` the crate returns. With a cost that has no exact
    /// binary representation, ten accumulated additions land one ulp below the
    /// closed-form product — so a screening gate written as `d >= bound` is
    /// unsound and must carry a tolerance.
    ///
    /// This is the case that the exactly-representable costs above cannot
    /// reach, and its absence is why the contract previously published a bound
    /// the code did not honour.
    #[test]
    fn the_weighted_length_lemma_needs_a_tolerance() {
        let costs = lev_costs(0.1, 0.1, 1.0);
        let d = levenshtein_weighted("", "abcdefghij", &costs);
        let bound = 0.1f64.min(0.1) * 10.0;

        // Strictly below the closed form: the unsound gate would reject this.
        assert!(d < bound, "expected the accumulation to round low, got {d}");
        // By exactly one ulp — not an arbitrary drift.
        assert_eq!(
            bound.to_bits() - d.to_bits(),
            1,
            "expected a one-ulp deficit"
        );

        // The gate the contract actually sanctions.
        let epsilon = 8.0 * f64::EPSILON * bound;
        assert!(d >= bound - epsilon, "tolerance gate rejected a true match");
    }

    #[test]
    fn damerau_never_exceeds_osa_and_both_bracket_levenshtein() {
        // A cross-algorithm invariant that no single implementation can
        // satisfy on its own: every OSA alignment is a legal unrestricted
        // Damerau edit script, and every Levenshtein script is a legal OSA
        // script, so `damerau <= osa <= levenshtein` pointwise. A canonicity
        // bug in any one of the three shows up as an inversion here.
        let mut rng = SplitMix64(0x0B0B_2026_0819_0009);
        for round in 0..3000 {
            let (a, b) = property_pair(&mut rng, round);
            let d = damerau_levenshtein(&a, &b);
            let o = osa(&a, &b);
            let l = levenshtein(&a, &b);
            assert!(d <= o, "damerau {d} > osa {o} for {a:?} vs {b:?}");
            assert!(o <= l, "osa {o} > levenshtein {l} for {a:?} vs {b:?}");
        }
    }

    #[test]
    fn damerau_levenshtein_satisfies_the_triangle_inequality() {
        // Unrestricted Damerau-Levenshtein is a metric; the recurrence this
        // replaced was not even symmetric, let alone a metric. Randomized
        // triples over a narrow alphabet, where near-collinear triples (and
        // therefore tight bounds) are common.
        let mut rng = SplitMix64(0x7817_2026_0819_000A);
        for round in 0..3000 {
            let make = |rng: &mut SplitMix64| -> String {
                let n = rng.next_range(12);
                (0..n)
                    .map(|_| b"abc"[rng.next_range(3)] as char)
                    .collect::<String>()
            };
            let (x, y, z) = (make(&mut rng), make(&mut rng), make(&mut rng));
            let xz = damerau_levenshtein(&x, &z);
            let xy = damerau_levenshtein(&x, &y);
            let yz = damerau_levenshtein(&y, &z);
            assert!(
                xz <= xy + yz,
                "triangle violated at round {round}: d({x:?},{z:?})={xz} > {xy} + {yz}"
            );
        }
    }

    // -- Bit-parallel search battery ----------------------------------------

    /// The full-matrix search forced end-to-end — `search_full_matrix`
    /// through `dispatch`, assembled into a `SearchResult` exactly as
    /// `search_unit_impl` does — bypassing the fast-path gate entirely. What
    /// every `search_bits` test below compares full `SearchResult`s
    /// (borrowed substring, `f64` distance bits, byte start) against.
    ///
    /// **This is not an independent oracle.** It calls `search_full_matrix`,
    /// so it can only show that the bit-parallel path and the matrix path
    /// agree — a defect they share is invisible here, which is how the
    /// `(n + m)` seed and the one-row-short backtrack survived. The
    /// independent one is `brute_force_search_distance` /
    /// `assert_search_matches_brute_force` above, which reach neither
    /// routine.
    fn oracle_search<'t>(
        a: &str,
        b: &'t str,
        costs: &Costs,
        variant: Variant,
    ) -> SearchResult<'t, f64> {
        let (start, end, dist) = dispatch(a, b, |ops| match ops {
            Operands::Bytes(s, t) => search_full_matrix(s, t, costs, variant),
            Operands::Units(s, t) => search_full_matrix(s, t, costs, variant),
        });
        borrow_span(b, start, end, dist)
    }

    /// [`oracle_search`] at unit costs, as the `SearchResult<usize>` the unit
    /// tier returns.
    fn oracle_search_unit<'t>(a: &str, b: &'t str, variant: Variant) -> SearchResult<'t, usize> {
        let got = oracle_search(a, b, &Costs::UNIT, variant);
        SearchResult {
            substring: got.substring(),
            distance: exact_usize(got.distance()),
            start: got.range().start,
        }
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
        for case in 0..3000usize {
            let alphabet = [2usize, 3, 4, 26][rng.next_range(4)];
            let n = 1 + rng.next_range(if case % 5 == 0 { 200 } else { 90 });
            let m = 1 + rng.next_range(220);
            let s = search_rand(&mut rng, n, alphabet);
            let mut t = search_rand(&mut rng, m, alphabet);
            if rng.next_range(2) == 0 {
                embed_near_match(&mut rng, &s, &mut t, alphabet);
            }
            let got = levenshtein_search(&s, &t);
            let want = oracle_search_unit(&s, &t, Variant::Plain);
            assert_eq!(got, want, "search mismatch: s={s:?} t={t:?}");
        }
    }

    #[test]
    fn search_bits_boundary_needle_lengths_agree() {
        // The word/blocks dispatch boundary (64) and the one/two-block
        // boundary (128) swept explicitly rather than left to the random
        // corpus, with embedded near-matches at every combination.
        let mut rng = SplitMix64(0x5EA2_C4B1_D00D_0002);
        for &n in &[1usize, 2, 63, 64, 65, 66, 127, 128, 129, 130] {
            for &m in &[1usize, 64, 65, 129, 200] {
                for _ in 0..6 {
                    let s = search_rand(&mut rng, n, 3);
                    let mut t = search_rand(&mut rng, m, 3);
                    embed_near_match(&mut rng, &s, &mut t, 3);
                    let got = levenshtein_search(&s, &t);
                    let want = oracle_search_unit(&s, &t, Variant::Plain);
                    assert_eq!(got, want, "boundary mismatch n={n} m={m}");
                }
            }
        }
    }

    #[test]
    fn search_bits_agrees_on_scalar_input() {
        // The same differential through the `Operands::Units` (char) path:
        // Cyrillic and astral characters, one unit each, exercising the
        // multi-byte byte-range derivation in the borrowed substring. The
        // char kernels share every line with the u8 ones except the
        // FxHashMap Peq, so this pins the table plumbing.
        let mut rng = SplitMix64(0x5EA2_C4B1_D00D_0003);
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
            let got = levenshtein_search(&s, &t);
            let want = oracle_search_unit(&s, &t, Variant::Plain);
            assert_eq!(got, want, "char search mismatch: s={s:?} t={t:?}");
        }
        // Astral needles and haystacks: unit lengths cross the word
        // boundary at half the character count.
        for _ in 0..200 {
            let s_units = 2 + rng.next_range(100);
            let t_units = 2 + rng.next_range(140);
            let s = random_unicode_wide(&mut rng, s_units);
            let t = random_unicode_wide(&mut rng, t_units);
            let got = levenshtein_search(&s, &t);
            let want = oracle_search_unit(&s, &t, Variant::Plain);
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
        let opts = Costs::UNIT;
        for _ in 0..150 {
            let n = 1 + rng.next_range(150);
            let m = 1 + rng.next_range(150);
            let s = search_rand(&mut rng, n, 3).into_bytes();
            let t = search_rand(&mut rng, m, 3).into_bytes();
            let mat = full_matrix(&s, &t, &opts, Variant::Plain, true);
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
        for (s, t) in [
            ("aaa", "aaaaaa"),
            ("aa", "aa"),
            ("ab", "ababab"),
            ("aba", "bab"),
            ("ca", "abc"),
            ("b", "aaa"),
        ] {
            let got = levenshtein_search(s, t);
            let want = oracle_search_unit(s, t, Variant::Plain);
            assert_eq!(got, want, "tie-break mismatch for {s:?} in {t:?}");
        }
        // The zero-distance prefix tie: "aaa" occurs at offsets 0..=3 in
        // "aaaaaa"; the first minimum keeps the earliest end (column 3),
        // and the backtrack walks pure matches to offset 0.
        let r = levenshtein_search("aaa", "aaaaaa");
        assert_eq!((r.substring(), r.distance(), r.range()), ("aaa", 0, 0..3));
    }

    #[test]
    fn search_weighted_damerau_and_empty_operands_keep_the_matrix_path() {
        // The three shapes the bit-parallel search path excludes, each
        // checked to still produce the matrix path's answers. Weighted costs
        // are the first: no bit-vector formulation exists, which is why they
        // are a different function rather than a different argument.
        let weighted = lev_costs(1.0, 1.0, 0.5);
        let got = levenshtein_search_weighted("kitten", "sitting", &weighted);
        let want = oracle_search("kitten", "sitting", &weighted.costs(), Variant::Plain);
        assert_eq!(got, want);

        // Damerau and OSA search: transposition parents depend on
        // `last_row_map`/row-gap state, unrecoverable from cell costs — so
        // neither may take the fast path even under unit costs. "ca" in
        // "abc" distinguishes them: the unrestricted transposition changes
        // both distance and backtrace.
        for (s, t) in [("ca", "abc"), ("ab", "xxbaxx"), ("abcd", "acbd")] {
            let got = damerau_levenshtein_search(s, t);
            let want = oracle_search_unit(s, t, Variant::Damerau);
            assert_eq!(got, want, "damerau search mismatch for {s:?} in {t:?}");

            let got_osa = osa_search(s, t);
            let want_osa = oracle_search_unit(s, t, Variant::Osa);
            assert_eq!(got_osa, want_osa, "osa search mismatch for {s:?} in {t:?}");
        }

        // Empty operands: excluded from the fast path so the kernels can
        // assume a non-empty pattern; answers come from the matrix path.
        for (s, t) in [("", "abc"), ("abc", ""), ("", "")] {
            let got = levenshtein_search(s, t);
            let want = oracle_search_unit(s, t, Variant::Plain);
            assert_eq!(got, want, "empty-operand mismatch for {s:?} in {t:?}");
        }
        assert_eq!(levenshtein_search("", "abc").distance, 0);
        assert_eq!(levenshtein_search("abc", "").distance, 3);
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
        for key in ["ascii", "cyrillic"] {
            for (size, pair) in json["pairs"][key].as_object().expect("pair map") {
                let a = pair[0].as_str().unwrap();
                let b = pair[1].as_str().unwrap();
                let got = levenshtein_search(a, b);
                let want = oracle_search_unit(a, b, Variant::Plain);
                assert_eq!(got, want, "bench pair {key}/{size}");
            }
        }
    }

    // -- Common-affix reduction (plain + OSA) -------------------------------

    /// The near-identical shape the competitive grid measures: a copy of
    /// `s` with exactly one unit changed at `at`. `at` indexes scalars, so
    /// astral input is mutated a whole character at a time — the unit this
    /// crate measures in, and the only one under which every mutation is
    /// still a valid `String`.
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

    /// A copy of `s` with one unit deleted — the other near-identical
    /// shape, and the one that makes the trimmed operands differ in
    /// length (so the suffix scan starts from unequal offsets).
    fn delete_one(s: &str, at: usize) -> String {
        let chars: Vec<char> = s.chars().collect();
        let i = at % chars.len();
        chars
            .iter()
            .enumerate()
            .filter_map(|(k, &c)| (k != i).then_some(c))
            .collect()
    }

    #[test]
    fn plain_trim_gate_boundary_lengths_agree_with_the_row_oracle() {
        // The trim gate moved from `min > 16` to `min > TRIM_MIN_LEN`, so
        // every length either side of BOTH thresholds is swept here against
        // the untrimmed `plain_rows` oracle, with the near-identical shapes
        // that make the trim actually fire. Lengths 1..=24 cover the
        // table-free tiny kernel (<= 4), the newly-trimming band (5..=16),
        // and the band that trimmed before (17+).
        let mut rng = SplitMix64(0x7411_0000_0BEE_F001);
        for len in 1usize..=24 {
            for round in 0..8 {
                let a = random_ascii_wide(&mut rng, len);
                let subst = substitute_one(&a, round * 3);
                let deleted = delete_one(&a, round * 5);
                let random = random_ascii_wide(&mut rng, len);
                for b in [subst, deleted, random, a.clone(), String::new()] {
                    assert_eq!(
                        levenshtein(&a, &b),
                        oracle_plain_rows_unit(&a, &b),
                        "len={len} round={round} {a:?} vs {b:?}"
                    );
                    assert_eq!(
                        levenshtein(&b, &a),
                        oracle_plain_rows_unit(&b, &a),
                        "reversed len={len} round={round}"
                    );
                }
            }
        }
    }

    #[test]
    fn plain_trim_agrees_on_near_identical_corpora_at_scale() {
        // Long near-identical pairs, which is exactly the shape the trim
        // turns from O(n*m/64) into O(1) bit-vector work: if the trim were
        // off by one unit at either end the distance would change, and the
        // untrimmed `plain_rows` oracle would catch it. ASCII, Cyrillic and
        // astral-bearing operands all included, since the trim runs on the
        // ASCII byte fast path and on the `char` slices alike.
        let mut rng = SplitMix64(0xA5A5_1234_9E37_79B9);
        for &len in &[17usize, 63, 64, 65, 127, 200, 256, 1024] {
            let ascii = random_ascii_wide(&mut rng, len);
            let unicode = random_unicode_wide(&mut rng, len);
            for base in [ascii, unicode] {
                for &at in &[0usize, 1, len / 2, len.saturating_sub(1)] {
                    for variant in [substitute_one(&base, at), delete_one(&base, at)] {
                        assert_eq!(
                            levenshtein(&base, &variant),
                            oracle_plain_rows_unit(&base, &variant),
                            "len={len} at={at}"
                        );
                        assert_eq!(
                            levenshtein(&variant, &base),
                            oracle_plain_rows_unit(&variant, &base),
                            "reversed len={len} at={at}"
                        );
                    }
                }
                // A shared prefix and suffix around a wholly different
                // middle: the trim must stop exactly where the operands
                // diverge, not run on into the differing region.
                let middle_a = format!("{base}xxxx{base}");
                let middle_b = format!("{base}yyzz{base}");
                assert_eq!(
                    levenshtein(&middle_a, &middle_b),
                    oracle_plain_rows_unit(&middle_a, &middle_b),
                    "shared-surround len={len}"
                );
            }
        }
    }

    #[test]
    fn osa_affix_trim_matches_the_untrimmed_oracle_exhaustively() {
        // The strongest available pin for the OSA trim: every pair over a
        // two-letter alphabet up to length 7 and a three-letter alphabet up
        // to length 5, compared against `osa_rows` running on the
        // UNTRIMMED operands. A transposition straddling either cut is
        // dense in these spaces (`"ab"`/`"ba"` fragments are everywhere),
        // so if the reduction were unsound for OSA at all it would fail
        // here rather than needing a hand-built witness.
        fn enumerate(alphabet: &[u8], max_len: usize) -> Vec<String> {
            let mut out = vec![String::new()];
            let mut frontier = vec![String::new()];
            for _ in 0..max_len {
                let mut next = Vec::new();
                for s in &frontier {
                    for &c in alphabet {
                        next.push(format!("{s}{}", c as char));
                    }
                }
                out.extend(next.iter().cloned());
                frontier = next;
            }
            out
        }

        for (alphabet, max_len) in [(&b"ab"[..], 7usize), (&b"abc"[..], 5usize)] {
            let all = enumerate(alphabet, max_len);
            for a in &all {
                for b in &all {
                    assert_eq!(
                        osa(a, b),
                        oracle_osa(a, b),
                        "osa trim mismatch for {a:?} vs {b:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn osa_affix_trim_straddling_transposition_witnesses_agree() {
        // Hand-built witnesses for the exact alignment shape the trim could
        // plausibly lose: an adjacent transposition sitting flush against
        // the prefix cut, against the suffix cut, and against both at once,
        // including the case where the affix's own last/first unit is one
        // of the transposed pair (the `(p+1, p+1)` guard that maximality is
        // what rules out). Each is checked against the untrimmed oracle.
        let affixes = ["", "a", "ab", "aab", "abcabcabc", &"z".repeat(70)];
        let cores = [
            ("ab", "ba"),
            ("aab", "aba"),
            ("aba", "baa"),
            ("abc", "bac"),
            ("abc", "acb"),
            ("xay", "xya"),
            ("ab", "b"),
            ("ab", "bab"),
            ("bb", "abbb"),
        ];
        for prefix in affixes {
            for suffix in affixes {
                for (ca, cb) in cores {
                    let a = format!("{prefix}{ca}{suffix}");
                    let b = format!("{prefix}{cb}{suffix}");
                    assert_eq!(
                        osa(&a, &b),
                        oracle_osa(&a, &b),
                        "straddle mismatch {a:?} vs {b:?}"
                    );
                    assert_eq!(
                        osa(&b, &a),
                        oracle_osa(&b, &a),
                        "reversed straddle mismatch {b:?} vs {a:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn osa_affix_trim_agrees_on_near_identical_corpora_at_scale() {
        // The competitive grid's own shape at the sizes where the OSA loss
        // was measured, plus Cyrillic and astral operands so the `char`
        // monomorphization of the trim is covered too. Sizes straddle the
        // 64-unit single-word/block dispatch boundary in both the untrimmed
        // and the trimmed length, since the trim can move a pair from the
        // block kernel to the single-word one or to the scalar fallback.
        let mut rng = SplitMix64(0x0A5A_0000_C0FF_EE01);
        for &len in &[5usize, 17, 63, 64, 65, 66, 130, 256, 1024] {
            let ascii = random_ascii_wide(&mut rng, len);
            let unicode = random_unicode_wide(&mut rng, len);
            for base in [ascii, unicode] {
                for &at in &[0usize, 1, len / 2, len.saturating_sub(1)] {
                    for variant in [substitute_one(&base, at), delete_one(&base, at)] {
                        assert_eq!(
                            osa(&base, &variant),
                            oracle_osa(&base, &variant),
                            "osa len={len} at={at}"
                        );
                        assert_eq!(
                            osa(&variant, &base),
                            oracle_osa(&variant, &base),
                            "osa reversed len={len} at={at}"
                        );
                    }
                }
                // Identical operands trim to nothing at all — the empty
                // short-circuits, not the kernels, must answer these.
                assert_eq!(osa(&base, &base), 0);
                // A transposed pair in the exact middle of two identical
                // halves: the trim leaves a two-unit residue whose whole
                // answer is the transposition.
                let mut units: Vec<char> = base.chars().collect();
                if units.len() >= 2 {
                    let mid = units.len() / 2;
                    units.swap(mid - 1, mid);
                    let swapped: String = units.into_iter().collect();
                    assert_eq!(
                        osa(&base, &swapped),
                        oracle_osa(&base, &swapped),
                        "osa mid-swap len={len}"
                    );
                }
            }
        }
    }

    #[test]
    fn the_two_tiers_agree_at_unit_costs() {
        // This replaces `affix_trim_never_runs_for_weighted_costs`, whose
        // witnesses were negative-cost cost sets (`substitution: -1.0` made
        // `levenshtein("abcdefg", "abcdefg")` answer `-4.0`, proving that
        // trimming first would have been wrong). Those cost sets are
        // unconstructable now, and what they proved has moved into the type
        // system: `levenshtein` trims and `levenshtein_weighted` does not,
        // and no cost *value* can move a call between them, because the
        // choice is which function the caller called. What remains to pin is
        // that the two tiers answer the same question where they overlap.
        let unit_plain = lev_costs(1.0, 1.0, 1.0);
        let unit_osa = osa_costs(1.0, 1.0, 1.0, 1.0);
        let unit_damerau = damerau_costs(1.0, 1.0, 1.0, 1.0);

        let mut rng = SplitMix64(0x0057_0057_0057_0057);
        for round in 0..3000 {
            let (a, b) = property_pair(&mut rng, round);
            assert_eq!(
                levenshtein(&a, &b) as f64,
                levenshtein_weighted(&a, &b, &unit_plain),
                "plain tiers disagree for {a:?} vs {b:?}"
            );
            assert_eq!(
                osa(&a, &b) as f64,
                osa_weighted(&a, &b, &unit_osa),
                "osa tiers disagree for {a:?} vs {b:?}"
            );
            assert_eq!(
                damerau_levenshtein(&a, &b) as f64,
                damerau_levenshtein_weighted(&a, &b, &unit_damerau),
                "damerau tiers disagree for {a:?} vs {b:?}"
            );
        }

        // The same equivalence on the near-identical shapes the unit tier's
        // affix trim collapses and the weighted tier does not touch at all —
        // the pairs where a trimming bug would show up as a tier
        // disagreement.
        let mut rng = SplitMix64(0x0058_0058_0058_0058);
        for &len in &[8usize, 40, 130, 300] {
            for base in [
                random_ascii_wide(&mut rng, len),
                random_unicode_wide(&mut rng, len),
            ] {
                for variant in [
                    substitute_one(&base, len / 2),
                    delete_one(&base, len / 2),
                    base.clone(),
                    format!("{base}{base}"),
                ] {
                    assert_eq!(
                        levenshtein(&base, &variant) as f64,
                        levenshtein_weighted(&base, &variant, &unit_plain),
                        "plain tiers disagree at len={len}"
                    );
                    assert_eq!(
                        osa(&base, &variant) as f64,
                        osa_weighted(&base, &variant, &unit_osa),
                        "osa tiers disagree at len={len}"
                    );
                    assert_eq!(
                        damerau_levenshtein(&base, &variant) as f64,
                        damerau_levenshtein_weighted(&base, &variant, &unit_damerau),
                        "damerau tiers disagree at len={len}"
                    );
                }
            }
        }
    }

    #[test]
    fn weighted_distances_match_the_scalar_recurrences_over_the_admissible_grid() {
        // The weighted tier against the row/matrix recurrences directly, over
        // cost sets that are all constructible for Levenshtein and OSA. Each
        // entry additionally declares whether unrestricted Damerau may be
        // built from it, worked out by hand from `2 * transposition >=
        // insertion + deletion`, so that a rejection is asserted rather than
        // skipped.
        let mut rng = SplitMix64(0x0057_0057_0057_0057);
        for (set, verdict) in [
            (
                CostSet {
                    insertion: 2.0,
                    deletion: 1.0,
                    substitution: 1.0,
                    transposition: 1.5,
                },
                Damerau::Admissible, // 3.0 >= 3.0
            ),
            (
                CostSet {
                    insertion: 1.0,
                    deletion: 1.0,
                    substitution: 1.0,
                    transposition: 0.25,
                },
                Damerau::BelowThreshold, // 0.5 < 2.0
            ),
            (
                CostSet {
                    insertion: 1.0,
                    deletion: 1.0,
                    substitution: 0.5,
                    transposition: 3.0,
                },
                Damerau::Admissible, // 6.0 >= 2.0
            ),
            (
                CostSet {
                    insertion: 0.0,
                    deletion: 1.0,
                    substitution: 1.0,
                    transposition: 1.0,
                },
                Damerau::Admissible, // 2.0 >= 1.0
            ),
            (
                CostSet {
                    insertion: 0.1,
                    deletion: 0.2,
                    substitution: 0.15,
                    transposition: 0.2,
                },
                Damerau::Admissible, // 0.4 >= 0.30000000000000004
            ),
        ] {
            for len in [8usize, 40, 130] {
                let a = random_ascii_wide(&mut rng, len);
                let b = substitute_one(&a, len / 2);
                let want = dispatch(&a, &b, |ops| match ops {
                    Operands::Bytes(s, t) => osa_rows(s, t, &set.costs()),
                    Operands::Units(s, t) => osa_rows(s, t, &set.costs()),
                });
                assert_eq!(
                    osa_weighted(&a, &b, &set.osa()).to_bits(),
                    want.to_bits(),
                    "weighted osa len={len} costs={set:?}"
                );
                assert_eq!(
                    levenshtein_weighted(&a, &b, &set.levenshtein()).to_bits(),
                    oracle_plain_rows(&a, &b, &set.costs()).to_bits(),
                    "weighted plain len={len} costs={set:?}"
                );
                // `DamerauCosts::new` accepts only the sets meeting the
                // Lowrance-Wagner threshold; those still exercise the same
                // weighted matrix path. A set the grid declares inadmissible
                // must be refused — `damerau_costs_expecting` asserts both
                // directions, so neither verdict is a silent skip.
                if let Some(damerau) = damerau_costs_expecting(set, verdict) {
                    assert_eq!(
                        damerau_levenshtein_weighted(&a, &b, &damerau).to_bits(),
                        matrix_unrestricted(&a, &b, &set.costs()).to_bits(),
                        "weighted damerau len={len} costs={set:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn no_entry_point_panics_on_any_constructible_cost_set() {
        // `docs/design/distance-contract.md` §1: no function in this crate
        // panics, on any input, under any cost set. Since an inadmissible
        // cost set cannot be constructed, "any cost set" is exactly the grid
        // below — and the sweep is a sweep over *everything the type system
        // still admits*, which is what makes it a proof rather than a
        // sample.
        //
        // Every weighted return is also checked to be a number: §1 promises
        // no `NaN`, never that the result is finite. A cost near `f64::MAX`
        // over a long operand saturates to `+inf`, which orders perfectly
        // well under `total_cmp` and is pinned separately below.
        #[track_caller]
        fn not_nan(value: f64, what: &str) {
            assert!(!value.is_nan(), "{what} returned NaN");
            assert!(value >= 0.0, "{what} returned {value}");
        }

        let operands = [
            "",
            "a",
            "ab",
            "ba",
            "abc",
            "kitten",
            "sitting",
            "aab",
            "baa",
            "café",
            "Москва",
            "😀",
            "a😀b",
            "\u{0}",
            "  ",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        ];
        // Each row declares the verdict `DamerauCosts::new` must return, so
        // that a rejection is asserted rather than skipped. The `f64::MAX`
        // row is `Admissible` — `2 * f64::MAX >= f64::MAX + f64::MAX`, both
        // sides `+inf` — and was silently skipped for as long as the
        // constructor evaluated a rearranged predicate; see
        // `damerau_costs_expecting`. The last row is here so the rejecting
        // branch is exercised too.
        let grid = [
            (
                CostSet {
                    insertion: 1.0,
                    deletion: 1.0,
                    substitution: 1.0,
                    transposition: 1.0,
                },
                Damerau::Admissible, // 2.0 >= 2.0
            ),
            (
                CostSet {
                    insertion: 0.0,
                    deletion: 0.0,
                    substitution: 0.0,
                    transposition: 0.0,
                },
                Damerau::Admissible, // 0.0 >= 0.0
            ),
            (
                CostSet {
                    insertion: 0.5,
                    deletion: 2.0,
                    substitution: 1.5,
                    transposition: 1.25,
                },
                Damerau::Admissible, // 2.5 >= 2.5
            ),
            (
                CostSet {
                    insertion: f64::MAX,
                    deletion: f64::MAX,
                    substitution: f64::MAX,
                    transposition: f64::MAX,
                },
                Damerau::Admissible, // +inf >= +inf
            ),
            (
                CostSet {
                    insertion: f64::MIN_POSITIVE,
                    deletion: f64::MIN_POSITIVE,
                    substitution: f64::MIN_POSITIVE,
                    transposition: f64::MIN_POSITIVE,
                },
                Damerau::Admissible, // 2 * MIN_POSITIVE >= 2 * MIN_POSITIVE
            ),
            (SUB_THRESHOLD_CHEAP_SWAP, Damerau::BelowThreshold), // 1.998 < 2.0
        ];
        for a in operands {
            for b in operands {
                // The unit tier takes no cost argument at all.
                let _ = levenshtein(a, b);
                let _ = damerau_levenshtein(a, b);
                let _ = osa(a, b);
                let _ = levenshtein_search(a, b);
                let _ = damerau_levenshtein_search(a, b);
                let _ = osa_search(a, b);
                for (set, verdict) in grid {
                    let what = &format!("({a:?}, {b:?}) at {set:?}");
                    not_nan(levenshtein_weighted(a, b, &set.levenshtein()), what);
                    not_nan(osa_weighted(a, b, &set.osa()), what);
                    not_nan(
                        levenshtein_search_weighted(a, b, &set.levenshtein()).distance(),
                        what,
                    );
                    not_nan(osa_search_weighted(a, b, &set.osa()).distance(), what);
                    if let Some(damerau) = damerau_costs_expecting(set, verdict) {
                        not_nan(damerau_levenshtein_weighted(a, b, &damerau), what);
                        not_nan(
                            damerau_levenshtein_search_weighted(a, b, &damerau).distance(),
                            what,
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn weighted_results_saturate_to_positive_infinity_and_are_never_nan() {
        // `docs/design/distance-contract.md` §1 and §3.1 ("Numeric limits").
        // The guarantee is **never `NaN`**, not "always finite": a weighted
        // result is a fold of up to `source_units + target_units` costs, so a
        // cost set near `f64::MAX` saturates to `+inf` on a long enough
        // operand. Rejecting such a set at construction would need a
        // length-dependent bound the constructor cannot know, since whether
        // the sum overflows depends on the input, not on the costs.
        //
        // `+inf` is a usable answer where `NaN` is not: it is ordered, so
        // `total_cmp`, `max_by`, `min_by` and `sort_by` over results stay
        // well defined, and it compares greater than every finite distance —
        // which is what a saturated cost means.
        let plain = LevenshteinCosts::new(f64::MAX, f64::MAX, f64::MAX).unwrap();
        let osa_costs = OsaCosts::new(f64::MAX, f64::MAX, f64::MAX, f64::MAX).unwrap();
        let damerau = DamerauCosts::new(f64::MAX, f64::MAX, f64::MAX, f64::MAX).unwrap();

        // Three costs of `f64::MAX` overflow after the second addition, so
        // the shortest saturating operand pair is three units against none.
        assert_eq!(levenshtein_weighted("abc", "", &plain), f64::INFINITY);
        assert_eq!(levenshtein_weighted("", "abc", &plain), f64::INFINITY);

        let long = "a".repeat(64);
        for (name, got) in [
            ("levenshtein", levenshtein_weighted(&long, "", &plain)),
            ("osa", osa_weighted(&long, "", &osa_costs)),
            ("damerau", damerau_levenshtein_weighted(&long, "", &damerau)),
            (
                "levenshtein_search",
                levenshtein_search_weighted(&long, "b", &plain).distance(),
            ),
            (
                "osa_search",
                osa_search_weighted(&long, "b", &osa_costs).distance(),
            ),
            (
                "damerau_search",
                damerau_levenshtein_search_weighted(&long, "b", &damerau).distance(),
            ),
        ] {
            assert_eq!(got, f64::INFINITY, "{name} did not saturate");
            assert!(!got.is_nan(), "{name} returned NaN");
            // Ordered, which is the property `NaN` would destroy.
            assert_eq!(got.total_cmp(&1.0), core::cmp::Ordering::Greater, "{name}");
        }

        // A saturated result is still the *minimum* of the candidates `f64`
        // can express, not a poisoned one: pricing insertion at `f64::MAX`
        // and deletion at `1.0` leaves the cheap script reachable.
        let lopsided = LevenshteinCosts::new(f64::MAX, 1.0, 1.0).unwrap();
        assert_eq!(levenshtein_weighted(&long, "", &lopsided), 64.0);

        // The unit tier cannot saturate at all: its result is a count
        // bounded by `max(source_units, target_units)`.
        assert_eq!(levenshtein(&long, ""), 64);
    }

    #[test]
    fn damerau_affix_trim_matches_the_untrimmed_oracle_exhaustively() {
        // The same exhaustive pin the OSA trim carries, for the unrestricted
        // variant: every pair over a two-letter alphabet up to length 7 and a
        // three-letter alphabet up to length 5, compared against the
        // from-scratch reference running on the UNTRIMMED operands. A
        // transposition straddling either cut is dense in these spaces, so an
        // unsound reduction fails here rather than needing a hand-built
        // witness. The fixture that used to prove trimming *invalid*
        // (`"bb"` vs `"abbb"`, whose common suffix is the whole shorter
        // operand) is inside this space.
        fn enumerate(alphabet: &[u8], max_len: usize) -> Vec<String> {
            let mut out = vec![String::new()];
            let mut frontier = vec![String::new()];
            for _ in 0..max_len {
                let mut next = Vec::new();
                for s in &frontier {
                    for &c in alphabet {
                        next.push(format!("{s}{}", c as char));
                    }
                }
                out.extend(next.iter().cloned());
                frontier = next;
            }
            out
        }

        for (alphabet, max_len) in [(&b"ab"[..], 7usize), (&b"abc"[..], 5usize)] {
            let all = enumerate(alphabet, max_len);
            for a in &all {
                for b in &all {
                    assert_eq!(
                        damerau_levenshtein(a, b),
                        oracle_unrestricted(a, b),
                        "damerau trim mismatch for {a:?} vs {b:?}"
                    );
                }
            }
        }

        // The former "trimming is impossible here" witness, now an ordinary
        // trimmed case: `"bb"`/`"abbb"` trims to `""`/`"ab"`, and the
        // untrimmed answer really is 2.
        let (trimmed_a, trimmed_b) = trim_common_affixes(b"bb".as_slice(), b"abbb".as_slice());
        assert_eq!((trimmed_a, trimmed_b), (b"".as_slice(), b"ab".as_slice()));
        assert_eq!(damerau_levenshtein("bb", "abbb"), 2);
        assert_eq!(damerau_levenshtein("", "ab"), 2);
    }

    #[test]
    fn damerau_affix_trim_straddling_transposition_witnesses_agree() {
        // Hand-built witnesses for the alignment shape the trim could
        // plausibly lose: an adjacent transposition flush against the prefix
        // cut, against the suffix cut, and against both at once — including
        // affixes long enough to push the trimmed residue onto a different
        // kernel than the untrimmed pair would have used.
        let affixes = ["", "a", "ab", "aab", "abcabcabc", &"z".repeat(70)];
        let cores = [
            ("ab", "ba"),
            ("aab", "aba"),
            ("aba", "baa"),
            ("abc", "bac"),
            ("abc", "acb"),
            ("xay", "xya"),
            ("ab", "b"),
            ("ab", "bab"),
            ("bb", "abbb"),
            ("ca", "abc"),
            ("dfcb", "bdffc"),
        ];
        for prefix in affixes {
            for suffix in affixes {
                for (ca, cb) in cores {
                    let a = format!("{prefix}{ca}{suffix}");
                    let b = format!("{prefix}{cb}{suffix}");
                    assert_eq!(
                        damerau_levenshtein(&a, &b),
                        oracle_unrestricted(&a, &b),
                        "straddle mismatch {a:?} vs {b:?}"
                    );
                    assert_eq!(
                        damerau_levenshtein(&b, &a),
                        oracle_unrestricted(&b, &a),
                        "reversed straddle mismatch {b:?} vs {a:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn damerau_affix_trim_agrees_on_near_identical_corpora_at_scale() {
        // The near-identical shape the competitive grid measures — the one
        // the trim exists for — at sizes straddling every dispatch boundary,
        // in ASCII, Cyrillic and astral text, against the untrimmed
        // reference.
        let mut rng = SplitMix64(0x00DE_AD00_BEEF_0001);
        for &len in &[5usize, 6, 17, 40, 63, 64, 65, 66, 130, 256] {
            let ascii = random_ascii_wide(&mut rng, len);
            let unicode = random_unicode_wide(&mut rng, len);
            for base in [ascii, unicode] {
                for &at in &[0usize, 1, len / 2, len - 1] {
                    for variant in [substitute_one(&base, at), delete_one(&base, at)] {
                        assert_eq!(
                            damerau_levenshtein(&base, &variant),
                            oracle_unrestricted(&base, &variant),
                            "damerau len={len} at={at}"
                        );
                        assert_eq!(
                            damerau_levenshtein(&variant, &base),
                            oracle_unrestricted(&variant, &base),
                            "damerau reversed len={len} at={at}"
                        );
                    }
                }
                // Identical operands trim to nothing at all — the empty
                // short-circuit, not the kernels, must answer these.
                assert_eq!(damerau_levenshtein(&base, &base), 0);
                // A transposed pair in the exact middle of two identical
                // halves: the trim leaves a two-unit residue whose whole
                // answer is the transposition.
                let mut units: Vec<char> = base.chars().collect();
                if units.len() >= 2 {
                    let mid = units.len() / 2;
                    units.swap(mid - 1, mid);
                    let swapped: String = units.into_iter().collect();
                    assert_eq!(
                        damerau_levenshtein(&base, &swapped),
                        oracle_unrestricted(&base, &swapped),
                        "damerau mid-swap len={len}"
                    );
                }
                // A shared prefix and suffix around a wholly different
                // middle: the trim must stop exactly where the operands
                // diverge.
                let middle_a = format!("{base}xxxx{base}");
                let middle_b = format!("{base}yyzz{base}");
                assert_eq!(
                    damerau_levenshtein(&middle_a, &middle_b),
                    oracle_unrestricted(&middle_a, &middle_b),
                    "damerau shared-surround len={len}"
                );
            }
        }
    }

    #[test]
    fn osa_single_unit_patterns_take_the_bit_kernel_and_agree() {
        // The gate's lower bound moved from 2 to 1, so the one-unit pattern
        // is now production code rather than a debug-assert-only domain.
        // Checked three ways: the kernel directly against the scalar
        // three-row DP over every single-unit-vs-anything shape a small
        // alphabet can produce, the public entry point (which reaches this
        // case mostly *through* the affix trim), and both operand orders,
        // since the dispatch picks whichever side is shorter as the
        // pattern.
        let alphabet = b"abc";
        for &unit in alphabet {
            let pattern = [unit];
            for len in 0usize..=9 {
                let mut target = vec![alphabet[0]; len];
                for mask in 0..3usize.pow(len.min(6) as u32) {
                    let mut m = mask;
                    for slot in target.iter_mut() {
                        *slot = alphabet[m % 3];
                        m /= 3;
                    }
                    let want = unit_osa_rows(&pattern[..], &target[..]);
                    if !target.is_empty() {
                        assert_eq!(
                            osa_bit_vector(&pattern[..], &target[..]),
                            want,
                            "kernel mismatch {pattern:?} vs {target:?}"
                        );
                        assert_eq!(
                            osa_bit_vector_blocks(&pattern[..], &target[..]),
                            want,
                            "block kernel mismatch {pattern:?} vs {target:?}"
                        );
                    }
                    let a = String::from_utf8(pattern.to_vec()).expect("ascii");
                    let b = String::from_utf8(target.clone()).expect("ascii");
                    assert_eq!(osa(&a, &b), want, "public mismatch {a:?} vs {b:?}");
                    assert_eq!(
                        osa(&b, &a),
                        oracle_osa(&b, &a),
                        "reversed public mismatch {b:?} vs {a:?}"
                    );
                }
            }
        }

        // A one-unit pattern against a long target, which is what the trim
        // leaves behind for a near-identical pair, plus the `char`
        // monomorphization.
        let mut rng = SplitMix64(0x0001_0001_5EED_ABCD);
        for &len in &[1usize, 63, 64, 65, 200, 1000] {
            let target = random_ascii_wide(&mut rng, len);
            for probe in ["a", "z", "0"] {
                assert_eq!(
                    osa(probe, &target),
                    oracle_osa(probe, &target),
                    "long-target mismatch len={len} probe={probe}"
                );
            }
            let unicode = random_unicode_wide(&mut rng, len);
            for probe in ["\u{430}", "\u{4e2d}"] {
                assert_eq!(
                    osa(probe, &unicode),
                    oracle_osa(probe, &unicode),
                    "scalar long-target mismatch len={len}"
                );
            }
        }
    }
}
