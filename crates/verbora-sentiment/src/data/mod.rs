//! The shipped `(kind, language)` table: packed vocabulary blobs and the
//! negation word lists they are paired with.
//!
//! MACHINE-DERIVED from the upstream lexicon releases — do not edit by hand.
//!
//! # Blob format
//!
//! Each `*.bin` is `key \0 polarity \0` repeated, entries in the order the
//! upstream file lists them. That order is load-bearing: with a stemmer the
//! vocabulary is rebuilt by iterating it and letting later stems overwrite
//! earlier ones, so a stem collision's winner is the last colliding key in file
//! order.
//!
//! Polarities are kept as the upstream file's own text — `"3"`, `"-0.30"`,
//! `"0.00"` — because how a value was published is observable through
//! [`Polarity::as_written`](crate::Polarity::as_written): AFINN writes
//! integers, senticon and pattern write decimal strings. Every one is a plain
//! finite decimal, so `str::parse` is correctly rounded and the stored `f64` is
//! exact; `tests/reachability.rs` enumerates all 75,803 of them and asserts it.
//!
//! # Size
//!
//! 75803 entries, 1230918 bytes of `rodata` across 13 files. The upstream JSON
//! is ~7.5 MB; all but the one polarity field per entry is discarded, since no
//! code path in `SentimentAnalyzer` can read `wordnet_id`, `subjectivity`,
//! `confidence` or `sense`.

use crate::{Language, VocabularyKind};

/// The upstream negation word list for this language, in file order.
pub static NEGATIONS_ENGLISH: &[&str] = &["not", "no", "never", "neither"];

/// The upstream negation word list for this language, in file order.
pub static NEGATIONS_SPANISH: &[&str] = &["no", "nunca", "jamás", "ni"];

/// The upstream negation word list for this language, in file order.
pub static NEGATIONS_DUTCH: &[&str] = &["niet", "nooit", "niemand", "niets", "nee", "neen"];

/// The upstream negation word list for this language, in file order.
pub static NEGATIONS_PORTUGUESE: &[&str] = &["não", "nunca", "jamais", "nem"];

/// The upstream negation word list for this language, in file order.
pub static NEGATIONS_GERMAN: &[&str] = &["kein", "nein", "nicht"];

/// One shipped vocabulary: which family and language it is, its packed
/// entries, and the negation list it is paired with.
pub struct Source {
    /// Which vocabulary family this row belongs to.
    pub kind: VocabularyKind,
    /// Which language this row is for.
    pub language: Language,
    /// The packed `key \0 polarity \0` blob, or `None` for an empty table.
    pub blob: Option<&'static [u8]>,
    /// Number of entries in `blob`, so the loader can size its map up front.
    pub entries: usize,
    /// The negation list this row is paired with; empty where the table has none.
    pub negations: &'static [&'static str],
}

/// Number of rows in [`SOURCES`], as a constant so the lazy cache can be an array.
pub const SOURCE_COUNT: usize = 14;

/// Every `(kind, language)` pair with a shipped vocabulary, in table order.
pub static SOURCES: &[Source] = &[
    Source {
        kind: VocabularyKind::Afinn,
        language: Language::English,
        blob: Some(include_bytes!("afinn_english.bin")),
        entries: 3382,
        negations: NEGATIONS_ENGLISH,
    },
    Source {
        kind: VocabularyKind::Afinn,
        language: Language::Spanish,
        blob: Some(include_bytes!("afinn_spanish.bin")),
        entries: 1653,
        negations: NEGATIONS_SPANISH,
    },
    Source {
        kind: VocabularyKind::Afinn,
        language: Language::Portuguese,
        blob: Some(include_bytes!("afinn_portuguese.bin")),
        entries: 1644,
        negations: NEGATIONS_PORTUGUESE,
    },
    Source {
        kind: VocabularyKind::AfinnFinancialMarketNews,
        language: Language::English,
        blob: None,
        entries: 0,
        negations: NEGATIONS_ENGLISH,
    },
    Source {
        kind: VocabularyKind::Senticon,
        language: Language::Spanish,
        blob: Some(include_bytes!("senticon_spanish.bin")),
        entries: 11344,
        negations: NEGATIONS_SPANISH,
    },
    Source {
        kind: VocabularyKind::Senticon,
        language: Language::English,
        blob: Some(include_bytes!("senticon_english.bin")),
        entries: 24839,
        negations: NEGATIONS_ENGLISH,
    },
    Source {
        kind: VocabularyKind::Senticon,
        language: Language::Galician,
        blob: Some(include_bytes!("senticon_galician.bin")),
        entries: 4885,
        negations: &[],
    },
    Source {
        kind: VocabularyKind::Senticon,
        language: Language::Catalan,
        blob: Some(include_bytes!("senticon_catalan.bin")),
        entries: 7270,
        negations: &[],
    },
    Source {
        kind: VocabularyKind::Senticon,
        language: Language::Basque,
        blob: Some(include_bytes!("senticon_basque.bin")),
        entries: 4311,
        negations: &[],
    },
    Source {
        kind: VocabularyKind::Pattern,
        language: Language::Dutch,
        blob: Some(include_bytes!("pattern_dutch.bin")),
        entries: 3304,
        negations: NEGATIONS_DUTCH,
    },
    Source {
        kind: VocabularyKind::Pattern,
        language: Language::Italian,
        blob: Some(include_bytes!("pattern_italian.bin")),
        entries: 3065,
        negations: &[],
    },
    Source {
        kind: VocabularyKind::Pattern,
        language: Language::English,
        blob: Some(include_bytes!("pattern_english.bin")),
        entries: 1528,
        negations: NEGATIONS_ENGLISH,
    },
    Source {
        kind: VocabularyKind::Pattern,
        language: Language::French,
        blob: Some(include_bytes!("pattern_french.bin")),
        entries: 5113,
        negations: &[],
    },
    Source {
        kind: VocabularyKind::Pattern,
        language: Language::German,
        blob: Some(include_bytes!("pattern_german.bin")),
        entries: 3465,
        negations: NEGATIONS_GERMAN,
    },
];
