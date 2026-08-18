//! The reference `Classifier` shell shared by Bayes and logistic regression.
//!
//! Everything here is stateful and order-dependent, and three of the behaviours
//! look like bugs a port would want to fix. They are not optional:
//!
//! * **The feature vector's layout is a reference object's enumeration order.**
//!   `textToFeatures` walks `for (const feature in this.features)`, so adding a
//!   token that *looks like an integer* hoists it to slot 0 and shifts every
//!   previously learned index. See [`crate::ordmap`].
//! * **`removeDocument` deletes feature entries rather than decrementing them**,
//!   removes the *last* of several matching documents, and leaves `lastAdded`
//!   untouched — so a subsequent `train()` silently skips documents and only
//!   `retrain()` recovers.
//! * **A document that tokenises to nothing is dropped in silence**, including
//!   `""` and any all-stop-word string.

use std::sync::Arc;

use rustc_hash::{FxHashMap, FxHashSet};
use verbora_core::whitespace::is_whitespace;

use crate::dynval::DynValue;
use crate::ordmap::OrderedMap;
use crate::stemmer::{Stemmer, default_stemmer};

/// One scored class, as `getClassifications` returns it.
#[derive(Debug, Clone, PartialEq)]
pub struct Classification {
    /// The class label.
    pub label: String,
    /// The engine's score. Bayes returns a probability-like quantity; logistic
    /// regression returns a sigmoid output in `(0, 1)`.
    pub value: f64,
}

/// A stored training document.
#[derive(Debug, Clone, PartialEq)]
pub struct Document {
    /// The class label, trimmed if it was supplied as a string.
    pub label: String,
    /// The token sequence, already stemmed if it came from a string.
    pub text: Vec<String>,
}

/// What a classifier is asked to classify.
///
/// The distinction is observable: a string is tokenised and stemmed (and
/// filtered against the stop-word list), while a token slice is used *verbatim*
/// — no lowercasing, no stemming, no stop-word removal, and `keepStops` has no
/// effect on it at all.
#[derive(Debug, Clone, Copy)]
pub enum Observation<'a> {
    /// Raw text, to be tokenised and stemmed.
    Text(&'a str),
    /// Tokens, used exactly as given.
    Tokens(&'a [String]),
}

impl<'a> From<&'a str> for Observation<'a> {
    fn from(s: &'a str) -> Self {
        Self::Text(s)
    }
}

impl<'a> From<&'a String> for Observation<'a> {
    fn from(s: &'a String) -> Self {
        Self::Text(s)
    }
}

impl<'a> From<&'a [String]> for Observation<'a> {
    fn from(t: &'a [String]) -> Self {
        Self::Tokens(t)
    }
}

impl<'a> From<&'a Vec<String>> for Observation<'a> {
    fn from(t: &'a Vec<String>) -> Self {
        Self::Tokens(t)
    }
}

/// Emitted during [`Classifier::train_with`].
///
/// The reference emits these synchronously through `EventEmitter`, and the stream
/// differs between the two classifiers: Bayes trains incrementally from
/// `lastAdded`, whereas logistic regression discards its engine and re-emits
/// every document on every call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrainingEvent {
    /// One document was handed to the engine.
    TrainedWithDocument {
        /// Index of the document in `docs`.
        index: usize,
        /// `docs.length` as captured at the start of this `train()` call.
        total: usize,
    },
    /// The engine's own `train()` returned. Carries the literal `true` the
    /// the reference emits.
    DoneTraining,
}

/// What the `apparatus` engines throw.
///
/// Two of these are bare the reference strings rather than `Error` objects — code
/// in the wild catches them by value — and the rest are `TypeError`s raised by
/// dereferencing state that training would have created.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClassifierError {
    /// `throw "Not Trained"` — `getClassifications` returned nothing.
    NotTrained,
    /// `TypeError: Cannot read properties of undefined (reading 'length')`,
    /// from asking an untrained logistic regression for classifications.
    LogisticRegressionNotTrained,
    /// `TypeError: Cannot read properties of undefined (reading '0')`, from
    /// `$M([])` when logistic regression is trained with no examples at all.
    NoExamples,
    /// `throw 'unable to find minimum'` from `descendGradient`.
    UnableToFindMinimum,
}

