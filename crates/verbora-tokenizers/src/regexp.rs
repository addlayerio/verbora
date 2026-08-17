//! The regex-driven tokenizers: `RegexpTokenizer` and its three subclasses.
//!
//! # Why these do not implement [`Tokenize`](crate::Tokenize)
//!
//! `RegexpTokenizer` has two modes, and one of them can return `null`:
//!
//! ```text
//! tokenize (s) {
//!   if (this._gaps) {
//!     return _.without(s.split(this._pattern), '', ' ')   // always an array
//!   } else {
//!     return s.match(this._pattern)                       // array OR null
//!   }
//! }
//! ```
//!
//! "No match" and "no tokens" are different observable outcomes, so these types
//! expose the same `tokens` / `tokenize` / `tokenize_into` shape as the rest of
//! the crate but wrapped in [`Option`], rather than pretending the distinction
//! does not exist.
//!
//! # Two dead options, faithfully dead
//!
//! * `discardEmpty` is written `options.discardEmpty || true`, which is `true`
//!   for every input. There is no way to switch it off, so this port does not
//!   offer one.
//! * `WordTokenizer` accepts `options.pattern` and then overwrites it
//!   unconditionally in its constructor. Same treatment.
//!
//! `gaps` *is* honoured by all of them, including the subclasses.

use std::sync::OnceLock;

use regex::Regex;

use crate::classes;
use crate::scan::{CharClass, WordRuns};
use crate::utf16::Utf16Token;
use crate::whitespace::is_line_terminator;

// ---------------------------------------------------------------------------
// RegexpTokenizer
// ---------------------------------------------------------------------------

/// A reference regular expression: the compiled pattern plus its `g` flag.
///
/// Rust's `regex` has no concept of a global flag, but the reference's match mode
/// depends on it: a non-global pattern yields `[fullMatch, ...captureGroups]`
/// while a global one yields every match's text and no groups at all.
#[derive(Debug, Clone)]
pub struct Pattern {
    /// The compiled pattern.
    pub regex: Regex,
    /// Whether the reference literal carried `/g`.
    pub global: bool,
}

impl Pattern {
    /// A pattern without the `g` flag.
    #[inline]
    pub const fn new(regex: Regex) -> Self {
        Self {
            regex,
            global: false,
        }
    }

    /// A pattern with the `g` flag.
    #[inline]
    pub const fn global(regex: Regex) -> Self {
        Self {
            regex,
            global: true,
        }
    }
}

/// Tokenizes by an arbitrary regular expression, splitting on it (the default)
/// or matching with it.
///
/// ```
/// use verbora_tokenizers::{Pattern, RegexpTokenizer};
/// let t = RegexpTokenizer::new(Pattern::new(regex::Regex::new(r"[^A-Za-z0-9_]+").unwrap()));
/// assert_eq!(t.tokenize("hello, world"), Some(vec![Some("hello"), Some("world")]));
/// ```
#[derive(Debug, Clone)]
pub struct RegexpTokenizer {
    pattern: Option<Pattern>,
    gaps: bool,
    /// Which [`split_into`] route `pattern` takes, decided on first use.
    ///
    /// Lazy because the constructors are `const fn`, so the probe in
    /// [`SplitStrategy::of`] cannot run in them; cacheable because `pattern`
    /// is private and never reassigned, so the answer cannot go stale.
    strategy: OnceLock<SplitStrategy>,
}

impl RegexpTokenizer {
    /// Splits on `pattern` (`gaps: true`, the default).
    #[inline]
    pub const fn new(pattern: Pattern) -> Self {
        Self {
            pattern: Some(pattern),
            gaps: true,
            strategy: OnceLock::new(),
        }
    }

    /// Matches `pattern` instead of splitting on it (`gaps: false`).
    #[inline]
    pub const fn matching(pattern: Pattern) -> Self {
        Self {
            pattern: Some(pattern),
            gaps: false,
            strategy: OnceLock::new(),
        }
    }

    /// Reproduces `new RegexpTokenizer()` with no pattern at all.
    ///
    /// The reference's `String#split(undefined)` returns `[s]`, so this tokenizer
    /// yields the whole input as one token (or nothing, if the input is empty or
    /// a single space, which `_.without` removes).
    #[inline]
    pub const fn without_pattern() -> Self {
        Self {
            pattern: None,
            gaps: true,
            strategy: OnceLock::new(),
        }
    }

    /// The full option matrix, matching the reference constructor.
    ///
    /// `discardEmpty` is deliberately absent: The reference computes it as
    /// `options.discardEmpty || true`, so it is `true` no matter what the caller
    /// passes.
    #[inline]
    pub const fn with_options(pattern: Option<Pattern>, gaps: bool) -> Self {
        Self {
            pattern,
            gaps,
            strategy: OnceLock::new(),
        }
    }

    /// Lazily yields tokens. `None` is the reference's `null`.
    pub fn tokens<'a>(&self, text: &'a str) -> Option<std::vec::IntoIter<Option<&'a str>>> {
        let mut out = Vec::new();
        self.tokenize_into(text, &mut out).then(|| out.into_iter())
    }

    /// Collects every token. `None` is the reference's `null`.
    pub fn tokenize<'a>(&self, text: &'a str) -> Option<Vec<Option<&'a str>>> {
        let mut out = Vec::new();
        self.tokenize_into(text, &mut out).then_some(out)
    }

    /// Appends tokens to `out`, returning `false` where the reference returned
    /// `null`.
    ///
    /// A token is `None` when a capture group did not participate in its match —
    /// the reference puts `undefined` in the array, and both `String#split` and
    /// `String#match` can produce it.
    pub fn tokenize_into<'a>(&self, text: &'a str, out: &mut Vec<Option<&'a str>>) -> bool {
        let Some(pattern) = &self.pattern else {
            if self.gaps {
                // `s.split(undefined)` is `[s]`, filtered by `_.without`.
                if !text.is_empty() && text != " " {
                    out.push(Some(text));
                }
            } else {
                // `s.match(undefined)` coerces to the empty pattern `/(?:)/`,
                // which matches at position 0 of anything; the result is `[""]`.
                out.push(Some(""));
            }
            return true;
        };
        if self.gaps {
            let strategy = *self
                .strategy
                .get_or_init(|| SplitStrategy::of(&pattern.regex));
            split_into(&pattern.regex, strategy, text, out);
            true
        } else {
            match_into(pattern, text, out)
        }
    }
}

