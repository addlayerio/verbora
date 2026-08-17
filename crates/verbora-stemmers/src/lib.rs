//! Stemmers with the reference parity.
//!
//! Seventeen exported APIs: the English Porter stemmer, twelve Snowball ports
//! (Persian, French, German, Ukrainian, Russian, Spanish, Italian, Norwegian,
//! Swedish, Portuguese, Dutch and the Carry variant of French), the Lancaster
//! stemmer, a Japanese katakana stemmer, an Indonesian dictionary stemmer, and
//! the [`Token`] helper the Portuguese algorithm is written against.
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
//! # Design
//!
//! * **Iterator first.** [`TokenizeAndStem::stems`] is the primitive; the
//!   `Vec`-returning `tokenize_and_stem` is `stems(..).collect()`. Everything
//!   else — the stop-word filter, the per-language casing rules, the gate
//!   regexes — lives in that one iterator, so the six variants of
//!   `tokenizeAndStem` in the reference are one piece of code here.
//! * **UTF-16 where it is observable.** The Snowball algorithms index by code
//!   unit and compare positions against constants, so they run over a `Vec<u16>`
//!   (see the private `units` module). Lancaster, Carry, Japanese and Indonesian are
//!   provably unaffected and stay on `&str`.
//! * **Generated tables.** Every rule table, stop-word list and character class
//!   is machine-derived from the reference, never transcribed by hand.
//!
//! # State that outlives a call
//!
//! Two APIs are stateful in ways that are part of their observable behaviour, and
//! both are reproduced rather than cleaned up:
//!
//! * [`PorterStemmerNl`] carries a **sticky** `suffix_e_removed` flag. Step 2 sets
//!   it, nothing resets it, and step 3b reads it — so `stem("onaantastbar")` is
//!   `"onaantastbar"` on a fresh stemmer and `"onaantast"` once any earlier word
//!   has tripped the flag. Over the reference's own 45,669-word Dutch corpus this
//!   is the difference between 237 mismatches (what the reference spec asserts)
//!   and 235.
//! * The **stop-word lists are process-global**. English lives in
//!   [`verbora_core::stopwords`] and is shared with the phonetics crate, exactly
//!   as the reference `stopwords` is shared in the reference: adding a stop
//!   word through [`PorterStemmer`] changes [`LancasterStemmer`]. The other
//!   thirteen languages are in [`stopwords`].
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

pub mod base;
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
pub mod stopwords;
mod sv;
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

/// The mutable stemming token from the reference `token`.
///
/// Re-exported from [`verbora_core`], which is where it lives so that other
/// crates can share it. Note its documented non-BMP divergence (**D1** in
/// `docs/PARITY.md`): it stores `Vec<char>`, so an astral-plane character counts
/// as one where the reference counts two. [`PorterStemmerPt`] does **not** use it
/// for exactly that reason — it runs on a UTF-16 buffer instead — but the type is
/// part of the reference's exported surface and so is part of this crate's.
pub use verbora_core::Token;
