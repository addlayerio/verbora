//! Shared support code for the §1.9-1.11 Language Detection / Script
//! Detection / Transliteration competitive benchmarks and accuracy report:
//! the dataset loader and the two cross-crate language-code mapping tables
//! (`lingua`'s own ISO-code accessor covers itself, but `whichlang` has no
//! such helper). Used by `benches/language.rs`,
//! `examples/language_accuracy.rs`, and
//! `tests/language_detection_correctness.rs` — written once here rather
//! than three times, so a mapping typo cannot silently diverge between the
//! benchmark and the accuracy report.

use std::path::Path;

use serde::Deserialize;

/// One language's four-tier test items, as read from
/// `datasets/language-accuracy/dataset.json`. See that directory's own
/// `README.md` for full sourcing, license, and extraction-methodology
/// documentation — this struct only describes the JSON shape.
#[derive(Debug, Clone, Deserialize)]
pub struct LanguageItems {
    pub short_word: String,
    pub short_phrase: String,
    pub sentence: String,
    pub paragraph: String,
}

/// The four tier names, in the dataset's own declared granularity order —
/// the exact categories `Fase 6 Benchmark.md`'s `LANGUAGE DETECTION
/// BENCHMARKS` section asks for ("short word / short phrase / sentence /
/// paragraph").
pub const TIERS: [&str; 4] = ["short_word", "short_phrase", "sentence", "paragraph"];

impl LanguageItems {
    /// Looks up one tier by name. Panics on an unknown tier — every caller
    /// in this workspace iterates [`TIERS`] itself rather than passing
    /// arbitrary strings, so this is a programmer-error guard, not a
    /// user-facing `Result`.
    #[must_use]
    pub fn get(&self, tier: &str) -> &str {
        match tier {
            "short_word" => &self.short_word,
            "short_phrase" => &self.short_phrase,
            "sentence" => &self.sentence,
            "paragraph" => &self.paragraph,
            other => panic!("unknown dataset tier {other:?}, expected one of {TIERS:?}"),
        }
    }
}

/// One language's dataset entry.
#[derive(Debug, Clone, Deserialize)]
pub struct LanguageEntry {
    /// The expected answer every detector is scored against.
    pub iso639_1: String,
    /// Matches `verbora_language::Language::name()`.
    pub verbora_language: String,
    /// The exact UDHR source file this entry was extracted from — see
    /// `datasets/README.md`.
    pub udhr_key: String,
    pub items: LanguageItems,
}

#[derive(Debug, Clone, Deserialize)]
struct DatasetFile {
    languages: Vec<LanguageEntry>,
}

/// Loads `datasets/language-accuracy/dataset.json` (repo-root-relative — see
/// `datasets/README.md` for what it contains and where it came from).
///
/// Deliberately no I/O-in-hot-loop concern here: every caller in this
/// workspace calls this once, outside any timed Criterion closure or
/// accuracy-scoring loop, per the spec's `NO I/O IN HOT LOOP` section.
#[must_use]
pub fn load_dataset() -> Vec<LanguageEntry> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(1)
        .expect("benchmarks/competitive/ is one level up from rust-competitors/")
        .join("datasets/language-accuracy/dataset.json");
    let body = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "cannot read {}: {e}\nSee benchmarks/competitive/datasets/README.md",
            path.display()
        )
    });
    let file: DatasetFile = serde_json::from_str(&body).expect("valid dataset JSON");
    assert_eq!(
        file.languages.len(),
        13,
        "dataset.json should have exactly the documented 13-language triple overlap"
    );
    file.languages
}

/// The 21-language overlap between `lingua` 1.8.0 and Verbora's
/// `Language::ALL` (all 22 but Galician, which lingua does not support) —
/// see `docs/COMPETITIVE_BENCHMARKS.md` §1.9's own note on why `lingua`
/// must be restricted to this set with `from_languages()`, never
/// benchmarked in its default 75-language configuration (the spec's own
/// "5 idiomas vs 75" fairness rule). `Bokmal` is the correct lingua variant
/// for Verbora's `Language::Norwegian` — lingua has no plain "Norwegian"
/// variant, only the Bokmal/Nynorsk written-standard split, and Bokmal is
/// what `whatlang`'s own `Lang::Nob` (see
/// `crates/verbora-language/src/whatlang_detector.rs`) is keyed to as well.
#[must_use]
pub fn lingua_restricted_languages() -> Vec<lingua::Language> {
    use lingua::Language::{
        Basque, Bokmal, Catalan, Chinese, Dutch, English, Finnish, French, German, Hindi,
        Indonesian, Italian, Japanese, Persian, Polish, Portuguese, Russian, Spanish, Swedish,
        Ukrainian, Vietnamese,
    };
    vec![
        English, Spanish, Portuguese, Italian, French, German, Dutch, Russian, Ukrainian, Polish,
        Persian, Hindi, Indonesian, Vietnamese, Japanese, Chinese, Bokmal, Swedish, Finnish,
        Catalan, Basque,
    ]
}

/// `lingua::Language` -> ISO 639-1, lowercase, for scoring against
/// `dataset.json`. `lingua` itself provides `Language::iso_code_639_1()`
/// (a `Display`-lowercased wrapper enum) — this just converts that to the
/// plain `&str`/`String` shape the accuracy scorer wants, rather than a
/// hand-written match, so it cannot silently drift from lingua's own
/// authoritative mapping. Note this is genuinely a different code for
/// `Bokmal` (`"nb"`) than Verbora's own `Language::Norwegian.iso639_1()`
/// (`"no"`) — irrelevant to this dataset (Norwegian is not one of the
/// 13-language triple overlap `dataset.json` uses), but documented here
/// since it is a real, easy-to-miss subtlety if this harness is ever
/// extended to include it.
#[must_use]
pub fn lingua_iso(lang: lingua::Language) -> String {
    lang.iso_code_639_1().to_string()
}

