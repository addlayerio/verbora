//! `TokenizerJa` — the reference's port of TinySegmenter 0.1 (Taku Kudo).
//!
//! A linear-chain classifier: for every position it sums ~46 weights drawn from
//! the surrounding six characters, their character *types*, and the last three
//! segmentation decisions, and starts a new token when the sum is positive.

mod model;
mod norm_tables;
mod normalize;

use crate::Tokenize;
use crate::utf16::Utf16Token;

/// Character type, the reference's `ctype_`.
///
/// The discriminants must match the alphabet order the generated tables were
/// flattened against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum Ctype {
    /// Numeral kanji: 〇一二三四五六七八九十百千万億兆.
    M = 0,
    /// Han (kanji) generally.
    H = 1,
    /// Hiragana.
    I = 2,
    /// Katakana.
    K = 3,
    /// ASCII letter.
    A = 4,
    /// ASCII digit.
    N = 5,
    /// Anything else.
    O = 6,
}

/// The previous segmentation decision, the reference's `p1`/`p2`/`p3`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
enum Pos {
    /// Unknown — the initial value before any decision.
    U = 0,
    /// No boundary here.
    O = 1,
    /// A boundary began here.
    B = 2,
}

/// A word-keyed weight table: sorted by UTF-16 key, looked up by binary search.
///
/// Keys are `[u16]` rather than `str` because the segmenter splits its input per
/// code unit, so a key built from an astral character is half a surrogate pair
/// and has no `str` representation.
pub(crate) struct WordTable(pub(crate) &'static [(&'static [u16], i32)]);

impl WordTable {
    /// The weight for `key`, or `0` when absent.
    ///
    /// The reference's `ts_(v)` is `if (v) return v; return 0`. Every stored
    /// weight is non-zero, so "falsy" and "absent" coincide and this is exactly
    /// `unwrap_or(0)`.
    #[inline]
    fn get(&self, key: &[u16]) -> i32 {
        match self.0.binary_search_by(|probe| <[u16]>::cmp(probe.0, key)) {
            Ok(i) => self.0[i].1,
            Err(_) => 0,
        }
    }
}

/// One element of the reference's `seg` array: a code unit, or a boundary tag
/// such as `B3` / `E1`.
#[derive(Debug, Clone, Copy)]
struct Seg {
    units: [u16; 2],
    len: u8,
}

impl Seg {
    #[inline]
    const fn unit(u: u16) -> Self {
        Self {
            units: [u, 0],
            len: 1,
        }
    }

    /// A two-ASCII-character boundary tag.
    #[inline]
    const fn tag(a: u8, b: u8) -> Self {
        Self {
            units: [a as u16, b as u16],
            len: 2,
        }
    }

    #[inline]
    fn as_slice(&self) -> &[u16] {
        &self.units[..self.len as usize]
    }
}

/// Concatenates `parts` into a stack buffer and looks the result up.
///
/// The reference builds these keys with string concatenation, allocating for
/// every one of thirteen lookups per character. Six code units is the maximum
/// (three boundary tags), so a fixed buffer removes the allocation entirely.
#[inline]
fn lookup(table: &WordTable, parts: &[Seg]) -> i32 {
    let mut buf = [0u16; 6];
    let mut n = 0;
    for p in parts {
        for &u in p.as_slice() {
            buf[n] = u;
            n += 1;
        }
    }
    table.get(&buf[..n])
}

/// Japanese word segmenter.
///
/// ```
/// use verbora_tokenizers::{TokenizerJa, Tokenize};
/// let t = TokenizerJa::new();
/// assert_eq!(t.tokenize("日本語"), ["日本語"]);
/// assert_eq!(t.tokenize("Hello, world!"), ["Hello", "world"]);
/// assert_eq!(t.tokenize("。。。"), Vec::<&str>::new());
/// ```
///
/// # Differences from the other tokenizers
///
/// * Empty input returns `[]` rather than throwing — this is the only tokenizer
///   in the reference that null-checks its argument.
/// * Punctuation is stripped from *inside* tokens, not just between them, and
///   tokens emptied by that stripping disappear (`"a.b"` becomes `"ab"`).
/// * Tokens are built by concatenating code units, so astral input can produce
///   tokens containing unpaired surrogates. That is why the token type is
///   [`Utf16Token`] rather than `String`.
///
/// # Performance
///
/// The reference rebuilds fifty hash tables on every `new TokenizerJa()`. Here
/// they are `static` data: the weights keyed by character type and previous
/// decision — thirty of the forty-six lookups — are dense arrays indexed by
/// small integers, so those lookups are a shift, an add and a load, with no
/// hashing and no key construction at all. The tokenizer itself is zero-sized.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TokenizerJa;

