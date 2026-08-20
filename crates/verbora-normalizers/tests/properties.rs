//! The properties `remove_diacritics` promises, and the fixtures whose
//! expected values are derived from the Unicode Character Database rather than
//! from running this code.
//!
//! Every derivation is stated inline: field 5 of `UnicodeData.txt` is
//! `Decomposition_Mapping` and field 3 is `Canonical_Combining_Class`, and the
//! `<tag>` prefix on a mapping marks it as *compatibility* rather than
//! canonical. Where a fixture asserts a fold, the mapping that produces it is
//! quoted; where it asserts no fold, the absent or tagged mapping is quoted.

use std::borrow::Cow;

use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::canonical_combining_class;
use verbora_normalizers::{nfc, nfd, nfkc, nfkd, remove_diacritics};

/// Text spanning the scripts whose behaviour under this fold differs, plus the
/// shapes that break naive implementations: decomposed and precomposed spellings
/// of the same word, repeated accents, mark-only strings, astral scalars, and
/// the two scalars every whitespace set disagrees about.
const MIXED: &[&str] = &[
    "",
    "a",
    "the quick brown fox jumps over the lazy dog",
    "3.14 1,000 don't node_js a:b",
    // Latin, precomposed and decomposed spellings of the same words.
    "café naïve crème brûlée Ångström coração",
    "cafe\u{0301} nai\u{0308}ve cre\u{0300}me bru\u{0302}le\u{0301}e",
    "ààà",
    "ÀÁÂÃÄÅÇÈÉÊËÌÍÎÏÑÒÓÔÕÖÙÚÛÜÝ",
    "ø Æ đ ł ħ ŋ ı ß ſ ǅ Ａ Ⓐ",
    "blåbærsyltetøy à la façon",
    // Greek and Cyrillic.
    "Ελλάδα ἀγαθός",
    "Москва не сразу строилась Ї ї Й й",
    // Hebrew with niqqud: ccc 10-26.
    "שָׁלוֹם עֲלֵיכֶם",
    "שלום עליכם",
    // Arabic with harakat: ccc 27-34.
    "بِسْمِ اللَّهِ",
    "بسم الله",
    // Devanagari: matras are ccc 0, the virama is ccc 9 and the nukta ccc 7.
    "भारत",
    "हिन्दी",
    "क\u{093C}",
    // Thai: SARA I is ccc 0, MAI EK is ccc 107, SARA U is ccc 103.
    "กิน",
    "ไทย",
    "ก\u{0E48}",
    "ก\u{0E38}",
    // Hangul, precomposed and as conjoining jamo.
    "한국어",
    "\u{1112}\u{1161}\u{11AB}\u{1100}\u{116E}\u{11A8}",
    // CJK, kana, halfwidth and fullwidth forms.
    "日本語 ｶﾀｶﾅ ＡＢＣ１２３ 時々刻々 ㌔",
    // Astral: emoji, mathematical alphanumerics, and combining musical symbols
    // (U+1D167..=U+1D169 are ccc 1 — the class a BMP-only sweep cannot reach).
    "😀 a😀é 𝕳𝖊𝖑𝖑𝖔",
    "a\u{1D167}\u{1D16D}b",
    // Marks with no base, and text that begins with one.
    "\u{0301}",
    "\u{0301}\u{0300}\u{0327}",
    "\u{0323}\u{0307}",
    // The two scalars ECMAScript and Unicode disagree about as whitespace.
    "\u{feff}",
    "\u{0085}",
    "a\r\nb",
    // Out of canonical order on input.
    "\u{1E0A}\u{0323}",
    "q\u{0307}\u{0323}",
];

/// Latin, Greek, Cyrillic, Hebrew and Arabic text only. No canonical
/// composition pair in these scripts has a `ccc = 0` second element, so
/// concatenation across a split needs no recomposition and the
/// position-independence property reads as plain string concatenation.
const NO_STARTER_COMPOSITION: &[&str] = &[
    "café naïve",
    "cafe\u{0301} nai\u{0308}ve",
    "ààà",
    "Ångström",
    "Ελλάδα",
    "Москва",
    "שָׁלוֹם עֲלֵיכֶם",
    "بِسْمِ اللَّهِ",
    "résumé résumé résumé",
];

// --------------------------------------------------------------------------
// Fixtures: expected values derived from the UCD, one derivation per row.
// --------------------------------------------------------------------------

