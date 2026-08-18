//! The "aggressive" tokenizer family: English plus fifteen language variants.
//!
//! # Why one scanner covers thirteen of them
//!
//! The variants look different in the reference but converge on the same
//! observable behaviour. Three shapes appear:
//!
//! | Shape | Languages | Reference code |
//! |---|---|---|
//! | split on a class run, then `trim` | en, nl, de, fr, es, it, vi, no, sv | `trim(text.split(/[^…]+/))` |
//! | rewrite non-class to space, collapse, split, drop empties | ru, pl, uk, id | `withoutEmpty(clearText(text).split(' '))` |
//! | split on *single* class chars, then `trim`, then drop empties | pt | `withoutEmpty(trim(text.split(/[^…]/)))` |
//!
//! All three produce **exactly the maximal non-empty runs of word characters**:
//!
//! * shape 1 — `+` collapses separator runs so no interior empty can exist, and
//!   `trim` removes the leading/trailing empties a boundary separator creates;
//! * shape 2 — every non-word character becomes a space, runs of whitespace
//!   collapse to one, and `withoutEmpty` drops every empty the split produces;
//! * shape 3 — the missing `+` *does* create interior empties, but the
//!   `withoutEmpty` that wraps `trim` removes all of them, edge and interior
//!   alike.
//!
//! Establishing that equivalence is what lets [`scan::WordRuns`](crate::scan)
//! replace a regex engine here. It also fixes the one thing a reader would
//! otherwise have to take on trust: the composition order in Portuguese is
//! `withoutEmpty(trim(...))`, not `trim(withoutEmpty(...))`, and only the former
//! is equivalent to "maximal runs".
//!
//! # Why the character classes are generated
//!
//! Every class in [`crate::classes`] is derived by running the reference regex
//! over the whole Basic Multilingual Plane rather than by transcription. That
//! caught several things a careful reader would not have: `/i` on Russian's
//! `а-я` admits U+1C80–U+1C86 (historic Cyrillic letters whose uppercase forms
//! fall in `А-Я`); Spanish's `á-ú` range contains `×` and `÷`; German's class
//! omits the uppercase umlauts, making `Ä`, `Ö` and `Ü` separators.
//!
//! The three tokenizers that are *not* pure splitters — Persian, Hindi,
//! Norwegian/Swedish — are implemented individually below, each with the reason
//! it cannot be.

use std::borrow::Cow;

use crate::Tokenize;
use crate::classes;
use crate::scan::{CharClass, Source, SourceRuns, WordRuns};
use crate::whitespace::is_whitespace;

/// Defines a tokenizer that emits maximal runs of one character class.
macro_rules! class_tokenizer {
    (
        $(#[$meta:meta])*
        $name:ident, $class:ident, $pred:path
    ) => {
        #[doc = concat!("Word-character class for [`", stringify!($name), "`].")]
        #[derive(Debug, Clone, Copy)]
        pub struct $class;

        impl CharClass for $class {
            const ASCII_LUT: Option<[u64; 2]> = crate::scan::ascii_lut!($pred);

            #[inline]
            fn is_word(c: char) -> bool {
                $pred(c)
            }
        }

        $(#[$meta])*
        #[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
        pub struct $name;

        impl $name {
            /// Creates the tokenizer. It is stateless and zero-sized.
            #[inline]
            pub const fn new() -> Self {
                Self
            }
        }

        impl Tokenize for $name {
            type Token<'a> = &'a str;

            #[inline]
            fn tokens<'a>(&self, text: &'a str) -> impl Iterator<Item = &'a str> {
                WordRuns::<$class>::new(text)
            }

            // Overridden for the reservation only — the tokens are the
            // default's, element for element. `WordRuns::size_hint` has a
            // lower bound of zero, so a bare `collect` grows through the
            // 4/8/16… malloc chain, which measures as ~25% of a small
            // document's tokenize cost. Prose averages one token per ~6
            // bytes; `len / 8 + 2` reserves once without over-allocating.
            // (Counting tokens exactly with a first pass was measured 2x
            // slower — the scan costs more than the growth it avoids.)
            #[inline]
            fn tokenize<'a>(&self, text: &'a str) -> Vec<&'a str> {
                let mut out = Vec::with_capacity(text.len() / 8 + 2);
                out.extend(self.tokens(text));
                out
            }
        }

        impl verbora_core::Tokenizer for $name {
            fn tokenize_into(&self, text: &str, out: &mut Vec<String>) {
                out.extend(Tokenize::tokens(self, text).map(str::to_owned));
            }
        }

        impl verbora_core::BorrowingTokenizer for $name {
            fn tokenize_borrowed_into<'a>(&self, text: &'a str, out: &mut Vec<&'a str>) {
                out.extend(Tokenize::tokens(self, text));
            }
        }
    };
}

