//! Verbora vs. a vendored C++ Double Metaphone — correctness only.
//!
//! **There is no longer a companion benchmark.** This file used to be the
//! `CORRECTNESS BEFORE PERFORMANCE` precondition for
//! `benches/double_metaphone_cpp.rs`; that benchmark was deleted by the
//! Rust-native migration and this file is now the whole of the comparison.
//! The reason is in "Why the timing comparison was deleted" below, and it is
//! a fairness reason, not a compile error.
//!
//! The competitor is
//! [pixelglow/double_metaphone](https://github.com/pixelglow/double_metaphone),
//! a from-scratch C++11 transcription of Lawrence Philips' original
//! algorithm, vendored under `vendor/pixelglow-double_metaphone/` (see that
//! directory's own `README.md` for provenance and license) and compiled by
//! `build.rs` into a small `extern "C"` shim — the one non-Cargo, non-Rust,
//! compiled-from-source competitor in this workspace. It remains valuable as
//! an *independent* oracle: unlike rphonetic it shares no lineage with
//! Verbora's implementation at all.
//!
//! # Result: 651/653 (99.7%) agree, on the published 4-character key
//!
//! Verbora's contract
//! (`crates/verbora-phonetics/src/double_metaphone.rs`) states "**Both keys
//! are at most four characters**, per the algorithm," and enforces it with
//! `MAX_KEY_LEN = 4`. The vendored library implements no such cap and offers
//! no way to request one — `dm::double_metaphone` takes a string and nothing
//! else — so it returns the full untruncated key: `Anderson` is `ANTRSN`
//! where Verbora is `ANTR`.
//!
//! Every comparison below therefore truncates the C++ output to four
//! characters before comparing. That is a *normalization onto the published
//! algorithm*, not a thumb on the scale: Philips' key is four characters, both
//! implementations append strictly left to right, and a length cap on such an
//! algorithm is exactly a prefix truncation — which is also how rphonetic's
//! own `max_code_length` is implemented. Comparing `ANTR` against `ANTRSN`
//! would measure the presence of a truncation step, not a disagreement about
//! phonetics.
//!
//! With that normalization, the two implementations agree on **651 of the 653
//! names** in `benches/data/names.json`.
//!
//! # The two remaining divergences: `CH` after a consonant
//!
//! `Czech` and `Koch` are the entire remainder, and both are the same rule:
//!
//! | name | Verbora | pixelglow |
//! |---|---|---|
//! | `Czech` | `("SK", "XK")` | `("SX", "XK")` |
//! | `Koch`  | `("KK", "KK")` | `("KX", "KK")` |
//!
//! In both, the two sides agree on the *alternate* key and differ on the
//! primary's treatment of a `CH` that follows a consonant: Verbora codes it
//! `K`, the vendored library codes it `X`. Both readings appear in the
//! published algorithm's own descendants — the `CH`-after-consonant case is
//! among the rules Philips' write-up leaves under-specified — so neither side
//! is "more correct" and neither is treated as an oracle for the other.
//! Verbora's behaviour here is pinned by its own crate tests against its own
//! contract; this file records the disagreement rather than resolving it.
//!
//! # What the migration changed
//!
//! This file previously recorded **584/653 (89.4%)** agreement, dominated by
//! a trailing-`S`-after-`A`/`I` rule: the old implementation silenced a
//! trailing `S` after any `A` or `I`, so `Davis` encoded `("TF", "TF")`,
//! while the vendored library silences it only in the narrow `ISL`/`YSL`
//! pattern its comment names (`island`, `isle`, `Carlisle`). Verbora's
//! `handle_s` now implements that same narrow `ISL`/`YSL` rule
//! (`crates/verbora-phonetics/src/double_metaphone.rs`), so `Davis` is
//! `("TFS", "TFS")` on both sides and that entire divergence family is gone.
//! The old 584 and the new 651 are therefore not comparable as a
//! "regression/improvement" — the key length being compared changed at the
//! same time. Both numbers come from running this file, neither is estimated.
//!
//! # Why the timing comparison was deleted
//!
//! `MAX_KEY_LEN` is not only an output cap: Verbora's encoder loop also
//! *stops* once both keys are full, so on any name whose key exceeds four
//! characters Verbora scans less of the input than the uncapped C++ does.
//! Neither side can be reconfigured to remove the asymmetry — Verbora's cap
//! is a contract rather than a parameter, and the vendored library takes no
//! length argument. Truncating the C++ *output* afterwards, as this file
//! does, fixes the comparison of answers but not the comparison of work: the
//! C++ would still have scanned the whole word.
//!
//! That is a systematic advantage to Verbora on a large fraction of the
//! corpus, which `AGENTS.md`'s "Cross-Implementation Benchmark Fairness"
//! forbids publishing ("Do not game benchmarks by excluding real costs from
//! only one implementation"), and it is the same trap the deleted
//! `dm_soundex` group's own doc comment named — comparing against a method
//! that does "a materially larger amount of work". Since no fair workload
//! exists for these two implementations, the timing group is deleted rather
//! than disclosed-and-published.
//!
//! **Coverage lost:** the Double Metaphone throughput row against a
//! non-Rust, independently-written implementation. Verbora's Double Metaphone
//! is still timed against rphonetic in `benches/phonetics.rs`, where the
//! competitor *is* configurable (`RDoubleMetaphone::new(Some(4))`) and the
//! comparison stays fair. **Coverage kept:** everything in this file — the
//! independent-oracle correctness check is unaffected by the work asymmetry,
//! since it compares answers rather than time.

