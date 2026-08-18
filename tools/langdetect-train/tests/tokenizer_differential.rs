//! Differential test for the shared feature extractor.
//!
//! `verbora_language::train_support::hashed_features` is the one function
//! both training and inference run. This test pins it against an
//! independent reference implementation of the `whichlang` v0.1.1 feature
//! definition — the same reference the decomposition analysis verified
//! bit-exact against `whichlang::detect_language` itself on 20,052 cases
//! (all 52 UDHR dataset items plus 20,000 randomized mixed-script
//! strings, zero mismatches) before this pipeline was built. If the
//! shipped extractor ever drifts from this reference, training and any
//! model trained earlier silently disagree — which is exactly the class
//! of bug a differential test exists to catch.

use langdetect_train::XorShift64;
use verbora_language::train_support::{DIMENSION, hashed_features, hashed_features_cyrillic};

// --------------------------------------------------------------- reference

const SEED: u32 = 3_242_157_231;
const BIGRAM_MASK: u32 = (1 << 16) - 1;
const TRIGRAM_MASK: u32 = (1 << 24) - 1;

fn murmurhash2(mut k: u32, seed: u32) -> u32 {
    const M: u32 = 0x5bd1_e995;
    let mut h: u32 = seed;
    k = k.wrapping_mul(M);
    k ^= k >> 24;
    k = k.wrapping_mul(M);
    h = h.wrapping_mul(M);
    h ^= k;
    h ^= h >> 13;
    h = h.wrapping_mul(M);
    h ^ (h >> 15)
}

const JP_RANGES: [u32; 10] = [
    0x3000, 0x303f, 0x3040, 0x309f, 0x30a0, 0x30ff, 0x4e00, 0x9faf, 0xff61, 0xff90,
];

fn classify_codepoint(chr: char) -> u32 {
    [
        160u32,
        161,
        171,
        172,
        173,
        174,
        187,
        192,
        196,
        199,
        200,
        201,
        202,
        205,
        214,
        220,
        223,
        224,
        225,
        226,
        227,
        228,
        231,
        232,
        233,
        234,
        235,
        236,
        237,
        238,
        239,
        242,
        243,
        244,
        245,
        246,
        249,
        250,
        251,
        252,
        333,
        339,
        JP_RANGES[0],
        JP_RANGES[1],
        JP_RANGES[2],
        JP_RANGES[3],
        JP_RANGES[4],
        JP_RANGES[5],
        JP_RANGES[6],
        JP_RANGES[7],
        JP_RANGES[8],
        JP_RANGES[9],
    ]
    .binary_search(&(chr as u32))
    .unwrap_or_else(|pos| pos) as u32
}

/// The reference feature stream: `whichlang`'s `emit_tokens` shape,
/// hashed and bucketed the way inference does it.
fn reference_buckets(text: &str) -> Vec<u32> {
    let mut out = Vec::new();
    let mut prev = ' ' as u32;
    let mut num_previous_ascii_chr = 1;
    for chr in text.chars() {
        let code = chr.to_ascii_lowercase() as u32;
        if !chr.is_ascii() {
            out.push(murmurhash2(chr as u32 / 128, SEED ^ 2) % DIMENSION as u32);
            out.push(murmurhash2(classify_codepoint(chr), SEED ^ 4) % DIMENSION as u32);
            num_previous_ascii_chr = 0;
            continue;
        }
        prev = prev << 8 | code;
        match num_previous_ascii_chr {
            0 => num_previous_ascii_chr = 1,
            1 => {
                out.push(murmurhash2(prev & BIGRAM_MASK, SEED) % DIMENSION as u32);
                num_previous_ascii_chr = 2;
            }
            2 => {
                out.push(murmurhash2(prev & BIGRAM_MASK, SEED) % DIMENSION as u32);
                out.push(murmurhash2(prev & TRIGRAM_MASK, SEED) % DIMENSION as u32);
                num_previous_ascii_chr = 3;
            }
            _ => {
                out.push(murmurhash2(prev & BIGRAM_MASK, SEED) % DIMENSION as u32);
                out.push(murmurhash2(prev & TRIGRAM_MASK, SEED) % DIMENSION as u32);
                out.push(murmurhash2(prev, SEED) % DIMENSION as u32);
            }
        }
        if !chr.is_alphanumeric() {
            prev = ' ' as u32;
        }
    }
    out
}

fn shipped_buckets(text: &str) -> Vec<u32> {
    let mut out = Vec::new();
    hashed_features(text, |b| out.push(b));
    out
}

