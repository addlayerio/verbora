//! Noun, verb and ordinal inflection, reproducing the reference exactly.
//!
//! Six public types, ported from the reference `inflectors` module:
//!
//! | Type | Reference | What it does |
//! |---|---|---|
//! | [`NounInflector`] | `noun_inflector` | English nouns |
//! | [`NounInflectorFr`] | `fr/noun_inflector` | French nouns |
//! | [`NounInflectorJa`] | `ja/noun_inflector` | Japanese nouns |
//! | [`PresentVerbInflector`] | `present_verb_inflector` | English present tense |
//! | [`CountInflector`] | `count_inflector` | English ordinals (`1st`, `2nd`) |
//! | [`CountInflectorFr`] | `fr/count_inflector` | French ordinals (`1er`, `2e`) |
//!
//! ```
//! use verbora_inflectors::{CountInflector, NounInflector, PresentVerbInflector};
//!
//! let nouns = NounInflector::new();
//! assert_eq!(nouns.pluralize("octopus").unwrap(), "octopi");
//! assert_eq!(nouns.singularize("parentheses").unwrap(), "parenthesis");
//!
//! let verbs = PresentVerbInflector::new();
//! assert_eq!(verbs.singularize("fly").unwrap(), "flies");
//!
//! assert_eq!(CountInflector::nth(23), "23rd");
//! ```
//!
//! # What "exactly" costs
//!
//! The first four share one engine — `TenseInflector.ize`, a four-stage `||`
//! chain described on [`SingularPluralInflector`] — and almost every interesting
//! behaviour of this crate follows from some the reference detail inside it. The
//! four that break a naive port, each documented at the code that reproduces it:
//!
//! 1. **`||` treats `""` as no match.** A rule that fires and rewrites the token
//!    to nothing is discarded, and the chain falls through to the *unchanged*
//!    token. `PresentVerbInflector::pluralize("Es")` is `"Es"`.
//! 2. **`restoreCase` indexes UTF-16 code units** and compares case with a
//!    string round trip. `pluralize("👍")` is `"👍s"` but `pluralize("A")` is
//!    `"AS"`, and `pluralize("1")` is `"1S"`. See [`case`].
//! 3. **The patterns are the reference regexes.** `.` excludes four line
//!    terminators rather than one, and `/i` refuses to fold `ſ` into `s`, so
//!    every pattern is translated rather than handed to the `regex` crate as-is.
//!    See [`pattern`].
//! 4. **One rule needs a negative lookahead** — `/^(?!talis|.*hu)(.*)man$/i`,
//!    which is why `workman` pluralises to `workmen` but `human` to `humans`.
//!    The `regex` crate cannot express it, so it is hand-written. See [`Rule`].
//!
//! All four are pinned by `fixtures/inflectors.json` — 17,000 calls recorded
//! from the reference implementation and replayed in `tests/parity.rs`.
//!
//! # Extending the rules
//!
//! The reference lets callers add rules at runtime, and so does this port. New
//! rules are consulted *before* every built-in table, and apply only to the
//! instance they were added to.
//!
//! ```
//! use verbora_inflectors::{NounInflector, Rule};
//!
//! let mut inflector = NounInflector::new();
//! inflector.add_plural(Rule::from_pattern("(code|ware)", true, "$1z").unwrap());
//! inflector.add_singular(Rule::from_pattern("(code|ware)z", true, "$1").unwrap());
//!
//! assert_eq!(inflector.pluralize("code").unwrap(), "codez");
//! assert_eq!(inflector.singularize("warez").unwrap(), "ware");
//! // The built-in rules still apply to everything else.
//! assert_eq!(inflector.pluralize("bus").unwrap(), "buses");
//! ```
//!
//! # Divergences
//!
//! * The empty token, on which the reference throws `TypeError`, is an
//!   [`EmptyToken`] error rather than a panic or a silent `""`.
//! * the reference's coercion of arguments that are neither string nor number to
//!   [`CountInflector::nth`] (`true` → `"truest"`, `[1]` → `"1st"`) has no Rust
//!   analogue and is not modelled; integers, floats and strings each get their
//!   own entry point.
//! * [`CountInflector::nth`] takes an `i64` and is exact across its whole range,
//!   where the reference's `f64` rounds beyond 2^53 − 1. Use
//!   [`CountInflector::nth_f64`] to reproduce that rounding on purpose.
//! * A caller-supplied rule always rewrites the first match only, matching the
//!   reference's non-global patterns. A reference `RegExp` carrying `/g` would
//!   rewrite every match; [`Rule`] has no equivalent flag.
//! * `TenseInflector.addForm` and the `FormSet` type are not exposed. `addForm`
//!   takes two raw table objects, and `FormSet` is a bare pair of a mutable
//!   array and a null-prototype object; neither survives translation as an API.
//!   Everything they are used for is reachable through
//!   [`add_irregular`](SingularPluralInflector::add_irregular) and [`Rule`],
//!   which is also all the reference itself uses them for.

pub mod case;
pub mod pattern;

mod count;
mod data;
mod inflector;
mod numfmt;
mod replace;
mod rules;

pub use case::{CaseMode, restore_case};
pub use count::{CountInflector, CountInflectorFr};
pub use inflector::{
    EmptyToken, NounInflector, NounInflectorFr, NounInflectorJa, PresentVerbInflector,
    SingularPluralInflector,
};
pub use pattern::PatternError;
pub use rules::Rule;