impl std::fmt::Display for ClassifierError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::NotTrained => "Not Trained",
            Self::LogisticRegressionNotTrained => {
                "Cannot read properties of undefined (reading 'length')"
            }
            Self::NoExamples => "Cannot read properties of undefined (reading '0')",
            Self::UnableToFindMinimum => "unable to find minimum",
        })
    }
}

impl std::error::Error for ClassifierError {}

/// The `apparatus` engine behind a [`Classifier`].
pub trait Engine: Default + Clone {
    /// Whether `train()` throws the engine away and starts over.
    ///
    /// `true` for logistic regression, whose matrices must have consistent
    /// widths, and `false` for Bayes, which accumulates. This single constant is
    /// the whole of `LogisticRegressionClassifier`'s `train()` override.
    const RESETS_ON_TRAIN: bool;

    /// Records one 0/1 observation under `label`.
    fn add_example(&mut self, observation: &[u8], label: &str);

    /// Fits the model. A no-op for Bayes.
    fn fit(&mut self) -> Result<(), ClassifierError>;

    /// Scores `observation` against every known class, best first.
    fn classifications(&self, observation: &[u8]) -> Result<Vec<Classification>, ClassifierError>;

    /// The engine's own serialised shape, as `JSON.stringify` would produce it.
    fn to_value(&self) -> DynValue;

    /// Rebuilds an engine from its serialised shape, for `restore`.
    fn from_value(value: &DynValue) -> Self;
}

/// `String.prototype.trim`.
///
/// Trims the reference's `WhiteSpace | LineTerminator` set, which is not Rust's
/// `char::is_whitespace`: `U+FEFF` is trimmed and `U+0085` is not.
fn trim_units(s: &str) -> &str {
    s.trim_matches(is_whitespace)
}

/// Why a saved classifier could not be read back.
#[derive(Debug)]
pub enum LoadError {
    /// The file could not be read.
    Io(std::io::Error),
    /// The file was not valid JSON.
    Parse(crate::dynval::ParseError),
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "{e}"),
            Self::Parse(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for LoadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::Parse(e) => Some(e),
        }
    }
}

/// A document classifier: vocabulary, documents, and an engine.
///
/// The two concrete classifiers are the type aliases
/// [`BayesClassifier`](crate::BayesClassifier) and
/// [`LogisticRegressionClassifier`](crate::LogisticRegressionClassifier).
pub struct Classifier<E: Engine> {
    engine: E,
    docs: Vec<Document>,
    features: OrderedMap<f64>,
    stemmer: Arc<dyn Stemmer + Send + Sync>,
    last_added: usize,
    /// `None` until `setOptions` is called — the property genuinely does not
    /// exist before then, and its absence is visible in the serialised JSON.
    keep_stops: Option<bool>,
}

impl<E: Engine> std::fmt::Debug for Classifier<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Classifier")
            .field("docs", &self.docs.len())
            .field("features", &self.features.len())
            .field("lastAdded", &self.last_added)
            .field("keepStops", &self.keep_stops)
            .finish_non_exhaustive()
    }
}

impl<E: Engine> Default for Classifier<E> {
    fn default() -> Self {
        Self::new()
    }
}

impl<E: Engine> Classifier<E> {
    /// A classifier using the default English Porter stemmer.
    pub fn new() -> Self {
        Self::with_stemmer(default_stemmer())
    }

    /// A classifier using `stemmer` for string documents.
    pub fn with_stemmer(stemmer: Arc<dyn Stemmer + Send + Sync>) -> Self {
        Self {
            engine: E::default(),
            docs: Vec::new(),
            features: OrderedMap::new(),
            stemmer,
            last_added: 0,
            keep_stops: None,
        }
    }

    /// `setOptions({ keepStops })`.
    ///
    /// The reference is `this.keepStops = !!(options.keepStops)`, so any truthy
    /// value becomes `true`. It affects string documents and string observations
    /// only — token slices bypass the stemmer, and therefore the stop-word list,
    /// entirely.
    pub fn set_keep_stops(&mut self, keep_stops: bool) {
        self.keep_stops = Some(keep_stops);
    }

