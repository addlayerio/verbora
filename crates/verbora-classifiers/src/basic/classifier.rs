use std::sync::{Arc, Mutex, PoisonError};

use crate::whitespace::is_whitespace;

use crate::dynval::DynValue;
use crate::ordmap::OrderedMap;
use crate::stamp::{ArtifactStamp, StampError};
use crate::stemmer::{StemCache, Stemmer, default_stemmer};

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
/// Emitted synchronously, and the stream differs between the two classifiers:
/// Bayes trains incrementally from `last_added`, whereas logistic regression
/// discards its engine and re-emits every document on every call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrainingEvent {
    /// One document was handed to the engine.
    TrainedWithDocument {
        /// Index of the document in `docs`.
        index: usize,
        /// The document count captured at the start of this `train()` call.
        total: usize,
    },
    /// The engine's own fit returned.
    DoneTraining,
}

/// Why a classifier could not answer.
///
/// Each variant is a distinct thing that is wrong and calls for a distinct
/// response, which is why none of them is folded into another:
///
/// | Variant | What happened | What to do |
/// |---|---|---|
/// | [`NotTrained`](Self::NotTrained) | no class has ever been scored into the engine | add documents, then `train()` |
/// | [`NotFitted`](Self::NotFitted) | documents were added but `train()` has not run since | call `train()` |
/// | [`NoExamples`](Self::NoExamples) | `train()` was called with nothing to fit | add documents first |
/// | [`StaleModel`](Self::StaleModel) | the vocabulary grew after the last fit | call `train()` again |
/// | [`NoMinimum`](Self::NoMinimum) | gradient descent exhausted its iteration budget | the corpus is degenerate — inspect it |
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClassifierError {
    /// The engine holds no classes at all, so there is nothing to rank.
    ///
    /// Reached by classifying against a classifier that has never been trained,
    /// or one every document of which tokenised to nothing.
    NotTrained,
    /// Examples were recorded but no parameters have been fitted to them.
    ///
    /// Logistic regression fits in `train()`; asking it for classifications
    /// before then has no answer, as distinct from having an empty one.
    NotFitted,
    /// `train()` was asked to fit a model with no examples.
    NoExamples,
    /// The fitted parameters and the feature vocabulary have different widths.
    ///
    /// A document added after `train()` extends the vocabulary without
    /// refitting, so the learned weight vector no longer describes an
    /// observation built from it. Both widths are carried because the
    /// difference is the diagnosis.
    StaleModel {
        /// How many weights the fitted model holds.
        weights: usize,
        /// How many features the vocabulary now has.
        observation: usize,
    },
    /// Gradient descent ran its whole iteration budget without the cost
    /// decreasing, at every learning rate it tried.
    NoMinimum,
}

impl std::fmt::Display for ClassifierError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotTrained => f.write_str(
                "classifier holds no classes; add documents and call train() before classifying",
            ),
            Self::NotFitted => f.write_str(
                "classifier has examples but no fitted parameters; call train() before classifying",
            ),
            Self::NoExamples => {
                f.write_str("train() was called with no examples; add documents first")
            }
            Self::StaleModel {
                weights,
                observation,
            } => write!(
                f,
                "model was fitted over {weights} features but the vocabulary now has \
                 {observation}; a document was added after the last train(), so call train() again"
            ),
            Self::NoMinimum => f.write_str(
                "gradient descent found no decreasing cost at any learning rate within its \
                 iteration budget",
            ),
        }
    }
}

impl std::error::Error for ClassifierError {}

/// The learning algorithm behind a [`Classifier`].
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
/// Trims a class label.
///
/// The trimmed set is [`crate::whitespace::is_whitespace`], which is not Rust's
/// `char::is_whitespace`: `U+FEFF` is trimmed and `U+0085` is not.
fn trim_units(s: &str) -> &str {
    s.trim_matches(is_whitespace)
}

