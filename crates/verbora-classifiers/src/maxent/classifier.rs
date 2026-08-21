//! `MaxEntClassifier`: a sample, the model fitted to it, and persistence.

use crate::basic::classifier::{Classification, sort_descending};
use crate::dynval::{DynValue, json_stringify_pretty};
use crate::maxent::MaxEntError;
use crate::maxent::gis::{Gis, TrainingReport, fit};
use crate::maxent::model::MaxEntModel;
use crate::maxent::sample::{Event, Sample};

/// A conditional maximum-entropy classifier: training events in, a fitted
/// [`MaxEntModel`] out.
///
/// The classifier **owns** its sample and its model. Training reads the sample
/// and replaces the model; it mutates nothing the caller still holds, and
/// nothing is memoised behind a shared reference, so the type is `Send + Sync`
/// and a fitted model can be shared across threads.
///
/// A context is a set of predicates the caller derives — the classifier applies
/// no tokenizer, no case fold and no stemmer of its own. That is what makes it
/// usable for anything with discrete features, and what distinguishes it from
/// [`Classifier`](crate::Classifier), whose features are stems of a document.
///
/// ```
/// use verbora_classifiers::{Gis, MaxEntClassifier};
///
/// // Part-of-speech disambiguation, with the caller choosing the predicates.
/// // "saw" is ambiguous; the surrounding words are what resolve it.
/// let mut tagger = MaxEntClassifier::new();
/// tagger.add("DT", ["w=the", "w-1=<s>"]);
/// tagger.add("NN", ["w=dog", "w-1=the"]);
/// tagger.add("VB", ["w=saw", "w-1=dog"]);
/// tagger.add("NN", ["w=saw", "w-1=the"]);
///
/// let report = *tagger.train_with(Gis::new(500, 1e-9).unwrap()).unwrap();
/// assert!(report.log_likelihood.is_finite());
///
/// assert_eq!(tagger.classify(["w=saw", "w-1=dog"]).unwrap(), "VB");
/// assert_eq!(tagger.classify(["w=saw", "w-1=the"]).unwrap(), "NN");
/// // The whole distribution is available, so a near-tie is visible rather
/// // than hidden behind a single label.
/// let scores = tagger.get_classifications(["w=saw", "w-1=dog"]).unwrap();
/// assert_eq!(scores[0].label, "VB");
/// assert!((scores.iter().map(|c| c.value).sum::<f64>() - 1.0).abs() < 1e-12);
/// ```
///
/// # The model
///
/// A *context* `x` is the set of predicates one call supplies — to
/// [`Self::add`] as training data, or to [`Self::classify`]/
/// [`Self::get_classifications`] at prediction time. A *feature* is an
/// indicator over one predicate `p` and one outcome `c`:
///
/// ```text
/// f_{p,c}(x, y) = 1  if  p ∈ x  and  y = c
///               = 0  otherwise
/// ```
///
/// [`Self::train`] and [`Self::train_with`] fit the conditional exponential
/// (log-linear) family of Berger, Della Pietra & Della Pietra, *A Maximum
/// Entropy Approach to Natural Language Processing*, Computational Linguistics
/// 22(1), 1996, §4:
///
/// ```text
/// p(y | x) = exp( Σⱼ λⱼ fⱼ(x, y) ) / Z(x),
/// Z(x)     = Σ_{y' ∈ Y} exp( Σⱼ λⱼ fⱼ(x, y') )
/// ```
///
/// Every score [`Self::get_classifications`] and [`MaxEntModel::distribution`]
/// return is a member of that distribution: non-negative, and the scores over
/// one context sum to `1`. There is no unnormalised score anywhere in the
/// public surface.
///
/// The feature set is derived from the training sample and nothing else: a
/// feature `f_{p,c}` exists exactly when some event passed to [`Self::add`]
/// paired predicate `p` with outcome `c`. A predicate that never co-occurred
/// with an outcome contributes no parameter, which is what keeps a fitted
/// model sparse.
///
/// # Training
///
/// Parameters are fitted by generalised iterative scaling — Darroch &
/// Ratcliff, *Generalized iterative scaling for log-linear models*, Annals of
/// Mathematical Statistics 43(5), 1972, in the conditional form Berger et al.
/// give in §6.1. Writing `N` for the sample size:
///
/// ```text
/// Ẽ[fⱼ]  = (1/N) Σᵢ fⱼ(xᵢ, yᵢ)                    the empirical expectation
/// E[fⱼ]  = (1/N) Σᵢ Σ_y p(y | xᵢ) fⱼ(xᵢ, y)       the model expectation
///
/// λⱼ ← λⱼ + (1/C) · log( Ẽ[fⱼ] / E[fⱼ] )
/// ```
///
/// starting from `λ = 0` — the uniform distribution — where `C` is
/// [`TrainingReport::scaling_constant`]. What [`Gis::tolerance`] and
/// [`Gis::max_iterations`] control, and what [`TrainingReport::stop`] reports
/// when fitting stops, are documented on [`Gis`] and [`TrainingReport`]
/// directly.
///
/// The fit is deterministic: expectations and the log-likelihood accumulate
/// over the sample's events in insertion order, so [`Self::train_with`] over
/// an unchanged sample always reproduces the same weights bit for bit. Nothing
/// here is ordered by a hash — outcomes keep the sample's first-appearance
/// order ([`Sample::outcomes`]) and so do a fitted model's predicates
/// ([`MaxEntModel::predicates`]) — which is what makes that reproducibility
/// meaningful rather than coincidental.
///
/// # No `NaN`, no infinities, no sentinels
///
/// No public method on this type or on [`MaxEntModel`] returns a `NaN`, an
/// infinity, or an out-of-band value standing in for one. Every feature is
/// created from an observed event, so its empirical expectation is never zero,
/// and `p(y | x) > 0` for every outcome at finite parameters, so the model
/// expectation a training step divides by is never zero either — the one case
/// a step would still compute something non-finite is discarded rather than
/// accepted, see [`StopReason::NumericalLimit`](crate::StopReason::NumericalLimit).
/// A persisted weight that is not finite is refused at load time rather than
/// let through — see
/// [`ModelDefect::NonFiniteWeight`](crate::ModelDefect::NonFiniteWeight) — and
/// [`MaxEntModel::distribution`] turns even an out-of-range restored weight
/// into an ordinary probability rather than propagating it. [`Self::classify`]
/// has no abstention sentinel either; see its own documentation for how a tie
/// is resolved.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MaxEntClassifier {
    sample: Sample,
    model: Option<MaxEntModel>,
    report: Option<TrainingReport>,
}

