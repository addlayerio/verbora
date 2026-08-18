//! `CORRECTNESS BEFORE PERFORMANCE` for `benches/normalizers.rs`.
//!
//! Verifies, once and outside the timed code, that Verbora's
//! `remove_diacritics` and the `diacritics` 0.2.2 crate agree on the domain
//! actually benchmarked (precomposed Latin text, ASCII, Cyrillic rejection),
//! and explicitly documents — via an assertion, not just a comment — every
//! real divergence found: standalone combining marks (first round), and the
//! ash/thorn fold family (`æ`/`Æ`/`ǣ`/`Ǣ`/`ǽ`/`Ǽ`/`þ`, found by this second
//! round's full-range sweep). The benchmarked domain deliberately excludes
//! both.
//!
//! Also covers `normalize_ja`'s two competitors: `unicode-jp` 0.4.0
//! (`kana::hira2kata`/`kana::kata2hira`) and `kana-converter` 0.1.2
//! (`kana_converter::to_double_byte(_, KanaOnly)`).
//!
//! The second coverage round adds, on top of the first round's fixtures:
//! agreement checks for every new benchmarked shape (`"<n>-late"`,
//! `"<n>-dense"`, `"<n>-noop"`, `"<n>-plain"` — see `benches/normalizers.rs`'s
//! own doc comment), exhaustive per-character sweeps over the full agreed
//! Unicode blocks (precomposed Latin U+00C0..=U+024F minus the documented
//! divergences, the whole hiragana block U+3041..=U+3096, the whole fullwidth
//! katakana block U+30A1..=U+30F6, and all 56 mark-free halfwidth katakana
//! U+FF66..=U+FF9D), a kana round-trip agreement dimension, and a
//! deterministic seeded random-mix sweep over the verified-agreeing alphabet.

use verbora_normalizers::ja::converters::{
    hiragana_to_katakana, katakana_hf, katakana_to_hiragana,
};
use verbora_normalizers::remove_diacritics;

fn ascii_prose(words: usize) -> String {
    "the quick brown fox jumps over the lazy dog "
        .repeat(words.div_ceil(9))
        .trim_end()
        .to_owned()
}

fn accented_prose(repeats: usize) -> String {
    "crème brûlée à la française, naïve résumé of Ångström ".repeat(repeats)
}

/// The exact domain `benches/normalizers.rs` measures: ASCII rejection and
/// precomposed accented Latin, at every benchmarked size.
#[test]
fn agrees_on_benchmarked_domain() {
    for n in [4, 16, 64, 256, 1024] {
        let a = ascii_prose(n);
        assert_eq!(
            remove_diacritics(&a).into_owned(),
            diacritics::remove_diacritics(&a)
        );

        let s = accented_prose(n);
        assert_eq!(
            remove_diacritics(&s).into_owned(),
            diacritics::remove_diacritics(&s)
        );
    }
}

/// Table-lookup quirks Verbora's own doc comment calls out by name, checked
/// against the `diacritics` crate one at a time. All match — an unusually
/// close pairing for two independently-written tables (see
/// `benches/normalizers.rs`'s own doc comment for why).
#[test]
fn shared_table_quirks_agree() {
    let cases = [
        (
            "piñon ça va über résumé œdipe",
            "pinon ca va uber resume oedipe",
        ),
        ("ſ", "l"),                     // the documented long-s-folds-to-l bug
        ("ß STRASSE ẞ", "s STRASSE S"), // ß→s and ẞ→S, not ss/SS
        ("İstanbul ıstanbul", "Istanbul istanbul"),
        ("Москва не сразу строилась", "Москва не сразу строилась"), // Cyrillic: untouched
        ("ﬁﬂĲĳŊŋĸȸȹ", "ﬁﬂĲĳŊŋĸȸȹ"), // ligatures: neither table folds these
    ];
    for (input, expected) in cases {
        let v = remove_diacritics(input).into_owned();
        let d = diacritics::remove_diacritics(input);
        assert_eq!(v, expected, "verbora mismatch for {input:?}");
        assert_eq!(d, expected, "diacritics-crate mismatch for {input:?}");
    }
}