/// Why a saved classifier could not be read back.
///
/// The three variants are three different things that can be wrong, and they
/// call for three different responses:
///
/// * [`Self::Io`] — the bytes never arrived. Only [`Classifier::load`] produces
///   this; [`Classifier::restore`] is handed the bytes and shares the type
///   rather than duplicating it, the same way [`crate::RestoreError`] does for
///   maximum entropy.
/// * [`Self::Parse`] — the bytes are not JSON. The file is damaged or is not a
///   saved classifier; repair or re-fetch it.
/// * [`Self::Stamp`] — the bytes parsed, but the model was not trained by this
///   build (or by any build that recorded which one it was). The file is
///   intact; its *features* are the problem, and the fix is to retrain. See
///   [`StampError`].
///
/// Collapsing the last two would leave a caller unable to tell a corrupt
/// download from a model that merely needs retraining, which is why they are
/// separate.
#[derive(Debug)]
pub enum LoadError {
    /// The file could not be read.
    Io(std::io::Error),
    /// The file was not valid JSON.
    Parse(crate::dynval::ParseError),
    /// The model carries no usable compatibility stamp, or one from another
    /// build.
    Stamp(StampError),
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "{e}"),
            Self::Parse(e) => write!(f, "{e}"),
            Self::Stamp(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for LoadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::Parse(e) => Some(e),
            Self::Stamp(e) => Some(e),
        }
    }
}

impl From<StampError> for LoadError {
    fn from(e: StampError) -> Self {
        Self::Stamp(e)
    }
}

/// A document classifier: vocabulary, documents, and an engine.
///
/// The shell shared by Bayes and logistic regression.
///
/// Everything here is stateful and order-dependent, and three of the behaviours
/// look like bugs a port would want to fix. They are not optional:
///
/// * **The feature vector's layout is a reference object's enumeration order.**
///   `textToFeatures` walks `for (const feature in this.features)`, so adding a
///   token that *looks like an integer* hoists it to slot 0 and shifts every
///   previously learned index. See [`OrderedMap`](crate::OrderedMap).
/// * **`removeDocument` deletes feature entries rather than decrementing them**,
///   removes the *last* of several matching documents, and leaves `lastAdded`
///   untouched — so a subsequent `train()` silently skips documents and only
///   `retrain()` recovers.
/// * **A document that tokenises to nothing is dropped in silence**, including
///   `""` and any all-stop-word string.
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
    /// `token → stem`, filled by whichever calls happen to run.
    ///
    /// Not part of the classifier's state in any observable sense: it changes
    /// no answer (see [`StemCache`]), is never serialised, and starts empty in
    /// a `restore`d classifier. The `Mutex` exists only because `classify`
    /// takes `&self` — the training entry points reach the map through
    /// `get_mut` and never lock at all — and it is only ever *tried*, never
    /// waited on, so a caller that finds it busy re-stems rather than blocking
    /// on it (`par_classify_batch` sidesteps the question entirely by giving
    /// each worker a memo of its own).
    stem_cache: Mutex<StemCache>,
}

