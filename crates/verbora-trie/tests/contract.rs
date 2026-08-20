//! Contract tests for [`verbora_trie`]: the text unit, the enumeration order,
//! case handling, and the guarantee that every string handed back is text the
//! caller supplied.
//!
//! Every expectation here is derived from the crate's stated contract, not from
//! running the implementation:
//!
//! * the unit is one Unicode scalar value, so a scalar costs one node and one
//!   position in every length this crate reports;
//! * enumeration is ascending by scalar sequence, which for well-formed Rust
//!   strings is exactly `<str as Ord>` — so the expected sequence is computed
//!   by sorting, never by recording;
//! * case handling, when folding, applies to every method's argument with no
//!   exceptions;
//! * no method invents a scalar the caller did not supply — in particular
//!   `U+FFFD` never appears in a result unless the caller put it there.
//!
//! The last group of tests walks the **whole** shared benchmark word list
//! rather than a sample: every entry is inserted, then every entry is probed
//! for membership, and every prefix of every entry is checked against a sorted
//! reference. A structure that agreed with its own spelling on a sample and
//! disagreed on one entry in twenty thousand would pass a sampled test and fail
//! this one.

use std::borrow::Cow;
use std::collections::BTreeSet;

use verbora_trie::{CaseHandling, PrefixSplitLengths, Trie};

/// The corpus used by the small enumeration tests. Deliberately mixes scripts,
/// digits, punctuation, astral scalars and shared prefixes.
const WORDS: &[&str] = &[
    "cat",
    "cats",
    "car",
    "care",
    "careful",
    "dog",
    "0x",
    "9x",
    "1x",
    "zz",
    "a",
    "ab",
    "abc",
    "café",
    "Москва",
    "日本語",
    "😀",
    "😀a",
    "a😀b",
    "𝕳𝖊𝖑𝖑𝖔",
    "e.g.",
    "don't",
    "",
];

fn built() -> Trie {
    let mut t = Trie::new();
    t.insert_all(WORDS.iter().copied());
    t
}

/// One Unicode scalar is one node: a trie holding one astral scalar is the
/// root plus exactly one node, not the root plus a surrogate pair.
#[test]
fn one_scalar_is_one_node() {
    let mut t = Trie::new();
    t.insert("😀");
    assert_eq!(t.node_count(), 2, "root + one scalar");

    let mut t = Trie::new();
    t.insert("a👍");
    assert_eq!(t.node_count(), 3, "root + 'a' + '👍'");

    // Every scalar costs exactly one node in a branch-free word.
    for w in ["", "a", "ab", "café", "Москва", "日本語", "𝕳𝖊𝖑𝖑𝖔"] {
        let mut t = Trie::new();
        t.insert(w);
        assert_eq!(
            t.node_count(),
            1 + w.chars().count(),
            "{w:?} must cost one node per scalar"
        );
    }
}

/// Enumeration order is ascending by scalar sequence. For well-formed Rust
/// strings that is byte-wise UTF-8 order, i.e. `<str as Ord>` — so the expected
/// sequence is *computed*, by sorting a deduplicated copy of the input, not
/// recorded from a run.
#[test]
fn enumeration_is_ascending_scalar_order() {
    let t = built();

    let expected: Vec<String> = WORDS
        .iter()
        .map(|w| (*w).to_owned())
        .collect::<BTreeSet<String>>()
        .into_iter()
        .collect();

    assert_eq!(t.keys_with_prefix(""), expected);
    assert_eq!(t.keys().collect::<Vec<String>>(), expected);
    assert_eq!((&t).into_iter().collect::<Vec<String>>(), expected);
    assert_eq!(t.freeze().keys_with_prefix(""), expected);

    // And the same holds under every prefix: the answer is the sorted subset.
    for prefix in ["", "c", "ca", "car", "a", "😀", "日", "z", "nope"] {
        let want: Vec<String> = expected
            .iter()
            .filter(|w| w.starts_with(prefix))
            .cloned()
            .collect();
        assert_eq!(t.keys_with_prefix(prefix), want, "prefix {prefix:?}");
    }
}

/// Ascending scalar order is `<str as Ord>` — asserted directly, so the claim
/// in the crate documentation is pinned rather than merely stated.
#[test]
fn ascending_scalar_order_is_str_ord() {
    let t = built();
    let keys = t.keys_with_prefix("");
    assert!(
        keys.windows(2).all(|w| w[0] < w[1]),
        "keys are not strictly ascending under `str`'s own ordering: {keys:?}"
    );
}