use competitive_rust::double_metaphone_cpp::double_metaphone_cpp;
use verbora_phonetics::{DoubleMetaphone, DoubleMetaphoneCode};

/// Verbora's `DoubleMetaphoneCode` expressed in the vendored library's own
/// shape: a (primary, secondary) pair of `&str`.
///
/// The two sides represent "this name has only one pronunciation"
/// differently, and this function is the whole of the translation between
/// them. `DoubleMetaphoneCode::alternate()` is `Option<&str>`, `None` exactly
/// when the encoder never forked or the alternate came out equal to the
/// primary (`crates/verbora-phonetics/src/double_metaphone.rs`); the vendored
/// C++ library always fills both output buffers, repeating the primary in
/// that case (`Davis` -> `("TFS", "TFS")`). Mapping `None` to the primary is
/// therefore lossless in both directions, not a fudge to force agreement:
/// `alternate_is_absent_exactly_when_the_cpp_side_repeats_the_primary` below
/// asserts the equivalence over the whole corpus rather than assuming it.
///
/// Borrows throughout — nothing here allocates to make the shapes line up.
fn keys(code: &DoubleMetaphoneCode) -> (&str, &str) {
    let primary = code.primary();
    (primary, code.alternate().unwrap_or(primary))
}

fn load_names() -> Vec<String> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("repo root is 3 levels up from rust-competitors/")
        .join("benches/data/names.json");
    let body = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "cannot read {}: {e}\nGenerate it with: python3 tools/bench-data/generate.py",
            path.display()
        )
    });
    let json: serde_json::Value = serde_json::from_str(&body).expect("valid bench data");
    json["names"]
        .as_array()
        .expect("name list")
        .iter()
        .map(|n| n.as_str().expect("name").to_owned())
        .collect()
}

/// The C++ side's keys, truncated to the four characters Philips' published
/// algorithm defines and Verbora's contract enforces.
///
/// See this file's module doc comment for why this normalization is a
/// restatement of the published key length rather than a concession: the
/// vendored library implements no cap and exposes no way to ask for one, and
/// both implementations append strictly left to right, so a cap on either is
/// exactly a prefix truncation.
fn cpp_keys_truncated(word: &str) -> (String, String) {
    let (primary, secondary) = double_metaphone_cpp(word);
    (
        primary.chars().take(4).collect(),
        secondary.chars().take(4).collect(),
    )
}

/// Pins the real, measured agreement rate — 651/653 — as a regression fact.
/// This is deliberately not `assert_eq!` per name: per the module doc
/// comment, two names (`Czech`, `Koch`) diverge on a genuinely
/// under-specified rule, and that is disclosed rather than chased to zero. If
/// this count ever changes, one side's algorithm changed (or the shared name
/// list did) and the module doc comment's analysis needs revisiting, which is
/// exactly what a hard-pinned count is for.
#[test]
fn agreement_rate_matches_the_documented_result() {
    let names = load_names();
    assert!(!names.is_empty(), "name list must not be empty");

    let dm = DoubleMetaphone::new();
    let mut agreements = 0usize;
    let mut longest_key = 0usize;

    for name in &names {
        let verbora = dm.process(name);
        let cpp = cpp_keys_truncated(name);
        let (v_primary, v_secondary) = keys(&verbora);
        if (v_primary, v_secondary) == (cpp.0.as_str(), cpp.1.as_str()) {
            agreements += 1;
        }
        longest_key = longest_key.max(v_primary.len()).max(v_secondary.len());
    }

    assert_eq!(
        agreements,
        651,
        "{agreements}/{} names agree, expected 651 -- see this file's module doc comment for the \
         two documented divergences; if this number changed, re-verify whether it is still \
         explained by the same CH-after-consonant rule",
        names.len()
    );

    // Verbora's documented four-character cap, observed rather than assumed.
    assert_eq!(
        longest_key, 4,
        "Verbora's contract caps both keys at 4 characters; a key of {longest_key} means the \
         contract moved and this file's truncation normalization needs rechecking"
    );
}

