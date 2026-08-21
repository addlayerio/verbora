//! The vocabulary the rest of Verbora is written against.
//!
//! Two things live here, and nothing else does:
//!
//! * the **traits** more than one crate needs to agree on — [`Tokenizer`],
//!   [`BorrowingTokenizer`], [`Stemmer`], [`Phonetic`] and
//!   [`DoubleKeyPhonetic`];
//! * the **stop-word lists**, which are shared data rather than shared
//!   behaviour: [`StopWordLanguage`], [`StopWords`] and the process-global
//!   English list.
//!
//! There are no dependencies on other `verbora-*` crates, which keeps the crate
//! graph acyclic and lets leaf crates (distance, phonetics, …) be used in
//! isolation without pulling in data assets they do not need.
//!
//! The crate root is the entire public surface: every module is private and
//! everything public is re-exported here, so there is exactly one path to each
//! item.
//!
//! # Two API levels
//!
//! Every processing trait here is offered at two levels:
//!
//! * a **high-level** API that returns owned data ([`Tokenizer::tokenize`],
//!   [`Stemmer::stem`]);
//! * a **low-level** API that writes into a caller-supplied buffer
//!   ([`Tokenizer::tokenize_into`], [`Stemmer::stem_into`]) so that hot loops
//!   can amortise allocation across many calls.
//!
//! Tokenizers whose output is always a *substring* of the input additionally
//! implement [`BorrowingTokenizer`], which yields `&str` slices that borrow the
//! input and allocate nothing at all. Each trait carries its own
//! `Choosing the Right API` table; start there rather than guessing from the
//! signatures.

#![cfg_attr(doctest, doc = include_str!("../README.md"))]

mod stopwords;

pub use stopwords::{
    STOPWORD_LANGUAGES, StopWordLanguage, StopWords, add_global_stopword, add_global_stopwords,
    global_stopwords, is_global_stopword, remove_global_stopword, remove_global_stopwords,
    reset_global_stopwords,
};

use std::borrow::Cow;

// ---------------------------------------------------------------------------
// Tokenization
// ---------------------------------------------------------------------------

/// Splits text into tokens.
///
/// This is the owned, ergonomic entry point every tokenizer provides. What
/// counts as a token is each implementation's own contract — this trait fixes
/// only the properties generic code can rely on across all of them:
///
/// * **Order.** Tokens are produced in the order they occur in the input.
/// * **No empty tokens.** An implementation never yields `""`. Callers may
///   therefore treat "no tokens" and "one empty token" as the same thing, which
///   is what makes `tokenize(text).is_empty()` a usable test for "no content".
/// * **Empty input yields no tokens.** `tokenize("")` is empty for every
///   implementation.
/// * **Determinism.** The same implementation, given the same input, produces
///   the same tokens; nothing here consults global state or a clock.
///
/// The trait deliberately says nothing about *where* boundaries fall, whether
/// tokens are substrings of the input, or how the input is normalised: a
/// sentence tokenizer and a word tokenizer are both honest implementations, and
/// the crate that ships one specifies it.
///
/// # Choosing the Right API
///
/// | Method | Use when | Allocations | Notes |
/// |---|---|---|---|
/// | [`tokenize`](Self::tokenize) | one document, simplest call | one `Vec`, one `String` per token | the right choice for the large majority of programs |
/// | [`tokenize_into`](Self::tokenize_into) | a corpus through one buffer | none once the buffer is warm, beyond each token's `String` | **appends** — forgetting `out.clear()` is a silent correctness bug, not an error |
/// | [`tokenize_batch`](Self::tokenize_batch) | generic code that needs "tokenize all of these" as one call | one `Vec<String>` per document | a plain sequential map; it allocates *more* than a `tokenize_into` loop, not less |
///
/// If the implementation also implements [`BorrowingTokenizer`], prefer that
/// trait's methods: they hand back slices of the input and allocate no `String`
/// at all. Reach for the owned methods here when you need `String`s anyway, or
/// when you are generic over tokenizers that may rewrite their input.
///
/// `tokenize` is not the slow API to be avoided — it is the correct one unless
/// you have measured a reason to manage a buffer yourself.
pub trait Tokenizer {
    /// Tokenizes `text`, returning owned tokens.
    ///
    /// One `String` per token, in order. Returns an empty `Vec` when `text`
    /// holds nothing the implementation recognises as a token, including when
    /// `text` is empty.
    fn tokenize(&self, text: &str) -> Vec<String> {
        let mut out = Vec::new();
        self.tokenize_into(text, &mut out);
        out
    }