impl MaxEntClassifier {
    /// A classifier with no training events.
    pub fn new() -> Self {
        Self::default()
    }

    /// A classifier over an existing sample, not yet trained.
    pub fn from_sample(sample: Sample) -> Self {
        Self {
            sample,
            model: None,
            report: None,
        }
    }

    /// Records one training event.
    ///
    /// Adding an event after training leaves the previous model in place; it is
    /// stale until [`Self::train`] runs again, and [`Self::report`] describes
    /// the fit that produced it, not the sample as it now stands.
    pub fn add(
        &mut self,
        outcome: impl Into<String>,
        predicates: impl IntoIterator<Item = impl Into<String>>,
    ) {
        self.sample.add(outcome, predicates);
    }

    /// Records one already-built training event.
    pub fn push(&mut self, event: Event) {
        self.sample.push(event);
    }

    /// The training events.
    pub fn sample(&self) -> &Sample {
        &self.sample
    }

    /// The fitted model, if [`Self::train`] has run.
    pub fn model(&self) -> Option<&MaxEntModel> {
        self.model.as_ref()
    }

    /// What the last fit did.
    pub fn report(&self) -> Option<&TrainingReport> {
        self.report.as_ref()
    }

    /// Fits the model with [`Gis::default`] settings.
    ///
    /// # Errors
    ///
    /// [`MaxEntError::NoEvents`] when the sample is empty.
    pub fn train(&mut self) -> Result<&TrainingReport, MaxEntError> {
        self.train_with(Gis::default())
    }

