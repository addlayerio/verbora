//! `CaseTokenizer` — keeps only characters that have a case, plus digits.
//!
//! The reference asks "does this code unit change under `toLowerCase` but not
//! under `toUpperCase`, or vice versa?" as a proxy for "is this a letter", blanks
//! out everything else, and splits on the resulting whitespace. Uncased scripts
//! therefore vanish entirely: `"日本語"` tokenizes to `[]`.

use crate::Tokenize;
use crate::utf16::Utf16Token;
use crate::whitespace::is_whitespace_unit;

/// Splits text on anything that is not a cased letter or an ASCII digit.
///
/// ```
/// use verbora_tokenizers::{CaseTokenizer, Tokenize};
/// let t = CaseTokenizer::new();
/// assert_eq!(t.tokenize("it's"), ["it", "s"]);
/// assert_eq!(CaseTokenizer::preserving_apostrophes().tokenize("it's"), ["it's"]);
/// assert_eq!(t.tokenize("日本語"), Vec::<&str>::new());
/// ```
///
/// # The `"undefined"` bug
///
/// The reference loop is bounded by `lower.length` but indexes into `text`:
///
/// ```text
/// for (i = 0; i < lower.length; ++i) { ... result += text[i] ... }
/// ```
///
/// When lowercasing *lengthens* the string — `İ` (U+0130) lowercases to two code
/// units — `i` runs past the end of `text`, `text[i]` evaluates to `undefined`,
/// and the reference's string concatenation appends the nine characters
/// `undefined`. So `tokenize("İstanbul")` really does return
/// `["İstanbulundefined"]`, and this implementation reproduces it:
///
/// ```
/// use verbora_tokenizers::{CaseTokenizer, Tokenize};
/// assert_eq!(CaseTokenizer::new().tokenize("İstanbul"), ["İstanbulundefined"]);
/// ```
///
/// The mirror case is harmless: when *upper*casing lengthens the string (`ß` →
/// `SS`) the loop bound is unaffected.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CaseTokenizer {
    preserve_apostrophe: bool,
}

impl CaseTokenizer {
    /// Creates a tokenizer that discards apostrophes.
    #[inline]
    pub const fn new() -> Self {
        Self {
            preserve_apostrophe: false,
        }
    }

    /// Creates a tokenizer that keeps the ASCII apostrophe `'` inside tokens.
    ///
    /// The reference takes this as the second argument to `tokenize`; carrying it
    /// on the tokenizer keeps the [`Tokenize`] signature uniform. Only U+0027 is
    /// honoured — the typographic `’` is not.
    #[inline]
    pub const fn preserving_apostrophes() -> Self {
        Self {
            preserve_apostrophe: true,
        }
    }

    /// Whether this tokenizer keeps apostrophes.
    #[inline]
    pub const fn preserves_apostrophes(&self) -> bool {
        self.preserve_apostrophe
    }
}

/// Whether an ASCII byte survives the filter.
///
/// For all-ASCII input the general algorithm collapses to this: `to_lowercase`
/// and `to_uppercase` are per-byte and length-preserving, so they differ exactly
/// on the letters.
#[inline]
const fn ascii_kept(b: u8, preserve_apostrophe: bool) -> bool {
    b.is_ascii_alphanumeric() || (preserve_apostrophe && b == b'\'')
}

/// The literal the reference appends when `text[i]` is out of range.
const UNDEFINED: &[u16] = &[
    b'u' as u16,
    b'n' as u16,
    b'd' as u16,
    b'e' as u16,
    b'f' as u16,
    b'i' as u16,
    b'n' as u16,
    b'e' as u16,
    b'd' as u16,
];

/// Builds the reference's intermediate `result` string as UTF-16 code units.
fn filtered_units(text: &str, preserve_apostrophe: bool) -> Vec<u16> {
    // `to_lowercase`/`to_uppercase` are the full, locale-independent Unicode
    // mappings — the same ones `String.prototype.toLowerCase` uses. The simple
    // per-character mappings would not reproduce the length changes that drive
    // the `"undefined"` bug.
    let lower: Vec<u16> = text.to_lowercase().encode_utf16().collect();
    let upper: Vec<u16> = text.to_uppercase().encode_utf16().collect();
    let src: Vec<u16> = text.encode_utf16().collect();

    let mut result = Vec::with_capacity(lower.len());
    for i in 0..lower.len() {
        let lo = lower.get(i).copied();
        let up = upper.get(i).copied();
        let ch = src.get(i).copied();
        // The reference compares `lower[i] !== upper[i]`; past the end of either
        // string that operand is `undefined`, and `undefined !== undefined` is
        // false. `Option` reproduces both halves of that.
        let cased = lo != up;
        let digit = matches!(lo, Some(u) if (0x30..=0x39).contains(&u));
        let apostrophe = preserve_apostrophe && ch == Some(0x27);
        if cased || digit || apostrophe {
            match ch {
                Some(u) => result.push(u),
                None => result.extend_from_slice(UNDEFINED),
            }
        } else {
            result.push(0x20);
        }
    }
    result
}

/// Iterator over case-tokenizer tokens.
#[derive(Debug, Clone)]
pub enum CaseTokens<'a> {
    /// All-ASCII input: tokens are slices of the original.
    Ascii {
        /// The input.
        text: &'a str,
        /// Current byte offset.
        pos: usize,
        /// Whether apostrophes are kept.
        preserve_apostrophe: bool,
    },
    /// General input: tokens come from the rewritten UTF-16 buffer.
    Units {
        /// The reference's intermediate `result`, as code units.
        units: Vec<u16>,
        /// Current code-unit offset.
        pos: usize,
    },
}