    /// Tokenizes `text`, **appending** to `out`.
    ///
    /// # The problem this solves
    ///
    /// Tokenizing a corpus with [`Self::tokenize`] allocates and frees one
    /// `Vec` per document. This method writes into a buffer the caller owns, so
    /// the `Vec`'s capacity — and, for tokenizers that reuse them, the token
    /// `String`s' capacity — survives from one document to the next.
    ///
    /// The cost is that the caller manages the buffer. `out` is **not** cleared,
    /// so a forgotten `clear()` accumulates every document into one vector
    /// instead of failing:
    ///
    /// ```ignore
    /// let mut buf = Vec::new();
    /// for line in corpus {
    ///     buf.clear();
    ///     tokenizer.tokenize_into(line, &mut buf);
    ///     consume(&buf);
    /// }
    /// ```
    ///
    /// Appending is the deliberate choice rather than an oversight: a caller
    /// who *wants* to accumulate — collecting a whole corpus's tokens into one
    /// vector — cannot recover that behaviour from a method that clears, while
    /// a caller who wants a fresh buffer needs one line.
    ///
    /// No measurement of the saving is published here; what is guaranteed is
    /// the allocation behaviour above.
    fn tokenize_into(&self, text: &str, out: &mut Vec<String>);

    /// Tokenizes a batch of inputs.
    ///
    /// The default implementation is a plain sequential map: one fresh
    /// `Vec<String>` per document, with no shared buffer and no parallelism. It
    /// therefore allocates *more* than a [`Self::tokenize_into`] loop over a
    /// reused buffer does, and exists so that generic code can express the
    /// operation — not because it is the fast path.
    ///
    /// Implementations that can do better (amortising one buffer, or
    /// parallelising) should override it. None in this workspace currently
    /// does.
    fn tokenize_batch<S: AsRef<str>>(&self, texts: &[S]) -> Vec<Vec<String>> {
        texts.iter().map(|t| self.tokenize(t.as_ref())).collect()
    }
}

/// A [`Tokenizer`] whose tokens are always contiguous substrings of the input.
///
/// This is the zero-copy path, and implementing it is a promise: every item
/// [`Self::tokens`] yields is a slice of the `text` it was given, byte for byte.
/// Tokenizers that fold case, transliterate, expand contractions or otherwise
/// rewrite the text cannot implement it, and the [`Tokenizer`] contract above
/// still holds — in particular, no yielded slice is empty.
///
/// # Choosing the Right API
///
/// | Method | Use when | Allocations |
/// |---|---|---|
/// | [`tokens`](Self::tokens) | pipelines, folds, early exit, counting | none |
/// | [`tokenize_borrowed`](Self::tokenize_borrowed) | one document, simplest zero-copy call | one `Vec` |
/// | [`tokenize_borrowed_into`](Self::tokenize_borrowed_into) | a corpus through one buffer | none once the buffer is warm |
///
/// [`Self::tokens`] is the primitive; the other two are written in terms of it,
/// so the behaviour exists in one place. Prefer it whenever the tokens are
/// consumed once — `tokens(text).filter(..).count()` never materialises a
/// collection at all. Reach for the owned [`Tokenizer`] methods only when the
/// tokens must outlive the input.
pub trait BorrowingTokenizer: Tokenizer {
    /// Lazily yields the tokens of `text`.
    ///
    /// This is the primitive: every other method on this trait is defined in
    /// terms of it, so an implementation writes the behaviour once. Every item
    /// is a non-empty contiguous slice of `text`.
    fn tokens<'a>(&self, text: &'a str) -> impl Iterator<Item = &'a str>;

    /// Tokenizes `text` into slices borrowed from it. Allocates only the `Vec`.
    fn tokenize_borrowed<'a>(&self, text: &'a str) -> Vec<&'a str> {
        self.tokens(text).collect()
    }

    /// Tokenizes `text` into `out` as borrowed slices, **appending**.
    /// Allocation-free once `out` has sufficient capacity.
    ///
    /// `out` is not cleared, for the reason [`Tokenizer::tokenize_into`] gives:
    /// forgetting `out.clear()` between documents silently accumulates rather
    /// than failing.
    fn tokenize_borrowed_into<'a>(&self, text: &'a str, out: &mut Vec<&'a str>) {
        out.extend(self.tokens(text));
    }
}

// ---------------------------------------------------------------------------
// Stemming
// ---------------------------------------------------------------------------

