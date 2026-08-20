//! `CORRECTNESS BEFORE PERFORMANCE` for `benches/phonetics.rs`.
//!
//! That bench file's rows make two different claims, so this file has two
//! regimes:
//!
//! # Regime 1 — the three reference-ported encoders: shape parity only
//!
//! `docs/COMPETITIVE_BENCHMARKS.md` §1.6 marks the Soundex / Metaphone /
//! Double Metaphone rows `Partial`, never `Yes`: those three Verbora
//! encoders are transcriptions of their own variants (condense-before-drop
//! Soundex, a Metaphone/Double Metaphone with a documented
//! `transformX`-after-`cTransform` ordering quirk), while rphonetic
//! implements the textbook / Apache
//! commons-codec originals. Byte-exact output equality between the two
//! sides is explicitly **not** the claim `benches/phonetics.rs` makes for
//! those groups (see its own doc comment) — so, unlike
//! `tests/trie_correctness.rs` (whose matrix rows really do promise the
//! same answer, just not the same order), this file does not assert it for
//! them.
//!
//! What *is* checked for that regime, once and outside the timed code,
//! before any timing number from that bench file is trusted:
//!
//! 1. Every name in the shared dataset round-trips through all three
//!    encoders on **both** sides without panicking. This is not a
//!    formality — rphonetic's `Soundex::encode` indexes a fixed 26-entry
//!    table with `self.mapping[ch as usize - 65]` and no bounds check, so a
//!    non-ASCII letter is a genuine out-of-bounds panic there, not merely
//!    an unfair comparison. It is exactly why
//!    `tools/bench-data/generate.py`'s `names.json` is ASCII-only, and this
//!    test is what actually proves that restriction is sufficient rather
//!    than merely assumed.
//! 2. Each rphonetic encoder's `max_code_length` genuinely matches its
//!    Verbora counterpart's output-length contract. The Rust-native
//!    migration moved Verbora's two contracts in *opposite* directions, so
//!    a single shared setting is no longer correct for both and each is
//!    asserted separately against Verbora's live behaviour:
//!    `Metaphone` is now documented as never truncating, so rphonetic is
//!    built with `new(None)`; `DoubleMetaphone` is now documented as capping
//!    both keys at four characters, so rphonetic is built with `Some(4)`.
//!    Both previously used `Some(32)`. Each check is cross-referenced
//!    against rphonetic's *un*-reconfigured default to prove the setting has
//!    an observable effect rather than silently no-op'ing.
//! 3. ~~rphonetic's `DaitchMokotoffSoundex::encode()` shape claim.~~
//!    **Removed by the Rust-native migration.** This claim paired
//!    rphonetic's single-branch `encode()` against `SoundExDM::process`,
//!    Verbora's own single-branch Daitch-Mokotoff variant. `SoundExDM`
//!    existed only to reproduce the JS reference's single-branch shape — the
//!    exact kind of external-compatibility-only surface
//!    `docs/design/rust-native-migration.md` § "Per-crate scope" item 3
//!    directs crates to drop — and it is gone. Verbora ships one
//!    Daitch-Mokotoff encoder now, the branching [`DaitchMokotoff`], so
//!    there is no Verbora side of this shape comparison left.
//!
//!    **Nothing is actually uncovered by the removal.** rphonetic's
//!    single-branch `encode()` is still pinned, and pinned *harder*, in
//!    regime 2 below: `daitch_mokotoff_branching_agrees_byte_for_byte_with_
//!    rphonetic_soundex` asserts `DaitchMokotoff::codes()[0] == encode()`
//!    byte for byte over the whole corpus, which subsumes the 6-ASCII-digit
//!    shape assertion this item used to make. What is genuinely lost is the
//!    crash-safety and shape coverage for a *Verbora* single-branch D-M
//!    encoder — because there no longer is one.
//!
//! # Regime 2 — the seven spec-pinned extensions: byte-exact on the
//! benchmarked domain
//!
//! Cologne, NYSIIS, Caverphone 1.0/2.0, Phonex, Refined Soundex, Match
//! Rating Approach, and the branching Daitch-Mokotoff are **Verbora-native
//! extensions**. Each used to pin its behaviour to **rphonetic 3.0.6
//! itself**, byte for byte on rphonetic's whole accepted input domain; the
//! Rust-native migration re-pinned each to the algorithm's own publication
//! instead, per `docs/design/rust-native-migration.md`'s rule that "a
//! competitor never defines correctness".
//!
//! Byte-exact agreement with rphonetic survives as the claim over the
//! **benchmarked domain** — the ASCII corpus `benches/phonetics.rs` actually
//! times, where the two still agree everywhere — and this file asserts it
//! there, one test per encoder, in the same verbora-vs-rphonetic pairing that
//! bench uses. The ten inputs where the publications and rphonetic now
//! disagree are enumerated and pinned separately in **regime 3** at the end
//! of this file:
//!
//! * `Cologne::process` ↔ `rphonetic::Cologne::encode` (rphonetic's Cologne
//!   never panics; extras include umlauts, `ß`/`ẞ`, and the skipped-character
//!   lookahead/context quirks Verbora's module doc pins).
//! * `Nysiis::new()` (strict, the commons-codec default on both sides) ↔
//!   `rphonetic::Nysiis::default()`, and `with_strict(false)` ↔
//!   `Nysiis::new(false)` to prove the agreement is not a truncation
//!   coincidence.
//! * `Caverphone1`/`Caverphone2::process` ↔ the rphonetic unit structs.
//! * `Phonex::new()` (max code length 4, the default on both sides) ↔
//!   `rphonetic::Phonex::default()`.
//! * `RefinedSoundex::process` ↔ `rphonetic::RefinedSoundex::default()`
//!   (US-English mapping, the only one Verbora ships).
//! * `MatchRatingApproach::process` ↔ `encode`, **and** the real MRA match
//!   *decision*: `compare` ↔ `is_encoded_equals` over every ordered pair of
//!   corpus names — the decision, not mere code equality, is the published
//!   algorithm's point.
//! * `DaitchMokotoff::process` ↔ `soundex()` (pipe-joined branch codes —
//!   this pairing, unlike regime 1's, IS output-format identical), plus
//!   `codes()` ↔ `inner_soundex(_, true)` and `codes()[0]` ↔ the
//!   non-branching `encode()`.
//!
//! **Two kinds of exclusion, both narrow and both enumerated.** First,
//! inputs where rphonetic *panics* (byte-slice truncation inside a multi-byte
//! character, the 26-entry mapping table, an empty-encoding underflow in
//! MRA's rating pass) — all unreachable from the ASCII-only shared corpus,
//! and the targeted extras below deliberately stay inside rphonetic's
//! accepted domain. Second, the ten alphabet-filter divergences regime 3
//! lists by name. Nothing else is excluded. The four rphonetic quirks Verbora's `daitch_mokotoff` module doc
//! documents as *deliberately reproduced* (duplicate branch codes surviving
//! into the final list; the `ą`/`ę`/`ţ`/`ț` rules consuming the following
//! character; their vowel probe landing one character too far; case folding
//! to the *first* char of the Unicode lowercase form, so `İ` drops its
//! combining dot) are matches, not divergences — verified live against
//! rphonetic in `daitch_mokotoff_reproduces_rphonetics_documented_quirks`
//! rather than trusted from the doc comment.
//!
//! # Beyond the fixed corpus: compound, prefixed, and seeded-random inputs
//!
//! Three input families extend both regimes past `names.json`'s fixed list,
//! each staying strictly inside rphonetic's accepted domain (the exclusions
//! above are unchanged — none of these can reach a panic input):
//!
//! * **The compound corpus** — one full cycle of the exact
//!   hyphenated-consecutive-names derivation `benches/phonetics.rs`'s new
//!   `compound_10000` shape variant cycles (see `compound_corpus` below).
//!   This file proving it crash-safe (regime 1) and byte-exact (regime 2)
//!   is precisely what lets that bench shape claim `CORRECTNESS BEFORE
//!   PERFORMANCE`. In-domain because the corpus is ASCII and the `-` is
//!   non-alphabetic — filtered/skipped by every rphonetic encoder's own
//!   preprocessing before any fixed-table lookup.
//! * **Seeded pseudo-names** — a deterministic LCG sweep
//!   (`seeded_pseudo_names` below; same generator constants as
//!   `benches/classifiers.rs`'s own `Lcg`, this workspace's existing
//!   convention for reproducible random input). Each pseudo-name is ASCII
//!   letters with at most one internal `' '`/`'-'`/`'\''` separator and a
//!   letter at both ends — so every encoder's cleaned form is non-empty
//!   pure A–Z (no 26-entry-table overrun, no mid-character truncation, no
//!   empty-encoding MRA underflow), while still probing letter sequences no
//!   real-name list would.
//! * **Beider-Morse shape parity** — `benches/phonetics.rs`'s
//!   `beider_morse` group (shape-parity regime, like regime 1: rphonetic's
//!   `embedded_bm` `"any"`-rules-only default is documented there as never
//!   byte-comparable to Verbora's full-corpus encoder) previously had no
//!   correctness backing at all in this file.
//!   `beider_morse_shape_parity_and_crash_safety_on_all_bench_shapes` now
//!   proves both sides' crash-safety, output non-emptiness, and determinism
//!   over every input shape that bench times — plain, compound, and
//!   name-prefixed — without asserting cross-implementation byte equality,
//!   which remains explicitly not the claim.

