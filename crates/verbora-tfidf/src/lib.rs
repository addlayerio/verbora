//! TF-IDF with the reference parity.
//!
//! A port of the reference `tfidf`, reproducing its observable behaviour
//! exactly — including the parts a "clean" rewrite would quietly repair.
//!
//! ```
//! use verbora_tfidf::{DocKey, DocumentInput, Terms, TfIdf};
//!
//! let mut tfidf = TfIdf::new();
//! for text in [
//!     "this document is about node.",
//!     "this document is about ruby.",
//!     "this document is about ruby and node.",
//!     "this document is about node. it has node examples",
//! ] {
//!     tfidf.add_document(DocumentInput::Text(text), DocKey::Undefined, false).unwrap();
//! }
//!
//! assert_eq!(tfidf.idf("node").unwrap(), 1.0 + (4.0f64 / 4.0).ln());
//! assert_eq!(tfidf.tfidfs(Terms::Text("node")).unwrap(), [1.0, 0.0, 1.0, 2.0]);
//!
//! let ranked = tfidf.list_terms(3).unwrap();
//! assert_eq!(ranked[0].term, "node");
//! ```
//!
//! # The five things that make this module hard to port
//!
//! 1. **Documents are the reference objects.** A term spelled `__proto__` is
//!    silently dropped, a term spelled `__key` corrupts the document's key by
//!    string concatenation, and a term named after an `Object.prototype` method
//!    has to be zeroed before it can be counted (issue #119). A
//!    `HashMap<String, f64>` reproduces none of that.
//! 2. **Key order is `for…in` order.** Array-index-like keys are hoisted to the
//!    front in ascending numeric order; everything else keeps insertion order.
//!    `list_terms` and the JSON serialization both expose it, and since the
//!    default tokenizer keeps digit runs, real corpora hit it constantly.
//! 3. **The idf cache probe is a truthiness test on a prototype-backed map.**
//!    The constructor and `addFileSync` install `{}`, while `addDocument` and
//!    `removeDocument` install `Object.create(null)`. On the first kind,
//!    `idf("toString")` returns a *function* and `tfidf(["toString"], 0)` is
//!    `NaN`. That asymmetry is reproduced, not smoothed over.
//! 4. **The tokenizer and the stop-word list are process-global**, mutated by
//!    methods that look like instance methods. See [`globals`].
//! 5. **Float accumulation order is observable.** `tfidf` reduces strictly left
//!    to right and the reference specs assert with `toBe`, so the division inside
//!    `Math.log` stays a division inside [`f64::ln`] and the sum stays
//!    sequential.
//!
//! # Representation
//!
//! Nothing here builds a `terms × documents` matrix. A document is a
//! `Vec<(TermId, f64)>` plus a hash index; terms are interned once per corpus,
//! so a word appearing in fifty documents is allocated once. The
//! document-frequency table is maintained incrementally, which turns the
//! reference's full-corpus rescan on every cache miss into an O(1) lookup —
//! measured flat at ~18 ns from 1 document to 256, against 2.6 µs for the
//! scanning path. Cache *invalidation* timing is untouched, so none of it is
//! observable.
//!
//! Corpora restored with [`TfIdf::from_json`] keep the scanning path, because a
//! deserialized document can hold values a count table cannot represent (a
//! string, a zero, a negative). They still get O(1) property lookup; see
//! [`RawDocument`].

#![cfg_attr(docsrs, feature(doc_cfg))]

mod comparator_sort;
pub mod document;
pub mod encoding;
mod fast_build;
pub mod globals;
pub mod mathlog;
pub mod tfidf;
pub mod value;

pub use document::{BuiltDocument, DocKey, Document, Interner, RawDocument, TermId};
pub use encoding::Encoding;
pub use globals::{StopwordElement, StopwordList, TfIdfTokenizer};
pub use mathlog::math_log;
pub use tfidf::{DocumentInput, TermScore, Terms, TfIdf, TfIdfError};
pub use value::{DynValue, JsonValue, Proto};
