//! `CORRECTNESS BEFORE PERFORMANCE` for `benches/double_metaphone_cpp.rs`.
//!
//! **Result: `Partial`, not byte-exact — 584/653 (89.4%) of real English
//! surnames agree exactly.** Both sides implement Lawrence Philips' Double
//! Metaphone algorithm directly (Verbora's own transcription of the
//! reference it ports; the vendored library's own header cites Philips'
//! original C, modified by Kevin Atkinson), but the algorithm's own
//! published write-up is famously under-specified on several rules, and
//! these two independent implementations resolved at least one of them
//! differently — confirmed by reading both sides' source, not assumed from
//! the mismatch alone.
//!
//! # The confirmed, dominant divergence: the trailing-S-after-A/I rule
//!
//! `crates/verbora-phonetics/src/double_metaphone.rs`'s `handle_s`, in its
//! final `else if` branch, silences a trailing `S` whenever it is preceded
//! by `A` or `I`, full stop (see that function's own closing comment:
//! `"the S is silently dropped: ISLE encodes as AL"`) — so `Davis` loses
//! its `S` (`("TF", "TF")`) the same way `ISLE` does.
//!
//! The vendored library's own `case 'S':` branch (`double_metaphone.h`,
//! search `StringAt(current - 1, 3, {"ISL", "YSL"})`) silences a trailing
//! `S` **only** in the specific `ISL`/`YSL` pattern the comment names —
//! `island`, `isle`, `carlisle`, `carlysle` — i.e. `I` (or `Y`) immediately
//! followed by `S` immediately followed by `L`. `Davis` does not match that
//! narrower pattern, so the vendored library keeps the `S`:
//! `("TFS", "TFS")`.
//!
//! Verbora's rule is the broader of the two: every `[AI]S` at a word
//! boundary is silenced, not only the specific `island`-style `[IY]SL`
//! shape. This single confirmed rule difference accounts for the clear
//! majority of the 69 mismatches in this corpus (names ending directly in
//! `-as`/`-is`, e.g. `Davis`, `Thomas`, `Harris`, `Lewis`, `Morris`); a
//! smaller remainder involves other rules (`G`/`J` sibilant choices,
//! `SC`-cluster handling in longer names) not individually traced here —
//! this file pins the aggregate agreement rate as a real, reproducible
//! fact rather than claiming a fully exhaustive rule-by-rule account of
//! every divergent name.
//!
//! # What is still a fair, apples-to-apples timing comparison
//!
//! Neither implementation is "more correct" than the other — Double
//! Metaphone's own original write-up never fully disambiguates this case,
//! and both sides are legitimate, independently-arrived-at readings of it.
//! Per this workspace's own precedent for every other phonetic-encoder
//! competitor (`rphonetic`'s Metaphone/SoundEx/Daitch–Mokotoff rows, all
//! `Partial`, never `Yes`, in `docs/COMPETITIVE_BENCHMARKS.md` §1.6): a
//! `Partial` correctness verdict does not disqualify a throughput
//! comparison. Both sides do the same *shape* of work — one left-to-right
//! scan producing a (primary, secondary) key pair — and
//! `benches/double_metaphone_cpp.rs` benchmarks that, not byte-exact
//! output equivalence.
//!
//! The one *other* difference, confirmed not to matter on this domain:
//! Verbora's `process()` applies a default 32-character output cap
//! (`docs/COMPETITIVE_BENCHMARKS.md`'s own convention for this crate's
//! phonetic encoders), and the vendored library has no cap at all — this
//! corpus (real English surnames, 2-14 characters,
//! `benches/data/names.json`) never produces a key anywhere near 32
//! characters, so the cap never fires here; asserted below, not assumed.

use competitive_rust::double_metaphone_cpp::double_metaphone_cpp;
use verbora_phonetics::DoubleMetaphone;

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

/// Pins the real, measured agreement rate — 584/653 — as a regression
/// fact. This is deliberately not `assert_eq!` per name: per the module
/// doc comment, a `Partial` (not byte-exact) result is expected and
/// disclosed, not a bug to chase to zero. If this count ever changes, it
/// means one side's algorithm changed (or the shared name list did) and
/// the module doc comment's analysis needs revisiting, which is exactly
/// what a hard-pinned count is for.
#[test]
fn agreement_rate_matches_the_documented_partial_result() {
    let names = load_names();
    assert!(!names.is_empty(), "name list must not be empty");

    let dm = DoubleMetaphone::new();
    let mut agreements = 0usize;
    let mut longest_key = 0usize;

    for name in &names {
        let verbora = dm.process(name);
        let cpp = double_metaphone_cpp(name);
        if verbora == cpp {
            agreements += 1;
        }
        longest_key = longest_key.max(verbora.0.len()).max(verbora.1.len());
    }

    assert_eq!(
        agreements,
        584,
        "{agreements}/{} names agree, expected 584 -- see this file's module doc comment for the \
         confirmed root cause of the documented mismatches; if this number changed, re-verify \
         whether it's still explained by the same rule difference",
        names.len()
    );

    // Confirms Verbora's default 32-char cap never fires on this domain --
    // see the module doc comment. If this ever fails, the two sides may
    // start disagreeing on the longest names for a second, unrelated
    // reason, and the cap difference would need its own accounting rather
    // than being silently assumed away.
    assert!(
        longest_key < 32,
        "a key reached {longest_key} characters -- Verbora's default cap may now be affecting \
         results; re-examine whether this comparison is still apples-to-apples"
    );
}

/// Confirms the specific, source-cited root cause from the module doc
/// comment on a handful of representative names, so the explanation is
/// pinned by a test, not just prose that could drift from reality.
#[test]
fn davis_diverges_on_the_documented_trailing_s_rule() {
    let dm = DoubleMetaphone::new();
    // Verbora: S preceded by 'I', dropped (not followed by 'L', so not
    // pixelglow's narrower ISL/YSL pattern either).
    assert_eq!(dm.process("Davis"), ("TF".into(), "TF".into()));
    assert_eq!(double_metaphone_cpp("Davis"), ("TFS".into(), "TFS".into()));
}

#[test]
fn isle_agrees_on_both_sides_narrower_shared_pattern() {
    let dm = DoubleMetaphone::new();
    // The one case both rules agree should be silenced: 'I' immediately
    // before 'S' immediately before 'L', pixelglow's own worked example.
    let verbora = dm.process("Isle");
    let cpp = double_metaphone_cpp("Isle");
    assert_eq!(
        verbora, cpp,
        "both implementations should silence this specific ISL pattern"
    );
}
