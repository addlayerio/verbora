//! Bayes, logistic-regression and maximum-entropy classifiers, matching
//! The reference exactly.
//!
//! Three classifiers over two unrelated designs:
//!
//! | Type | Learns | Trained by |
//! |---|---|---|
//! | [`BayesClassifier`] | per-class feature counts | one pass, incremental |
//! | [`LogisticRegressionClassifier`] | one-vs-rest weights | gradient descent, from scratch each call |
//! | [`MaxEntClassifier`] | feature weights `alpha` | generalised iterative scaling |
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
//! assert_eq!(classifier.classify("did the tests pass?").unwrap(), "other");
//! ```
//!
//! # Parity notes
//!
//! These classifiers are numerically and structurally fussy in ways an
//! idiomatic Rust port gets wrong. The five that break a port outright:
//!
//! 1. **`Math.log` and `Math.exp` are not `f64::ln` and `f64::exp`.** The reference engine bundles
//!    its own FDLIBM port; the platform libm disagrees with it by one ULP on
//!    roughly 5% of logarithms and 10% of exponentials. Those functions sit
//!    inside a gradient-descent loop with a `1e-4` convergence threshold, so the
//!    difference changes the *number of iterations* and hence the model.
//!    [`transcendental`] reimplements both, verified bit-exact on 95 000 arguments.
//! 2. **The feature vector's layout is a reference object's key order.** Adding
//!    a token that looks like an integer hoists it to slot 0 and shifts every
//!    previously learned index, silently invalidating a trained model. See
//!    [`ordmap`].
//! 3. **Floating-point accumulation order is observable.** Bayes sums logs from
//!    the highest set feature index downwards; `sylvester` contracts dot products
//!    descending but sums cost vectors ascending; every maximum-entropy
//!    summation walks the sample in insertion order with duplicates. Reordering
//!    any of them changes the last bits, and the results are then sorted.
//! 4. **Maximum-entropy scores are unnormalised weights**, not probabilities —
//!    the normalising division is commented out in the reference.
//! 5. **Context keys come from `safe-stable-stringify`**, which sorts object
//!    keys by UTF-16 code unit and formats numbers as the reference does. Rust's
//!    `str: Ord` and `{}` both differ. See [`dynval`].
//!
//! Divergences that could not be reproduced in Rust's type system are listed in
//! [Known divergences](#known-divergences) below.
//!
//! # Persistence
//!
//! `to_json`/`restore` reproduce the reference's `save()`/`load()` bytes,
//! including own-property order, the `EventEmitter` internals, the empty
//! `stemmer` object and the dead logistic-regression fields; `save`/`load` are
//! the synchronous file wrappers around them, where the reference's are
//! asynchronous and report through callbacks. Note that
//! [`MaxEntClassifier::restore`] deliberately returns an **untrained**
//! classifier: The reference's `load` reads only the sample's elements and
//! regenerates the features from them, discarding `alpha`.
//!
//! # Known divergences
//!
//! * **Labels and class names are `String`.** The reference stores whatever was
//!   passed (`docs[i].label` can be a number, `null` or an object) and only
//!   coerces when a value becomes an object key — so Bayes reports the label
//!   `"5"` for a document added with the number `5` while logistic regression
//!   reports `5`. With `String` throughout the two agree. The *string* form of
//!   the quirk — integer-like labels enumerating out of insertion order — is
//!   fully reproduced, because that is the case reachable from real text.
//! * **`addDocument(text, undefined)` cannot be expressed.** The reference drops
//!   such a document silently; Rust has no `undefined` distinct from `null`.
//! * **Bayes smoothing is an `f64`.** The reference accepts `true` and numeric
//!   *strings* through its `smoothing && isFinite(smoothing)` guard, after which
//!   `1 + smoothing` becomes string concatenation and the stored counts are
//!   strings such as `"10.5"`. Only the numeric guard is reproduced.
//! * **Prototype-chain key collisions are not reproduced.** Every reference map
//!   is a plain object tested with `if (!map[key])`, so a context or element
//!   whose key is `"constructor"` or `"toString"` reads as already-present and
//!   corrupts the frequency table. A Rust `HashMap` has no prototype and behaves
//!   correctly, which here means *differently*.
//! * **Applying a POS feature to a context without windows returns 0** rather
//!   than throwing the reference's `TypeError`. Generating features from such a
//!   context is still rejected, with [`MaxEntError::PosContextMissingWindows`].
//! * **Parallel training is not ported.** `trainParallel`, `retrainParallel` and
//!   `trainParallelBatches` require the optional `webworker-threads` package,
//!   which is absent from every standard install, so all three die with a
//!   `TypeError` on `Threads.createPool` before reaching any of their own logic
//!   — which contains two further fatal bugs of its own. The `Threads: null`
//!   sentinel is still emitted in the serialised form.
//! * **`MECorpus::split_in_train_and_test_with` takes the randomness as an
//!   argument**, where the reference calls `Math.random()` directly.

#![doc(html_root_url = "https://docs.rs/verbora-classifiers")]

pub mod basic;
pub mod dynval;
pub mod maxent;
pub mod ordmap;
pub mod stemmer;
pub mod transcendental;

pub use basic::{
    BayesClassifier, BayesEngine, Classification, Classifier, ClassifierError, Document, Engine,
    LoadError, LogisticEngine, LogisticRegressionClassifier, Observation, TrainingEvent,
};
pub use dynval::DynValue;
pub use maxent::{
    Context, Distribution, Element, Feature, FeatureFn, FeatureSet, GISScaler, GenerateFeatures,
    MECorpus, MESentence, MaxEntClassifier, MaxEntError, POSElement, RestoreError, SEElement,
    Sample, ScalerState, TaggedWord,
};
pub use ordmap::OrderedMap;
pub use stemmer::{Stemmer, StemmerOf, default_stemmer};
