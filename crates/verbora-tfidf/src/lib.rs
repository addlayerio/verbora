//! Sparse TF-IDF over a growable in-memory corpus.
//!
//! ```
//! use verbora_tfidf::TfIdf;
//!
//! let mut corpus = TfIdf::new();
//! corpus.add_document("this document is about node");
//! corpus.add_document("this document is about ruby");
//! corpus.add_document("this document is about ruby and node");
//! corpus.add_document("this document is about node, it has node examples");
//!
//! // Every score, in corpus order.
//! assert_eq!(corpus.tfidfs("node"), [1.0, 0.0, 1.0, 2.0]);
//!
//! // …or ranked, best first.
//! assert_eq!(corpus.rank("node")[0].document, 3);
//!
//! // The terms that make one document distinctive.
//! assert_eq!(corpus.list_terms(3).unwrap()[0].term, "node");
//! ```
//!
//! # What this crate specifies
//!
//! TF-IDF is a family of weightings, not one formula, so Verbora states which
//! member it computes and pins it by test. With `n` documents, `count(t, d)`
//! occurrences of term `t` in document `d`, and `df(t)` documents containing
//! `t`:
//!
//! ```text
//! idf(t)      = 1 + ln(n / (1 + df(t)))
//! tfidf(q, d) = Σ  count(t, d) · idf(t)          summed left to right
//!              t∈q
//! ```
//!
//! Term frequency is the **raw count**, not a normalised frequency: nothing is
//! divided by document length, because a caller who wants that division can
//! perform it with [`Document::total_terms`] and a caller who does not cannot
//! undo it. The two `1 +` terms are the smoothing — the inner one keeps a term
//! that appears in no document finite, and the outer one keeps a term that
//! appears in *every* document weighted rather than annihilated.
//!
//! Both of the crate's numeric hazards are closed rather than documented:
//!
//! * **No sentinel.** An out-of-range document is [`Option::None`], never a
//!   score. An empty corpus has no idf, and says so with `None` rather than
//!   returning the `-∞` that `ln(0)` would produce.
//! * **No `NaN`, no infinity.** Every score this crate can produce is finite.
//!   [`TfIdf`] states the bound and the test enumerates it.
//! * **A specified logarithm.** [`natural_log`] rather than `f64::ln`, so the
//!   same corpus scores identically on every platform. `f64::ln` is the
//!   platform's `libm`, which Rust does not specify.
//!
//! # What a term is
//!
//! Everything above is defined over *terms*, and a term is whatever this
//! corpus's [`Analyzer`] produces: segment the text at [UAX #29] word
//! boundaries, keep the segments containing a letter or a digit, case-fold each
//! one, drop the ones on the stop-word list. Segmentation is
//! `verbora-tokenizers`' — this crate does not re-derive word boundaries, so
//! `"don't"`, `"3.14"`, `"1,000"` and `"a:b"` are each one term, as the
//! standard says.
//!
//! The analyzer belongs to the corpus. There is no process-global tokenizer and
//! no process-global stop-word list, so two corpora in one program cannot
//! change each other's answers, and nothing has to be locked to read one.
//!
//! # Choosing the right API
//!
//! Three axes, and one rule each.
//!
//! **Text or terms.** Every query comes in two forms. The plain one takes text
//! and runs the analyzer on it; the `_terms` one takes strings that have
//! already been analyzed and looks them up verbatim.
//!
//! | Call | Use when |
//! |---|---|
//! | [`tfidf`](TfIdf::tfidf), [`tfidfs`](TfIdf::tfidfs), [`rank`](TfIdf::rank) | you have text — a search box, a sentence, a document |
//! | [`tfidf_terms`](TfIdf::tfidf_terms), [`tfidfs_terms`](TfIdf::tfidfs_terms) | your terms came from elsewhere: a stemmer, an n-gram builder, or [`add_terms`](TfIdf::add_terms) |
//!
//! Use the text form unless you built the corpus with
//! [`add_terms`](TfIdf::add_terms). The term form skips the analyzer entirely,
//! so a query it is given must already be spelled the way the corpus stores it
//! — that is its purpose, and its hazard.
//!
//! **One document or many.** Resolving a query costs one analyzer pass plus one
//! term-table lookup per term; scoring one document costs one hash probe per
//! term.
//!
//! | Call | Resolves the query | Use when |
//! |---|---|---|
//! | [`tfidf`](TfIdf::tfidf) | once per call | you want one document's score |
//! | [`tfidfs`](TfIdf::tfidfs) | once for the whole corpus | you want every score, in corpus order |
//! | [`rank`](TfIdf::rank) | once for the whole corpus | you want every score, best first |
//!
//! `tfidfs` in a loop is not the same as `tfidf` in a loop: it resolves once.
//! Prefer it whenever you are about to score more than one document.
//! `rank` is `tfidfs` plus a sort into a total order — it costs the sort and
//! saves you writing one.
//!
//! **Statistics take terms, never text.** [`idf`](TfIdf::idf),
//! [`document_frequency`](TfIdf::document_frequency) and
//! [`term_count`](TfIdf::term_count) match their argument literally against the
//! corpus's stored terms. If what you have is text, run
//! [`Analyzer::terms`] first. They are the primitives the query APIs are built
//! from, exposed because reproducing a score by hand is a reasonable thing to
//! want.
//!
//! **Ingestion.**
//!
//! | Call | Use when | Allocates |
//! |---|---|---|
//! | [`add_document`](TfIdf::add_document) | the common case | nothing per term with the default analyzer |
//! | [`add_document_with_key`](TfIdf::add_document_with_key) | you need to map a position back to a source | one `String` for the key |
//! | [`add_documents`](TfIdf::add_documents) | you have a slice and want the assigned positions | as above; **no** speed advantage over a loop |
//! | [`add_terms`](TfIdf::add_terms) | your terms came from another pipeline | nothing beyond the corpus growth |
//! | [`par_add_documents`](TfIdf::par_add_documents) | a large batch, `parallel` feature | one `String` per term, plus a `Vec` per document |
//! | [`add_document_from_path`](TfIdf::add_document_from_path) | the document is a UTF-8 file | the file's contents |
//!
//! `add_document` is the right call for the large majority of programs. The
//! parallel batch trades one allocation per term for running the analyzer on
//! more than one core, and its crossover point is **UNMEASURED**; do not adopt
//! it without measuring your own corpus shape.
//!
//! # Representation
//!
//! Nothing here builds a `terms × documents` matrix. Terms are interned once
//! per corpus, so a word appearing in fifty documents is stored once; a
//! document is a `Vec<(TermId, u32)>` plus a hash index; and the
//! document-frequency table is a `Vec<u32>` maintained incrementally, which
//! makes [`TfIdf::idf`] an array load rather than a scan of the corpus. There
//! is no idf cache, and so no question about when one is stale.
//!
//! # Persistence is version-locked
//!
//! Every key a serialized corpus holds is a term, and terms come out of Unicode
//! tables that move between releases — so a corpus written by one build is not
//! readable by a build whose word boundaries or case mappings differ.
//! [`TfIdf::to_json`] therefore writes a compatibility stamp,
//! [`TfIdf::from_json`] refuses any artifact whose stamp is absent, damaged or
//! foreign, and an artifact whose derivation the stamp *cannot* describe — one
//! built with a custom tokenizer — is refused at write time instead. See
//! [`ArtifactStamp`] for what the stamp covers, and [`RestoreError`] for how a
//! caller tells a damaged file from an incompatible one.
//!
//! [UAX #29]: https://www.unicode.org/reports/tr29/

#![cfg_attr(docsrs, feature(doc_cfg))]

mod analyzer;
mod corpus;
mod document;
mod interner;
mod log;
mod persist;
mod stamp;

pub use analyzer::{Analyzer, CaseFold, Tokenize};
pub use corpus::{DocumentScore, TermScore, TfIdf};
pub use document::{Document, MAX_TERM_COUNT};
pub use log::natural_log;
pub use persist::{ExportError, RestoreError};
pub use stamp::{
    ArtifactStamp, CONTEXT_PROBES, SCHEMA, STAMP_PROPERTY, StampError, lowercase_fingerprint,
};
