//! `CORRECTNESS BEFORE PERFORMANCE` for `benches/ngrams.rs`.
//!
//! Verifies, once and outside the timed code, that Verbora's character
//! n-grams and `ngrammatic`'s agree exactly — same set of grams, same
//! counts — over every word in the shared word list, for both arity 2 and
//! arity 3. This is the evidence backing `benches/ngrams.rs`'s claim that
//! the two are compared on genuinely equivalent output, not just a
//! similarly-named operation.
//!
//! Both sides pad with `arity - 1` copies of a space on each side — Verbora's
//! `Padded::new(&chars, arity, Some(&' '), Some(&' '))`, `ngrammatic`'s
//! default `Pad::Auto` — and slide a window of size `arity` across the padded
//! sequence. The two definitions are now literally the same one: `ngrammatic`
//! builds `" ".repeat(arity - 1) + text + " ".repeat(arity - 1)` and calls
//! `chars_padded.windows(arity)` (`ngrammatic-0.7.0/src/ngram.rs:205-217`),
//! which is `docs/design/text-shaping-contract.md` §3.3's padding rule
//! spelled out.
//!
//! The divergence this file used to disclose — short inputs, where Verbora's
//! right padding re-anchored a negative slice start and produced tuples
//! shorter than `arity` — is gone with that implementation. Under the current
//! contract every window is exactly `arity` elements for every input length,
//! including `len == 0`, so short inputs are now checked for *agreement*
//! rather than pinned as a difference.

use std::collections::HashMap;
use std::num::NonZeroUsize;

use ngrammatic::{NgramBuilder, Pad};
use verbora_ngrams::Padded;

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
    let arity = NonZeroUsize::new(arity).expect("arity is non-zero");
    let chars: Vec<char> = word.chars().collect();
    let padded = Padded::new(&chars, arity, Some(&' '), Some(&' '));
    let mut map: HashMap<String, usize> = HashMap::new();
    for gram in padded.ngrams() {
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

/// Inputs shorter than `arity - 1`, which the previous implementation got
/// wrong on both counts — short tuples and out-of-order emission — and which
/// `benches/ngrams.rs` never reaches, since the shortest word in the shared
/// list is three characters. Checked here so the benchmark's equivalence
/// claim does not rest on the input happening to avoid the hard case.
#[test]
fn agrees_on_inputs_shorter_than_arity() {
    for arity in 2..=4usize {
        for word in ["", "a", "ab"] {
            let verbora = verbora_char_grams(word, arity);
            let ngrammatic = ngrammatic_char_grams(word, arity);
            assert_eq!(
                verbora, ngrammatic,
                "mismatch for {word:?} at arity {arity}: \
                 verbora={verbora:?} ngrammatic={ngrammatic:?}"
            );
        }
    }
}

/// Verbora's own answer for a short input, derived from
/// `docs/design/text-shaping-contract.md` §3.3 rather than from `ngrammatic`:
/// `"a"` at arity 3 pads to `[' ', ' ', 'a', ' ', ' ']`, which holds
/// `5 - 3 + 1 = 3` windows, each of exactly three characters.
#[test]
fn short_inputs_are_padded_symmetrically() {
    let grams = verbora_char_grams("a", 3);
    assert_eq!(grams.len(), 3);
    assert_eq!(grams.get("  a"), Some(&1));
    assert_eq!(grams.get(" a "), Some(&1));
    assert_eq!(grams.get("a  "), Some(&1));
}
