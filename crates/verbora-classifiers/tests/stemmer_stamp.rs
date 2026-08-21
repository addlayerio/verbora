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
    ArtifactStamp, BayesClassifier, DynValue, LoadError, SCHEMA, STAMP_PROPERTY, StampError,
    Stemmer, StemmerOf, default_stemmer, stemmer_fingerprint,
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

// ---------------------------------------------------------------------------
// The build that measured text in UTF-16 code units.
// ---------------------------------------------------------------------------

/// The twelve probes `stemmer_fingerprint` absorbed before the text unit moved.
///
/// A historical fact, written out here so this file can compute the stamp a
/// pre-migration build wrote without owning a copy of that build. Every one of
/// the twelve stays on the basic multilingual plane, which is the whole reason
/// the fingerprint could not see the change: counting characters and counting
/// UTF-16 code units are the same function on the BMP.
///
/// `STEMMER_PROBES` still opens with all twelve in this order and now carries
/// an astral probe after them, so this list is a prefix of it rather than a
/// second corpus that could drift.
const CODE_UNIT_BUILD_PROBES: [&str; 12] = [
    "the running dogs jumped over relational happiness quickly",
    "conditional formalize electricity sensitivity hopefulness adjustable",
    "les chiens chantaient doucement pendant la nuit entière",
    "die Häuser wurden gestern Abend vollständig renoviert",
    "los niños corrían rápidamente hacia la montaña nevada",
    "as crianças brincavam alegremente no jardim florido",
    "de honden renden snel door het bos heen",
    "i bambini correvano velocemente verso la montagna",
    "быстрые коричневые лисицы прыгают через ленивых собак",
    "барвисті українські міста зустрічають гостей щоранку",
    "日本語のテキストを解析します",
    "unit-tests 3.14 İD ΟΔΌΣ naïve",
];

/// The schema counter that build wrote.
const CODE_UNIT_BUILD_SCHEMA: f64 = 3.0;

