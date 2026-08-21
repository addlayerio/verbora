//! `SentimentAnalyzer` — the constructor's lookup rules and the scoring loop.
//!
//! # The scoring loop, exactly
//!
//! The loop walks the token stream left to right and consumes it one **unit**
//! at a time. A unit is either a matched multi-token span or a single token:
//!
//! ```text
//! sum = 0; negator = +1; units = 0
//! while tokens remain:
//!     lower = lowercase(tokens[0])
//!     span  = vocabulary.longest_key_starting_with(lower)     // in pieces
//!     for k = min(span, tokens remaining) down to 2:          // longest first
//!         if vocabulary[join(lower(tokens[0..k]), " ")]:      exact
//!             sum += negator * polarity; units += 1; consume k; continue
//!         if stemmer and vocabulary[join(stem(tokens[0..k]), " ")]:
//!             sum += negator * polarity; units += 1; consume k; continue
//!     if lower in negations:  negator = -1                    // and never back
//!     else if vocabulary[lower]:            sum += negator * polarity
//!     else if stemmer and vocabulary[stem(lower)]: sum += negator * polarity
//!     units += 1; consume 1
//! return sum / units
//! ```
//!
//! Five details in there are what a careful reading gets wrong:
//!
//! * **A key is matched by its lookup form, not by its spelling.** The tables
//!   are lexicons, and a lexicon key is text: `cover-up`, `bad luck`, `Abfall`.
//!   A token stream contains none of those. Both sides are reduced to word
//!   pieces and lowercased before they meet — see [`Vocabulary`] for the exact
//!   definition and for the last-wins rule when two keys reduce to one form.
//! * **The longest span wins, and it counts once.** `son-of-a-bitch` is four
//!   tokens and one lexical unit worth -5. Scoring it as four units would
//!   divide its own polarity by four and report -1.25, which is not a number
//!   any lexicon published. The denominator counts units, so a phrase
//!   contributes exactly one — and therefore
//!   `get_sentiment(tokens("son-of-a-bitch"))` is the key's own polarity.
//! * **Negation is sticky, not windowed.** `negator` is set to -1 by the first
//!   negation word and is never restored — not after one token, not after
//!   punctuation, not at a sentence boundary. `["not","happy","happy"]` scores
//!   -2, not -1 and not 0. The obvious readings ("negate the next word",
//!   "reset on each hit") both disagree on the second `happy`.
//! * **The negation test runs before the single-token lookup**, so a word in
//!   both lists contributes nothing at all. `no` is an English AFINN entry
//!   worth -1 and an English negation word; the negation branch wins and it
//!   scores zero. Same for `no`/`nunca` in Spanish and `não`/`nunca` in
//!   Portuguese. It runs *after* the span scan, though: a phrase is a lexical
//!   unit and the negation list is a list of words, so a key that happens to
//!   begin with `no` is matched as itself rather than read as a negation.
//! * **The stemmer is consulted second and only on a miss**, at every span
//!   length as well as on the single token, and it stems the *already
//!   lowercased* text. The stem it produces is the probe, verbatim: the table
//!   was built by lowercasing and stemming the pieces of every key with the
//!   same two functions and joining them with the same third, so nothing here
//!   reshapes what the stemmer returned. [`Vocabulary`] states why that is not
//!   an implementation detail.
//!
//! # The division, and why it is an `Option`
//!
//! `sum / units` — and an empty input has no units, so there is nothing to
//! divide by. The mean polarity of no text is not zero: returning zero would
//! make "nothing to say" indistinguishable from "perfectly neutral". Nor is it
//! `NaN`, which is what `0.0 / 0.0` produces and what this crate used to
//! return: a `NaN` compares false against everything including itself, so it
//! sorts unpredictably, poisons any average taken over a batch of scores, and
//! is discovered — if at all — a long way from the empty document that caused
//! it. Absence is [`None`], as it is everywhere else in Verbora.
//!
//! Callers who want a denominator of their own — a document-level count while
//! scoring one sentence, say — use [`SentimentAnalyzer::get_sentiment_over`],
//! which answers `None` for a denominator of zero for the same reason.
//!
//! # Accumulation order
//!
//! `sum` is an `f64` accumulated strictly left to right and divided exactly
//! once at the end. Reordering the sum, dividing per term, or summing in `f32`
//! all change the last bits, and the published lexicon values are dense enough
//! that this is observable. [`Contributions`] therefore yields one addend per
//! unit, in order, including a `0.0` for units that score nothing, and
//! [`SentimentAnalyzer::score`] is its plain left fold. Adding `0.0` cannot
//! perturb a sum whose running value started at `+0.0`, so the extra addends
//! are exact.

use std::borrow::Cow;

use crate::stemmer::{NoStemmer, Stemmer};
use crate::vocabulary::{self, Vocabulary};
use crate::{Language, VocabularyKind};

/// The one way an analyzer can fail to be built: no vocabulary ships for the
/// requested `(kind, language)` pair.
///
/// It is a struct rather than a single-variant enum because there is exactly
/// one thing that can be wrong and the caller wants to know which pair it was,
/// not which of several shapes the failure took. Both arguments are enums, so
/// a misspelling cannot reach this — only a pair the table genuinely lacks,
/// such as AFINN Dutch. [`supported_pairs`](crate::supported_pairs) lists the
/// fourteen that exist.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnsupportedPair {
    /// The vocabulary family that was asked for.
    pub kind: VocabularyKind,
    /// The language that was asked for.
    pub language: Language,
}

