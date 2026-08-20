//! `CORRECTNESS BEFORE PERFORMANCE` for `benches/normalizers.rs`.
//!
//! # What the text-shaping migration did to this file
//!
//! `docs/design/text-shaping-contract.md` deleted
//! `verbora_normalizers::ja::converters` in full and redefined
//! `remove_diacritics` from a precomposed-scalar table lookup to
//!
//! > `s` under Canonical Decomposition (NFD), with every scalar whose
//! > `Canonical_Combining_Class` is non-zero removed, under Canonical
//! > Composition (NFC).  (§3.2)
//!
//! Neither is a rename, so this file was not re-pointed at lookalikes:
//!
//! * The two `unicode-jp` (`kana::hira2kata`/`kata2hira`) sections are
//!   **deleted**. Hiragana ↔ katakana conversion is a transliteration, not a
//!   normalization, and Verbora ships none — there is no Verbora side left to
//!   compare against, so a "correctness" test here could only have asserted
//!   things about `unicode-jp`. Coverage genuinely lost, recorded rather than
//!   faked.
//! * The `kana-converter` sections are **re-pointed** from `katakana_hf` to
//!   `nfkc`, which subsumes the width fold (contract §3.2: NFKC decomposes
//!   halfwidth katakana to fullwidth and `U+FF9E` to the combining `U+3099`,
//!   which canonical composition recombines). Every agreement claim is
//!   re-proved against `nfkc` here rather than inherited.
//! * The `remove_diacritics` sections are **rewritten**, because the previous
//!   round's central findings are now false in both directions: the one
//!   documented divergence (standalone combining marks) has *disappeared*
//!   — both sides now fold `"e\u{0301}"` to `"e"` — and the seven-codepoint
//!   `LATIN_DIVERGENT_CODEPOINTS` exclusion list has grown to 105. Keeping
//!   the old shape and widening the list would have buried the real change.
//!
//! # Two questions, kept apart
//!
//! 1. **Does Verbora implement its own contract?** Asserted against an
//!    independent re-derivation of §3.2's definition from `unicode-normalization`
//!    primitives (`nfd`, `canonical_combining_class`, `nfc`), swept over
//!    `U+0000..=U+2FFFF`, plus the contract's own worked table transcribed by
//!    hand from the normative document. This is not a competitor question and
//!    does not depend on any competitor agreeing.
//!
//!    Its honest limit: `verbora-normalizers` is *also* built on
//!    `unicode-normalization`, so the re-derivation checks the composition
//!    Verbora specifies — which decomposition, which filter predicate, which
//!    order — and cannot catch a defect inside `unicode-normalization` itself.
//!    That is why the hand-transcribed contract table is there too: its
//!    expected values come from the contract and the UCD, not from running
//!    either crate.
//!
//! 2. **Is the benchmark comparing like with like?** Asserted only over the
//!    domain `benches/normalizers.rs` actually feeds both implementations:
//!    every distinct character of `ascii_prose`/`accented_prose`
//!    individually, and every whole document at every benchmarked size. The
//!    much larger disagreement outside that domain is characterised by
//!    mechanism and pinned by count, so it cannot drift silently.

use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::canonical_combining_class;
use verbora_normalizers::{nfkc, remove_diacritics};

fn ascii_prose(words: usize) -> String {
    "the quick brown fox jumps over the lazy dog "
        .repeat(words.div_ceil(9))
        .trim_end()
        .to_owned()
}

fn accented_prose(repeats: usize) -> String {
    "crème brûlée à la française, naïve résumé of Ångström ".repeat(repeats)
}

/// The benchmarked sizes, a subset of `benches/normalizers.rs`'s [`SIZES`]
/// chosen to span the whole grid without repeating near-identical documents.
const BENCHMARKED_SIZES: [usize; 5] = [4, 16, 64, 256, 1024];

// ---------------------------------------------------------------------------
// 1. Verbora against its own contract (no competitor involved)
// ---------------------------------------------------------------------------