/// The one real divergence the first round documented: `diacritics` silently
/// drops standalone combining marks (U+0300-U+036F and friends); Verbora
/// never decomposes, so it leaves them untouched. `benches/normalizers.rs`
/// deliberately never feeds combining-mark input to either side.
#[test]
fn standalone_combining_marks_are_a_documented_divergence() {
    let input = "e\u{0301}"; // 'e' + COMBINING ACUTE ACCENT, not precomposed 'é'
    let v = remove_diacritics(input).into_owned();
    let d = diacritics::remove_diacritics(input);
    assert_eq!(
        v, "e\u{0301}",
        "Verbora must not decompose/strip combining marks"
    );
    assert_eq!(
        d, "e",
        "diacritics crate is expected to strip the combining mark"
    );
    assert_ne!(
        v, d,
        "the combining-mark divergence is expected to still exist"
    );
}

// ---------------------------------------------------------------------------
// Second-round `remove_diacritics` coverage: the new benchmarked shapes, the
// full-range sweep, and the ash/thorn divergence that sweep surfaced
// ---------------------------------------------------------------------------

/// The 53 precomposed accented Latin letters `benches/normalizers.rs`'s
/// `"<n>-dense"` shape repeats — every character folds, and both tables fold
/// every one of them identically (verified below and, per character, by the
/// full-range sweep). Deliberately excludes `æ`/`Æ`/`ǣ`/`Ǣ`/`ǽ`/`Ǽ`/`þ`,
/// where the two tables genuinely diverge — see
/// [`ae_and_thorn_folds_are_a_documented_divergence`]. Identical to the
/// bench's own `dense_accented` alphabet.
pub(crate) const DENSE_ACCENTED_ALPHABET: &str =
    "àáâãäåçèéêëìíîïñòóôõöùúûüýÿÀÁÂÃÄÅÇÈÉÊËÌÍÎÏÑÒÓÔÕÖÙÚÛÜÝ";

/// The exact codepoints, inside the swept `U+00C0..=U+024F` range, where the
/// two tables genuinely diverge — the documented exclusion list for
/// [`agrees_across_the_full_precomposed_latin_range`], asserted one by one in
/// [`ae_and_thorn_folds_are_a_documented_divergence`] so this list can never
/// silently grow or shrink.
const LATIN_DIVERGENT_CODEPOINTS: [u32; 7] =
    [0x00C6, 0x00E6, 0x00FE, 0x01E2, 0x01E3, 0x01FC, 0x01FD];

/// The two new `remove_diacritics_*` shapes `benches/normalizers.rs`'s second
/// coverage round adds (`"<n>-late"`: one fold at the very end of otherwise
/// pure-ASCII prose; `"<n>-dense"`: every character folds), at every
/// benchmarked size — the same generators, byte for byte.
#[test]
fn agrees_on_new_benchmarked_shapes() {
    for n in [4, 16, 64, 256, 1024] {
        let mut late = ascii_prose(n);
        late.push('é');
        assert_eq!(
            remove_diacritics(&late).into_owned(),
            diacritics::remove_diacritics(&late),
            "\"{n}-late\" shape"
        );

        let dense = DENSE_ACCENTED_ALPHABET.repeat(n);
        assert_eq!(
            remove_diacritics(&dense).into_owned(),
            diacritics::remove_diacritics(&dense),
            "\"{n}-dense\" shape"
        );
    }
}

/// Not just cross-agreement: the dense alphabet's fold is pinned to its
/// expected ASCII value on both sides, so a future change to *either* table
/// that still happened to keep them agreeing would be caught too.
#[test]
fn dense_alphabet_folds_to_expected_ascii() {
    let expected = "aaaaaaceeeeiiiinooooouuuuyyAAAAAACEEEEIIIINOOOOOUUUUY";
    let v = remove_diacritics(DENSE_ACCENTED_ALPHABET).into_owned();
    let d = diacritics::remove_diacritics(DENSE_ACCENTED_ALPHABET);
    assert_eq!(v, expected, "verbora dense-alphabet fold");
    assert_eq!(d, expected, "diacritics-crate dense-alphabet fold");
}