/// Letters whose mark is part of their identity: `Decomposition_Mapping` is
/// empty, so NFD leaves them whole and there is nothing with a non-zero
/// combining class to remove.
#[test]
fn letters_without_a_canonical_decomposition_survive() {
    for (s, why) in [
        ("ø", "U+00F8 LATIN SMALL LETTER O WITH STROKE, no mapping"),
        ("Æ", "U+00C6 LATIN CAPITAL LETTER AE, no mapping"),
        ("đ", "U+0111 LATIN SMALL LETTER D WITH STROKE, no mapping"),
        ("ł", "U+0142 LATIN SMALL LETTER L WITH STROKE, no mapping"),
        ("ħ", "U+0127 LATIN SMALL LETTER H WITH STROKE, no mapping"),
        ("ŋ", "U+014B LATIN SMALL LETTER ENG, no mapping"),
        ("ı", "U+0131 LATIN SMALL LETTER DOTLESS I, no mapping"),
        ("ß", "U+00DF LATIN SMALL LETTER SHARP S, no mapping"),
    ] {
        let got = remove_diacritics(s);
        assert_eq!(got, s, "{why}");
        assert!(matches!(got, Cow::Borrowed(_)), "{why}");
    }
}

/// Compatibility mappings are not canonical mappings, so NFD does not apply
/// them and this function never does either. `nfkc` is the composition a
/// caller who wants them writes explicitly.
#[test]
fn compatibility_decompositions_are_not_applied() {
    for (s, mapping) in [
        ("Ａ", "U+FF21 -> <wide> 0041"),
        ("Ⓐ", "U+24B6 -> <circle> 0041"),
        ("ǅ", "U+01C5 -> <compat> 0044 017E"),
        ("ſ", "U+017F -> <compat> 0073"),
        ("ﬁ", "U+FB01 -> <compat> 0066 0069"),
    ] {
        assert_eq!(remove_diacritics(s), s, "{mapping}");
    }
    // …and the caller who wants them says so.
    assert_eq!(remove_diacritics(&nfkc("Ａ")), "A");
    assert_eq!(remove_diacritics(&nfkc("ǅ")), "Dz");
}

/// Canonical mappings, including the singleton and the recursive case.
#[test]
fn canonical_decompositions_are_applied() {
    // U+212B ANGSTROM SIGN -> 00C5, and U+00C5 -> 0041 030A with ccc(030A)=230.
    assert_eq!(remove_diacritics("Å"), "A");
    assert_eq!(remove_diacritics("\u{212B}"), "A");
    // U+0130 LATIN CAPITAL LETTER I WITH DOT ABOVE -> 0049 0307, ccc(0307)=230.
    assert_eq!(remove_diacritics("İ"), "I");
    // U+00E5 -> 0061 030A, U+00F6 -> 006F 0308, U+00FC -> 0075 0308:
    // the Nordic vowels are ordinary precomposed letters and do fold.
    assert_eq!(remove_diacritics("åäöü"), "aaou");
}

/// The scripts the `ccc != 0` rule was chosen for, and the scripts it costs.
///
/// A Latin-only fixture set cannot tell the `ccc != 0` rule apart from
/// "strip every `General_Category` mark", which is the design decision the
/// whole definition rests on.
#[test]
fn script_behaviour() {
    // Kept: every mark here is ccc 0.
    // Devanagari matras U+093E..U+094C, Thai SARA I U+0E34, Hangul jamo.
    for s in ["भारत", "กิน", "ไทย", "한국어", "日本語", "Москва"] {
        let got = remove_diacritics(s);
        assert_eq!(got, s, "{s:?} should be untouched");
        assert!(matches!(got, Cow::Borrowed(_)), "{s:?}");
    }

    // Removed: Hebrew niqqud (ccc 10-26).
    // U+05B8 QAMATS ccc 18, U+05C1 SHIN DOT ccc 24, U+05B9 HOLAM ccc 19.
    assert_eq!(remove_diacritics("שָׁלוֹם"), "שלום");
    // Removed: Arabic harakat (ccc 27-34).
    // U+0650 KASRA ccc 32, U+0652 SUKUN ccc 34, U+0651 SHADDA ccc 33.
    assert_eq!(remove_diacritics("بِسْمِ"), "بسم");
    // Removed: Devanagari nukta, ccc 7.
    assert_eq!(remove_diacritics("क\u{093C}"), "क");

    // Removed, and this is the honest cost rather than a bug: these are not
    // accents in any editorial sense, but their combining class is non-zero.
    // U+094D DEVANAGARI SIGN VIRAMA, ccc 9 — dropping it un-conjoins the word.
    assert_eq!(remove_diacritics("हिन्दी"), "हिनदी");
    // U+0E48 THAI CHARACTER MAI EK, ccc 107 — a tone mark, not an accent.
    assert_eq!(remove_diacritics("ก\u{0E48}"), "ก");
    // U+0E38 THAI CHARACTER SARA U, ccc 103 — a vowel sign, and it goes.
    assert_eq!(remove_diacritics("ก\u{0E38}"), "ก");
}