/// The divergence family this file used to document is **gone**. The old
/// implementation silenced a trailing `S` after any `A` or `I`, so `Davis`
/// encoded `("TF", "TF")` while the vendored library kept the `S`. Verbora's
/// `handle_s` now silences a trailing `S` only in the narrow `ISL`/`YSL`
/// pattern (`island`, `isle`, `Carlisle`) — the same rule the vendored
/// library's own `case 'S':` branch implements — so the two now agree.
///
/// Kept, inverted, at the same fixture rather than deleted: this test is what
/// would catch the broad rule coming back.
#[test]
fn davis_now_agrees_on_the_narrow_isl_trailing_s_rule() {
    let dm = DoubleMetaphone::new();

    let davis = dm.process("Davis");
    assert_eq!(davis.primary(), "TFS");
    // One pronunciation: Verbora reports that as `None` rather than as a
    // duplicated key.
    assert_eq!(davis.alternate(), None);
    assert_eq!(keys(&davis), ("TFS", "TFS"));
    assert_eq!(cpp_keys_truncated("Davis"), ("TFS".into(), "TFS".into()));
}

#[test]
fn isle_agrees_on_both_sides_narrower_shared_pattern() {
    let dm = DoubleMetaphone::new();
    // 'I' immediately before 'S' immediately before 'L' — pixelglow's own
    // worked example, and now Verbora's rule too.
    let verbora = dm.process("Isle");
    let cpp = cpp_keys_truncated("Isle");
    assert_eq!(
        keys(&verbora),
        (cpp.0.as_str(), cpp.1.as_str()),
        "both implementations should silence this specific ISL pattern"
    );
}

/// The two documented divergences, pinned individually so the module doc
/// comment's table cannot drift from reality. Both are the same rule — a `CH`
/// following a consonant — and in both the two sides agree on the *alternate*
/// key and differ only on the primary.
#[test]
fn the_two_documented_divergences_are_the_ch_after_consonant_rule() {
    let dm = DoubleMetaphone::new();

    for (name, v_expected, c_expected) in [
        ("Czech", ("SK", "XK"), ("SX", "XK")),
        ("Koch", ("KK", "KK"), ("KX", "KK")),
    ] {
        let verbora = dm.process(name);
        let cpp = cpp_keys_truncated(name);
        assert_eq!(keys(&verbora), v_expected, "verbora {name:?}");
        assert_eq!((cpp.0.as_str(), cpp.1.as_str()), c_expected, "cpp {name:?}");
        // The disagreement is confined to the primary.
        assert_eq!(
            keys(&verbora).1,
            cpp.1.as_str(),
            "{name:?}: the alternate keys are expected to still agree"
        );
    }
}

/// Pins the claim [`keys`] rests on: across the corpus, Verbora reports
/// `alternate() == None` on essentially exactly the names where the C++ side
/// writes the same key into both buffers. This is what makes the `None` →
/// primary mapping lossless rather than a convenience that could hide a real
/// disagreement about whether a name forks at all.
///
/// The sole exception is `Koch`, and it is not a representation problem: it is
/// one of the two `CH`-after-consonant divergences above, where the C++
/// primary (`KX`) differs from its own alternate (`KK`) while Verbora's
/// primary (`KK`) equals what its alternate would be. Disagreeing about the
/// letters of the key necessarily disagrees about whether the two keys are
/// equal, so this is the same single defect surfacing twice, not a second
/// one. It is asserted by name so the exception cannot silently widen.
#[test]
fn alternate_is_absent_exactly_when_the_cpp_side_repeats_the_primary() {
    let names = load_names();
    let dm = DoubleMetaphone::new();
    let mut exceptions = Vec::new();

    for name in &names {
        let verbora = dm.process(name);
        let cpp = cpp_keys_truncated(name);
        if verbora.alternate().is_none() != (cpp.0 == cpp.1) {
            exceptions.push(name.clone());
        }
    }

    assert_eq!(
        exceptions,
        vec!["Koch".to_string()],
        "the fork-agreement exception set changed; `keys`'s None-to-primary mapping may now be \
         hiding a real divergence"
    );
}