/// Exhaustive per-character sweep over the whole precomposed range the
/// benchmarked prose draws from — Latin-1 Supplement letters plus Latin
/// Extended-A and Extended-B's start (U+00C0..=U+024F) — asserting the two
/// tables agree on every single codepoint except the seven in
/// [`LATIN_DIVERGENT_CODEPOINTS`]. Agreement here includes agreeing to
/// *reject* (both sides leaving a character like `đ` untouched), so this is
/// a much stronger claim than the prose fixtures alone: the whole block is
/// verified-agreeing terrain, not just the sampled words.
#[test]
fn agrees_across_the_full_precomposed_latin_range() {
    for cp in 0x00C0u32..=0x024F {
        if LATIN_DIVERGENT_CODEPOINTS.contains(&cp) {
            continue; // documented divergences, asserted individually below
        }
        let c = char::from_u32(cp).expect("U+00C0..=U+024F is all valid scalar values");
        let s = c.to_string();
        let v = remove_diacritics(&s).into_owned();
        let d = diacritics::remove_diacritics(&s);
        assert_eq!(v, d, "U+{cp:04X} ({c}): verbora={v:?} diacritics={d:?}");
    }
}

/// A second real divergence family, surfaced by this round's full-range sweep
/// and previously undocumented: the ash and thorn folds. Verbora folds ash
/// (`æ`/`Æ`, and the derived `ǣ`/`Ǣ`/`ǽ`/`Ǽ`) to the digraph `ae`/`AE`; the
/// `diacritics` crate maps each to a bare `a`/`A`. Verbora leaves thorn (`þ`)
/// untouched (a distinct letter, not a diacritic-carrying one); the
/// `diacritics` crate folds it to `b`. Neither character appears in
/// `accented_prose`, the `"<n>-dense"` alphabet, or any other benchmarked
/// input — `benches/normalizers.rs` deliberately never feeds either family to
/// either side, exactly as it already avoids standalone combining marks.
#[test]
fn ae_and_thorn_folds_are_a_documented_divergence() {
    let cases = [
        ("Æ", "AE", "A"),
        ("æ", "ae", "a"),
        ("Ǣ", "AE", "A"),
        ("ǣ", "ae", "a"),
        ("Ǽ", "AE", "A"),
        ("ǽ", "ae", "a"),
        ("þ", "þ", "b"),
    ];
    // Every case here must be listed in `LATIN_DIVERGENT_CODEPOINTS` and
    // vice versa, so the sweep's exclusion list and this test can never
    // drift apart.
    assert_eq!(cases.len(), LATIN_DIVERGENT_CODEPOINTS.len());
    for (input, verbora_expected, diacritics_expected) in cases {
        let cp = input.chars().next().expect("single char") as u32;
        assert!(
            LATIN_DIVERGENT_CODEPOINTS.contains(&cp),
            "U+{cp:04X} missing from LATIN_DIVERGENT_CODEPOINTS"
        );
        let v = remove_diacritics(input).into_owned();
        let d = diacritics::remove_diacritics(input);
        assert_eq!(v, verbora_expected, "verbora fold of {input:?}");
        assert_eq!(d, diacritics_expected, "diacritics-crate fold of {input:?}");
        assert_ne!(v, d, "the {input:?} divergence is expected to still exist");
    }
}

/// SplitMix64 — the standard tiny deterministic PRNG, inlined so this test
/// crate needs no `rand` dependency. Seeded with a fixed constant below, so
/// every run checks the identical case set.
fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// 512 deterministic random mixes (fixed seed) of the verified-agreeing
/// alphabet — ASCII prose characters, the dense accented alphabet, and the
/// named table quirks (`œ`/`Œ`/`ß`/`ẞ`/`ſ`/`İ`/`ı`/`Å`) — in shuffled orders
/// and lengths the fixed prose generators never produce. Every mix must agree
/// exactly; the alphabet contains no character from either documented
/// divergence family, so a failure here is a *new* divergence, not a known
/// one resurfacing.
#[test]
fn seeded_random_mixes_of_the_agreed_alphabet_agree() {
    let alphabet: Vec<char> = "the quick brown fox jumps over the lazy dog ,.!?"
        .chars()
        .chain(DENSE_ACCENTED_ALPHABET.chars())
        .chain("œŒßẞſİıÅ".chars())
        .collect();
    let mut state = 0x5EED_0001_D1AC_0001u64;
    for case in 0..512 {
        let len = 1 + (splitmix64(&mut state) % 64) as usize;
        let s: String = (0..len)
            .map(|_| alphabet[(splitmix64(&mut state) as usize) % alphabet.len()])
            .collect();
        let v = remove_diacritics(&s).into_owned();
        let d = diacritics::remove_diacritics(&s);
        assert_eq!(v, d, "case {case}: input={s:?}");
    }
}

