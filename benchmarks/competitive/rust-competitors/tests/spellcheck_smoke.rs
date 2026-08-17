//! Sanity check for `benches/spellcheck.rs`.
//!
//! `symspell`, `harper-core`, `fast_symspell` and `spellbook` are all four
//! matrix `Partial` rows — genuinely different algorithms from Verbora's
//! Norvig port, so `CORRECTNESS BEFORE PERFORMANCE`'s "same algorithmic
//! answer" clause does not apply here (there is no shared answer to check
//! equivalence against). What this file does check, once and outside the
//! timed code, is that every implementation is actually *doing its job* on
//! the corpus it was built from: a word literally present in the corpus is
//! reported correct, and a word literally absent is not — the minimum bar
//! for trusting that a benchmark measures live, working calls rather than a
//! degenerate always-true/always-false no-op. `fast_symspell` gets three
//! more, dedicated tests of its own in
//! `tests/spellcheck_fast_symspell_correctness.rs` — see that file's own
//! doc comment for the `count_threshold` trap and distance-semantics
//! divergence this file's single membership check does not cover.

use fast_symspell::{
    AsciiStringStrategy as FastAsciiStringStrategy, SymSpell as FastSymSpell,
    SymSpellBuilder as FastSymSpellBuilder, Verbosity as FastVerbosity,
};
use harper_core::spell::{Dictionary as HarperDictionary, FstDictionary};
use harper_core::{CharString, DictWordMetadata};
use spellbook::Dictionary as SpellbookDictionary;
use symspell::{AsciiStringStrategy, SymSpell, SymSpellBuilder, Verbosity};
use verbora_spellcheck::Spellcheck;

fn load_words() -> Vec<String> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("repo root is 3 levels up from rust-competitors/")
        .join("benches/data/words.json");
    let body = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "cannot read {}: {e}\nGenerate it with: python3 tools/bench-data/generate.py",
            path.display()
        )
    });
    let json: serde_json::Value = serde_json::from_str(&body).expect("valid bench data");
    json["words"]
        .as_array()
        .expect("word list")
        .iter()
        .map(|w| w.as_str().expect("word").to_owned())
        .collect()
}

#[test]
fn verbora_symspell_and_harper_agree_the_corpus_is_the_corpus() {
    let words = load_words();
    let sample = &words[..2_000];

    let sc = Spellcheck::new(sample);

    let mut symspell: SymSpell<AsciiStringStrategy> = SymSpellBuilder::default()
        .max_dictionary_edit_distance(2)
        .build()
        .unwrap();
    for w in sample {
        symspell.load_dictionary_line(&format!("{w} 1"), 0, 1, " ");
    }

    let entries: Vec<_> = sample
        .iter()
        .map(|w| {
            (
                w.chars().collect::<CharString>(),
                DictWordMetadata::default(),
            )
        })
        .collect();
    let harper = FstDictionary::new(entries);

    // `count_threshold(0)`, not the untouched default -- see
    // `benches/spellcheck.rs`'s own `build_fast_symspell` doc comment and
    // `tests/spellcheck_fast_symspell_correctness.rs`'s
    // `demonstrates_default_count_threshold_would_empty_this_corpus` for why
    // `fast_symspell`'s default (100) would silently empty this corpus
    // (frequencies here top out at 1, every word loaded with count "1").
    let mut fast_symspell: FastSymSpell<FastAsciiStringStrategy> = FastSymSpellBuilder::default()
        .max_dictionary_edit_distance(2)
        .count_threshold(0)
        .build()
        .unwrap();
    for w in sample {
        fast_symspell.load_dictionary_line(&format!("{w} 1"), 0, 1, " ");
    }

    // A word unmistakably absent from a corpus of short lowercase ASCII
    // strings: uppercase and long enough that no `word()` draw could match
    // it (see tools/bench-data/generate.py's own generator, capped at 14
    // characters, lowercase a-z only).
    const DEFINITELY_ABSENT: &str = "ZZZZZZZZZZZZZZZZZZ";

    for word in sample.iter().take(50) {
        assert!(
            sc.is_correct(word),
            "verbora should accept corpus word {word:?}"
        );
        assert!(
            !symspell.lookup(word, Verbosity::Top, 0).is_empty(),
            "symspell should accept corpus word {word:?}"
        );
        assert!(
            harper.contains_word_str(word),
            "harper-core should accept corpus word {word:?}"
        );
        assert!(
            !fast_symspell.lookup(word, FastVerbosity::Top, 0).is_empty(),
            "fast_symspell should accept corpus word {word:?}"
        );
    }

    assert!(!sc.is_correct(DEFINITELY_ABSENT));
    assert!(
        symspell
            .lookup(DEFINITELY_ABSENT, Verbosity::Top, 0)
            .is_empty()
    );
    assert!(!harper.contains_word_str(DEFINITELY_ABSENT));
    assert!(
        fast_symspell
            .lookup(DEFINITELY_ABSENT, FastVerbosity::Top, 0)
            .is_empty()
    );
}

#[test]
fn spellbook_recognises_real_english_on_its_own_dictionary() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(1)
        .expect("rust-competitors is one level below benchmarks/competitive/")
        .join("models/hunspell-en_US");
    if !dir.join("en_US.aff").is_file() {
        eprintln!(
            "spellbook smoke test skipped — en_US dictionary not found.\n\
             Fetch it with: benchmarks/competitive/scripts/fetch-models.sh hunspell-en-us"
        );
        return;
    }
    let aff = std::fs::read_to_string(dir.join("en_US.aff")).unwrap();
    let dic = std::fs::read_to_string(dir.join("en_US.dic")).unwrap();
    let dict = SpellbookDictionary::new(&aff, &dic).unwrap();

    assert!(dict.check("hello"), "spellbook should accept 'hello'");
    assert!(dict.check("world"), "spellbook should accept 'world'");
    assert!(
        !dict.check("zxqjkvvvv"),
        "spellbook should reject an unpronounceable non-word"
    );

    let mut suggestions = Vec::new();
    dict.suggest("helo", &mut suggestions);
    assert!(
        suggestions.iter().any(|s| s == "hello"),
        "spellbook should suggest 'hello' for 'helo', got {suggestions:?}"
    );
}