impl<'a> Iterator for CaseTokens<'a> {
    type Item = Utf16Token<'a>;

    fn next(&mut self) -> Option<Utf16Token<'a>> {
        match self {
            Self::Ascii {
                text,
                pos,
                preserve_apostrophe,
            } => {
                let bytes = text.as_bytes();
                while *pos < bytes.len() && !ascii_kept(bytes[*pos], *preserve_apostrophe) {
                    *pos += 1;
                }
                if *pos >= bytes.len() {
                    return None;
                }
                let start = *pos;
                while *pos < bytes.len() && ascii_kept(bytes[*pos], *preserve_apostrophe) {
                    *pos += 1;
                }
                Some(Utf16Token::borrowed(&text[start..*pos]))
            }
            Self::Units { units, pos } => {
                // `result.replace(/\s+/g, ' ').split(' ')` plus `trim` is exactly
                // "every maximal run of non-whitespace units": the collapse
                // guarantees no interior empty, and `trim` removes the edge ones.
                while *pos < units.len() && is_whitespace_unit(units[*pos]) {
                    *pos += 1;
                }
                if *pos >= units.len() {
                    return None;
                }
                let start = *pos;
                while *pos < units.len() && !is_whitespace_unit(units[*pos]) {
                    *pos += 1;
                }
                Some(Utf16Token::from_units(&units[start..*pos]))
            }
        }
    }
}

impl Tokenize for CaseTokenizer {
    type Token<'a> = Utf16Token<'a>;

    fn tokens<'a>(&self, text: &'a str) -> impl Iterator<Item = Utf16Token<'a>> {
        if text.is_ascii() {
            CaseTokens::Ascii {
                text,
                pos: 0,
                preserve_apostrophe: self.preserve_apostrophe,
            }
        } else {
            CaseTokens::Units {
                units: filtered_units(text, self.preserve_apostrophe),
                pos: 0,
            }
        }
    }
}

impl verbora_core::Tokenizer for CaseTokenizer {
    /// Tokens containing unpaired surrogates are rendered with U+FFFD here; use
    /// [`Tokenize::tokens`] for the lossless form.
    fn tokenize_into(&self, text: &str, out: &mut Vec<String>) {
        out.extend(Tokenize::tokens(self, text).map(|t| t.to_string_lossy().into_owned()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strs(t: &CaseTokenizer, s: &str) -> Vec<String> {
        t.tokens(s)
            .map(|t| t.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn erases_everything_uncased() {
        let t = CaseTokenizer::new();
        assert_eq!(strs(&t, ""), Vec::<String>::new());
        assert_eq!(strs(&t, "   "), Vec::<String>::new());
        assert_eq!(strs(&t, "\n"), Vec::<String>::new());
        assert_eq!(strs(&t, "日本語"), Vec::<String>::new());
        assert_eq!(strs(&t, "..."), Vec::<String>::new());
    }

    #[test]
    fn keeps_digits_and_letters() {
        let t = CaseTokenizer::new();
        assert_eq!(strs(&t, "abc 123"), ["abc", "123"]);
        assert_eq!(strs(&t, "MiXeDcAsE"), ["MiXeDcAsE"]);
        assert_eq!(strs(&t, "café naïve"), ["café", "naïve"]);
        assert_eq!(strs(&t, "Москва"), ["Москва"]);
        // Non-ASCII digits are not whitelisted.
        assert_eq!(strs(&t, "٣"), Vec::<String>::new());
    }

    #[test]
    fn apostrophes_are_optional() {
        assert_eq!(strs(&CaseTokenizer::new(), "it's"), ["it", "s"]);
        assert_eq!(
            strs(&CaseTokenizer::preserving_apostrophes(), "it's"),
            ["it's"]
        );
        // Only U+0027, not the typographic apostrophe.
        assert_eq!(
            strs(&CaseTokenizer::preserving_apostrophes(), "it’s"),
            ["it", "s"]
        );
    }

    #[test]
    fn reproduces_the_undefined_append() {
        assert_eq!(
            strs(&CaseTokenizer::new(), "İstanbul"),
            ["İstanbulundefined"]
        );
        assert_eq!(strs(&CaseTokenizer::new(), "İ😀"), ["İ😀undefined"]);
    }

    #[test]
    fn lengthening_uppercase_is_harmless() {
        let t = CaseTokenizer::new();
        for word in ["ß", "ǰ", "ΐ", "ﬁre"] {
            assert_eq!(strs(&t, word), [word]);
        }
    }

    #[test]
    fn surrogate_halves_are_uncased_and_dropped() {
        assert_eq!(
            strs(&CaseTokenizer::new(), "test😀emoji"),
            ["test", "emoji"]
        );
    }

    #[test]
    fn ascii_path_borrows() {
        let t = CaseTokenizer::new();
        let toks: Vec<_> = t.tokens("hello world").collect();
        assert!(
            toks.iter()
                .all(|t| matches!(t, Utf16Token::Text(std::borrow::Cow::Borrowed(_))))
        );
    }

    #[test]
    fn very_long_input_scales() {
        let text = "lorem ipsum ".repeat(5_000);
        assert_eq!(CaseTokenizer::new().tokens(&text).count(), 10_000);
    }
}
