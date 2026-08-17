//! `CORRECTNESS BEFORE PERFORMANCE` for `benches/ngrams.rs`.
//!
//! Verifies, once and outside the timed code, that Verbora's character
//! n-grams and `ngrammatic`'s agree exactly — same set of grams, same
//! counts — over every word in the shared word list, for both arity 2 and
//! arity 3. This is the evidence backing `benches/ngrams.rs`'s claim that
//! the two are compared on genuinely equivalent output, not just a
//! similarly-named operation.
//!
//! Both sides pad with `arity - 1` space characters on each side (Verbora's
//! `ngrams(&chars, arity, Some(' '), Some(' '))`, `ngrammatic`'s default
//! `Pad::Auto`) and slide a window of size `arity` across the padded
//! sequence — the same algorithm, independently arrived at. They are
//! expected to diverge only on inputs shorter than `arity - 1`, where
//! Verbora's padding follows the reference's own negative-slice
//! re-anchoring quirk (see `crates/verbora-ngrams/src/engine.rs`'s
//! `clamp_slice_start` doc comment) and `ngrammatic` does not — no word in
//! `benches/data/words.json` is that short (the generator's own minimum is
//! 3 characters), so that divergence is disclosed here rather than
//! silently avoided by the input, and is not exercised by this test.

use std::collections::HashMap;

use ngrammatic::{NgramBuilder, Pad};
use verbora_ngrams::ngrams;

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

/// Character n-grams of `word`, padded and counted the way `benches/ngrams.rs`
/// times it — see that file for why this exact shape (a `HashMap<String,
/// usize>` built by folding [`ngrams`]'s output) is the fair comparison
/// point against `ngrammatic::Ngram::grams`.
fn verbora_char_grams(word: &str, arity: usize) -> HashMap<String, usize> {
    let chars: Vec<char> = word.chars().collect();
    let mut map: HashMap<String, usize> = HashMap::new();
    for gram in ngrams(&chars, arity, Some(' '), Some(' ')) {
        *map.entry(gram.iter().collect()).or_insert(0) += 1;
    }
    map
}

fn ngrammatic_char_grams(word: &str, arity: usize) -> HashMap<String, usize> {
    NgramBuilder::new(word)
        .arity(arity)
        .pad_full(Pad::Auto)
        .finish()
        .grams
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect()
}

#[test]
fn agrees_on_every_word_arity_2() {
    let words = load_words();
    assert!(!words.is_empty(), "word list must not be empty");
    for word in &words {
        let verbora = verbora_char_grams(word, 2);
        let ngrammatic = ngrammatic_char_grams(word, 2);
        assert_eq!(
            verbora, ngrammatic,
            "bigram mismatch for {word:?}: verbora={verbora:?} ngrammatic={ngrammatic:?}"
        );
    }
}

#[test]
fn agrees_on_every_word_arity_3() {
    let words = load_words();
    assert!(!words.is_empty(), "word list must not be empty");
    for word in &words {
        let verbora = verbora_char_grams(word, 3);
        let ngrammatic = ngrammatic_char_grams(word, 3);
        assert_eq!(
            verbora, ngrammatic,
            "trigram mismatch for {word:?}: verbora={verbora:?} ngrammatic={ngrammatic:?}"
        );
    }
}

/// The one documented divergence: inputs shorter than `arity - 1` hit
/// Verbora's negative-slice re-anchoring quirk (ported from the reference),
/// which `ngrammatic` has no equivalent of. Recorded as a real, disclosed
/// difference — not exercised by `benches/ngrams.rs`, whose input (real
/// words, minimum length 3) never reaches it.
#[test]
fn diverges_on_inputs_shorter_than_arity_minus_one() {
    // "a" has length 1, arity 3 needs 2 pad positions per side: Verbora's
    // right-padding re-anchors past the start of the sequence.
    let verbora = verbora_char_grams("a", 3);
    let ngrammatic = ngrammatic_char_grams("a", 3);
    assert_ne!(
        verbora, ngrammatic,
        "expected a real divergence on a short input, per this file's module doc comment"
    );
}