impl std::fmt::Display for UnsupportedPair {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "no {} vocabulary ships for {:?} ({}); {} does ship for {:?}",
            self.kind,
            self.language,
            self.language,
            self.kind,
            self.kind.languages(),
        )
    }
}

impl std::error::Error for UnsupportedPair {}

/// The table a shipped vocabulary lives in, borrowed when nothing rewrote it.
#[derive(Debug, Clone)]
enum Table {
    /// The process-wide decode of one blob, shared by every analyzer that
    /// wants it — so two analyzers over the same pair cost one decode.
    Shared(&'static Vocabulary),
    /// A stemmed rebuild, private to the analyzer that asked for it.
    Owned(Vocabulary),
}

impl Table {
    fn get(&self) -> &Vocabulary {
        match self {
            Self::Shared(v) => v,
            Self::Owned(v) => v,
        }
    }
}

/// A lexicon-based sentiment analyzer over one `(language, vocabulary kind)`
/// pair.
///
/// Scoring walks a token stream left to right and consumes it one unit at a
/// time — a matched multi-token span, or a single token — accumulating
/// `negator × polarity` and dividing by the unit count at the end. The module
/// documentation gives the loop in full; the five details worth knowing before
/// reading a score are there too.
///
/// ```
/// use verbora_sentiment::{Language, SentimentAnalyzer, VocabularyKind};
///
/// let a = SentimentAnalyzer::without_stemmer(Language::English, VocabularyKind::Afinn).unwrap();
/// assert_eq!(a.get_sentiment(["good"]), Some(3.0));
/// // Sticky negation: the second `happy` is still negated.
/// assert_eq!(a.get_sentiment(["not", "happy"]), Some(-1.5));
/// assert_eq!(a.get_sentiment(["not", "happy", "happy"]), Some(-2.0));
/// // …but a phrase the lexicon publishes is matched as one unit first.
/// assert_eq!(a.get_sentiment(["not", "good"]), Some(-2.0));
/// // Nothing was scored, so there is no mean.
/// assert_eq!(a.get_sentiment(Vec::<&str>::new()), None);
/// ```
#[derive(Debug, Clone)]
pub struct SentimentAnalyzer<S = NoStemmer> {
    language: Language,
    kind: VocabularyKind,
    vocabulary: Table,
    negations: &'static [&'static str],
    stemmer: Option<S>,
}

impl SentimentAnalyzer<NoStemmer> {
    /// Builds an analyzer with no stemmer.
    ///
    /// # Choosing the right constructor
    ///
    /// | | [`without_stemmer`](Self::without_stemmer) | [`with_stemmer`](SentimentAnalyzer::with_stemmer) |
    /// |---|---|---|
    /// | Table | the process-wide decode, shared | a private rebuild, one `stem` call per piece of every key |
    /// | Construction cost | a pointer copy after that table's first use | 33,874 stem calls for English senticon |
    /// | What a token can reach | its own lookup form | its lookup form first, then its stem |
    /// | What it costs | nothing | keys whose stems collide share one entry — 121 of English AFINN's 3,382 lose their own polarity |
    ///
    /// Prefer this one. A stemmer widens recall (`running` finds `run`) but
    /// collapses the table under it, and the crosstalk that introduces is
    /// described on [`Vocabulary::stemmed`]. It is a deliberate trade, not a
    /// free improvement.
    ///
    /// # Errors
    ///
    /// [`UnsupportedPair`] when no vocabulary ships for that pair.
    pub fn without_stemmer(
        language: Language,
        kind: VocabularyKind,
    ) -> Result<Self, UnsupportedPair> {
        Self::build(language, kind, None)
    }
}

impl<S: Stemmer> SentimentAnalyzer<S> {
    /// Builds an analyzer whose vocabulary is rebuilt through `stemmer`.
    ///
    /// See [`without_stemmer`](SentimentAnalyzer::without_stemmer) for the
    /// table that compares the two, and [`Vocabulary::stemmed`] for exactly
    /// what the rebuild does.
    ///
    /// A stemmer is **not** how capitalised keys are reached. `pattern`/German
    /// ships 1,234 capitalised entries, and no stemmer bridged by this crate
    /// folds case, so stemming `Abfall` yields a capitalised stem that no
    /// lowercased token can match. They are reachable because keys are indexed
    /// by their lookup form, which is lowercased, with or without a stemmer.
    ///
    /// # Errors
    ///
    /// [`UnsupportedPair`] when no vocabulary ships for that pair.
    pub fn with_stemmer(
        language: Language,
        kind: VocabularyKind,
        stemmer: S,
    ) -> Result<Self, UnsupportedPair> {
        Self::build(language, kind, Some(stemmer))
    }

    /// The shared body of the two constructors.
    fn build(
        language: Language,
        kind: VocabularyKind,
        stemmer: Option<S>,
    ) -> Result<Self, UnsupportedPair> {
        let index =
            vocabulary::source_index(kind, language).ok_or(UnsupportedPair { kind, language })?;
        let source = crate::data::SOURCES
            .get(index)
            .expect("source_index returns an index into SOURCES");
        let base = vocabulary::load(index);
        let table = match stemmer.as_ref() {
            Some(stemmer) => Table::Owned(base.stemmed(stemmer)),
            None => Table::Shared(base),
        };
        Ok(Self {
            language,
            kind,
            vocabulary: table,
            negations: source.negations,
            stemmer,
        })
    }

