//! The document-oriented classifiers: `BayesClassifier` and
//! `LogisticRegressionClassifier`.
//!
//! Both share one shell — tokenise, accumulate a feature vocabulary, turn
//! documents into 0/1 vectors, hand them to an engine — over two unrelated
//! engines. See [`Classifier`], [`BayesEngine`] and [`LogisticEngine`].

mod bayes;
pub(crate) mod classifier;
mod logistic;

pub use bayes::{BayesClassifier, BayesEngine};
pub use classifier::{
    Classification, Classifier, ClassifierError, Document, Engine, LoadError, Observation,
    TrainingEvent,
};
pub use logistic::{LogisticEngine, LogisticRegressionClassifier};
