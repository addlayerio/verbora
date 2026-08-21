//! The UAX #29 sentence tokenizer and its abbreviation tailoring.

use std::fmt;

use unicode_segmentation::{USentenceBoundIndices, UnicodeSegmentation};
use verbora_core::{BorrowingTokenizer, Tokenizer};

/// The reason an abbreviation set was rejected.
///
/// Constructed only by [`SentenceTokenizer::with_abbreviations`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum AbbreviationError {
    /// An abbreviation was the empty string.
    ///
    /// Every string ends with `""`, so an empty abbreviation would suppress
    /// every interior boundary — the whole document would be one sentence, from
    /// a one-character constructor argument. The state is rejected rather than
    /// documented.
    Empty {
        /// The position of the offending abbreviation in the supplied iterator.
        index: usize,
    },
}

impl fmt::Display for AbbreviationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty { index } => {
                write!(f, "abbreviation at index {index} is the empty string")
            }
        }
    }
}

impl std::error::Error for AbbreviationError {}

/// Yields the UAX #29 sentences of the input, optionally suppressing breaks
/// that follow a caller-supplied abbreviation.
///
/// Sentences are the segments produced by the [UAX #29] §5 sentence boundary
/// rules SB1–SB998, in order, **with no trimming**: a sentence includes its own
/// trailing whitespace, so concatenation reproduces the input exactly and
/// `tokens("   ")` is one token, `["   "]`. Callers who want trimmed sentences
/// write `.map(str::trim)`; trimming here would make tokens that are not
/// substrings of the input.
///
/// # Guarantees
///
/// * `SentenceTokenizer::new().tokens(t).collect::<String>() == t`, for every
///   `t`. This holds with abbreviations configured too — suppression joins
///   adjacent segments, it never drops text.
/// * Every token is a contiguous slice of the input, at strictly increasing,
///   non-overlapping byte ranges.
/// * No token is ever the empty string. `tokens("")` yields nothing.
///
/// # Abbreviations are a tailoring, and the standard says one is needed
///
/// UAX #29 §5 breaks after any sentence terminator, so `"Dr. Smith"` is two
/// sentences under the default rules, and notes that abbreviation handling
/// requires tailoring. Verbora's tailoring is exactly this:
///
/// > Let `B` be the set of boundary positions the default rules produce over
/// > `text`. A position `b` with `0 < b < text.len()` is **suppressed** if some
/// > abbreviation `a` in the set satisfies
/// > `text[..b].trim_end_matches(char::is_whitespace).ends_with(a)`.
/// > Suppressed boundaries are not emitted; the segments on either side are
/// > joined. The final boundary at `text.len()` is never suppressed.
///
/// Four consequences, each deliberate:
///
/// * **Matching is case-sensitive**, and is an exact scalar-sequence
///   comparison. Case-insensitive matching needs a case-folding decision
///   (UAX #21) that belongs to the caller; a caller wanting both casings
///   supplies both strings.
/// * **Whitespace is Unicode `White_Space`** — `char::is_whitespace`. This is
///   the one place this crate consults a whitespace set at all.
/// * **Suppression is suffix matching, so it can over-suppress.** With `"no."`
///   in the set, `"Visit the casino. Then leave."` is *one* sentence, because
///   `"casino."` ends with `"no."`. Qualifying the match by a word boundary
///   would fix that case and break `"e.g."`, `"i.e."` and `"Ph.D."`, whose
///   interior periods are themselves boundaries. The caller controls the set.
/// * **The last sentence is never lost.** An abbreviation at end of input
///   suppresses nothing, because the boundary at `text.len()` is exempt.
///
/// # Examples
///
/// ```
/// use verbora_tokenizers::{BorrowingTokenizer, SentenceTokenizer};
///
/// let plain = SentenceTokenizer::new();
/// assert_eq!(
///     plain.tokenize_borrowed("Dr. Smith arrived. He left."),
///     ["Dr. ", "Smith arrived. ", "He left."],
/// );
///
/// let tailored = SentenceTokenizer::with_abbreviations(["Dr."]).unwrap();
/// assert_eq!(
///     tailored.tokenize_borrowed("Dr. Smith arrived. He left."),
///     ["Dr. Smith arrived. ", "He left."],
/// );
///
/// // Trailing whitespace belongs to the sentence; trim at the call site.
/// let trimmed: Vec<&str> = plain
///     .tokens("One. Two.")
///     .map(str::trim)
///     .collect();
/// assert_eq!(trimmed, ["One.", "Two."]);
/// ```
///
/// [UAX #29]: https://www.unicode.org/reports/tr29/
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SentenceTokenizer {
    abbreviations: Vec<String>,
}