/// Random mixed-script text: ASCII words, accented Latin, Cyrillic, kana,
/// Han — the same generator shape the original 20k-case verification
/// used.
fn rand_text(rng: &mut XorShift64, max_len: usize) -> String {
    let mut s = String::new();
    let len = rng.next_below(max_len) + 1;
    for _ in 0..len {
        let c = match rng.next_below(10) {
            0..=4 => (b'a' + rng.next_below(26) as u8) as char,
            5 => ' ',
            6 => char::from_u32(0x00C0 + rng.next_below(0x2AF - 0xC0) as u32).unwrap_or('é'),
            7 => char::from_u32(0x0400 + rng.next_below(0x130) as u32).unwrap_or('ж'),
            8 => char::from_u32(0x3040 + rng.next_below(0xC0) as u32).unwrap_or('あ'),
            _ => char::from_u32(0x4E00 + rng.next_below(0x51FF) as u32).unwrap_or('中'),
        };
        s.push(c);
    }
    s
}

// ------------------------------------------------------------------- tests

#[test]
fn shipped_extractor_matches_reference_on_fixtures() {
    for text in [
        "",
        "a",
        "ab",
        "abc",
        "abcd",
        "hello world",
        "Hello, World! 123",
        "café müller straße",
        "Это русское предложение",
        "これはにほんごです",
        "日本語と中文が混ざった text",
        "word-with-hyphens and.dots",
        "ALL CAPS TEXT",
        "a b c d e f",
        "\u{0}\u{7F}",
        "😀 emoji 😀 mixed",
    ] {
        assert_eq!(
            shipped_buckets(text),
            reference_buckets(text),
            "feature stream mismatch on fixture {text:?}"
        );
    }
}

#[test]
fn shipped_extractor_matches_reference_on_randomized_corpus() {
    let mut rng = XorShift64::new(0xDEAD_BEEF_CAFE);
    for case in 0..20_000u32 {
        let text = rand_text(&mut rng, 120);
        assert_eq!(
            shipped_buckets(&text),
            reference_buckets(&text),
            "feature stream mismatch on random case {case}: {text:?}"
        );
    }
}

// -------------------------------------------- cyrillic feature reference

/// Independent restatement of the Cyrillic feature definition (case-folded
/// codepoint unigrams + in-word bigrams). Unlike the whichlang-shape
/// reference above this has no external oracle — its value is pinning the
/// shipped extractor against accidental drift: any change to
/// `hashed_features_cyrillic` invalidates the compiled Cyrillic weights,
/// so a change here must never happen silently.
fn reference_cyrillic_buckets(text: &str) -> Vec<u32> {
    fn fold(chr: char) -> u32 {
        let cp = chr as u32;
        match cp {
            0x0410..=0x042F => cp + 0x20,
            0x0400..=0x040F => cp + 0x50,
            _ => chr.to_ascii_lowercase() as u32,
        }
    }
    let mut out = Vec::new();
    let mut prev: u32 = 0;
    for chr in text.chars() {
        if !chr.is_alphabetic() {
            prev = 0;
            continue;
        }
        let folded = fold(chr);
        out.push(murmurhash2(folded, SEED ^ 8) % DIMENSION as u32);
        if prev != 0 {
            out.push(murmurhash2(prev.wrapping_mul(65_599) ^ folded, SEED ^ 16) % DIMENSION as u32);
        }
        prev = folded;
    }
    out
}

fn shipped_cyrillic_buckets(text: &str) -> Vec<u32> {
    let mut out = Vec::new();
    hashed_features_cyrillic(text, |b| out.push(b));
    out
}

#[test]
fn shipped_cyrillic_extractor_matches_reference() {
    let fixtures = [
        "",
        "Это русское предложение о погоде.",
        "Сьогодні чудова погода, і діти граються надворі!",
        "ЁЄІЇҐ ыэъё",
        "смешанный text із latin",
        "а",
        "12345 !!!",
    ];
    for text in fixtures {
        assert_eq!(
            shipped_cyrillic_buckets(text),
            reference_cyrillic_buckets(text),
            "cyrillic feature stream mismatch on {text:?}"
        );
    }
    let mut rng = XorShift64::new(0xC0FF_EE11);
    for case in 0..20_000u32 {
        let text = rand_text(&mut rng, 120);
        assert_eq!(
            shipped_cyrillic_buckets(&text),
            reference_cyrillic_buckets(&text),
            "cyrillic feature stream mismatch on random case {case}: {text:?}"
        );
    }
}

#[test]
fn every_bucket_is_in_range() {
    let mut rng = XorShift64::new(0x1234_5678);
    for _ in 0..2_000u32 {
        let text = rand_text(&mut rng, 80);
        for b in shipped_buckets(&text) {
            assert!(
                (b as usize) < DIMENSION,
                "bucket {b} out of range for {text:?}"
            );
        }
    }
}