impl TokenizerJa {
    /// Creates the tokenizer. It is stateless and zero-sized.
    #[inline]
    pub const fn new() -> Self {
        Self
    }

    /// Runs the segmenter, returning tokens as UTF-16 code-unit runs.
    fn segment(text: &str) -> Vec<Vec<u16>> {
        if text.is_empty() {
            return Vec::new();
        }
        let units = normalize::normalize_ja(text);
        if units.is_empty() {
            return Vec::new();
        }

        let mut seg = Vec::with_capacity(units.len() + 6);
        let mut ctype = Vec::with_capacity(units.len() + 6);
        seg.extend([
            Seg::tag(b'B', b'3'),
            Seg::tag(b'B', b'2'),
            Seg::tag(b'B', b'1'),
        ]);
        ctype.extend([Ctype::O, Ctype::O, Ctype::O]);
        for &u in &units {
            seg.push(Seg::unit(u));
            ctype.push(model::ctype(u));
        }
        seg.extend([
            Seg::tag(b'E', b'1'),
            Seg::tag(b'E', b'2'),
            Seg::tag(b'E', b'3'),
        ]);
        ctype.extend([Ctype::O, Ctype::O, Ctype::O]);

        let mut result: Vec<Vec<u16>> = Vec::new();
        let mut word: Vec<u16> = seg[3].as_slice().to_vec();
        let (mut p1, mut p2, mut p3) = (Pos::U, Pos::U, Pos::U);

        for i in 4..seg.len() - 3 {
            let (w1, w2, w3) = (seg[i - 3], seg[i - 2], seg[i - 1]);
            let (w4, w5, w6) = (seg[i], seg[i + 1], seg[i + 2]);
            let (c1, c2, c3) = (
                ctype[i - 3] as usize,
                ctype[i - 2] as usize,
                ctype[i - 1] as usize,
            );
            let (c4, c5, c6) = (
                ctype[i] as usize,
                ctype[i + 1] as usize,
                ctype[i + 2] as usize,
            );
            let (q1, q2, q3) = (p1 as usize, p2 as usize, p3 as usize);

            let mut score = model::BIAS;
            score += model::UP1__[q1];
            score += model::UP2__[q2];
            score += model::UP3__[q3];
            score += model::BP1__[q1 * 3 + q2];
            score += model::BP2__[q2 * 3 + q3];
            score += lookup(&model::UW1__, &[w1]);
            score += lookup(&model::UW2__, &[w2]);
            score += lookup(&model::UW3__, &[w3]);
            score += lookup(&model::UW4__, &[w4]);
            score += lookup(&model::UW5__, &[w5]);
            score += lookup(&model::UW6__, &[w6]);
            score += lookup(&model::BW1__, &[w2, w3]);
            score += lookup(&model::BW2__, &[w3, w4]);
            score += lookup(&model::BW3__, &[w4, w5]);
            score += lookup(&model::TW1__, &[w1, w2, w3]);
            score += lookup(&model::TW2__, &[w2, w3, w4]);
            score += lookup(&model::TW3__, &[w3, w4, w5]);
            score += lookup(&model::TW4__, &[w4, w5, w6]);
            score += model::UC1__[c1];
            score += model::UC2__[c2];
            score += model::UC3__[c3];
            score += model::UC4__[c4];
            score += model::UC5__[c5];
            score += model::UC6__[c6];
            score += model::BC1__[c2 * 7 + c3];
            score += model::BC2__[c3 * 7 + c4];
            score += model::BC3__[c4 * 7 + c5];
            score += model::TC1__[(c1 * 7 + c2) * 7 + c3];
            score += model::TC2__[(c2 * 7 + c3) * 7 + c4];
            score += model::TC3__[(c3 * 7 + c4) * 7 + c5];
            score += model::TC4__[(c4 * 7 + c5) * 7 + c6];
            score += model::UQ1__[q1 * 7 + c1];
            score += model::UQ2__[q2 * 7 + c2];
            score += model::UQ3__[q3 * 7 + c3];
            score += model::BQ1__[(q2 * 7 + c2) * 7 + c3];
            score += model::BQ2__[(q2 * 7 + c3) * 7 + c4];
            score += model::BQ3__[(q3 * 7 + c2) * 7 + c3];
            score += model::BQ4__[(q3 * 7 + c3) * 7 + c4];
            score += model::TQ1__[((q2 * 7 + c1) * 7 + c2) * 7 + c3];
            score += model::TQ2__[((q2 * 7 + c2) * 7 + c3) * 7 + c4];
            score += model::TQ3__[((q3 * 7 + c1) * 7 + c2) * 7 + c3];
            score += model::TQ4__[((q3 * 7 + c2) * 7 + c3) * 7 + c4];

            let mut p = Pos::O;
            if score > 0 {
                result.push(std::mem::take(&mut word));
                p = Pos::B;
            }
            p1 = p2;
            p2 = p3;
            p3 = p;
            word.extend_from_slice(seg[i].as_slice());
        }
        result.push(word);

        Self::remove_punc_tokens(result)
    }

