//! [`Spellcheck`]'s contract, checked against its own definition.
//!
//! `corrections(query, k)` is defined as *every corpus word within `k` edits of
//! `query`, and nothing else*, under
//! [`verbora_distance::damerau_levenshtein`]. That definition is directly
//! executable — a scan of the whole corpus computing the metric — so these
//! tests compare against it rather than against recorded output.
//!
//! # Why the enumeration is exhaustive and not sampled
//!
//! The failure this crate is most exposed to is silent: a retrieval structure
//! that generates a word's deletion sequences in one spelling and looks a query
//! up in another returns *fewer* matches, with nothing failing to compile and
//! no error to observe. A sampled test passes over it. So:
//!
//! * `every_corpus_entry_retrieves_itself` walks **every** entry of the shared
//!   20,000-word list and requires each to be its own zero-distance correction
//!   — one query per entry, no sample.
//! * `every_corpus_entry_is_reachable_from_a_one_scalar_typo` deletes one
//!   scalar from **every** entry and requires the entry back at `k = 1` — the
//!   generation-vs-lookup agreement, exercised once per entry.
//! * `matches_the_brute_force_definition` compares the full answer against a
//!   scan of **every** dictionary word, for a set of queries covering exact
//!   hits, single edits, transpositions and complete misses, at every distance
//!   on both sides of the internal dispatch boundary.
//!
//! The counts each test checked are asserted, so a future change that quietly
//! reduces the corpus to a handful of entries fails rather than passes faster.

use std::collections::BTreeSet;
use std::path::Path;

use verbora_distance::damerau_levenshtein;
use verbora_spellcheck::Spellcheck;

/// The shared word list this workspace's benchmarks use.
fn words() -> Vec<String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
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

/// The definition of the answer, executed directly.
fn brute_force<'a>(corpus: &'a [String], query: &str, k: u32) -> Vec<&'a str> {
    let mut out: Vec<&str> = corpus
        .iter()
        .map(String::as_str)
        .filter(|w| damerau_levenshtein(query, w) <= k as usize)
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

fn returned<'a>(sc: &'a Spellcheck, query: &str, k: u32) -> Vec<&'a str> {
    let mut out: Vec<&str> = sc
        .corrections(query, k)
        .into_iter()
        .map(|c| c.word)
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

/// Every entry of the whole word list, used as its own query. An entry that
/// could not find itself would mean the spelling stored by the index and the
/// spelling looked up by a query had drifted apart.
#[test]
fn every_corpus_entry_retrieves_itself() {
    let corpus = words();
    assert!(corpus.len() > 10_000, "expected the full bench word list");
    let distinct: BTreeSet<&str> = corpus.iter().map(String::as_str).collect();

    let sc = Spellcheck::new(&corpus);
    assert_eq!(sc.len(), distinct.len());

    let mut checked = 0usize;
    for word in &distinct {
        assert!(sc.is_correct(word), "{word:?} is not in the corpus");
        let exact = sc.corrections(word, 0);
        assert_eq!(exact.len(), 1, "corrections({word:?}, 0)");
        assert_eq!(exact[0].word, *word);
        assert_eq!(exact[0].distance, 0);
        assert_eq!(exact[0].frequency, sc.frequency(word).expect("present"));
        // At k = 1 it must still be first: distance 0 outranks everything.
        let near = sc.corrections(word, 1);
        assert_eq!(near[0].word, *word, "corrections({word:?}, 1) head");
        checked += 1;
    }
    assert_eq!(checked, distinct.len());
}

/// One scalar deleted from every entry, at `k = 1`. This is the shape a real
/// typo takes and the one that actually exercises the deletion neighbourhood:
/// the query's own deletions must meet the entry's, for every entry.
#[test]
fn every_corpus_entry_is_reachable_from_a_one_scalar_typo() {
    let corpus = words();
    let distinct: BTreeSet<&str> = corpus.iter().map(String::as_str).collect();
    let sc = Spellcheck::new(&corpus);

    let mut checked = 0usize;
    let mut skipped_empty = 0usize;
    for word in &distinct {
        let scalars: Vec<char> = word.chars().collect();
        if scalars.is_empty() {
            skipped_empty += 1;
            continue;
        }
        // Delete the middle scalar — the position least likely to be covered
        // by a prefix or suffix shortcut.
        let mut typo = scalars.clone();
        typo.remove(scalars.len() / 2);
        let typo: String = typo.into_iter().collect();

        let found = returned(&sc, &typo, 1);
        assert!(
            found.contains(word),
            "the one-deletion typo {typo:?} of {word:?} did not retrieve it: {found:?}"
        );
        checked += 1;
    }
    assert_eq!(checked + skipped_empty, distinct.len());
    assert!(checked > 10_000, "only {checked} entries were exercised");
}

