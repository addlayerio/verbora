//! Maximum entropy classification by generalised iterative scaling.
//!
//! Ten of the crate's thirteen exported APIs live here, and they are wired
//! together by **shared mutable references**, not by ownership: a
//! [`MaxEntClassifier`] holds the caller's [`FeatureSet`] and [`Sample`], and
//! `train()` mutates both — it appends a correction feature to the feature set
//! and memoises an observed expectation onto every feature. The Rust types use
//! `Rc<RefCell<…>>` for exactly this reason; a port that took ownership would
//! quietly diverge the moment a caller trained twice.
//!
//! # What a naive port gets wrong
//!
//! * **The scores are not probabilities.** `calculateAPriori` returns the
//!   unnormalised weight `∏ αⱼ^fⱼ(x)`; the normalising division is commented out
//!   in the reference. They routinely exceed 1 and do not sum to 1. Normalising
//!   changes every score, the Kullback-Leibler trajectory, and therefore the
//!   iteration at which training stops.
//! * **Training always runs at least once.** The loop is a `do…while`, so
//!   `train(0, x)` performs one full iteration where `for _ in 0..0` performs
//!   none.
//! * **`observedExpectation` memoises and is never invalidated.** Add an
//!   eleventh element to a ten-element sample and retrain: The reference still
//!   reports the expectations of the ten.
//! * **The correction feature cannot be replaced.** It closes over the scaler
//!   that built it, and `addFeature` rejects a second feature with the same
//!   name, so retraining after the sample changed evaluates the correction
//!   against the *first* run's cached feature sums.
//! * **Context keys come from `safe-stable-stringify`**, whose key ordering is
//!   by UTF-16 code unit. See [`DynValue`](crate::DynValue).
//!
//! Every one of those is recorded in `fixtures/classifiers.json`.

mod classifier;
mod context;
mod distribution;
mod element;
mod feature;
mod feature_set;
mod gis_scaler;
mod pos;
mod sample;
mod simple;

pub use classifier::{MaxEntClassifier, RestoreError};
pub use context::Context;
pub use distribution::Distribution;
pub use element::{Element, GenerateFeatures};
pub use feature::{Feature, FeatureFn};
pub use feature_set::FeatureSet;
pub use gis_scaler::{GISScaler, ScalerState};
pub use pos::{MECorpus, MESentence, POSElement, TaggedWord};
pub use sample::Sample;
pub use simple::SEElement;

/// What the maximum-entropy code throws.
///
/// Three of these are behaviours a Rust port would "fix" by accident. They are
/// preserved because the reference's own specs depend on them: `addDocument`
/// really does throw for every input, `new Sample(elements)` really does reject
/// any non-empty array, and a base [`Element`] really has no way to generate
/// features.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MaxEntError {
    /// `Distribution.weight` indexed past the end of the feature list because
    /// `alpha` is longer than the feature set.
    AlphaLongerThanFeatures,
    /// `x.generateFeatures is not a function`: a base [`Element`] was asked to
    /// generate features, which only the subclasses can do.
    ElementCannotGenerateFeatures,
    /// A [`POSElement`] was applied to a context whose data is not a
    /// `{ wordWindow, tagWindow }` object.
    PosContextMissingWindows,
    /// `getClassifications` before `train()`: `this.p` is `undefined`.
    NotTrained,
    /// `classify()` with no classes at all: `scores[-1]` is `undefined`.
    NoClasses,
    /// `new Sample(elements)` with a non-empty array. The constructor calls
    /// `analyse()`, whose `forEach` callback loses its `this` under strict mode
    /// and throws on the very first element.
    SampleAnalyseIsBroken,
    /// `Classifier.prototype.addDocument` invoked with `this === prototype`, so
    /// `this.sample` is `undefined`. It throws for every input, always.
    AddDocumentAlwaysThrows,
}

impl std::fmt::Display for MaxEntError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::AlphaLongerThanFeatures => {
                "Cannot read properties of undefined (reading 'apply')"
            }
            Self::ElementCannotGenerateFeatures => "x.generateFeatures is not a function",
            Self::PosContextMissingWindows => "Cannot read properties of undefined (reading '0')",
            Self::NotTrained => "Cannot read properties of undefined (reading 'calculateAPriori')",
            Self::NoClasses => "Cannot read properties of undefined (reading 'value')",
            Self::SampleAnalyseIsBroken => {
                "Cannot read properties of undefined (reading 'classes')"
            }
            Self::AddDocumentAlwaysThrows => {
                "Cannot read properties of undefined (reading 'addElement')"
            }
        })
    }
}

impl std::error::Error for MaxEntError {}