    /// Fits the model, replacing any previous one.
    ///
    /// Fitting always starts from the uniform model rather than from the
    /// parameters already held, so training twice over an unchanged sample gives
    /// bit-identical parameters and training after the sample grew gives a fit
    /// to the sample as it now is. Nothing is carried over and nothing is
    /// memoised.
    ///
    /// # Errors
    ///
    /// [`MaxEntError::NoEvents`] when the sample is empty, and
    /// [`MaxEntError::InvalidTolerance`] when `settings.tolerance` is not finite
    /// and non-negative.
    pub fn train_with(&mut self, settings: Gis) -> Result<&TrainingReport, MaxEntError> {
        let (model, report) = fit(&self.sample, settings)?;
        self.model = Some(model);
        self.report = Some(report);
        Ok(self.report.as_ref().expect("just assigned"))
    }

    /// `p(y | x)` for every outcome, sorted by probability, greatest first.
    ///
    /// The sort is stable, so outcomes of exactly equal probability come back in
    /// the sample's outcome order. The values are probabilities: non-negative,
    /// and summing to `1` up to rounding.
    ///
    /// # Errors
    ///
    /// [`MaxEntError::NotTrained`] before [`Self::train`] has run.
    pub fn get_classifications(
        &self,
        predicates: impl IntoIterator<Item = impl AsRef<str>>,
    ) -> Result<Vec<Classification>, MaxEntError> {
        let model = self.model.as_ref().ok_or(MaxEntError::NotTrained)?;
        let probabilities = model.distribution(predicates);
        let mut scores: Vec<Classification> = model
            .outcomes()
            .iter()
            .zip(probabilities)
            .map(|(label, value)| Classification {
                label: label.clone(),
                value,
            })
            .collect();
        sort_descending(&mut scores);
        Ok(scores)
    }

    /// The most probable outcome for a context.
    ///
    /// Outcomes of exactly equal probability are ranked by the sample's outcome
    /// order and the first is returned — there is no abstention sentinel. A
    /// caller that needs to know the answer was a tie reads
    /// [`Self::get_classifications`], which carries every probability.
    ///
    /// A context of predicates the model does not know is scored by the uniform
    /// distribution, so it returns the sample's first outcome.
    ///
    /// # Errors
    ///
    /// [`MaxEntError::NotTrained`] before [`Self::train`] has run.
    pub fn classify(
        &self,
        predicates: impl IntoIterator<Item = impl AsRef<str>>,
    ) -> Result<&str, MaxEntError> {
        let model = self.model.as_ref().ok_or(MaxEntError::NotTrained)?;
        let probabilities = model.distribution(predicates);
        let mut best = 0;
        for (i, p) in probabilities.iter().enumerate() {
            if *p > probabilities[best] {
                best = i;
            }
        }
        Ok(&model.outcomes()[best])
    }

    /// The serialised shape [`Self::save`] writes.
    ///
    /// The object opens with the compatibility stamp (see
    /// [`ArtifactStamp`](crate::ArtifactStamp)), then carries the training
    /// sample, then the fitted model if there is one. Both are written because
    /// they answer different questions: the model is what classifies, and the
    /// sample is what a caller needs in order to refit after adding events.
    ///
    /// Maximum entropy's exposure to a text-pipeline change is narrower than
    /// [`Classifier`](crate::Classifier)'s — predicates are caller-supplied
    /// strings, so no tokenizer, case mapping or stemmer of this crate's stands
    /// between a document and a feature key. The stamp is written and checked
    /// anyway: a caller should not have to know which of this crate's three
    /// classifiers produced a file in order to know whether it is safe to read.
    pub fn to_value(&self) -> DynValue {
        let mut fields = vec![
            (
                crate::stamp::STAMP_PROPERTY.to_owned(),
                crate::stamp::ArtifactStamp::current().to_value(),
            ),
            ("sample".to_owned(), self.sample.to_value()),
        ];
        if let Some(model) = &self.model {
            fields.push(("model".to_owned(), model.to_value()));
        }
        DynValue::Obj(fields)
    }