/// The defect the old first-occurrence folders had, pinned by a fixture a
/// single-occurrence test cannot reach.
#[test]
fn every_occurrence_folds() {
    assert_eq!(remove_diacritics("ààà"), "aaa");
    assert_eq!(remove_diacritics("ééé àà"), "eee aa");
    assert_eq!(remove_diacritics("àÀàÀ"), "aAaA");
    let long = "é".repeat(10_000);
    assert_eq!(remove_diacritics(&long), "e".repeat(10_000));
}

// --------------------------------------------------------------------------
// Properties.
// --------------------------------------------------------------------------

/// Idempotence, over every Unicode scalar value.
///
/// The exhaustive sweep is deliberately not BMP-only: `U+1D167..=U+1D169`
/// MUSICAL SYMBOL COMBINING TREMOLO have `Canonical_Combining_Class` 1, and a
/// BMP-only sweep never touches an astral combining mark at all.
#[test]
fn idempotent_over_every_scalar() {
    let mut swept = 0u32;
    for cp in 0..=0x10_FFFFu32 {
        let Some(c) = char::from_u32(cp) else {
            continue;
        };
        let s = c.to_string();
        let once = remove_diacritics(&s);
        let twice = remove_diacritics(&once);
        assert_eq!(twice, once, "U+{cp:04X}");
        swept += 1;
    }
    assert!(swept > 1_000_000, "swept only {swept} scalars");
}

/// Idempotence over multi-scalar text, where composition and canonical
/// ordering both have something to do.
#[test]
fn idempotent_over_the_corpus() {
    for s in MIXED {
        let once = remove_diacritics(s);
        assert_eq!(remove_diacritics(&once), once, "{s:?}");
    }
}

/// The output is in NFC and holds no scalar with a non-zero combining class —
/// the two structural facts the idempotence and position-independence proofs
/// both rest on.
#[test]
fn output_is_composed_and_mark_free() {
    for s in MIXED {
        let got = remove_diacritics(s);
        assert!(
            matches!(nfc(&got), Cow::Borrowed(_)),
            "{s:?} -> {got:?} is not in NFC"
        );
        assert!(
            got.chars().all(|c| canonical_combining_class(c) == 0),
            "{s:?} -> {got:?} still holds a non-starter"
        );
    }
}

/// The answer does not depend on how the caller typed the text.
///
/// The current fixtures for the old table-driven folder were all precomposed,
/// which is exactly the class that hid its dependence on input spelling:
/// it folded `"é"` and left `"e\u{0301}"` alone.
#[test]
fn independent_of_the_input_normalization_form() {
    for s in MIXED {
        let plain = remove_diacritics(s);
        assert_eq!(remove_diacritics(&nfd(s)), plain, "nfd disagreed on {s:?}");
        assert_eq!(remove_diacritics(&nfc(s)), plain, "nfc disagreed on {s:?}");
    }
    // The worked example, spelled out.
    assert_eq!(remove_diacritics("é"), "e");
    assert_eq!(remove_diacritics("e\u{0301}"), "e");
}

/// Position independence, in the exact form the function documents.
///
/// Split `s` anywhere that `nfd` of the tail begins with a `ccc = 0` scalar;
/// then `remove_diacritics(s)` is `nfc` of the two halves' results joined.
/// The `nfc` is load-bearing: canonical composition can act across the join
/// between two `ccc = 0` scalars — Hangul `L` + `V` is the case in this
/// corpus — so plain concatenation is not equal there.
#[test]
fn position_independent_up_to_composition_at_the_join() {
    for s in MIXED {
        let whole = remove_diacritics(s);
        for i in 0..=s.len() {
            if !s.is_char_boundary(i) {
                continue;
            }
            let (a, b) = s.split_at(i);
            if b.nfd()
                .next()
                .is_some_and(|c| canonical_combining_class(c) != 0)
            {
                continue; // the split would cut into a combining sequence
            }
            let joined = format!("{}{}", remove_diacritics(a), remove_diacritics(b));
            assert_eq!(nfc(&joined), whole, "{s:?} split at {i}");
        }
    }
}

