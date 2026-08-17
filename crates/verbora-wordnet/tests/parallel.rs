//! Sequential-vs-parallel parity for [`WordNet::par_lookup_batch`] (`parallel`
//! feature only).
//!
//! `par_lookup_batch` is architecturally required to be
//! `words.par_iter().map(Self::lookup).collect()` — a thin fan-out over the
//! existing sequential primitive, nothing more. This suite proves that by
//! comparing its output, item for item, against the sequential
//! `.iter().map(WordNet::lookup).collect()` it must be indistinguishable from.
//!
//! Inputs reuse edge cases this crate's own suites already know to exercise,
//! rather than inventing new ones: the hand-built dictionary's hit/miss/empty
//! words from `src/lib.rs`'s `tiny_dict` tests (mirrored here — that helper is
//! private to that module), and the Unicode normalisation cases from
//! `src/whitespace.rs`'s `normalize_lookup_word` tests (Turkish dotted İ, Greek
//! final sigma, Cyrillic, CJK, emoji). When the real dictionary is installed
//! (separately licensed, not vendored — see `tests/parity.rs`), this also
//! replays the exact `WORDS` list `benches/wordnet.rs` uses, including `"run"`
//! (57 senses, the worst case in the database) and `"awful"` (a false miss
//! from the reference's own incomplete bisection).

#![cfg(feature = "parallel")]

use std::path::PathBuf;

use verbora_wordnet::WordNet;

/// A hand-built dictionary, byte-for-byte the same one `src/lib.rs`'s private
/// `tiny_dict` test helper builds — not the WordNet data, just its format.
fn tiny_dict() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "verbora-wordnet-parallel-test-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let index =
        "aaa n 1 0 1 0 00000000  \nbbb n 2 1 @ 2 0 00000000 00000083  \nccc n 1 0 1 0 00000083  \n";
    let data = "00000000 06 n 01 alpha 0 001 @ 00000083 n 0000 | the first letter; \"as in alpha\"  \n00000083 06 n 02 beta 0 second 1 000 | the second letter  \n";
    for pos in ["noun", "verb", "adj", "adv"] {
        std::fs::write(dir.join(format!("index.{pos}")), index).unwrap();
        std::fs::write(dir.join(format!("data.{pos}")), data).unwrap();
    }
    dir
}

/// The real WordNet dictionary, when installed; `None` skips the section that
/// needs it — it is separately licensed (Princeton University) and
/// deliberately not vendored, exactly the pattern `benches/wordnet.rs` uses.
fn real_dict_dir() -> Option<PathBuf> {
    for var in ["WORDNET_DB_PATH", "VERBORA_WORDNET_DICT"] {
        if let Some(v) = std::env::var_os(var) {
            let p = PathBuf::from(v);
            if p.join("index.noun").is_file() {
                return Some(p);
            }
        }
    }
    None
}

/// Asserts [`WordNet::par_lookup_batch`] agrees with the sequential
/// `.iter().map(WordNet::lookup)` loop, item for item and in order.
fn assert_parity(wn: &WordNet, words: &[&str]) {
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
    assert_parity(&wn, &[]);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_single_item_matches_the_sequential_call() {
    let dir = tiny_dict();
    let wn = WordNet::open(&dir).unwrap();
    assert_parity(&wn, &["ccc"]);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn many_items_preserve_order_and_match_the_sequential_loop() {
    let dir = tiny_dict();
    let wn = WordNet::open(&dir).unwrap();
    // Hits, the "zzz" miss and the "" edge case
    // `a_prebuilt_sidecar_reproduces_the_scanned_index` (src/lib.rs) already
    // exercises, repeated to a batch large enough to span several rayon tasks.
    let base = ["aaa", "bbb", "ccc", "zzz", ""];
    let words: Vec<&str> = base.iter().copied().cycle().take(500).collect();
    assert_parity(&wn, &words);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn unicode_normalisation_matches_the_sequential_call() {
    let dir = tiny_dict();
    let wn = WordNet::open(&dir).unwrap();
    // The exact cases `whitespace::normalize_lookup_word`'s own unit test asserts
    // by value (src/whitespace.rs): Turkish dotted İ, Greek final sigma, Cyrillic,
    // CJK, emoji, plus the whitespace-collapsing cases that change which key
    // gets probed against the index.
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
    ];
    assert_parity(&wn, &words);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn agrees_with_the_sequential_loop_on_the_real_dictionary_worst_case() {
    let Some(dict) = real_dict_dir() else {
        eprintln!(
            "skipped: no WordNet dictionary found (separately licensed, not vendored); \
             see tests/parity.rs for how to install it"
        );
        return;
    };
    let wn = WordNet::open(&dict).unwrap();
    // The exact word list `benches/wordnet.rs` uses: it already spans a
    // common hit, the worst case (`run`, 57 senses across four parts of
    // speech), a false miss from the reference's incomplete bisection
    // (`awful`), a genuine miss (`zzzzz`), and a multi-word lookup
    // (`new_york`).
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
    assert_parity(&wn, WORDS);
}
