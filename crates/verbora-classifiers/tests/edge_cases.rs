//! Boundary inputs, applied uniformly across the three classifiers.
//!
//! The parity suite already proves agreement with the reference on a large recorded
//! corpus. What it does not do is state, in one readable place, what happens at
//! the edges — and several of these answers are surprising enough that a future
//! reader deserves to find them asserted rather than inferred:
//!
//! * an empty document is dropped in silence, and so is one made entirely of
//!   stop words;
//! * a bare `"7"` is a stop word, so a document of digits can vanish;
//! * an astral-plane token is an ordinary feature key, because a classifier
//!   compares feature keys for equality and never indexes into one — note that
//!   this is a claim about the *classifier*, and the documents below are given
//!   as token slices, which `Observation::Tokens` hands over without stemming.
//!   On the `Observation::Text` path a token is very much indexed, by the
//!   stemmer in front of the classifier, which measures text in Unicode scalar
//!   values; that boundary is what `ArtifactStamp` fingerprints and what
//!   `tests/stemmer_stamp.rs` covers;
//! * a maximum-entropy predicate is an opaque string, so an astral-plane
//!   predicate is an ordinary feature key and a context of nothing the model
//!   knows is scored by the uniform distribution rather than refused.

use verbora_classifiers::{
    BayesClassifier, DynValue, Gis, LogisticRegressionClassifier, MaxEntClassifier, StopReason,
    number_to_string,
};

/// One document per category of awkward input, all given as token slices so the
/// tokenizer cannot quietly drop them.
fn categories() -> Vec<(&'static str, Vec<String>)> {
    [
        ("single-char", vec!["q"]),
        ("uppercase", vec!["ALLCAPS", "MiXeD"]),
        ("accented-latin", vec!["café", "naïve", "Ångström"]),
        ("cyrillic", vec!["Москва", "Ленинград"]),
        ("greek", vec!["Ελλάδα", "Αθήνα"]),
        ("cjk", vec!["日本語", "中文测试", "한국어"]),
        ("astral", vec!["😀", "a😀b", "𝕳𝖊𝖑𝖑𝖔"]),
        ("punctuation", vec!["...", "!", "--", "don't"]),
        ("digits", vec!["0", "42", "3.14", "1000"]),
        ("combining", vec!["e\u{301}", "é"]),
    ]
    .into_iter()
    .map(|(name, tokens)| {
        (
            name,
            tokens.into_iter().map(str::to_owned).collect::<Vec<_>>(),
        )
    })
    .collect()
}

#[test]
fn bayes_survives_every_category() {
    let mut classifier = BayesClassifier::new();
    for (name, tokens) in categories() {
        classifier.add_document(&tokens, name);
    }
    classifier.train().expect("Bayes training cannot fail");

    for (name, tokens) in categories() {
        assert_eq!(
            classifier.classify(&tokens).expect("trained"),
            name,
            "{name} should classify as itself"
        );
    }
    // The vocabulary holds one slot per distinct token, in the order the tokens
    // were first added — every category's tokens land where they were fed, and
    // the digit tokens `"0"` and `"42"` take their turn like the rest instead of
    // being sorted to the front.
    let expected: Vec<String> = categories()
        .into_iter()
        .flat_map(|(_, tokens)| tokens)
        .collect();
    assert_eq!(classifier.feature_order(), expected);
}

#[test]
fn logistic_regression_survives_every_category() {
    let mut classifier = LogisticRegressionClassifier::new();
    for (name, tokens) in categories() {
        classifier.add_document(&tokens, name);
    }
    classifier.train().expect("the corpus has examples");
    for (name, tokens) in categories() {
        assert_eq!(classifier.classify(&tokens).expect("trained"), name);
    }
}