/// `whichlang::Lang` -> ISO 639-1, for scoring against `dataset.json`.
/// `whichlang` 0.1.1 has no ISO-code accessor at all (only
/// `three_letter_code()`, which is not even standard ISO 639-3 for every
/// variant — e.g. `Cmn` -> `"cmn"` is fine, but there is no 639-1 form
/// exposed anywhere in the crate), so this is a hand-written, exhaustive
/// match over all 16 of `whichlang` 0.1.1's variants — not just the 13 in
/// this dataset — so it stays correct if the crate or the dataset ever
/// grows. Verified exhaustive by `#[deny(unreachable_patterns)]`-equivalent
/// compiler exhaustiveness checking on `match` with no wildcard arm.
#[must_use]
pub const fn whichlang_iso(lang: whichlang::Lang) -> &'static str {
    use whichlang::Lang::{
        Ara, Cmn, Deu, Eng, Fra, Hin, Ita, Jpn, Kor, Nld, Por, Rus, Spa, Swe, Tur, Vie,
    };
    match lang {
        Ara => "ar",
        Cmn => "zh",
        Deu => "de",
        Eng => "en",
        Fra => "fr",
        Hin => "hi",
        Ita => "it",
        Jpn => "ja",
        Kor => "ko",
        Nld => "nl",
        Por => "pt",
        Rus => "ru",
        Spa => "es",
        Swe => "sv",
        Tur => "tr",
        Vie => "vi",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dataset_loads_and_has_all_four_tiers_per_language() {
        let languages = load_dataset();
        assert_eq!(languages.len(), 13);
        for entry in &languages {
            assert!(!entry.items.short_word.is_empty(), "{}", entry.iso639_1);
            assert!(!entry.items.short_phrase.is_empty(), "{}", entry.iso639_1);
            assert!(!entry.items.sentence.is_empty(), "{}", entry.iso639_1);
            assert!(!entry.items.paragraph.is_empty(), "{}", entry.iso639_1);
            // The dataset's own documented ordering: each tier should be no
            // shorter (in chars) than the previous one, confirming the
            // extraction rules in datasets/README.md actually produced
            // increasing granularity rather than, say, a copy-paste bug
            // that left two tiers identical-length by accident.
            assert!(
                entry.items.short_word.chars().count() <= entry.items.short_phrase.chars().count()
            );
            assert!(
                entry.items.short_phrase.chars().count() <= entry.items.sentence.chars().count()
            );
            assert!(entry.items.sentence.chars().count() <= entry.items.paragraph.chars().count());
        }
    }

    #[test]
    fn lingua_restricted_set_has_21_languages() {
        // 22 Verbora languages minus Galician, which lingua 1.8.0 does not
        // support at all (not even a `#[cfg]`-gated variant to select) --
        // see this function's own doc comment.
        let langs = lingua_restricted_languages();
        assert_eq!(langs.len(), 21);
        assert!(langs.contains(&lingua::Language::Chinese));
        assert!(langs.contains(&lingua::Language::Bokmal));
    }

    #[test]
    fn lingua_iso_matches_dataset_codes_for_every_triple_overlap_language() {
        use lingua::Language::{
            Chinese, Dutch, English, French, German, Hindi, Italian, Japanese, Portuguese, Russian,
            Spanish, Swedish, Vietnamese,
        };
        let expected = [
            (English, "en"),
            (Spanish, "es"),
            (French, "fr"),
            (German, "de"),
            (Italian, "it"),
            (Portuguese, "pt"),
            (Dutch, "nl"),
            (Russian, "ru"),
            (Hindi, "hi"),
            (Vietnamese, "vi"),
            (Japanese, "ja"),
            (Chinese, "zh"),
            (Swedish, "sv"),
        ];
        for (lang, iso) in expected {
            assert_eq!(lingua_iso(lang), iso, "{lang:?}");
        }
    }

    #[test]
    fn whichlang_iso_matches_dataset_codes_for_every_triple_overlap_language() {
        use whichlang::Lang::{Cmn, Deu, Eng, Fra, Hin, Ita, Jpn, Nld, Por, Rus, Spa, Swe, Vie};
        let expected = [
            (Eng, "en"),
            (Spa, "es"),
            (Fra, "fr"),
            (Deu, "de"),
            (Ita, "it"),
            (Por, "pt"),
            (Nld, "nl"),
            (Rus, "ru"),
            (Hin, "hi"),
            (Vie, "vi"),
            (Jpn, "ja"),
            (Cmn, "zh"),
            (Swe, "sv"),
        ];
        for (lang, iso) in expected {
            assert_eq!(whichlang_iso(lang), iso, "{lang:?}");
        }
    }

    #[test]
    fn every_dataset_language_is_in_the_lingua_restricted_set() {
        // The whole point of the restricted-language builder: it must
        // actually be *able* to answer every language dataset.json expects,
        // or the accuracy numbers would be measuring an artificial
        // exclusion rather than lingua's real algorithm.
        let restricted = lingua_restricted_languages();
        for entry in load_dataset() {
            let found = restricted.iter().any(|l| lingua_iso(*l) == entry.iso639_1);
            assert!(found, "lingua restricted set is missing {}", entry.iso639_1);
        }
    }
}
