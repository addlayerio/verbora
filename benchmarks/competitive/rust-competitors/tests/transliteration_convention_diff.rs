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