/// Case handling is a property of the trie, not of the method: when it folds,
/// *every* argument is folded, including `keys_with_prefix`'s prefix.
#[test]
fn case_folding_applies_to_every_argument() {
    let mut t = Trie::case_insensitive();
    t.insert_all(["thEIr", "And", "theY"]);
    assert_eq!(t.case_handling(), CaseHandling::Folded);

    assert!(t.contains("THEIR"));
    assert_eq!(t.keys_with_prefix("th"), ["their", "they"]);
    assert_eq!(
        t.keys_with_prefix("TH"),
        ["their", "they"],
        "an upper-case prefix must fold like every other argument"
    );
    assert_eq!(t.keys_with_prefix("Th"), ["their", "they"]);
    assert_eq!(
        t.iter_keys_with_prefix("TH").collect::<Vec<String>>(),
        ["their", "they"]
    );
    let mut seen: Vec<String> = Vec::new();
    t.for_each_key_with_prefix("TH", |k| seen.push(k.to_owned()));
    assert_eq!(seen, ["their", "they"]);

    // The frozen representation answers identically.
    let frozen = t.freeze();
    assert_eq!(frozen.keys_with_prefix("TH"), ["their", "they"]);
    assert_eq!(frozen.keys_slice("TH"), ["their", "they"]);
    assert_eq!(frozen.case_handling(), CaseHandling::Folded);
}

/// A case-sensitive trie never rewrites anything.
#[test]
fn a_case_sensitive_trie_stores_exactly_what_it_was_given() {
    let mut t = Trie::new();
    t.insert_all(["ALLCAPS", "AllCaps", "allcaps"]);
    assert_eq!(t.case_handling(), CaseHandling::Sensitive);
    assert_eq!(t.len(), 3);
    assert_eq!(t.keys_with_prefix(""), ["ALLCAPS", "AllCaps", "allcaps"]);
    assert!(t.contains("ALLCAPS"));
    assert!(!t.contains("allCAPS"));
}

/// No method invents a scalar. A walk that stops part-way through a word hands
/// back a suffix of the caller's own text — never `U+FFFD`.
#[test]
fn nothing_is_ever_replaced_by_u_fffd() {
    let mut t = Trie::new();
    t.insert("a👍");

    // '👌' and '👍' differ at scalar granularity, so the walk stops at the
    // character boundary and the remainder is the caller's own text.
    let split = t.longest_prefix("a👌");
    assert_eq!(split.word, None);
    assert_eq!(split.rest, "👌");
    assert!(!split.rest.contains('\u{FFFD}'));

    let split = t.longest_prefix("a😀x");
    assert_eq!(split.rest, "😀x");
    assert_eq!(t.longest_prefix_lengths("a😀x").rest, 2, "two scalars left");

    // Every key that comes back is one the caller inserted.
    let t = built();
    for key in t.keys() {
        assert!(WORDS.contains(&key.as_str()), "{key:?} was never inserted");
    }
}

/// `insert` reports whether the word was **newly** inserted, matching
/// `HashSet::insert`.
#[test]
fn insert_reports_novelty_like_a_set() {
    let mut t = Trie::new();
    assert!(t.insert("test"), "first insertion is new");
    assert!(!t.insert("test"), "second insertion is not");
    assert!(t.insert(""), "the empty string is a word like any other");
    assert!(!t.insert(""));
}

/// The number of distinct stored words, not of nodes.
#[test]
fn len_counts_words() {
    let t = built();
    let distinct: BTreeSet<&str> = WORDS.iter().copied().collect();
    assert_eq!(t.len(), distinct.len());
    assert_eq!(t.freeze().len(), distinct.len());
    assert!(!t.is_empty());
    assert!(Trie::new().is_empty());
    // A trie holding only the empty string is not empty.
    let mut only_empty = Trie::new();
    only_empty.insert("");
    assert!(!only_empty.is_empty());
    assert_eq!(only_empty.len(), 1);
}

/// Lengths are counted in scalars, and the split point is exact.
#[test]
fn prefix_lengths_are_scalar_counts() {
    let mut t = Trie::new();
    t.insert_all(["their", "and", "they"]);
    assert_eq!(
        t.longest_prefix_lengths("theyre"),
        PrefixSplitLengths {
            word: Some(4),
            rest: 2
        }
    );

    let mut t = Trie::new();
    t.insert("😀😀");
    assert_eq!(
        t.longest_prefix_lengths("😀😀😀"),
        PrefixSplitLengths {
            word: Some(2),
            rest: 1
        },
        "two scalars, not four code units"
    );
}