/// How many distinct tokens a classifier's [`StemCache`] may hold before it is
/// emptied and refilled.
///
/// A bound is needed because `classify` may be fed unbounded vocabulary for
/// the life of a long-running process, and dropping the whole map is the
/// cheapest eviction that cannot get the answer wrong. The number is high
/// enough that a realistic corpus never reaches it (a 1 000-document corpus
/// interns a few thousand distinct tokens) and small enough that the memo
/// stays far below what `docs` already costs — that field stores every token
/// of every training document, so the memo is never the dominant allocation.
const STEM_CACHE_CAP: usize = 1 << 16;

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
            stem_cache: Mutex::new(StemCache::new()),
        }
    }

    /// `setOptions({ keepStops })`.
    ///
    /// Affects [`Observation::Text`] only — an [`Observation::Tokens`] slice
    /// bypasses the stemmer, and therefore the stop-word list, entirely.
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
    /// Takes `&mut self` purely to reach the stem memo without locking: every
    /// caller (`add_document`, `remove_document`) already holds the classifier
    /// mutably. Porter stemming is around 65% of training time on a
    /// repeated-vocabulary corpus, and each distinct token's stem is the same
    /// every time, so the memo is where that 65% goes.
    fn resolve(&mut self, observation: Observation<'_>) -> Vec<String> {
        match observation {
            Observation::Text(s) => {
                let keep_stops = self.keep_stops.unwrap_or(false);
                let cache = self
                    .stem_cache
                    .get_mut()
                    .unwrap_or_else(PoisonError::into_inner);
                let out = self.stemmer.tokenize_and_stem_cached(s, keep_stops, cache);
                if cache.len() > STEM_CACHE_CAP {
                    cache.clear();
                }
                out
            }
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
            //
            // A token already in the vocabulary is updated in place: assigning
            // to an existing key keeps its position, so this is the same
            // `insert` minus the key clone and the second hash lookup — and,
            // because the key set does not change, minus the enumeration
            // memo's invalidation, which matters on corpora where almost every
            // token is a repeat.
            if let Some(count) = self.features.get_mut(token) {
                *count = if *count != 0.0 && !count.is_nan() {
                    *count + 1.0
                } else {
                    1.0
                };
            } else {
                self.features.insert(token.clone(), 1.0);
            }
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
        // `last_added` is deliberately left alone: a later `train()` therefore
        // skips documents, and only `retrain()` recovers. Inherited behaviour
        // this migration has not yet redefined.
    }

    /// `textToFeatures(observation)`: a 0/1 vector, one slot per known feature.
    ///
    /// The slot order is [`OrderedMap::enumeration_order`] as it stands *now* —
    /// which is exactly why adding an integer-like token invalidates a trained
    /// model, and why the memo behind that order is thrown away by every
    /// mutation that could reshuffle it.
    ///
    /// An `Observation::Tokens` input is already an owned `&[String]` the
    /// caller holds; going through the crate-internal `resolve` step that
    /// [`Self::add_document`] uses would `.to_vec()` it (cloning every token)
    /// just to borrow `&str`s back out a line later. This builds the presence
    /// set straight from the borrow instead — `resolve` still owns the
    /// `Observation::Text` path, which genuinely needs the stemmer's fresh
    /// `Vec<String>`, and the memo that makes that path cheap is
    /// [`StemCache`].
    pub fn text_to_features<'a>(&self, observation: impl Into<Observation<'a>>) -> Vec<u8> {
        match observation.into() {
            // The classifier's own memo is *tried*, never waited on: a
            // blocking lock here would make a parallel fan-out serialise on
            // the one thing every one of its tasks does. A caller that finds
            // it busy re-stems instead, which costs time and changes nothing
            // else — the memo holds nothing but stems the stemmer itself
            // produced.
            Observation::Text(s) => match self.stem_cache.try_lock() {
                Ok(mut cache) => self.features_from_text(s, &mut cache),
                Err(_) => {
                    let tokens = self
                        .stemmer
                        .tokenize_and_stem(s, self.keep_stops.unwrap_or(false));
                    self.features_for(tokens.iter().map(String::as_str))
                }
            },
            Observation::Tokens(t) => self.features_for(t.iter().map(String::as_str)),
        }
    }

    /// The shared tail of [`Self::text_to_features`]: a 0/1 vector over
    /// [`OrderedMap::enumeration_order`], one slot per known feature.
    ///
    /// Driven by the *tokens*, not by the features. "Slot `s` is 1 iff some
    /// token equals the feature enumerated at `s`" reads equally well from
    /// either end, but a probe holds a dozen tokens where a trained
    /// vocabulary holds hundreds, so asking [`OrderedMap::slot_of`] once per
    /// token replaces one hash probe per *feature* — and drops the
    /// `enumeration_order` vector the per-feature form had to materialise.
    /// A token outside the vocabulary has no slot and contributes nothing,
    /// exactly as it failed the per-feature membership test; a token repeated
    /// in the document sets the same slot twice, which is still 1.
    fn features_for<'b>(&self, tokens: impl Iterator<Item = &'b str>) -> Vec<u8> {
        let mut out = vec![0u8; self.features.len()];
        for token in tokens {
            if let Some(slot) = self.features.slot_of(token) {
                out[slot] = 1;
            }
        }
        out
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
            // Calling `text_to_features` per document would recompute the
            // enumeration order — an O(vocabulary) allocation and sort — for
            // every document in the loop. But nothing inside the loop touches
            // `features` (only `addDocument`/`removeDocument` do), so the order,
            // and with it the whole token→slot layout, is loop-invariant: the
            // memo behind `slot_of` is filled by the first token and answers
            // every later one. Marking each document's tokens into a reused
            // buffer produces byte-identical observations: a slot is 1 exactly
            // when the document contains that feature, which is the same
            // membership test `text_to_features` runs per slot.
            let mut observation = vec![0u8; self.features.len()];
            for i in self.last_added..total {
                observation.fill(0);
                for token in &self.docs[i].text {
                    // A token absent from the vocabulary (never added, or
                    // deleted by `removeDocument`) contributes nothing, exactly
                    // as it fails `text_to_features`'s presence test.
                    if let Some(slot) = self.features.slot_of(token.as_str()) {
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
    /// [`ClassifierError::NotTrained`] when there are no classes to score, and
    /// whatever the engine reports otherwise — [`ClassifierError::NotFitted`]
    /// or [`ClassifierError::StaleModel`] for logistic regression.
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
    /// `texts.par_iter().map(|t| self.classify(*t)).collect()` with one
    /// difference that is invisible in the output — each worker thread keeps
    /// its own stem memo, see below — not a second implementation of the
    /// scoring. `get_classifications` and `text_to_features` are untouched; if
    /// you need those shapes in parallel, apply the same `par_iter().map(...)`
    /// pattern at your own call site (see `site/performance/parallelism.md`).
    ///
    /// # Why each task gets its own stem memo
    ///
    /// A sequential `classify` answers from the classifier's own
    /// [`StemCache`], which it reaches through a `Mutex` it only ever *tries*
    /// — so a fan-out that shared it would find it busy, fall back to
    /// re-stemming, and pay the full Porter cost on every token of every text.
    /// Instead each `rayon` worker initialises an empty memo once and warms it
    /// over the texts it happens to be handed, which needs no synchronisation
    /// at all. The tokens are identical either way (a memo holds nothing but
    /// stems the stemmer itself produced), so this changes throughput, never
    /// an answer.
    ///
    /// # When to reach for it vs. the sequential loop
    ///
    /// Reach for this only when the *batch*, not the single classification, is
    /// the unit of work — for example, labelling every document in a large
    /// corpus offline. A single `classify` call costs on the order of 2 µs for
    /// a Bayes classifier trained on a few dozen documents (this crate's own
    /// `bayes/predict/classify` benchmark), and a `rayon` task costs on the
    /// order of a microsecond to schedule, so a handful of texts is well below
    /// the break-even point — measure before reaching for this on small
    /// batches, and prefer a plain `texts.iter().map(Classifier::classify)`
    /// loop there. Batches in the thousands amortise both the scheduling cost
    /// and each worker's memo warm-up easily; see this crate's
    /// `bayes/predict_batch` benchmark for measured crossover numbers.
    ///
    /// # Allocation behaviour
    ///
    /// One `Vec<Result<String, ClassifierError>>` sized to `texts.len()` for
    /// the output, one [`StemCache`] per participating worker thread, plus
    /// whatever [`Classifier::classify`] itself allocates per text (a fresh
    /// token `Vec<String>` from the stemmer, a 0/1 feature vector, and the
    /// engine's own per-call scoring allocations). No further buffering, no
    /// locking, no per-call thread-pool construction — this uses whichever
    /// global `rayon` pool is already installed (or `rayon`'s default one), so
    /// pool configuration remains the caller's responsibility, not this
    /// crate's.
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
        texts
            .par_iter()
            .map_init(StemCache::new, |cache, text| {
                let observation = self.features_from_text(text, cache);
                self.engine
                    .classifications(&observation)?
                    .into_iter()
                    .next()
                    .map(|c| c.label)
                    .ok_or(ClassifierError::NotTrained)
            })
            .collect()
    }

    /// The `Observation::Text` half of [`Self::text_to_features`], against a
    /// memo the caller owns rather than the classifier's own.
    ///
    /// Identical work to the shared-memo path — same stemmer call, same
    /// token→slot marking — so which memo answers a token is unobservable.
    fn features_from_text(&self, text: &str, cache: &mut StemCache) -> Vec<u8> {
        let tokens =
            self.stemmer
                .tokenize_and_stem_cached(text, self.keep_stops.unwrap_or(false), cache);
        if cache.len() > STEM_CACHE_CAP {
            cache.clear();
        }
        self.features_for(tokens.iter().map(String::as_str))
    }

    /// The classifier as [`Self::from_value`] reads it back.
    ///
    /// The object opens with the compatibility stamp — see [`ArtifactStamp`](crate::ArtifactStamp) for
    /// what it covers and why loading without one is refused — and the rest is
    /// the recorded shape: the key order, the `EventEmitter` internals, the
    /// empty `stemmer` object and the `Threads: null` sentinel.
    ///
    /// The stamp is written here rather than by an opt-in wrapper because an
    /// unstamped model is unvalidatable for the whole of its life: the
    /// boundaries its features were derived under are not recoverable after the
    /// fact, so a path that could produce one would be a path that produces
    /// files nobody can ever safely load.
    pub fn to_value(&self) -> DynValue {
        let mut fields = vec![
            (
                crate::stamp::STAMP_PROPERTY.to_owned(),
                ArtifactStamp::for_stemmer(&*self.stemmer).to_value(),
            ),
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
    /// Rebuilds with [`default_stemmer`]. A model trained through
    /// [`Self::with_stemmer`] carries a different stemmer fingerprint in its
    /// stamp and is **refused** here rather than silently rekeyed; read it back
    /// with [`Self::restore_with`].
    ///
    /// # Errors
    ///
    /// [`LoadError::Parse`] if `json` is not valid JSON, and [`LoadError::Stamp`]
    /// if it is valid JSON that this build must not read: a model saved before
    /// stamping existed, one whose stamp is damaged, or one from a build whose
    /// word boundaries differ from this one's. The two are separate because a
    /// damaged file and a stale model need different fixes; see [`LoadError`].
    /// [`LoadError::Io`] cannot occur here — the type is shared with
    /// [`Self::load`].
    ///
    /// Past the stamp, fields that are missing or of the wrong type fall back
    /// to their empty values rather than failing: the stamp is what decides
    /// whether an artifact may be read at all.
    pub fn restore(json: &str) -> Result<Self, LoadError> {
        Self::restore_with(json, default_stemmer())
    }

    /// [`Self::restore`], rebuilding with a stemmer you name.
    ///
    /// Use this whenever the model was trained through
    /// [`Self::with_stemmer`]. The stamp records which stemmer keyed the
    /// features, so passing the wrong one is refused rather than silently
    /// mispredicting — but naming the right one is still the caller's job,
    /// because only the caller knows it.
    ///
    /// # Errors
    ///
    /// As [`Self::restore`].
    pub fn restore_with(
        json: &str,
        stemmer: Arc<dyn Stemmer + Send + Sync>,
    ) -> Result<Self, LoadError> {
        let value = DynValue::parse(json).map_err(LoadError::Parse)?;
        Ok(Self::from_value_with(&value, stemmer)?)
    }

    /// Writes [`Self::to_json`] to `path` as UTF-8.
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
    /// [`LoadError::Io`] if the file cannot be read, and whatever
    /// [`Self::restore`] reports otherwise.
    pub fn load(path: impl AsRef<std::path::Path>) -> Result<Self, LoadError> {
        Self::load_with(path, default_stemmer())
    }

    /// [`Self::load`], rebuilding with a stemmer you name.
    ///
    /// # Errors
    ///
    /// As [`Self::load`].
    pub fn load_with(
        path: impl AsRef<std::path::Path>,
        stemmer: Arc<dyn Stemmer + Send + Sync>,
    ) -> Result<Self, LoadError> {
        let body = std::fs::read_to_string(path).map_err(LoadError::Io)?;
        Self::restore_with(&body, stemmer)
    }

    /// Rebuilds a classifier from an already-parsed saved shape.
    ///
    /// # Errors
    ///
    /// [`StampError`] when `value` does not carry this build's compatibility
    /// stamp. This entry point is checked for the same reason [`Self::restore`]
    /// is — it restores persisted feature keys, and a checked file reader in
    /// front of an unchecked value reader would leave the hazard exactly where
    /// it was.
    pub fn from_value(value: &DynValue) -> Result<Self, StampError> {
        Self::from_value_with(value, default_stemmer())
    }

    /// [`Self::from_value`], rebuilding with a stemmer you name.
    ///
    /// # Errors
    ///
    /// [`StampError`] when `value` was not keyed by `stemmer` under this
    /// build's word boundaries and case fold.
    pub fn from_value_with(
        value: &DynValue,
        stemmer: Arc<dyn Stemmer + Send + Sync>,
    ) -> Result<Self, StampError> {
        crate::stamp::verify_stamp_against(value, ArtifactStamp::for_stemmer(&*stemmer))?;
        let mut out = Self::with_stemmer(stemmer);
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
        Ok(out)
    }
}

/// Sorts scores descending by value.
///
/// Two details are load-bearing. The sort must be **stable**, so exact ties come
/// back in the engine's own enumeration order rather than in some order the sort
/// happened to produce. And the comparator must treat a `NaN` difference as
/// "equal" rather than panicking: `partial_cmp().unwrap()` would abort a whole
/// ranking over one unorderable score, which a caller-restored model can
/// contain.
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
        // U+0085 NEXT LINE is not in the trimmed set.
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

    // -- the compatibility stamp --------------------------------------------

    /// A trained model to serialize and then tamper with.
    fn saved() -> String {
        let mut c = BayesClassifier::new();
        c.add_document("my unit-tests failed.", "software");
        c.add_document("tomorrow we will do standard tests", "other");
        c.train().expect("two labelled documents train");
        c.to_json()
    }

    /// Where the stamp member ends in a saved model: `to_value` writes it
    /// first, so everything from `,"_events"` onwards is the model proper.
    fn body_start(json: &str) -> usize {
        assert!(json.starts_with(r#"{"_verbora":"#), "{json}");
        json.find(r#","_events""#)
            .expect("the stamp is followed by _events")
    }

    /// Replaces the `_verbora` member of a saved model with `replacement`,
    /// leaving every other byte alone.
    fn with_stamp(json: &str, replacement: &str) -> String {
        format!(r#"{{"_verbora":{replacement}{}"#, &json[body_start(json)..])
    }

    /// A model trained under different word boundaries is refused, rather than
    /// classifying against a feature index that no longer describes its
    /// vocabulary.
    #[test]
    fn a_model_from_another_unicode_version_is_refused() {
        let current = ArtifactStamp::for_stemmer(&*default_stemmer());
        let (major, minor, update) = current.unicode;
        let json = with_stamp(
            &saved(),
            &format!(
                r#"{{"schema":{},"unicode":"{}.{minor}.{update}","lowercase":"{:016x}","stemmer":"{:016x}"}}"#,
                current.schema,
                major + 1,
                current.lowercase.expect("this build stamps a fingerprint"),
                current
                    .stemmer
                    .expect("a classifier stamp names its stemmer")
            ),
        );
        let Err(LoadError::Stamp(StampError::Incompatible(mismatch))) =
            BayesClassifier::restore(&json)
        else {
            panic!("a foreign Unicode version must be refused");
        };
        assert_eq!(mismatch.found.unicode, (major + 1, minor, update));
        assert_eq!(mismatch.expected, current);
    }

    /// A schema bump refuses older models even when the UCD has not moved — the
    /// case a Unicode version alone cannot see.
    #[test]
    fn a_model_from_another_schema_is_refused() {
        let current = ArtifactStamp::for_stemmer(&*default_stemmer());
        let (major, minor, update) = current.unicode;
        let json = with_stamp(
            &saved(),
            &format!(
                r#"{{"schema":{},"unicode":"{major}.{minor}.{update}","lowercase":"{:016x}","stemmer":"{:016x}"}}"#,
                current.schema + 1,
                current.lowercase.expect("this build stamps a fingerprint"),
                current
                    .stemmer
                    .expect("a classifier stamp names its stemmer")
            ),
        );
        let Err(LoadError::Stamp(StampError::Incompatible(mismatch))) =
            BayesClassifier::restore(&json)
        else {
            panic!("a foreign schema must be refused");
        };
        assert_eq!(mismatch.found.schema, current.schema + 1);
        assert_eq!(mismatch.expected, current);
    }

    /// The dangerous artifact: a model saved before stamping existed. It parses,
    /// it looks exactly like a model, and nothing in it records the boundaries
    /// its features were derived under — so it is refused, under its own
    /// variant.
    #[test]
    fn a_model_saved_before_stamping_is_refused_as_unstamped() {
        let saved = saved();
        let pre_stamp = format!("{{{}", &saved[body_start(&saved) + 1..]);
        assert!(matches!(
            BayesClassifier::restore(&pre_stamp),
            Err(LoadError::Stamp(StampError::Missing))
        ));
        // The same value handed to the in-memory constructor is refused
        // identically: a checked file reader in front of an unchecked value
        // reader would not be a refusal at all.
        let value = DynValue::parse(&pre_stamp).expect("valid JSON");
        assert_eq!(
            BayesClassifier::from_value(&value).err(),
            Some(StampError::Missing)
        );
    }

    /// A damaged file and a stale one are different problems and are reported as
    /// different errors.
    #[test]
    fn a_corrupt_file_is_distinguishable_from_an_incompatible_one() {
        assert!(matches!(
            BayesClassifier::restore("{not json"),
            Err(LoadError::Parse(_))
        ));
        let damaged = with_stamp(&saved(), r#""17.0.0""#);
        assert!(matches!(
            BayesClassifier::restore(&damaged),
            Err(LoadError::Stamp(StampError::Malformed))
        ));
    }

    /// What `to_json` writes is what `restore` accepts, and the stamp is the
    /// first thing in it.
    #[test]
    fn a_saved_model_carries_this_builds_stamp() {
        let json = saved();
        let stamp = ArtifactStamp::for_stemmer(&*default_stemmer());
        let (major, minor, update) = stamp.unicode;
        let lowercase = stamp
            .lowercase
            .expect("this build's stamp always carries a fingerprint");
        let stemmer = stamp
            .stemmer
            .expect("a classifier's stamp always names its stemmer");
        assert!(
            json.starts_with(&format!(
                r#"{{"_verbora":{{"schema":{},"unicode":"{major}.{minor}.{update}","lowercase":"{lowercase:016x}","stemmer":"{stemmer:016x}"}},"#,
                stamp.schema
            )),
            "{json}"
        );
        let revived = BayesClassifier::restore(&json).expect("its own output round-trips");
        assert_eq!(revived.to_json(), json);
    }
}
