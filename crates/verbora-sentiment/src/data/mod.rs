//! Vocabularies and negation lists, dumped from the reference tree.
//!
//! MACHINE-DERIVED from the reference tables — do not edit by hand.
//!
//! # Blob format
//!
//! Each `*.bin` is `key \0 polarity \0` repeated, entries in the order the
//! source JSON lists them. That order is load-bearing: with a stemmer the
//! vocabulary is rebuilt by iterating it and letting later stems overwrite
//! earlier ones, so a stem collision's winner is the last colliding key in file
//! order. The generator asserts that no source has an array-index-like key,
//! which is the only way the reference's `for...in` would depart from file order.
//!
//! Polarities are kept as the original text — `"3"`, `"-0.30"`, `"0.00"` —
//! because their the reference *type* is observable through `vocabulary[word]`:
//! AFINN values are numbers, senticon and pattern polarities are strings.
//! Every one is a plain decimal, the form on which the reference's ToNumber and
//! Rust's `str::parse` are both correctly rounded and therefore identical.
//!
//! # Size
//!
//! 75803 entries, 1230918 bytes of `rodata` across 13 files. The source JSON
//! is ~7.5 MB; all but the one polarity field per entry is discarded, since no
//! code path in `SentimentAnalyzer` can read `wordnet_id`, `subjectivity`,
//! `confidence` or `sense`.

pub mod ordered_object;

use crate::VocabularyKind;

/// The reference `negations_*.json`, `.words`, in file order.
pub static NEGATIONS_ENGLISH: &[&str] = &["not", "no", "never", "neither"];

/// The reference `negations_*.json`, `.words`, in file order.
pub static NEGATIONS_SPANISH: &[&str] = &["no", "nunca", "jamás", "ni"];

/// The reference `negations_*.json`, `.words`, in file order.
pub static NEGATIONS_DUTCH: &[&str] = &["niet", "nooit", "niemand", "niets", "nee", "neen"];

/// The reference `negations_*.json`, `.words`, in file order.
pub static NEGATIONS_PORTUGUESE: &[&str] = &["não", "nunca", "jamais", "nem"];

/// The reference `negations_*.json`, `.words`, in file order.
pub static NEGATIONS_GERMAN: &[&str] = &["kein", "nein", "nicht"];

/// One row of the reference's `languageFiles` table.
pub struct Source {
    /// Which vocabulary family this row belongs to.
    pub kind: VocabularyKind,
    /// The language name the constructor matches on, exactly as spelled there.
    pub language: &'static str,
    /// The packed `key \0 polarity \0` blob, or `None` for an empty table.
    pub blob: Option<&'static [u8]>,
    /// Number of entries in `blob`, so the loader can size its map up front.
    pub entries: usize,
    /// The negation list this row is paired with; empty where the table has none.
    pub negations: &'static [&'static str],
}

/// Number of rows in [`SOURCES`], as a constant so the lazy cache can be an array.
pub const SOURCE_COUNT: usize = 14;

/// Every `(type, language)` pair `languageFiles` accepts, in table order.
pub static SOURCES: &[Source] = &[
    Source {
        kind: VocabularyKind::Afinn,
        language: "English",
        blob: Some(include_bytes!("afinn_english.bin")),
        entries: 3382,
        negations: NEGATIONS_ENGLISH,
    },
    Source {
        kind: VocabularyKind::Afinn,
        language: "Spanish",
        blob: Some(include_bytes!("afinn_spanish.bin")),
        entries: 1653,
        negations: NEGATIONS_SPANISH,
    },
    Source {
        kind: VocabularyKind::Afinn,
        language: "Portuguese",
        blob: Some(include_bytes!("afinn_portuguese.bin")),
        entries: 1644,
        negations: NEGATIONS_PORTUGUESE,
    },
    Source {
        kind: VocabularyKind::AfinnFinancialMarketNews,
        language: "English",
        blob: None,
        entries: 0,
        negations: NEGATIONS_ENGLISH,
    },
    Source {
        kind: VocabularyKind::Senticon,
        language: "Spanish",
        blob: Some(include_bytes!("senticon_spanish.bin")),
        entries: 11344,
        negations: NEGATIONS_SPANISH,
    },
    Source {
        kind: VocabularyKind::Senticon,
        language: "English",
        blob: Some(include_bytes!("senticon_english.bin")),
        entries: 24839,
        negations: NEGATIONS_ENGLISH,
    },
    Source {
        kind: VocabularyKind::Senticon,
        language: "Galician",
        blob: Some(include_bytes!("senticon_galician.bin")),
        entries: 4885,
        negations: &[],
    },
    Source {
        kind: VocabularyKind::Senticon,
        language: "Catalan",
        blob: Some(include_bytes!("senticon_catalan.bin")),
        entries: 7270,
        negations: &[],
    },
    Source {
        kind: VocabularyKind::Senticon,
        language: "Basque",
        blob: Some(include_bytes!("senticon_basque.bin")),
        entries: 4311,
        negations: &[],
    },
    Source {
        kind: VocabularyKind::Pattern,
        language: "Dutch",
        blob: Some(include_bytes!("pattern_dutch.bin")),
        entries: 3304,
        negations: NEGATIONS_DUTCH,
    },
    Source {
        kind: VocabularyKind::Pattern,
        language: "Italian",
        blob: Some(include_bytes!("pattern_italian.bin")),
        entries: 3065,
        negations: &[],
    },
    Source {
        kind: VocabularyKind::Pattern,
        language: "English",
        blob: Some(include_bytes!("pattern_english.bin")),
        entries: 1528,
        negations: NEGATIONS_ENGLISH,
    },
    Source {
        kind: VocabularyKind::Pattern,
        language: "French",
        blob: Some(include_bytes!("pattern_french.bin")),
        entries: 5113,
        negations: &[],
    },
    Source {
        kind: VocabularyKind::Pattern,
        language: "German",
        blob: Some(include_bytes!("pattern_german.bin")),
        entries: 3465,
        negations: NEGATIONS_GERMAN,
    },
];