    /// Whether stop words are kept. `None` before `setOptions` was ever called.
    pub fn keep_stops(&self) -> Option<bool> {
        self.keep_stops
    }

    /// The stored documents, in insertion order.
    pub fn docs(&self) -> &[Document] {
        &self.docs
    }

    /// The feature vocabulary and its counts.
    pub fn features(&self) -> &OrderedMap<f64> {
        &self.features
    }

    /// The feature names in the order `textToFeatures` emits them.
    pub fn feature_order(&self) -> Vec<&str> {
        self.features.enumeration_order()
    }

    /// How many documents have already been handed to the engine.
    pub fn last_added(&self) -> usize {
        self.last_added
    }

    /// The underlying engine.
    pub fn engine(&self) -> &E {
        &self.engine
    }

    /// Installs a pre-configured engine, for constructors that take engine
    /// options (Bayes's smoothing is the only one).
    pub(crate) fn set_engine(&mut self, engine: E) {
        self.engine = engine;
    }

    /// Tokenises an observation, or takes its tokens verbatim.
    ///
    /// TODO(perf): stem memoization — a per-classifier token→stem cache used by
    /// `add_document`/`remove_document` — is the single largest remaining win
    /// for `bayes_train` (Porter stemming is ~65% of it on repeated-vocabulary
    /// corpora, and `stem_token` is pure for the default English stemmer). It
    /// waits on a cached tokenize-and-stem entry point in `verbora-stemmers`;
    /// the `classify` path must stay uncached because it takes `&self`.
    fn resolve(&self, observation: Observation<'_>) -> Vec<String> {
        match observation {
            Observation::Text(s) => self
                .stemmer
                .tokenize_and_stem(s, self.keep_stops.unwrap_or(false)),
            Observation::Tokens(t) => t.to_vec(),
        }
    }

