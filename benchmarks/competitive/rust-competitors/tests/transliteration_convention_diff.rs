//! `CORRECTNESS BEFORE PERFORMANCE` for `benches/language.rs`'s
//! transliteration group — except here "correctness" means proving the
//! opposite of equivalence: `docs/COMPETITIVE_BENCHMARKS.md` §1.11
//! documents that `wana_kana` uses a different romanization convention
//! (doubled vowels) than Verbora/the reference's modified Hepburn (macrons),
//! so the timing comparison in `benches/language.rs` is fair for
//! *throughput on identical input* only, never for output correctness.
//!
//! This file makes that divergence a real, executed fact rather than a
//! citation of the matrix's own prose — if a future `wana_kana` release
//! ever switched conventions to match Hepburn macrons, this test would
//! start failing, which is exactly the signal that `benches/language.rs`'s
//! doc comment would need updating.
//!
//! Beyond the matrix's two cited long-vowel examples, this file pins the
//! *executed boundary* of the divergence in both directions:
//!
//! - **Where the conventions differ** (all inside the matrix's documented
//!   "different romanization convention" domain, refined by execution, not
//!   widened): u-lengthened vowels and chōonpu (the headline difference),
//!   the particle kana `を` (`o` vs. `wo`), leftover sokuon before the
//!   d-row (`ベッド` -> `betdo` vs. `beddo` — the reference's own
//!   final-sokuon `-> 't'` rule, faithfully ported per
//!   `crates/verbora-transliterators/src/ja.rs`'s `Phase::FinalSokuon`,
//!   vs. `wana_kana`'s plain consonant doubling), fullwidth punctuation
//!   and half-width katakana (Verbora passes both through; `wana_kana`
//!   rewrites both).
//! - **Where the two agree exactly** (so "different convention" is never
//!   inflated into "different everywhere"): plain kana without long
//!   vowels, doubled `a`/`o` written with kana vowels (`おかあさん` ->
//!   `okaasan` on *both* sides — Verbora's macrons cover u-lengthened and
//!   chōonpu shapes, not every phonetically long vowel), `ei` sequences,
//!   sokuon before the k/s/g/p rows, `ん` apostrophes, and the
//!   kanji/Latin pass-through domain.
//!
//! None of this changes the benchmark's framing — throughput on identical
//! input only — it just replaces a two-example citation with an executed
//! map of exactly which output shapes differ and which don't.

use verbora_transliterators::transliterate_ja;
use wana_kana::ConvertJapanese;

#[test]
fn long_vowel_romanization_conventions_genuinely_differ() {
    // "とうきょう" (Tokyo) is the matrix's own cited example: Verbora emits
    // modified-Hepburn macrons, wana_kana emits doubled vowels.
    let verbora_output = transliterate_ja("とうきょう");
    let wana_kana_output = "とうきょう".to_romaji();

    assert_eq!(
        verbora_output.as_ref(),
        "tōkyō",
        "Verbora's own documented output changed"
    );
    assert_eq!(
        wana_kana_output, "toukyou",
        "wana_kana's own documented output changed"
    );
    assert_ne!(
        verbora_output.as_ref(),
        wana_kana_output,
        "the two transliterators unexpectedly agree -- benches/language.rs's \
         'throughput only, not correctness' framing would need re-examining"
    );
}

#[test]
fn super_market_loanword_also_differs() {
    // The matrix's own second example: "スーパー" (super/supermarket).
    let verbora_output = transliterate_ja("スーパー");
    let wana_kana_output = "スーパー".to_romaji();

    assert_eq!(verbora_output.as_ref(), "sūpā");
    assert_eq!(wana_kana_output, "suupaa");
    assert_ne!(verbora_output.as_ref(), wana_kana_output);
}

#[test]
fn both_implementations_run_to_completion_on_a_range_of_kana_input() {
    // Not a correctness claim -- just confirms neither implementation
    // panics on the inputs benches/language.rs actually times, so a panic
    // never shows up disguised as a Criterion timing outlier.
    let inputs = [
        "とうきょうとっきょきょかきょくのぼーじょれーぬーゔぉー",
        "アヴァンギャルドなドキュメントィンドウとフューチャー",
        "ほんや",
        "ざっし",
        "",
    ];
    for input in inputs {
        let _ = transliterate_ja(input);
        let _ = input.to_romaji();
    }
}

