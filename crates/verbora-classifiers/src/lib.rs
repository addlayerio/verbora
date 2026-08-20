//! Naive-Bayes, logistic-regression and maximum-entropy text classifiers.
//!
//! Three classifiers over two unrelated designs:
//!
//! | Type | Learns | Trained by | Published formulation |
//! |---|---|---|---|
//! | [`BayesClassifier`] | per-class feature counts | one pass, incremental | multinomial naive Bayes with additive smoothing — Manning, Raghavan & Schütze, *Introduction to Information Retrieval* (2008) §13.2 |
//! | [`LogisticRegressionClassifier`] | one-vs-rest weights | batch gradient descent, refitted from scratch each call | Cox (1958), *The regression analysis of binary sequences*; the one-vs-rest reduction is Rifkin & Klautau (2004) |
//! | [`MaxEntClassifier`] | feature weights `alpha` | generalised iterative scaling | Darroch & Ratcliff (1972), *Generalized iterative scaling for log-linear models* |
//!
//! ```
//! use verbora_classifiers::BayesClassifier;
//!
//! let mut classifier = BayesClassifier::new();
//! classifier.add_document("my unit-tests failed.", "software");
//! classifier.add_document("tried the program, but it was buggy.", "software");
//! classifier.add_document("tomorrow we will do standard tests", "other");
//! classifier.add_document("the drive has a 2TB capacity", "other");
//! classifier.train().unwrap();
//!
//! assert_eq!(classifier.classify("did the program crash?").unwrap(), "software");
//! ```
//!
//! # The text unit
//!
//! **A feature key is the stem of one UAX #29 word token of the lowercased
//! document.** Nothing here works in characters or bytes; the unit is a token,
//! and which tokens a document has is decided by three steps, in this order:
//!
//! 1. the Unicode full lowercase mapping, applied to the *whole document*;
//! 2. UAX #29 word boundaries over the result;
//! 3. the installed [`Stemmer`], applied per token after the stop-word filter.
//!
//! The order is observable. `"unit-tests"` above trains `unit` **and** `test`,
//! because U+002D is `Word_Break=Other` and breaks on both sides — so a query
//! of `"tests"` alone matches both classes and ties. And because the fold runs
//! before the cut, a changed case mapping can move a boundary rather than
//! merely a spelling.
//!
//! An [`Observation::Tokens`] input skips all three: a token slice is used
//! verbatim, with no lowercasing, no stemming and no stop-word removal. That is
//! the escape hatch for a caller who owns their own pipeline, and the only way
//! to put a key into the vocabulary that this crate would not derive.
//!
//! # Floating point is part of the contract
//!
//! Scores are `f64`, and three properties of the arithmetic are specified
//! rather than incidental:
//!
//! * **Evaluation order is fixed.** Naive Bayes sums its logs from the highest
//!   set feature index downwards; the logistic fit contracts dot products
//!   descending and sums its cost vector ascending; every maximum-entropy
//!   summation walks the sample in insertion order, duplicates included.
//!   Floating-point addition is not associative, so reordering any of them
//!   moves the last bits of a score that is then sorted against its rivals, and
//!   a near-tie can flip. `tests/train_parity.rs` and `tests/predict_parity.rs`
//!   pin the orders on raw bits.
//! * **[`log`], [`exp`], [`pow`] and [`sigmoid`] are Verbora's own.** A
//!   platform libm is not specified to be correctly rounded and disagrees
//!   between targets, so a model trained on one machine and loaded on another
//!   would score differently through `f64::ln`. These are in-tree FDLIBM ports
//!   with no target-dependent behaviour, which is what makes a saved model
//!   reproducible rather than merely portable. They sit inside a gradient
//!   descent whose convergence threshold is `1e-4`, so a one-ULP difference
//!   changes the iteration count and hence the model.
//! * **Ties are resolved, not left to chance.** `get_classifications` sorts
//!   descending with a *stable* sort, so classes scoring exactly equal come
//!   back in the engine's own enumeration order. A `NaN` difference compares as
//!   "equal" rather than panicking, so an unorderable score never aborts a
//!   ranking.
//!
//! ## `NaN` is computable, not merely restorable
//!
//! A `NaN` score is **not** confined to a corrupt saved model. Ordinary calls
//! against ordinary inputs produce one, and this crate propagates it rather
//! than rejecting it:
//!
//! | Path | How the `NaN` arises |
//! |---|---|
//! | [`BayesEngine::with_smoothing`] with a negative constant, then any observation bit unseen for a class | the per-class count falls back to the smoothing constant, so the class score takes `log` of a negative ratio |
//! | [`Classifier::restore`] of a model whose stored count is negative | the same `log`, from a stamp-valid artifact |
//! | [`Distribution`] with a negative alpha | `log_likelihood`, `entropy` and `kullback_liebler_distance` all return `Ok(NaN)` |
//! | [`Distribution`] with an alpha of zero | `calculate_a_posteriori` returns `Ok(NaN)` from `0 / 0`, without `log` being involved at all |
//!
//! The consequence worth stating plainly: **a `NaN` score can be returned as
//! the winner.** The comparator treats an unorderable difference as a tie and
//! the sort is stable, so a class scoring `NaN` keeps its enumeration position
//! and [`Classifier::classify`] returns it — `Ok(label)`, no error, no panic,
//! and a score that is not a number. Compare `NaN` explicitly (`f64::is_nan`)
//! if that matters to your caller; the ranking will not do it for you.
//!
//! None of this is an accident of the arithmetic primitives. [`log`] returns
//! `NaN` for a negative argument because IEEE 754 requires it, and changing
//! that would misreport the logarithm rather than fix anything: the `NaN`'s
//! source is a negative *input*, admitted at the boundary. `with_smoothing`
//! admits a negative constant deliberately — see its own documentation — so
//! the boundary is where a caller who needs "no `NaN` escapes" has to stand.
//! `negative_smoothing_computes_a_nan_score_that_can_win` pins the whole chain.
//!
//! ## What an empty, unseen or stale input answers
//!
//! | Situation | Answer |
//! |---|---|
//! | No documents added, then `classify` | [`ClassifierError::NotTrained`] for Bayes; [`ClassifierError::NotFitted`] for logistic regression |
//! | `train()` with no examples | [`ClassifierError::NoExamples`] for logistic regression; Bayes has nothing to fit and succeeds |
//! | A document that tokenises to nothing | dropped, silently: `""` and any all-stop-word string leave the classifier untouched |
//! | A token absent from the vocabulary | contributes nothing to the observation vector; in Bayes its per-class count falls back to the smoothing constant |
//! | An unknown class label handed to [`BayesEngine::probability_of_class`] | `None` — an untrained class has no prior |
//! | A document added after `train()` | the vocabulary widens and the fit does not, so classifying is [`ClassifierError::StaleModel`] until `train()` runs again |
//! | Two classes scoring exactly equal | both are returned; `classify` takes the first in enumeration order |
//!
//! # Feature layout, and why it is fragile
//!
//! A feature's id is its position in an insertion-ordered map ([`OrderedMap`]),
//! which enumerates **integer-like keys first, in ascending numeric order**,
//! and the rest in insertion order. Adding a token that looks like an integer
//! therefore hoists it to slot 0 and shifts every previously learned index. A
//! trained weight vector restored against a shifted index is scrambled rather
//! than merely stale, which is why persistence is stamped.
//!
//! # Persistence
//!
//! `to_json`/`restore` are the in-memory halves of `save`/`load`.
//!
//! **Saved models are version-locked.** A feature key is a stem of a UAX #29
//! word token of lowercased text, so a model trained under one set of word
//! boundaries, one case mapping or one stemmer mispredicts under another —
//! silently, because every number in it stays arithmetically valid. Every saved
//! model therefore opens with a four-fact compatibility stamp
//! ([`ArtifactStamp`]) covering the schema, the Unicode version, the lowercase
//! mapping and the stemmer, and every load refuses an artifact whose stamp is
//! absent, damaged or foreign. See [`ArtifactStamp`] for what each fact covers
//! and what is deliberately outside it, and [`LoadError`]/[`RestoreError`] for
//! how a caller tells a damaged file from an incompatible one.
//!
//! A model trained through [`Classifier::with_stemmer`] must be read back with
//! [`Classifier::restore_with`]; [`Classifier::restore`] assumes
//! [`default_stemmer`] and is refused otherwise rather than silently rekeying
//! the model.
//!
//! [`MaxEntClassifier::restore`] deliberately returns an **untrained**
//! classifier: it reads only the sample's elements and regenerates the features
//! from them, discarding `alpha`.
//!
//! # Maximum entropy
//!
//! Ten of the crate's exported types belong to [`MaxEntClassifier`], and they
//! are wired together by **shared mutable references**, not by ownership: a
//! classifier holds the caller's [`FeatureSet`] and [`Sample`], and `train`
//! mutates both — it appends a correction feature to the feature set and
//! memoises an observed expectation onto every feature. The Rust types use
//! `Rc<RefCell<…>>` for exactly that reason.
//!
//! Four consequences are worth knowing before reading a maximum-entropy score:
//!
//! * **The scores are not probabilities.** `calculate_a_priori` returns the
//!   unnormalised weight `∏ αⱼ^fⱼ(x)`. They routinely exceed 1 and do not sum
//!   to 1. Normalising would change every score, the Kullback–Leibler
//!   trajectory, and therefore the iteration at which training stops.
//! * **Training always runs at least once**, so `train(0, x)` performs one full
//!   iteration rather than none.
//! * **An observed expectation is memoised and never invalidated.** Add an
//!   eleventh element to a ten-element sample and retrain, and the reported
//!   expectations are still the ten's.
//! * **The correction feature cannot be replaced.** It closes over the scaler
//!   that built it, and `add_feature` rejects a second feature of the same
//!   name, so retraining after the sample changed evaluates the correction
//!   against the first run's cached feature sums.
//!
//! These four are **inherited behaviour this migration has not yet redefined**.
//! They are written down so a caller is not surprised, not because they are
//! defensible; see `docs/design/rust-native-migration.md`.
//!
//! # Limits
//!
//! * **Labels and class names are `String`.** A label is text.
//! * **Bayes smoothing is an `f64`**, and [`BayesEngine::with_smoothing`]
//!   accepts only a non-zero finite one, falling back to `1.0` otherwise.
//! * **Applying a POS feature to a context without windows returns 0.**
//!   Generating features from such a context is rejected, with
//!   [`MaxEntError::PosContextMissingWindows`].
//! * **[`MECorpus::split_in_train_and_test_with`] takes its randomness as an
//!   argument**, so a split is reproducible.
//! * **The stop-word list is process-global mutable state** inherited from
//!   `verbora-stemmers`, so two classifiers built at different moments in one
//!   process can tokenise the same document differently. No stamp can cover
//!   that; see [`ArtifactStamp`].

