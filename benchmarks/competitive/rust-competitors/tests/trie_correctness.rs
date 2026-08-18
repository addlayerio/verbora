//! `CORRECTNESS BEFORE PERFORMANCE` for `benches/trie.rs`.
//!
//! Verifies, once and outside the timed code, that Verbora, `trie-rs`,
//! `qp-trie`, `fast_radix_trie` and `fst` — built from the exact same word
//! list — answer the exact same yes/no questions
//! (`contains`/`exact_match`/`contains_key`/`Set::contains`) and enumerate
//! the exact same *sets* of words for prefix search and common-prefix
//! search. This is the evidence backing `benches/trie.rs`'s own `fst`
//! section's NARROWED_EXACT classification for `contains`/`predictive_search`
//! — a classification is not asserted anywhere in this workbench without a
//! test like this one behind it.
//!
//! **This file never compares order.** `trie-rs` and `fast_radix_trie` both
//! return byte-lexicographic order (independently, from unrelated
//! implementations — see `benches/trie.rs`'s module doc comment), `qp-trie`
//! its own internal nybble-trie order, `fst` is byte-lexicographic by
//! construction but iterates via `Streamer`, not `Iterator`, Verbora
//! The reference `for…in` order — genuinely different orders/APIs per
//! implementation, per `docs/COMPETITIVE_BENCHMARKS.md` §1.18. Every
//! comparison below goes through a `BTreeSet<String>`, which is exactly the
//! "same answer, order irrelevant" claim `benches/trie.rs`'s module doc
//! comment makes and no stronger one.
//!
//! `fst` is built via its own `build_fst` helper (sort + dedup, mirroring
//! `benches/trie.rs::build_fst`) rather than threaded through `build_all`'s
//! shared tuple, so this file's existing four-structure comparisons stay
//! untouched and `fst`'s real sorted-input precondition stays visible at
//! its own call sites instead of being absorbed into a generic builder.
//!
//! Beyond the original word-list checks, this file also pins the claims the
//! shape-variant benchmark rows added to `benches/trie.rs` depend on: the
//! derived `build` datasets ("prefix_heavy", "sorted", "short", "long")
//! yield the same key set in every structure, first-character misses
//! (`QQ{w}`) and truncated-word probes agree everywhere, multi-character and
//! full-word predictive prefixes agree as sets, and the "extended"
//! (`{w}ed`) common-prefix probes agree across the three structures that
//! natively answer that question. All of it stays inside the same
//! lowercase-ASCII domain as the rest of this file — the case-folding quirk
//! of `Trie::keys_with_prefix` (documented in `verbora_trie`'s own crate
//! docs, and part of the exact-semantics row the matrix marks as having no
//! fair competitor) is never exercised, deliberately.

use std::collections::BTreeSet;

use fast_radix_trie::StringRadixMap;
use fst::automaton::Str;
use fst::{Automaton, IntoStreamer, Set, Streamer};
use qp_trie::Trie as QpTrie;
use qp_trie::wrapper::BString;
use trie_rs::{Trie as TrieRs, TrieBuilder};
use verbora_trie::{FrozenTrie, Trie};

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

fn build_all(words: &[String]) -> (Trie, TrieRs<u8>, QpTrie<BString, ()>, StringRadixMap<()>) {
    let mut verbora = Trie::new();
    verbora.add_strings(words.iter().map(String::as_str));

    let mut b = TrieBuilder::new();
    for w in words {
        b.push(w.as_str());
    }
    let trie_rs = b.build();

    let mut qp = QpTrie::new();
    for w in words {
        qp.insert_str(w.as_str(), ());
    }

    // `StringRadixMap<()>`, not `StringRadixSet` — see `benches/trie.rs`'s
    // module doc comment for why the set wrapper cannot answer
    // `common_prefixes`, which this file also verifies below.
    let mut fast_radix = StringRadixMap::new();
    for w in words {
        fast_radix.insert(w.as_str(), ());
    }

    (verbora, trie_rs, qp, fast_radix)
}

/// `Trie::freeze`'s path-compressed representation, built from the exact
/// same word list — kept as its own function here (rather than added to
/// `build_all`'s tuple) for the same reason `build_fst` is: it depends on
/// `verbora` already being built, not an independent structure of its own.
fn build_verbora_frozen(verbora: &Trie) -> FrozenTrie {
    verbora.freeze()
}