/// §3.2's definition, re-derived from `unicode-normalization` primitives —
/// deliberately written as the contract sentence reads, not as
/// `verbora-normalizers` happens to implement it.
fn contract_definition(s: &str) -> String {
    s.nfd()
        .filter(|c| canonical_combining_class(*c) == 0)
        .collect::<String>()
        .nfc()
        .collect()
}

/// Every scalar value from `U+0000` through `U+2FFFF` — the whole BMP plus
/// the two supplementary planes that carry letters (SMP and SIP) — must fold
/// exactly as §3.2 defines. A sweep rather than samples, because the
/// definition is a total function and any codepoint-specific special case
/// would be a contract violation.
#[test]
fn remove_diacritics_implements_the_contract_definition_over_three_planes() {
    let mut checked = 0usize;
    for cp in 0u32..=0x2FFFF {
        let Some(c) = char::from_u32(cp) else {
            continue; // surrogate range: not a scalar value
        };
        let s = c.to_string();
        assert_eq!(
            remove_diacritics(&s).into_owned(),
            contract_definition(&s),
            "U+{cp:04X}: remove_diacritics must be NFD -> strip ccc != 0 -> NFC"
        );
        checked += 1;
    }
    // 0x30000 codepoints minus the 2048 surrogates.
    assert_eq!(checked, 0x30000 - 2048);
}

/// The contract's own worked table (§3.2, "What does not fold, and why", plus
/// the two folding examples that follow it), transcribed by hand from the
/// normative document. Independent of both `unicode-normalization` and
/// `verbora-normalizers`: if the re-derivation above and Verbora shared a
/// defect, these values would still catch it.
#[test]
fn remove_diacritics_matches_the_contracts_worked_table() {
    let cases = [
        // No canonical decomposition — the mark is part of the letter.
        ("ø", "ø"),
        ("Æ", "Æ"),
        ("đ", "đ"),
        ("ł", "ł"),
        ("ħ", "ħ"),
        ("ŋ", "ŋ"),
        ("ı", "ı"),
        // Not a diacritic: `ß -> ss` is case folding (UAX #21), not this.
        ("ß", "ß"),
        // Compatibility decompositions, not canonical ones.
        ("Ａ", "Ａ"),
        ("Ⓐ", "Ⓐ"),
        ("ǅ", "ǅ"),
        // A letter, not a decorated `l`. (The pre-migration table folded this
        // to "l"; the contract calls that out by name as a defect removed.)
        ("ſ", "ſ"),
        // U+212B ANGSTROM SIGN: canonical singleton to U+00C5, which
        // decomposes further.
        ("\u{212B}", "A"),
        // U+0130 decomposes to I + U+0307 (ccc = 230).
        ("İ", "I"),
        // NFC last: Hangul must recompose, not leak decomposed jamo.
        ("한국", "한국"),
    ];
    for (input, expected) in cases {
        assert_eq!(
            remove_diacritics(input).into_owned(),
            expected,
            "contract table: {input:?}"
        );
    }
}

/// NFD first, so the answer does not depend on how the caller typed the text.
/// Pre-migration this was false — precomposed `é` folded and decomposed
/// `e` + `U+0301` did not — and it is the property the whole redefinition
/// exists to buy.
#[test]
fn remove_diacritics_is_independent_of_the_callers_normalization_form() {
    for (precomposed, decomposed) in [
        ("é", "e\u{0301}"),
        ("ü", "u\u{0308}"),
        ("Å", "A\u{030A}"),
        ("ṩ", "s\u{0307}\u{0323}"),
    ] {
        let a = remove_diacritics(precomposed).into_owned();
        let b = remove_diacritics(decomposed).into_owned();
        assert_eq!(a, b, "{precomposed:?} vs {decomposed:?}");
    }
}