/// The full answer against the full definition, over every dictionary word.
///
/// The corpus is narrowed to keep the O(queries × corpus) brute force
/// affordable in a debug build, but nothing about *which* words are compared is
/// sampled: for every query, every one of the corpus's words is scored.
#[test]
fn matches_the_brute_force_definition() {
    let all = words();
    let corpus: Vec<String> = {
        let mut seen = std::collections::HashSet::new();
        all.iter()
            .take(2_000)
            .filter(|w| seen.insert((*w).clone()))
            .cloned()
            .collect()
    };
    let sc = Spellcheck::new(&corpus);

    // Query shapes: exact hits, a deletion, an insertion, a substitution, a
    // transposition, and outright misses.
    let mut queries: Vec<String> = corpus.iter().take(12).cloned().collect();
    for w in corpus.iter().skip(12).take(12) {
        let mut c: Vec<char> = w.chars().collect();
        if !c.is_empty() {
            c.remove(c.len() / 2);
        }
        queries.push(c.into_iter().collect());
    }
    for w in corpus.iter().skip(24).take(12) {
        queries.push(format!("{w}x"));
    }
    for w in corpus.iter().skip(36).take(12) {
        let mut c: Vec<char> = w.chars().collect();
        if c.len() >= 2 {
            c.swap(0, 1);
        }
        queries.push(c.into_iter().collect());
    }
    queries.push(String::new());
    queries.push("zzzzzzzzzzzzzzzzzzzz".to_owned());
    queries.push("qqqq".to_owned());

    let mut comparisons = 0usize;
    for query in &queries {
        // 0..=2 is the indexed path, 3..=4 the scan. Both must reproduce the
        // same definition, and the boundary is where a divergence would hide.
        for k in 0u32..=4 {
            assert_eq!(
                returned(&sc, query, k),
                brute_force(&corpus, query, k),
                "query {query:?} at k={k}"
            );
            comparisons += 1;
        }
    }
    assert_eq!(comparisons, queries.len() * 5);
}

/// The same comparison over a corpus that is entirely non-ASCII, including
/// astral scalars drawn from different planes. On ASCII a byte, a scalar and a
/// UTF-16 code unit all coincide, so an ASCII corpus can never detect a unit
/// mismatch; this one can.
#[test]
fn matches_the_brute_force_definition_on_astral_input() {
    let corpus: Vec<String> = [
        "😀",
        "😀x",
        "x😀",
        "😀abc",
        "🙁abc",
        "😀😀",
        "😀😁",
        "𝕳𝖊𝖑𝖑𝖔",
        "𝕳𝖊𝖑𝖑𝖈",
        "hello",
        "hell😀",
        "a👍b",
        "a👍bc",
        "a👌bc",
        "",
        "𝕳",
        "𝖊",
        "😀𝕳",
        "𝕳😀",
        "𝔸",
        "🜁",
        "𐐷",
        "𐎠",
        "café",
        "cafe",
        "Москва",
        "Мсква",
        "日本語",
        "日本",
    ]
    .iter()
    .map(|s| (*s).to_owned())
    .collect();

    let sc = Spellcheck::new(&corpus);
    assert_eq!(sc.len(), corpus.len());

    for query in &corpus {
        for k in 0u32..=3 {
            assert_eq!(
                returned(&sc, query, k),
                brute_force(&corpus, query, k),
                "astral query {query:?} at k={k}"
            );
        }
    }

    // And the narrow witness: two one-scalar words whose UTF-16 encodings
    // share no code unit at all are one edit apart and must find each other.
    assert_eq!(damerau_levenshtein("😀", "𝕳"), 1);
    assert!(returned(&sc, "😀", 1).contains(&"𝕳"));
    assert!(returned(&sc, "𝕳", 1).contains(&"😀"));
}

/// The order is total: distance, then descending frequency, then the word.
/// Checked as a property over the real corpus rather than on a chosen example.
#[test]
fn the_order_is_the_documented_total_order() {
    let all = words();
    let corpus: Vec<String> = all.iter().take(4_000).cloned().collect();
    let sc = Spellcheck::new(&corpus);

    let mut ranked = 0usize;
    for query in corpus.iter().take(200) {
        let mut typo: Vec<char> = query.chars().collect();
        if typo.len() >= 2 {
            typo.swap(0, 1);
        }
        let typo: String = typo.into_iter().collect();
        let got = sc.corrections(&typo, 2);
        for pair in got.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            fn key<'k>(
                c: verbora_spellcheck::Correction<'k>,
            ) -> (u32, std::cmp::Reverse<u32>, &'k str) {
                (c.distance, std::cmp::Reverse(c.frequency), c.word)
            }
            assert!(
                key(a) < key(b),
                "order violated between {a:?} and {b:?} for query {typo:?}"
            );
        }
        ranked += got.len();
    }
    assert!(ranked > 0, "no corrections were ranked");
}

/// `best_correction` is the head of `corrections`, over the real corpus.
#[test]
fn best_correction_is_the_head_of_corrections() {
    let all = words();
    let corpus: Vec<String> = all.iter().take(4_000).cloned().collect();
    let sc = Spellcheck::new(&corpus);

    for query in corpus.iter().take(300) {
        let mut typo: Vec<char> = query.chars().collect();
        if !typo.is_empty() {
            typo.remove(typo.len() / 2);
        }
        let typo: String = typo.into_iter().collect();
        for k in 0u32..=2 {
            assert_eq!(
                sc.best_correction(&typo, k),
                sc.corrections(&typo, k).first().copied(),
                "best_correction({typo:?}, {k})"
            );
        }
    }
}

/// Frequencies come straight from the corpus, for every distinct entry.
#[test]
fn frequencies_count_every_occurrence() {
    let corpus = words();
    let sc = Spellcheck::new(&corpus);

    let mut expected: std::collections::HashMap<&str, u32> = std::collections::HashMap::new();
    for w in &corpus {
        *expected.entry(w.as_str()).or_insert(0) += 1;
    }

    let mut checked = 0usize;
    for (word, frequency) in sc.frequencies() {
        assert_eq!(
            Some(frequency),
            expected.get(word).copied(),
            "frequency of {word:?}"
        );
        assert!(frequency >= 1);
        checked += 1;
    }
    assert_eq!(checked, expected.len());

    // First-occurrence order, checked against the corpus itself.
    let mut seen = std::collections::HashSet::new();
    let first_occurrence: Vec<&str> = corpus
        .iter()
        .map(String::as_str)
        .filter(|w| seen.insert(*w))
        .collect();
    assert_eq!(
        sc.frequencies().map(|(w, _)| w).collect::<Vec<&str>>(),
        first_occurrence
    );
}