/// Sorts and deduplicates `words`, then streams the result into an
/// `fst::Set` — identical construction to `benches/trie.rs::build_fst`, kept
/// as its own function here (rather than added to `build_all`'s tuple) for
/// the reason given in the module doc comment.
fn build_fst(words: &[String]) -> Set<Vec<u8>> {
    let mut sorted: Vec<&str> = words.iter().map(String::as_str).collect();
    sorted.sort_unstable();
    sorted.dedup();
    Set::from_iter(sorted).expect("sorted, deduplicated input builds an fst::Set")
}

/// Every generated word is
/// `contains`/`exact_match`/`contains_key`/`contains_key`-true in all four
/// structures — the literal claim the `build`/`contains_hit` benchmarks
/// depend on ("built the same set of keys").
#[test]
fn contains_agrees_on_hits() {
    let words = load_words();
    let (verbora, trie_rs, qp, fast_radix) = build_all(&words);
    let verbora_frozen = build_verbora_frozen(&verbora);
    let fst_set = build_fst(&words);

    // Dedup: the shared corpus has a small number of duplicate words (LCG
    // collisions), which is irrelevant to set membership but would make a
    // naive per-word assertion loop redundant, not wrong.
    let unique: BTreeSet<&str> = words.iter().map(String::as_str).collect();

    for w in &unique {
        assert!(verbora.contains(w), "verbora missing hit {w:?}");
        assert!(
            verbora_frozen.contains(w),
            "verbora_frozen missing hit {w:?}"
        );
        assert!(trie_rs.exact_match(*w), "trie-rs missing hit {w:?}");
        assert!(qp.contains_key_str(*w), "qp-trie missing hit {w:?}");
        assert!(
            fast_radix.contains_key(*w),
            "fast_radix_trie missing hit {w:?}"
        );
        assert!(fst_set.contains(*w), "fst missing hit {w:?}");
    }
}

/// Every word with a guaranteed-absent uppercase suffix is `contains`-false
/// in all four — the literal claim the `contains_miss` benchmark depends
/// on. Uses the same `{word}QQ` convention as
/// `crates/verbora-trie/benches/trie.rs::misses`.
#[test]
fn contains_agrees_on_misses() {
    let words = load_words();
    let (verbora, trie_rs, qp, fast_radix) = build_all(&words);
    let verbora_frozen = build_verbora_frozen(&verbora);
    let fst_set = build_fst(&words);

    for w in words.iter().take(2000) {
        let miss = format!("{w}QQ");
        assert!(!verbora.contains(&miss), "verbora false hit on {miss:?}");
        assert!(
            !verbora_frozen.contains(&miss),
            "verbora_frozen false hit on {miss:?}"
        );
        assert!(
            !trie_rs.exact_match(miss.as_str()),
            "trie-rs false hit on {miss:?}"
        );
        assert!(
            !qp.contains_key_str(miss.as_str()),
            "qp-trie false hit on {miss:?}"
        );
        assert!(
            !fast_radix.contains_key(miss.as_str()),
            "fast_radix_trie false hit on {miss:?}"
        );
        assert!(
            !fst_set.contains(miss.as_str()),
            "fst false hit on {miss:?}"
        );
    }
}

