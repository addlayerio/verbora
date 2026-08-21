//! Turning text into terms: the one pipeline documents and queries share.
//!
//! A TF-IDF corpus is only as well defined as its notion of "term", so this
//! crate states that notion in one place and runs it on both sides of every
//! query. An [`Analyzer`] belongs to a [`TfIdf`](crate::TfIdf) instance — there
//! is no process-global tokenizer and no process-global stop-word list, so two
//! corpora in one program cannot silently change each other's answers.

use std::borrow::Cow;
use std::sync::Arc;

use verbora_core::StopWords;
use verbora_tokenizers::{BorrowingTokenizer, WordTokenizer};

/// A tokenizer an [`Analyzer`] can be given.
///
/// This is the object-safe shape a shared, runtime-selected tokenizer needs.
/// Anything implementing [`verbora_core::Tokenizer`] — every tokenizer in
/// `verbora-tokenizers` — gets it for free through the blanket implementation
/// below, so a custom tokenizer only has to implement `Tokenizer`.
///
/// # Examples
///
/// ```
/// use std::sync::Arc;
/// use verbora_tfidf::{Analyzer, TfIdf, Tokenize};
///
/// #[derive(Debug)]
/// struct SplitOnWhitespace;
///
/// impl Tokenize for SplitOnWhitespace {
///     fn tokenize_into(&self, text: &str, out: &mut Vec<String>) {
///         out.extend(text.split_whitespace().map(str::to_owned));
///     }
/// }
///
/// let analyzer = Analyzer::new().with_tokenizer(Arc::new(SplitOnWhitespace));
/// assert_eq!(analyzer.terms("Hello, world!"), ["hello,", "world!"]);
///
/// let corpus = TfIdf::with_analyzer(analyzer);
/// assert!(corpus.is_empty());
/// ```
pub trait Tokenize: std::fmt::Debug + Send + Sync {
    /// Tokenizes `text`, **appending** to `out`.
    ///
    /// `out` is not cleared, matching the buffer-reuse convention the rest of
    /// the workspace uses. Implementations must push tokens in the order they
    /// occur in `text`.
    fn tokenize_into(&self, text: &str, out: &mut Vec<String>);
}

impl<T> Tokenize for T
where
    T: verbora_core::Tokenizer + std::fmt::Debug + Send + Sync,
{
    fn tokenize_into(&self, text: &str, out: &mut Vec<String>) {
        verbora_core::Tokenizer::tokenize_into(self, text, out);
    }
}

/// Whether an [`Analyzer`] case-folds each token.
///
/// Folding is named here rather than applied silently: the crate never rewrites
/// text without the caller having chosen the rewrite. The default is
/// [`Self::Lowercase`], because a TF-IDF corpus that treats `"The"` and `"the"`
/// as two terms is measuring typography rather than content — but the choice is
/// visible at the construction site and recorded in every serialized artifact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum CaseFold {
    /// Fold each token with [`str::to_lowercase`] — the full Unicode
    /// `Lowercase_Mapping`, including the context-sensitive Greek final sigma
    /// rule and the mappings that change a token's length (`İ` becomes `i` plus
    /// `U+0307`).
    #[default]
    Lowercase,
    /// Use each token exactly as the tokenizer produced it.
    None,
}

impl CaseFold {
    /// The name this fold is recorded under in a serialized corpus.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Lowercase => "lowercase",
            Self::None => "none",
        }
    }

    /// The inverse of [`Self::as_str`].
    pub(crate) fn parse(name: &str) -> Option<Self> {
        match name {
            "lowercase" => Some(Self::Lowercase),
            "none" => Some(Self::None),
            _ => None,
        }
    }
}

/// Which tokenizer an [`Analyzer`] runs.
///
/// The default is kept as its own variant rather than as an
/// `Arc<dyn Tokenize>` holding a `WordTokenizer`, because the default path
/// yields tokens **borrowed** from the input while the `dyn` path must return
/// owned `String`s. Keeping them apart is what lets ingestion of a normal
/// document allocate nothing per token.
#[derive(Debug, Clone)]
enum Tokenization {
    /// [`WordTokenizer`].
    Words,
    /// A tokenizer installed through [`Analyzer::with_tokenizer`].
    Custom(Arc<dyn Tokenize>),
}

