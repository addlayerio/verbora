//! Sequential-vs-parallel equivalence for [`WordNet::par_lookup_batch`]
//! (`parallel` feature only).
//!
//! `par_lookup_batch` is architecturally required to be
//! `words.par_iter().map(WordNet::lookup).collect()` — a fan-out over the
//! sequential primitive, nothing more. This suite proves that by comparing its
//! output, item for item and in order, against the sequential
//! `.iter().map(WordNet::lookup).collect()` it must be indistinguishable from.
//!
//! Inputs reuse edge cases the crate's other suites already exercise rather
//! than inventing new ones: the hand-built dictionary's hit, miss and empty
//! words, and the normalisation cases `index_key`'s own tests assert by value
//! (Turkish dotted İ, Greek final sigma, Cyrillic, CJK, emoji, whitespace
//! collapsing). When the real dictionary is installed — separately licensed and
//! deliberately not vendored — it also replays the word list `benches/wordnet.rs`
//! uses.

#![cfg(feature = "parallel")]

use std::path::PathBuf;

use verbora_wordnet::{PartOfSpeech, WordNet};

/// A hand-built dictionary in the WordNet text format: three lemmas, two
/// synsets, and the two-space copyright header the format requires. The byte
/// offsets quoted in the index lines are the real positions of the records.
fn tiny_dict() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "verbora-wordnet-parallel-test-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&dir).unwrap();

    let header = "  1 Not WordNet data: the file format only\n";
    assert_eq!(header.len(), 43);
    let alpha =
        "00000043 06 n 01 alpha 0 001 @ 00000126 n 0000 | the first letter; \"as in alpha\"  \n";
    assert_eq!(header.len() + alpha.len(), 126);
    let beta = "00000126 06 n 02 beta 0 second 1 000 | the second letter  \n";

    let index = format!(
        "{header}\
         aaa n 1 0 1 0 00000043  \n\
         bbb n 2 1 @ 2 0 00000043 00000126  \n\
         ccc n 1 0 1 0 00000126  \n"
    );
    let data = format!("{header}{alpha}{beta}");

    for pos in PartOfSpeech::ALL {
        let suffix = pos.file_suffix();
        let tag = pos.tag();
        std::fs::write(
            dir.join(format!("index.{suffix}")),
            index.replace(" n ", &format!(" {tag} ")),
        )
        .unwrap();
        std::fs::write(
            dir.join(format!("data.{suffix}")),
            data.replace(" 06 n ", &format!(" 06 {tag} ")),
        )
        .unwrap();
    }
    dir
}

/// The real WordNet dictionary, when installed; `None` skips the section that
/// needs it.
fn real_dict_dir() -> Option<PathBuf> {
    for var in ["VERBORA_WORDNET_DICT", "WORDNET_DB_PATH"] {
        if let Some(value) = std::env::var_os(var) {
            let dir = PathBuf::from(value);
            if dir.join("index.noun").is_file() {
                return Some(dir);
            }
        }
    }
    None
}

/// Asserts [`WordNet::par_lookup_batch`] agrees with the sequential
/// `.iter().map(WordNet::lookup)` loop, item for item and in order.
fn assert_equivalent(wn: &WordNet, words: &[&str]) {
    let sequential: Vec<_> = words.iter().map(|w| wn.lookup(w)).collect();
    let parallel = wn.par_lookup_batch(words);
    assert_eq!(
        parallel.len(),
        sequential.len(),
        "batch of {} words produced {} results",
        words.len(),
        parallel.len()
    );
    for (i, (p, s)) in parallel.iter().zip(sequential.iter()).enumerate() {
        match (p, s) {
            (Ok(pv), Ok(sv)) => assert_eq!(pv, sv, "word {i} ({:?}) diverged", words[i]),
            (Err(pe), Err(se)) => assert_eq!(
                pe.to_string(),
                se.to_string(),
                "word {i} ({:?}) errored differently",
                words[i]
            ),
            _ => panic!(
                "word {i} ({:?}): parallel={p:?} but sequential={s:?}",
                words[i]
            ),
        }
    }
}

#[test]
fn empty_input_produces_an_empty_output() {
    let dir = tiny_dict();
    let wn = WordNet::open(&dir).unwrap();
    assert_equivalent(&wn, &[]);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_single_item_matches_the_sequential_call() {
    let dir = tiny_dict();
    let wn = WordNet::open(&dir).unwrap();
    assert_equivalent(&wn, &["ccc"]);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn many_items_preserve_order_and_match_the_sequential_loop() {
    let dir = tiny_dict();
    let wn = WordNet::open(&dir).unwrap();
    let base = ["aaa", "bbb", "ccc", "zzz", ""];
    let words: Vec<&str> = base.iter().copied().cycle().take(500).collect();
    assert_equivalent(&wn, &words);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn normalisation_matches_the_sequential_call() {
    let dir = tiny_dict();
    let wn = WordNet::open(&dir).unwrap();
    // The exact cases `index_key`'s own unit tests assert by value, plus the
    // whitespace-collapsing ones that change which key gets searched for.
    let words = [
        "New York",
        "new  york",
        "  entity  ",
        "NEW\tYORK",
        "\t\n\r",
        "CAFÉ",
        "МОСКВА",
        "日本語",
        "😀",
        "İSTANBUL",
        "ΟΔΟΣ",
        "AAA",
        "Ccc",
        "  bbb  ",
    ];
    assert_equivalent(&wn, &words);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn agrees_with_the_sequential_loop_on_the_real_dictionary() {
    let Some(dict) = real_dict_dir() else {
        eprintln!(
            "skipped: no WordNet dictionary found. It is separately licensed \
             (Princeton University) and not vendored; point $WORDNET_DB_PATH at \
             a directory holding index.noun and its seven siblings."
        );
        return;
    };
    let wn = WordNet::open(&dict).unwrap();
    // The word list `benches/wordnet.rs` uses: a common hit, the worst case
    // (`run`, senses across all four categories), a genuine miss, and a
    // collocation.
    const WORDS: &[&str] = &[
        "entity",
        "node",
        "fast",
        "run",
        "dog",
        "cat",
        "good",
        "light",
        "water",
        "computer",
        "awful",
        "abandon",
        "quickly",
        "beautiful",
        "new_york",
        "zzzzz",
    ];
    assert_equivalent(&wn, WORDS);
}