    /// The language this analyzer scores.
    #[must_use]
    pub fn language(&self) -> Language {
        self.language
    }

    /// The lexicon family this analyzer scores with.
    #[must_use]
    pub fn kind(&self) -> VocabularyKind {
        self.kind
    }

    /// The word → polarity table this analyzer scores with.
    #[must_use]
    pub fn vocabulary(&self) -> &Vocabulary {
        self.vocabulary.get()
    }

    /// The negation words for this language, in file order.
    ///
    /// Empty for the five languages whose upstream projects published no
    /// negation list — Galician, Catalan, Basque, Italian and French — so the
    /// sticky-negation rule never fires for them. The slice is `'static` and
    /// immutable, so one analyzer cannot alter another's.
    #[must_use]
    pub fn negations(&self) -> &'static [&'static str] {
        self.negations
    }

    /// The stemmer, if one was supplied.
    #[must_use]
    pub fn stemmer(&self) -> Option<&S> {
        self.stemmer.as_ref()
    }

    /// The per-unit addends, lazily, in order.
    ///
    /// This is the primitive: it borrows the analyzer, consumes any iterator of
    /// string-like tokens, materialises nothing beyond a lookahead of at most
    /// the longest phrase key, and carries the sticky negation state across the
    /// whole run. Everything else here is built on it, so a tokenizer can be
    /// piped straight in:
    ///
    /// ```
    /// use verbora_sentiment::{Language, SentimentAnalyzer, VocabularyKind};
    ///
    /// let a = SentimentAnalyzer::without_stemmer(Language::English, VocabularyKind::Afinn).unwrap();
    /// let deltas: Vec<f64> = a.contributions(["it", "is", "not", "happy"]).collect();
    /// assert_eq!(deltas, [0.0, 0.0, 0.0, -3.0]);
    /// ```
    ///
    /// One addend per **unit**, not per token: a span that matched a phrase key
    /// yields a single addend and swallows its tokens, which is what makes the
    /// denominator count lexical units.
    ///
    /// ```
    /// use verbora_sentiment::{Language, SentimentAnalyzer, VocabularyKind};
    /// use verbora_tokenizers::{BorrowingTokenizer, WordTokenizer};
    ///
    /// let a = SentimentAnalyzer::without_stemmer(Language::English, VocabularyKind::Afinn).unwrap();
    /// // Four tokens, one unit: `son-of-a-bitch` is a shipped key worth -5.
    /// let deltas: Vec<f64> = a.contributions(WordTokenizer.tokens("son-of-a-bitch")).collect();
    /// assert_eq!(deltas, [-5.0]);
    /// ```
    pub fn contributions<I>(&self, words: I) -> Contributions<'_, S, I::IntoIter>
    where
        I: IntoIterator,
        I::Item: AsRef<str>,
    {
        Contributions {
            analyzer: self,
            words: words.into_iter(),
            negator: 1.0,
            buffer: Vec::new(),
            live: 0,
            stems: Vec::new(),
            stemmed: 0,
            scratch: String::new(),
        }
    }

    /// The running total and the unit count, without dividing.
    ///
    /// Useful when the denominator is not the unit count — see
    /// [`Self::get_sentiment_over`] — or when several segments are scored and
    /// combined.
    pub fn score<I>(&self, words: I) -> Score
    where
        I: IntoIterator,
        I::Item: AsRef<str>,
    {
        let mut sum = 0.0;
        let mut count = 0usize;
        for delta in self.contributions(words) {
            sum += delta;
            count += 1;
        }
        Score { sum, count }
    }

    /// The summed polarity divided by the number of units scored, or `None`
    /// when nothing was scored.
    ///
    /// `None` is the mean of no text. It is not `0.0` — that is a real score,
    /// and a neutral document must stay distinguishable from an absent one —
    /// and it is not `NaN`, which would compare false against itself and
    /// silently poison any aggregate taken over a batch.
    ///
    /// # Choosing the right entry point
    ///
    /// | | Yields | Allocates | Use when |
    /// |---|---|---|---|
    /// | [`contributions`](Self::contributions) | one `f64` per unit, lazily | a bounded lookahead | you want the per-unit addends, or to stop early |
    /// | [`score`](Self::score) | [`Score`] — sum and unit count | the same | you need the two halves separately, or will combine several segments |
    /// | `get_sentiment` | `Option<f64>` | the same | you want one number for one self-contained text |
    /// | [`get_sentiment_over`](Self::get_sentiment_over) | `Option<f64>` | the same | the denominator is not this text's own unit count |
    ///
    /// All four run the identical scoring loop — the latter three are folds
    /// over the first — so they never disagree, and none is faster than
    /// another for the same question.
    pub fn get_sentiment<I>(&self, words: I) -> Option<f64>
    where
        I: IntoIterator,
        I::Item: AsRef<str>,
    {
        self.score(words).mean()
    }

    /// The summed polarity divided by a denominator you supply.
    ///
    /// The unit count is the right denominator for scoring one self-contained
    /// piece of text, but not for scoring a *part* of one: a sentence's share
    /// of its document's sentiment is its own sum over the document's length,
    /// not over the sentence's. Passing the denominator explicitly says which
    /// of the two is meant instead of leaving it to be inferred.
    ///
    /// `None` for a denominator of zero, for the reason
    /// [`get_sentiment`](Self::get_sentiment) gives.
    ///
    /// ```
    /// use verbora_sentiment::{Language, SentimentAnalyzer, VocabularyKind};
    ///
    /// let a = SentimentAnalyzer::without_stemmer(Language::English, VocabularyKind::Afinn).unwrap();
    /// assert_eq!(a.get_sentiment_over(["good"], 2), Some(1.5));
    /// assert_eq!(a.get_sentiment_over(["good"], 0), None);
    /// ```
    pub fn get_sentiment_over<I>(&self, words: I, len: usize) -> Option<f64>
    where
        I: IntoIterator,
        I::Item: AsRef<str>,
    {
        self.score(words).over(len)
    }

    /// Whether `word` — already lowercased — is a negation word.
    fn is_negation(&self, word: &str) -> bool {
        self.negations.contains(&word)
    }
}