// ---------------------------------------------------------------------------
// `unicode-jp` (`kana::hira2kata`/`kata2hira`) vs
// `hiragana_to_katakana`/`katakana_to_hiragana`
// ---------------------------------------------------------------------------

/// The Iroha pangram (いろは歌), all 47 base hiragana with no repeats,
/// historically used as a Japanese font/encoding test string for exactly
/// this reason — pure hiragana, entirely inside U+3041..=U+3096, no
/// iteration marks, no small tsu, no halfwidth. The same text
/// `benches/normalizers.rs`'s `ja_hiragana_to_katakana`/
/// `ja_katakana_to_hiragana` groups repeat to build their input.
pub(crate) const IROHA_HIRAGANA: &str = "いろはにほへとちりぬるをわかよたれそつねならむうゐのおくやまけふこえてあさきゆめみしゑひもせす";

/// The same pangram transliterated to katakana, used as
/// `katakana_to_hiragana`'s input — entirely inside U+30A1..=U+30F6 (no
/// `ー`, no `ヽヾ`, no halfwidth).
pub(crate) const IROHA_KATAKANA: &str = "イロハニホヘトチリヌルヲワカヨタレソツネナラムウヰノオクヤマケフコエテアサキユメミシヱヒモセス";

/// `kana::hira2kata` is a bare `char` shift over U+3041..=U+3096
/// (`unicode-jp-0.4.0/src/kana.rs`'s `shift_code`); Verbora's
/// `hiragana_to_katakana` additionally folds halfwidth katakana to fullwidth
/// and fixes standalone voiced marks / small-tsu-before-n-row first. On input
/// that never exercises those extra stages, both reduce to the same shift —
/// confirmed here on real Japanese text, not synthetic input.
#[test]
fn hira2kata_agrees_with_verbora_on_pure_hiragana() {
    let v = hiragana_to_katakana(IROHA_HIRAGANA);
    let u = kana::hira2kata(IROHA_HIRAGANA);
    assert_eq!(v.as_ref(), u, "verbora={v} unicode-jp={u}");
}

#[test]
fn kata2hira_agrees_with_verbora_on_pure_katakana() {
    let v = katakana_to_hiragana(IROHA_KATAKANA);
    let u = kana::kata2hira(IROHA_KATAKANA);
    assert_eq!(v.as_ref(), u, "verbora={v} unicode-jp={u}");
}

/// Two real divergences, verified explicitly rather than only described:
/// `kana::hira2kata` never touches halfwidth input at all (Verbora's
/// `hiragana_to_katakana` folds it to fullwidth first via `katakana_hf`),
/// and it has no phonetic small-tsu-before-n-row fix (`っな` -> `んな`,
/// which Verbora's `fix_fullwidth_kana` stage applies and then shifts to
/// katakana). `benches/normalizers.rs` never feeds either shape to either
/// side because of this.
#[test]
fn hira2kata_diverges_on_small_tsu_and_halfwidth() {
    let v = hiragana_to_katakana("まっなか");
    let u = kana::hira2kata("まっなか");
    assert_eq!(
        v.as_ref(),
        "マンナカ",
        "Verbora fixes っ before な-row to ん"
    );
    assert_eq!(u, "マッナカ", "unicode-jp has no such fix -- a bare shift");
    assert_ne!(v.as_ref(), u);

    let v2 = hiragana_to_katakana("ｶﾀｶﾅ");
    let u2 = kana::hira2kata("ｶﾀｶﾅ");
    assert_eq!(v2.as_ref(), "カタカナ", "Verbora folds halfwidth first");
    assert_eq!(u2, "ｶﾀｶﾅ", "unicode-jp's shift range excludes halfwidth");
    assert_ne!(v2.as_ref(), u2);
}