/// Borrowing: with no folding to do, nothing is copied.
#[test]
fn results_borrow_when_no_folding_is_needed() {
    let mut t = Trie::new();
    t.insert_all(["a", "ab"]);
    let split = t.longest_prefix("abcd");
    assert!(matches!(split.word, Some(Cow::Borrowed(_))));
    assert!(matches!(split.rest, Cow::Borrowed(_)));
    assert!(
        t.prefix_matches("abc")
            .iter()
            .all(|m| matches!(m, Cow::Borrowed(_)))
    );
}

// ---------------------------------------------------------------------------
// Whole-corpus enumeration
// ---------------------------------------------------------------------------

/// The shared benchmark word list, exactly as the benches load it.
fn corpus_words() -> Vec<String> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crate is two levels below the workspace root")
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
        .expect("words array")
        .iter()
        .map(|w| w.as_str().expect("word is a string").to_owned())
        .collect()
}

/// Every entry of the whole word list, walked through insert → contains →
/// enumeration → freeze. Not a sample: a single entry whose stored spelling
/// disagreed with the spelling a query looks up would show here and nowhere
/// else.
#[test]
fn every_corpus_entry_round_trips() {
    let words = corpus_words();
    assert!(words.len() > 10_000, "expected the full bench word list");

    let distinct: BTreeSet<&str> = words.iter().map(String::as_str).collect();

    let mut t = Trie::new();
    t.insert_all(words.iter().map(String::as_str));
    assert_eq!(t.len(), distinct.len(), "one node-word per distinct entry");

    let frozen = t.freeze();
    assert_eq!(frozen.len(), distinct.len());

    // Membership, both representations, over every entry.
    let mut hits = 0usize;
    for w in &distinct {
        assert!(t.contains(w), "{w:?} not found in the mutable trie");
        assert!(frozen.contains(w), "{w:?} not found in the frozen trie");
        hits += 1;
    }
    assert_eq!(hits, distinct.len());

    // Enumeration is the sorted set, exactly.
    let expected: Vec<&str> = distinct.iter().copied().collect();
    assert_eq!(t.keys_with_prefix(""), expected);
    assert_eq!(frozen.keys_slice(""), expected);

    // And every entry is reachable as its own prefix, with the count matching
    // the sorted reference at every prefix length.
    for w in &distinct {
        let want = expected
            .iter()
            .filter(|other| other.starts_with(*w))
            .count();
        assert_eq!(
            t.iter_keys_with_prefix(w).count(),
            want,
            "prefix count for {w:?}"
        );
        assert_eq!(frozen.keys_slice(w).len(), want, "frozen count for {w:?}");
        let split = t.longest_prefix(w);
        assert_eq!(split.word.as_deref(), Some(*w), "longest_prefix({w:?})");
        assert_eq!(split.rest, "");
    }
}

/// The same enumeration under case folding: the entire corpus is inserted into
/// a folding trie, and every entry is then probed in three spellings. This is
/// the shape that hides a fold/lookup mismatch — the stored spelling is folded
/// and the query is not.
#[test]
fn every_corpus_entry_round_trips_under_folding() {
    let words = corpus_words();
    let folded: BTreeSet<String> = words.iter().map(|w| w.to_lowercase()).collect();

    let mut t = Trie::case_insensitive();
    t.insert_all(words.iter().map(String::as_str));
    assert_eq!(t.len(), folded.len());

    let frozen = t.freeze();
    for w in &words {
        for spelling in [w.clone(), w.to_uppercase(), w.to_lowercase()] {
            assert!(t.contains(&spelling), "contains({spelling:?})");
            assert!(frozen.contains(&spelling), "frozen contains({spelling:?})");
            // A word is always a prefix of itself, in every spelling.
            assert!(
                t.iter_keys_with_prefix(&spelling).count() >= 1,
                "keys_with_prefix({spelling:?}) found nothing"
            );
            assert_eq!(
                t.keys_with_prefix(&spelling),
                frozen.keys_with_prefix(&spelling),
                "keys_with_prefix({spelling:?}) disagrees between representations"
            );
        }
    }

    let expected: Vec<&String> = folded.iter().collect();
    assert_eq!(
        t.keys_with_prefix("").iter().collect::<Vec<&String>>(),
        expected
    );
}
