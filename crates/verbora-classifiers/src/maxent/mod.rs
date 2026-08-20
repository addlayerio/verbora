//! Conditional maximum-entropy classification, trained by generalised
//! iterative scaling.
//!
//! # The model
//!
//! A *context* `x` is a **set of contextual predicates** — short strings the
//! caller derives from whatever it is classifying. An *outcome* `y` is a label.
//! A *feature* is an indicator over the two:
//!
//! ```text
//! f_{p,c}(x, y) = 1  if  p ∈ x  and  y = c
//!               = 0  otherwise
//! ```
//!
//! The model is the conditional exponential (log-linear) family of Berger,
//! Della Pietra & Della Pietra, *A Maximum Entropy Approach to Natural Language
//! Processing*, Computational Linguistics 22(1), 1996, §4:
//!
//! ```text
//! p(y | x) = exp( Σⱼ λⱼ fⱼ(x, y) ) / Z(x),
//! Z(x)     = Σ_{y' ∈ Y} exp( Σⱼ λⱼ fⱼ(x, y') )
//! ```
//!
//! Every score this module returns is a member of that distribution: a genuine
//! conditional probability in `[0, 1]`, and the scores over one context sum to
//! `1`. There is no unnormalised score anywhere in the public surface.
//!
//! The feature set is **derived from the training sample and nothing else**: a
//! feature `f_{p,c}` exists exactly when some training event observed predicate
//! `p` together with outcome `c`. Predicates that never co-occur with an outcome
//! contribute no parameter, which is what keeps the parameter vector sparse.
//!
//! # Training: generalised iterative scaling
//!
//! Parameters are fitted by GIS — Darroch & Ratcliff, *Generalized iterative
//! scaling for log-linear models*, Annals of Mathematical Statistics 43(5),
//! 1972 — in the conditional form Berger et al. give in §6.1. Writing `N` for
//! the number of training events and `p̃` for the empirical distribution:
//!
//! ```text
//! Ẽ[fⱼ]  = (1/N) Σᵢ fⱼ(xᵢ, yᵢ)                    the empirical expectation
//! E[fⱼ]  = (1/N) Σᵢ Σ_y p(y | xᵢ) fⱼ(xᵢ, y)       the model expectation
//!
//! λⱼ ← λⱼ + (1/C) · log( Ẽ[fⱼ] / E[fⱼ] )
//! ```
//!
//! starting from `λ = 0`, which is the uniform distribution. `C` is the GIS
//! scaling constant: the largest number of features active on any
//! `(context, outcome)` pair the objective evaluates, i.e.
//!
//! ```text
//! C = max over training events (xᵢ) and outcomes (y) of  Σⱼ fⱼ(xᵢ, y)
//! ```
//!
//! See [`TrainingReport::scaling_constant`].
//!
//! ## The slack feature, and why there is not one
//!
//! GIS's derivation needs `Σⱼ fⱼ(x, y) = C` for every `(x, y)`. The classical
//! repair (Berger et al. §6.1) adds a *slack* — or *correction* — feature
//! `f_#(x, y) = C − Σⱼ fⱼ(x, y)`, which is non-negative by the definition of
//! `C`. Verbora computes `C` and uses `f_#` **only inside the convergence
//! argument**; it is not a model parameter, because in a conditional model it
//! cannot be one:
//!
//! ```text
//! Σⱼ λⱼ fⱼ(x,y) + λ_# ( C − Σⱼ fⱼ(x,y) )  =  λ_# C  +  Σⱼ (λⱼ − λ_#) fⱼ(x,y)
//! ```
//!
//! `λ_# C` does not depend on `y`, so it cancels in `Z(x)`. A model with a
//! weighted slack feature is therefore *the same distribution* as one without,
//! at shifted parameters — the slack feature adds no expressive power and is
//! not identifiable. Omitting it is also what Curran & Clark, *Investigating GIS
//! and Smoothing for Maximum Entropy Taggers* (EACL 2003), report doing with no
//! loss.
//!
//! This is not a detail. A slack feature that is *stored* rather than computed
//! is how the previous implementation of this module went wrong: it memoised
//! `C − Σⱼ fⱼ` per training element and returned `0` for any
//! `(context, outcome)` pair absent from the sample — pairs the expectation sum
//! evaluates on every iteration — so `Σ f` was not `C` and the fitted
//! parameters were not GIS's.
//!
//! ## Convergence
//!
//! GIS monotonically increases the conditional log-likelihood
//!
//! ```text
//! L(λ) = (1/N) Σᵢ log p(yᵢ | xᵢ)
//! ```
//!
//! and that quantity — not a derived one — is what [`Gis::tolerance`] tests.
//! Training stops when one iteration raises `L` by at most the tolerance, or
//! when [`Gis::max_iterations`] iterations have run, whichever comes first, and
//! [`TrainingReport::stop`] says which. `max_iterations` is an exact count:
//! `0` performs no iterations and leaves the uniform model, and `n` performs at
//! most `n`.
//!
//! At a fixed point the defining property of the maximum-entropy solution holds:
//! `E[fⱼ] = Ẽ[fⱼ]` for every feature. `constraints_are_satisfied_at_convergence`
//! asserts it directly, recomputing both expectations from their definitions
//! rather than from the fitting code.
//!
//! One shape never converges, and it is a property of the data rather than of
//! the algorithm: a predicate observed with **exactly one** outcome sets a
//! constraint of `p = 1`, whose maximum-likelihood weight is unbounded. GIS
//! approaches it logarithmically — the weight grows like `log(iterations)` —
//! so the fit is perfectly usable but never meets a tight tolerance. That is
//! reported as [`StopReason::MaxIterations`] rather than dressed up as
//! convergence. Smoothing the constraint away is not this module's job; a
//! caller who needs it adds events.
//!
//! # No `NaN`, no infinities, no sentinels
//!
//! Unlike the rest of this crate — see the crate-level "`NaN` is computable"
//! section, which is about [`BayesEngine`](crate::BayesEngine) and
//! [`Classifier`](crate::Classifier) — **no maximum-entropy API returns a
//! `NaN`, an infinity, or an out-of-band value standing in for one.** Four
//! facts together make that structural rather than aspirational:
//!
//! * Every feature is created from an observed event, so `Ẽ[fⱼ] ≥ 1/N > 0`.
//!   The GIS ratio never divides by an empirical expectation of zero.
//! * `p(y | x) > 0` for every outcome at finite parameters, and every feature
//!   fires on the event that created it, so `E[fⱼ] > 0` too.
//! * A weight is finite when training writes it and when
//!   [`MaxEntModel`] parses it; a sum of finitely many finite `f64` can reach
//!   `±∞` but never `NaN`, and [`MaxEntModel::distribution`] gives `±∞` scores
//!   a defined limit rather than propagating them.
//! * Should an iteration nonetheless compute a non-finite ratio, training keeps
//!   the last finite parameters and reports [`StopReason::NumericalLimit`]
//!   instead of storing the result.
//!
//! `classify` likewise has no abstention sentinel. Outcomes that tie are ranked
//! by the sample's outcome order and the first is returned; a caller who needs
//! to detect the tie reads [`MaxEntClassifier::get_classifications`], which
//! carries the whole distribution.
//!
//! # Floating point is part of the contract
//!
//! Summation order is specified, because floating-point addition is not
//! associative and a near-tie flips when it changes:
//!
//! * A context's scores accumulate **over its predicates in the order the
//!   caller supplied them**, after duplicates are dropped keeping the first
//!   occurrence.
//! * Both expectations and the log-likelihood accumulate **over the sample's
//!   events in insertion order**, and within an event over outcomes in the
//!   sample's outcome order.
//! * Normalisation subtracts the greatest score before exponentiating, and sums
//!   the exponentials in outcome order.
//! * [`exp`](crate::exp) and [`log`](crate::log) are Verbora's own in-tree
//!   FDLIBM ports, so a model fitted on one target scores identically on
//!   another.
//!
//! # Determinism
//!
//! Nothing here is ordered by a hash. Outcomes are held in first-appearance
//! order, predicates in first-appearance order, and each predicate's parameters
//! in the order its outcomes were first seen with it. The same events in the
//! same order always produce bit-identical parameters.