impl SentenceTokenizer {
    /// Creates a tokenizer with no abbreviations: the untailored UAX #29 §5
    /// rules.
    #[must_use]
    pub fn new() -> Self {
        Self {
            abbreviations: Vec::new(),
        }
    }

    /// Creates a tokenizer that suppresses a boundary following any of
    /// `abbreviations`.
    ///
    /// Duplicates and order are irrelevant: suppression asks only whether
    /// *some* abbreviation matches.
    ///
    /// # Errors
    ///
    /// [`AbbreviationError::Empty`] if any abbreviation is the empty string,
    /// carrying the index of the first such entry.
    ///
    /// ```
    /// use verbora_tokenizers::{AbbreviationError, SentenceTokenizer};
    ///
    /// assert_eq!(
    ///     SentenceTokenizer::with_abbreviations(["Dr.", ""]),
    ///     Err(AbbreviationError::Empty { index: 1 }),
    /// );
    /// ```
    pub fn with_abbreviations<I, S>(abbreviations: I) -> Result<Self, AbbreviationError>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let mut out = Vec::new();
        for (index, abbreviation) in abbreviations.into_iter().enumerate() {
            let abbreviation = abbreviation.into();
            if abbreviation.is_empty() {
                return Err(AbbreviationError::Empty { index });
            }
            out.push(abbreviation);
        }
        Ok(Self { abbreviations: out })
    }

    /// The configured abbreviations, in the order they were supplied.
    #[must_use]
    pub fn abbreviations(&self) -> &[String] {
        &self.abbreviations
    }
}

impl BorrowingTokenizer for SentenceTokenizer {
    fn tokens<'a>(&self, text: &'a str) -> impl Iterator<Item = &'a str> {
        Sentences {
            text,
            inner: text.split_sentence_bound_indices(),
            abbreviations: &self.abbreviations,
            start: 0,
        }
    }
}

impl Tokenizer for SentenceTokenizer {
    fn tokenize_into(&self, text: &str, out: &mut Vec<String>) {
        out.extend(self.tokens(text).map(str::to_owned));
    }
}

/// The lazy sentence iterator returned by
/// [`SentenceTokenizer::tokens`](BorrowingTokenizer::tokens).
struct Sentences<'a, 'b> {
    text: &'a str,
    inner: USentenceBoundIndices<'a>,
    abbreviations: &'b [String],
    /// Byte offset of the sentence currently being accumulated.
    start: usize,
}

impl<'a> Iterator for Sentences<'a, '_> {
    type Item = &'a str;

    fn next(&mut self) -> Option<&'a str> {
        loop {
            let (offset, segment) = self.inner.next()?;
            let end = offset + segment.len();
            // The final boundary is never suppressed, so the last sentence can
            // never be swallowed.
            if end < self.text.len() && self.is_suppressed(end) {
                continue;
            }
            let sentence = self.text.get(self.start..end)?;
            self.start = end;
            return Some(sentence);
        }
    }
}

impl Sentences<'_, '_> {
    /// Whether the boundary at byte offset `at` follows an abbreviation.
    fn is_suppressed(&self, at: usize) -> bool {
        if self.abbreviations.is_empty() {
            return false;
        }
        let Some(head) = self.text.get(..at) else {
            return false;
        };
        let head = head.trim_end_matches(char::is_whitespace);
        self.abbreviations
            .iter()
            .any(|abbreviation| head.ends_with(abbreviation.as_str()))
    }
}