class_tokenizer! {
    /// English. Word characters are ASCII letters and digits plus `'`, `-` and `/`.
    ///
    /// `_` is **not** a word character here (unlike the Dutch, German and Italian
    /// variants), and no accented letter is: `"café naïve"` tokenizes to
    /// `["caf", "na", "ve"]`.
    ///
    /// ```
    /// use verbora_tokenizers::{AggressiveTokenizer, Tokenize};
    /// let t = AggressiveTokenizer::new();
    /// assert_eq!(t.tokenize("it's a-b c/d"), ["it's", "a-b", "c/d"]);
    /// assert_eq!(t.tokenize("a_b"), ["a", "b"]);
    /// ```
    AggressiveTokenizer, EnClass, classes::is_word_en
}

class_tokenizer! {
    /// Dutch. Like English but `_` is a word character and `/` is a separator.
    AggressiveTokenizerNl, NlClass, classes::is_word_nl
}

class_tokenizer! {
    /// German. Word characters are ASCII alphanumerics, `ß`, the **lowercase**
    /// umlauts `ä ö ü`, `_`, `'` and `-`.
    ///
    /// The uppercase umlauts are missing from the reference class and there is no
    /// `i` flag, so `Ä`, `Ö` and `Ü` split words: `"Äpfel Öl Über weiß"` gives
    /// `["pfel", "l", "ber", "weiß"]`. That is a bug in the reference, and
    /// reproducing it is the point.
    AggressiveTokenizerDe, DeClass, classes::is_word_de
}

class_tokenizer! {
    /// French. The `i` flag admits the uppercase forms of the accented letters,
    /// so unlike German this variant handles `ÉCOLE` and `Œuvre` correctly.
    /// `_`, `/` and `'` are separators; `-` is not.
    AggressiveTokenizerFr, FrClass, classes::is_word_fr
}

class_tokenizer! {
    /// Spanish. **No digits**: `"123 456"` tokenizes to `[]`.
    ///
    /// The class is built from raw Latin-1 ranges (`á-ú`, `Á-Ú`), which
    /// incidentally include `×` (U+00D7) and `÷` (U+00F7) — so `"a×b÷c"` is a
    /// single token.
    AggressiveTokenizerEs, EsClass, classes::is_word_es
}

class_tokenizer! {
    /// Italian. Splits on `\W+`, which in the reference is ASCII-only
    /// (`[^A-Za-z0-9_]`).
    ///
    /// Rust's `regex` makes `\W` Unicode-aware by default, which would keep
    /// `café` intact; the reference yields `["caf", "na", "ve"]` for
    /// `"café naïve"` and `[]` for `"привет, мир"`.
    AggressiveTokenizerIt, ItClass, classes::is_word_it
}

class_tokenizer! {
    /// Portuguese. **No digits**, and the split regex has no `+` quantifier — see
    /// the module documentation for why that still yields maximal runs.
    AggressiveTokenizerPt, PtClass, classes::is_word_pt
}

class_tokenizer! {
    /// Vietnamese. The `i` flag admits the uppercase form of every listed letter,
    /// which works out to the whole of Latin Extended Additional U+1EA0–U+1EF9
    /// plus the Latin-1 and Latin Extended-A vowels Vietnamese uses.
    AggressiveTokenizerVi, ViClass, classes::is_word_vi
}

class_tokenizer! {
    /// Russian. ASCII alphanumerics plus `а-я`/`А-Я` and `ё`/`Ё`.
    ///
    /// The `i` flag also admits U+1C80–U+1C86, historic Cyrillic letters whose
    /// uppercase forms land inside `А-Я`. No hand transcription would have
    /// included them; the generated class does.
    AggressiveTokenizerRu, RuClass, classes::is_word_ru
}