/// How [`split_into`] executes a gaps-mode pattern.
///
/// The three routes produce identical output — the differential suites in the
/// test module pin this — but at very different speeds, and the right one is a
/// property of the pattern alone, so it is decided once per tokenizer and
/// cached (see the `strategy` field on [`RegexpTokenizer`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SplitStrategy {
    /// The pattern is the default-mode `\s+`: a hand-written `White_Space`
    /// run scanner replaces the regex engine entirely.
    Whitespace,
    /// No capture groups: match bounds are all the split needs, so
    /// `find_iter` replaces `captures_iter`.
    NoGroups,
    /// Capture groups present: the full interleaving loop.
    Captures,
}

impl SplitStrategy {
    /// Classifies `re`, probing for the one thing `as_str` cannot see.
    ///
    /// `RegexBuilder::unicode(false)` shrinks `\s` to its ASCII subset under
    /// the same pattern string; matching NBSP separates the two. Every other
    /// builder knob is inert for `\s+` — nothing in the pattern is case-,
    /// line-, or `.`-sensitive, and a `swap_greed`-lazy `\s+?` splits
    /// identically because its extra gaps between adjacent one-character
    /// matches are all `""` and filtered — which the builder-flag
    /// differential test pins.
    fn of(re: &Regex) -> Self {
        if re.as_str() == r"\s+" && re.is_match("\u{a0}") {
            Self::Whitespace
        } else if re.captures_len() == 1 {
            // `captures_len` counts the implicit group 0, so 1 means the
            // pattern has no capture groups at all.
            Self::NoGroups
        } else {
            Self::Captures
        }
    }
}

/// `String.prototype.split(regexp)` followed by `_.without(result, '', ' ')`.
///
/// Two details a Rust `Regex::split` would lose:
///
/// * **capture groups are interleaved into the result.** This is not a curiosity
///   — it is the entire mechanism of [`WordPunctTokenizer`], whose pattern
///   matches everything and returns its tokens through a capture.
/// * **`_.without` removes only exactly `""` and exactly `" "`.** Not tabs, not
///   NBSP, not two spaces. The idiomatic `filter(|s| !s.trim().is_empty())` would
///   discard tokens the reference keeps.
///
/// [`split_into_captures`] is the direct transcription of those semantics and
/// the reference the other two routes are differentially tested against; the
/// dispatch itself never changes what comes out, only how fast it does.
fn split_into<'a>(
    re: &Regex,
    strategy: SplitStrategy,
    text: &'a str,
    out: &mut Vec<Option<&'a str>>,
) {
    match strategy {
        SplitStrategy::Whitespace => split_whitespace_into(text, out),
        SplitStrategy::NoGroups => split_into_no_groups(re, text, out),
        SplitStrategy::Captures => split_into_captures(re, text, out),
    }
}

/// The general [`split_into`] loop, for patterns with capture groups.
fn split_into_captures<'a>(re: &Regex, text: &'a str, out: &mut Vec<Option<&'a str>>) {
    let mut push = |t: Option<&'a str>| {
        if t != Some("") && t != Some(" ") {
            out.push(t);
        }
    };
    let mut p = 0;
    for caps in re.captures_iter(text) {
        let Some(m) = caps.get(0) else { continue };
        // The reference language skips a match that ends where the current piece began,
        // which is how a zero-width pattern avoids emitting infinite empties.
        if m.end() == p {
            continue;
        }
        push(Some(&text[p..m.start()]));
        for i in 1..caps.len() {
            push(caps.get(i).map(|c| c.as_str()));
        }
        p = m.end();
    }
    push(Some(&text[p..]));
}

/// [`split_into_captures`] for a pattern with no capture groups.
///
/// With `captures_len() == 1` the interleaving loop is a no-op and the only
/// thing the captures loop reads from each match is group 0's span — exactly
/// what [`Regex::find_iter`] yields, with the same leftmost-first spans from
/// the same meta engine, minus a per-match second pass that resolves group
/// slots which do not exist and a per-match heap-allocated `Captures` clone.
/// The body is otherwise kept line-for-line parallel to the captures loop,
/// including the zero-width skip, which group-free patterns such as `x*` and
/// `\b` still reach.
fn split_into_no_groups<'a>(re: &Regex, text: &'a str, out: &mut Vec<Option<&'a str>>) {
    let mut push = |t: &'a str| {
        if !t.is_empty() && t != " " {
            out.push(Some(t));
        }
    };
    let mut p = 0;
    for m in re.find_iter(text) {
        if m.end() == p {
            continue;
        }
        push(&text[p..m.start()]);
        p = m.end();
    }
    push(&text[p..]);
}

/// The ASCII rows of Unicode `White_Space`: TAB through CR, and the space.
///
/// Deliberately **not** [`u8::is_ascii_whitespace`], which omits VT (U+000B)
/// — `\s` includes it. Pinned against [`char::is_whitespace`] for every ASCII
/// byte by a test below.
#[inline]
const fn is_ascii_white_space(b: u8) -> bool {
    matches!(b, b'\t'..=b'\r' | b' ')
}