#![doc(html_root_url = "https://docs.rs/verbora-classifiers")]

mod basic;
mod dynval;
mod maxent;
mod ordmap;
mod stamp;
mod stemmer;
mod transcendental;
mod whitespace;

pub use basic::{
    BayesClassifier, BayesEngine, Classification, Classifier, ClassifierError, Document, Engine,
    LoadError, LogisticEngine, LogisticRegressionClassifier, Observation, TrainingEvent,
};
pub use dynval::{DynValue, ParseError, json_stringify_pretty, number_to_string, utf16_cmp};
pub use maxent::{
    Context, Distribution, Element, Feature, FeatureFn, FeatureSet, GISScaler, GenerateFeatures,
    MECorpus, MESentence, MaxEntClassifier, MaxEntError, POSElement, RestoreError, SEElement,
    Sample, ScalerState, TaggedWord,
};
pub use ordmap::{OrderedMap, is_array_index};
pub use stamp::{
    // `StampMismatch` is exported because `StampError::Incompatible` carries
    // one: a caller matching on that variant has to be able to name its
    // payload, and the type was previously reachable only through pattern
    // matching.
    ArtifactStamp,
    CONTEXT_PROBES,
    SCHEMA,
    STAMP_PROPERTY,
    STEMMER_PROBES,
    StampError,
    StampMismatch,
    lowercase_fingerprint,
    stemmer_fingerprint,
    verify_stamp,
    verify_stamp_against,
};
pub use stemmer::{StemCache, Stemmer, StemmerOf, default_stemmer};
pub use transcendental::{exp, log, pow, sigmoid};
