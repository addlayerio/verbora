//! `MaxEntClassifier`: the user-facing wrapper around the scaler.

use std::cell::RefCell;
use std::rc::Rc;

use crate::basic::classifier::{Classification, sort_descending};
use crate::dynval::{DynValue, json_stringify_pretty};
use crate::maxent::MaxEntError;
use crate::maxent::context::Context;
use crate::maxent::distribution::Distribution;
use crate::maxent::element::Element;
use crate::maxent::feature_set::FeatureSet;
use crate::maxent::gis_scaler::GISScaler;
use crate::maxent::sample::Sample;

/// A maximum-entropy classifier over a shared feature set and sample.
///
/// Both are held by reference and **mutated by training**: the correction
/// feature is appended to the caller's feature set, and every feature gains a
/// memoised observed expectation. That sharing is the reference's design, not an
/// accident, and several of its recorded behaviours depend on it.
///
/// ```
/// use std::cell::RefCell;
/// use std::rc::Rc;
/// use verbora_classifiers::{Context, FeatureSet, MaxEntClassifier, SEElement, Sample};
///
/// let mut sample = Sample::new();
/// let zero = Rc::new(Context::of_str("0"));
/// let one = Rc::new(Context::of_str("1"));
/// for _ in 0..3 { sample.add_element(SEElement::new("x", Rc::clone(&zero))); }
/// for _ in 0..3 { sample.add_element(SEElement::new("y", Rc::clone(&zero))); }
/// sample.add_element(SEElement::new("x", Rc::clone(&one)));
/// for _ in 0..3 { sample.add_element(SEElement::new("y", Rc::clone(&one))); }
///
/// let mut features = FeatureSet::new();
/// sample.generate_features(&mut features).unwrap();
///
/// let mut classifier = MaxEntClassifier::new(
///     Rc::new(RefCell::new(features)),
///     Rc::new(RefCell::new(sample)),
/// );
/// classifier.train(20, 0.01).unwrap();
/// assert_eq!(classifier.classify(&zero).unwrap(), "x");
/// assert_eq!(classifier.classify(&one).unwrap(), "y");
/// // An unseen context scores every class equally, and the classifier abstains.
/// assert_eq!(classifier.classify(&Rc::new(Context::of_str("2"))).unwrap(), "");
/// ```
#[derive(Debug)]
pub struct MaxEntClassifier {
    features: Rc<RefCell<FeatureSet>>,
    sample: Rc<RefCell<Sample>>,
    scaler: Option<GISScaler>,
    p: Option<Rc<Distribution>>,
}

impl MaxEntClassifier {
    /// A classifier over `features` and `sample`.
    pub fn new(features: Rc<RefCell<FeatureSet>>, sample: Rc<RefCell<Sample>>) -> Self {
        Self {
            features,
            sample,
            scaler: None,
            p: None,
        }
    }

    /// The shared feature set.
    pub fn features(&self) -> &Rc<RefCell<FeatureSet>> {
        &self.features
    }

    /// The shared sample.
    pub fn sample(&self) -> &Rc<RefCell<Sample>> {
        &self.sample
    }

    /// The scaler from the last [`Self::train`].
    pub fn scaler(&self) -> Option<&GISScaler> {
        self.scaler.as_ref()
    }

    /// The trained distribution, `classifier.p`.
    pub fn distribution(&self) -> Option<&Rc<Distribution>> {
        self.p.as_ref()
    }

    /// `addElement(x)`.
    ///
    /// Invalidates nothing: the memoised observed expectations, the scaler's
    /// feature sums and the distribution's weight tables all survive, so
    /// retraining after this produces a model built partly from stale numbers.
    /// That is the reference's behaviour and the fixtures record it.
    pub fn add_element(&self, x: impl Into<Rc<Element>>) {
        self.sample.borrow_mut().add_element(x);
    }

    /// `train(maxIterations, minImprovement)`.
    ///
    /// # Errors
    ///
    /// Propagates [`MaxEntError::AlphaLongerThanFeatures`] from the scaler.
    pub fn train(&mut self, max_iterations: i64, min_improvement: f64) -> Result<(), MaxEntError> {
        let mut scaler = GISScaler::new(Rc::clone(&self.features), Rc::clone(&self.sample));
        self.p = Some(scaler.run(max_iterations, min_improvement)?);
        self.scaler = Some(scaler);
        Ok(())
    }