/// Second-round widening of [`hira2kata_agrees_with_verbora_on_pure_hiragana`]
/// from the pangram's 47 characters to the **whole** shift range both
/// implementations define (U+3041..=U+3096 — small kana, every voiced/
/// semi-voiced form, and ゔ/ゕ/ゖ included): per character, and once as the
/// single 86-character string in codepoint order (whose っ lands before つ,
/// a t-row consonant, so Verbora's small-tsu-before-*n-row* fix stays
/// dormant — the narrowing the first round documented is respected, not
/// widened). The reverse direction sweeps the mirror katakana block the same
/// two ways.
#[test]
fn agrees_across_the_full_kana_shift_ranges() {
    let mut hira = String::new();
    for cp in 0x3041u32..=0x3096 {
        let c = char::from_u32(cp).expect("U+3041..=U+3096 is all valid scalar values");
        let s = c.to_string();
        let v = hiragana_to_katakana(&s);
        let u = kana::hira2kata(&s);
        assert_eq!(v.as_ref(), u, "hira2kata U+{cp:04X} ({c})");
        hira.push(c);
    }
    assert_eq!(
        hiragana_to_katakana(&hira).as_ref(),
        kana::hira2kata(&hira),
        "whole hiragana block as one string"
    );

    let mut kata = String::new();
    for cp in 0x30A1u32..=0x30F6 {
        let c = char::from_u32(cp).expect("U+30A1..=U+30F6 is all valid scalar values");
        let s = c.to_string();
        let v = katakana_to_hiragana(&s);
        let u = kana::kata2hira(&s);
        assert_eq!(v.as_ref(), u, "kata2hira U+{cp:04X} ({c})");
        kata.push(c);
    }
    assert_eq!(
        katakana_to_hiragana(&kata).as_ref(),
        kana::kata2hira(&kata),
        "whole katakana block as one string"
    );
}

/// The `"<n>-noop"` shapes `benches/normalizers.rs`'s second coverage round
/// adds: already-converted input (katakana into the hira→kata direction and
/// vice versa) must be a byte-identical pass-through on **both** sides — not
/// merely cross-agreeing, but a true no-op, which is what makes the shape a
/// rejection benchmark rather than a conversion one.
#[test]
fn already_converted_kana_is_a_verified_noop_on_both_sides() {
    let v = hiragana_to_katakana(IROHA_KATAKANA);
    let u = kana::hira2kata(IROHA_KATAKANA);
    assert_eq!(
        v.as_ref(),
        IROHA_KATAKANA,
        "verbora: katakana passes through"
    );
    assert_eq!(u, IROHA_KATAKANA, "unicode-jp: katakana passes through");

    let v2 = katakana_to_hiragana(IROHA_HIRAGANA);
    let u2 = kana::kata2hira(IROHA_HIRAGANA);
    assert_eq!(
        v2.as_ref(),
        IROHA_HIRAGANA,
        "verbora: hiragana passes through"
    );
    assert_eq!(u2, IROHA_HIRAGANA, "unicode-jp: hiragana passes through");
}

/// A new agreement dimension the first round never checked: the round trip.
/// Converting the pangram hira→kata→hira (and kata→hira→kata) must restore
/// the original on both sides — composition agreement, not just single-step
/// agreement.
#[test]
fn kana_round_trips_agree_and_restore_the_original() {
    let v = katakana_to_hiragana(hiragana_to_katakana(IROHA_HIRAGANA).as_ref()).into_owned();
    let u = kana::kata2hira(&kana::hira2kata(IROHA_HIRAGANA));
    assert_eq!(
        v, IROHA_HIRAGANA,
        "verbora round trip restores the original"
    );
    assert_eq!(
        u, IROHA_HIRAGANA,
        "unicode-jp round trip restores the original"
    );

    let v2 = hiragana_to_katakana(katakana_to_hiragana(IROHA_KATAKANA).as_ref()).into_owned();
    let u2 = kana::hira2kata(&kana::kata2hira(IROHA_KATAKANA));
    assert_eq!(v2, IROHA_KATAKANA);
    assert_eq!(u2, IROHA_KATAKANA);
}

// ---------------------------------------------------------------------------
// `kana-converter` (`kana_converter::to_double_byte(_, KanaOnly)`) vs
// `katakana_hf`
// ---------------------------------------------------------------------------