use rphonetic::{
    Caverphone1 as RCaverphone1, Caverphone2 as RCaverphone2, Cologne as RCologne,
    DaitchMokotoffSoundex, DoubleMetaphone as RDoubleMetaphone, Encoder,
    MatchRatingApproach as RMatchRating, Metaphone as RMetaphone, Nysiis as RNysiis,
    Phonex as RPhonex, RefinedSoundex as RRefinedSoundex, Soundex as RSoundex,
};
use verbora_phonetics::{
    Caverphone1, Caverphone2, Cologne, DaitchMokotoff, DoubleMetaphone, MatchRatingApproach,
    Metaphone, Nysiis, Phonex, RefinedSoundex, SoundEx,
};

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

/// The shared corpus plus a test's own algorithm-targeted extras.
///
/// Every regime-2 test runs over the full `names.json` list (same file the
/// bench reads) and appends inputs chosen to hit its algorithm's own edge
/// rules. Extras must stay inside rphonetic's accepted (non-panicking)
/// input domain — each call site documents why its own do.
fn corpus_with(extras: &[&str]) -> Vec<String> {
    let mut names = load_names();
    assert!(
        names.len() > 500,
        "expected a substantial multilingual name list (see tools/bench-data/generate.py), got {}",
        names.len()
    );
    names.extend(extras.iter().map(|s| (*s).to_owned()));
    names
}

/// One full cycle of `benches/phonetics.rs`'s `compound_10000` shape —
/// consecutive corpus names joined with `-` (`"Smith-Johnson"`,
/// `"Johnson-Williams"`, …, wrapping at the end). Byte-for-byte the same
/// derivation that bench's `compound_names` cycles up to 10,000, so
/// verifying every distinct element here verifies every element the bench
/// times. See the module doc comment's "Beyond the fixed corpus" section for
/// why the hyphen stays inside rphonetic's accepted domain.
fn compound_corpus() -> Vec<String> {
    let names = load_names();
    names
        .iter()
        .zip(names.iter().cycle().skip(1))
        .map(|(a, b)| format!("{a}-{b}"))
        .collect()
}