/// Idempotence and position independence — both stated as guarantees in §3.2,
/// both false of the functions this one replaced (`normalize_no`/`normalize_sv`
/// folded only the *first* occurrence of each needle).
#[test]
fn remove_diacritics_is_idempotent_and_position_independent() {
    let corpus = [
        "ààà",
        "crème brûlée à la française",
        "Ångström",
        "e\u{0301}e\u{0301}e\u{0301}",
        "한국 ñ ø ß",
        "",
    ];
    for s in corpus {
        let once = remove_diacritics(s).into_owned();
        assert_eq!(
            remove_diacritics(&once).into_owned(),
            once,
            "idempotence: {s:?}"
        );

        // Position independence: folding each `ccc == 0`-delimited piece
        // separately and concatenating must equal folding the whole.
        let pieces: Vec<String> = split_at_starter_boundaries(s);
        let piecewise: String = pieces
            .iter()
            .map(|p| remove_diacritics(p).into_owned())
            .collect();
        assert_eq!(piecewise, once, "position independence: {s:?}");
    }
}

/// Splits `s` at every starter (`ccc == 0`) boundary — the decomposition
/// §3.2's position-independence guarantee is stated over ("any decomposition
/// of `s` at `ccc = 0` boundaries"), so each piece is a starter plus the
/// marks that follow it.
///
/// Note that this splits `s` itself, not `s.nfd()`. Splitting the *decomposed*
/// form would cut Hangul syllables into their individual `ccc == 0` jamo, and
/// the final NFC of §3.2's definition cannot recompose a syllable whose jamo
/// were folded in separate calls — `remove_diacritics("한") == "한"` but
/// `remove_diacritics("ᄒ") + remove_diacritics("ᅡ") + remove_diacritics("ᆫ")`
/// is the decomposed sequence. That is not a counterexample to the guarantee,
/// it is a different (and stronger) claim the contract does not make.
fn split_at_starter_boundaries(s: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for c in s.chars() {
        if canonical_combining_class(c) == 0 || out.is_empty() {
            out.push(String::new());
        }
        out.last_mut().expect("just pushed").push(c);
    }
    out
}

/// `Cow::Borrowed` if and only if nothing changed — a guarantee callers are
/// documented as allowed to branch on (contract §3.2, "The `Cow` contract"),
/// which makes it a correctness property rather than a fast-path detail.
#[test]
fn remove_diacritics_borrows_exactly_when_it_changes_nothing() {
    for s in [
        "the quick brown fox",
        "",
        "ø Æ ß ſ",
        "한국",
        "crème",
        "e\u{0301}",
    ] {
        let out = remove_diacritics(s);
        let borrowed = matches!(out, std::borrow::Cow::Borrowed(_));
        assert_eq!(
            borrowed,
            out.as_ref() == s,
            "{s:?}: Borrowed iff byte-identical"
        );
    }
}

// ---------------------------------------------------------------------------
// 2. `remove_diacritics` vs the `diacritics` crate, over what is benchmarked
// ---------------------------------------------------------------------------

/// Every distinct character `benches/normalizers.rs`'s two generators can
/// produce. Both groups draw from exactly this alphabet, so per-character
/// agreement here plus whole-document agreement below is the complete
/// fairness argument for those groups — nothing is extrapolated.
fn benchmarked_alphabet() -> Vec<char> {
    let mut chars: Vec<char> = ascii_prose(9)
        .chars()
        .chain(accented_prose(1).chars())
        .collect();
    chars.sort_unstable();
    chars.dedup();
    chars
}

#[test]
fn agrees_per_character_over_the_whole_benchmarked_alphabet() {
    let alphabet = benchmarked_alphabet();
    // Pinned so a generator edit that widened the domain cannot slip past.
    assert_eq!(alphabet.len(), 36, "benchmarked alphabet size");
    for c in alphabet {
        let s = c.to_string();
        assert_eq!(
            remove_diacritics(&s).into_owned(),
            diacritics::remove_diacritics(&s),
            "U+{:04X} ({c})",
            c as u32
        );
    }
}

/// The exact documents `benches/normalizers.rs` measures, at every
/// benchmarked size.
#[test]
fn agrees_on_benchmarked_domain() {
    for n in BENCHMARKED_SIZES {
        let a = ascii_prose(n);
        assert_eq!(
            remove_diacritics(&a).into_owned(),
            diacritics::remove_diacritics(&a),
            "ascii_prose({n})"
        );

        let s = accented_prose(n);
        assert_eq!(
            remove_diacritics(&s).into_owned(),
            diacritics::remove_diacritics(&s),
            "accented_prose({n})"
        );
    }
}

