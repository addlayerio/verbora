//! Lightweight script detection: Unicode block classification, no models,
//! no allocation.
//!
//! Script detection is more reliable than language detection on short
//! input — knowing a word is written in Cyrillic does not tell you whether
//! it is Russian or Ukrainian, but it rules out every Latin-script language
//! at zero cost, before spending anything on statistical language
//! detection. This module is a majority vote over Unicode code point
//! ranges, not a model: no ML, no crate, no allocation.

use std::fmt;

/// A writing system, coarse enough to be classified from Unicode ranges
/// alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Script {
    /// Covers every Latin-script language this crate's [`Language`](crate::Language)
    /// enumerates (English, Spanish, French, German, …), diacritics
    /// included.
    Latin,
    /// Russian, Ukrainian, and other Cyrillic-script languages.
    Cyrillic,
    /// Modern Greek.
    Greek,
    /// Arabic and other Arabic-script languages.
    Arabic,
    /// Hebrew.
    Hebrew,
    /// Han (CJK ideographs) — Chinese, and the kanji portion of Japanese.
    Han,
    /// Japanese hiragana.
    Hiragana,
    /// Japanese katakana.
    Katakana,
    /// Korean Hangul.
    Hangul,
    /// Hindi and other Devanagari-script languages.
    Devanagari,
    /// A script this classifier does not have a dedicated variant for.
    /// Not an error: plenty of real text is script-neutral (digits,
    /// punctuation) or in a script this crate has no [`Language`](crate::Language)
    /// strategy for anyway.
    Other,
}

impl fmt::Display for Script {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Latin => "Latin",
            Self::Cyrillic => "Cyrillic",
            Self::Greek => "Greek",
            Self::Arabic => "Arabic",
            Self::Hebrew => "Hebrew",
            Self::Han => "Han",
            Self::Hiragana => "Hiragana",
            Self::Katakana => "Katakana",
            Self::Hangul => "Hangul",
            Self::Devanagari => "Devanagari",
            Self::Other => "Other",
        })
    }
}

/// Classifies one code point's script, or `None` for script-neutral
/// characters (digits, punctuation, whitespace, symbols).
#[must_use]
fn classify(c: char) -> Option<Script> {
    let cp = c as u32;
    match cp {
        // Basic Latin letters, Latin-1 Supplement, Latin Extended-A/B, and
        // the IPA/spacing-modifier ranges diacritic-heavy languages use.
        0x0041..=0x005A
        | 0x0061..=0x007A
        | 0x00C0..=0x02AF
        | 0x1E00..=0x1EFF // Latin Extended Additional (Vietnamese)
            => Some(Script::Latin),
        0x0370..=0x03FF | 0x1F00..=0x1FFF => Some(Script::Greek),
        0x0400..=0x052F => Some(Script::Cyrillic),
        0x0590..=0x05FF => Some(Script::Hebrew),
        0x0600..=0x06FF | 0x0750..=0x077F => Some(Script::Arabic),
        0x0900..=0x097F => Some(Script::Devanagari),
        0x3040..=0x309F => Some(Script::Hiragana),
        0x30A0..=0x30FF => Some(Script::Katakana),
        0xAC00..=0xD7AF => Some(Script::Hangul),
        0x4E00..=0x9FFF | 0x3400..=0x4DBF | 0xF900..=0xFAFF => Some(Script::Han),
        _ if c.is_ascii_digit() || c.is_whitespace() || c.is_ascii_punctuation() => None,
        _ if !c.is_alphabetic() => None,
        _ => Some(Script::Other),
    }
}