class_tokenizer! {
    /// Ukrainian. Like Russian, but `ё`/`Ё` are **not** in the class and are
    /// therefore deleted: `"привет, мир ёж"` gives `["привет", "мир", "ж"]`.
    /// The Ukrainian-specific `ґ є і ї` and their uppercase forms are kept.
    AggressiveTokenizerUk, UkClass, classes::is_word_uk
}

class_tokenizer! {
    /// Polish. ASCII alphanumerics plus `ą ż ś ź ę ć ń ó ł` and, via the `i` flag,
    /// their uppercase forms.
    AggressiveTokenizerPl, PlClass, classes::is_word_pl
}

class_tokenizer! {
    /// Indonesian. **Lowercase only**: The reference class `[^a-z0-9 -]` has no
    /// `i` flag, so every uppercase ASCII letter is replaced by a space.
    ///
    /// ```
    /// use verbora_tokenizers::{AggressiveTokenizerId, Tokenize};
    /// let t = AggressiveTokenizerId::new();
    /// assert_eq!(t.tokenize("A B"), Vec::<&str>::new());
    /// assert_eq!(t.tokenize("Hello World-2 !!"), ["ello", "orld-2"]);
    /// ```
    AggressiveTokenizerId, IdClass, classes::is_word_id
}

// ---------------------------------------------------------------------------
// Persian — a pure splitter, but with a rewrite step that (almost) never fires
// ---------------------------------------------------------------------------

/// The literal the Persian `clearText` regex actually searches for.
///
/// The reference writes `/.:\+-=\(\)"'!\?،,؛;/g`, intending a character class of
/// punctuation but forgetting the brackets. What it compiles to is a *sequence*:
/// any character except a line terminator, followed by these fourteen literal
/// characters in this exact order. It therefore matches essentially nothing.
///
/// A port that reads the author's intent and strips punctuation would produce
/// visibly different tokens — the reference keeps punctuation attached
/// (`"ist!"` stays one token), which is the behaviour under test.
const FA_CLEAR_LITERAL: &str = ":+-=()\"'!?،,؛;";

/// Persian. Splits on whitespace runs only, keeping punctuation attached to
/// tokens.
///
/// ```
/// use verbora_tokenizers::{AggressiveTokenizerFa, Tokenize};
/// let t = AggressiveTokenizerFa::new();
/// assert_eq!(t.tokenize("Ich weiß, daß Öl teuer ist!"),
///            ["Ich", "weiß,", "daß", "Öl", "teuer", "ist!"]);
/// ```
///
/// This is the one variant that does not use `Tokenizer#trim`: it filters every
/// empty string, not just the edges. Since the split is on `\s+`, that
/// distinction is not observable — but the `clearText` rewrite it applies first
/// is (see `FA_CLEAR_LITERAL`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AggressiveTokenizerFa;

impl AggressiveTokenizerFa {
    /// Creates the tokenizer. It is stateless and zero-sized.
    #[inline]
    pub const fn new() -> Self {
        Self
    }
}

/// Byte ranges that `clearText` turns into a single space.
///
/// Each is 15 characters: the matched "any character" plus the fourteen literal
/// ones. Matches are found left to right and do not overlap, as `/g` requires.
fn fa_masked_ranges(text: &str) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(rel) = text[from..].find(FA_CLEAR_LITERAL) {
        let lit = from + rel;
        let end = lit + FA_CLEAR_LITERAL.len();
        // The leading `.` needs a character that is not a line terminator.
        match text[..lit].chars().next_back() {
            Some(c) if !crate::whitespace::is_line_terminator(c) => {
                out.push((lit - c.len_utf8(), end));
                from = end;
            }
            _ => from = lit + 1,
        }
    }
    out
}

/// Iterator over Persian tokens.
#[derive(Debug, Clone)]
pub struct FaTokens<'a> {
    text: &'a str,
    pos: usize,
    /// Ranges consumed by `clearText`; almost always empty.
    masked: Vec<(usize, usize)>,
    mask_idx: usize,
}

impl<'a> FaTokens<'a> {
    /// Whether `pos` starts a masked range, and where that range ends.
    fn mask_end(&mut self, pos: usize) -> Option<usize> {
        while let Some(&(_, end)) = self.masked.get(self.mask_idx) {
            if end <= pos {
                self.mask_idx += 1;
            } else {
                break;
            }
        }
        match self.masked.get(self.mask_idx) {
            Some(&(start, end)) if pos >= start => Some(end),
            _ => None,
        }
    }
}