/// `Trie::keys_with_prefix("")` / `trie_rs::predictive_search("")` /
/// `qp_trie::iter_prefix_str("")` / `fast_radix_trie::iter_prefix("")` report
/// the same *set* of words — order is deliberately not compared, see the
/// module doc comment.
#[test]
fn predictive_search_full_enumeration_matches_as_a_set() {
    let words = load_words();
    let (verbora, trie_rs, qp, fast_radix) = build_all(&words);
    let verbora_frozen = build_verbora_frozen(&verbora);
    let fst_set = build_fst(&words);

    // Already the deduplicated set (`BTreeSet` collapses the corpus's small
    // number of duplicate words) -- exactly what `fst::Set` can ever contain
    // (see `build_fst`'s own dedup), so this is the right baseline for it
    // too without any extra dedup step here.
    let expected: BTreeSet<String> = words.iter().cloned().collect();

    let verbora_set: BTreeSet<String> = verbora.keys_with_prefix("").into_iter().collect();
    assert_eq!(
        verbora_set, expected,
        "verbora full enumeration set mismatch"
    );

    let verbora_frozen_set: BTreeSet<String> =
        verbora_frozen.keys_with_prefix("").into_iter().collect();
    assert_eq!(
        verbora_frozen_set, expected,
        "verbora_frozen full enumeration set mismatch"
    );

    let trie_rs_set: BTreeSet<String> = trie_rs.predictive_search::<String, _>("").collect();
    assert_eq!(
        trie_rs_set, expected,
        "trie-rs full enumeration set mismatch"
    );

    let qp_set: BTreeSet<String> = qp
        .iter_prefix_str("")
        .map(|(k, _)| k.as_str().to_owned())
        .collect();
    assert_eq!(qp_set, expected, "qp-trie full enumeration set mismatch");

    let fast_radix_set: BTreeSet<String> = fast_radix.iter_prefix("").map(|(k, _)| k).collect();
    assert_eq!(
        fast_radix_set, expected,
        "fast_radix_trie full enumeration set mismatch"
    );

    let mut fst_stream = fst_set.stream();
    let mut fst_result = BTreeSet::new();
    while let Some(key) = fst_stream.next() {
        fst_result.insert(String::from_utf8(key.to_vec()).expect("ASCII corpus is valid UTF-8"));
    }
    assert_eq!(fst_result, expected, "fst full enumeration set mismatch");
}

/// One-letter-prefix predictive search agrees as a set, for every letter —
/// the shape `benches/trie.rs`'s `predictive_search/1char` group times.
#[test]
fn predictive_search_one_char_prefixes_match_as_a_set() {
    let words = load_words();
    let (verbora, trie_rs, qp, fast_radix) = build_all(&words);
    let verbora_frozen = build_verbora_frozen(&verbora);
    let fst_set = build_fst(&words);

    for c in b'a'..=b'z' {
        let prefix = (c as char).to_string();
        let expected: BTreeSet<String> = words
            .iter()
            .filter(|w| w.starts_with(&prefix))
            .cloned()
            .collect();

        let verbora_set: BTreeSet<String> = verbora.keys_with_prefix(&prefix).into_iter().collect();
        assert_eq!(
            verbora_set, expected,
            "verbora prefix {prefix:?} set mismatch"
        );

        let verbora_frozen_set: BTreeSet<String> = verbora_frozen
            .keys_with_prefix(&prefix)
            .into_iter()
            .collect();
        assert_eq!(
            verbora_frozen_set, expected,
            "verbora_frozen prefix {prefix:?} set mismatch"
        );

        let trie_rs_set: BTreeSet<String> = trie_rs
            .predictive_search::<String, _>(prefix.as_str())
            .collect();
        assert_eq!(
            trie_rs_set, expected,
            "trie-rs prefix {prefix:?} set mismatch"
        );

        let qp_set: BTreeSet<String> = qp
            .iter_prefix_str(prefix.as_str())
            .map(|(k, _)| k.as_str().to_owned())
            .collect();
        assert_eq!(qp_set, expected, "qp-trie prefix {prefix:?} set mismatch");

        let fast_radix_set: BTreeSet<String> = fast_radix
            .iter_prefix(prefix.as_str())
            .map(|(k, _)| k)
            .collect();
        assert_eq!(
            fast_radix_set, expected,
            "fast_radix_trie prefix {prefix:?} set mismatch"
        );

        // `Str::new(prefix).starts_with()` is `fst`'s prefix-enumeration
        // equivalent to `keys_with_prefix`/`predictive_search`/
        // `iter_prefix_str`/`iter_prefix` above -- see `benches/trie.rs`'s
        // `fst` section.
        let automaton = Str::new(prefix.as_str()).starts_with();
        let mut fst_stream = fst_set.search(automaton).into_stream();
        let mut fst_set_result = BTreeSet::new();
        while let Some(key) = fst_stream.next() {
            fst_set_result
                .insert(String::from_utf8(key.to_vec()).expect("ASCII corpus is valid UTF-8"));
        }
        assert_eq!(
            fst_set_result, expected,
            "fst prefix {prefix:?} set mismatch"
        );
    }
}

