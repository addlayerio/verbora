//! The stemmer that keyed a model's features is part of its compatibility
//! stamp, and restoring under a different one is refused.
//!
//! # The defect this file pins
//!
//! A feature key is a **stem**. `Classifier::restore` rebuilds with
//! `default_stemmer()` — English Porter — whatever stemmer trained the model,
//! because a saved model recorded nothing about which one it was. So a French
//! classifier, saved and restored, kept a feature table of French stems and
//! probed it with English ones. `chantait` stems to `chant` under
//! `PorterStemmerFr` and to `chantait` under English Porter, so the restored
//! classifier's probe missed a feature its own vocabulary held, and every
//! number in the model stayed arithmetically valid while the answer changed.
//!
//! That is the transform-then-lookup shape: an index built by one derivation
//! and consulted by another. It is now a `StampError::Incompatible` — the
//! stamp carries `stemmer_fingerprint`, and `restore` demands the fingerprint
//! of whatever stemmer it is about to rebuild with.

use std::sync::Arc;

use verbora_classifiers::{
    ArtifactStamp, BayesClassifier, LoadError, StampError, Stemmer, StemmerOf, default_stemmer,
    stemmer_fingerprint,
};
use verbora_stemmers::PorterStemmerFr;

/// A French-stemmed classifier, and the probe that discriminates.
fn french_model() -> (Arc<dyn Stemmer + Send + Sync>, BayesClassifier) {
    let french: Arc<dyn Stemmer + Send + Sync> = Arc::new(StemmerOf(PorterStemmerFr::new()));
    let mut c = BayesClassifier::with_stemmer(Arc::clone(&french));
    c.add_document("les chiens chantaient doucement pendant la nuit", "a");
    c.add_document("nous mangeons des pommes rouges au jardin", "b");
    c.add_document("elle finissait ses devoirs lentement", "a");
    c.train().expect("three labelled documents train");
    (french, c)
}

/// The two stemmers really do key `chantait` differently — the premise the rest
/// of the file rests on, stated rather than assumed.
#[test]
fn the_two_stemmers_disagree_about_the_probe() {
    let (french, c) = french_model();
    let english = default_stemmer();
    assert_eq!(french.tokenize_and_stem("chantait", true), ["chant"]);
    assert_eq!(english.tokenize_and_stem("chantait", true), ["chantait"]);
    // …and `chant` is a feature of the trained model, while `chantait` is not.
    assert!(c.feature_order().contains(&"chant"));
    assert!(!c.feature_order().contains(&"chantait"));
}

/// Restoring under the wrong stemmer is refused, and the error names both
/// fingerprints.
#[test]
fn a_model_restored_under_another_stemmer_is_refused() {
    let (french, c) = french_model();
    let json = c.to_json();

    let Err(LoadError::Stamp(StampError::Incompatible(mismatch))) = BayesClassifier::restore(&json)
    else {
        panic!("a French-keyed model must not restore under the English default");
    };
    assert_eq!(mismatch.found.stemmer, Some(stemmer_fingerprint(&*french)));
    assert_eq!(
        mismatch.expected.stemmer,
        Some(stemmer_fingerprint(&*default_stemmer()))
    );
    assert_ne!(mismatch.found.stemmer, mismatch.expected.stemmer);
    // Only the stemmer differs: everything else about the build agrees.
    assert_eq!(mismatch.found.schema, mismatch.expected.schema);
    assert_eq!(mismatch.found.unicode, mismatch.expected.unicode);
    assert_eq!(mismatch.found.lowercase, mismatch.expected.lowercase);
    // The message tells the caller what to do about it.
    let text = StampError::Incompatible(mismatch).to_string();
    assert!(text.contains("different stemmer"), "{text}");
    assert!(text.contains("retrain"), "{text}");
}

/// …and naming the right stemmer restores a classifier that derives features
/// exactly as the saved one did.
#[test]
fn restoring_with_the_training_stemmer_reproduces_every_feature_vector() {
    let (french, c) = french_model();
    let revived = BayesClassifier::restore_with(&c.to_json(), Arc::clone(&french))
        .expect("the stemmer that keyed it is accepted");

    for probe in [
        "chantait",
        "chantaient",
        "mangeons",
        "finissait",
        "les chiens",
        "rien de tout cela",
        "",
    ] {
        assert_eq!(
            revived.text_to_features(probe),
            c.text_to_features(probe),
            "{probe}"
        );
        assert_eq!(
            revived.get_classifications(probe).map(|v| {
                v.into_iter()
                    .map(|s| (s.label, s.value.to_bits()))
                    .collect::<Vec<_>>()
            }),
            c.get_classifications(probe).map(|v| {
                v.into_iter()
                    .map(|s| (s.label, s.value.to_bits()))
                    .collect::<Vec<_>>()
            }),
            "{probe}"
        );
    }
    assert_eq!(revived.to_json(), c.to_json());
}

/// A model saved by the default stemmer still round-trips through the plain
/// entry point, so the common case pays nothing for the check.
#[test]
fn the_default_stemmer_round_trips_through_restore() {
    let mut c = BayesClassifier::new();
    c.add_document("my unit-tests failed.", "software");
    c.add_document("tomorrow we will do standard tests", "other");
    c.train().unwrap();
    let revived = BayesClassifier::restore(&c.to_json()).expect("the default stemmer keyed it");
    assert_eq!(revived.to_json(), c.to_json());
    assert_eq!(
        ArtifactStamp::for_stemmer(&*default_stemmer()).stemmer,
        Some(stemmer_fingerprint(&*default_stemmer()))
    );
}