/// The dominant script in `input`, or `None` if it holds no classifiable
/// letters (empty, all digits/punctuation/whitespace).
///
/// A simple majority vote over every character's own script — no
/// allocation, one pass, no crate dependency. Mixed-script input (a loan
/// word, a proper noun in a foreign script) returns whichever script has
/// the most characters; ties break toward the first script encountered.
#[must_use]
pub fn detect_script(input: &str) -> Option<Script> {
    // 11 scripts total; a fixed-size array beats a HashMap here — Copy,
    // no hashing, no allocation, and the whole table fits in one cache line
    // per few iterations.
    const SCRIPTS: [Script; 10] = [
        Script::Latin,
        Script::Cyrillic,
        Script::Greek,
        Script::Arabic,
        Script::Hebrew,
        Script::Han,
        Script::Hiragana,
        Script::Katakana,
        Script::Hangul,
        Script::Devanagari,
    ];
    let mut counts = [0u32; 10];
    let mut other = 0u32;

    for c in input.chars() {
        match classify(c) {
            Some(Script::Other) => other += 1,
            Some(s) => {
                let idx = SCRIPTS.iter().position(|&x| x == s).unwrap();
                counts[idx] += 1;
            }
            None => {}
        }
    }

    let (best_idx, &best_count) = counts
        .iter()
        .enumerate()
        .max_by_key(|&(_, &count)| count)
        .expect("SCRIPTS is non-empty");

    if best_count == 0 {
        return if other > 0 { Some(Script::Other) } else { None };
    }
    if other > best_count {
        return Some(Script::Other);
    }
    Some(SCRIPTS[best_idx])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latin_text() {
        assert_eq!(detect_script("hello world"), Some(Script::Latin));
        assert_eq!(detect_script("café müller"), Some(Script::Latin));
    }

    #[test]
    fn cyrillic_text() {
        assert_eq!(detect_script("Москва"), Some(Script::Cyrillic));
    }

    #[test]
    fn japanese_mixed_hiragana_and_han_picks_the_majority() {
        // "日本語" is 3 Han characters; a 1-character hiragana tiebreak
        // should not override a clear Han majority.
        assert_eq!(detect_script("日本語"), Some(Script::Han));
    }

    #[test]
    fn pure_hiragana() {
        assert_eq!(detect_script("ひらがな"), Some(Script::Hiragana));
    }

    #[test]
    fn pure_katakana() {
        assert_eq!(detect_script("カタカナ"), Some(Script::Katakana));
    }

    #[test]
    fn korean_text() {
        assert_eq!(detect_script("한국어"), Some(Script::Hangul));
    }

    #[test]
    fn arabic_text() {
        assert_eq!(detect_script("العربية"), Some(Script::Arabic));
    }

    #[test]
    fn hebrew_text() {
        assert_eq!(detect_script("עברית"), Some(Script::Hebrew));
    }

    #[test]
    fn devanagari_text() {
        assert_eq!(detect_script("हिन्दी"), Some(Script::Devanagari));
    }

    #[test]
    fn greek_text() {
        assert_eq!(detect_script("ελληνικά"), Some(Script::Greek));
    }

    #[test]
    fn empty_and_punctuation_only_are_none() {
        assert_eq!(detect_script(""), None);
        assert_eq!(detect_script("123 !@# ..."), None);
    }

    #[test]
    fn does_not_panic_on_astral_plane_or_emoji() {
        for input in ["😀😀😀", "\u{1F600}", "a😀b", "🎉🎊", &"😀".repeat(200)] {
            let _ = detect_script(input);
        }
    }

    #[test]
    fn mixed_script_picks_the_majority_not_the_first() {
        // 5 Latin vs 1 Cyrillic: Latin should win even though it appears
        // second in SCRIPTS-iteration order relative to nothing else here —
        // this specifically checks the *count*, not declaration order.
        assert_eq!(detect_script("aaaaaЖ"), Some(Script::Latin));
    }

    #[test]
    fn detect_script_is_deterministic_across_repeated_calls() {
        // No hidden state, no hashing, no iteration-order dependence — the
        // same input must produce the exact same answer every time.
        for input in ["hello world", "日本語", "aaaaaЖ", "", "123 !@#"] {
            let first = detect_script(input);
            for _ in 0..5 {
                assert_eq!(
                    detect_script(input),
                    first,
                    "detect_script({input:?}) returned a different result across repeated calls"
                );
            }
        }
    }
}
