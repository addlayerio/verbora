//! Number inflection for English, French and Japanese nouns, English verbs, and
//! ordinal numerals.
//!
//! ```
//! use verbora_inflectors::{NounInflector, OrdinalInflector, PresentVerbInflector};
//!
//! let nouns = NounInflector::new();
//! assert_eq!(nouns.pluralize("cactus"), "cacti");
//! assert_eq!(nouns.singularize("parentheses"), "parenthesis");
//!
//! let verbs = PresentVerbInflector::new();
//! assert_eq!(verbs.singularize("fly"), "flies");
//!
//! assert_eq!(OrdinalInflector::nth(23), "23rd");
//! ```
//!
//! # The contract
//!
//! **The text unit is one Unicode scalar value.** Case classification iterates
//! [`char`]s; nothing in this crate counts UTF-16 code units or bytes. `"👍"` is
//! one character, and `pluralize("👍")` is `"👍s"`.
//!
//! **Every inflector method is total.** There is no input — empty, whitespace,
//! astral, malformed-looking — for which an inflector returns an error or
//! panics. The empty token has no inflected form and is returned unchanged. The
//! only fallible operation in the crate is building a [`Rule`], which reports a
//! [`RuleError`] at construction so that applying it cannot fail later.
//!
//! **No sentinels, no `NaN`.** Absence is [`Option::None`]. The crate contains
//! no floating point at all: an ordinal takes an [`i64`], because `1.5th` is not
//! an ordinal.
//!
//! **Nothing silently rewrites its input.** A token whose case the crate cannot
//! classify as capitalised or upper-case keeps its own case exactly — `iPhone`
//! pluralises to `iPhones`, not `iphones`. A word an inflector has no rule for
//! comes back byte-identical.
//!
//! **The crate root is the entire public surface.** Every module is private and
//! everything is re-exported here.
//!
//! **Determinism.** The same input gives the same output on every platform and
//! every build. There is no global mutable state and no hash-order dependence.
//! The compiled rule tables are shared process-wide behind a
//! [`LazyLock`](std::sync::LazyLock), but they are immutable; rules and
//! irregular pairs a caller adds live on the instance they were added to.
//!
//! # The pipeline
//!
//! Both directions of every inflector run the same four stages, and the first
//! stage that produces a form wins:
//!
//! 1. **Rules the caller added**, in the order they were added
//!    ([`add_plural`](SingularPluralInflector::add_plural),
//!    [`add_singular`](SingularPluralInflector::add_singular)).
//! 2. **The invariant list** — words this direction leaves alone. Looked up by
//!    the lowercase form.
//! 3. **The irregular table** — forms no rule derives (`child`/`children`),
//!    with [`add_irregular`](SingularPluralInflector::add_irregular) additions
//!    shadowing the built-in entries. Also looked up by the lowercase form.
//! 4. **The ordered rule list**, first match wins.
//!
//! If no stage claims the token, the token itself is the answer.
//!
//! A stage that matches but would rewrite a non-empty token to the empty string
//! **declines**, and the next stage is consulted. An inflected form of a word is
//! a word.
//!
//! Case is restored from the *original* token onto whatever the winning stage
//! produced — see [`CaseMode`] for the classification. A stage that returns the
//! token unchanged skips restoration entirely, so it cannot rewrite anything.
//!
//! # Choosing the right API
//!
//! Several groups of paired calls exist. In every one, the plain form is the
//! right choice for the large majority of programs.
//!
//! | Call | Allocates | Use when | Cost to the caller |
//! |---|---|---|---|
//! | [`pluralize`](SingularPluralInflector::pluralize) / [`singularize`](SingularPluralInflector::singularize) | one `String` per call | almost always | none |
//! | [`pluralize_into`](SingularPluralInflector::pluralize_into) / [`singularize_into`](SingularPluralInflector::singularize_into) | nothing, once the buffer is warm | a loop over a corpus, where one allocation per word is measurable | a buffer to own, and a `clear()` that is a silent correctness bug when forgotten |
//! | [`OrdinalInflector::nth`] | one `String` per call | almost always | none |
//! | [`OrdinalInflector::nth_into`] | nothing | formatting many ordinals into one buffer | the same `clear()` hazard |
//! | [`OrdinalInflector::suffix`] | nothing, ever | the numeral is formatted elsewhere — a table cell, a thousands-separated figure | you format the numeral |
//! | [`CaseMode::apply`] / [`CaseMode::apply_into`] | one `String` / nothing | reproducing the crate's case restoration outside it | as above |
//!
//! The decision, as a tree:
//!
//! ```text
//! do you need only the suffix, not the numeral?     -> OrdinalInflector::suffix
//! are you inflecting one word, or a handful?        -> pluralize / singularize / nth
//! are you inflecting a corpus in a loop?
//!     can you keep one String alive across it?      -> *_into, clearing each iteration
//!     otherwise                                     -> pluralize / singularize / nth
//! ```
//!
//! `*_into` is **not** faster per word: it runs exactly the same rules. It
//! removes one allocation per call and nothing else. `benches/inflectors.rs`
//! measures both shapes over the shared corpus; no figure is quoted here,
//! because a number in prose outlives the code it described.
//!
//! ```
//! use verbora_inflectors::NounInflector;
//!
//! let inflector = NounInflector::new();
//! let mut buffer = String::new();
//! let mut plurals = Vec::new();
//! for word in ["party", "wolf", "criterion"] {
//!     buffer.clear(); // forgetting this concatenates the corpus
//!     inflector.pluralize_into(word, &mut buffer);
//!     plurals.push(buffer.clone());
//! }
//! assert_eq!(plurals, ["parties", "wolves", "criteria"]);
//! ```
//!
//! # Extending an inflector
//!
//! Caller-added rules are consulted before every built-in table, and apply only
//! to the instance they were added to.
//!
//! ```
//! use verbora_inflectors::{NounInflector, Rule};
//!
//! let mut inflector = NounInflector::new();
//! inflector.add_plural(Rule::new("(?i)(code|ware)$", "${1}z").unwrap());
//! inflector.add_singular(Rule::new("(?i)(code|ware)z$", "${1}").unwrap());
//!
//! assert_eq!(inflector.pluralize("code"), "codez");
//! assert_eq!(inflector.singularize("warez"), "ware");
//! // The built-in rules still apply to everything else.
//! assert_eq!(inflector.pluralize("bus"), "buses");
//! ```
//!
//! # Where the rules come from
//!
//! Each rule table states a productive pattern of its language and cites the
//! grammar that describes it; each lexical list has a membership criterion that
//! a test enforces. The sources are recorded once, at the top of the table
//! module, and are in summary: Quirk et al., *A Comprehensive Grammar of the
//! English Language*, and Huddleston & Pullum, *The Cambridge Grammar of the
//! English Language*, for English; Grevisse & Goosse, *Le Bon Usage*, and the
//! *Dictionnaire de l'Académie française*, for French; Martin, *A Reference
//! Grammar of Japanese*, and Shibatani, *The Languages of Japan*, for Japanese;
//! *The Chicago Manual of Style* and *New Hart's Rules* for the ordinals.
//!
//! Every rule carries a *witness*: a token that must reach that rule and no
//! earlier one. The test suite walks every rule of all eight tables and fails if
//! any witness is claimed by an earlier rule, if any rule does not match its own
//! witness, or if the table does not produce the recorded form. A rule without a
//! reachable witness is deleted rather than documented.
//!
//! # What this crate does not do
//!
//! * **It does not resolve orthographic ambiguity that the language leaves
//!   open.** `bases` is the plural of both `base` and `basis`;
//!   [`NounInflector::singularize`] answers `base`, and the choice is recorded
//!   in the rule table rather than derived.
//! * **It does not know parts of speech.** `singularize` on an English verb
//!   form and on a noun are different calls, on different types, and neither
//!   checks which you meant.
//! * **It does not segment.** A token is whatever the caller passes; nothing
//!   here splits on spaces or punctuation.
//! * **The lexical lists are not exhaustive** and do not claim to be. A word
//!   absent from a list takes the regular rule.

#![cfg_attr(doctest, doc = include_str!("../README.md"))]

mod case;
mod data;
mod engine;
mod ordinal;
mod rule;

pub use case::CaseMode;
pub use engine::{
    NounInflector, NounInflectorFr, NounInflectorJa, PresentVerbInflector, SingularPluralInflector,
};
pub use ordinal::{Gender, OrdinalInflector, OrdinalInflectorFr};
pub use rule::{Rule, RuleError};