/// Byte-for-byte identical to `benches/classifiers.rs`'s own `Lcg` — this
/// workspace's existing deterministic-PRNG convention, reused rather than
/// inventing a second generator (and keeping this crate free of a `rand`
/// dependency no bench needs).
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 11
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

/// `count` deterministic pseudo-names (fixed seed — every run sweeps the
/// identical list): one uppercase ASCII letter, 1–11 lowercase ASCII
/// letters, and for roughly one name in eight a single internal separator
/// drawn from `' '`/`'-'`/`'\''` (the three non-letter characters the real
/// corpus itself contains, never in first or last position).
///
/// Domain safety, per the module doc comment's "Beyond the fixed corpus"
/// section: all-ASCII (so no mid-character byte-slice truncation exists),
/// uppercasing lands in A–Z (no 26-entry-table overrun), and at least two
/// letters survive every encoder's cleaning pass (no empty-encoding MRA
/// rating underflow) — every documented rphonetic panic input stays
/// unreachable, exactly as the fixed corpus keeps it.
fn seeded_pseudo_names(count: usize) -> Vec<String> {
    let mut rng = Lcg(0x9E37_79B9_7F4A_7C15);
    (0..count)
        .map(|_| {
            let len = 2 + rng.below(11); // 2..=12 letters
            let mut name = String::with_capacity(len + 1);
            name.push((b'A' + rng.below(26) as u8) as char);
            for _ in 1..len {
                name.push((b'a' + rng.below(26) as u8) as char);
            }
            if len >= 4 && rng.below(8) == 0 {
                // All-ASCII, so this byte offset is always a char boundary;
                // 1..len-1 keeps a letter at both ends.
                let pos = 1 + rng.below(len - 2);
                let sep = [' ', '-', '\''][rng.below(3)];
                name.insert(pos, sep);
            }
            name
        })
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

    let r_soundex = RSoundex::default();
    let r_metaphone = RMetaphone::new(None);
    let r_double = RDoubleMetaphone::new(Some(4));

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
        assert!(
            !v_double.process(name).primary().is_empty(),
            "verbora double_metaphone empty for {name:?}"
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
    }
}

#[test]
fn regime1_round_trips_on_compound_and_seeded_pseudo_names() {
    // The same regime-1 claims as
    // `every_name_round_trips_without_panicking_on_both_sides`, extended to
    // the two input families the fixed corpus doesn't cover: every distinct
    // element of `benches/phonetics.rs`'s new `compound_10000` shape (this
    // is the crash-safety proof that bench shape's doc comment cites) and a
    // 1,000-name deterministic pseudo-name sweep. Same assertions, same
    // encoder constructions, same regime — shape and crash-safety only,
    // never cross-implementation byte equality.
    let mut inputs = compound_corpus();
    assert!(inputs.len() > 500, "compound corpus unexpectedly small");
    inputs.extend(seeded_pseudo_names(1_000));

    let v_soundex = SoundEx::new();
    let v_metaphone = Metaphone::new();
    let v_double = DoubleMetaphone::new();

    let r_soundex = RSoundex::default();
    let r_metaphone = RMetaphone::new(None);
    let r_double = RDoubleMetaphone::new(Some(4));

    for name in &inputs {
        assert!(
            !v_soundex.process(name).is_empty(),
            "verbora soundex empty for {name:?}"
        );
        assert!(
            !v_metaphone.process(name).is_empty(),
            "verbora metaphone empty for {name:?}"
        );
        assert!(
            !v_double.process(name).primary().is_empty(),
            "verbora double_metaphone empty for {name:?}"
        );

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
    }
}

#[test]
fn rphonetic_metaphone_is_uncapped_to_match_verboras_untruncated_key() {
    // Same input `crates/verbora-phonetics/src/metaphone.rs`'s own test suite
    // uses — reused rather than invented, so this test fails loudly if that
    // documented contract ever moves without this file being updated.
    //
    // The contract itself changed with the Rust-native migration. Verbora's
    // Metaphone used to default to a 32-character cap, and the bench matched
    // it with `RMetaphone::new(Some(32))`. `metaphone.rs` now states: "The key
    // is not truncated. `process` returns the complete key, however long the
    // word makes it." rphonetic's match for that is `new(None)` — its own doc
    // says `None` means "the resulting code can be of any length".
    let long = "ab".repeat(300);

    let verbora_len = Metaphone::new().process(&long).len();
    assert!(
        verbora_len > 32,
        "Verbora's Metaphone is documented as never truncating, but a \
         600-character input produced only {verbora_len} characters — if the \
         contract regained a cap, update this test and the bench's \
         max_code_length alongside it"
    );

    let uncapped_len = RMetaphone::new(None).encode(&long).len();
    let default_len = RMetaphone::default().encode(&long).len();

    assert_eq!(
        default_len, 4,
        "rphonetic's Metaphone default is documented as a 4-character cap"
    );
    assert!(
        uncapped_len > default_len,
        "None produced the same length as rphonetic's own capped default — \
         the bench would be truncating rphonetic where Verbora does not"
    );
    // Both sides run the whole word rather than stopping early. The lengths
    // need not be equal (the two implementations resolve several rules
    // differently — that is regime 1's whole premise), only uncapped.
    assert!(
        uncapped_len > 32,
        "rphonetic with None still capped at {uncapped_len} characters"
    );
}