#[test]
fn empty_and_stop_word_only_documents_vanish() {
    let mut classifier = BayesClassifier::new();
    for text in ["", " ", "   ", "\t\n\r", "the a of", "!!! ...", "x", "7"] {
        classifier.add_document(text, "dropped");
    }
    assert_eq!(
        classifier.docs().len(),
        0,
        "the default stop-word list contains single letters and digits, so \
         even \"x\" and \"7\" tokenize to nothing"
    );
    assert_eq!(classifier.features().len(), 0);
    // An empty token slice is dropped too.
    classifier.add_document(&Vec::<String>::new(), "dropped");
    assert_eq!(classifier.docs().len(), 0);
}

#[test]
fn a_very_long_document_is_linear_and_deduplicated() {
    let long: String = "lorem ipsum dolor sit amet ".repeat(4_000);
    let mut classifier = BayesClassifier::new();
    classifier.add_document(long.as_str(), "long");
    classifier.add_document("something else entirely", "short");
    classifier.train().expect("Bayes training cannot fail");

    // 20 000 words, but only the five distinct stems survive deduplication.
    let doc = &classifier.docs()[0];
    assert_eq!(doc.text.len(), 20_000);
    assert_eq!(classifier.features().get("lorem"), Some(&4_000.0));
    assert_eq!(classifier.classify(long.as_str()).expect("trained"), "long");
}

#[test]
fn a_single_character_document_trains_and_classifies() {
    let mut classifier = BayesClassifier::new();
    classifier.add_document(&vec!["q".to_owned()], "solo");
    classifier.train().expect("Bayes training cannot fail");
    assert_eq!(classifier.feature_order(), vec!["q"]);
    assert_eq!(classifier.classify(&vec!["q".to_owned()]).unwrap(), "solo");
    // An observation sharing no features still scores, because Bayes falls back
    // to the class prior.
    assert_eq!(classifier.classify(&vec!["z".to_owned()]).unwrap(), "solo");
}

// 3.14 is a fixture-style input, not an approximation of pi.
#[allow(clippy::approx_constant)]
#[test]
fn serialised_values_cover_every_category() {
    for (payload, want) in [
        (DynValue::Str(String::new()), "\"\""),
        (DynValue::Str("q".into()), "\"q\""),
        (DynValue::Str("ALLCAPS".into()), "\"ALLCAPS\""),
        (DynValue::Str("café".into()), "\"café\""),
        (DynValue::Str("Москва".into()), "\"Москва\""),
        (DynValue::Str("Ελλάδα".into()), "\"Ελλάδα\""),
        (DynValue::Str("日本語".into()), "\"日本語\""),
        (DynValue::Str("😀".into()), "\"😀\""),
        (DynValue::Str("...".into()), "\"...\""),
        (DynValue::Num(42.0), "42"),
        (DynValue::Num(3.14), "3.14"),
        (DynValue::Num(-0.0), "0"),
        (DynValue::Num(f64::NAN), "null"),
        (DynValue::Null, "null"),
        (DynValue::Bool(true), "true"),
        (DynValue::Arr(vec![]), "[]"),
    ] {
        assert_eq!(
            payload.json_stringify().as_deref(),
            Some(want),
            "{payload:?}"
        );
    }
}

#[test]
fn a_very_long_serialised_string_round_trips() {
    let long = "é😀".repeat(20_000);
    let json = DynValue::Str(long.clone())
        .json_stringify()
        .expect("a string always stringifies");
    assert_eq!(json.chars().count(), long.chars().count() + 2);
    assert_eq!(DynValue::parse(&json).unwrap(), DynValue::Str(long));
}