/// A summed score and the number of units it came from.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Score {
    /// The polarity total, accumulated left to right in `f64`.
    pub sum: f64,
    /// How many units the scan produced: one per token that was scored on its
    /// own, and one per multi-token span that matched a phrase key. This is
    /// **not** the token count — `["son","of","a","bitch"]` is one unit —
    /// which is exactly what stops a phrase from dividing its own polarity by
    /// its own length.
    pub count: usize,
}

impl Score {
    /// `sum / count`, or `None` when nothing was scored.
    ///
    /// The empty case is the only one that is absent: `sum` is a finite sum of
    /// finite shipped polarities, so a non-zero `count` always divides to a
    /// finite mean.
    #[must_use]
    pub fn mean(self) -> Option<f64> {
        self.over(self.count)
    }

    /// `sum / len`, for a denominator that is not the unit count, or `None`
    /// when `len` is zero.
    #[must_use]
    pub fn over(self, len: usize) -> Option<f64> {
        if len == 0 {
            return None;
        }
        #[expect(
            clippy::cast_precision_loss,
            reason = "a denominator beyond 2^53 cannot arise from text that fits in memory"
        )]
        {
            Some(self.sum / len as f64)
        }
    }
}

/// The lazy per-unit addends of [`SentimentAnalyzer::contributions`].
///
/// Yields one `f64` per scored unit — `0.0` for a unit that scored nothing,
/// including the negation words themselves — so summing it left to right
/// reproduces the scoring loop's `sum += …` sequence addend for addend, and
/// counting it gives the denominator.
///
/// # Lookahead, and what it costs
///
/// Matching a phrase key needs to see the tokens after the current one, so the
/// iterator buffers up to the length of the longest key that *begins with the
/// current token* — a number the vocabulary stores in the same hash slot as
/// the token's own polarity, so a token that begins no phrase reads it for
/// free and never fills the buffer past one. The buffered `String`s are
/// cleared and reused rather than reallocated, so a long document allocates a
/// bounded, tiny amount however many tokens it holds.
///
/// The lookahead is pulled from the source iterator, so a `Contributions` that
/// is dropped mid-run may have consumed a few tokens more than it reported.
#[derive(Debug)]
pub struct Contributions<'a, S, I> {
    analyzer: &'a SentimentAnalyzer<S>,
    words: I,
    negator: f64,
    /// Lowercased lookahead. The first `live` are pending tokens; the rest are
    /// spent buffers, kept for their capacity.
    buffer: Vec<String>,
    live: usize,
    /// Stems of `buffer[..stemmed]`, filled only when a stemmer is installed
    /// and only as far as a lookup actually needs.
    stems: Vec<String>,
    stemmed: usize,
    /// The joined form of the span currently being probed.
    scratch: String,
}