    /// [`Self::to_value`] as pretty-printed JSON, indented by two — the bytes
    /// [`Self::save`] writes.
    pub fn to_json(&self) -> String {
        json_stringify_pretty(&self.to_value(), 2).expect("an object is never undefined")
    }

    /// Rebuilds a classifier from [`Self::to_json`] output.
    ///
    /// **The fit is restored, not discarded.** A file carrying a model comes back
    /// trained and classifies identically; a file written before training comes
    /// back untrained. The training report is not persisted — it describes a run,
    /// not a model — so [`Self::report`] is `None` until the next
    /// [`Self::train`].
    ///
    /// # Errors
    ///
    /// [`RestoreError::Parse`] for a JSON syntax failure, [`RestoreError::Stamp`]
    /// for a model this build must not read (see
    /// [`ArtifactStamp`](crate::ArtifactStamp)), and [`RestoreError::MaxEnt`]
    /// carrying a [`ModelDefect`](crate::ModelDefect) for a file whose `model`
    /// member does not describe a distribution.
    pub fn restore(json: &str) -> Result<Self, RestoreError> {
        let value = DynValue::parse(json).map_err(RestoreError::Parse)?;
        crate::stamp::verify_stamp(&value).map_err(RestoreError::Stamp)?;
        let sample = match value.get("sample") {
            Some(sample) => Sample::from_value(sample),
            None => Sample::new(),
        };
        let model = match value.get("model") {
            Some(model) => Some(
                MaxEntModel::from_value(model)
                    .map_err(|defect| RestoreError::MaxEnt(MaxEntError::MalformedModel(defect)))?,
            ),
            None => None,
        };
        Ok(Self {
            sample,
            model,
            report: None,
        })
    }

    /// Writes [`Self::to_json`] to `path` as UTF-8.
    ///
    /// # Errors
    ///
    /// Whatever the write fails with.
    pub fn save(&self, path: impl AsRef<std::path::Path>) -> std::io::Result<()> {
        std::fs::write(path, self.to_json())
    }

    /// Reads a saved classifier back.
    ///
    /// # Errors
    ///
    /// [`RestoreError::Io`] if the file cannot be read, and whatever
    /// [`Self::restore`] reports otherwise.
    pub fn load(path: impl AsRef<std::path::Path>) -> Result<Self, RestoreError> {
        let body = std::fs::read_to_string(path).map_err(|e| RestoreError::Io(e.to_string()))?;
        Self::restore(&body)
    }
}

/// Why a saved maximum-entropy classifier could not be revived.
///
/// [`Self::Parse`] and [`Self::Stamp`] are deliberately distinct: the first says
/// the file is damaged, the second says it is intact but was written by another
/// build, and those need opposite responses from a user.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum RestoreError {
    /// The file could not be read. Carries the message rather than the
    /// `io::Error` so the enum stays cheap to compare in tests.
    Io(String),
    /// The bytes were not valid JSON.
    Parse(crate::dynval::ParseError),
    /// The model carries no usable compatibility stamp, or one from another
    /// build. See [`ArtifactStamp`](crate::ArtifactStamp).
    Stamp(crate::stamp::StampError),
    /// The file parsed and its stamp is current, but its contents do not
    /// describe a maximum-entropy model.
    MaxEnt(MaxEntError),
}

