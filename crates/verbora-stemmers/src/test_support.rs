//! Shared fixtures for the per-language differential test suites.
//!
//! Each Snowball module keeps its pre-`find_among` implementation as a test
//! oracle and replays three corpora through both: the module's own documented
//! vectors, a seeded random corpus, and the real benchmark word list — which
//! lives in one JSON file at the workspace root and is loaded once here.

use std::sync::LazyLock;

static WORDS: LazyLock<serde_json::Value> = LazyLock::new(|| {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../benches/data/stemmer-words.json"
    );
    let body = std::fs::read_to_string(path).expect("benches/data/stemmer-words.json");
    serde_json::from_str(&body).expect("valid bench-word JSON")
});

/// The benchmark word list for `lang`, exactly as `benches/stemmers.rs` sees it.
pub(crate) fn bench_words(lang: &str) -> Vec<String> {
    WORDS["languages"][lang]
        .as_array()
        .unwrap_or_else(|| panic!("no bench words for {lang}"))
        .iter()
        .map(|w| w.as_str().expect("bench words are strings").to_owned())
        .collect()
}