/// Not just cross-agreement: the accented prose's fold is pinned to its
/// expected value, so a change to *both* tables that kept them agreeing would
/// still be caught.
#[test]
fn accented_prose_folds_to_expected_ascii() {
    let expected = "creme brulee a la francaise, naive resume of Angstrom ";
    assert_eq!(remove_diacritics(&accented_prose(1)).into_owned(), expected);
    assert_eq!(diacritics::remove_diacritics(&accented_prose(1)), expected);
}

/// 512 deterministic random mixes (fixed seed) of the benchmarked alphabet,
/// in shuffled orders and lengths the fixed prose generators never produce.
/// A failure here is a *new* divergence inside the benchmarked domain, not a
/// known one outside it resurfacing.
#[test]
fn seeded_random_mixes_of_the_benchmarked_alphabet_agree() {
    let alphabet = benchmarked_alphabet();
    let mut state = 0x5EED_0001_D1AC_0001u64;
    for case in 0..512 {
        let len = 1 + (splitmix64(&mut state) % 64) as usize;
        let s: String = (0..len)
            .map(|_| alphabet[(splitmix64(&mut state) as usize) % alphabet.len()])
            .collect();
        assert_eq!(
            remove_diacritics(&s).into_owned(),
            diacritics::remove_diacritics(&s),
            "case {case}: input={s:?}"
        );
    }
}

/// SplitMix64 — the standard tiny deterministic PRNG, inlined so this test
/// crate needs no `rand` dependency.
fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

// ---------------------------------------------------------------------------
// 3. Where the two implementations now disagree, and why
// ---------------------------------------------------------------------------

/// Every divergence over `U+00C0..=U+024F` — the range the benchmarked prose
/// draws from — classified by mechanism instead of transcribed as a list.
///
/// Pre-migration this range held 7 divergent codepoints and they were listed
/// one by one. It now holds 105, because Verbora's answer is derived from a
/// UCD property (`Canonical_Combining_Class`) while `diacritics`' is a
/// hand-curated table, so no shorter rule characterises the *agreement* set.
/// What can be characterised — and is asserted here — is that every
/// divergence falls into exactly one of three mechanical classes, that
/// Verbora's side of every one of them is the contract's answer (so none is a
/// Verbora defect), and that the three class sizes are what they are.
#[test]
fn the_divergence_set_is_characterised_and_pinned() {
    let mut verbora_leaves_untouched = Vec::new();
    let mut both_fold_differently = Vec::new();
    let mut diacritics_leaves_untouched = Vec::new();

    for cp in 0x00C0u32..=0x024F {
        let c = char::from_u32(cp).expect("U+00C0..=U+024F is all valid scalar values");
        let s = c.to_string();
        let v = remove_diacritics(&s).into_owned();
        let d = diacritics::remove_diacritics(&s);
        if v == d {
            continue;
        }

        // Verbora's side of every divergence is the contract's answer: the
        // disagreement is `diacritics`' table, never a Verbora defect.
        assert_eq!(
            v,
            contract_definition(&s),
            "U+{cp:04X} ({c}): verbora must still satisfy its own contract"
        );

        if v == s {
            // Class A: the character has no `ccc != 0` scalar in its NFD, so
            // there is nothing for Verbora to remove (`ø`, `ß`, `đ`, `ſ`);
            // `diacritics`' table folds it to a bare ASCII letter anyway.
            verbora_leaves_untouched.push(cp);
        } else if d == s {
            // Class C: a canonical decomposition `diacritics`' table lacks.
            diacritics_leaves_untouched.push(cp);
        } else {
            // Class B: both fold, but Verbora's base is itself a
            // non-decomposing special letter (`Ǣ` -> `Æ`, `Ǿ` -> `Ø`) where
            // the table goes all the way to ASCII.
            both_fold_differently.push(cp);
        }
    }

    assert_eq!(
        verbora_leaves_untouched.len(),
        97,
        "class A (verbora leaves untouched, diacritics folds)"
    );
    assert_eq!(
        both_fold_differently,
        vec![0x01E2, 0x01E3, 0x01FC, 0x01FD, 0x01FE, 0x01FF],
        "class B (both fold, different targets): Ǣ ǣ Ǽ ǽ Ǿ ǿ"
    );
    assert_eq!(
        diacritics_leaves_untouched,
        vec![0x01EE, 0x01EF],
        "class C (verbora folds, diacritics does not): Ǯ ǯ"
    );

    let total = verbora_leaves_untouched.len()
        + both_fold_differently.len()
        + diacritics_leaves_untouched.len();
    assert_eq!(total, 105, "total divergences over U+00C0..=U+024F");

    // None of them is reachable from the benchmarked alphabet.
    let alphabet = benchmarked_alphabet();
    for cp in verbora_leaves_untouched
        .iter()
        .chain(&both_fold_differently)
        .chain(&diacritics_leaves_untouched)
    {
        let c = char::from_u32(*cp).expect("swept range");
        assert!(
            !alphabet.contains(&c),
            "U+{cp:04X} ({c}) is divergent and must not be in the benchmarked alphabet"
        );
    }
}