impl<S: Stemmer, I> Contributions<'_, S, I>
where
    I: Iterator,
    I::Item: AsRef<str>,
{
    /// Buffers lowercased tokens until `want` are pending or the input ends.
    fn fill(&mut self, want: usize) {
        while self.live < want {
            let Some(token) = self.words.next() else {
                return;
            };
            if self.live == self.buffer.len() {
                self.buffer.push(String::new());
            }
            let slot = &mut self.buffer[self.live];
            slot.clear();
            slot.push_str(lowercase(token.as_ref()).as_ref());
            self.live += 1;
        }
    }

    /// Stems the first `want` pending tokens, if a stemmer is installed.
    ///
    /// The stem of a piece is [`vocabulary::stem_piece`] and nothing else,
    /// which is the same function the table's own keys were built from.
    fn fill_stems(&mut self, want: usize) {
        let Some(stemmer) = self.analyzer.stemmer.as_ref() else {
            return;
        };
        let want = want.min(self.live);
        let Self {
            buffer,
            stems,
            stemmed,
            ..
        } = self;
        while *stemmed < want {
            let stem = vocabulary::stem_piece(&buffer[*stemmed], stemmer);
            if stems.len() == *stemmed {
                stems.push(String::new());
            }
            let slot = &mut stems[*stemmed];
            slot.clear();
            slot.push_str(&stem);
            *stemmed += 1;
        }
    }

    /// The polarity of the lookup form of the first `k` pending pieces, taking
    /// them either as written or as stems.
    ///
    /// The form is spelled by [`vocabulary::write_form`], which is also what
    /// spelled the table's keys, over pieces derived by the same
    /// [`lowercase`] and [`vocabulary::stem_piece`] the table used — so this
    /// probe cannot ask the index for a spelling the index was not built with.
    /// Every lookup in the scoring loop goes through here, single tokens
    /// included; a one-piece form is that piece verbatim, so it is borrowed
    /// rather than copied into `scratch`.
    fn probe(&mut self, k: usize, stemmed: bool) -> Option<f64> {
        let Self {
            analyzer,
            buffer,
            stems,
            scratch,
            ..
        } = self;
        let pieces = (if stemmed { &*stems } else { &*buffer }).get(..k)?;
        let form = if let [only] = pieces {
            only.as_str()
        } else {
            vocabulary::write_form(scratch, pieces);
            scratch.as_str()
        };
        analyzer.vocabulary.get().form_polarity(form)
    }

    /// Consumes `k` pending tokens and reports `value` as this unit's addend.
    fn take(&mut self, k: usize, value: f64) -> f64 {
        self.buffer.rotate_left(k);
        self.live -= k;
        if k >= self.stemmed {
            self.stemmed = 0;
        } else {
            self.stems.rotate_left(k);
            self.stemmed -= k;
        }
        value
    }
}

impl<S: Stemmer, I> Iterator for Contributions<'_, S, I>
where
    I: Iterator,
    I::Item: AsRef<str>,
{
    type Item = f64;

    fn next(&mut self) -> Option<f64> {
        self.fill(1);
        if self.live == 0 {
            return None;
        }
        let analyzer = self.analyzer;
        let vocabulary = analyzer.vocabulary.get();
        let stemmed = analyzer.stemmer.is_some();

        // 1. The longest multi-token span, exact first and stemmed on a miss at
        //    each length, so a phrase always beats a prefix of itself.
        let mut span = vocabulary.span_from(&self.buffer[0]);
        if stemmed {
            self.fill_stems(1);
            span = span.max(vocabulary.span_from(&self.stems[0]));
        }
        if span >= 2 {
            self.fill(usize::from(span));
            for k in (2..=usize::from(span).min(self.live)).rev() {
                if let Some(value) = self.probe(k, false) {
                    let delta = self.negator * value;
                    return Some(self.take(k, delta));
                }
                if stemmed {
                    self.fill_stems(k);
                    if let Some(value) = self.probe(k, true) {
                        let delta = self.negator * value;
                        return Some(self.take(k, delta));
                    }
                }
            }
        }

        // 2. Negation, which runs before the single-token lookup so a word in
        //    both lists contributes nothing at all.
        if analyzer.is_negation(&self.buffer[0]) {
            self.negator = -1.0;
            return Some(self.take(1, 0.0));
        }

        // 3. The token itself, then 4. its stem — through the same `probe`,
        //    and so through the same key derivation, as every span above.
        if let Some(value) = self.probe(1, false) {
            let delta = self.negator * value;
            return Some(self.take(1, delta));
        }
        if stemmed {
            self.fill_stems(1);
            if let Some(value) = self.probe(1, true) {
                let delta = self.negator * value;
                return Some(self.take(1, delta));
            }
        }
        Some(self.take(1, 0.0))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let (lower, upper) = self.words.size_hint();
        // A span can swallow several tokens, so the token count is only an
        // upper bound on the unit count; one pending token guarantees one unit.
        let at_least = usize::from(self.live > 0 || lower > 0);
        (at_least, upper.and_then(|u| u.checked_add(self.live)))
    }
}

/// The Unicode full lowercase mapping of `s`, without allocating for text that
/// is already lowercase.
///
/// The rewriting path must be `str::to_lowercase` and nothing else: it is the
/// only one of Rust's four lowercasing routines that implements the full
/// string-level algorithm of The Unicode Standard §3.13. `to_ascii_lowercase`
/// leaves `İ` and `Σ` alone; a `char`-by-`char` loop expands `İ` correctly but
/// has no context for Final_Sigma, so `ΑΣ` comes out as `ασ` where the
/// standard gives `ας`.
///
/// Two fast paths avoid it without weakening it. All-ASCII input can be tested
/// with one byte scan, because neither special rule can apply. Non-ASCII input
/// is tested by [`needs_lowering`], which matters far more here than it looks:
/// nine of the fourteen vocabularies are non-English, so most of this crate's
/// tokens are accented Latin, Greek or Cyrillic that is *already* lowercase and
/// whose `to_lowercase` is the identity — and paying an allocation and a copy
/// to discover that is most of the cost of scoring non-English text.
pub(crate) fn lowercase(s: &str) -> Cow<'_, str> {
    if s.is_ascii() {
        if s.bytes().any(|b| b.is_ascii_uppercase()) {
            Cow::Owned(s.to_ascii_lowercase())
        } else {
            Cow::Borrowed(s)
        }
    } else if needs_lowering(s) {
        Cow::Owned(s.to_lowercase())
    } else {
        Cow::Borrowed(s)
    }
}

