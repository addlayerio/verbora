//! The language identifier. User-facing prose lives on [`Language`], not
//! in this private module's `//!` block.

use std::fmt;
use std::str::FromStr;

/// A language Verbora has at least one language-specific strategy for
/// somewhere in the workspace (a tokenizer character class, a stemmer, a
/// sentiment lexicon, or — via this crate — a phonetic-strategy
/// recommendation).
///
/// This is deliberately **not** an exhaustive list of human languages, and
/// it is not the same enumeration as [`verbora_sentiment`](https://docs.rs/verbora-sentiment)'s
/// own `Language` (a ten-language subset scoped to that crate's own
/// lexicons) — the two are not interchangeable, and this crate does not
/// assume they will ever be unified. `Language` here answers "does *some*
/// Verbora subsystem know something about this language," not "is this a
/// real language."
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Language {
    /// ISO 639-1 `en`.
    English,
    /// ISO 639-1 `es`.
    Spanish,
    /// ISO 639-1 `pt`.
    Portuguese,
    /// ISO 639-1 `it`.
    Italian,
    /// ISO 639-1 `fr`.
    French,
    /// ISO 639-1 `de`.
    German,
    /// ISO 639-1 `nl`.
    Dutch,
    /// ISO 639-1 `ru`.
    Russian,
    /// ISO 639-1 `uk`.
    Ukrainian,
    /// ISO 639-1 `pl`.
    Polish,
    /// ISO 639-1 `fa`.
    Persian,
    /// ISO 639-1 `hi`.
    Hindi,
    /// ISO 639-1 `id`.
    Indonesian,
    /// ISO 639-1 `vi`.
    Vietnamese,
    /// ISO 639-1 `ja`.
    Japanese,
    /// ISO 639-1 `zh` — not script/region-qualified; see
    /// [`Language::iso639_1`]'s own doc comment.
    Chinese,
    /// ISO 639-1 `no` — the Norwegian macrolanguage. Verbora has no
    /// separate Bokmål/Nynorsk strategy anywhere, so the two are one
    /// variant here; a detector that distinguishes them (`whatlang` reports
    /// Bokmål) maps its answer onto this.
    Norwegian,
    /// ISO 639-1 `sv`.
    Swedish,
    /// ISO 639-1 `fi`.
    Finnish,
    /// ISO 639-1 `gl`.
    Galician,
    /// ISO 639-1 `ca`.
    Catalan,
    /// ISO 639-1 `eu`.
    Basque,
}

impl Language {
    /// Every language this type represents, in declaration order.
    pub const ALL: [Self; 22] = [
        Self::English,
        Self::Spanish,
        Self::Portuguese,
        Self::Italian,
        Self::French,
        Self::German,
        Self::Dutch,
        Self::Russian,
        Self::Ukrainian,
        Self::Polish,
        Self::Persian,
        Self::Hindi,
        Self::Indonesian,
        Self::Vietnamese,
        Self::Japanese,
        Self::Chinese,
        Self::Norwegian,
        Self::Swedish,
        Self::Finnish,
        Self::Galician,
        Self::Catalan,
        Self::Basque,
    ];

    /// The ISO 639-1 two-letter code, lowercase.
    ///
    /// Chinese uses `"zh"` (not a script/region-qualified tag — Verbora has
    /// no script-specific Chinese strategy to distinguish).
    #[must_use]
    pub const fn iso639_1(self) -> &'static str {
        match self {
            Self::English => "en",
            Self::Spanish => "es",
            Self::Portuguese => "pt",
            Self::Italian => "it",
            Self::French => "fr",
            Self::German => "de",
            Self::Dutch => "nl",
            Self::Russian => "ru",
            Self::Ukrainian => "uk",
            Self::Polish => "pl",
            Self::Persian => "fa",
            Self::Hindi => "hi",
            Self::Indonesian => "id",
            Self::Vietnamese => "vi",
            Self::Japanese => "ja",
            Self::Chinese => "zh",
            Self::Norwegian => "no",
            Self::Swedish => "sv",
            Self::Finnish => "fi",
            Self::Galician => "gl",
            Self::Catalan => "ca",
            Self::Basque => "eu",
        }
    }

    /// The English name, for display and error messages.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::English => "English",
            Self::Spanish => "Spanish",
            Self::Portuguese => "Portuguese",
            Self::Italian => "Italian",
            Self::French => "French",
            Self::German => "German",
            Self::Dutch => "Dutch",
            Self::Russian => "Russian",
            Self::Ukrainian => "Ukrainian",
            Self::Polish => "Polish",
            Self::Persian => "Persian",
            Self::Hindi => "Hindi",
            Self::Indonesian => "Indonesian",
            Self::Vietnamese => "Vietnamese",
            Self::Japanese => "Japanese",
            Self::Chinese => "Chinese",
            Self::Norwegian => "Norwegian",
            Self::Swedish => "Swedish",
            Self::Finnish => "Finnish",
            Self::Galician => "Galician",
            Self::Catalan => "Catalan",
            Self::Basque => "Basque",
        }
    }

    /// Parses an ISO 639-1 code, case-insensitively, or [`None`] for a code
    /// no Verbora subsystem has a strategy for.
    ///
    /// ASCII case folding only: the codes are all two ASCII letters, so
    /// there is no Unicode case-folding subtlety to get wrong, and no input
    /// — of any length, in any script — can panic.
    #[must_use]
    pub fn from_iso639_1(code: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|l| l.iso639_1().eq_ignore_ascii_case(code))
    }
}