/// How a document's text — and a query's text — becomes terms.
///
/// # The pipeline, in order
///
/// 1. **Tokenize.** The text is cut into tokens by the analyzer's tokenizer.
///    The default is [`WordTokenizer`]: the [UAX #29] word segments that
///    contain at least one scalar with the `Alphabetic` property or with
///    `General_Category` in `{Nd, Nl, No}`. Tokens are contiguous substrings of
///    the input, in order, non-overlapping, never empty.
/// 2. **Fold.** Each token is case-folded according to [`CaseFold`].
/// 3. **Filter.** If the analyzer carries a stop-word list, a token whose
///    *folded* form is in it is dropped.
///
/// What survives step 3 is a **term**. Terms are what the corpus counts, what
/// `document_frequency` counts documents of, and what a serialized corpus
/// stores.
///
/// # The text unit is the token, and it is not re-derived here
///
/// This crate does not implement word boundaries. It calls
/// `verbora-tokenizers`, which owns the [UAX #29] contract, so there is exactly
/// one boundary rule in the workspace and no second copy to drift from it. That
/// matters concretely: `"don't"`, `"3.14"`, `"1,000"` and `"a:b"` are each one
/// token under UAX #29, and any hand-rolled "letters and digits" scan splits
/// all four.
///
/// Segmentation runs on the text **as given**; folding runs afterwards, per
/// token. Folding therefore cannot move a boundary, and every term is the
/// folded image of a substring of the input.
///
/// # Documents and queries are analyzed identically
///
/// There is one pipeline and both sides use it. A query for `"The"` against a
/// lowercasing corpus finds `"the"`, and a query for a stop word against a
/// stop-word-filtered corpus finds nothing — because the query term was dropped
/// too, not because the document happened not to contain it.
///
/// # Examples
///
/// ```
/// use verbora_core::StopWords;
/// use verbora_tfidf::{Analyzer, CaseFold};
///
/// // The default: UAX #29 words, lowercased, nothing filtered.
/// let default = Analyzer::new();
/// assert_eq!(default.terms("The Quick, brown fox"), ["the", "quick", "brown", "fox"]);
///
/// // Case-sensitive.
/// let exact = Analyzer::new().with_case_fold(CaseFold::None);
/// assert_eq!(exact.terms("The Quick"), ["The", "Quick"]);
///
/// // With a stop-word list.
/// let filtered = Analyzer::new().with_stop_words(StopWords::from_iter_of(["the"]));
/// assert_eq!(filtered.terms("The quick"), ["quick"]);
/// ```
///
/// [UAX #29]: https://www.unicode.org/reports/tr29/
#[derive(Debug, Clone)]
pub struct Analyzer {
    tokenizer: Tokenization,
    case_fold: CaseFold,
    stop_words: Option<Arc<StopWords>>,
}

impl Default for Analyzer {
    fn default() -> Self {
        Self::new()
    }
}

impl Analyzer {
    /// The default analyzer: [`WordTokenizer`], [`CaseFold::Lowercase`], no
    /// stop-word list.
    ///
    /// Stop-word filtering is **off** by default because it deletes
    /// information: a corpus built with a filter cannot answer a question about
    /// a filtered term, and cannot be persuaded to later. Turning it on is one
    /// call and is recorded in the artifact; turning it off after the fact
    /// means re-ingesting every document.
    #[must_use]
    pub fn new() -> Self {
        Self {
            tokenizer: Tokenization::Words,
            case_fold: CaseFold::Lowercase,
            stop_words: None,
        }
    }

    /// Returns this analyzer with `fold` in place of its case-folding setting.
    #[must_use]
    pub fn with_case_fold(mut self, fold: CaseFold) -> Self {
        self.case_fold = fold;
        self
    }

    /// Returns this analyzer with `words` as its stop-word list.
    #[must_use]
    pub fn with_stop_words(mut self, words: StopWords) -> Self {
        self.stop_words = Some(Arc::new(words));
        self
    }