/// Reduces one token to its stem.
///
/// What a stem *is* belongs to the implementation — each stemmer in this
/// workspace cites the algorithm it implements. This trait fixes only what
/// generic code can rely on:
///
/// * **Total.** Every `&str` has a stem, including `""`. There is no error case
///   and no input that panics.
/// * **Deterministic** for a given stemmer value and input.
/// * **A stem is not required to be a prefix, a substring, or shorter.** Several
///   stemmers here rewrite characters (Snowball's vowel marking, Persian's
///   normalisation), so code that assumes `token.starts_with(&*stem)` is wrong.
///
/// # Choosing the Right API
///
/// | Method | Use when | Allocations |
/// |---|---|---|
/// | [`stem`](Self::stem) | anything — the default choice | none when the token is already its own stem, which is the common case |
/// | [`stem_into`](Self::stem_into) | a hot loop that reuses one `String` | none once the buffer is warm |
/// | [`stem_batch`](Self::stem_batch) | generic code that needs "stem all of these" | one `String` per token, always |
///
/// [`Self::stem`] returns [`Cow`], so the borrowed case costs nothing; that
/// makes it the right call in almost every program. [`Self::stem_into`] is
/// worth the buffer only when profiling says the owned case dominates —
/// note that it **clears** `out` first, the opposite of the tokenizer buffer
/// convention, because a stem is one value rather than a stream.
pub trait Stemmer {
    /// Stems a single token.
    ///
    /// Returns [`Cow::Borrowed`] when the token is already its own stem, which
    /// is the common case for short and irregular words. Callers that need an
    /// owned `String` can use `.into_owned()`.
    fn stem<'a>(&self, token: &'a str) -> Cow<'a, str>;

    /// Stems `token`, writing the result into `out`.
    ///
    /// `out` is **cleared first**, so the result is exactly the stem. This is
    /// deliberately the opposite of [`Tokenizer::tokenize_into`]: that method
    /// produces a stream a caller may want to accumulate, while this one
    /// produces a single value, and appending stems end to end is never what
    /// anyone means.
    fn stem_into(&self, token: &str, out: &mut String) {
        out.clear();
        out.push_str(&self.stem(token));
    }

    /// Stems a batch of tokens.
    ///
    /// A plain sequential map that owns every result, so it allocates for the
    /// borrowed case too — where [`Self::stem`] would not. It exists for
    /// generic code, not for speed.
    fn stem_batch<S: AsRef<str>>(&self, tokens: &[S]) -> Vec<String> {
        tokens
            .iter()
            .map(|t| self.stem(t.as_ref()).into_owned())
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Phonetics
// ---------------------------------------------------------------------------

/// Maps a word to a phonetic key, so that similar-sounding words collide.
///
/// The key's alphabet, length and rules are each algorithm's own contract; this
/// trait fixes only what holds across all of them:
///
/// * **Total.** Every `&str` has a key, and no input panics.
/// * **The empty key is a value, not an error.** A token holding nothing the
///   algorithm can index — punctuation, digits, a script it does not cover —
///   encodes to `""`. Two such tokens therefore [`compare`](Self::compare)
///   equal, which is the honest reading of "same key": neither carries a name
///   the algorithm can index. Callers that need to distinguish "no key" from a
///   key should test the token, not the output.
/// * **Deterministic** for a given encoder value and input.
///
/// # Choosing the Right API
///
/// | Method | Use when | Allocations |
/// |---|---|---|
/// | [`process`](Self::process) | you need the key — to index, store or group by | one `String` |
/// | [`compare`](Self::compare) | you only need "do these two match?" | two `String`s by default |
///
/// [`Self::compare`] is defined as `process(a) == process(b)` and defaults to
/// exactly that, so it allocates both keys. An encoder able to decide the
/// question incrementally may override it; none in this workspace currently
/// does. Encoders with two keys implement [`DoubleKeyPhonetic`] instead of
/// widening this trait.
pub trait Phonetic {
    /// Computes the phonetic key for `token`.
    fn process(&self, token: &str) -> String;

    /// Returns whether two strings share a phonetic key.
    ///
    /// Defined as `process(a) == process(b)`. The default implementation is
    /// literally that, so it computes and allocates both keys.
    fn compare(&self, a: &str, b: &str) -> bool {
        self.process(a) == self.process(b)
    }
}

/// A phonetic algorithm that yields a primary key and, when the name admits a
/// second pronunciation, an alternate.
///
/// Double Metaphone is the worked example: `Smith` and `Schmidt` have different
/// primary keys and match on `Smith`'s alternate, so an index that stores only
/// the primary loses the pair. Modelling the second key here avoids forcing
/// single-key algorithms to invent one.
///
/// **The alternate is [`Option`], not a repeat of the primary.** Most names have
/// no second pronunciation, and an implementation that duplicated the primary
/// into the alternate slot to mean "absent" would make "this name has two
/// spellings that sound the same" indistinguishable from "this name has one" —
/// a sentinel that silently doubles the size of any index built by storing both
/// keys. `None` means there is no alternate.
pub trait DoubleKeyPhonetic {
    /// Computes the primary key and, if the token has one, the alternate.
    ///
    /// Both keys follow the [`Phonetic`] contract on emptiness: a token the
    /// algorithm cannot index has `""` as its primary and `None` as its
    /// alternate.
    fn process_double(&self, token: &str) -> (String, Option<String>);
}