    /// `removePuncTokens`: strips punctuation from inside every token, then drops
    /// the tokens that emptied.
    fn remove_punc_tokens(tokens: Vec<Vec<u16>>) -> Vec<Vec<u16>> {
        tokens
            .into_iter()
            .filter_map(|mut t| {
                t.retain(|&u| !model::is_punct(u));
                (!t.is_empty()).then_some(t)
            })
            .collect()
    }
}

impl Tokenize for TokenizerJa {
    type Token<'a> = Utf16Token<'a>;

    /// Segmentation is a whole-text operation, so the work happens eagerly and
    /// the iterator walks the finished list. The signature stays uniform with the
    /// streaming tokenizers so generic code does not have to special-case it.
    fn tokens<'a>(&self, text: &'a str) -> impl Iterator<Item = Utf16Token<'a>> {
        Self::segment(text)
            .into_iter()
            .map(|u| Utf16Token::from_units(&u))
    }
}

impl verbora_core::Tokenizer for TokenizerJa {
    /// Tokens containing unpaired surrogates are rendered with U+FFFD here; use
    /// [`Tokenize::tokens`] for the lossless form.
    fn tokenize_into(&self, text: &str, out: &mut Vec<String>) {
        out.extend(Tokenize::tokens(self, text).map(|t| t.to_string_lossy().into_owned()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strs(s: &str) -> Vec<String> {
        TokenizerJa::new()
            .tokens(s)
            .map(|t| t.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn empty_input_returns_nothing() {
        assert_eq!(strs(""), Vec::<String>::new());
        assert_eq!(strs("。。。"), Vec::<String>::new());
    }

    #[test]
    fn single_characters() {
        assert_eq!(strs("a"), ["a"]);
        assert_eq!(strs("あ"), ["あ"]);
        assert_eq!(strs("日本語"), ["日本語"]);
    }

    #[test]
    fn latin_and_digits_segment_as_the_reference_does() {
        assert_eq!(strs("ABC 123"), ["ABC", "1", "2", "3"]);
        assert_eq!(strs("Hello, world!"), ["Hello", "world"]);
    }

    #[test]
    fn the_classic_tongue_twister() {
        assert_eq!(
            strs("すもももももももものうち。"),
            ["すも", "も", "も", "も", "も", "も", "も", "もの", "うち"]
        );
    }

    #[test]
    fn normalisation_runs_first() {
        assert_eq!(strs("ﾊﾝｶｸ"), ["ハンカク"]);
        assert_eq!(strs("１２３"), ["1", "2", "3"]);
        assert_eq!(strs("ＡＢＣ"), ["ABC"]);
    }

    #[test]
    fn character_types_are_ordered_numerals_first() {
        // 一 and 十 are numerals (M), not general Han (H).
        assert_eq!(model::ctype(0x4e00), Ctype::M);
        assert_eq!(model::ctype(0x5341), Ctype::M);
        assert_eq!(model::ctype(0x3042), Ctype::I);
        assert_eq!(model::ctype(0x30a2), Ctype::K);
        assert_eq!(model::ctype(u16::from(b'A')), Ctype::A);
        assert_eq!(model::ctype(u16::from(b'1')), Ctype::N);
        assert_eq!(model::ctype(u16::from(b'!')), Ctype::O);
    }

    #[test]
    fn astral_input_splits_into_surrogate_halves() {
        let toks: Vec<Vec<u16>> = TokenizerJa::new()
            .tokens("𠀋テスト")
            .map(|t| t.to_utf16())
            .collect();
        assert_eq!(
            toks,
            vec![vec![0xd840], vec![0xdc0b], vec![0x30c6, 0x30b9, 0x30c8]]
        );
    }

    #[test]
    fn punctuation_is_stripped_from_inside_tokens() {
        let out = TokenizerJa::remove_punc_tokens(vec![
            "a.b".encode_utf16().collect(),
            "...".encode_utf16().collect(),
            "  ".encode_utf16().collect(),
            "x".encode_utf16().collect(),
        ]);
        let out: Vec<String> = out
            .into_iter()
            .map(|u| String::from_utf16(&u).unwrap())
            .collect();
        assert_eq!(out, ["ab", "x"]);
    }
}