/// `stemmer_fingerprint`'s documented definition over [`CODE_UNIT_BUILD_PROBES`].
///
/// Transcribed from the doc comment on `verbora_classifiers::stemmer_fingerprint`
/// — FNV-1a with offset basis `0xcbf29ce484222325` and prime `0x100000001b3`, a
/// `0xFF` after every token and a `0xFE` closing every probe — rather than
/// obtained by calling it, so the value this file compares against is derived
/// from the specification and not from whatever the crate currently does.
fn code_unit_build_fingerprint(stemmer: &(impl Stemmer + ?Sized)) -> u64 {
    fn feed(hash: &mut u64, bytes: &[u8]) {
        for byte in bytes {
            *hash ^= u64::from(*byte);
            *hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for probe in CODE_UNIT_BUILD_PROBES {
        for token in stemmer.tokenize_and_stem(probe, true) {
            feed(&mut hash, token.as_bytes());
            feed(&mut hash, &[0xFF]);
        }
        feed(&mut hash, &[0xFE]);
    }
    hash
}

/// `saved` with its stamp replaced by the one a pre-migration build wrote:
/// schema 3 and a stemmer fingerprint taken over the twelve BMP probes, with
/// the Unicode version and the lowercase mapping left as this build's — so the
/// text unit is the only thing that differs.
fn stamped_as_the_code_unit_build(saved: &DynValue, fingerprint: u64) -> DynValue {
    let DynValue::Obj(members) = saved else {
        panic!("a saved classifier is a JSON object");
    };
    let current = ArtifactStamp::for_stemmer(&*default_stemmer());
    let (major, minor, update) = current.unicode;
    let older = DynValue::Obj(vec![
        ("schema".to_owned(), DynValue::Num(CODE_UNIT_BUILD_SCHEMA)),
        (
            "unicode".to_owned(),
            DynValue::Str(format!("{major}.{minor}.{update}")),
        ),
        (
            "lowercase".to_owned(),
            DynValue::Str(format!(
                "{:016x}",
                current.lowercase.expect("this build records its case fold")
            )),
        ),
        (
            "stemmer".to_owned(),
            DynValue::Str(format!("{fingerprint:016x}")),
        ),
    ]);
    DynValue::Obj(
        members
            .iter()
            .map(|(name, value)| {
                if name == STAMP_PROPERTY {
                    (name.clone(), older.clone())
                } else {
                    (name.clone(), value.clone())
                }
            })
            .collect(),
    )
}

/// The premise: the two builds really do key an astral-bearing document
/// differently.
///
/// Derived from `verbora-stemmers`' own contract rather than recorded. English
/// Porter returns a token untouched when it is shorter than three characters
/// and otherwise applies its suffix rules, and `U+1D573` MATHEMATICAL BOLD
/// FRAKTUR CAPITAL H is one character and two UTF-16 code units. So `"𝕳s"` is
/// two characters — below the gate, returned whole — and three code units,
/// which is above it; and `"fli𝕳es"` is six characters, where step 1a's
/// `-es`/`-e` arithmetic lands one position further left when the same string
/// is measured as seven units.
///
/// Astral text is not a curiosity here: 97,491 astral scalars are `Alphabetic`,
/// so they form whole UAX #29 word tokens and reach the stemmer as ordinary
/// words.
#[test]
fn the_two_units_key_an_astral_document_differently() {
    let stemmer = default_stemmer();

    assert_eq!("𝕳s".chars().count(), 2);
    assert_eq!("𝕳s".encode_utf16().count(), 3);
    assert_eq!(stemmer.tokenize_and_stem("𝕳s", true), ["𝕳s"]);

    assert_eq!("fli𝕳es".chars().count(), 6);
    assert_eq!("fli𝕳es".encode_utf16().count(), 7);
    assert_eq!(stemmer.tokenize_and_stem("fli𝕳es", true), ["fli𝕳e"]);

    // …and the same document read in code units keyed both of them one
    // character shorter, which is a different feature in the same model.
    assert_ne!(stemmer.tokenize_and_stem("𝕳s", true), ["𝕳"]);
    assert_ne!(stemmer.tokenize_and_stem("fli𝕳es", true), ["fli𝕳"]);
}

/// A model saved by the build that measured text in UTF-16 code units is
/// refused, on both of the two facts that moved.
///
/// # The defect this pins
///
/// `verbora-stemmers` used to index text by UTF-16 code unit and now indexes it
/// by Unicode scalar value. Every region bound, length gate and cut position
/// moved with it, so the same document yields different feature keys whenever
/// it carries a character outside the basic multilingual plane.
///
/// Three of the stamp's four facts are structurally blind to that: `unicode`
/// describes `unicode-segmentation`, `lowercase` describes `str::to_lowercase`,
/// and `stemmer` described what a stemmer does to twelve probes that never left
/// the BMP — where the two units are the same function by definition. So a
/// model trained under the old unit loaded silently under the new one and
/// mispredicted on exactly the documents the change was about, with every
/// number in it still arithmetically valid.
///
/// It is refused twice over now. [`SCHEMA`] was bumped by hand, which is the
/// obligation `stamp.rs` states in prose; and the probe corpus gained an astral
/// probe, so the fingerprint sees the unit without anyone having to remember
/// the counter next time. Both are asserted separately below, because either
/// one alone would let this test pass while the other regressed.
#[test]
fn a_model_saved_by_the_code_unit_build_is_refused() {
    let mut c = BayesClassifier::new();
    c.add_document("the running dogs jumped over relational happiness", "a");
    c.add_document("conditional formalize electricity sensitivity", "b");
    c.train().expect("two labelled documents train");

    let stemmer = default_stemmer();
    let older = code_unit_build_fingerprint(&*stemmer);
    let saved = stamped_as_the_code_unit_build(&c.to_value(), older);

    let Err(StampError::Incompatible(mismatch)) = BayesClassifier::from_value(&saved) else {
        panic!(
            "a model keyed under the UTF-16 code unit must be refused; schema \
             {CODE_UNIT_BUILD_SCHEMA} and stemmer fingerprint {older:016x} are \
             what such a model carries"
        );
    };

    // Refused on the hand-bumped counter…
    assert_eq!(mismatch.found.schema, 3);
    assert_eq!(mismatch.expected.schema, SCHEMA);
    // Checked at compile time, because it is a claim about this build rather
    // than about the model under test: moving the text unit re-keys every
    // astral-bearing document, so it is a schema bump.
    const { assert!(SCHEMA > 3) };

    // …and independently on the fingerprint, so the counter is a second line of
    // defence rather than the only one.
    assert_eq!(mismatch.found.stemmer, Some(older));
    assert_eq!(
        mismatch.expected.stemmer,
        Some(stemmer_fingerprint(&*stemmer))
    );
    assert_ne!(
        mismatch.found.stemmer, mismatch.expected.stemmer,
        "the probe corpus does not distinguish the two text units"
    );

    // Everything else about the build agrees, so those two are the whole of the
    // difference and neither assertion above is passing by accident.
    assert_eq!(mismatch.found.unicode, mismatch.expected.unicode);
    assert_eq!(mismatch.found.lowercase, mismatch.expected.lowercase);

    // The message tells the caller what to do about it.
    let text = StampError::Incompatible(mismatch).to_string();
    assert!(text.contains("retrain"), "{text}");
}

/// A model saved by *this* build still loads, so the refusal above is aimed at
/// the pre-migration stamp rather than at every stamp.
#[test]
fn a_model_saved_by_this_build_still_loads() {
    let mut c = BayesClassifier::new();
    c.add_document("the running dogs jumped over relational happiness", "a");
    c.add_document("conditional formalize electricity sensitivity", "b");
    c.train().expect("two labelled documents train");
    let revived = BayesClassifier::from_value(&c.to_value()).expect("this build keyed it");
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