/// [`split_into_captures`] specialized to the default-mode `\s+`.
///
/// Rust-regex `\s` is `\p{White_Space}`, which is also exactly
/// [`char::is_whitespace`]; a test below re-proves that over every scalar
/// value, so a Unicode-table skew between the linked `regex` and `std` would
/// fail there first. It is **not** the reference-language `\s` of
/// [`crate::whitespace::SPACE_CLASS`] — that class keeps U+FEFF and drops
/// U+0085, so reaching for it here would change behaviour.
///
/// Against `\s+` the general split collapses:
///
/// * matches are the maximal `White_Space` runs, so every gap piece is a
///   maximal run of non-whitespace characters — the `" "` half of the
///   `_.without` filter can never fire, and the `""` half fires exactly for
///   the pieces flanking leading/trailing whitespace (and the empty input's
///   lone tail piece);
/// * `\s+` consumes at least one character, so no match can end where the
///   previous piece began — the zero-width skip is unreachable;
/// * there are no capture groups to interleave.
///
/// What remains is: push every maximal non-empty run of non-whitespace
/// characters. The scanner below does that with a byte loop that decodes only
/// non-ASCII characters, plus a word-at-a-time skip over the printable-ASCII
/// bytes `0x21..=0x7F` that make up nearly all of a typical token: in
/// `w - 0x21×LO`, every lane holding a byte below `0x21` produces a borrow
/// (its high bit survives `& !w`, and it stays set even when a lower lane
/// borrows into it), and `w & HI` flags every byte `>= 0x80` — so a zero
/// combined mask proves all eight bytes are printable ASCII, and a non-zero
/// mask's lowest flagged lane never lies past the first non-printable byte —
/// a real candidate is always flagged — so every byte below it is printable
/// ASCII. A byte flagged spuriously by borrow propagation (an `0x21` above a
/// real candidate) merely falls through to the exact per-character check.
fn split_whitespace_into<'a>(text: &'a str, out: &mut Vec<Option<&'a str>>) {
    /// The low bit of every lane, for building per-lane subtrahends.
    const LO: u64 = 0x0101_0101_0101_0101;
    /// The high bit of every lane, where borrow and non-ASCII flags land.
    const HI: u64 = 0x8080_8080_8080_8080;
    let bytes = text.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    while i < len {
        // Separator phase: usually a single space, so scalar is enough.
        let b = bytes[i];
        if b < 0x80 {
            if is_ascii_white_space(b) {
                i += 1;
                continue;
            }
        } else {
            // A non-ASCII leading byte is always a character boundary; the
            // fallback is unreachable, mirroring `scan::char_at`.
            let c = text[i..].chars().next().unwrap_or('\u{fffd}');
            if c.is_whitespace() {
                i += c.len_utf8();
                continue;
            }
        }
        // Token phase: `i` sits on a non-whitespace character, so the run is
        // non-empty and the `_.without` filter provably has nothing to do.
        let start = i;
        loop {
            while let Some(&chunk) = bytes[i..].first_chunk::<8>() {
                let w = u64::from_le_bytes(chunk);
                let mask = (w.wrapping_sub(LO * 0x21) & !w & HI) | (w & HI);
                if mask != 0 {
                    // Jump to the first byte outside 0x21..=0x7F; everything
                    // below that lane is printable ASCII (see above), so the
                    // jump can neither skip whitespace nor split a character.
                    i += (mask.trailing_zeros() / 8) as usize;
                    break;
                }
                i += 8;
            }
            if i >= len {
                break;
            }
            let b = bytes[i];
            if b < 0x80 {
                if is_ascii_white_space(b) {
                    break;
                }
                i += 1;
            } else {
                let c = text[i..].chars().next().unwrap_or('\u{fffd}');
                if c.is_whitespace() {
                    break;
                }
                i += c.len_utf8();
            }
        }
        out.push(Some(&text[start..i]));
    }
}

/// `String.prototype.match(regexp)`, whose shape depends on the `g` flag.
///
/// Returns `false` for the reference's `null`.
fn match_into<'a>(pattern: &Pattern, text: &'a str, out: &mut Vec<Option<&'a str>>) -> bool {
    if pattern.global {
        let before = out.len();
        out.extend(pattern.regex.find_iter(text).map(|m| Some(m.as_str())));
        return out.len() > before;
    }
    let Some(caps) = pattern.regex.captures(text) else {
        return false;
    };
    for i in 0..caps.len() {
        out.push(caps.get(i).map(|c| c.as_str()));
    }
    true
}

// ---------------------------------------------------------------------------
// WordTokenizer
// ---------------------------------------------------------------------------

/// Word-character class for [`WordTokenizer`]: ASCII alphanumerics, `_`, and
/// Cyrillic `А-Я` / `а-я`. Note that `Ё` and `ё` are *not* included.
#[derive(Debug, Clone, Copy)]
pub struct WordClass;

impl CharClass for WordClass {
    #[inline]
    fn is_word(c: char) -> bool {
        classes::is_word_word(c)
    }
}

/// Splits on `[^A-Za-zА-Яа-я0-9_]+`.
///
/// ```
/// use verbora_tokenizers::WordTokenizer;
/// let t = WordTokenizer::new();
/// assert_eq!(t.tokenize("She said 'hello'. Привет мир 123_456"),
///            Some(vec!["She", "said", "hello", "Привет", "мир", "123_456"]));
/// ```
///
/// A caller-supplied `pattern` is accepted and discarded by the reference
/// constructor, so this type takes none.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WordTokenizer {
    gaps: bool,
}

impl WordTokenizer {
    /// Splitting mode — the default.
    #[inline]
    pub const fn new() -> Self {
        Self { gaps: true }
    }

    /// Matching mode (`gaps: false`): returns the first run of separators, since
    /// the fixed pattern is not global.
    #[inline]
    pub const fn matching() -> Self {
        Self { gaps: false }
    }

    /// Lazily yields tokens. `None` is the reference's `null`.
    pub fn tokens<'a>(&self, text: &'a str) -> Option<WordTokens<'a>> {
        if self.gaps {
            return Some(WordTokens::Runs(WordRuns::new(text)));
        }
        // `s.match(/[^…]+/)` without `g`: the first separator run, or null.
        let start = text.char_indices().find(|(_, c)| !WordClass::is_word(*c))?;
        let end = text[start.0..]
            .char_indices()
            .find(|(_, c)| WordClass::is_word(*c))
            .map_or(text.len(), |(i, _)| start.0 + i);
        Some(WordTokens::First(Some(&text[start.0..end])))
    }

    /// Collects every token. `None` is the reference's `null`.
    pub fn tokenize<'a>(&self, text: &'a str) -> Option<Vec<&'a str>> {
        Some(self.tokens(text)?.collect())
    }

    /// Appends tokens to `out`, returning `false` where the reference returned
    /// `null`.
    pub fn tokenize_into<'a>(&self, text: &'a str, out: &mut Vec<&'a str>) -> bool {
        match self.tokens(text) {
            Some(it) => {
                out.extend(it);
                true
            }
            None => false,
        }
    }
}

/// Iterator over [`WordTokenizer`] tokens.
#[derive(Debug, Clone)]
pub enum WordTokens<'a> {
    /// Splitting mode: every run of word characters.
    Runs(WordRuns<'a, WordClass>),
    /// Matching mode: the single match a non-global pattern produces.
    First(Option<&'a str>),
}

impl<'a> Iterator for WordTokens<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<&'a str> {
        match self {
            Self::Runs(it) => it.next(),
            Self::First(slot) => slot.take(),
        }
    }
}