/// Whether `str::to_lowercase` would change `s`.
///
/// `str::to_lowercase` is the per-character full lowercase mapping plus exactly
/// one context-sensitive rule, Final_Sigma, which fires only on `Σ`. If no
/// character maps to anything but itself then none of them is `Σ` — `Σ` already
/// maps to `σ` — so the whole-string result is the input, and the answer is
/// exact rather than conservative. `lowercase_agrees_with_the_standard_library`
/// checks it over every code point below U+3000 plus the astral cased blocks.
fn needs_lowering(s: &str) -> bool {
    s.chars().any(|c| {
        let mut lower = c.to_lowercase();
        lower.next() != Some(c) || lower.next().is_some()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Language, Polarity, VocabularyKind};

    fn afinn_en() -> SentimentAnalyzer<NoStemmer> {
        SentimentAnalyzer::without_stemmer(Language::English, VocabularyKind::Afinn)
            .expect("English AFINN exists")
    }

    #[test]
    fn negation_is_sticky_and_unscored() {
        // `happy` is worth +3 and no shipped key begins `not happy`, so these
        // exercise the negation heuristic and nothing else. (`not good` *is* a
        // shipped key — see `a_phrase_key_is_matched_before_the_negation_list`.)
        let a = afinn_en();
        assert_eq!(a.vocabulary().get("happy").map(Polarity::value), Some(3.0));
        assert_eq!(a.vocabulary().get("not happy").map(Polarity::value), None);
        assert_eq!(a.get_sentiment(["happy"]), Some(3.0));
        assert_eq!(a.get_sentiment(["not", "happy"]), Some(-1.5));
        assert_eq!(a.get_sentiment(["not", "happy", "happy"]), Some(-2.0));
        assert_eq!(a.get_sentiment(["happy", "not", "happy"]), Some(0.0));
        assert_eq!(a.get_sentiment(["not", "not", "happy"]), Some(-1.0));
        assert_eq!(a.get_sentiment(["NOT", "HAPPY"]), Some(-1.5));
    }

    /// The span scan runs before the negation list, and that is the point.
    ///
    /// The negation heuristic exists because a bag of words cannot see
    /// `not good`. When the lexicon *does* publish the phrase — AFINN-165
    /// scores `not good` at -2 and `no fun` at -3 — its curated value is the
    /// better answer, and the `not` inside it is part of a lexical unit rather
    /// than a negation of everything that follows.
    #[test]
    fn a_phrase_key_is_matched_before_the_negation_list() {
        let a = afinn_en();
        assert_eq!(
            a.vocabulary().get("not good").map(Polarity::value),
            Some(-2.0)
        );
        assert_eq!(a.get_sentiment(["not", "good"]), Some(-2.0));
        assert_eq!(a.score(["not", "good"]).count, 1);

        // The negator is *not* set, so what follows is not flipped: -2 then +3.
        assert_eq!(a.get_sentiment(["not", "good", "good"]), Some(0.5));

        // `no fun` likewise, and it disagrees with the heuristic: `fun` is +4,
        // so negating it would give -4 where the lexicon says -3.
        assert_eq!(a.vocabulary().get("fun").map(Polarity::value), Some(4.0));
        assert_eq!(a.get_sentiment(["no", "fun"]), Some(-3.0));

        // A phrase the lexicon does not have still falls through to negation.
        assert_eq!(a.vocabulary().get("no idea").map(Polarity::value), None);
        assert_eq!(a.get_sentiment(["no", "idea"]), Some(0.0));
    }

    /// A sticky negator already in force applies to a matched span too.
    #[test]
    fn a_span_is_negated_by_a_negator_already_in_force() {
        let a = afinn_en();
        // `never` negates; `cover-up` is -3, so the span contributes +3.
        assert_eq!(
            a.vocabulary().get("cover-up").map(Polarity::value),
            Some(-3.0)
        );
        let scored = a.score(["never", "cover", "up"]);
        assert_eq!(scored.count, 2);
        assert_eq!(scored.sum, 3.0);
    }

    #[test]
    fn a_word_in_both_lists_scores_nothing() {
        // `no` is worth -1 in English AFINN and is also a negation word.
        let a = afinn_en();
        assert_eq!(a.vocabulary().get("no").map(Polarity::value), Some(-1.0));
        assert_eq!(a.get_sentiment(["no"]), Some(0.0));
        assert_eq!(a.get_sentiment(["no", "good"]), Some(-1.5));
    }

    /// The denominator is the unit count, so the tokenizer is part of every
    /// score this crate produces — and a boundary change moves all of them.
    ///
    /// The crate's only previous coverage of the tokenizer-to-analyzer path was
    /// one lowercase-ASCII doctest with no punctuation, which is invariant
    /// under every boundary rule anyone would propose. These cases are not:
    /// each expected value is `sum / count` with both halves shown, and each
    /// count comes from a named UAX #29 rule.
    #[test]
    fn the_tokenizer_boundary_is_part_of_the_score() {
        use verbora_tokenizers::{BorrowingTokenizer, WordTokenizer};

        let a = afinn_en();
        let score = |text: &str| a.score(WordTokenizer.tokens(text));

        // WB6/WB7 keep "don't" whole, so this is 3 tokens (don't, like, this).
        // A maximal-`[a-z]`-run split would make it 4 (don, t, like, this) and
        // divide the same sum by a larger denominator. Only "like" scores, at
        // +2; "don't" is not in the English AFINN negation list (which is why
        // the sum is positive).
        let s = score("don't like this");
        assert_eq!(s.count, 3);
        assert_eq!(s.mean(), Some(2.0 / 3.0));

        // U+002D is Word_Break=Other, so "cover-up" is 2 tokens, not 1 — and
        // `cover-up` is a shipped key worth -3. The span scan rejoins them into
        // one unit; without it the two halves score nothing at all and the
        // denominator gains a token, so this whole line reads `3.0 / 3.0`
        // instead. (`well-known` was the case tested here before, and its two
        // halves both score zero, so it passed either way.)
        let s = score("cover-up good");
        assert_eq!(s.count, 2);
        assert_eq!(s.mean(), Some(0.0 / 2.0));

        // The same key spelled with a space is the same lookup form.
        assert_eq!(score("cover up good"), s);

        // WB11/WB12 keep a decimal number whole: 2 tokens, not 3.
        let s = score("3.14 good");
        assert_eq!(s.count, 2);
        assert_eq!(s.mean(), Some(3.0 / 2.0));

        // Punctuation runs are not tokens at all, so they do not inflate the
        // denominator: still 2 tokens.
        let s = score("good, good!");
        assert_eq!(s.count, 2);
        assert_eq!(s.mean(), Some(6.0 / 2.0));

        // Non-ASCII words stay whole rather than being cut at each accent, so
        // an accented unknown word counts once.
        let s = score("café good");
        assert_eq!(s.count, 2);
        assert_eq!(s.mean(), Some(3.0 / 2.0));
    }

    #[test]
    fn nothing_scored_has_no_mean() {
        assert_eq!(afinn_en().get_sentiment(Vec::<&str>::new()), None);
        assert_eq!(afinn_en().score(Vec::<&str>::new()).mean(), None);
    }

    #[test]
    fn holes_are_counted_by_the_denominator() {
        let a = afinn_en();
        assert_eq!(a.get_sentiment_over(["good", "bad"], 4), Some(0.0));
        assert_eq!(a.get_sentiment_over(["good"], 2), Some(1.5));
        assert_eq!(a.get_sentiment_over(Vec::<&str>::new(), 5), Some(0.0));
    }

    #[test]
    fn lowercase_matches_the_full_string_algorithm() {
        assert_eq!(lowercase("ABC"), "abc");
        assert!(matches!(lowercase("abc"), Cow::Borrowed("abc")));
        assert_eq!(lowercase("İ"), "i\u{307}");
        assert_eq!(lowercase("ΑΣ"), "ας");
        assert_eq!(lowercase("Σ"), "σ");
        assert_eq!(lowercase(""), "");

        // The point of the non-ASCII fast path: already-lowercase accented text
        // is borrowed, not copied.
        assert!(matches!(lowercase("película"), Cow::Borrowed(_)));
        assert!(matches!(lowercase("ελλάδα"), Cow::Borrowed(_)));
        assert!(matches!(lowercase("москва"), Cow::Borrowed(_)));
        assert!(matches!(lowercase("Película"), Cow::Owned(_)));
    }

    #[test]
    fn lowercase_agrees_with_the_standard_library() {
        // Every code point where the fast path could wrongly claim "no change",
        // plus the two-character contexts Final_Sigma needs.
        for cp in 0u32..0x3000 {
            if let Some(c) = char::from_u32(cp) {
                let s = c.to_string();
                assert_eq!(lowercase(&s), s.to_lowercase(), "U+{cp:04X}");
            }
        }
        // Deseret and Warang Citi: astral blocks that really do have case.
        for cp in (0x1_0400u32..0x1_0450).chain(0x1_18A0..0x1_18E0) {
            if let Some(c) = char::from_u32(cp) {
                let s = c.to_string();
                assert_eq!(lowercase(&s), s.to_lowercase(), "U+{cp:04X}");
            }
        }
        for a in ['Σ', 'σ', 'ς', 'Α', 'α', 'İ', 'ß', 'ẞ', 'ǅ', 'a'] {
            for b in ['Σ', 'α', ' ', '.', 'Α', '😀'] {
                let s = format!("{a}{b}");
                assert_eq!(lowercase(&s), s.to_lowercase(), "{s:?}");
                let s = format!("{a}{b}{a}");
                assert_eq!(lowercase(&s), s.to_lowercase(), "{s:?}");
            }
        }
    }

    #[test]
    fn contributions_are_lazy_and_ordered() {
        let a = afinn_en();
        let first: Vec<f64> = a.contributions(["good", "bad", "good"]).take(2).collect();
        assert_eq!(first, [3.0, -3.0]);
    }

    // ---- input classes ---------------------------------------------------
    //
    // Every one of these must produce a finite, defined answer rather than a
    // panic or a slice-boundary error: the analyzer's input is arbitrary text
    // and the lookup key is whatever the caller's tokenizer produced.

    #[test]
    fn empty_and_single_character_inputs() {
        let a = afinn_en();
        assert_eq!(a.get_sentiment(Vec::<&str>::new()), None);
        assert_eq!(a.get_sentiment([""]), Some(0.0));
        assert_eq!(a.get_sentiment(["", "", ""]), Some(0.0));
        assert_eq!(a.get_sentiment(["a"]), Some(0.0));
        assert_eq!(a.get_sentiment(["A"]), Some(0.0));
        assert_eq!(a.get_sentiment(["0"]), Some(0.0));
        assert_eq!(a.get_sentiment(["9"]), Some(0.0));
        assert_eq!(a.score([""]).count, 1);
    }

    #[test]
    fn case_folding_reaches_the_same_entry() {
        let a = afinn_en();
        for token in ["good", "Good", "GOOD", "gOOd", "GooD"] {
            assert_eq!(a.get_sentiment([token]), Some(3.0), "{token}");
        }
    }

    #[test]
    fn accented_latin_cyrillic_greek_and_cjk() {
        // Spanish AFINN has accented entries; the lookup must be on the
        // lowercased *string*, not on ASCII-folded bytes.
        let es =
            SentimentAnalyzer::without_stemmer(Language::Spanish, VocabularyKind::Afinn).unwrap();
        assert!(es.vocabulary().get("sueño").map(Polarity::value).is_some());
        assert_eq!(es.get_sentiment(["SUEÑO"]), es.get_sentiment(["sueño"]));
        assert!(es.vocabulary().get("éxito").map(Polarity::value).is_some());
        assert_eq!(es.get_sentiment(["ÉXITO"]), es.get_sentiment(["éxito"]));

        let a = afinn_en();
        // `naïve` really is an AFINN-165 key, accent and all — which is why the
        // lookup must not fold accents away.
        assert_eq!(a.vocabulary().get("naïve").map(Polarity::value), Some(-2.0));
        assert_eq!(a.get_sentiment(["Naïve"]), Some(-2.0));
        assert_eq!(a.get_sentiment(["café", "Ångström", "crème"]), Some(0.0));
        assert_eq!(a.get_sentiment(["Москва", "Ελλάδα", "ΑΣΤΕΙΟ"]), Some(0.0));
        assert_eq!(a.get_sentiment(["日本語", "中文测试", "한국어"]), Some(0.0));
        // Greek Final_Sigma: the lowercase of "ΑΣ" is "ας", not "ασ".
        assert_eq!(lowercase("ΑΣΤΕΙΟ"), "αστειο");
        assert_eq!(lowercase("ΟΔΟΣ"), "οδος");
    }

    #[test]
    fn astral_characters_and_emoji() {
        let a = afinn_en();
        assert_eq!(a.get_sentiment(["😀", "a😀b", "𝕳𝖊𝖑𝖑𝖔"]), Some(0.0));
        // Spanish and Portuguese AFINN really do key on emoji.
        let es =
            SentimentAnalyzer::without_stemmer(Language::Spanish, VocabularyKind::Afinn).unwrap();
        assert_eq!(es.vocabulary().get("😂").map(Polarity::value), Some(1.0));
        assert_eq!(es.get_sentiment(["😂"]), Some(1.0));
        assert_eq!(es.get_sentiment(["no", "😂"]), Some(-0.5));
    }

    #[test]
    fn punctuation_and_digits_are_inert() {
        let a = afinn_en();
        assert_eq!(
            a.get_sentiment(["!", "...", "--", "'", "?!", "«»", "—"]),
            Some(0.0)
        );
        assert_eq!(a.get_sentiment(["123", "3.14", "1,000", "-0"]), Some(0.0));
        // Punctuation does not reset the negation, which is the whole point.
        assert_eq!(a.get_sentiment(["not", ".", "good"]), Some(-1.0));
    }

    #[test]
    fn very_long_inputs() {
        let a = afinn_en();
        let long_token = "a".repeat(100_000);
        assert_eq!(a.get_sentiment([long_token.as_str()]), Some(0.0));

        let many: Vec<&str> = std::iter::repeat_n("good", 10_000).collect();
        assert_eq!(a.get_sentiment(many.iter().copied()), Some(3.0));

        // One negation at the front flips all 10,000 that follow. `happy`
        // rather than `good`, because `not good` is itself a shipped key and
        // would be matched as one unit before the negation list is consulted.
        let negated: Vec<&str> = std::iter::once("not")
            .chain(std::iter::repeat_n("happy", 10_000))
            .collect();
        let expected = -3.0 * 10_000.0 / 10_001.0;
        assert_eq!(a.get_sentiment(negated.iter().copied()), Some(expected));
    }

    #[test]
    fn the_sum_is_left_to_right() {
        // Three values whose f64 sum differs by an ULP if reassociated.
        let a = SentimentAnalyzer::without_stemmer(Language::English, VocabularyKind::Senticon)
            .unwrap();
        let tokens = ["good", "bad", "excellent", "awful", "fine"];
        let by_fold: f64 = a.contributions(tokens).fold(0.0, |acc, d| acc + d);
        assert_eq!(a.score(tokens).sum.to_bits(), by_fold.to_bits());
    }

    #[test]
    fn iterators_of_owned_strings_work_too() {
        let a = afinn_en();
        let owned: Vec<String> = vec!["not".into(), "happy".into()];
        assert_eq!(a.get_sentiment(&owned), Some(-1.5));
        assert_eq!(
            a.get_sentiment(owned.iter().map(String::as_str)),
            Some(-1.5)
        );
        assert_eq!(a.get_sentiment(owned), Some(-1.5));
    }
}