#[test]
fn rphonetic_double_metaphone_is_capped_at_four_to_match_verbora() {
    // The opposite direction from Metaphone above, and the reason the two
    // rphonetic encoders are now configured differently.
    // `crates/verbora-phonetics/src/double_metaphone.rs` states "**Both keys
    // are at most four characters**, per the algorithm" and enforces it with
    // `MAX_KEY_LEN = 4`, which also ends the scan early once both keys fill.
    // Pre-migration this encoder shared Metaphone's 32-character cap and the
    // bench used `Some(32)`; leaving that would now have rphonetic doing far
    // more work than Verbora on every name longer than four key characters.
    let long = "ab".repeat(300);
    let dm = DoubleMetaphone::new();

    let code = dm.process(&long);
    assert!(
        code.primary().len() <= 4,
        "Verbora's Double Metaphone is documented as capping both keys at 4, \
         got a {}-character primary",
        code.primary().len()
    );
    assert!(code.alternate().is_none_or(|a| a.len() <= 4));

    let capped = RDoubleMetaphone::new(Some(4)).double_metaphone(&long);
    assert!(capped.primary().len() <= 4);
    assert!(capped.alternate().len() <= 4);

    // `Some(4)` is rphonetic's own default here, so the two constructions must
    // agree — this is what makes the bench's explicit `Some(4)` a statement of
    // intent rather than a reconfiguration.
    let default = RDoubleMetaphone::default().double_metaphone(&long);
    assert_eq!(capped.primary(), default.primary());
    assert_eq!(capped.alternate(), default.alternate());

    // And the cap is genuinely doing something: uncapped, rphonetic runs long.
    let uncapped = RDoubleMetaphone::new(None).double_metaphone(&long);
    assert!(
        uncapped.primary().len() > capped.primary().len(),
        "the 4-character cap had no observable effect, so this test would not \
         catch the bench silently reverting to an uncapped rphonetic"
    );
}

// ---------------------------------------------------------------------------
// Regime 2: byte-exact agreement for the seven rphonetic-pinned extensions.
// ---------------------------------------------------------------------------

#[test]
fn cologne_agrees_byte_for_byte_with_rphonetic() {
    // Extras: the umlaut/ß folding path, the `ẞ` (U+1E9E) asymmetry, and the
    // skipped-character lookahead/context quirks
    // `crates/verbora-phonetics/src/cologne.rs`'s module doc pins to
    // rphonetic ("p1h" — a skipped digit is still the peeked lookahead;
    // "c-x" — a skipped character does not update the C/X predecessor
    // context; "hc" — word-initial H leaves the output empty so C still
    // takes the initial branch; "Test test" — equal codes merge across
    // skipped characters). Further extras: common German surnames through
    // the SCH/C-context rules ("Fischer", "Schneider", "Weber"), the D/T
    // before-C/S/Z context ("dsz"), doubled X ("xx") and CK ("ck") clusters,
    // more umlaut/ß carriers ("Öztürk", "Deußen"), and the same
    // apostrophe/hyphen separator shapes the bench's compound variant now
    // times ("O'Neill", "Meyer-Lansky"). rphonetic's Cologne never panics,
    // so nothing here needs a domain restriction.
    let corpus = corpus_with(&[
        "Müller",
        "Müller-Lüdenscheidt",
        "Meyer",
        "Mayr",
        "Wikipedia",
        "Breschnew",
        "Aychlmajr",
        "Straße",
        "äöü",
        "Test test",
        "p1h",
        "c-x",
        "hc",
        "sas",
        "shs",
        "aha",
        "Fischer",
        "Schneider",
        "Weber",
        "Öztürk",
        "Deußen",
        "O'Neill",
        "Meyer-Lansky",
        "dsz",
        "xx",
        "ck",
    ]);

    let verbora = Cologne::new();
    let rphonetic = RCologne;

    for name in &corpus {
        assert_eq!(
            verbora.process(name),
            rphonetic.encode(name),
            "Cologne diverges from rphonetic for {name:?}"
        );
    }
}

#[test]
fn nysiis_agrees_byte_for_byte_with_rphonetic_in_both_strict_modes() {
    // Extras: every prefix/suffix rewrite family (`MAC`, `KN`, `SCH`,
    // `PH`, `EE`, `ND`), the retained-first-letter quirk ("AEIOU" → "A",
    // "Um" → "UN"), the cascading tail trims that can empty the key
    // ("AZ" → ""), and two safe non-ASCII shapes: "Straße" cleans to the
    // all-ASCII "STRASSE", and "日本語"'s strict cut falls exactly on a
    // character boundary (bytes 0..6 = two CJK chars) — the module doc's
    // one divergence (rphonetic panicking on a *mid*-character byte-6
    // slice, e.g. "BCDFGÉX") is deliberately NOT in this corpus because
    // rphonetic aborts on it; it is outside rphonetic's accepted domain.
    // Further extras (all ASCII, so the strict byte-6 cut is always on a
    // char boundary): more members of each rewrite family ("MacIntyre",
    // "Knowles", "Schaefer", "Pheasant"), the vowel-after-first-letter and
    // W/Y tail rules ("Yeager", "Vaughn", "Wray", "Micky"), and the same
    // hyphen/apostrophe separator shapes the bench's compound variant now
    // times ("Smith-Johnson", "O'Malley").
    let corpus = corpus_with(&[
        "MacDonald",
        "Knight",
        "Schmidt",
        "Phillips",
        "Westerlund",
        "Trueman",
        "O'Brien",
        "brown sr",
        "AEIOU",
        "Um",
        "AZ",
        "MacIntyre",
        "Knowles",
        "Schaefer",
        "Pheasant",
        "Yeager",
        "Vaughn",
        "Wray",
        "Micky",
        "Smith-Johnson",
        "O'Malley",
    ]);

    // Strict (6-character cap) is the commons-codec default on BOTH sides:
    // Verbora's `Nysiis::new()` and rphonetic's `Nysiis::default()`. The
    // bench times exactly this pairing.
    let v_strict = Nysiis::new();
    let r_strict = RNysiis::default();
    // Non-strict cross-check: proves the strict agreement is real rule
    // agreement, not two implementations happening to truncate the same way.
    let v_full = Nysiis::with_strict(false);
    let r_full = RNysiis::new(false);

    for name in &corpus {
        assert_eq!(
            v_strict.process(name),
            r_strict.encode(name),
            "strict NYSIIS diverges from rphonetic for {name:?}"
        );
        assert_eq!(
            v_full.process(name),
            r_full.encode(name),
            "non-strict NYSIIS diverges from rphonetic for {name:?}"
        );
    }
}