/// Worked examples of each class, spelled out so a reader does not have to
/// re-run the sweep to know what the disagreement looks like.
#[test]
fn divergence_worked_examples() {
    let cases = [
        // (input, verbora, diacritics, class)
        ("ø", "ø", "o", 'A'),
        ("ß", "ß", "s", 'A'),
        ("ſ", "ſ", "l", 'A'),
        ("Æ", "Æ", "A", 'A'),
        ("þ", "þ", "b", 'A'),
        ("Ǣ", "Æ", "A", 'B'),
        ("ǿ", "ø", "o", 'B'),
        ("Ǯ", "Ʒ", "Ǯ", 'C'),
    ];
    for (input, verbora_expected, diacritics_expected, class) in cases {
        let v = remove_diacritics(input).into_owned();
        let d = diacritics::remove_diacritics(input);
        assert_eq!(
            v, verbora_expected,
            "class {class}: verbora fold of {input:?}"
        );
        assert_eq!(
            d, diacritics_expected,
            "class {class}: diacritics-crate fold of {input:?}"
        );
        assert_ne!(v, d, "class {class}: {input:?} must still diverge");
    }
}

/// The pre-migration round's single documented divergence — `diacritics`
/// strips standalone combining marks, Verbora did not — **no longer exists**,
/// because Verbora now decomposes first. Asserted rather than deleted, so the
/// change is recorded in the suite instead of only in a comment.
#[test]
fn the_old_standalone_combining_mark_divergence_is_gone() {
    for input in ["e\u{0301}", "u\u{0308}", "a\u{030A}\u{0301}"] {
        let v = remove_diacritics(input).into_owned();
        let d = diacritics::remove_diacritics(input);
        assert_eq!(v, d, "{input:?}: the two now agree on decomposed input");
    }
    assert_eq!(remove_diacritics("e\u{0301}").into_owned(), "e");
}

// ---------------------------------------------------------------------------
// 4. `nfkc` vs `kana-converter` (`to_double_byte(_, KanaOnly)`)
// ---------------------------------------------------------------------------

/// Halfwidth katakana using only the voiced/semi-voiced pairs both sides
/// handle — `kana-converter`'s own doctest input
/// (`kana-converter-0.1.2/src/lib.rs`), extended with the か/さ/た/は-row
/// voiced and は-row semi-voiced combinations. No halfwidth punctuation or
/// space, and no `ｦﾞ`/`ﾜﾞ` (see the divergence test below for both). Identical
/// to `benches/normalizers.rs`'s own `halfwidth_katakana`.
pub(crate) const HALFWIDTH_KATAKANA: &str = "ｼﾝｸﾞﾙﾊﾞｲﾄｶﾅｶﾀｶﾅｶﾞｷﾞｸﾞｹﾞｺﾞｻﾞｼﾞｽﾞｾﾞｿﾞﾀﾞﾁﾞﾂﾞﾃﾞﾄﾞﾊﾟﾋﾟﾌﾟﾍﾟﾎﾟ";