    /// `getClassifications(b)`: one score per class, in **class insertion
    /// order** — not sorted.
    ///
    /// The values are unnormalised weights, not probabilities.
    ///
    /// # Errors
    ///
    /// [`MaxEntError::NotTrained`] when `train` has not run and the sample has
    /// at least one class. With no classes at all the reference returns an empty
    /// array without ever touching `p`, and so does this.
    pub fn get_classifications(&self, b: &Rc<Context>) -> Result<Vec<Classification>, MaxEntError> {
        let classes: Vec<String> = self.sample.borrow().classes().to_vec();
        if classes.is_empty() {
            return Ok(Vec::new());
        }
        let p = self.p.as_ref().ok_or(MaxEntError::NotTrained)?;
        let mut scores = Vec::with_capacity(classes.len());
        for a in classes {
            let x = Element::new(a.clone(), Rc::clone(b));
            scores.push(Classification {
                label: a,
                value: p.calculate_a_priori(&x)?,
            });
        }
        Ok(scores)
    }

    /// `classify(b)`.
    ///
    /// Sorts the scores descending — stably, so ties keep class insertion order
    /// — and then returns the **empty string** if the highest and lowest scores
    /// are exactly equal, because a classifier that cannot discriminate declines
    /// to answer. Otherwise the top label, which among tied maxima is the class
    /// that appeared first in the training data.
    ///
    /// # Errors
    ///
    /// [`MaxEntError::NoClasses`] when there are no classes: The reference reads
    /// `scores[-1].value` and throws.
    pub fn classify(&self, b: &Rc<Context>) -> Result<String, MaxEntError> {
        let mut scores = self.get_classifications(b)?;
        if scores.is_empty() {
            return Err(MaxEntError::NoClasses);
        }
        sort_descending(&mut scores);
        let min = scores[scores.len() - 1].value;
        let max = scores[0].value;
        // `===`, so an all-NaN score vector does *not* count as a tie.
        if min == max {
            Ok(String::new())
        } else {
            Ok(scores[0].label.clone())
        }
    }

    /// `addDocument(context, classification, ElementClass)`.
    ///
    /// # Errors
    ///
    /// Always [`MaxEntError::AddDocumentAlwaysThrows`]. The reference's body is
    /// `Classifier.prototype.addElement(new ElementClass(...))`, which invokes
    /// the method with `this === Classifier.prototype`, whose `sample` is
    /// `undefined`. Nothing in the reference calls it, and the obviously
    /// intended behaviour is [`Self::add_element`].
    pub fn add_document(
        &self,
        _context: &Rc<Context>,
        _classification: &str,
    ) -> Result<(), MaxEntError> {
        Err(MaxEntError::AddDocumentAlwaysThrows)
    }

    /// The serialised shape `save()` writes.
    ///
    /// The object opens with the compatibility stamp (see [`ArtifactStamp`](crate::ArtifactStamp)).
    /// After it, the feature set and sample appear three times over — once at
    /// the top level, once under `scaler` and once under `p` — because the
    /// reference follows the references and the graph has no cycle.
    ///
    /// Maximum entropy's exposure to a boundary change is narrower than
    /// [`Classifier`](crate::Classifier)'s — [`Self::restore`] regenerates the
    /// features from the sample's elements and discards the trained parameters,
    /// so nothing derived survives the round trip — but the *sample* it revives
    /// is text produced by whatever pipeline built it, and a caller should not
    /// have to know which of this crate's three classifiers is exposed in order
    /// to know whether their file is safe. Every persisted artifact this crate
    /// writes is stamped, and every load checks.
    pub fn to_value(&self) -> DynValue {
        let mut fields = vec![
            (
                crate::stamp::STAMP_PROPERTY.to_owned(),
                crate::stamp::ArtifactStamp::current().to_value(),
            ),
            ("features".to_owned(), self.features.borrow().to_value()),
            ("sample".to_owned(), self.sample.borrow().to_value()),
        ];
        if let Some(scaler) = &self.scaler {
            fields.push(("scaler".to_owned(), scaler.to_value()));
        }
        if let Some(p) = &self.p {
            fields.push(("p".to_owned(), p.to_value()));
        }
        DynValue::Obj(fields)
    }

    /// `JSON.stringify(classifier, null, 2)` — what `save()` writes to disk.
    pub fn to_json(&self) -> String {
        json_stringify_pretty(&self.to_value(), 2).expect("an object is never undefined")
    }