mod classifier;
mod gis;
mod model;
mod sample;

pub use classifier::{MaxEntClassifier, RestoreError};
pub use gis::{Gis, StopReason, TrainingReport};
pub use model::{MaxEntModel, ModelDefect};
pub use sample::{Event, Sample};

/// What a maximum-entropy operation could not do.
///
/// Every variant names a condition of *this model* — an empty sample, an
/// unfitted classifier, a setting outside its domain, a persisted model that
/// does not describe a distribution. None of them reports the internal state of
/// some other runtime.
#[derive(Debug, Clone, PartialEq)]
pub enum MaxEntError {
    /// [`MaxEntClassifier::train`] was called on a sample holding no events.
    ///
    /// There is no empirical distribution to constrain the model to, so there
    /// is nothing to fit — as distinct from fitting something uninformative.
    NoEvents,
    /// A prediction was requested from a classifier that has not been trained.
    NotTrained,
    /// [`Gis::tolerance`] was not a finite, non-negative number.
    ///
    /// The tolerance is compared against an increase in log-likelihood, which
    /// GIS makes non-negative; a negative or non-finite threshold could never
    /// be met and would silently turn `max_iterations` into the only stopping
    /// rule.
    InvalidTolerance(f64),
    /// A persisted model parsed as JSON but does not describe a distribution
    /// this crate can evaluate.
    MalformedModel(ModelDefect),
}

impl std::fmt::Display for MaxEntError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoEvents => f.write_str("the sample holds no training events"),
            Self::NotTrained => f.write_str("the classifier has not been trained"),
            Self::InvalidTolerance(t) => {
                write!(
                    f,
                    "a convergence tolerance must be finite and non-negative, not {t}"
                )
            }
            Self::MalformedModel(defect) => write!(f, "malformed maximum-entropy model: {defect}"),
        }
    }
}

impl std::error::Error for MaxEntError {}