    /// Returns this analyzer with no stop-word list.
    #[must_use]
    pub fn without_stop_words(mut self) -> Self {
        self.stop_words = None;
        self
    }

    /// Returns this analyzer with `tokenizer` in place of [`WordTokenizer`].
    ///
    /// A corpus built with a custom tokenizer **cannot be serialized**: the
    /// artifact would carry terms whose derivation it could not describe, and
    /// [`TfIdf::to_json`](crate::TfIdf::to_json) refuses rather than writing a
    /// file whose compatibility stamp would be a lie. See
    /// [`crate::stamp`](crate::ArtifactStamp) for the reasoning.
    #[must_use]
    pub fn with_tokenizer(mut self, tokenizer: Arc<dyn Tokenize>) -> Self {
        self.tokenizer = Tokenization::Custom(tokenizer);
        self
    }

    /// This analyzer's case-folding setting.
    #[must_use]
    pub fn case_fold(&self) -> CaseFold {
        self.case_fold
    }

    /// This analyzer's stop-word list, or `None` if it filters nothing.
    #[must_use]
    pub fn stop_words(&self) -> Option<&StopWords> {
        self.stop_words.as_deref()
    }

    /// Whether this analyzer still uses the default [`WordTokenizer`].
    ///
    /// Serialization is only possible while this is `true`; see
    /// [`Self::with_tokenizer`].
    #[must_use]
    pub fn uses_default_tokenizer(&self) -> bool {
        matches!(self.tokenizer, Tokenization::Words)
    }

    /// The terms `text` yields, materialised in order.
    ///
    /// This is the pipeline itself, exposed so a caller can see exactly what a
    /// document will contribute before adding it. It allocates one `String` per
    /// term; the ingest path does not, which is why this is a convenience
    /// rather than the primitive.
    #[must_use]
    pub fn terms(&self, text: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut scratch = Vec::new();
        self.for_each_term(text, &mut scratch, |term| out.push(term.to_owned()));
        out
    }