    /// Rebuilds a classifier from saved bytes, as `load()` does.
    ///
    /// **The trained parameters are discarded.** `load` reads only
    /// `sample.elements`, revives each one through the caller's element
    /// constructor, and regenerates the features from them; `alpha`, the scaler
    /// and the distribution in the file are ignored. The returned classifier is
    /// therefore untrained, and the reference's own spec retrains after loading.
    ///
    /// # Errors
    ///
    /// [`RestoreError::Parse`] for a JSON syntax failure, [`RestoreError::Stamp`]
    /// for a model this build must not read (see [`ArtifactStamp`](crate::ArtifactStamp)), and
    /// [`RestoreError::MaxEnt`] for an element that cannot generate features.
    pub fn restore(
        json: &str,
        revive: impl FnMut(&str, Rc<Context>) -> Rc<Element>,
    ) -> Result<Self, RestoreError> {
        let value = DynValue::parse(json).map_err(RestoreError::Parse)?;
        crate::stamp::verify_stamp(&value).map_err(RestoreError::Stamp)?;
        let sample_value = value.get("sample").cloned().unwrap_or(DynValue::Undefined);
        let sample = Sample::from_value(&sample_value, revive);
        let mut features = FeatureSet::new();
        sample
            .generate_features(&mut features)
            .map_err(RestoreError::MaxEnt)?;
        Ok(Self::new(
            Rc::new(RefCell::new(features)),
            Rc::new(RefCell::new(sample)),
        ))
    }

    /// `save(filename)`: writes [`Self::to_json`] to `path` as UTF-8.
    ///
    /// # Errors
    ///
    /// Whatever the write fails with. The reference's `save` is asynchronous
    /// and reports through a callback.
    pub fn save(&self, path: impl AsRef<std::path::Path>) -> std::io::Result<()> {
        std::fs::write(path, self.to_json())
    }

    /// `load(filename, ElementClass)`: reads a saved classifier back.
    ///
    /// Like [`Self::restore`], the result is **untrained**.
    ///
    /// # Errors
    ///
    /// [`RestoreError::Io`] if the file cannot be read, and whatever
    /// [`Self::restore`] reports otherwise.
    pub fn load(
        path: impl AsRef<std::path::Path>,
        revive: impl FnMut(&str, Rc<Context>) -> Rc<Element>,
    ) -> Result<Self, RestoreError> {
        let body = std::fs::read_to_string(path).map_err(|e| RestoreError::Io(e.to_string()))?;
        Self::restore(&body, revive)
    }
}

