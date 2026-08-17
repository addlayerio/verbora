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
//! * an astral-plane token is an ordinary feature key, because feature keys are
//!   compared for equality and never indexed;
//! * a maxent context key is `safe-stable-stringify` output, so non-ASCII is
//!   emitted raw and object keys are sorted by UTF-16 code unit.

use std::cell::RefCell;
use std::rc::Rc;

use verbora_classifiers::{
    BayesClassifier, Context, DynValue, FeatureSet, LogisticRegressionClassifier, MaxEntClassifier,
    SEElement, Sample, dynval,
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
    // The vocabulary holds one slot per distinct token, and `"0"` and `"42"` are
    // array-index keys, so they enumerate first.
    let order = classifier.feature_order();
    assert_eq!(&order[..2], &["0", "42"]);
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
fn maxent_context_keys_cover_every_category() {
    // `safe-stable-stringify` emits non-ASCII raw and sorts object keys by
    // UTF-16 code unit, so none of these are escaped and the astral key sorts
    // before U+FFFD.
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
            Context::new(payload.clone()).to_key().as_deref(),
            Some(want),
            "{payload:?}"
        );
    }

    let sorted = DynValue::Obj(vec![
        ("\u{fffd}".to_owned(), DynValue::Num(1.0)),
        ("😀".to_owned(), DynValue::Num(2.0)),
        ("é".to_owned(), DynValue::Num(3.0)),
        ("a".to_owned(), DynValue::Num(4.0)),
    ]);
    assert_eq!(
        Context::new(sorted).to_key().unwrap(),
        "{\"a\":4,\"é\":3,\"😀\":2,\"\u{fffd}\":1}"
    );
}

#[test]
fn a_very_long_context_key_round_trips() {
    let long = "é😀".repeat(20_000);
    let context = Context::new(DynValue::Str(long.clone()));
    let key = context
        .to_key()
        .expect("a string payload always stringifies");
    assert_eq!(key.chars().count(), long.chars().count() + 2);
    // Memoised, so the second call is free and returns the identical string.
    assert_eq!(context.to_key().as_deref(), Some(key.as_str()));
}

#[test]
fn maxent_trains_over_unicode_classes_and_contexts() {
    let mut sample = Sample::new();
    for (class, data) in [
        ("Ελλάδα", "0"),
        ("Ελλάδα", "0"),
        ("😀", "1"),
        ("日本語", "1"),
    ] {
        sample.add_element(SEElement::new(class, Rc::new(Context::of_str(data))));
    }
    let mut features = FeatureSet::new();
    sample
        .generate_features(&mut features)
        .expect("SE elements");
    assert_eq!(sample.classes(), ["Ελλάδα", "😀", "日本語"]);

    let mut classifier = MaxEntClassifier::new(
        Rc::new(RefCell::new(features)),
        Rc::new(RefCell::new(sample)),
    );
    classifier.train(20, 0.01).expect("SE features are finite");
    // None of these classes is "x" or "y", so both features fire zero for every
    // element: C is 0, the exponent is infinite, and every score stays at 1 —
    // which makes the classifier abstain rather than guess.
    assert_eq!(
        classifier
            .classify(&Rc::new(Context::of_str("0")))
            .expect("classes exist"),
        ""
    );
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
        assert_eq!(dynval::number_to_string(value), want, "{value}");
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