/// All 56 mark-free halfwidth katakana, U+FF66..=U+FF9D in codepoint order.
/// No dakuten/handakuten pair ever forms, so NFKC's decomposition and
/// `kana-converter`'s `+1`/`+2` offset arithmetic both stay on the
/// one-to-one path.
pub(crate) const PLAIN_HALFWIDTH_KATAKANA: &str =
    "ｦｧｨｩｪｫｬｭｮｯｰｱｲｳｴｵｶｷｸｹｺｻｼｽｾｿﾀﾁﾂﾃﾄﾅﾆﾇﾈﾉﾊﾋﾌﾍﾎﾏﾐﾑﾒﾓﾔﾕﾖﾗﾘﾙﾚﾛﾜﾝ";

/// The benchmarked fixture, at every benchmarked size.
#[test]
fn nfkc_agrees_with_kana_converter_on_the_benchmarked_domain() {
    for n in BENCHMARKED_SIZES {
        let s = HALFWIDTH_KATAKANA.repeat(n);
        assert_eq!(
            nfkc(&s).into_owned(),
            kana_converter::to_double_byte(&s, kana_converter::ConvertMode::KanaOnly),
            "halfwidth_katakana({n})"
        );
    }
}

/// Not just cross-agreement: the fixture's fold is pinned to its expected
/// fullwidth form on both sides.
#[test]
fn halfwidth_fixture_folds_to_expected_fullwidth() {
    let expected = "シングルバイトカナカタカナガギグゲゴザジズゼゾダヂヅデドパピプペポ";
    assert_eq!(nfkc(HALFWIDTH_KATAKANA).into_owned(), expected);
    assert_eq!(
        kana_converter::to_double_byte(HALFWIDTH_KATAKANA, kana_converter::ConvertMode::KanaOnly),
        expected
    );
}

/// Every mark-free halfwidth katakana individually (so a single-character
/// regression is pinpointed, not merely detected) and as one string. The two
/// mark characters U+FF9E/U+FF9F are deliberately not swept standalone — an
/// orphan mark is a documented divergence, asserted separately below.
#[test]
fn nfkc_agrees_with_kana_converter_on_all_mark_free_halfwidth() {
    use kana_converter::{ConvertMode::KanaOnly, to_double_byte};

    for cp in 0xFF66u32..=0xFF9D {
        let c = char::from_u32(cp).expect("U+FF66..=U+FF9D is all valid scalar values");
        let s = c.to_string();
        assert_eq!(
            nfkc(&s).into_owned(),
            to_double_byte(&s, KanaOnly),
            "U+{cp:04X} ({c})"
        );
    }

    assert_eq!(
        nfkc(PLAIN_HALFWIDTH_KATAKANA).into_owned(),
        to_double_byte(PLAIN_HALFWIDTH_KATAKANA, KanaOnly),
        "the whole mark-free block as one string"
    );
}

/// The width fold is exactly what `katakana_hf` used to do, so the migration
/// did not lose the capability — pinned here on the same inputs the deleted
/// function's own tests used, against `nfkc` rather than against the
/// competitor, so this survives even if `kana-converter` is ever unpinned.
#[test]
fn nfkc_performs_the_width_fold_katakana_hf_used_to() {
    for (input, expected) in [
        ("ｶﾞ", "ガ"),              // base + halfwidth voiced mark composes
        ("ﾊﾟ", "パ"),              // base + halfwidth semi-voiced mark composes
        ("ｶﾀｶﾅ", "カタカナ"),     // plain width fold
        ("ヲﾞ", "ヺ"),             // fullwidth base + halfwidth mark also composes
        ("カタカナ", "カタカナ"), // already fullwidth: unchanged
    ] {
        assert_eq!(nfkc(input).into_owned(), expected, "nfkc({input:?})");
    }
}