    /// Case-folds one token, borrowing when the fold is provably the identity.
    ///
    /// `str::to_lowercase` allocates unconditionally, and corpus text is
    /// overwhelmingly already-lowercase ASCII. The guard is deliberately
    /// conservative: **any** non-ASCII byte hands the work to `to_lowercase`,
    /// so every Unicode special case — `İ` expanding to two scalars, Greek
    /// final sigma — is untouched. Only pure ASCII with no `A`–`Z` takes the
    /// borrowing path, and no ASCII scalar outside `A`–`Z` has a lowercase
    /// mapping other than itself.
    #[inline]
    fn fold<'t>(&self, token: &'t str) -> Cow<'t, str> {
        match self.case_fold {
            CaseFold::None => Cow::Borrowed(token),
            CaseFold::Lowercase => {
                if token.bytes().any(|b| b >= 0x80 || b.is_ascii_uppercase()) {
                    Cow::Owned(token.to_lowercase())
                } else {
                    Cow::Borrowed(token)
                }
            }
        }
    }

    /// Whether `term` — already folded — is filtered out.
    #[inline]
    fn is_stop_word(&self, term: &str) -> bool {
        self.stop_words.as_ref().is_some_and(|s| s.contains(term))
    }

    /// Applies `f` to each token of `text`, in order.
    ///
    /// `scratch` is used only by the custom-tokenizer path, which must
    /// materialise owned strings; the default path never touches it, so a
    /// caller passing a fresh empty `Vec` pays no allocation for it.
    fn for_each_token(&self, text: &str, scratch: &mut Vec<String>, f: impl FnMut(&str)) {
        match &self.tokenizer {
            Tokenization::Words => WordTokenizer.tokens(text).for_each(f),
            Tokenization::Custom(tokenizer) => {
                scratch.clear();
                tokenizer.tokenize_into(text, scratch);
                scratch.iter().map(String::as_str).for_each(f);
            }
        }
    }

    /// Applies `f` to each **term** of `text`, in order — the whole pipeline.
    ///
    /// This is the primitive every other entry point is written on top of, so
    /// there is one implementation of the pipeline and no second copy to drift.
    pub(crate) fn for_each_term(
        &self,
        text: &str,
        scratch: &mut Vec<String>,
        mut f: impl FnMut(&str),
    ) {
        self.for_each_token(text, scratch, |token| {
            let folded = self.fold(token);
            if !self.is_stop_word(&folded) {
                f(&folded);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every 1- and 2-character ASCII string, enumerated: the borrowing fast
    /// path in [`Analyzer::fold`] must agree with `str::to_lowercase` on all
    /// 16,512 of them.
    ///
    /// The claim under test is that "pure ASCII with no `A`–`Z`" implies
    /// "`to_lowercase` is the identity". Sampling would not find a
    /// counterexample if one existed, because it would be one specific
    /// character.
    #[test]
    fn the_ascii_borrowing_fold_agrees_with_to_lowercase_everywhere() {
        let analyzer = Analyzer::new();
        let mut buf = String::new();
        let mut checked = 0u32;
        for a in 0u8..=0x7F {
            buf.clear();
            buf.push(char::from(a));
            assert_eq!(
                analyzer.fold(&buf).as_ref(),
                buf.to_lowercase(),
                "fold({buf:?})"
            );
            checked += 1;
            for b in 0u8..=0x7F {
                buf.clear();
                buf.push(char::from(a));
                buf.push(char::from(b));
                assert_eq!(
                    analyzer.fold(&buf).as_ref(),
                    buf.to_lowercase(),
                    "fold({buf:?})"
                );
                checked += 1;
            }
        }
        assert_eq!(checked, 128 + 128 * 128);
    }

    /// The non-ASCII cases the fast path must decline, so `to_lowercase`'s own
    /// rules apply unchanged.
    #[test]
    fn non_ascii_folding_is_delegated_intact() {
        let analyzer = Analyzer::new();
        for token in [
            "İstanbul", // expands to two scalars
            "ΑΣ",       // word-final sigma
            "ΑΣΑ",      // medial sigma
            "ẞ",
            "Ǆ",
            "Café",
            "МОСКВА",
            "𝕳𝖊𝖑𝖑𝖔",
        ] {
            assert_eq!(
                analyzer.fold(token).as_ref(),
                token.to_lowercase(),
                "fold({token:?})"
            );
        }
        assert_eq!(analyzer.fold("İstanbul").as_ref(), "i\u{307}stanbul");
        assert_eq!(analyzer.fold("ΑΣ").as_ref(), "ας");
        assert_eq!(analyzer.fold("ΑΣΑ").as_ref(), "ασα");
    }

    #[test]
    fn case_fold_none_leaves_tokens_alone() {
        let analyzer = Analyzer::new().with_case_fold(CaseFold::None);
        assert_eq!(
            analyzer.terms("The QUICK İstanbul"),
            ["The", "QUICK", "İstanbul"]
        );
    }

    /// Segmentation is `verbora-tokenizers`', and these are the ASCII cases a
    /// hand-rolled `[a-z0-9_]` scan gets wrong. Each expected value comes from
    /// a named UAX #29 rule.
    #[test]
    fn segmentation_follows_uax29_on_ascii_punctuation() {
        let a = Analyzer::new();
        // WB6/WB7 over MidNumLetQ (the apostrophe).
        assert_eq!(a.terms("don't"), ["don't"]);
        // WB11/WB12: Numeric x MidNumLet x Numeric.
        assert_eq!(a.terms("3.14"), ["3.14"]);
        // WB11/WB12 with MidNum.
        assert_eq!(a.terms("1,000"), ["1,000"]);
        // WB6/WB7 with MidLetter.
        assert_eq!(a.terms("a:b"), ["a:b"]);
        // WB13a/WB13b: ExtendNumLet.
        assert_eq!(a.terms("node_js"), ["node_js"]);
        // ...and the cases that do split: Word_Break=Other on both sides.
        assert_eq!(a.terms("open-source"), ["open", "source"]);
        assert_eq!(a.terms("input/output"), ["input", "output"]);
        // A run of underscores is a segment but holds no letter or digit, so it
        // is not a token at all.
        assert!(a.terms("___").is_empty());
    }

    #[test]
    fn folding_happens_after_segmentation_and_cannot_move_a_boundary() {
        let a = Analyzer::new();
        // `İ` is ALetter; the combining dot its fold introduces is Extend, so
        // WB4 keeps it attached either way — one term, not two.
        assert_eq!(a.terms("İstanbul"), ["i\u{307}stanbul"]);
        // A word-final capital sigma folds to the final form.
        assert_eq!(a.terms("ΑΣ ΟΣ"), ["ας", "ος"]);
        assert_eq!(a.terms("Don'T NODE_JS"), ["don't", "node_js"]);
    }

    #[test]
    fn degenerate_inputs_yield_no_terms() {
        let a = Analyzer::new();
        for text in ["", " ", "  .,;  ", "\n\t", "--- ///", "😀", "\u{301}"] {
            assert!(a.terms(text).is_empty(), "{text:?}");
        }
    }

    #[test]
    fn astral_and_cjk_behave_as_the_tokenizer_says() {
        let a = Analyzer::new();
        // Emoji are Word_Break=Other: they separate and are not terms. No
        // astral scalar is ever replaced by U+FFFD.
        assert_eq!(a.terms("😀abc😀"), ["abc"]);
        assert_eq!(a.terms("𝕳𝖊𝖑𝖑𝖔"), ["𝕳𝖊𝖑𝖑𝖔".to_lowercase()]);
        // UAX #29 §4 does not segment scripts that do not use spaces.
        assert_eq!(a.terms("日本語 test"), ["日", "本", "語", "test"]);
    }

    #[test]
    fn stop_words_are_matched_against_the_folded_form() {
        let a = Analyzer::new().with_stop_words(StopWords::from_iter_of(["the", "a"]));
        assert_eq!(a.terms("The a quick THE fox"), ["quick", "fox"]);
        // Matching is exact on the folded form, so a list entry that is not
        // itself lowercase can never match under `CaseFold::Lowercase`.
        let b = Analyzer::new().with_stop_words(StopWords::from_iter_of(["The"]));
        assert_eq!(b.terms("The quick"), ["the", "quick"]);
        assert_eq!(a.stop_words().map(StopWords::len), Some(2));
        assert!(Analyzer::new().stop_words().is_none());
        assert!(a.without_stop_words().stop_words().is_none());
    }

    #[test]
    fn a_custom_tokenizer_replaces_only_step_one() {
        #[derive(Debug)]
        struct SplitOnApostrophe;
        impl Tokenize for SplitOnApostrophe {
            fn tokenize_into(&self, text: &str, out: &mut Vec<String>) {
                out.extend(
                    text.split(|c: char| c.is_whitespace() || c == '\'')
                        .filter(|piece| !piece.is_empty())
                        .map(str::to_owned),
                );
            }
        }

        let a = Analyzer::new().with_tokenizer(Arc::new(SplitOnApostrophe));
        assert!(!a.uses_default_tokenizer());
        assert!(Analyzer::new().uses_default_tokenizer());
        // The custom tokenizer splits where WordTokenizer would not...
        assert_eq!(a.terms("Isn'T node"), ["isn", "t", "node"]);
        // ...and folding and filtering still run after it.
        let b = a.with_stop_words(StopWords::from_iter_of(["t"]));
        assert_eq!(b.terms("Isn'T node"), ["isn", "node"]);
    }

    #[test]
    fn a_core_tokenizer_satisfies_the_trait_through_the_blanket_impl() {
        let a = Analyzer::new().with_tokenizer(Arc::new(verbora_tokenizers::SegmentTokenizer));
        // SegmentTokenizer keeps whitespace and punctuation runs, which
        // WordTokenizer drops.
        assert_eq!(a.terms("hi there"), ["hi", " ", "there"]);
    }

    #[test]
    fn case_fold_names_round_trip() {
        for fold in [CaseFold::Lowercase, CaseFold::None] {
            assert_eq!(CaseFold::parse(fold.as_str()), Some(fold));
        }
        assert_eq!(CaseFold::parse("Lowercase"), None);
        assert_eq!(CaseFold::parse(""), None);
        assert_eq!(CaseFold::default(), CaseFold::Lowercase);
    }
}