/// The matrix's two examples generalized: every u-lengthened or chōonpu
/// long vowel diverges the same way (Verbora macron, `wana_kana` doubled
/// letter), across hiragana `ou`/`uu`, katakana chōonpu, and long vowels
/// produced *after* a sokuon or inside an `n'` cluster. Each case asserts
/// both concrete outputs, so a silent convention change on either side
/// fails loudly with the exact input in hand.
#[test]
fn every_u_lengthened_and_choonpu_vowel_shape_diverges() {
    let cases = [
        // (input, verbora modified-Hepburn, wana_kana doubled-vowel)
        ("きょう", "kyō", "kyou"),
        ("ゆうめい", "yūmei", "yuumei"),
        ("べんきょう", "benkyō", "benkyou"),
        ("ありがとう", "arigatō", "arigatou"),
        ("がっこう", "gakkō", "gakkou"),
        ("しんよう", "shin'yō", "shin'you"),
        ("コーヒー", "kōhī", "koohii"),
        ("ラーメン", "rāmen", "raamen"),
        ("サッカー", "sakkā", "sakkaa"),
    ];
    for (input, verbora_expected, wana_kana_expected) in cases {
        let verbora_output = transliterate_ja(input);
        let wana_kana_output = input.to_romaji();
        assert_eq!(
            verbora_output.as_ref(),
            verbora_expected,
            "Verbora's convention changed on {input:?}"
        );
        assert_eq!(
            wana_kana_output, wana_kana_expected,
            "wana_kana's convention changed on {input:?}"
        );
        assert_ne!(
            verbora_output.as_ref(),
            wana_kana_output,
            "the two transliterators unexpectedly agree on {input:?}"
        );
    }
}

/// The divergence's executed *inner boundary*: Verbora's macrons cover
/// u-lengthened and chōonpu vowels, not every phonetically long vowel —
/// doubled `a`/`o` written with the kana vowels `あ`/`お` and `ei`
/// sequences come out letter-doubled on the Verbora side too, i.e.
/// *identical* to `wana_kana`. This does not narrow the documented
/// divergence (the matrix's cited shapes still differ, asserted above); it
/// stops "different convention" from being misread as "no output ever
/// matches".
#[test]
fn doubled_a_o_and_ei_sequences_agree_exactly() {
    let cases = [
        ("おかあさん", "okaasan"),
        ("おおきい", "ookii"),
        ("せんせい", "sensei"),
    ];
    for (input, expected) in cases {
        let verbora_output = transliterate_ja(input);
        let wana_kana_output = input.to_romaji();
        assert_eq!(verbora_output.as_ref(), expected, "{input:?}");
        assert_eq!(wana_kana_output, expected, "{input:?}");
    }
}

/// The agreement domain proper: plain kana with no long vowel in sight —
/// basic syllables, sokuon before k/s/p (both sides double the consonant),
/// `ん` before y/vowel (both sides emit the disambiguating apostrophe),
/// word-final `ん`, and katakana loanwords without chōonpu. Both outputs
/// are asserted equal *and* against the concrete expected romaji, so this
/// keeps meaning something even if both sides changed together.
#[test]
fn plain_kana_without_long_vowels_agrees_exactly() {
    let cases = [
        ("にほん", "nihon"),
        ("さくら", "sakura"),
        ("すし", "sushi"),
        ("ねこ", "neko"),
        ("はな", "hana"),
        ("やま", "yama"),
        ("こんにちは", "konnichiha"),
        ("ざっし", "zasshi"),
        ("きっぷ", "kippu"),
        ("ほんや", "hon'ya"),
        ("きんえん", "kin'en"),
        ("ほんい", "hon'i"),
        ("テスト", "tesuto"),
        ("カメラ", "kamera"),
        ("ホテル", "hoteru"),
        ("バナナ", "banana"),
        ("ドッグ", "doggu"),
    ];
    for (input, expected) in cases {
        let verbora_output = transliterate_ja(input);
        let wana_kana_output = input.to_romaji();
        assert_eq!(verbora_output.as_ref(), expected, "{input:?}");
        assert_eq!(wana_kana_output, expected, "{input:?}");
    }
}