impl fmt::Display for Language {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// Parses an ISO 639-1 code (e.g. `"en"`, `"es"`) — the same as
/// [`Language::from_iso639_1`], via the standard trait.
impl FromStr for Language {
    type Err = ParseLanguageError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::from_iso639_1(s).ok_or_else(|| ParseLanguageError(s.to_owned()))
    }
}

/// [`Language::from_str`] was given a code no Verbora subsystem has a
/// strategy for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseLanguageError(String);

impl ParseLanguageError {
    /// The code that failed to parse, exactly as given — not lowercased,
    /// not trimmed.
    #[must_use]
    pub fn code(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ParseLanguageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{:?} is not a language Verbora has a strategy for",
            self.0
        )
    }
}

impl std::error::Error for ParseLanguageError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_language_round_trips_through_its_own_code() {
        for lang in Language::ALL {
            assert_eq!(Language::from_iso639_1(lang.iso639_1()), Some(lang));
        }
    }

    #[test]
    fn parsing_is_case_insensitive() {
        assert_eq!(Language::from_iso639_1("EN"), Some(Language::English));
        assert_eq!(Language::from_iso639_1("Ja"), Some(Language::Japanese));
    }

    #[test]
    fn unknown_code_is_none() {
        assert_eq!(Language::from_iso639_1("xx"), None);
        assert_eq!(Language::from_iso639_1(""), None);
    }

    #[test]
    fn from_str_matches_from_iso639_1() {
        assert_eq!("en".parse::<Language>().unwrap(), Language::English);
        assert!("xx".parse::<Language>().is_err());
    }

    #[test]
    fn all_has_no_duplicate_codes() {
        let mut codes: Vec<&str> = Language::ALL.iter().map(|l| l.iso639_1()).collect();
        let before = codes.len();
        codes.sort_unstable();
        codes.dedup();
        assert_eq!(
            codes.len(),
            before,
            "duplicate ISO 639-1 code in Language::ALL"
        );
    }

    #[test]
    fn display_matches_name() {
        assert_eq!(Language::English.to_string(), "English");
    }

    #[test]
    fn all_lists_every_variant_exactly_once() {
        // `ALL` is what every enumeration test in this crate sweeps, so a
        // variant missing from it would silently exempt itself from all of
        // them. Names are unique per variant, so counting distinct names is
        // a proxy the compiler cannot drift from.
        let mut names: Vec<&str> = Language::ALL.iter().map(|l| l.name()).collect();
        let before = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), before, "duplicate entry in Language::ALL");
        assert_eq!(before, 22);
    }

    #[test]
    fn a_parse_error_reports_the_code_it_was_given() {
        let error = "XX".parse::<Language>().expect_err("XX is not a language");
        assert_eq!(error.code(), "XX", "the code is reported verbatim");
        assert!(error.to_string().contains("XX"));
    }

    #[test]
    fn parsing_never_panics_on_arbitrary_input() {
        for code in [
            "",
            " ",
            "e",
            "eng",
            "EN ",
            "😀",
            "\u{0}",
            &"e".repeat(10_000),
        ] {
            let _ = Language::from_iso639_1(code);
        }
    }
}