#[test]
fn maxent_trains_over_unicode_outcomes_and_predicates() {
    let mut classifier = MaxEntClassifier::new();
    for (outcome, predicate) in [
        ("Ελλάδα", "π=Αθήνα"),
        ("Ελλάδα", "π=Αθήνα"),
        ("😀", "π=🎉"),
        ("日本語", "π=東京"),
    ] {
        classifier.add(outcome, [predicate]);
    }
    assert_eq!(classifier.sample().outcomes(), ["Ελλάδα", "😀", "日本語"]);

    // Every predicate here occurs with exactly one outcome, so the constraints
    // ask for p = 1 and the maximum-likelihood weights are unbounded: the fit
    // approaches them logarithmically and reports that it ran out of iterations
    // rather than claiming convergence.
    let report = *classifier
        .train_with(Gis::new(2_000, 1e-12).unwrap())
        .expect("four events");
    assert_eq!(report.stop, StopReason::MaxIterations);
    assert!(report.log_likelihood.is_finite());

    // Every predicate occurs with exactly one outcome, so each context resolves
    // to the outcome it was seen with.
    for (outcome, predicate) in [("Ελλάδα", "π=Αθήνα"), ("😀", "π=🎉"), ("日本語", "π=東京")]
    {
        assert_eq!(classifier.classify([predicate]).unwrap(), outcome);
        let scores = classifier.get_classifications([predicate]).unwrap();
        assert!(
            (scores.iter().map(|s| s.value).sum::<f64>() - 1.0).abs() < 1e-12,
            "{scores:?}"
        );
    }

    // A context of nothing the model knows is uniform, which is the
    // maximum-entropy answer under no constraint — not an abstention.
    let scores = classifier.get_classifications(["π=unseen"]).unwrap();
    for score in &scores {
        assert!((score.value - 1.0 / 3.0).abs() < 1e-15, "{scores:?}");
    }
    assert_eq!(classifier.classify(["π=unseen"]).unwrap(), "Ελλάδα");
}

#[test]
fn a_maxent_model_round_trips_through_a_file() {
    let mut classifier = MaxEntClassifier::new();
    for (name, tokens) in categories() {
        classifier.add(name, tokens);
    }
    classifier
        .train_with(Gis::new(500, 1e-12).unwrap())
        .expect("one event per category");

    let path = std::env::temp_dir().join(format!(
        "verbora-maxent-edge-{}-{}.json",
        std::process::id(),
        line!()
    ));
    classifier
        .save(&path)
        .expect("the temp directory is writable");
    let revived = MaxEntClassifier::load(&path).expect("what was just written parses");
    std::fs::remove_file(&path).ok();

    assert_eq!(revived.to_json(), classifier.to_json());
    for (name, tokens) in categories() {
        assert_eq!(revived.classify(&tokens).expect("trained"), name);
    }
}

#[test]
fn number_formatting_matches_the_reference_at_the_boundaries() {
    for (value, want) in [
        (0.0, "0"),
        (-0.0, "0"),
        (1.0, "1"),
        (1e20, "100000000000000000000"),
        (1e21, "1e+21"),
        (1e-6, "0.000001"),
        (1e-7, "1e-7"),
        (f64::MIN_POSITIVE, "2.2250738585072014e-308"),
        (f64::MAX, "1.7976931348623157e+308"),
    ] {
        assert_eq!(number_to_string(value), want, "{value}");
    }
}

#[test]
fn save_and_load_round_trip_through_a_file() {
    let mut classifier = BayesClassifier::new();
    for (name, tokens) in categories() {
        classifier.add_document(&tokens, name);
    }
    classifier.train().expect("Bayes training cannot fail");

    let path = std::env::temp_dir().join(format!(
        "verbora-classifiers-{}-{}.json",
        std::process::id(),
        line!()
    ));
    classifier
        .save(&path)
        .expect("the temp directory is writable");
    let revived = BayesClassifier::load(&path).expect("what was just written parses");
    std::fs::remove_file(&path).ok();

    assert_eq!(revived.to_json(), classifier.to_json());
    for (name, tokens) in categories() {
        assert_eq!(revived.classify(&tokens).expect("trained"), name);
    }
}

#[test]
fn serialised_bytes_survive_a_unicode_round_trip() {
    let mut classifier = BayesClassifier::new();
    for (name, tokens) in categories() {
        classifier.add_document(&tokens, name);
    }
    classifier.train().expect("Bayes training cannot fail");
    let json = classifier.to_json();
    let revived = BayesClassifier::restore(&json).expect("saved bytes reparse");
    assert_eq!(revived.to_json(), json);
    for (name, tokens) in categories() {
        assert_eq!(revived.classify(&tokens).expect("trained"), name);
    }
}
