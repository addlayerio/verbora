//! Sentence structure analysis for Rust.
//!
//! Ports the reference `analyzers` module — [`SentenceAnalyzer`] and the [`SenType`]
//! classification from `SenType.ts`. Given a POS-tagged sentence the analyzer
//! marks prepositional phrases, splits subject from predicate, and labels the
//! sentence as a statement, question, exclamation or command.
//!
//! ```
//! use verbora_analyzers::{SenType, SentenceAnalyzer, TaggedWord as W};
//!
//! let mut a = SentenceAnalyzer::new(vec![
//!     W::new("Vote", "VB"),
//!     W::new("for", "IN"),
//!     W::new("me", "PRP"),
//! ]);
//! a.part();
//!
//! // A verb-initial sentence has no subject, so one is supplied.
//! assert_eq!(a.to_string(), "Vote for me You");
//! assert!(a.implicit_you());
//! assert_eq!(a.type_of(), Ok(Some(SenType::Command)));
//! ```
//!
//! # The input is yours to supply
//!
//! `SentenceAnalyzer` is the one the reference API with no upstream stage: the
//! Brill tagger emits `{ taggedWords: [{ token, tag }] }`, not the
//! `{ tags: [{ token, pos }], punct }` this expects. Tag your text however you
//! like — [`TaggedWord`] borrows the tokens, so building the input costs one
//! `Vec`.
//!
//! # These methods mutate, on purpose
//!
//! The reference holds a *reference* to the caller's tag array and rewrites it:
//! [`SentenceAnalyzer::part`] annotates every tag and can append a subject,
//! [`SentenceAnalyzer::type_of`] pops the last tag. Turning any of that into a
//! pure function would change results, not just style — for instance
//! `type_of` twice in a row classifies a *shorter* sentence the second time.
//! The port keeps the mutation and documents it.
//!
//! # What a naive port gets wrong
//!
//! Each of these is reproduced here and pinned by
//! [`fixtures/analyzers.json`][fixtures]:
//!
//! | Source | Reads as | Actually |
//! |---|---|---|
//! | `pos.match('IN')` | `pos == "IN"` | substring: `VBIN` matches too |
//! | `pos.match('NN')` | `pos == "NN"` | `NNS`, `NNP`, `NNPS` match |
//! | `switch (token)` | exhaustive | no default arm: `sen_type` can stay unset |
//! | `.map(…).join(' ')` | `filter_map` | `null` slots join as `''`: stray spaces |
//! | `tags[i].pp !== true` | "pp is falsy" | strict: `pp: false` is still annotated |
//! | `pos === 'VB'` | any verb | only `VB`; not `VBD`/`VBG`/`VBN`/`VBP`/`VBZ` |
//! | `part()` | idempotent | appends another `You` every call |
//! | `type()` | pure | pops a tag whenever `punct()` is empty |
//!
//! [fixtures]: https://github.com/addlayerio/verbora/blob/main/fixtures/analyzers.json
//!
//! # Deliberate divergences
//!
//! Four the reference behaviours have no Rust counterpart. All four are recorded in
//! the fixtures and asserted as "the reference throws here" or "documented model"
//! by `tests/parity.rs`, so none can drift unnoticed.
//!
//! 1. **Callbacks are not the API.** `new SentenceAnalyzer(pos, cb)` invokes
//!    `cb` synchronously and a non-function `cb` throws. The constructors here
//!    just return the analyzer; [`SentenceAnalyzer::part_with`] and
//!    [`SentenceAnalyzer::type_with`] keep the callback shape for ported code.
//! 2. **`type()` returns two different things.** `undefined` for a function
//!    callback, the classification otherwise. Split into
//!    [`SentenceAnalyzer::type_of`] and [`SentenceAnalyzer::type_with`].
//! 3. **Ill-typed input is unrepresentable.** A tag with a numeric or missing
//!    `pos`, a `punct()` returning `null`/`undefined`/a non-empty string — all
//!    throw in the reference and none can be constructed here.
//! 4. **`punct()` returns an owned value.** The reference's `pop()` mutates
//!    whatever array `punct()` handed back, so a `punct` that returns a
//!    *shared* array shrinks it on every call. A Rust closure returning an
//!    owned [`Punct`] loses that aliasing; reproduce it by popping from state
//!    the closure captures.
//!
//! Everything else is exact, including the key-insertion order of the tag
//! objects — see [`TaggedWord::keys`].

pub mod analyzer;
#[cfg(feature = "parallel")]
mod par;
pub mod sen_type;
pub mod tagged;

pub use analyzer::{SentenceAnalyzer, TypeError};
#[cfg(feature = "parallel")]
pub use par::{AnalyzedSentence, par_analyze_batch};
pub use sen_type::{ParseSenTypeError, SenType};
pub use tagged::{Field, NoPunct, Punct, TaggedSentence, TaggedWord, no_punct};