impl<'a> Iterator for FaTokens<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<&'a str> {
        let text: &'a str = self.text;
        // Skip separators: whitespace, and anything `clearText` blanked out.
        loop {
            if self.pos >= text.len() {
                return None;
            }
            if let Some(end) = self.mask_end(self.pos) {
                self.pos = end;
                continue;
            }
            let c = text[self.pos..].chars().next()?;
            if is_whitespace(c) {
                self.pos += c.len_utf8();
            } else {
                break;
            }
        }

        let start = self.pos;
        while self.pos < text.len() {
            if self.mask_end(self.pos).is_some() {
                break;
            }
            let Some(c) = text[self.pos..].chars().next() else {
                break;
            };
            if is_whitespace(c) {
                break;
            }
            self.pos += c.len_utf8();
        }
        Some(&text[start..self.pos])
    }
}

impl Tokenize for AggressiveTokenizerFa {
    type Token<'a> = &'a str;

    fn tokens<'a>(&self, text: &'a str) -> impl Iterator<Item = &'a str> {
        FaTokens {
            text,
            pos: 0,
            // The common case allocates nothing: the literal is not present.
            masked: if text.contains(FA_CLEAR_LITERAL) {
                fa_masked_ranges(text)
            } else {
                Vec::new()
            },
            mask_idx: 0,
        }
    }
}

impl verbora_core::Tokenizer for AggressiveTokenizerFa {
    fn tokenize_into(&self, text: &str, out: &mut Vec<String>) {
        out.extend(Tokenize::tokens(self, text).map(str::to_owned));
    }
}

impl verbora_core::BorrowingTokenizer for AggressiveTokenizerFa {
    fn tokenize_borrowed_into<'a>(&self, text: &'a str, out: &mut Vec<&'a str>) {
        out.extend(Tokenize::tokens(self, text));
    }
}

// ---------------------------------------------------------------------------
// Norwegian and Swedish — diacritic removal, first occurrence only
// ---------------------------------------------------------------------------

/// `normalizer_no.removeDiacritics`, in source order — thirteen letters, each in
/// both cases.
///
/// The reference is twenty-six `text.replace('à', 'a')` calls. Passing a
/// **string** as the first argument to `String.prototype.replace` replaces only
/// the *first* occurrence — Rust's [`str::replace`] replaces all of them, which
/// is a silent divergence on any text with a repeated accent:
/// `tokenize("àà ààà")` is `["a"]` in the reference and `["aa", "aaa"]` from a naive
/// port.
const NO_DIACRITICS: [(char, char); 26] = [
    ('à', 'a'),
    ('À', 'A'),
    ('á', 'a'),
    ('Á', 'A'),
    ('â', 'a'),
    ('Â', 'A'),
    ('ç', 'c'),
    ('Ç', 'C'),
    ('è', 'e'),
    ('È', 'E'),
    ('é', 'e'),
    ('É', 'E'),
    ('ê', 'e'),
    ('Ê', 'E'),
    ('î', 'i'),
    ('Î', 'I'),
    ('ñ', 'n'),
    ('Ñ', 'N'),
    ('ó', 'o'),
    ('Ó', 'O'),
    ('ô', 'o'),
    ('Ô', 'O'),
    ('û', 'u'),
    ('Û', 'U'),
    ('š', 's'),
    ('Š', 'S'),
];

/// `normalizer_sv.removeDiacritics`, in source order — only four letters.
const SV_DIACRITICS: [(char, char); 8] = [
    ('à', 'a'),
    ('À', 'A'),
    ('á', 'a'),
    ('Á', 'A'),
    ('è', 'e'),
    ('È', 'E'),
    ('é', 'e'),
    ('É', 'E'),
];

