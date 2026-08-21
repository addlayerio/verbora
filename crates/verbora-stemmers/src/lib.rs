//! Sixteen stemmers for fourteen languages.
//!
//! A stemmer maps inflected forms of a word onto one string, so that a search
//! or a classifier can treat them as the same term. Verbora ships the English
//! Porter stemmer, the Lancaster (Paice/Husk) stemmer, Snowball-family
//! stemmers for eleven more languages, the Carry variant for French, a
//! Japanese katakana stemmer and a dictionary-driven Indonesian stemmer.
//!
//! ```
//! use verbora_stemmers::{LancasterStemmer, PorterStemmer, TokenizeAndStem};
//!
//! let porter = PorterStemmer::new();
//! assert_eq!(porter.stem("running"), "run");
//! assert_eq!(
//!     porter.tokenize_and_stem("My dog is very fun to play with", false),
//!     ["dog", "fun", "plai"]
//! );
//!
//! // A different algorithm on the same shared English stop-word list.
//! assert_eq!(LancasterStemmer::new().stem("maximum"), "maxim");
//! ```
//!
//! # Two ways in
//!
//! Every stemmer has an inherent `stem` that takes one token and returns its
//! stem. Fifteen of them also implement [`TokenizeAndStem`], which cuts text
//! into tokens, drops stop words and stems what is left — read that trait for
//! the four entry points it offers and which to choose. [`StemmerJa`] is the
//! exception: UAX #29's default word rules do not segment Japanese, so the
//! caller supplies the segmentation and calls `stem` per token.
//!
//! # The text unit
//!
//! **One Unicode scalar value is one unit**, in every stemmer here and in
//! every other Verbora crate. Region boundaries (R1, R2, RV), the short-word
//! gates and each rule's removal `size` are all counts of scalar values.
//!
//! The algorithms themselves choose this. Snowball and Porter are published
//! over *letters* — "the region after the first non-vowel following a vowel",
//! a measure over consonant/vowel sequences, `length < 3` — and not one of
//! those definitions names a unit of storage. Every letter class they test is
//! a set of scalar values and every table entry is a sequence of them, so the
//! scalar reading is the faithful one.
//!
//! Below `U+10000` the readings coincide exactly, which the crate proves
//! rather than assumes: `among`'s tables are built in both and swept across
//! the whole Basic Multilingual Plane asserting identical answers. Astral
//! characters are where they part. `PorterStemmer::stem("😀s")` returns
//! `"😀s"` unchanged, because `"😀s"` is two scalar values and the
//! three-letter gate declines to run.
//!
//! # State that outlives a call
//!
//! Two APIs are stateful in ways that are part of their observable behaviour:
//!
//! * [`PorterStemmerNl`] carries a **sticky** `suffix_e_removed` flag. Step 2
//!   sets it, nothing resets it, and step 3b reads it — so
//!   `stem("onaantastbar")` is `"onaantastbar"` on a fresh stemmer and
//!   `"onaantast"` once any earlier word has tripped the flag. The state lives
//!   in the value, so a caller who wants determinism constructs a fresh
//!   stemmer per word.
//! * The **stop-word lists are process-global**. English lives in
//!   [`verbora_core`] and is shared with the phonetics crate, so
//!   adding a stop word through [`PorterStemmer`] changes
//!   [`LancasterStemmer`]. The other thirteen languages hang off
//!   [`Language`], which documents what a concurrent reader can observe.
//!
//! # Data is audited, not trusted
//!
//! Every stop-word list and every rule table is walked through the pipeline
//! that consumes it, entry by entry, by the crate's own test suite: a stop word
//! the tokenizer can no longer produce and a suffix the prelude can no longer
//! leave standing are both silent failures that a green suite will not catch
//! on its own. Four dead entries have been found that way — Dutch `"je "`,
//! German `"ei,"`, Spanish `"  aseis"` and Italian `"Yamo"` — and the last two
//! were rules, not stop words.
//!
//! # Batch stemming (`parallel` feature)
//!
//! [`TokenizeAndStem::par_tokenize_and_stem_batch`] runs many documents' worth
//! of [`TokenizeAndStem::tokenize_and_stem`] across a rayon thread pool, one
//! task per document — deliberately *not* one task per word. See that method's
//! documentation for the measurements behind that choice: this crate's
//! per-word costs (tens of nanoseconds to a few microseconds) are close enough
//! to rayon's own per-task scheduling cost that word-level parallelism would
//! mostly measure the scheduler, while a whole document clears that floor.

#![cfg_attr(doctest, doc = include_str!("../README.md"))]
#![cfg_attr(docsrs, feature(doc_cfg))]
// Documentation in this crate links to `TokenizeAndStem::par_tokenize_and_stem_batch` (`parallel`).
// Those links resolve on docs.rs, which builds all features; without the
// feature the targets do not exist, and the lint would fire on every plain
// `cargo doc`. It stays armed in the all-features build, which is the one
// that ships.
#![cfg_attr(not(feature = "parallel"), allow(rustdoc::broken_intra_doc_links))]

mod among;
mod base;
mod carry;
mod data;
mod de;
mod en;
mod es;
mod fa;
mod fr;
mod id;
mod it;
mod ja;
mod lancaster;
mod nl;
mod no;
mod pt;
mod ru;
mod stopwords;
mod sv;
#[cfg(test)]
mod test_support;
mod uk;
mod units;

pub use base::{Casing, Stems, TokenizeAndStem};
pub use carry::CarryStemmerFr;
pub use de::{PorterStemmerDe, PorterStemmerDeOptions};
pub use en::PorterStemmer;
pub use es::PorterStemmerEs;
pub use fa::PorterStemmerFa;
pub use fr::{PorterStemmerFr, Regions};
pub use id::{Removal, RemovalKind, RuleResult, StemmerId};
pub use it::PorterStemmerIt;
pub use ja::StemmerJa;
pub use lancaster::LancasterStemmer;
pub use nl::PorterStemmerNl;
pub use no::PorterStemmerNo;
pub use pt::PorterStemmerPt;
pub use ru::PorterStemmerRu;
pub use stopwords::Language;
pub use sv::PorterStemmerSv;
pub use uk::PorterStemmerUk;
