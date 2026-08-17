//! `CORRECTNESS BEFORE PERFORMANCE` for the four competitors this round adds
//! to `benches/distance.rs`: `stringmetrics`, `eddie`, `triple_accel`,
//! `editdistancek`. See `docs/COMPETITIVE_BENCHMARKS.md` §1.8 for why each
//! was selected and `benches/distance.rs`'s own doc comment for exactly
//! which rows are and are not benchmarked.
//!
//! What is checked here, once and outside the timed code, before any timing
//! number for these four crates is trusted:
//!
//! 1. `stringmetrics::levenshtein`, `triple_accel::levenshtein` and
//!    `editdistancek::edit_distance` agree numerically with Verbora's
//!    `levenshtein` on the whole shared ASCII corpus (matrix: all three
//!    `Yes`/`Selected cases` for full algorithmic equivalence on unit
//!    costs).
//! 2. `stringmetrics::hamming` and `triple_accel::hamming` agree with
//!    Verbora's `hamming` on the same corpus (all pairs are same-length by
//!    construction, so the `-1`/`Err` incomparable-length branch never
//!    triggers here).
//! 3. `triple_accel::rdamerau` (restricted-only Damerau) agrees with
//!    Verbora's `damerau_levenshtein` in restricted/OSA mode.
//! 4. `eddie::Jaro`/`eddie::JaroWinkler` agree with Verbora's `jaro`/
//!    `jaro_winkler` on the shared corpus AND a set of hand-picked edge
//!    cases — the matrix's own "crate abandoned since 2020, re-verify
//!    against fresh vectors" flag for this crate, made concrete. One real
//!    divergence is found and documented (not hidden): both-empty-string
//!    Jaro similarity. It never affects `benches/distance.rs` because the
//!    shared corpus contains no empty strings at any of its five sizes.
//! 5. `triple_accel::levenshtein_search` runs to completion against the
//!    shared corpus without panicking, alongside Verbora's own
//!    `levenshtein_search` — this is *not* an output-equivalence claim
//!    (matrix: `Selected cases`, "a different problem shape": Verbora
//!    returns one best match with a backtrace, triple_accel a bounded-`k`
//!    iterator over every match), only the "does not panic, produces
//!    sane offsets" claim the fuzzy-substring-search row can honestly make.
//! 6. `stringmetrics::damerau_levenshtein` is confirmed NOT reachable at
//!    all in 2.2.2 by reading the vendored source directly (see
//!    `manifests/competitors.json`'s own `stringmetrics` entry) — the
//!    `damerau` module is commented out of both `mod` and `pub use` in
//!    `algorithms.rs`, so there is nothing to call here even if we wanted
//!    to; this is recorded in prose, not as a runtime test, because a test
//!    cannot call a symbol the crate does not export.

use eddie::{Jaro as EddieJaro, JaroWinkler as EddieJaroWinkler};
use verbora_distance::{
    hamming, jaro, jaro_winkler,
    levenshtein::{Options as LevOptions, damerau_levenshtein, levenshtein, levenshtein_search},
};

