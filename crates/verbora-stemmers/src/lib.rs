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
//! Eleven of the sixteen index by **UTF-16 code unit** rather than by Unicode
//! scalar value, because their region arithmetic compares positions against
//! literal constants (`if rv > 3`, `if r1 < 3 { r1 = 3 }`, the Italian and
//! Portuguese `length < 3` gates) and those constants were chosen against a
//! code-unit count. For the Basic Multilingual Plane the two units coincide
//! exactly, so the choice is observable only for astral-plane characters —
//! emoji, mathematical alphanumerics — which no rule table in this crate is
//! written over. `PorterStemmer::stem("😀s")` treats its input as three units
//! long and stems it; a scalar-counting implementation would see two and
//! return it unchanged.
//!
//! This is inherited rather than chosen, and it is the one item of this
//! crate's own migration that is still open: settling it means restating every
//! region constant in scalars and re-deriving eleven algorithms against the
//! result. Lancaster, Carry, Japanese and Indonesian are provably unaffected
//! and already run on `&str`.
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
