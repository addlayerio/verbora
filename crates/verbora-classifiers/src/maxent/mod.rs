//! Conditional maximum-entropy classification, trained by generalised
//! iterative scaling.
//!
//! [`MaxEntClassifier`](crate::MaxEntClassifier) is the public entry point; its
//! own documentation states the model, the training objective and the
//! guarantees a caller needs. What follows here is implementation rationale
//! that does not change any of that.
//!
//! # The slack feature, and why there is not one
//!
//! GIS's derivation needs `Σⱼ fⱼ(x, y) = C` for every `(x, y)`, where `C` is
//! [`TrainingReport::scaling_constant`](crate::TrainingReport::scaling_constant).
//! The classical repair (Berger, Della Pietra & Della Pietra, *A Maximum
//! Entropy Approach to Natural Language Processing*, Computational Linguistics
//! 22(1), 1996, §6.1) adds a *slack* — or *correction* — feature
//! `f_#(x, y) = C − Σⱼ fⱼ(x, y)`, which is non-negative by the definition of
//! `C`. This module computes `C` and reasons about `f_#` only inside the
//! convergence argument; it is not a model parameter, because in a conditional
//! model it cannot be one:
//!
//! ```text
//! Σⱼ λⱼ fⱼ(x,y) + λ_# ( C − Σⱼ fⱼ(x,y) )  =  λ_# C  +  Σⱼ (λⱼ − λ_#) fⱼ(x,y)
//! ```
//!
//! `λ_# C` does not depend on `y`, so it cancels in `Z(x)`. A model with a
//! weighted slack feature is therefore *the same distribution* as one without,
//! at shifted parameters — the slack feature adds no expressive power and is
//! not identifiable, which is why nothing here stores one. Curran & Clark,
//! *Investigating GIS and Smoothing for Maximum Entropy Taggers* (EACL 2003),
//! report the same omission with no loss.
//!
//! Storing `f_#` rather than only reasoning about it this way would need
//! memoising `C − Σⱼ fⱼ` per training element, and would answer `0` for any
//! `(context, outcome)` pair absent from the sample — a pair the expectation
//! sum evaluates on every iteration — which would make `Σ f` something other
//! than `C` and the fitted parameters something other than GIS's. Nothing here
//! stores it, which is what keeps that failure mode unreachable.
//!
//! # Ordering inside a fitted model
//!
//! [`MaxEntModel::predicates`](crate::MaxEntModel::predicates) is
//! first-appearance order and is part of the public contract. The order of the
//! `(outcome, weight)` pairs *within* one predicate's row is not part of it —
//! it is whatever order that predicate's outcomes were first seen in — because
//! nothing in the public surface iterates a row independently of
//! [`MaxEntModel::weight`](crate::MaxEntModel::weight), which looks a feature
//! up by key rather than by position.

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