/// Applies a first-occurrence-only replacement table in a single pass.
///
/// # Why one pass is equivalent to N sequential replaces
///
/// Each rule searches for a distinct character, and every *replacement* is an
/// unaccented ASCII letter that appears in no rule's search position. A rule's
/// first occurrence in the original string is therefore also its first
/// occurrence in every intermediate string, so the passes are independent and
/// can be fused. (Were that not so, the fusion would change which occurrence
/// each rule hit.)
fn remove_diacritics<'a>(text: &'a str, table: &[(char, char)]) -> Cow<'a, str> {
    // Every source character is non-ASCII, so ASCII input is already normalised.
    if text.is_ascii() {
        return Cow::Borrowed(text);
    }
    let mut done: u32 = 0;
    let mut out: Option<String> = None;
    for (i, c) in text.char_indices() {
        if let Some(k) = table.iter().position(|&(from, _)| from == c) {
            if done & (1 << k) == 0 {
                done |= 1 << k;
                let buf = out.get_or_insert_with(|| {
                    let mut s = String::with_capacity(text.len());
                    s.push_str(&text[..i]);
                    s
                });
                buf.push(table[k].1);
                continue;
            }
        }
        if let Some(buf) = out.as_mut() {
            buf.push(c);
        }
    }
    out.map_or(Cow::Borrowed(text), Cow::Owned)
}

/// Defines one of the two Scandinavian tokenizers, which normalise before
/// splitting and so cannot always borrow.
macro_rules! diacritic_tokenizer {
    (
        $(#[$meta:meta])*
        $name:ident, $class:ident, $pred:path, $table:ident
    ) => {
        #[doc = concat!("Word-character class for [`", stringify!($name), "`].")]
        #[derive(Debug, Clone, Copy)]
        pub struct $class;

        impl CharClass for $class {
            #[inline]
            fn is_word(c: char) -> bool {
                $pred(c)
            }
        }

        $(#[$meta])*
        #[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
        pub struct $name;

        impl $name {
            /// Creates the tokenizer. It is stateless and zero-sized.
            #[inline]
            pub const fn new() -> Self {
                Self
            }

            /// The diacritic removal this tokenizer applies before splitting.
            ///
            /// Exposed because it is observable in the token *text*, not just in
            /// where the boundaries fall.
            pub fn normalize(text: &str) -> Cow<'_, str> {
                remove_diacritics(text, &$table)
            }
        }

        impl Tokenize for $name {
            type Token<'a> = Cow<'a, str>;

            fn tokens<'a>(&self, text: &'a str) -> impl Iterator<Item = Cow<'a, str>> {
                SourceRuns::<$class>::new(Source::from(Self::normalize(text)))
            }
        }

        impl verbora_core::Tokenizer for $name {
            fn tokenize_into(&self, text: &str, out: &mut Vec<String>) {
                out.extend(Tokenize::tokens(self, text).map(Cow::into_owned));
            }
        }
    };
}

diacritic_tokenizer! {
    /// Norwegian. Strips a fixed set of diacritics (first occurrence of each
    /// only), then splits on `[^A-Za-z0-9_æøåÆØÅäÄöÖüÜ]+`.
    ///
    /// `-` is **not** a word character here, so `"e-post"` becomes two tokens —
    /// the opposite of the Swedish variant.
    ///
    /// ```
    /// use verbora_tokenizers::{AggressiveTokenizerNo, Tokenize};
    /// let t = AggressiveTokenizerNo::new();
    /// // Only the first `à` is normalised; the rest stay non-word characters.
    /// assert_eq!(t.tokenize("àà ààà"), ["a"]);
    /// ```
    AggressiveTokenizerNo, NoClass, classes::is_word_no, NO_DIACRITICS
}

diacritic_tokenizer! {
    /// Swedish. Strips only `à á è é` (and their uppercase forms), first
    /// occurrence of each, then splits on `[^A-Za-z0-9_åÅäÄöÖüÜ-]+`.
    ///
    /// `-` **is** a word character here, so `"e-post"` stays one token.
    AggressiveTokenizerSv, SvClass, classes::is_word_sv, SV_DIACRITICS
}

// ---------------------------------------------------------------------------
// Hindi — deletes characters, so tokens are not always substrings
// ---------------------------------------------------------------------------

/// Characters the Hindi tokenizer deletes before splitting.
///
/// The reference class is `[।॥...?,]`: danda, double danda, then the
/// literal `...` — which inside a character class is just `.` written three
/// times — then `?` and `,`.
#[inline]
const fn hi_deleted(c: char) -> bool {
    matches!(c, '\u{964}' | '\u{965}' | '.' | '?' | ',')
}