    /// `addDocument(text, classification)`.
    ///
    /// A string label is trimmed. A document that tokenises to nothing is
    /// dropped without a word — `""`, `"the a of"` and an empty token slice all
    /// leave the classifier untouched.
    pub fn add_document<'a>(&mut self, text: impl Into<Observation<'a>>, classification: &str) {
        let label = trim_units(classification).to_owned();
        let text = self.resolve(text.into());
        if text.is_empty() {
            return;
        }
        for token in &text {
            // `(this.features[token] || 0) + 1`: a truthiness test, so a count
            // that somehow reached 0 would restart at 1 rather than at 1 + 1.
            let previous = self
                .features
                .get(token)
                .copied()
                .filter(|v| *v != 0.0 && !v.is_nan())
                .unwrap_or(0.0);
            self.features.insert(token.clone(), previous + 1.0);
        }
        self.docs.push(Document { label, text });
    }

    /// `removeDocument(text, classification)`.
    ///
    /// Scans every document and keeps the **last** index that matches, so with
    /// duplicates it is the final copy that goes. The matched document's tokens
    /// are then `delete`d from the vocabulary outright — not decremented — so a
    /// token still present in a surviving document loses its feature slot and
    /// can never be a feature again unless re-added.
    pub fn remove_document<'a>(&mut self, text: impl Into<Observation<'a>>, classification: &str) {
        let text = self.resolve(text.into());
        let joined = text.join(" ");
        let mut pos = None;
        for (i, doc) in self.docs.iter().enumerate() {
            if doc.text.join(" ") == joined && doc.label == classification {
                pos = Some(i);
            }
        }
        let Some(pos) = pos else { return };
        self.docs.remove(pos);
        for token in &text {
            self.features.remove(token);
        }
        // `lastAdded` is deliberately left alone, matching the reference.
    }

    /// `textToFeatures(observation)`: a 0/1 vector, one slot per known feature.
    ///
    /// The slot order is [`OrderedMap::enumeration_order`], recomputed on every call — which
    /// is exactly why adding an integer-like token invalidates a trained model.
    ///
    /// An `Observation::Tokens` input is already an owned `&[String]` the
    /// caller holds; going through [`Self::resolve`] would `.to_vec()` it
    /// (cloning every token) just to borrow `&str`s back out a line later. This
    /// builds the presence set straight from the borrow instead — `resolve`
    /// still owns the `Observation::Text` path, which genuinely needs the
    /// stemmer's fresh `Vec<String>`.
    pub fn text_to_features<'a>(&self, observation: impl Into<Observation<'a>>) -> Vec<u8> {
        match observation.into() {
            Observation::Text(s) => {
                let tokens = self
                    .stemmer
                    .tokenize_and_stem(s, self.keep_stops.unwrap_or(false));
                self.features_for(tokens.iter().map(String::as_str))
            }
            Observation::Tokens(t) => self.features_for(t.iter().map(String::as_str)),
        }
    }

    /// The shared tail of [`Self::text_to_features`]: a 0/1 vector over
    /// [`OrderedMap::enumeration_order`], one slot per known feature.
    fn features_for<'b>(&self, tokens: impl Iterator<Item = &'b str>) -> Vec<u8> {
        let present: FxHashSet<&str> = tokens.collect();
        self.features
            .enumeration_order()
            .into_iter()
            .map(|f| u8::from(present.contains(f)))
            .collect()
    }

    /// `train()`, discarding the events.
    pub fn train(&mut self) -> Result<(), ClassifierError> {
        self.train_with(|_| {})
    }

    /// `train()`, reporting each `trainedWithDocument` and `doneTraining`.
    ///
    /// For Bayes this adds only the documents past `lastAdded`; for logistic
    /// regression it first throws the engine away and re-adds everything, which
    /// is why the two emit different event streams for the same script.
    pub fn train_with(
        &mut self,
        mut listener: impl FnMut(TrainingEvent),
    ) -> Result<(), ClassifierError> {
        if E::RESETS_ON_TRAIN {
            self.last_added = 0;
            self.engine = E::default();
        }
        let total = self.docs.len();
        if self.last_added < total {
            // The reference calls `textToFeatures` per document, which recomputes
            // the enumeration order — an O(vocabulary) allocation and sort — for
            // every document in the loop. But nothing inside the loop touches
            // `features` (only `addDocument`/`removeDocument` do), so the order,
            // and with it the whole token→slot layout, is loop-invariant.
            // Computing both once and marking each document's tokens into a
            // reused buffer produces byte-identical observations: a slot is 1
            // exactly when the document contains that feature, which is the same
            // membership test `text_to_features` runs per slot.
            let order = self.features.enumeration_order();
            let slot_of: FxHashMap<&str, usize> = order
                .iter()
                .enumerate()
                .map(|(slot, &feature)| (feature, slot))
                .collect();
            let mut observation = vec![0u8; order.len()];
            for i in self.last_added..total {
                observation.fill(0);
                for token in &self.docs[i].text {
                    // A token absent from the vocabulary (never added, or
                    // deleted by `removeDocument`) contributes nothing, exactly
                    // as it fails `text_to_features`'s presence test.
                    if let Some(&slot) = slot_of.get(token.as_str()) {
                        observation[slot] = 1;
                    }
                }
                self.engine.add_example(&observation, &self.docs[i].label);
                listener(TrainingEvent::TrainedWithDocument { index: i, total });
                self.last_added += 1;
            }
        }
        self.engine.fit()?;
        listener(TrainingEvent::DoneTraining);
        Ok(())
    }

    /// `retrain()`: a fresh engine, then a full `train()`.
    ///
    /// The engine is reconstructed with **no arguments**, so a Bayes classifier
    /// built with a custom smoothing silently reverts to 1.0.
    pub fn retrain(&mut self) -> Result<(), ClassifierError> {
        self.retrain_with(|_| {})
    }

    /// `retrain()`, reporting training events.
    pub fn retrain_with(
        &mut self,
        listener: impl FnMut(TrainingEvent),
    ) -> Result<(), ClassifierError> {
        self.engine = E::default();
        self.last_added = 0;
        self.train_with(listener)
    }

    /// `getClassifications(observation)`, best-scoring class first.
    pub fn get_classifications<'a>(
        &self,
        observation: impl Into<Observation<'a>>,
    ) -> Result<Vec<Classification>, ClassifierError> {
        self.engine
            .classifications(&self.text_to_features(observation))
    }

    /// `classify(observation)`: the label of the best-scoring class.
    ///
    /// # Errors
    ///
    /// [`ClassifierError::NotTrained`] when there are no classes to score —
    /// `apparatus` throws the bare string `"Not Trained"` here.
    pub fn classify<'a>(
        &self,
        observation: impl Into<Observation<'a>>,
    ) -> Result<String, ClassifierError> {
        let scores = self.get_classifications(observation)?;
        scores
            .into_iter()
            .next()
            .map(|c| c.label)
            .ok_or(ClassifierError::NotTrained)
    }

    /// [`Classifier::classify`], fanned out across a `rayon` thread pool.
    /// Requires the `parallel` feature.
    ///
    /// # Why this exists
    ///
    /// A trained [`Classifier`] is read-only from `classify`'s point of view —
    /// `text_to_features`, `get_classifications` and `classify` all take
    /// `&self` — and every field involved (`docs`, `features`, the engine, and
    /// the `stemmer`, which is stored as `Arc<dyn Stemmer + Send + Sync>` for
    /// exactly this reason) is `Send + Sync`. Classifying many independent
    /// texts against the same trained classifier is therefore embarrassingly
    /// parallel with zero coordination cost between calls. This function is
    /// exactly `texts.par_iter().map(|t| self.classify(*t)).collect()` — a
    /// thin fan-out over the existing sequential primitive, not a second
    /// implementation of it. `get_classifications` and `text_to_features` are
    /// untouched; if you need those shapes in parallel, apply the same
    /// `par_iter().map(...)` pattern at your own call site (see
    /// `site/performance/parallelism.md`).
    ///
    /// # When to reach for it vs. the sequential loop
    ///
    /// Reach for this only when the *batch*, not the single classification, is
    /// the unit of work — for example, labelling every document in a large
    /// corpus offline. A single `classify` call costs on the order of 13 µs
    /// for a Bayes classifier trained on a few dozen documents (this crate's
    /// own `bayes/predict/classify` benchmark), and a `rayon` task costs on
    /// the order of a microsecond to schedule, so a handful of texts is close
    /// to the break-even point — measure before reaching for this on small
    /// batches, and prefer a plain `texts.iter().map(Classifier::classify)`
    /// loop there. Batches in the thousands amortise the scheduling cost
    /// easily; see this crate's `bayes/predict_batch` benchmark for measured
    /// crossover numbers.
    ///
    /// # Allocation behaviour
    ///
    /// One `Vec<Result<String, ClassifierError>>` sized to `texts.len()` for
    /// the output, plus whatever [`Classifier::classify`] itself allocates per
    /// text (a fresh token `Vec<String>` from the stemmer, a 0/1 feature
    /// vector, and the engine's own per-call scoring allocations). No
    /// additional buffering, no locking, no per-call thread-pool construction
    /// — this uses whichever global `rayon` pool is already installed (or
    /// `rayon`'s default one), so pool configuration remains the caller's
    /// responsibility, not this crate's.
    ///
    /// # Order and errors
    ///
    /// Output order matches input order — `results[i]` is
    /// `self.classify(texts[i])` — via `rayon`'s order-preserving `map` +
    /// `collect`. Each element carries its own `Result`, exactly as a
    /// sequential `texts.iter().map(|t| self.classify(*t)).collect::<Vec<_>>()`
    /// would: one text's [`ClassifierError`] does not abort the others.
    #[cfg(feature = "parallel")]
    pub fn par_classify_batch(&self, texts: &[&str]) -> Vec<Result<String, ClassifierError>>
    where
        E: Sync,
    {
        use rayon::prelude::*;
        texts.par_iter().map(|text| self.classify(*text)).collect()
    }

    /// The classifier as `JSON.stringify` would serialise it.
    ///
    /// The key order, the presence of the `EventEmitter` internals, the empty
    /// `stemmer` object and the `Threads: null` sentinel are all part of the
    /// recorded shape that the reference's own `load()` reads back.
    pub fn to_value(&self) -> DynValue {
        let mut fields = vec![
            ("_events".to_owned(), DynValue::Obj(vec![])),
            ("_eventsCount".to_owned(), DynValue::Num(0.0)),
            ("classifier".to_owned(), self.engine.to_value()),
            (
                "docs".to_owned(),
                DynValue::Arr(
                    self.docs
                        .iter()
                        .map(|d| {
                            DynValue::Obj(vec![
                                ("label".to_owned(), DynValue::Str(d.label.clone())),
                                (
                                    "text".to_owned(),
                                    DynValue::Arr(
                                        d.text.iter().cloned().map(DynValue::Str).collect(),
                                    ),
                                ),
                            ])
                        })
                        .collect(),
                ),
            ),
            (
                "features".to_owned(),
                DynValue::Obj(
                    self.features
                        .ordered_entries()
                        .into_iter()
                        .map(|(k, v)| (k.to_owned(), DynValue::Num(*v)))
                        .collect(),
                ),
            ),
            // PorterStemmer has only function properties, so it serialises to {}.
            ("stemmer".to_owned(), DynValue::Obj(vec![])),
            (
                "lastAdded".to_owned(),
                DynValue::Num(self.last_added as f64),
            ),
            // The optional `webworker-threads` dependency is absent in every
            // standard install, so the parallel-training handle is always null.
            ("Threads".to_owned(), DynValue::Null),
        ];
        if let Some(keep) = self.keep_stops {
            fields.push(("keepStops".to_owned(), DynValue::Bool(keep)));
        }
        DynValue::Obj(fields)
    }

    /// `JSON.stringify(classifier)` — what `save()` writes to disk.
    pub fn to_json(&self) -> String {
        self.to_value()
            .json_stringify()
            .expect("an object is never undefined")
    }

    /// `restore(JSON.parse(data))`: rebuilds a classifier from saved bytes.
    ///
    /// Uses the default stemmer, matching `stemmer || PorterStemmer`. Note that
    /// The reference's restored object permanently loses its parallel-training
    /// methods (they are functions, which JSON drops); the Rust port has no such
    /// methods to lose.
    ///
    /// # Errors
    ///
    /// Returns the parse error if `json` is not valid JSON. Fields that are
    /// missing or of the wrong type fall back to their empty values, exactly as
    /// the reference does by leaving them `undefined`.
    pub fn restore(json: &str) -> Result<Self, crate::dynval::ParseError> {
        Ok(Self::from_value(&DynValue::parse(json)?))
    }

    /// `save(filename)`: writes [`Self::to_json`] to `path` as UTF-8.
    ///
    /// The reference's `save` is asynchronous and reports through a callback,
    /// and swallows the error entirely when no callback is given; this returns
    /// it instead.
    ///
    /// # Errors
    ///
    /// Whatever the write fails with.
    pub fn save(&self, path: impl AsRef<std::path::Path>) -> std::io::Result<()> {
        std::fs::write(path, self.to_json())
    }

    /// `load(filename)`: reads a saved classifier back.
    ///
    /// # Errors
    ///
    /// [`LoadError::Io`] if the file cannot be read, [`LoadError::Parse`] if it
    /// is not valid JSON. The reference does not catch the parse failure at all
    /// — it surfaces as an uncaught throw inside the `fs` callback.
    pub fn load(path: impl AsRef<std::path::Path>) -> Result<Self, LoadError> {
        let body = std::fs::read_to_string(path).map_err(LoadError::Io)?;
        Self::restore(&body).map_err(LoadError::Parse)
    }

    /// Rebuilds a classifier from an already-parsed serialised shape.
    pub fn from_value(value: &DynValue) -> Self {
        let mut out = Self::new();
        if let Some(engine) = value.get("classifier") {
            out.engine = E::from_value(engine);
        }
        if let Some(DynValue::Arr(docs)) = value.get("docs") {
            for doc in docs {
                let label = doc.get("label").map(DynValue::to_text).unwrap_or_default();
                let text = match doc.get("text") {
                    Some(DynValue::Arr(items)) => items.iter().map(DynValue::to_text).collect(),
                    _ => Vec::new(),
                };
                out.docs.push(Document { label, text });
            }
        }
        if let Some(DynValue::Obj(fields)) = value.get("features") {
            for (k, v) in fields {
                out.features
                    .insert(k.clone(), if let DynValue::Num(n) = v { *n } else { 0.0 });
            }
        }
        if let Some(DynValue::Num(n)) = value.get("lastAdded") {
            out.last_added = *n as usize;
        }
        if let Some(DynValue::Bool(b)) = value.get("keepStops") {
            out.keep_stops = Some(*b);
        }
        out
    }
}