#[test]
fn caverphone1_agrees_byte_for_byte_with_rphonetic() {
    // Extras: Hood's own 2002 paper fixtures ("Thompson", "Lee", "David",
    // "Whittle", the "Peter"/"Peady" collision pair), letter-free input
    // (falls through the cascade to the all-`1` code on both sides), and
    // "café" — the surviving-multi-byte-character case the module doc
    // documents as byte-identical (`é` lands inside the 6-byte code without
    // straddling its end; the straddling shapes where rphonetic panics are
    // outside its accepted domain and hence not here). Further extras: the
    // initial-cluster rewrites 1.0 spells out ("cough" → cou2f, "rough" →
    // rou2f, "gn" → 2n, "kn"/"mb" tail handling via "knife"/"thumb"), an
    // all-vowel run ("aaaa"), and the hyphen separator shape the bench's
    // compound variant now times ("Stevenson-Moore").
    let corpus = corpus_with(&[
        "Thompson",
        "Lee",
        "ready",
        "David",
        "Whittle",
        "Peter",
        "Peady",
        "Stevenson",
        "123",
        "!!!",
        "cough",
        "rough",
        "gnat",
        "knife",
        "thumb",
        "aaaa",
        "Stevenson-Moore",
    ]);

    let verbora = Caverphone1::new();
    let rphonetic = RCaverphone1;

    for name in &corpus {
        assert_eq!(
            verbora.process(name),
            rphonetic.encode(name),
            "Caverphone 1.0 diverges from rphonetic for {name:?}"
        );
    }
}

#[test]
fn caverphone2_agrees_byte_for_byte_with_rphonetic() {
    // Extras: the 2004 revision's own behavioural deltas vs 1.0 — trailing
    // vowel kept as `A` ("ready", "Peter"), final-`e` drop ("enough"), the
    // ten-byte pad — plus the same safe non-ASCII survivor and letter-free
    // shapes as the 1.0 test.
    let corpus = corpus_with(&[
        "Thompson",
        "ready",
        "Lee",
        "Stevenson",
        "Peter",
        "Whittle",
        "enough",
        "trough",
        "123",
        "!!!",
    ]);

    let verbora = Caverphone2::new();
    let rphonetic = RCaverphone2;

    for name in &corpus {
        assert_eq!(
            verbora.process(name),
            rphonetic.encode(name),
            "Caverphone 2.0 diverges from rphonetic for {name:?}"
        );
    }
}

#[test]
fn phonex_agrees_byte_for_byte_with_rphonetic() {
    // Extras: every preprocessing family (trailing-S removal — "Jones",
    // "SSS"; leading-pair rewrites — "KNUTH", "Phillips", "Wright"; the
    // remove-only-one-leading-H rule — "HHART", "HARRINGTON") and the
    // pinned rphonetic quirks Verbora's module doc reproduces:
    // "Czarkowska" (duplicate suppression resetting to the head letter),
    // "Ng"/"Na" (the M/N + D/G swallow emitting a first-position digit),
    // and "ß"/"Straße" (case expansion to SS *before* trailing-S removal,
    // so "Straße" ≡ "Strasse" and bare "ß" encodes to "0000"). Phonex on
    // both sides accepts any &str — no domain restriction needed.
    let corpus = corpus_with(&[
        "KNUTH",
        "Wright",
        "Phillips",
        "Jones",
        "SSS",
        "HHART",
        "HARRINGTON",
        "Czarkowska",
        "Ng",
        "Na",
        "Strasse",
    ]);

    // Max code length 4 is the default on BOTH sides: Verbora's
    // `Phonex::new()` and rphonetic's `Phonex::default()`. The bench times
    // exactly this pairing.
    let verbora = Phonex::new();
    let rphonetic = RPhonex::default();

    for name in &corpus {
        assert_eq!(
            verbora.process(name),
            rphonetic.encode(name),
            "Phonex diverges from rphonetic for {name:?}"
        );
    }
}