/// Real divergences, verified rather than only described — each one is a
/// reason `benches/normalizers.rs` narrows its input domain.
///
/// `kana-converter`'s `convert_kana_char` maps the base character and then
/// blindly adds `+1` (dakuten) or `+2` (handakuten) to its fullwidth
/// codepoint, with no table of which combinations are real. That arithmetic
/// lands correctly for the ordinary gojuon rows only because Unicode groups
/// them adjacently; it does not for `ｦ`/`ﾜ`, whose real voiced forms `ヺ`/`ヷ`
/// were encoded later and non-adjacently. NFKC has no such problem — it
/// decomposes `U+FF9E` to the combining `U+3099` and lets canonical
/// composition decide — so here NFKC is *more* correct, not merely different,
/// and the exclusion protects the competitor rather than Verbora.
#[test]
fn kana_converter_diverges_on_punctuation_orphan_marks_and_wo_wa() {
    use kana_converter::{ConvertMode::KanaOnly, to_double_byte};

    // ｦ/ﾜ + dakuten: NFKC composes the correct ヺ/ヷ; kana-converter's blind
    // offset arithmetic lands on two unrelated real katakana (ン, ヰ).
    let v = nfkc("ｦﾞﾜﾞ").into_owned();
    let k = to_double_byte("ｦﾞﾜﾞ", KanaOnly);
    assert_eq!(v, "ヺヷ", "NFKC composes the real voiced forms");
    assert_eq!(k, "ンヰ", "kana-converter miscomposes by +1 offset");
    assert_ne!(v, k);

    // A mark after a base with no voiced form: NFKC leaves the combining
    // mark uncomposed; kana-converter lands on an unrelated character.
    let v2 = nfkc("ｰﾞ").into_owned();
    let k2 = to_double_byte("ｰﾞ", KanaOnly);
    assert_eq!(v2, "ー\u{3099}", "NFKC: mark stays combining, uncomposed");
    assert_eq!(k2, "ヽ", "kana-converter miscomposes it");
    assert_ne!(v2, k2);

    // A leading mark with nothing before it.
    let v3 = nfkc("ﾞ").into_owned();
    let k3 = to_double_byte("ﾞ", KanaOnly);
    assert_eq!(v3, "\u{3099}", "NFKC maps U+FF9E to the combining U+3099");
    assert_eq!(
        k3, "",
        "kana-converter silently drops an orphan leading mark"
    );
    assert_ne!(v3, k3);

    // Halfwidth punctuation: both fold it, but the space differs — NFKC maps
    // U+0020 to itself, `KanaOnly` maps it to the ideographic U+3000.
    let v4 = nfkc("｡｢｣､･ ｶ").into_owned();
    let k4 = to_double_byte("｡｢｣､･ ｶ", KanaOnly);
    assert_eq!(v4, "。「」、・ カ", "NFKC leaves the ASCII space alone");
    assert_eq!(
        k4, "。「」、・\u{3000}カ",
        "kana-converter widens the space"
    );
    assert_ne!(v4, k4);
}

/// NFKC does categorically more than a halfwidth-kana table: it is the full
/// UAX #15 Normalization Form KC. This is the comparability limit
/// `benches/nfkc_halfwidth_katakana` states, asserted so it is a fact in the
/// suite rather than a claim in a comment — a reader must not generalise that
/// group's numbers beyond halfwidth kana.
#[test]
fn nfkc_does_strictly_more_than_the_competitor_outside_the_domain() {
    use kana_converter::{ConvertMode::KanaOnly, to_double_byte};

    for (input, nfkc_expected) in [
        ("Ａ", "A"),    // fullwidth Latin -> ASCII
        ("㌔", "キロ"), // squared katakana abbreviation
        ("ﬁ", "fi"),    // Latin ligature
        ("²", "2"),     // superscript digit
        ("Ⅷ", "VIII"),  // Roman numeral
    ] {
        assert_eq!(nfkc(input).into_owned(), nfkc_expected, "nfkc({input:?})");
        assert_eq!(
            to_double_byte(input, KanaOnly),
            input,
            "kana-converter leaves {input:?} alone — it only knows halfwidth kana"
        );
    }
}