/// …and where no canonical composition pair has a `ccc = 0` second element —
/// all Latin, Greek, Cyrillic, Hebrew and Arabic text — the same property
/// reads as plain concatenation: the same word folds the same way wherever it
/// appears in a document.
#[test]
fn position_independent_by_plain_concatenation_in_alphabetic_scripts() {
    for s in NO_STARTER_COMPOSITION {
        let whole = remove_diacritics(s);
        for i in 0..=s.len() {
            if !s.is_char_boundary(i) {
                continue;
            }
            let (a, b) = s.split_at(i);
            if b.nfd()
                .next()
                .is_some_and(|c| canonical_combining_class(c) != 0)
            {
                continue;
            }
            let joined = format!("{}{}", remove_diacritics(a), remove_diacritics(b));
            assert_eq!(joined, whole, "{s:?} split at {i}");
        }
    }
}

/// Hangul is the exception plain concatenation has, and it is pinned so the
/// documented caveat cannot quietly become false in either direction.
#[test]
fn hangul_jamo_compose_across_a_join() {
    let l = "\u{1100}"; // HANGUL CHOSEONG KIYEOK, ccc 0
    let v = "\u{1161}"; // HANGUL JUNGSEONG A, ccc 0
    let s = format!("{l}{v}");
    assert_eq!(remove_diacritics(&s), "가");
    // Folded separately, the two halves stay apart …
    let joined = format!("{}{}", remove_diacritics(l), remove_diacritics(v));
    assert_eq!(joined, s);
    assert_ne!(joined, "가");
    // … and `nfc` of the join is what closes the gap.
    assert_eq!(nfc(&joined), "가");
}

/// The `Cow` iff-guarantee for all five functions over the whole corpus.
#[test]
fn cow_borrows_exactly_when_nothing_changed() {
    for s in MIXED.iter().chain(NO_STARTER_COMPOSITION) {
        for (name, got) in [
            ("nfc", nfc(s)),
            ("nfd", nfd(s)),
            ("nfkc", nfkc(s)),
            ("nfkd", nfkd(s)),
            ("remove_diacritics", remove_diacritics(s)),
        ] {
            assert_eq!(
                matches!(got, Cow::Borrowed(_)),
                got.as_ref() == *s,
                "{name}({s:?}) borrowed-ness disagrees with equality"
            );
        }
    }
}

/// Nothing in this crate fabricates a replacement character.
#[test]
fn u_fffd_appears_only_if_the_input_had_one() {
    for s in MIXED {
        for got in [nfc(s), nfd(s), nfkc(s), nfkd(s), remove_diacritics(s)] {
            assert_eq!(
                got.contains('\u{FFFD}'),
                s.contains('\u{FFFD}'),
                "{s:?} -> {got:?}"
            );
        }
    }
    assert_eq!(nfkc("😀々"), "😀々");
    assert_eq!(remove_diacritics("👍你好"), "👍你好");
}

/// Sequential-vs-parallel equivalence, over the inputs the sequential suite
/// above already exercises rather than a fresh set of edge cases.
#[cfg(feature = "parallel")]
mod parallel_parity {
    use super::{MIXED, NO_STARTER_COMPOSITION};
    use std::borrow::Cow;
    use verbora_normalizers::{par_remove_diacritics_batch, remove_diacritics};

    fn assert_parity(inputs: &[&str]) {
        let sequential: Vec<Cow<'_, str>> = inputs.iter().map(|s| remove_diacritics(s)).collect();
        assert_eq!(
            par_remove_diacritics_batch(inputs),
            sequential,
            "batch of {} inputs diverged from the sequential loop",
            inputs.len()
        );
    }

    #[test]
    fn empty_one_and_many() {
        assert_parity(&[]);
        assert_parity(&["café"]);
        assert_parity(&[""]);
        assert_parity(MIXED);
        assert_parity(NO_STARTER_COMPOSITION);
    }

    #[test]
    fn order_is_preserved_well_past_any_task_count() {
        let inputs: Vec<&str> = MIXED.iter().copied().cycle().take(2_000).collect();
        assert_parity(&inputs);
    }

    #[test]
    fn pathological_lengths() {
        let long = "é".repeat(10_000);
        let cyrillic = "Москва не сразу строилась ".repeat(40);
        let hebrew = "שָׁלוֹם ".repeat(500);
        let inputs: Vec<&str> = vec![long.as_str(), cyrillic.as_str(), hebrew.as_str(), "é", ""];
        assert_parity(&inputs);
    }
}