#[test]
fn refined_soundex_agrees_byte_for_byte_with_rphonetic() {
    // Extras: commons-codec's own fixtures ("jumped", "testing"), the
    // vowel-separates-consonant-runs rule ("bab" → "B101"), and the
    // uppercase expansions that stay inside A–Z, which the module doc
    // documents as byte-identical: "ß" → SS → "S3", the ﬁ ligature → FI,
    // long s ſ → S. Alphabetic characters whose uppercase leaves A–Z
    // (é, Cyrillic, CJK, …) are rphonetic's out-of-bounds panic — the
    // module doc's one divergence — and are therefore outside the shared
    // domain, exactly like the classic-Soundex restriction regime 1 tests.
    let corpus = corpus_with(&[
        "jumped", "testing", "Smith", "Smythe", "bab", "Braz", "Broz", "Caren", "Corwin",
    ]);

    // US-English mapping — rphonetic's `default()`, the only mapping
    // Verbora ships.
    let verbora = RefinedSoundex::new();
    let rphonetic = RRefinedSoundex::default();

    for name in &corpus {
        assert_eq!(
            verbora.process(name),
            rphonetic.encode(name),
            "Refined Soundex diverges from rphonetic for {name:?}"
        );
    }
}

#[test]
fn match_rating_encoding_agrees_byte_for_byte_with_rphonetic() {
    // Extras: the punctuation-stripping rule ("O'Brien",
    // "McDonald-Smith"), digits surviving into codes ("1234567" →
    // "123567"), the first3+last3 truncation, and two safe non-ASCII
    // shapes the module doc documents as byte-identical: "Straße" (ß
    // uppercases to SS, all-ASCII from then on) and "日本語" (the 3 and
    // len−3 truncation offsets both land on character boundaries → "日語").
    // The mid-character truncation shapes where rphonetic panics (e.g.
    // "Москва") are outside its accepted domain and hence not here.
    let corpus = corpus_with(&[
        "Smith",
        "Smyth",
        "Byrne",
        "Boern",
        "Catherine",
        "Kathryn",
        "O'Brien",
        "McDonald-Smith",
        "Straße",
    ]);

    let verbora = MatchRatingApproach::new();
    let rphonetic = RMatchRating;

    for name in &corpus {
        assert_eq!(
            verbora.process(name),
            rphonetic.encode(name),
            "MRA encoding diverges from rphonetic for {name:?}"
        );
    }
}

#[test]
fn match_rating_comparison_decision_agrees_with_rphonetic_on_name_pairs() {
    // MRA's point is the match DECISION (`is_encoded_equals`, the published
    // rating algorithm), not code equality — so the decision itself is
    // verified over every ordered pair of corpus names (~653² ≈ 426K
    // pairs; ordered, because the empty-encoding corner rphonetic handles
    // asymmetrically is exactly the kind of thing an unordered sweep would
    // hide — no corpus name hits it, but the sweep must be able to see it
    // if one ever does).
    let names = load_names();
    let verbora = MatchRatingApproach::new();
    let rphonetic = RMatchRating;

    let mut positives = 0usize;
    let mut total = 0usize;
    for a in &names {
        for b in &names {
            let want = rphonetic.is_encoded_equals(a, b);
            assert_eq!(
                verbora.compare(a, b),
                want,
                "MRA match decision diverges from rphonetic for ({a:?}, {b:?})"
            );
            positives += usize::from(want);
            total += 1;
        }
    }
    // The corpus genuinely exercises both outcomes: more positives than the
    // trivial self-match diagonal, and plenty of rejections.
    assert!(
        positives > names.len(),
        "corpus produced no off-diagonal MRA matches — the sweep is not \
         exercising the accept path"
    );
    assert!(
        positives < total / 2,
        "corpus matched more than half of all pairs — the sweep is not \
         exercising the reject path"
    );

    // Classic homophone pairs, pinned against BOTH implementations so a
    // future rphonetic upgrade changing the oracle fails loudly here too.
    for (a, b, want) in [
        ("Byrne", "Boern", true),
        ("Smith", "Smyth", true),
        ("Catherine", "Kathryn", true),
        ("Franciszek", "Frances", true),
        ("Karl", "Alessandro", false),
    ] {
        assert_eq!(
            rphonetic.is_encoded_equals(a, b),
            want,
            "rphonetic oracle moved for ({a:?}, {b:?})"
        );
        assert_eq!(
            verbora.compare(a, b),
            want,
            "MRA match decision wrong for ({a:?}, {b:?})"
        );
    }
}

#[test]
fn daitch_mokotoff_branching_agrees_byte_for_byte_with_rphonetic_soundex() {
    // Unlike regime 1's `SoundExDM` (single-branch, paired with
    // rphonetic's `encode()`), Verbora's branching `DaitchMokotoff` is
    // pinned to rphonetic's full multi-branch output: `process` returns
    // exactly what `soundex()` returns (pipe-joined branch codes, branch
    // order included). Extras: branching clusters ("AUERBACH" → two codes,
    // "Rosochowaciec" → eight, the hyphenated "Jackson-Jackson" — the
    // worst branching fixture Verbora's module doc names), whitespace
    // removal ("Ben Aron"), word-start context across skipped characters
    // ("'OBrien"), rule-less characters not resetting the adjacent-code
    // merge ("b0b" vs "bob"), the m/n merge exception ("m-n"), and the
    // ASCII-folding fixtures from rphonetic's own test suite ("Straßburg",
    // "Éregon"). rphonetic's D-M is BTreeMap-driven and panic-free, so
    // non-ASCII input is safely in-domain here.
    let corpus = corpus_with(&[
        "AUERBACH",
        "GOLDEN",
        "Rosochowaciec",
        "Jackson-Jackson",
        "Mintz",
        "Moskowitz",
        "Moskovitz",
        "AKSSOL",
        "GERSCHFELD",
        "Ben Aron",
        "'OBrien",
        "b0b",
        "bob",
        "m-n",
        "Straßburg",
        "Strasburg",
        "Éregon",
        "Eregon",
    ]);

    let verbora = DaitchMokotoff::new();
    let rphonetic = DaitchMokotoffSoundex::default();

    for name in &corpus {
        // process ↔ soundex(): the pipe-joined branch codes.
        assert_eq!(
            verbora.process(name),
            rphonetic.soundex(name),
            "branching D-M diverges from rphonetic soundex() for {name:?}"
        );
        // codes() ↔ inner_soundex(_, true): same codes, structured.
        let codes = verbora.codes(name);
        assert_eq!(
            codes,
            rphonetic.inner_soundex(name, true),
            "D-M codes() diverges from rphonetic inner_soundex(_, true) for {name:?}"
        );
        // codes()[0] ↔ encode(): the first branch IS the non-branching walk.
        assert_eq!(
            codes[0],
            rphonetic.encode(name),
            "D-M first branch diverges from rphonetic's non-branching encode() for {name:?}"
        );
    }
}

