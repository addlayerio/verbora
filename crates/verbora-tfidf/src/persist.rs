//! The serialized corpus: its shape, and the two ways writing or reading one
//! can fail.
//!
//! The shape itself is documented on [`crate::TfIdf::to_json`], which is where
//! a caller meets it; this module is private, so its `//!` prose would never be
//! rendered.
use serde::{Deserialize, Serialize};

use crate::stamp::StampError;

/// A serialized corpus.
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Artifact {
    /// The compatibility stamp. Verified against this build *before* the rest
    /// of the artifact is interpreted, so a stale artifact is refused rather
    /// than half-read.
    #[serde(rename = "_verbora", default)]
    pub(crate) stamp: serde_json::Value,
    /// The analyzer the terms were derived with.
    pub(crate) analyzer: AnalyzerRecord,
    /// The documents, in corpus order.
    pub(crate) documents: Vec<DocumentRecord>,
}

/// The recorded analyzer.
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct AnalyzerRecord {
    /// [`CaseFold::as_str`](crate::CaseFold).
    pub(crate) case_fold: String,
    /// The stop-word list, in order, or absent when the analyzer filters
    /// nothing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) stop_words: Option<Vec<String>>,
}

/// One recorded document.
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct DocumentRecord {
    /// The caller-supplied key, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) key: Option<String>,
    /// `(term, count)` pairs in first-occurrence order.
    pub(crate) terms: Vec<(String, u32)>,
}

/// Why a corpus could not be serialized.
///
/// Both variants are refusals to write, so nothing partial ever reaches the
/// caller's file.
#[derive(Debug)]
#[non_exhaustive]
pub enum ExportError {
    /// The corpus's analyzer runs a tokenizer installed through
    /// [`Analyzer::with_tokenizer`](crate::Analyzer::with_tokenizer).
    ///
    /// The artifact would carry terms whose derivation it cannot describe: a
    /// custom tokenizer is code, not data, and no compatibility stamp can
    /// record it. Writing the file anyway would produce something that *looks*
    /// validated and is not, which is the exact failure the stamp exists to
    /// prevent — so serialization is refused instead.
    ///
    /// Serialize the corpus's source documents, or rebuild it under the default
    /// tokenizer.
    CustomTokenizer,
    /// The JSON writer failed.
    ///
    /// Every value in the artifact is a `String`, a `u32` or a container of
    /// those, none of which `serde_json` can reject, so this variant exists for
    /// completeness rather than because a caller should expect it.
    Json(serde_json::Error),
}

impl std::fmt::Display for ExportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CustomTokenizer => f.write_str(
                "this corpus uses a tokenizer installed with `Analyzer::with_tokenizer`, \
                 which a serialized artifact cannot describe; serialize the source \
                 documents instead, or rebuild the corpus under the default tokenizer",
            ),
            Self::Json(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for ExportError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::CustomTokenizer => None,
            Self::Json(e) => Some(e),
        }
    }
}

/// Why a serialized corpus could not be restored.
///
/// The variants separate three different things that can be wrong, because they
/// call for three different responses:
///
/// * [`Self::Parse`] — the bytes are not the JSON this crate writes. The file is
///   damaged or is not a serialized corpus; repair or re-fetch it.
/// * [`Self::Stamp`] — the bytes parsed, but the corpus was not written by this
///   build (or by any build that recorded which one it was). The file is
///   intact; its *terms* are the problem, and the fix is to rebuild the corpus
///   from its source documents. See [`StampError`].
/// * the remaining variants — the file is this build's shape but says something
///   a corpus cannot mean. It was edited, or written by something that is not
///   this crate.
#[derive(Debug)]
#[non_exhaustive]
pub enum RestoreError {
    /// The bytes were not valid JSON, or not the shape this crate writes.
    Parse(serde_json::Error),
    /// The corpus carries no usable compatibility stamp, or one from another
    /// build.
    Stamp(StampError),
    /// The recorded `case_fold` is not one this build knows.
    UnknownCaseFold(String),
    /// A term was recorded with a count of zero.
    ///
    /// A term with no occurrences is not a term of the document: it would make
    /// `document_frequency` count a document that does not contain it, and the
    /// difference is invisible in every later answer. Refused rather than
    /// normalised away.
    ZeroCount {
        /// The document's position in the corpus.
        document: usize,
        /// The term whose count was zero.
        term: String,
    },
    /// A document recorded the same term twice.
    ///
    /// Which count wins would be a property of the reader rather than of the
    /// artifact, so there is no safe way to accept it.
    DuplicateTerm {
        /// The document's position in the corpus.
        document: usize,
        /// The term that appeared more than once.
        term: String,
    },
}

impl std::fmt::Display for RestoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Parse(e) => write!(f, "{e}"),
            Self::Stamp(e) => write!(f, "{e}"),
            Self::UnknownCaseFold(name) => write!(
                f,
                "serialized corpus records an unknown case fold {name:?} \
                 (expected \"lowercase\" or \"none\")"
            ),
            Self::ZeroCount { document, term } => write!(
                f,
                "document {document} records term {term:?} with a count of zero; \
                 a term with no occurrences is not a term of the document"
            ),
            Self::DuplicateTerm { document, term } => write!(
                f,
                "document {document} records term {term:?} more than once"
            ),
        }
    }
}

impl std::error::Error for RestoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Parse(e) => Some(e),
            Self::Stamp(e) => Some(e),
            Self::UnknownCaseFold(_) | Self::ZeroCount { .. } | Self::DuplicateTerm { .. } => None,
        }
    }
}

impl From<StampError> for RestoreError {
    fn from(e: StampError) -> Self {
        Self::Stamp(e)
    }
}