/// Halfwidth katakana using only the voiced/semi-voiced pairs both Verbora's
/// `HF_KATAKANA` table (`crates/verbora-normalizers/src/ja/tables.rs`) and
/// `kana-converter`'s `VOICED_HALVES`/`SEMIVOICED_HALVES` maps recognize —
/// `kana-converter`'s own doctest input
/// (`kana-converter-0.1.2/src/lib.rs`), extended with the か/さ/た/は-row
/// voiced and は-row semi-voiced combinations. No halfwidth punctuation or
/// space (see the divergence test below for why), and no `ｦﾞ`/`ﾜﾞ` (valid in
/// `kana-converter`'s table but absent from Verbora's, a second divergence
/// also documented below).
pub(crate) const HALFWIDTH_KATAKANA: &str = "ｼﾝｸﾞﾙﾊﾞｲﾄｶﾅｶﾀｶﾅｶﾞｷﾞｸﾞｹﾞｺﾞｻﾞｼﾞｽﾞｾﾞｿﾞﾀﾞﾁﾞﾂﾞﾃﾞﾄﾞﾊﾟﾋﾟﾌﾟﾍﾟﾎﾟ";

/// All 56 mark-free halfwidth katakana, U+FF66..=U+FF9D in codepoint order —
/// the `"<n>-plain"` shape `benches/normalizers.rs`'s second coverage round
/// repeats. No dakuten/handakuten pair ever forms, so Verbora's two-character
/// lookup and `kana-converter`'s `+1`/`+2` offset arithmetic both stay on the
/// one-to-one path, where they agree per character (verified below over the
/// whole range, not just this string). Identical to the bench's own
/// `plain_halfwidth_katakana` alphabet.
pub(crate) const PLAIN_HALFWIDTH_KATAKANA: &str =
    "ｦｧｨｩｪｫｬｭｮｯｰｱｲｳｴｵｶｷｸｹｺｻｼｽｾｿﾀﾁﾂﾃﾄﾅﾆﾇﾈﾉﾊﾋﾌﾍﾎﾏﾐﾑﾒﾓﾔﾕﾖﾗﾘﾙﾚﾛﾜﾝ";

#[test]
fn kana_converter_kana_only_agrees_with_katakana_hf_on_valid_dakuten_input() {
    let v = katakana_hf(HALFWIDTH_KATAKANA);
    let k =
        kana_converter::to_double_byte(HALFWIDTH_KATAKANA, kana_converter::ConvertMode::KanaOnly);
    assert_eq!(v.as_ref(), k, "verbora={v} kana-converter={k}");
}

/// Second-round widening of the halfwidth agreement from the voiced-pair
/// fixture to the **entire** mark-free halfwidth katakana range: every
/// character U+FF66..=U+FF9D individually (so a single-character regression
/// is pinpointed, not just detected) and [`PLAIN_HALFWIDTH_KATAKANA`] as one
/// string — the exact `"<n>-plain"` bench input. The two mark characters
/// U+FF9E/U+FF9F are deliberately *not* swept standalone: an orphan mark is
/// the first round's documented divergence (see
/// [`kana_converter_diverges_on_punctuation_and_orphan_dakuten`]), and this
/// sweep respects that exclusion rather than widening the agreed domain into
/// it.
#[test]
fn kana_converter_agrees_with_katakana_hf_on_all_mark_free_halfwidth() {
    use kana_converter::{ConvertMode::KanaOnly, to_double_byte};

    for cp in 0xFF66u32..=0xFF9D {
        let c = char::from_u32(cp).expect("U+FF66..=U+FF9D is all valid scalar values");
        let s = c.to_string();
        let v = katakana_hf(&s);
        let k = to_double_byte(&s, KanaOnly);
        assert_eq!(v.as_ref(), k, "U+{cp:04X} ({c})");
    }

    let v = katakana_hf(PLAIN_HALFWIDTH_KATAKANA);
    let k = to_double_byte(PLAIN_HALFWIDTH_KATAKANA, KanaOnly);
    assert_eq!(v.as_ref(), k, "\"<n>-plain\" bench string");
}

