//! Brill part-of-speech tagging for Rust.
//!
//! A [`Lexicon`] assigns each token its most common tag; a [`RuleSet`] of
//! context-sensitive [`TransformationRule`]s then rewrites tags that the context
//! contradicts. English (18 rules, 92,662 lexicon entries) and Dutch (285 rules,
//! 11,699 entries) ship in the binary.
//!
//! ```
//! use verbora_tagger::{BrillPosTagger, Language, Lexicon, RuleSet};
//!
//! let lexicon = Lexicon::detached(Some("EN"), Some("NN"), Some("NNP"));
//! let rules = RuleSet::for_language(Language::English);
//! let tagger = BrillPosTagger::new(&lexicon, &rules);
//!
//! let tagged = tagger.tag(["I", "would", "book", "a", "flight"]).unwrap();
//! let tags: Vec<_> = tagged.pairs().map(|(_, t)| t).collect();
//! assert_eq!(tags, [Some("NN"), Some("MD"), Some("VB"), Some("DT"), Some("NN")]);
//! ```
//!
//! `book` is `NN` in the lexicon; `NN VB PREV-WORD-IS would` is what makes it a
//! verb. `I` comes back `NN` rather than `PRP` because that is what the bundled
//! English lexicon says — a known defect in the bundled data, kept because
//! callers depend on the tag it produces, and pinned by a test.
//!
//! # Surprising behaviour, deliberately kept
//!
//! Several behaviours here look like bugs and are specified anyway, because
//! callers depend on them. Each is documented where it lives; the ones most
//! likely to surprise:
//!
//! | Behaviour | Where |
//! |---|---|
//! | `CURRENT-WORD-ENDS-WITH s` is **false** for `"sees"` — it is a first-occurrence test, not a suffix test | [`templates`] |
//! | `new Lexicon()` with no language is a **Dutch** lexicon; `new RuleSet()` with no language is an **English** rule set | [`ruleset::Language`] |
//! | [`Corpus::build_lexicon`] returns the 11,699 Dutch words plus the corpus, and pollutes a process-global dictionary | [`corpus`] |
//! | Every `Lexicon` for a language shares one mutable dictionary; `add_word` on any of them is visible to all | [`lexicon`] |
//! | The trainer's incremental re-scoring is dead code, and it seeds every run with the 18 English rules | [`trainer`] |
//! | Nine of the 36 rule templates cannot be trained at all: they throw | [`templates::ParamSource`] |
//! | `Lexicon::tag_word("")` finds a real entry with **no** tags, so the token is left untagged rather than taking the default | [`lexicon::Categories`] |
//!
//! Where a quirk is genuinely unusable, an opt-out sits beside it —
//! [`Lexicon::detached`], [`Corpus::build_lexicon_detached`],
//! [`Lexicon::reset_shared`] — and each says so in its own documentation. None of
//! them changes the default behaviour.
//!
//! # Strings are UTF-16
//!
//! Positions here are UTF-16 code-unit indices, and three predicates plus the lexicon's
//! capitalisation test are position-sensitive. [`utf16`] carries that dispatch,
//! with an exact ASCII fast path. The one place a lone surrogate would have to be
//! materialised — a PEG syntax error naming the offending character — reports
//! `U+FFFD` instead; see [`parser::SyntaxError::found`].
//!
//! # Aliasing
//!
//! A tagger that aliased the arrays it was handed would let training rewrite
//! the caller's data in place. Rust's ownership rules make that impossible, so
//! values are moved or cloned instead: a caller that inspects its own input
//! after training always sees it unchanged.

#![forbid(unsafe_code)]

pub mod corpus;
pub mod data;
pub mod error;
pub mod lexicon;
pub mod numfmt;
pub mod ordered_object;
pub mod parser;
pub mod rule;
pub mod ruleset;
pub mod sentence;
pub mod tagger;
pub mod templates;
pub mod tester;
pub mod trainer;
pub mod utf16;

pub use corpus::Corpus;
pub use error::TaggerError;
pub use lexicon::{Categories, Lexicon, LookupKey};
pub use parser::{ParsedRule, SyntaxError};
pub use rule::{CATEGORY_WILDCARD, Predicate, TransformationRule};
pub use ruleset::{Language, RuleSet};
pub use sentence::{Prop, Sentence, Tag, TaggedWord};
pub use tagger::{BrillPosTagger, TagIter};
pub use templates::{ParamSource, PredValue, PredicateKind, RuleTemplate, TEMPLATES, Template};
pub use tester::{Accuracy, BrillPosTester};
pub use trainer::BrillPosTrainer;