/// Whether `c` survives the Hindi split.
///
/// The reference splits on `\s+|(?![ऀ-ॿ -])./u`: a run of
/// whitespace, or any single code point outside Devanagari and ASCII. Alternation
/// is ordered, so whitespace is tested first — which only matters for U+0020,
/// the one character in both sets.
///
/// The `u` flag makes `.` consume a whole code point, so an emoji is one split
/// point rather than two. This is the only tokenizer in the crate where astral
/// input does *not* produce surrogate halves.
#[inline]
const fn hi_word(c: char) -> bool {
    matches!(c, '\u{900}'..='\u{97f}' | '\u{21}'..='\u{7f}')
}

/// Hindi. Deletes `। ॥ . ? ,`, then splits on whitespace and on any character
/// outside Devanagari and ASCII.
///
/// ```
/// use verbora_tokenizers::{AggressiveTokenizerHi, Tokenize};
/// let t = AggressiveTokenizerHi::new();
/// assert_eq!(t.tokenize("राजा, बाजीराव ... ? ।॥"), ["राजा", "बाजीराव"]);
/// // ASCII punctuation other than the five deleted characters stays inside tokens.
/// assert_eq!(t.tokenize("a!b;c"), ["a!b;c"]);
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AggressiveTokenizerHi;

impl AggressiveTokenizerHi {
    /// Creates the tokenizer. It is stateless and zero-sized.
    #[inline]
    pub const fn new() -> Self {
        Self
    }
}

/// Iterator over Hindi tokens.
///
/// Deletion happens before splitting, so `"a.b"` is one token `"ab"` — text that
/// exists in no contiguous slice of the input. The iterator borrows anyway
/// whenever a token happens to contain no deleted character, which is most of
/// them.
#[derive(Debug, Clone)]
pub struct HiTokens<'a> {
    text: &'a str,
    pos: usize,
}

impl<'a> Iterator for HiTokens<'a> {
    type Item = Cow<'a, str>;

    fn next(&mut self) -> Option<Cow<'a, str>> {
        let text: &'a str = self.text;
        let base = self.pos;
        let mut start: Option<usize> = None;
        let mut end = base;
        let mut kept_bytes = 0usize;

        for (rel, c) in text[base..].char_indices() {
            let at = base + rel;
            if hi_deleted(c) {
                continue;
            }
            if hi_word(c) {
                if start.is_none() {
                    start = Some(at);
                }
                end = at + c.len_utf8();
                kept_bytes += c.len_utf8();
            } else if let Some(s) = start {
                // A separator ends the token; resume scanning at it.
                self.pos = at;
                return Some(self.slice(s, end, kept_bytes));
            }
        }

        self.pos = text.len();
        start.map(|s| self.slice(s, end, kept_bytes))
    }
}

impl<'a> HiTokens<'a> {
    /// Borrows when the kept characters were contiguous, copies when a deleted
    /// character sat between two of them.
    fn slice(&self, start: usize, end: usize, kept_bytes: usize) -> Cow<'a, str> {
        if end - start == kept_bytes {
            Cow::Borrowed(&self.text[start..end])
        } else {
            Cow::Owned(
                self.text[start..end]
                    .chars()
                    .filter(|c| !hi_deleted(*c))
                    .collect(),
            )
        }
    }
}

impl Tokenize for AggressiveTokenizerHi {
    type Token<'a> = Cow<'a, str>;

    fn tokens<'a>(&self, text: &'a str) -> impl Iterator<Item = Cow<'a, str>> {
        HiTokens { text, pos: 0 }
    }
}