/// Two real divergences, both rooted in the same design difference: Verbora's
/// `katakana_hf` is a literal table lookup, keyed on real two-character
/// dakuten/handakuten pairs (`HF_KATAKANA.two` in
/// `crates/verbora-normalizers/src/ja/tables.rs`); `kana-converter`'s
/// `convert_kana_char` (`kana-converter-0.1.2/src/lib.rs`) instead maps the
/// base character then blindly adds `+1` (dakuten) or `+2` (handakuten) to
/// *its fullwidth codepoint*, with no table of which combinations are real.
///
/// First: that arithmetic happens to land on the correct voiced/semi-voiced
/// character for the ordinary gojuon rows only because Unicode groups them
/// adjacently in the katakana block — it does not for `ｦ`/`ﾜ`, whose real
/// voiced forms `ヺ`/`ヷ` (U+30FA/U+30F7) were added later and are not
/// adjacent: `ｦﾞ` lands on U+30F2+1 = U+30F3 `ン`, and `ﾜﾞ` on
/// U+30EF+1 = U+30F0 `ヰ` — two unrelated, real katakana characters, not the
/// intended voiced forms and not what Verbora produces either (Verbora's
/// table has no `ｦﾞ`/`ﾜﾞ` entries at all, so both mark characters pass
/// through unmatched, as usual). Second: the same arithmetic applied to a
/// mark with no valid preceding base (`ｰﾞ`) also lands on an unrelated real
/// character by the same coincidence-of-layout reasoning (U+30FC+1 =
/// U+30FD `ヽ`), and a standalone leading mark with nothing before it at all
/// is silently dropped (confirmed: `to_double_byte("ﾞ", KanaOnly)` is `""`).
/// Verbora's table lookup instead passes an unmatched mark through as the
/// standalone spacing character U+309B/U+309C. Third, unrelated to the
/// arithmetic: `kana-converter`'s `KanaOnly` mode also folds halfwidth CJK
/// punctuation and space (its `HW_FW_KANA_MAP` includes `｡｢｣､･` and `' '`) —
/// a job Verbora keeps in the separate `pure_punctuation_hf` function, not
/// `katakana_hf`. `benches/normalizers.rs` never feeds punctuation, space, or
/// an orphan/`ｦﾞ`/`ﾜﾞ` mark to either side because of any of this.
#[test]
fn kana_converter_diverges_on_punctuation_and_orphan_dakuten() {
    use kana_converter::{ConvertMode::KanaOnly, to_double_byte};

    // Halfwidth punctuation + space: kana-converter folds it, katakana_hf
    // does not (that is `pure_punctuation_hf`'s job in Verbora).
    let v = katakana_hf("｡｢｣､･ ｶ");
    let k = to_double_byte("｡｢｣､･ ｶ", KanaOnly);
    assert_eq!(
        v.as_ref(),
        "｡｢｣､･ カ",
        "katakana_hf leaves punctuation alone"
    );
    assert_eq!(
        k, "。「」、・　カ",
        "kana-converter folds punctuation and space too"
    );
    assert_ne!(v.as_ref(), k);

    // Orphan mark after a character with no valid dakuten form.
    let v2 = katakana_hf("ｰﾞ");
    let k2 = to_double_byte("ｰﾞ", KanaOnly);
    assert_eq!(
        v2.as_ref(),
        "ー゛",
        "katakana_hf: unmatched mark stays standalone"
    );
    assert_eq!(
        k2, "ヽ",
        "kana-converter miscomposes it into an unrelated katakana"
    );
    assert_ne!(v2.as_ref(), k2);

    // A leading mark with nothing before it at all.
    let v3 = katakana_hf("ﾞ");
    let k3 = to_double_byte("ﾞ", KanaOnly);
    assert_eq!(v3.as_ref(), "゛");
    assert_eq!(
        k3, "",
        "kana-converter silently drops an orphan leading mark"
    );

    // ｦ/ﾜ + dakuten: Verbora has no entry for either, so both pass through
    // unmatched; kana-converter's blind offset arithmetic lands on two
    // unrelated real katakana (ン, ヰ), not the intended ヺ/ヷ.
    let v4 = katakana_hf("ｦﾞﾜﾞ");
    let k4 = to_double_byte("ｦﾞﾜﾞ", KanaOnly);
    assert_eq!(v4.as_ref(), "ヲ゛ワ゛");
    assert_eq!(k4, "ンヰ");
    assert_ne!(v4.as_ref(), k4);
}
