//! `CORRECTNESS BEFORE PERFORMANCE` for `benches/phonetics.rs`.
//!
//! `docs/COMPETITIVE_BENCHMARKS.md` §1.6 marks every phonetic row `Partial`,
//! never `Yes`: Verbora's four encoders are transcriptions of the reference's
//! own variants (condense-before-drop Soundex, a Metaphone/Double Metaphone
//! with a documented `transformX`-after-`cTransform` ordering quirk plus a
//! family of the reference-truthiness accidents, a single-branch
//! Daitch-Mokotoff), while rphonetic implements the textbook / Apache
//! commons-codec originals. Byte-exact output equality between the two
//! sides is explicitly **not** the claim `benches/phonetics.rs` makes (see
//! its own doc comment) — so, unlike `tests/trie_correctness.rs` (whose
//! matrix rows really do promise the same answer, just not the same
//! order), this file does not assert it.
//!
//! What *is* checked here, once and outside the timed code, before any
//! timing number from that bench file is trusted:
//!
//! 1. Every name in the shared dataset round-trips through all four
//!    encoders on **both** sides without panicking. This is not a
//!    formality — rphonetic's `Soundex::encode` indexes a fixed 26-entry
//!    table with `self.mapping[ch as usize - 65]` and no bounds check, so a
//!    non-ASCII letter is a genuine out-of-bounds panic there, not merely
//!    an unfair comparison. It is exactly why
//!    `tools/bench-data/generate.py`'s `names.json` is ASCII-only, and this
//!    test is what actually proves that restriction is sufficient rather
//!    than merely assumed.
//! 2. The `Some(32)` reconfiguration `docs/COMPETITIVE_BENCHMARKS.md`'s
//!    Notes column requires for rphonetic's Metaphone/Double Metaphone
//!    genuinely takes effect — i.e. the bench is not silently doing less
//!    work at the crate's own default of 4. Verified against the same long
//!    input `crates/verbora-phonetics/src/metaphone.rs`'s own test suite
//!    uses to pin Verbora's default of 32
//!    (`m.process(&"ab".repeat(300)).len() == 32`), and cross-checked
//!    against rphonetic's *un*-reconfigured `Metaphone::default()` to prove
//!    the two really do differ.
//! 3. rphonetic's `DaitchMokotoffSoundex::encode()` — the single-branch
//!    method, never `.soundex()` — produces the same **shape** of output as
//!    Verbora's `SoundExDM::process`: a fixed 6-digit string. This is the
//!    matrix's own claim about why `encode()` (not `soundex()`, which can
//!    return up to 8 pipe-separated codes) is the fair match for Verbora's
//!    single-`String` return type, made concrete as a test rather than left
//!    as a doc-comment assertion.

use rphonetic::{
    DaitchMokotoffSoundex, DoubleMetaphone as RDoubleMetaphone, Encoder, Metaphone as RMetaphone,
    Soundex as RSoundex,
};
use verbora_phonetics::{DoubleMetaphone, Metaphone, SoundEx, SoundExDM};

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
        .expect("names array")
        .iter()
        .map(|w| w.as_str().expect("name is a string").to_owned())
        .collect()
}

#[test]
fn every_name_round_trips_without_panicking_on_both_sides() {
    let names = load_names();
    assert!(
        names.len() > 500,
        "expected a substantial multilingual name list (see tools/bench-data/generate.py), got {}",
        names.len()
    );

    let v_soundex = SoundEx::new();
    let v_metaphone = Metaphone::new();
    let v_double = DoubleMetaphone::new();
    let v_dm = SoundExDM::new();

    let r_soundex = RSoundex::default();
    let r_metaphone = RMetaphone::new(Some(32));
    let r_double = RDoubleMetaphone::new(Some(32));
    let r_dm = DaitchMokotoffSoundex::default();

    for name in &names {
        // --- Verbora side: never panics, always produces real output. ---
        assert!(
            !v_soundex.process(name).is_empty(),
            "verbora soundex empty for {name:?}"
        );
        assert!(
            !v_metaphone.process(name).is_empty(),
            "verbora metaphone empty for {name:?}"
        );
        let (v_primary, _) = v_double.process(name);
        assert!(
            !v_primary.is_empty(),
            "verbora double_metaphone empty for {name:?}"
        );
        assert_eq!(
            v_dm.process(name).len(),
            6,
            "verbora dm_soundex not 6 digits for {name:?}"
        );

        // --- rphonetic side: same crash-safety and shape claim. ---
        assert_eq!(
            r_soundex.encode(name).len(),
            4,
            "rphonetic soundex not 4 chars for {name:?}"
        );
        assert!(
            r_metaphone.encode(name).len() <= 32,
            "rphonetic metaphone exceeded the configured max_code_length for {name:?}"
        );
        let dm_result = r_double.double_metaphone(name);
        assert!(
            dm_result.primary().len() <= 32,
            "rphonetic double_metaphone primary exceeded the configured max_code_length for {name:?}"
        );
        let dm_code = r_dm.encode(name);
        assert_eq!(
            dm_code.len(),
            6,
            "rphonetic dm_soundex (Encoder::encode, single-branch) not 6 digits for {name:?}: {dm_code:?}"
        );
        assert!(
            dm_code.chars().all(|c| c.is_ascii_digit()),
            "rphonetic dm_soundex not all digits for {name:?}: {dm_code:?}"
        );
    }
}

#[test]
fn rphonetic_metaphone_max_code_length_is_genuinely_32_not_the_crate_default_of_4() {
    // Same input `crates/verbora-phonetics/src/metaphone.rs`'s own test
    // suite uses to pin Verbora's documented default of 32
    // (`process(&"ab".repeat(300)).len() == 32`) — reused here rather than
    // invented, so this test fails loudly if that documented default ever
    // moves without this file being updated to match.
    let long = "ab".repeat(300);

    let verbora_len = Metaphone::new().process(&long).len();
    assert_eq!(
        verbora_len, 32,
        "Verbora's own documented Metaphone default (32) changed; \
         update this test and the bench's max_code_length alongside it"
    );

    let reconfigured_len = RMetaphone::new(Some(32)).encode(&long).len();
    let unreconfigured_len = RMetaphone::default().encode(&long).len();

    assert!(
        reconfigured_len <= 32,
        "rphonetic exceeded its own configured max_code_length of 32"
    );
    assert!(
        reconfigured_len > unreconfigured_len,
        "Some(32) produced the same length as rphonetic's own un-reconfigured \
         default (max_code_length=4) -- the bench would be doing strictly \
         less work than Verbora, exactly what docs/COMPETITIVE_BENCHMARKS.md's \
         Notes column warns against"
    );
}

#[test]
fn rphonetic_double_metaphone_max_code_length_is_genuinely_32() {
    let long = "ab".repeat(300);

    let verbora_len = Metaphone::new().process(&long).len(); // same pipeline shape, sanity anchor
    assert_eq!(verbora_len, 32);

    let reconfigured = RDoubleMetaphone::new(Some(32)).double_metaphone(&long);
    let unreconfigured = RDoubleMetaphone::default().double_metaphone(&long);

    assert!(reconfigured.primary().len() <= 32);
    assert!(reconfigured.primary().len() > unreconfigured.primary().len());
}