// ---------------------------------------------------------------------------
// OrthographyTokenizer
// ---------------------------------------------------------------------------

/// Word-character class for Finnish: `[A-Za-zÅåÄäÖö]`.
#[derive(Debug, Clone, Copy)]
pub struct FinnishClass;

impl CharClass for FinnishClass {
    #[inline]
    fn is_word(c: char) -> bool {
        classes::is_word_fi(c)
    }
}

/// Splits using a per-language alphabet, falling back to [`WordTokenizer`].
///
/// Only Finnish is defined in the reference's matcher table.
///
/// ```
/// use verbora_tokenizers::OrthographyTokenizer;
/// let fi = OrthographyTokenizer::new("fi");
/// assert_eq!(fi.tokenize("Hyvää, kiitos!!  entä").unwrap(), ["Hyvää", "kiitos", "entä"]);
/// // An unknown language falls back, and — because the fallback is constructed
/// // with no options at all — ignores `gaps`.
/// let zz = OrthographyTokenizer::matching("zz");
/// assert_eq!(zz.tokenize("She said 'hello'.").unwrap(), ["She", "said", "hello"]);
/// ```
///
/// # Why the token type is [`Utf16Token`] and not `&str`
///
/// In splitting mode every token is a slice of the input. In *matching* mode it
/// need not be: the Finnish matcher `[^A-Za-zÅåÄäÖö]` has no `+`, so it matches
/// exactly one UTF-16 code unit — and for an astral character that is the high
/// surrogate alone, which no `&str` can hold. The type covers both modes;
/// [`Utf16Token::as_str`] gets the slice back, and never returns `None` in
/// splitting mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OrthographyTokenizer {
    /// Whether the requested language had a matcher; if not, the fallback runs.
    finnish: bool,
    gaps: bool,
}

impl OrthographyTokenizer {
    /// Splitting mode — the default.
    #[inline]
    pub fn new(language: &str) -> Self {
        Self {
            finnish: language == "fi",
            gaps: true,
        }
    }

    /// Matching mode (`gaps: false`).
    ///
    /// Only reaches the delegate when the language is known: The reference builds
    /// its fallback with `new WordTokenizer()` — no options — so an unknown
    /// language silently discards `gaps`.
    #[inline]
    pub fn matching(language: &str) -> Self {
        Self {
            finnish: language == "fi",
            gaps: false,
        }
    }

    /// Lazily yields tokens. `None` is the reference's `null`.
    pub fn tokens<'a>(&self, text: &'a str) -> Option<OrthographyTokens<'a>> {
        if !self.finnish {
            return WordTokenizer::new()
                .tokens(text)
                .map(OrthographyTokens::Fallback);
        }
        if self.gaps {
            return Some(OrthographyTokens::Runs(WordRuns::new(text)));
        }
        // The Finnish matcher has no `+`, so a match is one code *unit*: for an
        // astral character that is the high surrogate on its own.
        let (i, c) = text
            .char_indices()
            .find(|(_, c)| !FinnishClass::is_word(*c))?;
        let token = if c.len_utf16() == 2 {
            Utf16Token::from_units(&[high_surrogate(c)])
        } else {
            Utf16Token::borrowed(&text[i..i + c.len_utf8()])
        };
        Some(OrthographyTokens::First(Some(token)))
    }

    /// Collects every token. `None` is the reference's `null`.
    pub fn tokenize<'a>(&self, text: &'a str) -> Option<Vec<Utf16Token<'a>>> {
        Some(self.tokens(text)?.collect())
    }

    /// Appends tokens to `out`, returning `false` where the reference returned
    /// `null`.
    pub fn tokenize_into<'a>(&self, text: &'a str, out: &mut Vec<Utf16Token<'a>>) -> bool {
        match self.tokens(text) {
            Some(it) => {
                out.extend(it);
                true
            }
            None => false,
        }
    }
}

/// Iterator over [`OrthographyTokenizer`] tokens.
#[derive(Debug, Clone)]
pub enum OrthographyTokens<'a> {
    /// Finnish, splitting mode.
    Runs(WordRuns<'a, FinnishClass>),
    /// Finnish, matching mode.
    First(Option<Utf16Token<'a>>),
    /// Unknown language: delegated to [`WordTokenizer`].
    Fallback(WordTokens<'a>),
}

impl<'a> Iterator for OrthographyTokens<'a> {
    type Item = Utf16Token<'a>;

    fn next(&mut self) -> Option<Utf16Token<'a>> {
        match self {
            Self::Runs(it) => it.next().map(Utf16Token::borrowed),
            Self::First(slot) => slot.take(),
            Self::Fallback(it) => it.next().map(Utf16Token::borrowed),
        }
    }
}

// ---------------------------------------------------------------------------
// WordPunctTokenizer
// ---------------------------------------------------------------------------

/// What the `WordPunctTokenizer` pattern matched at one position.
#[derive(Debug, Clone, Copy)]
enum Hit {
    /// A run of the letter or digit class, ending at this byte offset.
    Run(usize),
    /// A single BMP character, ending at this byte offset.
    Single(usize),
    /// An astral character: the reference matches its two surrogate halves as two
    /// separate single-code-unit tokens.
    Astral(char),
}

/// Attempts the reference pattern at byte offset `q`.
///
/// The pattern is
/// `/([A-Za-zÀ-ÿ-]+|[0-9._]+|.|!|\?|'|"|:|;|,|-)/i`; the alternatives after the
/// bare `.` are unreachable — `.` already matches every one of them — so only
/// three cases remain. Alternation is ordered, so the letter run is tried first.
fn wordpunct_hit(text: &str, q: usize) -> Option<Hit> {
    let rest = text.get(q..)?;
    let c = rest.chars().next()?;

    if classes::is_word_wordpunct_letter(c) {
        let end = rest
            .char_indices()
            .find(|(_, c)| !classes::is_word_wordpunct_letter(*c))
            .map_or(text.len(), |(i, _)| q + i);
        return Some(Hit::Run(end));
    }
    if classes::is_word_wordpunct_digit(c) {
        let end = rest
            .char_indices()
            .find(|(_, c)| !classes::is_word_wordpunct_digit(*c))
            .map_or(text.len(), |(i, _)| q + i);
        return Some(Hit::Run(end));
    }
    // A bare `.` refuses the four line terminators, which is why newlines survive
    // as *gap* text rather than as matches.
    if is_line_terminator(c) {
        return None;
    }
    if c.len_utf16() == 2 {
        return Some(Hit::Astral(c));
    }
    Some(Hit::Single(q + c.len_utf8()))
}

/// Splits text into alphabetic runs, numeric runs and single characters.
///
/// ```
/// use verbora_tokenizers::WordPunctTokenizer;
/// let t = WordPunctTokenizer::new();
/// assert_eq!(t.tokenize("She said 'hello'.").unwrap(),
///            ["She", "said", "'", "hello", "'", "."]);
/// assert_eq!(t.tokenize("café-au-lait").unwrap(), ["café-au-lait"]);
/// assert_eq!(t.tokenize("1.100").unwrap(), ["1.100"]);
/// ```
///
/// # What survives and what does not
///
/// The reference splits on a pattern that matches nearly everything and returns
/// its tokens through a capture group, then filters the result with
/// `_.without(results, '', ' ')`. The consequences are worth stating explicitly:
///
/// * a single ASCII space is matched and then filtered away;
/// * a **tab** or an NBSP is matched and **kept** as its own token;
/// * a newline is *not* matched (`.` excludes line terminators), so it is emitted
///   as the un-matched gap between two matches — and therefore also kept;
/// * an astral character becomes **two** tokens, one per surrogate half, which is
///   why the token type is [`Utf16Token`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WordPunctTokenizer {
    gaps: bool,
}