impl verbora_core::Tokenizer for AggressiveTokenizerHi {
    fn tokenize_into(&self, text: &str, out: &mut Vec<String>) {
        out.extend(Tokenize::tokens(self, text).map(Cow::into_owned));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn english_matches_the_reference_vectors() {
        let t = AggressiveTokenizer::new();
        assert_eq!(t.tokenize(""), Vec::<&str>::new());
        assert_eq!(t.tokenize("..."), Vec::<&str>::new());
        assert_eq!(t.tokenize("  hello  world  "), ["hello", "world"]);
        assert_eq!(t.tokenize("café naïve"), ["caf", "na", "ve"]);
        assert_eq!(t.tokenize("привет, мир ёж"), Vec::<&str>::new());
        assert_eq!(t.tokenize("日本語テスト"), Vec::<&str>::new());
        assert_eq!(t.tokenize("test😀emoji"), ["test", "emoji"]);
        assert_eq!(t.tokenize("123 456"), ["123", "456"]);
        assert_eq!(t.tokenize("A"), ["A"]);
    }

    #[test]
    fn german_treats_uppercase_umlauts_as_separators() {
        let t = AggressiveTokenizerDe::new();
        assert_eq!(
            t.tokenize("Äpfel Öl Über weiß"),
            ["pfel", "l", "ber", "weiß"]
        );
        assert_eq!(t.tokenize("a_b"), ["a_b"]);
        assert_eq!(t.tokenize("c/d"), ["c", "d"]);
    }

    #[test]
    fn spanish_keeps_the_latin1_math_symbols() {
        let t = AggressiveTokenizerEs::new();
        assert_eq!(t.tokenize("a×b÷c"), ["a×b÷c"]);
        assert_eq!(t.tokenize("123 456"), Vec::<&str>::new());
        assert_eq!(t.tokenize("café naïve"), ["café", "naïve"]);
    }

    #[test]
    fn russian_and_ukrainian_differ_on_yo() {
        assert_eq!(
            AggressiveTokenizerRu::new().tokenize("привет, мир ёж"),
            ["привет", "мир", "ёж"]
        );
        assert_eq!(
            AggressiveTokenizerUk::new().tokenize("привет, мир ёж"),
            ["привет", "мир", "ж"]
        );
    }

    #[test]
    fn persian_keeps_punctuation_attached() {
        let t = AggressiveTokenizerFa::new();
        assert_eq!(t.tokenize("..."), ["..."]);
        assert_eq!(t.tokenize("test😀emoji"), ["test😀emoji"]);
        assert_eq!(t.tokenize(""), Vec::<&str>::new());
        assert_eq!(t.tokenize("  a  b  "), ["a", "b"]);
    }

    #[test]
    fn persian_clear_text_fires_only_on_the_exact_literal() {
        let t = AggressiveTokenizerFa::new();
        // The whole 15-character window collapses to a separator.
        assert_eq!(t.tokenize("ab:+-=()\"'!?،,؛;cd"), ["a", "cd"]);
    }

    #[test]
    fn scandinavian_diacritics_are_first_occurrence_only() {
        assert_eq!(AggressiveTokenizerNo::new().tokenize("àà ààà"), ["a"]);
        assert_eq!(
            AggressiveTokenizerSv::new().tokenize("éé ééé café café"),
            ["e", "caf", "caf"]
        );
        // Hyphen differs between the two.
        assert_eq!(
            AggressiveTokenizerNo::new().tokenize("e-post"),
            ["e", "post"]
        );
        assert_eq!(AggressiveTokenizerSv::new().tokenize("e-post"), ["e-post"]);
    }

    #[test]
    fn hindi_deletes_before_splitting() {
        let t = AggressiveTokenizerHi::new();
        assert_eq!(t.tokenize("हिंदी ß English 日本"), ["हिंदी", "English"]);
        assert_eq!(t.tokenize("a\tb\nc"), ["a", "b", "c"]);
        assert_eq!(t.tokenize("a.b"), ["ab"]);
        assert_eq!(t.tokenize("a😀b"), ["a", "b"]);
        assert_eq!(t.tokenize(""), Vec::<&str>::new());
    }

    #[test]
    fn very_long_input_scales() {
        let text = "lorem ipsum ".repeat(5_000);
        assert_eq!(AggressiveTokenizer::new().tokenize(&text).len(), 10_000);
    }

    #[test]
    fn tokenize_into_appends_without_clearing() {
        let t = AggressiveTokenizer::new();
        let mut buf = vec!["seed"];
        t.tokenize_into("a b", &mut buf);
        assert_eq!(buf, ["seed", "a", "b"]);
    }

    /// `tokenize` is overridden for its capacity reservation only, so it must
    /// yield exactly what collecting the iterator yields — on empty input,
    /// separator-only input, and text on both sides of the reservation
    /// heuristic's break-even point.
    #[test]
    fn tokenize_override_matches_the_iterator() {
        let long = "lorem ipsum dolor ".repeat(500);
        for s in ["", " ", "...", "a", "a b", "don't stop-me now/then", &long] {
            let t = AggressiveTokenizer::new();
            assert_eq!(t.tokenize(s), Tokenize::tokens(&t, s).collect::<Vec<_>>());
            let t = AggressiveTokenizerRu::new();
            assert_eq!(t.tokenize(s), Tokenize::tokens(&t, s).collect::<Vec<_>>());
        }
    }
}