#[test]
fn daitch_mokotoff_reproduces_rphonetics_documented_quirks() {
    // `crates/verbora-phonetics/src/daitch_mokotoff.rs` documents four
    // rphonetic quirks as deliberately reproduced rather than repaired.
    // Reproduced means MATCHING output — so each is verified live against
    // rphonetic here (not trusted from the doc comment), and additionally
    // pinned to the literal value the module doc records, so this test
    // fails loudly if either side ever moves.
    let verbora = DaitchMokotoff::new();
    let rphonetic = DaitchMokotoffSoundex::default();

    for (input, documented) in [
        // Quirk 1: branch dedup happens DURING the walk on (partial code,
        // last replacement), never on the finished codes — duplicates
        // survive into the final list.
        ("rsrs", "400000|494000|940000|940000"),
        // Quirk 2: the non-ASCII rule keys (ą, ę, ţ, ț) consume the
        // following character as well — the swallowed k never codes.
        ("ţka", "300000|400000"),
        // Quirk 3: their before-a-vowel probe looks one character too far —
        // "bąb" branches (probe sees no vowel), "bąbel" does not (probe
        // lands on the e), and the swallowed b never codes.
        ("bąb", "700000|760000"),
        ("bąbel", "780000"),
        // Quirk 4: case folding takes the FIRST character of the Unicode
        // lowercase form — İ becomes bare i, its combining dot dropped and
        // then skipped as rule-less.
        ("İstanbul", "043678"),
    ] {
        let ours = verbora.process(input);
        assert_eq!(
            ours,
            rphonetic.soundex(input),
            "D-M quirk NOT actually reproduced for {input:?} — module doc claim is wrong"
        );
        assert_eq!(
            ours, documented,
            "D-M quirk output moved for {input:?} — update the module doc and this pin together"
        );
    }
}

// ---------------------------------------------------------------------------
// Regime 3: where the pin moved from rphonetic to the publication.
// ---------------------------------------------------------------------------
//
// Regime 2 above used to assert byte-exact agreement with rphonetic over the
// shared corpus *and* over a set of deliberately non-ASCII "extras". The
// Rust-native migration moved these seven encoders' source of truth: each is
// now pinned to the algorithm's own publication rather than to rphonetic, per
// `docs/design/rust-native-migration.md`'s rule that "behaviour is derived
// from a published standard or from an explicit Verbora specification — not
// from another implementation's output," and that "a competitor never defines
// correctness."
//
// The practical consequence is narrow and entirely in one place. Every
// publication in this family states its alphabet — Hood's Caverphone paper
// says "remove anything not in the standard alphabet a-z"; Taft's NYSIIS,
// Lait & Randell's Phonex and commons-codec's Refined Soundex are all stated
// over `A`-`Z`; the MRA publication gates on name *characters* — and Verbora
// now applies that step literally. rphonetic instead passes an unmapped
// scalar through to its output. So the two diverge on exactly the inputs
// carrying a scalar outside the algorithm's own alphabet, and agree
// everywhere else.
//
// Measured, not assumed: over the full 653-name corpus plus every extra the
// regime-2 tests carry, the complete divergence set is **ten inputs across
// six encoders** — Cologne `"ẞ"`; NYSIIS `"Straße"`, `"日本語"`; Caverphone
// 1.0 and 2.0 `"café"`; Phonex `"Straße"`; Refined Soundex `"ß"`, `"ﬁ"`,
// `"ſ"`; MRA `"1234567"`, `"日本語"`. Those ten moved out of regime 2's
// byte-exact corpora and into the tests below, which pin **Verbora's own
// contract** for them instead of deleting the coverage. Everything else in
// regime 2 still asserts byte-exact agreement with rphonetic, unchanged —
// including the whole ASCII corpus `benches/phonetics.rs` actually times, so
// no benchmark row loses its correctness precondition.
//
// The assertions below deliberately express each contract as an *equivalence*
// (`process(x) == process(y)`, where `y` is `x` with the out-of-alphabet
// scalars already resolved) rather than as a hardcoded expected code. A
// hardcoded code would be a recording of what the implementation currently
// emits; an equivalence is a restatement of the documented rule, and fails if
// the rule stops holding.

/// Cologne states its own exception explicitly: `ß` and `ẞ` are **encoded**,
/// as `SS`, rather than skipped like other non-`A`-`Z` scalars
/// (`crates/verbora-phonetics/src/cologne.rs`). rphonetic has no `ẞ` rule and
/// drops it, which is the whole divergence.
#[test]
fn cologne_encodes_sharp_s_as_ss_per_its_own_contract() {
    let verbora = Cologne::new();
    assert_eq!(verbora.process("ẞ"), verbora.process("SS"));
    assert_eq!(verbora.process("ß"), verbora.process("ss"));
    // Not vacuous: `SS` has a real code, so this is not two empty strings.
    assert!(!verbora.process("ẞ").is_empty());
}