/// Sorts scores descending by value, the way the reference engine's `sort` does.
///
/// Two details are load-bearing. The sort must be **stable**, so exact ties come
/// back in the engine's own enumeration order — three classes all scoring 0.5
/// are recorded as `['2','10','x']`, which is that order and not any sorted one.
/// And the comparator must treat a `NaN` difference as "equal": the reference engine reads a `NaN`
/// comparator result as "not greater than zero" and leaves the pair alone, while
/// `partial_cmp().unwrap()` would panic.
pub(crate) fn sort_descending(scores: &mut [Classification]) {
    scores.sort_by(|x, y| {
        let d = y.value - x.value;
        if d > 0.0 {
            std::cmp::Ordering::Greater
        } else if d < 0.0 {
            std::cmp::Ordering::Less
        } else {
            std::cmp::Ordering::Equal
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::BayesClassifier;

    #[test]
    fn trim_units_handles_the_whitespace_set() {
        assert_eq!(trim_units("  padded  "), "padded");
        assert_eq!(trim_units("\t\n padded \r\n"), "padded");
        assert_eq!(trim_units("\u{feff}padded\u{feff}"), "padded");
        // U+0085 NEXT LINE is not the reference whitespace.
        assert_eq!(trim_units("\u{85}x\u{85}"), "\u{85}x\u{85}");
        assert_eq!(trim_units("   "), "");
    }

    #[test]
    fn empty_documents_are_dropped() {
        let mut c = BayesClassifier::new();
        c.add_document("", "A");
        c.add_document("the a of", "A");
        c.add_document(&Vec::<String>::new(), "A");
        assert_eq!(c.docs().len(), 0);
        assert_eq!(c.features().len(), 0);
    }

    #[test]
    fn labels_are_trimmed() {
        let mut c = BayesClassifier::new();
        c.add_document(&vec!["alpha".to_owned()], "  padded  ");
        assert_eq!(c.docs()[0].label, "padded");
    }

    #[test]
    fn remove_document_takes_the_last_match_and_deletes_features() {
        let mut c = BayesClassifier::new();
        let ab = vec!["alpha".to_owned(), "beta".to_owned()];
        let g = vec!["gamma".to_owned()];
        c.add_document(&ab, "L");
        c.add_document(&g, "M");
        c.add_document(&ab, "L");
        assert_eq!(c.features().len(), 3);
        c.remove_document(&ab, "L");
        // The surviving 'alpha beta' document keeps its tokens, but the tokens
        // are gone from the vocabulary entirely.
        assert_eq!(c.docs().len(), 2);
        assert_eq!(c.docs()[1].text, g);
        assert_eq!(c.feature_order(), vec!["gamma"]);
    }

    #[test]
    fn integer_like_tokens_are_hoisted() {
        let mut c = BayesClassifier::new();
        c.add_document(
            &vec!["zebra".to_owned(), "42".to_owned(), "appl".to_owned()],
            "A",
        );
        assert_eq!(c.feature_order(), vec!["42", "zebra", "appl"]);
        assert_eq!(c.text_to_features(&vec!["42".to_owned()]), vec![1, 0, 0]);
    }

    #[test]
    fn stable_sort_keeps_ties_in_engine_order() {
        let mut scores = vec![
            Classification {
                label: "2".into(),
                value: 0.5,
            },
            Classification {
                label: "10".into(),
                value: 0.5,
            },
            Classification {
                label: "x".into(),
                value: 0.5,
            },
        ];
        sort_descending(&mut scores);
        let labels: Vec<&str> = scores.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(labels, vec!["2", "10", "x"]);
    }

    #[test]
    fn nan_scores_do_not_panic() {
        let mut scores = vec![
            Classification {
                label: "a".into(),
                value: f64::NAN,
            },
            Classification {
                label: "b".into(),
                value: 1.0,
            },
            Classification {
                label: "c".into(),
                value: 2.0,
            },
        ];
        sort_descending(&mut scores);
        let labels: Vec<&str> = scores.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(labels, vec!["a", "c", "b"]);
    }
}
