//! `CORRECTNESS BEFORE PERFORMANCE` for `benches/normalizers.rs`.
//!
//! Verifies, once and outside the timed code, that Verbora's
//! `remove_diacritics` and the `diacritics` 0.2.2 crate agree on the domain
//! actually benchmarked (precomposed Latin text, ASCII, Cyrillic rejection),
//! and explicitly documents — via an assertion, not just a comment — the one
//! real divergence found (standalone combining marks), which the benchmarked
//! domain deliberately excludes.
//!
//! Also covers `normalize_ja`'s two new competitors this pass adds:
//! `unicode-jp` 0.4.0 (`kana::hira2kata`/`kana::kata2hira`) and
//! `kana-converter` 0.1.2 (`kana_converter::to_double_byte(_, KanaOnly)`).

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

/// The one real, documented divergence: `diacritics` silently drops
/// standalone combining marks (U+0300-U+036F and friends); Verbora never
/// decomposes, so it leaves them untouched. `benches/normalizers.rs`
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

#[test]
fn kana_converter_kana_only_agrees_with_katakana_hf_on_valid_dakuten_input() {
    let v = katakana_hf(HALFWIDTH_KATAKANA);
    let k =
        kana_converter::to_double_byte(HALFWIDTH_KATAKANA, kana_converter::ConvertMode::KanaOnly);
    assert_eq!(v.as_ref(), k, "verbora={v} kana-converter={k}");
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