/// Why a saved maximum-entropy classifier could not be revived.
///
/// [`Self::Parse`] and [`Self::Stamp`] are deliberately distinct: the first says
/// the file is damaged, the second says it is intact but was written by another
/// build, and those need opposite responses from a user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RestoreError {
    /// The file could not be read. Carries the message rather than the
    /// `io::Error` so the enum stays cheap to compare in tests.
    Io(String),
    /// The bytes were not valid JSON.
    Parse(crate::dynval::ParseError),
    /// The model carries no usable compatibility stamp, or one from another
    /// build. See [`ArtifactStamp`](crate::ArtifactStamp).
    Stamp(crate::stamp::StampError),
    /// The revived elements could not regenerate their features.
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
    use crate::SEElement;

    fn reference() -> MaxEntClassifier {
        let mut sample = Sample::new();
        let zero = Rc::new(Context::of_str("0"));
        let one = Rc::new(Context::of_str("1"));
        for _ in 0..3 {
            sample.add_element(SEElement::new("x", Rc::clone(&zero)));
        }
        for _ in 0..3 {
            sample.add_element(SEElement::new("y", Rc::clone(&zero)));
        }
        sample.add_element(SEElement::new("x", Rc::clone(&one)));
        for _ in 0..3 {
            sample.add_element(SEElement::new("y", Rc::clone(&one)));
        }
        let mut features = FeatureSet::new();
        sample.generate_features(&mut features).unwrap();
        MaxEntClassifier::new(
            Rc::new(RefCell::new(features)),
            Rc::new(RefCell::new(sample)),
        )
    }

    #[test]
    fn classifications_match_the_recorded_golden_values() {
        let mut c = reference();
        c.train(20, 0.01).unwrap();
        let scores = c
            .get_classifications(&Rc::new(Context::of_str("0")))
            .unwrap();
        assert_eq!(scores[0].label, "x");
        assert_eq!(scores[0].value, 0.844_285_714_285_714_2);
        assert_eq!(scores[1].value, 0.777_505_117_061_545);
        let scores = c
            .get_classifications(&Rc::new(Context::of_str("1")))
            .unwrap();
        assert_eq!(scores[1].value, 1.885_178_571_428_571_1);
    }

    #[test]
    fn an_unseen_context_makes_the_classifier_abstain() {
        let mut c = reference();
        c.train(20, 0.01).unwrap();
        assert_eq!(c.classify(&Rc::new(Context::of_str("2"))).unwrap(), "");
    }

    #[test]
    fn training_twice_is_idempotent_on_an_unchanged_sample() {
        let mut c = reference();
        c.train(20, 0.01).unwrap();
        let first = c.distribution().unwrap().alpha();
        c.train(20, 0.01).unwrap();
        assert_eq!(c.distribution().unwrap().alpha(), first);
        // The correction feature was not added a second time.
        assert_eq!(c.features().borrow().size(), 3);
    }

    #[test]
    fn untrained_get_classifications_reports_not_trained() {
        let c = reference();
        assert_eq!(
            c.get_classifications(&Rc::new(Context::of_str("0"))),
            Err(MaxEntError::NotTrained)
        );
    }

    #[test]
    fn a_classifier_with_no_classes_returns_nothing_and_cannot_classify() {
        let c = MaxEntClassifier::new(
            Rc::new(RefCell::new(FeatureSet::new())),
            Rc::new(RefCell::new(Sample::new())),
        );
        assert_eq!(
            c.get_classifications(&Rc::new(Context::of_str("0")))
                .unwrap(),
            vec![]
        );
        assert_eq!(
            c.classify(&Rc::new(Context::of_str("0"))),
            Err(MaxEntError::NoClasses)
        );
    }

    #[test]
    fn add_document_always_throws() {
        let c = reference();
        assert_eq!(
            c.add_document(&Rc::new(Context::of_str("0")), "x"),
            Err(MaxEntError::AddDocumentAlwaysThrows)
        );
    }

    #[test]
    fn restore_regenerates_features_and_returns_an_untrained_classifier() {
        let mut c = reference();
        c.train(20, 0.01).unwrap();
        let json = c.to_json();
        let revived =
            MaxEntClassifier::restore(&json, |a, b| Rc::new(SEElement::new(a, b))).unwrap();
        assert_eq!(revived.sample().borrow().size(), 10);
        assert_eq!(
            revived.features().borrow().size(),
            2,
            "no correction feature"
        );
        assert!(revived.distribution().is_none(), "alpha is not restored");
    }

    // -- the compatibility stamp --------------------------------------------

    fn revive(a: &str, b: Rc<Context>) -> Rc<Element> {
        Rc::new(SEElement::new(a, b))
    }

    /// Replaces the pretty-printed `_verbora` member with `replacement`.
    ///
    /// `to_value` writes the stamp first and `json_stringify_pretty` indents by
    /// two, so the model proper begins at the next top-level member.
    fn with_stamp(json: &str, replacement: &str) -> String {
        assert!(json.starts_with("{\n  \"_verbora\":"), "{json}");
        let tail = json
            .find("\n  \"features\":")
            .expect("the stamp is followed by features");
        format!("{{\n  \"_verbora\": {replacement},{}", &json[tail..])
    }

    #[test]
    fn a_model_from_another_build_is_refused() {
        let c = reference();
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
            MaxEntClassifier::restore(&json, revive)
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
        let c = reference();
        let json = c.to_json();
        let tail = json
            .find("\n  \"features\":")
            .expect("the stamp is followed by features");
        let pre_stamp = format!("{{{}", &json[tail..]);
        assert_eq!(
            MaxEntClassifier::restore(&pre_stamp, revive).err(),
            Some(RestoreError::Stamp(crate::StampError::Missing))
        );
    }

    /// A damaged file and a stale one are different problems and are reported as
    /// different errors.
    #[test]
    fn a_corrupt_file_is_distinguishable_from_an_incompatible_one() {
        assert!(matches!(
            MaxEntClassifier::restore("{not json", revive),
            Err(RestoreError::Parse(_))
        ));
        let damaged = with_stamp(&reference().to_json(), r#""17.0.0""#);
        assert_eq!(
            MaxEntClassifier::restore(&damaged, revive).err(),
            Some(RestoreError::Stamp(crate::StampError::Malformed))
        );
    }
}