impl WordPunctTokenizer {
    /// Splitting mode — the default.
    #[inline]
    pub const fn new() -> Self {
        Self { gaps: true }
    }

    /// Matching mode (`gaps: false`): the pattern is not global, so the result is
    /// the first match followed by its single capture group — the same text
    /// twice.
    #[inline]
    pub const fn matching() -> Self {
        Self { gaps: false }
    }

    /// Lazily yields tokens. `None` is the reference's `null`.
    pub fn tokens<'a>(&self, text: &'a str) -> Option<WordPunctTokens<'a>> {
        if self.gaps {
            return Some(WordPunctTokens::Split {
                text,
                piece: 0,
                pos: 0,
                finished: false,
                next1: None,
                next2: None,
            });
        }
        let (q, hit) = text
            .char_indices()
            .find_map(|(i, _)| wordpunct_hit(text, i).map(|h| (i, h)))?;
        let token = match hit {
            Hit::Run(end) | Hit::Single(end) => Utf16Token::borrowed(&text[q..end]),
            Hit::Astral(c) => Utf16Token::from_units(&[high_surrogate(c)]),
        };
        Some(WordPunctTokens::Match {
            // `[fullMatch, group1]`, and group 1 is the whole match.
            first: Some(token.clone()),
            second: Some(token),
        })
    }

    /// Collects every token. `None` is the reference's `null`.
    pub fn tokenize<'a>(&self, text: &'a str) -> Option<Vec<Utf16Token<'a>>> {
        Some(self.tokens(text)?.collect())
    }

    /// Appends tokens to `out`, returning `false` where the reference returned
    /// `null`.
    pub fn tokenize_into<'a>(&self, text: &'a str, out: &mut Vec<Utf16Token<'a>>) -> bool {
        match self.tokens(text) {
            Some(it) => {
                out.extend(it);
                true
            }
            None => false,
        }
    }
}

/// The high half of an astral character's surrogate pair.
#[inline]
fn high_surrogate(c: char) -> u16 {
    let mut buf = [0u16; 2];
    c.encode_utf16(&mut buf);
    buf[0]
}

/// The low half of an astral character's surrogate pair.
#[inline]
fn low_surrogate(c: char) -> u16 {
    let mut buf = [0u16; 2];
    c.encode_utf16(&mut buf);
    buf[1]
}

/// Iterator over [`WordPunctTokenizer`] tokens.
#[derive(Debug, Clone)]
pub enum WordPunctTokens<'a> {
    /// Splitting mode.
    Split {
        /// The input.
        text: &'a str,
        /// Start of the current un-matched piece (`p` in the reference language
        /// algorithm).
        piece: usize,
        /// Current scan position (`q`).
        pos: usize,
        /// Whether the trailing piece has been emitted.
        finished: bool,
        /// First deferred token — a step can produce up to three.
        next1: Option<Utf16Token<'a>>,
        /// Second deferred token.
        next2: Option<Utf16Token<'a>>,
    },
    /// Matching mode: the match and its capture group.
    Match {
        /// The full match.
        first: Option<Utf16Token<'a>>,
        /// Capture group 1, which is the same text.
        second: Option<Utf16Token<'a>>,
    },
}

impl<'a> Iterator for WordPunctTokens<'a> {
    type Item = Utf16Token<'a>;