/// Divergence dimensions beyond long vowels, each pinned with both concrete
/// outputs (see the module doc comment):
///
/// - `を`: Verbora romanizes the particle kana as `o` (modified Hepburn);
///   `wana_kana` emits literal `wo`.
/// - Sokuon before the d-row: the reference's rule table has no `ッ`+d
///   doubling rule, so its final-sokuon pass emits `t` (`betdo`) — a
///   faithfully-ported reference convention (see
///   `crates/verbora-transliterators/src/ja.rs`, `Phase::FinalSokuon`),
///   vs. `wana_kana`'s doubling (`beddo`). Note the contrast with `ッ`+g
///   (`ドッグ` above), where both sides double — the divergence is
///   row-specific, not sokuon-wide.
/// - Fullwidth punctuation `、`/`。`: Verbora passes it through untouched;
///   `wana_kana` rewrites it to ASCII `,`/`.`.
/// - Half-width katakana: Verbora passes it through (outside its tables);
///   `wana_kana` converts it, including voicing marks.
#[test]
fn particle_wo_sokuon_d_row_punctuation_and_halfwidth_kana_also_diverge() {
    let cases = [
        // (input, verbora, wana_kana)
        ("を", "o", "wo"),
        ("をとこ", "otoko", "wotoko"),
        ("とを", "too", "towo"),
        ("ベッド", "betdo", "beddo"),
        ("グッド", "gutdo", "guddo"),
        ("、", "、", ","),
        ("。", "。", "."),
        (
            "とうきょう、にっぽん。",
            "tōkyō、nippon。",
            "toukyou,nippon.",
        ),
        ("ｶﾀｶﾅ", "ｶﾀｶﾅ", "katakana"),
        ("ﾊ\u{ff9e}", "ﾊ\u{ff9e}", "ba"),
    ];
    for (input, verbora_expected, wana_kana_expected) in cases {
        let verbora_output = transliterate_ja(input);
        let wana_kana_output = input.to_romaji();
        assert_eq!(
            verbora_output.as_ref(),
            verbora_expected,
            "Verbora's convention changed on {input:?}"
        );
        assert_eq!(
            wana_kana_output, wana_kana_expected,
            "wana_kana's convention changed on {input:?}"
        );
        assert_ne!(
            verbora_output.as_ref(),
            wana_kana_output,
            "the two transliterators unexpectedly agree on {input:?}"
        );
    }
}

/// The pass-through domain both sides share (the matrix's own "kanji/Latin
/// pass-through" note, executed): ASCII text, kanji-only text, and the
/// empty string come back byte-identical from both implementations, and
/// mixed kanji+kana prose agrees wherever no divergent shape (long vowel,
/// `を`, punctuation) is present.
#[test]
fn kanji_and_latin_pass_through_agrees_exactly() {
    let identity_cases = ["hello", "abc 123", "", "漢字"];
    for input in identity_cases {
        assert_eq!(transliterate_ja(input).as_ref(), input, "verbora");
        assert_eq!(input.to_romaji(), input, "wana_kana");
    }
    // Mixed prose: kanji passed through, kana converted, no divergent
    // shape present -- the two agree on the whole string.
    let mixed = "日本語のテキスト";
    let expected = "日本語notekisuto";
    assert_eq!(transliterate_ja(mixed).as_ref(), expected);
    assert_eq!(mixed.to_romaji(), expected);
}

/// Every input shape `benches/language.rs`'s expanded transliteration
/// groups actually time — the size sweep's `kana_prose` repetitions up to
/// x1024 and all four `transliteration_ja_by_shape` shapes (baseline
/// prose, the dataset paragraph's hiragana extract, the katakana +0x60
/// shift of the prose, and the dataset's kanji-heavy paragraph itself,
/// each repeated to the group's ~4 KiB budget) — runs to completion on
/// both sides, with non-empty output on non-empty input. Same rationale
/// as `both_implementations_run_to_completion_on_a_range_of_kana_input`,
/// extended to the new benchmarked inputs.
#[test]
fn every_expanded_bench_input_shape_runs_to_completion() {
    let prose = "とうきょうとっきょきょかきょくのぼーじょれーぬーゔぉー";
    let dataset = competitive_rust::language_support::load_dataset();
    let ja = dataset
        .iter()
        .find(|l| l.iso639_1 == "ja")
        .expect("japanese is in the dataset");
    let paragraph = ja.items.get("paragraph");
    let hiragana_pure: String = paragraph
        .chars()
        .filter(|c| ('\u{3041}'..='\u{3096}').contains(c))
        .collect();
    let katakana_choonpu: String = prose
        .chars()
        .map(|c| {
            if ('\u{3041}'..='\u{3096}').contains(&c) {
                char::from_u32(c as u32 + 0x60).expect("hiragana +0x60 is always valid katakana")
            } else {
                c
            }
        })
        .collect();

    let mut inputs = vec![
        prose.repeat(4),
        prose.repeat(64),
        prose.repeat(1024),
        hiragana_pure.clone(),
        katakana_choonpu.clone(),
        paragraph.to_owned(),
    ];
    for base in [hiragana_pure.as_str(), katakana_choonpu.as_str(), paragraph] {
        inputs.push(base.repeat(4096_usize.div_ceil(base.len())));
    }
    for input in &inputs {
        assert!(
            !transliterate_ja(input).is_empty(),
            "verbora emitted nothing"
        );
        assert!(!input.to_romaji().is_empty(), "wana_kana emitted nothing");
    }
}
