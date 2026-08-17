//! The document-oriented classifiers: `BayesClassifier` and
//! `LogisticRegressionClassifier`.
//!
//! Both are the same the reference shell — tokenise, accumulate a feature
//! vocabulary, turn documents into 0/1 vectors, hand them to an engine — over
//! two very different `apparatus` engines. The shell is in [`classifier`], the
//! engines in [`bayes`] and [`logistic`].

pub mod bayes;
pub mod classifier;
pub mod logistic;

pub use bayes::{BayesClassifier, BayesEngine};
pub use classifier::{
    Classification, Classifier, ClassifierError, Document, Engine, LoadError, Observation,
    TrainingEvent,
};
pub use logistic::{LogisticEngine, LogisticRegressionClassifier};