/// NYSIIS reads only `A`-`Z`, skipping every other scalar, and a token with no
/// `A`-`Z` letter encodes to `""` — "genuinely 'this name has no NYSIIS key',
/// not a sentinel" (`crates/verbora-phonetics/src/nysiis.rs`).
#[test]
fn nysiis_skips_scalars_outside_a_to_z_per_its_own_contract() {
    for strict in [true, false] {
        let verbora = if strict {
            Nysiis::new()
        } else {
            Nysiis::with_strict(false)
        };
        // `ß` is skipped, so "Straße" must encode as "Strae" does.
        assert_eq!(
            verbora.process("Straße"),
            verbora.process("Strae"),
            "strict={strict}"
        );
        // No `A`-`Z` letter at all.
        assert_eq!(verbora.process("日本語"), "", "strict={strict}");
    }
}

/// Hood's Caverphone papers open with "convert to lowercase, remove anything
/// not in the standard alphabet a-z", and
/// `crates/verbora-phonetics/src/caverphone.rs` takes that literally: the code
/// "is therefore always six ASCII characters, for every input in every
/// script" (ten for 2.0). rphonetic carries the unmapped scalar into its code
/// instead, which also makes its code's byte length input-dependent.
#[test]
fn caverphone_drops_scalars_outside_a_to_z_per_its_own_contract() {
    let c1 = Caverphone1::new();
    let c2 = Caverphone2::new();

    assert_eq!(c1.process("café"), c1.process("caf"));
    assert_eq!(c2.process("café"), c2.process("caf"));

    // The fixed-width claim the contract makes, on the same input.
    assert_eq!(c1.process("café").len(), 6);
    assert_eq!(c2.process("café").len(), 10);
}

/// Phonex reads only `A`-`Z` after ASCII case folding, skipping every other
/// scalar (`crates/verbora-phonetics/src/phonex.rs`), so `ß` cannot reach the
/// key. A token with no `A`-`Z` letter encodes to all-`0` padding at the
/// configured length rather than to an empty string.
#[test]
fn phonex_skips_scalars_outside_a_to_z_per_its_own_contract() {
    let verbora = Phonex::new();
    assert_eq!(verbora.process("Straße"), verbora.process("Strae"));
    // The letter-free case, per the same contract paragraph.
    assert_eq!(verbora.process("ß"), verbora.process(""));
}

/// Refined Soundex reads only `A`-`Z`, and "returns the empty string when the
/// input contains no letters" (`crates/verbora-phonetics/src/
/// refined_soundex.rs`). None of `ß`, `ﬁ` (U+FB01) or `ſ` (U+017F) is an
/// `A`-`Z` letter — they are a sharp s, a typographic ligature and a long s,
/// none of which any `A`-`Z`-stated rule mentions — so all three encode to
/// `""`. rphonetic maps them through its own fallback instead.
#[test]
fn refined_soundex_returns_empty_for_letterless_input_per_its_own_contract() {
    let verbora = RefinedSoundex::new();
    for input in ["ß", "ﬁ", "ſ"] {
        assert_eq!(verbora.process(input), "", "input {input:?}");
    }
    // Skipping is transparent rather than cluster-breaking, per the same
    // contract paragraph: an interposed skipped scalar must not change a code.
    assert_eq!(verbora.process("Braﬁz"), verbora.process("Braz"));
}

/// MRA folds accented Latin letters to plain ASCII and skips every scalar that
/// is still not `A`-`Z` — "digits, punctuation, whitespace, Cyrillic, CJK,
/// emoji" (`crates/verbora-phonetics/src/match_rating.rs`) — so a purely
/// numeric or purely CJK input reaches the encoder with no letters left and
/// encodes to `""`. rphonetic passes digits straight through instead, which
/// is the divergence that moved these two inputs out of regime 2.
#[test]
fn match_rating_skips_non_letters_per_its_own_contract() {
    let verbora = MatchRatingApproach::new();
    for input in ["1234567", "日本語"] {
        assert_eq!(verbora.process(input), "", "input {input:?}");
    }
    // Skipped rather than encoded, so interposing them changes no code.
    assert_eq!(verbora.process("Sm1th"), verbora.process("Smth"));
    assert_eq!(verbora.process("Smith"), verbora.process("Smi日th"));
}

/// MRA's *comparison* is a separate published decision from its encoding, and
/// the contract's "Comparison" section orders its rules deliberately. Two of
/// them are easy to conflate with the encoding rule above, so both are pinned
/// here:
///
/// 1. "Either side trimming to fewer than two scalars → not a match." This
///    gate counts scalars after trimming *whitespace*, before the `A`-`Z`
///    filter — it is the publication's "a single initial is not a name" rule.
/// 2. "Raw string equality, before any encoding → a match." This fires
///    *before* encoding, so two inputs that both encode to `""` still match
///    when the raw strings are identical. `match_rating.rs`'s own comment
///    calls this out: `".." == ".."` matches even though both encode to `""`.
#[test]
fn match_rating_comparison_gates_and_raw_equality_per_its_own_contract() {
    let verbora = MatchRatingApproach::new();

    // Rule 1: fewer than two scalars after trimming is never a match, even
    // against itself.
    for short in ["", "A", " A ", "1"] {
        assert!(!verbora.compare(short, short), "input {short:?}");
    }

    // Rule 2: raw equality short-circuits ahead of encoding, so a letterless
    // input matches itself despite encoding to "".
    for letterless in ["1234567", "日本語"] {
        assert_eq!(verbora.process(letterless), "");
        assert!(
            verbora.compare(letterless, letterless),
            "raw equality must short-circuit for {letterless:?}"
        );
    }
}
