//! The two UAX #29 word-boundary tokenizers.

use unicode_segmentation::UnicodeSegmentation;
use verbora_core::{BorrowingTokenizer, Tokenizer};

/// Yields every UAX #29 word segment of the input, including whitespace and
/// punctuation runs.
///
/// This is the *unfiltered* boundary iterator: it applies the [UAX #29] §4 word
/// boundary rules WB1–WB999 and emits every segment they delimit, in order.
///
/// # Guarantees
///
/// * **Concatenation reproduces the input.**
///   `SegmentTokenizer.tokens(t).collect::<String>() == t`, for every `t`.
///   This is the property a highlighter, a re-assembler or an offset consumer
///   needs, and it is why this tokenizer exists alongside [`WordTokenizer`]:
///   the word filter is irreversible.
/// * Every token is a contiguous slice of the input, at strictly increasing,
///   non-overlapping byte ranges.
/// * No token is ever the empty string. `tokens("")` yields nothing.
///
/// # Examples
///
/// ```
/// use verbora_tokenizers::{BorrowingTokenizer, SegmentTokenizer};
///
/// let t = SegmentTokenizer;
/// assert_eq!(
///     t.tokenize_borrowed("The quick (\"brown\") fox"),
///     ["The", " ", "quick", " ", "(", "\"", "brown", "\"", ")", " ", "fox"],
/// );
///
/// // Concatenation is the input, byte for byte.
/// let text = "a\r\nb — c\u{0301}!";
/// assert_eq!(t.tokens(text).collect::<String>(), text);
/// ```
///
/// [UAX #29]: https://www.unicode.org/reports/tr29/
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct SegmentTokenizer;

/// Yields the UAX #29 word segments that contain at least one letter or digit.
///
/// A segment is kept when it contains at least one scalar with the
/// [`Alphabetic`] property, or one scalar whose [`General_Category`] is `Nd`,
/// `Nl` or `No`. Everything else — whitespace runs, punctuation runs, symbol
/// runs — is dropped. This is what almost every consumer of a tokenizer wants,
/// and having it here is what stops each of them writing its own boundary rule.
///
/// # Guarantees
///
/// * Every token is a contiguous slice of the input, at strictly increasing,
///   non-overlapping byte ranges.
/// * No token is ever the empty string. `tokens("")` yields nothing.
/// * The output is a subsequence of [`SegmentTokenizer`]'s output on the same
///   input, with equal pointer identity for corresponding tokens. Concatenation
///   does **not** reproduce the input — use [`SegmentTokenizer`] when it must.
/// * Nothing is folded, trimmed, transliterated or substituted. A token that
///   came out of this tokenizer occurs verbatim in the input.
///
/// # What UAX #29 does that a character-class scan does not
///
/// The rules keep a word together across interior punctuation where the
/// standard says a word is one thing, and break it where the standard says it
/// is two. The cases that reach real corpora:
///
/// | Input | Tokens | Rule |
/// |---|---|---|
/// | `"well-known"` | `["well", "known"]` | `U+002D` is `Word_Break=Other`; WB999 breaks |
/// | `"and/or"` | `["and", "or"]` | `U+002F` is `Word_Break=Other` |
/// | `"don't"` | `["don't"]` | WB6/WB7 over `MidNumLetQ` |
/// | `"3.14"` | `["3.14"]` | WB11/WB12, `Numeric × MidNumLet × Numeric` |
/// | `"1,000"` | `["1,000"]` | WB11/WB12, `MidNum` |
/// | `"node_js"` | `["node_js"]` | WB13a/WB13b, `ExtendNumLet` |
/// | `"a:b"` | `["a:b"]` | WB6/WB7, `MidLetter` |
/// | `"café naïve"` | `["café", "naïve"]` | `é`, `ï` are `ALetter` |
/// | `"привет, мир"` | `["привет", "мир"]` | Cyrillic is `ALetter` |
/// | `"日本語"` | `["日", "本", "語"]` | Han is `Other`; WB999 breaks between each |
///
/// # The limitation, stated rather than hidden
///
/// [UAX #29] §4 says outright that its default rules **do not segment languages
/// that do not use spaces** — Thai, Lao, Khmer, Myanmar, Chinese, Japanese —
/// and that a dictionary or statistical approach is required for those. The
/// last row of the table above is that limitation: each Han scalar becomes its
/// own token, and `"すもももももも"` is seven tokens rather than a word
/// segmentation. Verbora ships no dictionary segmenter; when one arrives it
/// will be a separate crate with a checked-in generator and a cited model.
///
/// # Examples
///
/// ```
/// use verbora_tokenizers::{BorrowingTokenizer, WordTokenizer};
///
/// let t = WordTokenizer;
/// assert_eq!(
///     t.tokenize_borrowed("The quick (\"brown\") fox can't jump 32.3 feet, right?"),
///     ["The", "quick", "brown", "fox", "can't", "jump", "32.3", "feet", "right"],
/// );
///
/// // Nothing is folded or stripped: tokens are substrings.
/// assert_eq!(t.tokenize_borrowed("İstanbul"), ["İstanbul"]);
/// assert_eq!(t.tokenize_borrowed("a😀b"), ["a", "b"]);
/// ```
///
/// [`Alphabetic`]: https://www.unicode.org/reports/tr44/#Alphabetic
/// [`General_Category`]: https://www.unicode.org/reports/tr44/#General_Category_Values
/// [UAX #29]: https://www.unicode.org/reports/tr29/
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct WordTokenizer;

impl BorrowingTokenizer for SegmentTokenizer {
    fn tokens<'a>(&self, text: &'a str) -> impl Iterator<Item = &'a str> {
        text.split_word_bounds()
    }
}

impl Tokenizer for SegmentTokenizer {
    fn tokenize_into(&self, text: &str, out: &mut Vec<String>) {
        out.extend(self.tokens(text).map(str::to_owned));
    }
}

impl BorrowingTokenizer for WordTokenizer {
    fn tokens<'a>(&self, text: &'a str) -> impl Iterator<Item = &'a str> {
        text.unicode_words()
    }
}

impl Tokenizer for WordTokenizer {
    fn tokenize_into(&self, text: &str, out: &mut Vec<String>) {
        out.extend(self.tokens(text).map(str::to_owned));
    }
}
