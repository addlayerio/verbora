//! Naive-Bayes, logistic-regression and maximum-entropy text classifiers.
//!
//! Three classifiers over two unrelated designs:
//!
//! | Type | Learns | Trained by | Published formulation |
//! |---|---|---|---|
//! | [`BayesClassifier`] | per-class feature counts | one pass, incremental | multinomial naive Bayes with additive smoothing — Manning, Raghavan & Schütze, *Introduction to Information Retrieval* (2008) §13.2 |
//! | [`LogisticRegressionClassifier`] | one-vs-rest weights | batch gradient descent, refitted from scratch each call | Cox (1958), *The regression analysis of binary sequences*; the one-vs-rest reduction is Rifkin & Klautau (2004) |
//! | [`MaxEntClassifier`] | a weight per `(predicate, outcome)` feature | generalised iterative scaling, refitted from the uniform model each call | the conditional exponential model of Berger, Della Pietra & Della Pietra (1996), *A Maximum Entropy Approach to Natural Language Processing*, fitted by Darroch & Ratcliff (1972), *Generalized iterative scaling for log-linear models* |
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
//!   back in the order the engine first saw those classes. A `NaN` difference compares as
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
//!
//! **Maximum entropy is the exception, and deliberately so.** No
//! [`MaxEntClassifier`] or [`MaxEntModel`] API returns a `NaN`, an infinity, or
//! a sentinel standing in for one — see that module's own "No `NaN`, no
//! infinities, no sentinels" section for the four facts that make it
//! structural. The rows above are about Bayes and logistic regression.
//!
//! The consequence worth stating plainly: **a `NaN` score can be returned as
//! the winner.** The comparator treats an unorderable difference as a tie and
//! the sort is stable, so a class scoring `NaN` keeps its first-appearance position
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
//! | Two classes scoring exactly equal | both are returned; `classify` takes the first the engine was trained on |
//!
//! # Feature layout
//!
//! A feature's id is its slot in an insertion-ordered map ([`OrderedMap`]): the
//! first token the vocabulary ever saw is slot 0, and a token keeps its slot
//! for as long as it is in the vocabulary. Adding a token — including one that
//! looks like an integer — appends it after every feature already known, so a
//! trained model's indices stay valid and only the new slot is untrained.
//!
//! [`Classifier::remove_document`] is the exception, and is destructive by
//! design: it *deletes* the matched document's tokens from the vocabulary
//! rather than decrementing them, and closing those gaps shifts every later
//! slot down. A model fitted before such a call is stale afterwards; `retrain`
//! is what recovers it.
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
//! [`MaxEntClassifier::restore`] restores the **fit**: a file carrying a model
//! comes back trained and classifies identically, and one saved before training
//! comes back untrained. Its predicates are caller-supplied strings rather than
//! stems of tokenised text, so no tokenizer, case mapping or stemmer of this
//! crate's stands between a document and a maximum-entropy feature key; the
//! stamp is still written and checked, because a caller should not have to know
//! which classifier wrote a file to know whether it is safe to read.
//!
//! # Maximum entropy
//!
//! [`MaxEntClassifier`] models `p(outcome | context)` where a context is a
//! **set of contextual predicates the caller supplies** — not a document this
//! crate tokenises. Training events go in through [`Sample`]/[`Event`],
//! [`Gis`] settings control the fit, and [`TrainingReport`] says what it did.
//! Everything it returns is a probability: the scores over one context are
//! non-negative and sum to `1`.
//!
//! It is owned rather than shared: fitting reads the sample and produces a new
//! [`MaxEntModel`], mutating nothing the caller still holds and memoising
//! nothing, so both types are `Send + Sync` and a fitted model can be shared
//! across threads behind an `Arc` with no lock. Refitting always restarts from
//! the uniform model, so training twice over an unchanged sample is
//! bit-identical and training after the sample grew fits the sample as it now
//! stands.
//!
//! The maximum-entropy contract — the model, the GIS update, why there is no
//! stored slack feature, what convergence is measured on, and the summation
//! orders — is stated in full on the [`MaxEntClassifier`] module's own
//! documentation.
//!
//! # Limits
//!
//! * **Labels and class names are `String`.** A label is text.
//! * **Bayes smoothing is an `f64`**, and [`BayesEngine::with_smoothing`]
//!   accepts only a non-zero finite one, falling back to `1.0` otherwise.
//! * **A maximum-entropy predicate is an opaque string.** It is never trimmed,
//!   lowercased, tokenised or stemmed, and deriving predicates from text is the
//!   caller's job. There is no built-in bias feature either: a caller who wants
//!   the model to learn outcome priors adds a predicate that every context
//!   carries.
//! * **[`MaxEntClassifier`] has no parallel batch API.** Every `par_*` API in
//!   this workspace requires sequential-versus-parallel benchmark evidence, and
//!   there is none for this model yet.
//! * **The stop-word list is process-global mutable state** inherited from
//!   `verbora-stemmers`, so two classifiers built at different moments in one
//!   process can tokenise the same document differently. No stamp can cover
//!   that; see [`ArtifactStamp`].

#![cfg_attr(doctest, doc = include_str!("../README.md"))]
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
    Event, Gis, MaxEntClassifier, MaxEntError, MaxEntModel, ModelDefect, RestoreError, Sample,
    StopReason, TrainingReport,
};
pub use ordmap::OrderedMap;
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