/// `Trie::find_matches_on_path` / `trie_rs::common_prefix_search` /
/// `fast_radix_trie::GenericRadixMap::common_prefixes` agree as a set, for a
/// real probe sample. `qp-trie` and `fst` have no native equivalent (see
/// `benches/trie.rs`'s module doc comment) so neither is checked here.
#[test]
fn common_prefix_search_matches_as_a_set() {
    let words = load_words();
    let (verbora, trie_rs, _qp, fast_radix) = build_all(&words);
    let unique: BTreeSet<&str> = words.iter().map(String::as_str).collect();

    for probe in words.iter().take(1000) {
        let expected: BTreeSet<String> = (1..=probe.len())
            .filter(|&i| probe.is_char_boundary(i))
            .map(|i| &probe[..i])
            .filter(|prefix| unique.contains(prefix))
            .map(str::to_owned)
            .collect();

        let verbora_set: BTreeSet<String> = verbora
            .find_matches_on_path(probe)
            .into_iter()
            .map(|c| c.into_owned())
            .collect();
        assert_eq!(
            verbora_set, expected,
            "verbora common-prefix set mismatch for {probe:?}"
        );

        let trie_rs_set: BTreeSet<String> = trie_rs
            .common_prefix_search::<String, _>(probe.as_bytes())
            .collect();
        assert_eq!(
            trie_rs_set, expected,
            "trie-rs common-prefix set mismatch for {probe:?}"
        );

        let fast_radix_set: BTreeSet<String> = fast_radix
            .common_prefixes(probe)
            .map(|(k, _)| k.to_owned())
            .collect();
        assert_eq!(
            fast_radix_set, expected,
            "fast_radix_trie common-prefix set mismatch for {probe:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// Shape-variant coverage — the claims behind the derived-shape benchmark rows
// added to `benches/trie.rs` (see its `# Derived input shapes` section). Each
// helper below is the identical deterministic expression the bench uses.
// ---------------------------------------------------------------------------

/// Identical construction to `benches/trie.rs::prefix_heavy` (and to
/// `crates/verbora-trie/benches/trie.rs`'s own) — the "prefix_heavy" `build`
/// dataset.
fn prefix_heavy(words: &[String]) -> Vec<String> {
    const SUFFIXES: [&str; 8] = ["", "s", "ed", "ing", "er", "est", "ly", "ness"];
    words
        .iter()
        .take(4000)
        .flat_map(|w| SUFFIXES.iter().map(move |s| format!("{w}{s}")))
        .collect()
}

/// The "sorted" shape — `benches/trie.rs::sorted_words`.
fn sorted_words(words: &[String]) -> Vec<String> {
    let mut v = words.to_vec();
    v.sort_unstable();
    v
}

/// The "short" shape — `benches/trie.rs::short_words`.
fn short_words(words: &[String]) -> Vec<String> {
    words.iter().filter(|w| w.len() <= 6).cloned().collect()
}

/// The "long" shape — `benches/trie.rs::long_words`.
fn long_words(words: &[String]) -> Vec<String> {
    words.iter().filter(|w| w.len() >= 11).cloned().collect()
}

/// Every distinct `n`-character prefix of the stored words, in sorted order —
/// `benches/trie.rs::distinct_prefixes`.
fn distinct_prefixes(words: &[String], n: usize) -> Vec<String> {
    words
        .iter()
        .filter(|w| w.len() >= n)
        .map(|w| w[..n].to_owned())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

/// All six structures built from `corpus` enumerate the same key set — the
/// "built the same set of keys" claim every derived-shape `build` row rests
/// on, checked per dataset exactly the way
/// `predictive_search_full_enumeration_matches_as_a_set` already checks the
/// "random" corpus. A sample of the corpus is also probed through each
/// yes/no `contains` API, since `build` rows report a size/count, not an
/// enumeration.
fn assert_corpus_agreement(corpus: &[String], label: &str) {
    let (verbora, trie_rs, qp, fast_radix) = build_all(corpus);
    let verbora_frozen = build_verbora_frozen(&verbora);
    let fst_set = build_fst(corpus);

    let expected: BTreeSet<String> = corpus.iter().cloned().collect();

    let verbora_set: BTreeSet<String> = verbora.keys_with_prefix("").into_iter().collect();
    assert_eq!(verbora_set, expected, "verbora enumeration on {label:?}");

    let verbora_frozen_set: BTreeSet<String> =
        verbora_frozen.keys_with_prefix("").into_iter().collect();
    assert_eq!(
        verbora_frozen_set, expected,
        "verbora_frozen enumeration on {label:?}"
    );

    let trie_rs_set: BTreeSet<String> = trie_rs.predictive_search::<String, _>("").collect();
    assert_eq!(trie_rs_set, expected, "trie-rs enumeration on {label:?}");

    let qp_set: BTreeSet<String> = qp
        .iter_prefix_str("")
        .map(|(k, _)| k.as_str().to_owned())
        .collect();
    assert_eq!(qp_set, expected, "qp-trie enumeration on {label:?}");

    let fast_radix_set: BTreeSet<String> = fast_radix.iter_prefix("").map(|(k, _)| k).collect();
    assert_eq!(
        fast_radix_set, expected,
        "fast_radix_trie enumeration on {label:?}"
    );

    let mut fst_stream = fst_set.stream();
    let mut fst_result = BTreeSet::new();
    while let Some(key) = fst_stream.next() {
        fst_result.insert(String::from_utf8(key.to_vec()).expect("ASCII corpus is valid UTF-8"));
    }
    assert_eq!(fst_result, expected, "fst enumeration on {label:?}");

    // Sampled `contains` hits (every 16th entry): the boolean API agrees on
    // the same corpus its enumeration was just verified for.
    for w in corpus.iter().step_by(16) {
        assert!(verbora.contains(w), "verbora missing {w:?} in {label:?}");
        assert!(
            verbora_frozen.contains(w),
            "verbora_frozen missing {w:?} in {label:?}"
        );
        assert!(
            trie_rs.exact_match(w.as_str()),
            "trie-rs missing {w:?} in {label:?}"
        );
        assert!(
            qp.contains_key_str(w.as_str()),
            "qp-trie missing {w:?} in {label:?}"
        );
        assert!(
            fast_radix.contains_key(w.as_str()),
            "fast_radix_trie missing {w:?} in {label:?}"
        );
        assert!(
            fst_set.contains(w.as_str()),
            "fst missing {w:?} in {label:?}"
        );
    }
}

/// The four derived `build` datasets each produce the same key set in all
/// six structures — the correctness claim behind `benches/trie.rs`'s
/// "prefix_heavy", "sorted", "short" and "long" `build` rows (the "random"
/// corpus is already covered by
/// `predictive_search_full_enumeration_matches_as_a_set` above).
#[test]
fn derived_build_datasets_agree_on_key_sets() {
    let words = load_words();
    assert_corpus_agreement(&prefix_heavy(&words), "prefix_heavy");
    assert_corpus_agreement(&sorted_words(&words), "sorted");
    assert_corpus_agreement(&short_words(&words), "short");
    assert_corpus_agreement(&long_words(&words), "long");
}

/// First-character misses (`QQ{w}` — the `contains_miss/first_char` bench
/// shape: no generated word starts with an uppercase letter) are
/// `contains`-false in all six structures, and truncated-word probes
/// (`w` minus its last character — sometimes a stored word, sometimes a
/// non-terminal interior node) get the *same* yes/no answer from all six,
/// equal to real set membership. The truncated shape matters because it is
/// the one probe family where the walk succeeds but terminality decides the
/// answer — a different code path from both miss conventions.
#[test]
fn contains_agrees_on_first_char_and_truncated_probes() {
    let words = load_words();
    let (verbora, trie_rs, qp, fast_radix) = build_all(&words);
    let verbora_frozen = build_verbora_frozen(&verbora);
    let fst_set = build_fst(&words);
    let unique: BTreeSet<&str> = words.iter().map(String::as_str).collect();

    for w in words.iter().take(2000) {
        let miss = format!("QQ{w}");
        assert!(!verbora.contains(&miss), "verbora false hit on {miss:?}");
        assert!(
            !verbora_frozen.contains(&miss),
            "verbora_frozen false hit on {miss:?}"
        );
        assert!(
            !trie_rs.exact_match(miss.as_str()),
            "trie-rs false hit on {miss:?}"
        );
        assert!(
            !qp.contains_key_str(miss.as_str()),
            "qp-trie false hit on {miss:?}"
        );
        assert!(
            !fast_radix.contains_key(miss.as_str()),
            "fast_radix_trie false hit on {miss:?}"
        );
        assert!(
            !fst_set.contains(miss.as_str()),
            "fst false hit on {miss:?}"
        );
    }

    for w in words.iter().take(2000) {
        let probe = &w[..w.len() - 1];
        let expected = unique.contains(probe);
        assert_eq!(
            verbora.contains(probe),
            expected,
            "verbora disagrees on truncated probe {probe:?}"
        );
        assert_eq!(
            verbora_frozen.contains(probe),
            expected,
            "verbora_frozen disagrees on truncated probe {probe:?}"
        );
        assert_eq!(
            trie_rs.exact_match(probe),
            expected,
            "trie-rs disagrees on truncated probe {probe:?}"
        );
        assert_eq!(
            qp.contains_key_str(probe),
            expected,
            "qp-trie disagrees on truncated probe {probe:?}"
        );
        assert_eq!(
            fast_radix.contains_key(probe),
            expected,
            "fast_radix_trie disagrees on truncated probe {probe:?}"
        );
        assert_eq!(
            fst_set.contains(probe),
            expected,
            "fst disagrees on truncated probe {probe:?}"
        );
    }
}

/// Multi-character predictive search agrees as a set — the
/// `predictive_search/2char` and `/4char` bench shapes. "2char" is checked
/// exhaustively (all 676 distinct prefixes); "4char" over the exact
/// deterministic sample the bench iterates (every 16th distinct prefix in
/// sorted order).
#[test]
fn predictive_search_multi_char_prefixes_match_as_a_set() {
    let words = load_words();
    let (verbora, trie_rs, qp, fast_radix) = build_all(&words);
    let verbora_frozen = build_verbora_frozen(&verbora);
    let fst_set = build_fst(&words);

    let two_char = distinct_prefixes(&words, 2);
    let four_char: Vec<String> = distinct_prefixes(&words, 4)
        .into_iter()
        .step_by(16)
        .collect();

    for prefix in two_char.iter().chain(four_char.iter()) {
        let expected: BTreeSet<String> = words
            .iter()
            .filter(|w| w.starts_with(prefix.as_str()))
            .cloned()
            .collect();

        let verbora_set: BTreeSet<String> = verbora.keys_with_prefix(prefix).into_iter().collect();
        assert_eq!(
            verbora_set, expected,
            "verbora prefix {prefix:?} set mismatch"
        );

        let verbora_frozen_set: BTreeSet<String> = verbora_frozen
            .keys_with_prefix(prefix)
            .into_iter()
            .collect();
        assert_eq!(
            verbora_frozen_set, expected,
            "verbora_frozen prefix {prefix:?} set mismatch"
        );

        let trie_rs_set: BTreeSet<String> = trie_rs
            .predictive_search::<String, _>(prefix.as_str())
            .collect();
        assert_eq!(
            trie_rs_set, expected,
            "trie-rs prefix {prefix:?} set mismatch"
        );

        let qp_set: BTreeSet<String> = qp
            .iter_prefix_str(prefix.as_str())
            .map(|(k, _)| k.as_str().to_owned())
            .collect();
        assert_eq!(qp_set, expected, "qp-trie prefix {prefix:?} set mismatch");

        let fast_radix_set: BTreeSet<String> = fast_radix
            .iter_prefix(prefix.as_str())
            .map(|(k, _)| k)
            .collect();
        assert_eq!(
            fast_radix_set, expected,
            "fast_radix_trie prefix {prefix:?} set mismatch"
        );

        let automaton = Str::new(prefix.as_str()).starts_with();
        let mut fst_stream = fst_set.search(automaton).into_stream();
        let mut fst_result = BTreeSet::new();
        while let Some(key) = fst_stream.next() {
            fst_result
                .insert(String::from_utf8(key.to_vec()).expect("ASCII corpus is valid UTF-8"));
        }
        assert_eq!(fst_result, expected, "fst prefix {prefix:?} set mismatch");
    }
}

/// Predictive search where the prefix is itself a stored word — the deepest
/// prefix shape (the result set must include the prefix itself, plus any
/// stored extensions of it). Distinct from `1char`/`2char`/`4char` above:
/// the descent ends exactly on a terminal node, so an off-by-one on
/// terminality would surface here and nowhere else.
#[test]
fn predictive_search_full_word_prefixes_match_as_a_set() {
    let words = load_words();
    let (verbora, trie_rs, qp, fast_radix) = build_all(&words);
    let verbora_frozen = build_verbora_frozen(&verbora);
    let fst_set = build_fst(&words);
    let unique: BTreeSet<&str> = words.iter().map(String::as_str).collect();

    for &prefix in unique.iter().take(500) {
        let expected: BTreeSet<String> = words
            .iter()
            .filter(|w| w.starts_with(prefix))
            .cloned()
            .collect();
        assert!(
            expected.contains(prefix),
            "a stored word is always in its own prefix's result set"
        );

        let verbora_set: BTreeSet<String> = verbora.keys_with_prefix(prefix).into_iter().collect();
        assert_eq!(
            verbora_set, expected,
            "verbora word-prefix {prefix:?} set mismatch"
        );

        let verbora_frozen_set: BTreeSet<String> = verbora_frozen
            .keys_with_prefix(prefix)
            .into_iter()
            .collect();
        assert_eq!(
            verbora_frozen_set, expected,
            "verbora_frozen word-prefix {prefix:?} set mismatch"
        );

        let trie_rs_set: BTreeSet<String> =
            trie_rs.predictive_search::<String, _>(prefix).collect();
        assert_eq!(
            trie_rs_set, expected,
            "trie-rs word-prefix {prefix:?} set mismatch"
        );

        let qp_set: BTreeSet<String> = qp
            .iter_prefix_str(prefix)
            .map(|(k, _)| k.as_str().to_owned())
            .collect();
        assert_eq!(
            qp_set, expected,
            "qp-trie word-prefix {prefix:?} set mismatch"
        );

        let fast_radix_set: BTreeSet<String> =
            fast_radix.iter_prefix(prefix).map(|(k, _)| k).collect();
        assert_eq!(
            fast_radix_set, expected,
            "fast_radix_trie word-prefix {prefix:?} set mismatch"
        );

        let automaton = Str::new(prefix).starts_with();
        let mut fst_stream = fst_set.search(automaton).into_stream();
        let mut fst_result = BTreeSet::new();
        while let Some(key) = fst_stream.next() {
            fst_result
                .insert(String::from_utf8(key.to_vec()).expect("ASCII corpus is valid UTF-8"));
        }
        assert_eq!(
            fst_result, expected,
            "fst word-prefix {prefix:?} set mismatch"
        );
    }
}

/// Common-prefix search on the "extended" probe shape (`{w}ed` — the probe
/// is not itself a stored key, and the stored word is a guaranteed strict
/// prefix on the probe's path), plus a second, disjoint slice of plain
/// word probes (`skip(1000).take(1000)`) doubling
/// `common_prefix_search_matches_as_a_set`'s sweep. Same three structures
/// as that test: `qp-trie` and `fst` have no native equivalent of this
/// operation (see `benches/trie.rs`'s module doc comment) so neither is
/// checked here.
#[test]
fn common_prefix_search_extended_and_second_slice_match_as_a_set() {
    let words = load_words();
    let (verbora, trie_rs, _qp, fast_radix) = build_all(&words);
    let unique: BTreeSet<&str> = words.iter().map(String::as_str).collect();

    let extended: Vec<String> = words.iter().take(1000).map(|w| format!("{w}ed")).collect();
    let second_slice: Vec<String> = words.iter().skip(1000).take(1000).cloned().collect();

    for probe in extended.iter().chain(second_slice.iter()) {
        let expected: BTreeSet<String> = (1..=probe.len())
            .filter(|&i| probe.is_char_boundary(i))
            .map(|i| &probe[..i])
            .filter(|prefix| unique.contains(prefix))
            .map(str::to_owned)
            .collect();

        let verbora_set: BTreeSet<String> = verbora
            .find_matches_on_path(probe)
            .into_iter()
            .map(|c| c.into_owned())
            .collect();
        assert_eq!(
            verbora_set, expected,
            "verbora common-prefix set mismatch for {probe:?}"
        );

        let trie_rs_set: BTreeSet<String> = trie_rs
            .common_prefix_search::<String, _>(probe.as_bytes())
            .collect();
        assert_eq!(
            trie_rs_set, expected,
            "trie-rs common-prefix set mismatch for {probe:?}"
        );

        let fast_radix_set: BTreeSet<String> = fast_radix
            .common_prefixes(probe)
            .map(|(k, _)| k.to_owned())
            .collect();
        assert_eq!(
            fast_radix_set, expected,
            "fast_radix_trie common-prefix set mismatch for {probe:?}"
        );
    }
}