impl std::fmt::Display for RestoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "{e}"),
            Self::Parse(e) => write!(f, "{e}"),
            Self::Stamp(e) => write!(f, "{e}"),
            Self::MaxEnt(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for RestoreError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ModelDefect;
    use crate::maxent::gis::StopReason;

    fn trained() -> MaxEntClassifier {
        let mut classifier = MaxEntClassifier::new();
        for _ in 0..2 {
            classifier.add("x", ["a"]);
        }
        classifier.add("y", ["a"]);
        classifier.add("x", ["b"]);
        for _ in 0..2 {
            classifier.add("y", ["b"]);
        }
        classifier
            .train_with(Gis::new(5_000, 0.0).unwrap())
            .expect("six events");
        classifier
    }

    #[test]
    fn classifications_are_a_probability_distribution_in_rank_order() {
        let c = trained();
        let scores = c.get_classifications(["a"]).unwrap();
        assert_eq!(scores[0].label, "x");
        assert!((scores[0].value - 2.0 / 3.0).abs() < 1e-8, "{scores:?}");
        assert!((scores[1].value - 1.0 / 3.0).abs() < 1e-8, "{scores:?}");
        assert!((scores.iter().map(|s| s.value).sum::<f64>() - 1.0).abs() < 1e-12);
        assert_eq!(c.classify(["a"]).unwrap(), "x");
        assert_eq!(c.classify(["b"]).unwrap(), "y");
    }

    #[test]
    fn an_unknown_context_is_uniform_and_resolves_by_outcome_order() {
        let c = trained();
        let scores = c.get_classifications(["never-seen"]).unwrap();
        assert_eq!(scores[0].value, 0.5);
        assert_eq!(scores[1].value, 0.5);
        // Tied, so the sample's first outcome wins — deterministically, and
        // without an out-of-band "cannot decide" value.
        assert_eq!(c.classify(["never-seen"]).unwrap(), "x");
    }

    #[test]
    fn an_untrained_classifier_says_so() {
        let c = MaxEntClassifier::new();
        assert_eq!(c.classify(["a"]).err(), Some(MaxEntError::NotTrained));
        assert_eq!(
            c.get_classifications(["a"]).err(),
            Some(MaxEntError::NotTrained)
        );
        assert_eq!(
            MaxEntClassifier::new().train().err(),
            Some(MaxEntError::NoEvents)
        );
    }

    #[test]
    fn training_twice_over_an_unchanged_sample_is_bit_identical() {
        let mut c = trained();
        let first: Vec<f64> = c
            .model()
            .unwrap()
            .predicates()
            .flat_map(|p| {
                ["x", "y"]
                    .into_iter()
                    .filter_map(|o| c.model().unwrap().weight(p, o))
                    .collect::<Vec<_>>()
            })
            .collect();
        c.train_with(Gis::new(5_000, 0.0).unwrap()).unwrap();
        let second: Vec<f64> = c
            .model()
            .unwrap()
            .predicates()
            .flat_map(|p| {
                ["x", "y"]
                    .into_iter()
                    .filter_map(|o| c.model().unwrap().weight(p, o))
                    .collect::<Vec<_>>()
            })
            .collect();
        assert_eq!(
            first.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
            second.iter().map(|v| v.to_bits()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn retraining_after_the_sample_grew_fits_the_sample_as_it_now_is() {
        let mut c = trained();
        let before = c.classify(["b"]).unwrap().to_owned();
        assert_eq!(before, "y");
        for _ in 0..20 {
            c.add("x", ["b"]);
        }
        // The stale model still answers until the refit — nothing is silently
        // invalidated — and the refit sees every event.
        assert_eq!(c.classify(["b"]).unwrap(), "y");
        c.train_with(Gis::new(5_000, 0.0).unwrap()).unwrap();
        assert_eq!(c.classify(["b"]).unwrap(), "x");
        assert_eq!(c.sample().len(), 26);
    }

    #[test]
    fn a_trained_classifier_round_trips_and_still_classifies_the_same() {
        let c = trained();
        let json = c.to_json();
        let revived = MaxEntClassifier::restore(&json).unwrap();
        assert_eq!(revived.sample(), c.sample());
        assert_eq!(revived.model(), c.model());
        assert_eq!(
            revived.report(),
            None,
            "a report describes a run, not a model"
        );
        assert_eq!(
            revived.get_classifications(["a"]).unwrap(),
            c.get_classifications(["a"]).unwrap()
        );
        assert_eq!(revived.to_json(), json, "the round trip is byte-stable");
    }

    #[test]
    fn an_untrained_classifier_round_trips_as_untrained() {
        let mut c = MaxEntClassifier::new();
        c.add("x", ["a"]);
        let revived = MaxEntClassifier::restore(&c.to_json()).unwrap();
        assert_eq!(revived.model(), None);
        assert_eq!(revived.sample(), c.sample());
    }

    #[test]
    fn a_file_whose_model_is_not_a_distribution_is_refused() {
        let json = trained().to_json();
        let damaged = json.replace("\"outcomes\": [", "\"outcomes\": {\"no\": 1}, \"gone\": [");
        assert_eq!(
            MaxEntClassifier::restore(&damaged).err(),
            Some(RestoreError::MaxEnt(MaxEntError::MalformedModel(
                ModelDefect::Outcomes
            )))
        );
    }

    #[test]
    fn an_event_can_be_pushed_ready_made() {
        let mut c = MaxEntClassifier::new();
        c.push(Event::new("x", ["a", "a"]));
        c.push(Event::new("y", ["b"]));
        assert_eq!(c.sample().len(), 2);
        assert_eq!(c.sample().events()[0].predicates(), ["a"]);
        assert_eq!(c.sample().outcomes(), ["x", "y"]);
    }

    #[test]
    fn every_error_says_what_went_wrong_in_the_models_own_terms() {
        assert_eq!(
            MaxEntError::NoEvents.to_string(),
            "the sample holds no training events"
        );
        assert_eq!(
            MaxEntError::NotTrained.to_string(),
            "the classifier has not been trained"
        );
        assert!(
            MaxEntError::InvalidTolerance(-1.0)
                .to_string()
                .contains("finite and non-negative")
        );
        assert_eq!(
            RestoreError::MaxEnt(MaxEntError::NoEvents).to_string(),
            "the sample holds no training events"
        );
        assert_eq!(
            RestoreError::Io("no such file".to_owned()).to_string(),
            "no such file"
        );
        assert!(
            !RestoreError::Stamp(crate::StampError::Missing)
                .to_string()
                .is_empty()
        );
    }

    #[test]
    fn a_classifier_is_send_and_sync() {
        fn assert_shareable<T: Send + Sync>() {}
        assert_shareable::<MaxEntClassifier>();
    }

    #[test]
    fn saving_and_loading_through_a_file_preserves_the_fit() {
        let c = trained();
        let path = std::env::temp_dir().join(format!(
            "verbora-maxent-{}-{:?}.json",
            std::process::id(),
            std::thread::current().id()
        ));
        c.save(&path).expect("a writable temp directory");
        let loaded = MaxEntClassifier::load(&path).expect("just written");
        std::fs::remove_file(&path).ok();
        assert_eq!(loaded.model(), c.model());
        assert!(matches!(
            MaxEntClassifier::load(path.join("not-a-directory")),
            Err(RestoreError::Io(_))
        ));
    }

    // -- the compatibility stamp --------------------------------------------

    /// Replaces the pretty-printed `_verbora` member with `replacement`.
    ///
    /// `to_value` writes the stamp first and `json_stringify_pretty` indents by
    /// two, so the model proper begins at the next top-level member.
    fn with_stamp(json: &str, replacement: &str) -> String {
        assert!(json.starts_with("{\n  \"_verbora\":"), "{json}");
        let tail = json
            .find("\n  \"sample\":")
            .expect("the stamp is followed by the sample");
        format!("{{\n  \"_verbora\": {replacement},{}", &json[tail..])
    }

    #[test]
    fn a_model_from_another_build_is_refused() {
        let c = trained();
        let current = crate::stamp::ArtifactStamp::current();
        let (major, minor, update) = current.unicode;
        let json = with_stamp(
            &c.to_json(),
            &format!(
                r#"{{"schema": {}, "unicode": "{}.{minor}.{update}"}}"#,
                current.schema,
                major + 1
            ),
        );
        let Err(RestoreError::Stamp(crate::StampError::Incompatible(mismatch))) =
            MaxEntClassifier::restore(&json)
        else {
            panic!("a foreign Unicode version must be refused");
        };
        assert_eq!(mismatch.found.unicode, (major + 1, minor, update));
        assert_eq!(mismatch.expected, current);
    }

    /// A model saved before stamping existed carries no version information at
    /// all, so it cannot be validated and is refused under its own variant.
    #[test]
    fn a_model_saved_before_stamping_is_refused_as_unstamped() {
        let json = trained().to_json();
        let tail = json
            .find("\n  \"sample\":")
            .expect("the stamp is followed by the sample");
        let pre_stamp = format!("{{{}", &json[tail..]);
        assert_eq!(
            MaxEntClassifier::restore(&pre_stamp).err(),
            Some(RestoreError::Stamp(crate::StampError::Missing))
        );
    }

    /// A damaged file and a stale one are different problems and are reported as
    /// different errors.
    #[test]
    fn a_corrupt_file_is_distinguishable_from_an_incompatible_one() {
        assert!(matches!(
            MaxEntClassifier::restore("{not json"),
            Err(RestoreError::Parse(_))
        ));
        let damaged = with_stamp(&trained().to_json(), r#""17.0.0""#);
        assert_eq!(
            MaxEntClassifier::restore(&damaged).err(),
            Some(RestoreError::Stamp(crate::StampError::Malformed))
        );
    }

    #[test]
    fn the_report_names_why_a_fit_stopped() {
        let mut c = trained();
        let report = *c.train_with(Gis::new(1, 0.0).unwrap()).unwrap();
        assert_eq!(report.stop, StopReason::MaxIterations);
        assert_eq!(report.iterations, 1);
        let report = *c.train_with(Gis::new(10_000, 1e-12).unwrap()).unwrap();
        assert_eq!(report.stop, StopReason::Converged);
        assert!(report.improvement <= 1e-12);
        assert!(report.log_likelihood < 0.0 && report.log_likelihood.is_finite());
    }

    /// A predicate observed with exactly one outcome has no finite
    /// maximum-likelihood weight: the constraint asks for `p = 1`, which needs
    /// `λ → ∞`. Generalised iterative scaling approaches it logarithmically, so
    /// the fit is usable but never meets a tight tolerance, and the report says
    /// so instead of claiming convergence.
    #[test]
    fn a_predicate_seen_with_one_outcome_is_reported_as_unconverged() {
        let mut c = MaxEntClassifier::new();
        c.add("x", ["a"]);
        c.add("y", ["a"]);
        c.add("y", ["only-y"]);
        let report = *c.train_with(Gis::new(2_000, 1e-12).unwrap()).unwrap();
        assert_eq!(report.stop, StopReason::MaxIterations);
        assert_eq!(report.iterations, 2_000);
        assert!(report.improvement > 0.0, "still climbing: {report:?}");
        assert!(
            report.log_likelihood.is_finite(),
            "the parameters stay finite: {report:?}"
        );
        let p = c.get_classifications(["only-y"]).unwrap();
        assert_eq!(p[0].label, "y");
        assert!(p[0].value > 0.99 && p[0].value < 1.0, "{p:?}");
    }
}