    fn next(&mut self) -> Option<Utf16Token<'a>> {
        match self {
            Self::Match { first, second } => first.take().or_else(|| second.take()),
            Self::Split {
                text,
                piece,
                pos,
                finished,
                next1,
                next2,
            } => {
                loop {
                    if let Some(t) = next1.take() {
                        *next1 = next2.take();
                        return Some(t);
                    }
                    if *finished {
                        return None;
                    }
                    if *pos >= text.len() {
                        *finished = true;
                        let tail = &text[*piece..];
                        if !tail.is_empty() && tail != " " {
                            return Some(Utf16Token::borrowed(tail));
                        }
                        return None;
                    }
                    let Some(hit) = wordpunct_hit(text, *pos) else {
                        // Un-matchable: the character joins the gap text.
                        *pos += text[*pos..].chars().next().map_or(1, char::len_utf8);
                        continue;
                    };

                    // `_.without(results, '', ' ')` drops exactly the empty gap
                    // and the single-space match, and nothing else.
                    let mut queued: [Option<Utf16Token<'a>>; 3] = [None, None, None];
                    let mut n = 0;
                    let gap = &text[*piece..*pos];
                    if !gap.is_empty() && gap != " " {
                        queued[n] = Some(Utf16Token::borrowed(gap));
                        n += 1;
                    }
                    match hit {
                        Hit::Run(end) | Hit::Single(end) => {
                            let m = &text[*pos..end];
                            if m != " " {
                                queued[n] = Some(Utf16Token::borrowed(m));
                                n += 1;
                            }
                            *pos = end;
                        }
                        Hit::Astral(c) => {
                            queued[n] = Some(Utf16Token::from_units(&[high_surrogate(c)]));
                            queued[n + 1] = Some(Utf16Token::from_units(&[low_surrogate(c)]));
                            n += 2;
                            *pos += c.len_utf8();
                        }
                    }
                    debug_assert!(n <= 3);
                    *piece = *pos;

                    let [a, b, c] = queued;
                    if let Some(a) = a {
                        *next1 = b;
                        *next2 = c;
                        return Some(a);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wp(s: &str) -> Vec<String> {
        WordPunctTokenizer::new()
            .tokens(s)
            .into_iter()
            .flatten()
            .map(|t| t.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn word_tokenizer_splits_on_the_fixed_class() {
        let t = WordTokenizer::new();
        assert_eq!(t.tokenize(""), Some(vec![]));
        assert_eq!(
            t.tokenize("She said 'hello'. Привет мир 123_456"),
            Some(vec!["She", "said", "hello", "Привет", "мир", "123_456"])
        );
        // Ё is outside А-Я and therefore a separator.
        assert_eq!(t.tokenize("Ёж"), Some(vec!["ж"]));
    }

    #[test]
    fn word_tokenizer_match_mode_can_return_null() {
        let t = WordTokenizer::matching();
        assert_eq!(t.tokenize("abc def"), Some(vec![" "]));
        assert_eq!(t.tokenize("abcdef"), None);
    }

    #[test]
    fn orthography_finnish_and_fallback() {
        assert_eq!(
            OrthographyTokenizer::new("fi")
                .tokenize("Hyvää, kiitos!!  entä")
                .unwrap(),
            ["Hyvää", "kiitos", "entä"]
        );
        assert!(
            OrthographyTokenizer::new("fi")
                .tokenize("")
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            OrthographyTokenizer::new("zz")
                .tokenize("She said 'hello'.")
                .unwrap(),
            ["She", "said", "hello"]
        );
    }

    #[test]
    fn wordpunct_token_shapes() {
        assert_eq!(
            wp("She said 'hello'."),
            ["She", "said", "'", "hello", "'", "."]
        );
        assert_eq!(wp("a×b"), ["a×b"]);
        assert_eq!(wp("a÷b"), ["a÷b"]);
        assert_eq!(wp("a--b"), ["a--b"]);
        assert_eq!(wp("a1.2b"), ["a", "1.2", "b"]);
        assert_eq!(wp("a__b"), ["a", "__", "b"]);
        assert_eq!(wp(".."), [".."]);
        assert_eq!(wp("日本語"), ["日", "本", "語"]);
        assert_eq!(wp(""), Vec::<String>::new());
        assert_eq!(wp("  "), Vec::<String>::new());
    }

    #[test]
    fn wordpunct_keeps_tabs_and_newlines_but_drops_spaces() {
        assert_eq!(wp("\t"), ["\t"]);
        assert_eq!(wp("a\tb"), ["a", "\t", "b"]);
        assert_eq!(wp("a\u{a0}b"), ["a", "\u{a0}", "b"]);
        assert_eq!(wp("\n"), ["\n"]);
        assert_eq!(wp("a\nb"), ["a", "\n", "b"]);
        assert_eq!(wp("a\n\nb"), ["a", "\n\n", "b"]);
    }

    #[test]
    fn wordpunct_does_not_fold_non_ascii_onto_ascii() {
        // Rust's `(?i)` would fold U+212A onto `k` and U+017F onto `s`, joining
        // them into the letter run. The reference's `/i` does not.
        assert_eq!(wp("a\u{212a}b"), ["a", "\u{212a}", "b"]);
        assert_eq!(wp("a\u{17f}b"), ["a", "\u{17f}", "b"]);
        // U+0178 is admitted, because `ÿ` uppercases to it.
        assert_eq!(wp("a\u{178}b"), ["a\u{178}b"]);
    }

    #[test]
    fn wordpunct_splits_astral_characters_in_two() {
        let toks: Vec<Vec<u16>> = WordPunctTokenizer::new()
            .tokens("a😀b")
            .into_iter()
            .flatten()
            .map(|t| t.to_utf16())
            .collect();
        assert_eq!(
            toks,
            vec![vec![0x61], vec![0xd83d], vec![0xde00], vec![0x62]]
        );
    }

    #[test]
    fn regexp_tokenizer_modes() {
        let split = RegexpTokenizer::new(Pattern::new(Regex::new(r"[^A-Za-z0-9_]+").unwrap()));
        assert_eq!(split.tokenize("  a b  "), Some(vec![Some("a"), Some("b")]));

        // Capture groups are interleaved into the split result. Note that the
        // gap `"xx "` keeps its trailing space — `_.without` only removes a piece
        // that is *exactly* one space.
        let grouped = RegexpTokenizer::new(Pattern::new(Regex::new(r"([a-z])([0-9])").unwrap()));
        assert_eq!(
            grouped.tokenize("xx a1 b2"),
            Some(vec![
                Some("xx "),
                Some("a"),
                Some("1"),
                Some("b"),
                Some("2")
            ])
        );

        // A capture group that did not participate is the reference's `undefined`.
        let optional = RegexpTokenizer::new(Pattern::new(Regex::new(r"(x)|([0-9])").unwrap()));
        assert_eq!(
            optional.tokenize("a1b"),
            Some(vec![Some("a"), None, Some("1"), Some("b")])
        );

        // Non-global match mode returns the match and its groups.
        let m = RegexpTokenizer::matching(Pattern::new(Regex::new(r"([a-z])([0-9])").unwrap()));
        assert_eq!(
            m.tokenize("xx a1 b2"),
            Some(vec![Some("a1"), Some("a"), Some("1")])
        );

        // No match is `null`, not an empty array.
        let none = RegexpTokenizer::matching(Pattern::global(Regex::new(r"[a-z]+").unwrap()));
        assert_eq!(none.tokenize("123"), None);

        assert_eq!(
            RegexpTokenizer::without_pattern().tokenize("a b"),
            Some(vec![Some("a b")])
        );
    }

    // -----------------------------------------------------------------------
    // split_into differential suites
    //
    // `split_into_captures` is the reference transcription; every faster
    // route must produce not merely equal text but the *same slices* of the
    // input, which is what `assert_same_slices` checks.
    // -----------------------------------------------------------------------

    /// A tiny, dependency-free PRNG — this crate has no `rand` dependency
    /// and does not need one just for this.
    struct Xorshift64(u64);

    impl Xorshift64 {
        fn next_u64(&mut self) -> u64 {
            let mut x = self.0;
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            self.0 = x;
            x
        }

        fn next_range(&mut self, bound: usize) -> usize {
            (self.next_u64() % bound as u64) as usize
        }

        fn choose<'a, T>(&mut self, items: &'a [T]) -> &'a T {
            &items[self.next_range(items.len())]
        }
    }

    /// Every character class the whitespace fast path branches on: the ASCII
    /// whitespace block (VT included — the byte `u8::is_ascii_whitespace`
    /// misses), Unicode whitespace of every UTF-8 width (NEL and NBSP are
    /// where Rust's `\s` and the reference-language class disagree), the
    /// non-whitespace lookalikes ZWSP and BOM, ASCII controls on both sides
    /// of the whitespace range, printable ASCII including the `0x21`/`0x7F`
    /// SWAR boundary bytes, and multibyte token characters.
    const STRESS_ALPHABET: &[&str] = &[
        " ", "\t", "\n", "\x0B", "\x0C", "\r", "\u{85}", "\u{A0}", "\u{1680}", "\u{2000}",
        "\u{2005}", "\u{200A}", "\u{2028}", "\u{2029}", "\u{202F}", "\u{205F}", "\u{3000}",
        "\u{200B}", "\u{FEFF}", "\x00", "\x08", "\x0E", "\x1F", "!", "~", "\x7F", "a", "b", "z",
        ".", "0", "_", "é", "ж", "ᚠ", "日", "😀",
    ];

    fn random_text(rng: &mut Xorshift64) -> String {
        let pieces = rng.next_range(40);
        let mut s = String::new();
        for _ in 0..pieces {
            s.push_str(rng.choose(STRESS_ALPHABET));
        }
        s
    }

    fn captures_split<'a>(re: &Regex, text: &'a str) -> Vec<Option<&'a str>> {
        let mut out = Vec::new();
        split_into_captures(re, text, &mut out);
        out
    }

    fn no_groups_split<'a>(re: &Regex, text: &'a str) -> Vec<Option<&'a str>> {
        let mut out = Vec::new();
        split_into_no_groups(re, text, &mut out);
        out
    }

    fn whitespace_split(text: &str) -> Vec<Option<&str>> {
        let mut out = Vec::new();
        split_whitespace_into(text, &mut out);
        out
    }

    /// Asserts two split results are the same *slices* of the input — a fast
    /// path that produced the right words from the wrong offsets would pass a
    /// plain `==` on text alone.
    fn assert_same_slices(want: &[Option<&str>], got: &[Option<&str>], ctx: &dyn Fn() -> String) {
        assert_eq!(want, got, "{}", ctx());
        for (w, g) in want.iter().zip(got) {
            if let (Some(w), Some(g)) = (w, g) {
                assert_eq!(w.as_ptr(), g.as_ptr(), "slice identity: {}", ctx());
            }
        }
    }

    #[test]
    fn whitespace_fast_path_matches_the_regex_engine_on_random_text() {
        let re = Regex::new(r"\s+").unwrap();
        let mut rng = Xorshift64(0x5EED_5EED_5EED_0001);
        for round in 0..1000 {
            let text = random_text(&mut rng);
            let ctx = || format!("round {round}: {text:?}");
            let want = captures_split(&re, &text);
            assert_same_slices(&want, &no_groups_split(&re, &text), &ctx);
            assert_same_slices(&want, &whitespace_split(&text), &ctx);
        }
    }

    #[test]
    fn find_iter_path_matches_the_captures_path_for_group_free_patterns() {
        // `x*` and `\b` reach the zero-width skip; `[a-z]` produces the
        // `""`-between-adjacent-matches pieces; `(?:\s)+` is a non-capturing
        // spelling that must *not* be mistaken for the whitespace pattern.
        let patterns = [
            r"[^A-Za-z0-9_]+",
            r"(?:\s)+",
            r"x*",
            r"\b",
            r"[a-z]",
            r"\d+",
        ];
        let mut rng = Xorshift64(0x5EED_5EED_5EED_0002);
        for pattern in patterns {
            let re = Regex::new(pattern).unwrap();
            assert_eq!(re.captures_len(), 1, "{pattern}");
            assert_eq!(SplitStrategy::of(&re), SplitStrategy::NoGroups, "{pattern}");
            for round in 0..300 {
                let text = random_text(&mut rng);
                let ctx = || format!("pattern {pattern}: round {round}: {text:?}");
                assert_same_slices(
                    &captures_split(&re, &text),
                    &no_groups_split(&re, &text),
                    &ctx,
                );
            }
        }
    }

    #[test]
    fn rust_regex_s_is_exactly_char_is_whitespace() {
        // The fast path substitutes `char::is_whitespace` for `\s`. This scan
        // re-proves the equivalence for every scalar value against the linked
        // regex/std pair, so a future Unicode-table skew fails here first.
        let re = Regex::new(r"\s").unwrap();
        let mut buf = [0u8; 4];
        for u in 0..=0x10FFFFu32 {
            let Some(c) = char::from_u32(u) else { continue };
            assert_eq!(
                re.is_match(c.encode_utf8(&mut buf)),
                c.is_whitespace(),
                "U+{u:04X}"
            );
        }
    }

    #[test]
    fn ascii_white_space_matches_char_is_whitespace_on_every_ascii_byte() {
        for b in 0u8..0x80 {
            assert_eq!(
                is_ascii_white_space(b),
                (b as char).is_whitespace(),
                "0x{b:02X}"
            );
        }
    }

    #[test]
    fn split_strategy_selection_is_pinned() {
        let of = |p: &str| SplitStrategy::of(&Regex::new(p).unwrap());
        assert_eq!(of(r"\s+"), SplitStrategy::Whitespace);
        // Equivalent spellings stay off the fast path — the probe keys on the
        // exact source text, and the general routes are correct for them.
        assert_eq!(of(r"(?:\s)+"), SplitStrategy::NoGroups);
        assert_eq!(of(r"\s\s*"), SplitStrategy::NoGroups);
        assert_eq!(of(r"[^A-Za-z0-9_]+"), SplitStrategy::NoGroups);
        assert_eq!(of(r"([a-z])([0-9])"), SplitStrategy::Captures);
        assert_eq!(of(r"(x)|([0-9])"), SplitStrategy::Captures);
    }

    #[test]
    fn ascii_only_s_plus_is_not_given_the_unicode_fast_path() {
        // `RegexBuilder::unicode(false)` shrinks `\s` to ASCII under the same
        // `as_str()`; the NBSP probe must catch it.
        let re = regex::RegexBuilder::new(r"\s+")
            .unicode(false)
            .build()
            .unwrap();
        assert_eq!(SplitStrategy::of(&re), SplitStrategy::NoGroups);
        let t = RegexpTokenizer::new(Pattern::new(re));
        assert_eq!(t.tokenize("a\u{a0}b"), Some(vec![Some("a\u{a0}b")]));
        assert_eq!(t.tokenize(" a\u{3000}b "), Some(vec![Some("a\u{3000}b")]));
    }

    #[test]
    fn builder_flags_that_keep_the_fast_path_cannot_change_its_output() {
        // Each of these still matches NBSP, so dispatch sends it to the
        // scanner; the scanner must therefore agree with what the captures
        // loop would have produced *under that flag*. `swap_greed` is the
        // interesting one — a lazy `\s+?` matches one whitespace character at
        // a time, and only the `_.without` filter makes that indistinguishable.
        let builds: &[(&str, Regex)] = &[
            ("swap_greed", {
                regex::RegexBuilder::new(r"\s+")
                    .swap_greed(true)
                    .build()
                    .unwrap()
            }),
            ("case_insensitive", {
                regex::RegexBuilder::new(r"\s+")
                    .case_insensitive(true)
                    .build()
                    .unwrap()
            }),
            ("multi_line+crlf", {
                regex::RegexBuilder::new(r"\s+")
                    .multi_line(true)
                    .crlf(true)
                    .build()
                    .unwrap()
            }),
            ("dot_matches_new_line", {
                regex::RegexBuilder::new(r"\s+")
                    .dot_matches_new_line(true)
                    .build()
                    .unwrap()
            }),
            ("ignore_whitespace", {
                regex::RegexBuilder::new(r"\s+")
                    .ignore_whitespace(true)
                    .build()
                    .unwrap()
            }),
        ];
        let mut rng = Xorshift64(0x5EED_5EED_5EED_0003);
        for (name, re) in builds {
            assert_eq!(SplitStrategy::of(re), SplitStrategy::Whitespace, "{name}");
            for round in 0..200 {
                let text = random_text(&mut rng);
                let ctx = || format!("flag {name}: round {round}: {text:?}");
                assert_same_slices(&captures_split(re, &text), &whitespace_split(&text), &ctx);
            }
        }
    }

    #[test]
    fn whitespace_split_edge_corpus() {
        let t = RegexpTokenizer::new(Pattern::new(Regex::new(r"\s+").unwrap()));
        let cases: &[(&str, &[Option<&str>])] = &[
            ("", &[]),
            (" ", &[]),
            ("  ", &[]),
            // One of each whitespace kind, including NEL, which the
            // reference-language class would *not* strip.
            ("\t\n\x0B\x0C\r \u{85}\u{A0}\u{1680}\u{2000}\u{3000}", &[]),
            ("a", &[Some("a")]),
            (" a", &[Some("a")]),
            ("a ", &[Some("a")]),
            ("a  b", &[Some("a"), Some("b")]),
            ("\ta\x0Bb", &[Some("a"), Some("b")]),
            ("a\u{A0}b", &[Some("a"), Some("b")]),
            ("a\u{85}b", &[Some("a"), Some("b")]),
            ("a\u{2028}b\u{2029}c", &[Some("a"), Some("b"), Some("c")]),
            // ZWSP and BOM are not `White_Space`: they stay inside the token.
            ("a\u{200B}b", &[Some("a\u{200B}b")]),
            ("a\u{FEFF}b", &[Some("a\u{FEFF}b")]),
            // Neither are the controls flanking the ASCII whitespace block.
            ("a\x00b", &[Some("a\x00b")]),
            ("a\x08b\x0Ec", &[Some("a\x08b\x0Ec")]),
            // Tokens of 7, 8, 9 and 16+ bytes straddle the SWAR chunk seam.
            ("abcdefg h", &[Some("abcdefg"), Some("h")]),
            ("abcdefgh i", &[Some("abcdefgh"), Some("i")]),
            ("abcdefghi j", &[Some("abcdefghi"), Some("j")]),
            (
                "abcdefghijklmnopq r",
                &[Some("abcdefghijklmnopq"), Some("r")],
            ),
            // A multibyte character straddling the first 8-byte window.
            ("abcdefgé x", &[Some("abcdefgé"), Some("x")]),
            ("😀 😀", &[Some("😀"), Some("😀")]),
            ("a \u{3000}\t b", &[Some("a"), Some("b")]),
        ];
        for (text, want) in cases {
            assert_eq!(t.tokenize(text).as_deref(), Some(*want), "{text:?}");
        }
    }

    #[test]
    fn whitespace_fast_path_matches_on_benchmark_shaped_documents() {
        // Lowercase ASCII words joined by single spaces — the shape of the
        // competitive benchmark corpus, at the sizes it measures.
        let re = Regex::new(r"\s+").unwrap();
        let mut rng = Xorshift64(0x5EED_5EED_5EED_0004);
        for target in [123usize, 1_187, 9_709, 77_684] {
            let mut doc = String::new();
            while doc.len() < target {
                if !doc.is_empty() {
                    doc.push(' ');
                }
                for _ in 0..1 + rng.next_range(12) {
                    doc.push((b'a' + rng.next_range(26) as u8) as char);
                }
            }
            let ctx = || format!("target {target}");
            let want = captures_split(&re, &doc);
            assert_same_slices(&want, &no_groups_split(&re, &doc), &ctx);
            assert_same_slices(&want, &whitespace_split(&doc), &ctx);
        }
    }
}