/// Same loader, same file, as `benches/distance.rs` — see that file's own
/// doc comment for why this stays ASCII-only.
fn load_ascii_pairs() -> Vec<(usize, String, String)> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("repo root is 3 levels up from rust-competitors/")
        .join("benches/data/distance-pairs.json");
    let body = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "cannot read {}: {e}\nGenerate it with: python3 tools/bench-data/generate.py",
            path.display()
        )
    });
    let json: serde_json::Value = serde_json::from_str(&body).expect("valid bench data");
    let mut v: Vec<(usize, String, String)> = json["pairs"]["ascii"]
        .as_object()
        .expect("pair map")
        .iter()
        .map(|(k, pair)| {
            (
                k.parse().expect("numeric key"),
                pair[0].as_str().unwrap().to_owned(),
                pair[1].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    v.sort_by_key(|(n, _, _)| *n);
    v
}

#[test]
fn levenshtein_agrees_across_every_new_competitor_on_ascii_pairs() {
    let pairs = load_ascii_pairs();
    assert!(
        pairs.len() >= 5,
        "expected the full 4..1024 size sweep, got {}",
        pairs.len()
    );
    let opts = LevOptions::default();

    for (n, a, b) in &pairs {
        let want = levenshtein(a, b, &opts) as u32;

        assert_eq!(
            stringmetrics::levenshtein(a, b),
            want,
            "stringmetrics::levenshtein diverges from verbora at n={n}"
        );
        assert_eq!(
            triple_accel::levenshtein(a.as_bytes(), b.as_bytes()),
            want,
            "triple_accel::levenshtein diverges from verbora at n={n}"
        );
        assert_eq!(
            editdistancek::edit_distance(a.as_bytes(), b.as_bytes()) as u32,
            want,
            "editdistancek::edit_distance diverges from verbora at n={n}"
        );
    }
}

#[test]
fn hamming_agrees_across_every_new_competitor_on_ascii_pairs() {
    let pairs = load_ascii_pairs();

    for (n, a, b) in &pairs {
        let want = hamming(a, b, false);
        assert!(
            want >= 0,
            "shared ascii pairs are same-length by construction, got INCOMPARABLE at n={n}"
        );
        let want = want as u32;

        assert_eq!(
            stringmetrics::hamming(a, b).expect("same-length pair"),
            want,
            "stringmetrics::hamming diverges from verbora at n={n}"
        );
        assert_eq!(
            triple_accel::hamming(a.as_bytes(), b.as_bytes()),
            want,
            "triple_accel::hamming diverges from verbora at n={n}"
        );
    }
}

#[test]
fn restricted_damerau_levenshtein_agrees_with_triple_accel_rdamerau() {
    let pairs = load_ascii_pairs();
    let opts = LevOptions {
        restricted: true,
        ..Default::default()
    };

    for (n, a, b) in &pairs {
        let want = damerau_levenshtein(a, b, &opts) as u32;
        assert_eq!(
            triple_accel::rdamerau(a.as_bytes(), b.as_bytes()),
            want,
            "triple_accel::rdamerau diverges from verbora's restricted/OSA mode at n={n}"
        );
    }
}

#[test]
fn eddie_jaro_and_jaro_winkler_agree_with_verbora_on_ascii_pairs() {
    let pairs = load_ascii_pairs();
    let ejaro = EddieJaro::new();
    let ejarwin = EddieJaroWinkler::new();
    let jw_opts = verbora_distance::jaro_winkler::Options::default();

    for (n, a, b) in &pairs {
        let v_jaro = jaro(a, b);
        let e_jaro = ejaro.similarity(a, b);
        assert!(
            (v_jaro - e_jaro).abs() < 1e-9,
            "jaro diverges at n={n}: verbora={v_jaro} eddie={e_jaro}"
        );

        let v_jw = jaro_winkler(a, b, &jw_opts);
        let e_jw = ejarwin.similarity(a, b);
        assert!(
            (v_jw - e_jw).abs() < 1e-9,
            "jaro_winkler diverges at n={n}: verbora={v_jw} eddie={e_jw}"
        );
    }
}

/// The matrix's own "crate abandoned since 2020 — re-verify against fresh
/// vectors before trusting" flag, made concrete with hand-picked edge cases
/// distinct from the random-ASCII corpus above: the textbook Wikipedia
/// example, a transposition, disjoint alphabets, and short strings — plus
/// the two real, documented divergences this re-verification pass actually
/// found (both operands empty; both operands the SAME single character).
#[test]
fn eddie_jaro_matches_verbora_on_edge_cases_except_the_documented_divergences() {
    let ejaro = EddieJaro::new();
    let ejarwin = EddieJaroWinkler::new();
    let jw_opts = verbora_distance::jaro_winkler::Options::default();

    let cases = [
        ("martha", "marhta"),
        ("dixon", "dicksonx"),
        ("jellyfish", "smellyfish"),
        ("abc", "abc"),
        ("abc", "xyz"),
        ("a", "b"),
        ("ab", "a"),
        ("ab", "ab"),
    ];
    for (a, b) in cases {
        let v_jaro = jaro(a, b);
        let e_jaro = ejaro.similarity(a, b);
        assert!(
            (v_jaro - e_jaro).abs() < 1e-9,
            "jaro diverges on {a:?}/{b:?}: verbora={v_jaro} eddie={e_jaro}"
        );

        let v_jw = jaro_winkler(a, b, &jw_opts);
        let e_jw = ejarwin.similarity(a, b);
        assert!(
            (v_jw - e_jw).abs() < 1e-9,
            "jaro_winkler diverges on {a:?}/{b:?}: verbora={v_jw} eddie={e_jw}"
        );
    }

    // Documented divergence #1, not a bug in either side: eddie's own
    // `equality()` test asserts Jaro similarity of two empty slices is 1.0
    // (nothing to disagree on == maximally similar). Verbora's `jaro`
    // returns 0.0 whenever EITHER operand is empty, with no special case
    // for "both empty" (crates/verbora-distance/src/jaro_winkler.rs's
    // `if len1 == 0 || len2 == 0 { return 0.0; }`). The textbook Jaro
    // formula divides by `len1 * len2`-derived terms and is undefined at
    // 0/0 — both choices are a deliberate convention, not a correctness
    // bug, but they are DIFFERENT conventions.
    assert_eq!(jaro("", ""), 0.0, "verbora: both-empty jaro is 0.0");
    assert_eq!(
        ejaro.similarity("", ""),
        1.0,
        "eddie: both-empty jaro is 1.0 — see this test's own doc comment"
    );

    // Documented divergence #2, found BY this re-verification pass (not
    // previously called out in docs/COMPETITIVE_BENCHMARKS.md): two
    // IDENTICAL single characters. Verbora's `match_window` formula is
    // `(len1.max(len2) as isize) / 2 - 1`; for two length-1 strings that is
    // `1 / 2 - 1 == -1`, which makes the match window empty and Jaro falls
    // through to its `m == 0` branch, returning 0.0 even though the
    // characters are equal — a quirk `crates/verbora-distance/src/jaro_winkler.rs`'s
    // own `single_char_ignore_case_exposes_the_prefix_quirk` test already
    // documents as "verified against the reference" (i.e. this is a faithful
    // port of the reference's own behavior, not a Verbora bug). eddie has no
    // such special case and returns the textbook 1.0 for equal single
    // characters (its own `equality()` test: `(1., vec![1])`). Neither of
    // the two divergences on this pair is reachable by
    // `benches/distance.rs`: the shared `distance-pairs.json` corpus's
    // shortest strings are 4 characters, and no pair is ever both-empty or
    // both-length-1.
    assert_eq!(
        jaro("a", "a"),
        0.0,
        "verbora: equal single-char jaro is 0.0"
    );
    assert_eq!(
        ejaro.similarity("a", "a"),
        1.0,
        "eddie: equal single-char jaro is 1.0 — see this test's own doc comment"
    );
}

/// Fuzzy substring search: matrix `Selected cases`, "a different problem
/// shape" — Verbora returns one best match with a full backtrace,
/// triple_accel a bounded-`k` iterator over every match within that bound.
/// This test makes no output-equivalence claim; it verifies both sides run
/// to completion on the shared corpus without panicking, and that whatever
/// matches triple_accel does report land inside the haystack — the honest
/// floor of "correct" for a matrix row never marked `Yes`.
#[test]
fn fuzzy_substring_search_runs_without_panicking_on_both_sides() {
    let pairs = load_ascii_pairs();
    let opts = LevOptions::default();

    for (n, a, b) in &pairs {
        // `a` is the needle/pattern, `b` the haystack/target — same
        // convention `crates/verbora-distance/benches/distance.rs`'s own
        // `search_matrix` group and `benches/distance.rs`'s new
        // `fuzzy_substring_search` group both use.
        let v = levenshtein_search(a, b, &opts);
        assert!(
            v.distance >= 0.0,
            "verbora search returned a negative distance at n={n}"
        );
        assert!(
            v.offset >= -1,
            "verbora search returned an implausible offset at n={n}: {}",
            v.offset
        );

        // triple_accel's own doc comment: "Each returned Match requires at
        // least half or more bytes of the needle to match somewhere in the
        // haystack." For unrelated random-ASCII pairs (this corpus) that
        // bound is frequently unmet, so an empty result is expected and
        // correct here, not a test failure — only malformed offsets would
        // be.
        let matches: Vec<_> =
            triple_accel::levenshtein_search(a.as_bytes(), b.as_bytes()).collect();
        for m in &matches {
            assert!(
                m.start <= m.end && m.end <= b.len(),
                "triple_accel match out of bounds at n={n}: {m:?}"
            );
        }
    }
}
