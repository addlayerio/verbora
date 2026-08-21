//! Brill transformation-based part-of-speech tagging.
//!
//! ```
//! use verbora_tagger::{BrillTagger, Language, Lexicon, RuleSet};
//!
//! let lexicon = Lexicon::bundled(Language::English);
//! let rules = RuleSet::bundled(Language::English);
//! let tagger = BrillTagger::new(&lexicon, &rules);
//!
//! let tagged = tagger.tag("would book a flight".split(' '));
//! let tags: Vec<&str> = tagged.iter().map(|w| w.tag().as_str()).collect();
//! assert_eq!(tags, ["MD", "VB", "DT", "NN"]);
//! ```
//!
//! `book` is a noun in the lexicon; the rule `NN VB PREV-WORD-IS would` is what
//! makes it a verb here.
//!
//! # How it works
//!
//! Two stages, both from Eric Brill's papers:
//!
//! 1. **Initial-state annotation.** [`Lexicon::tag_of`] gives every token its
//!    most frequent tag; an unknown token takes a default, or a *capitalised*
//!    default when its first scalar has the Unicode `Uppercase` property.
//!    Brill (1995) §2.
//! 2. **Transformation.** Each [`Rule`] of the [`RuleSet`] is applied, in order,
//!    to the whole sentence before the next rule runs. The [`Condition`]s a rule
//!    can test are Brill's contextual templates (1992) and lexicalised templates
//!    (1994), plus three Verbora-defined token-shape tests.
//!
//! [`Trainer`] learns a rule set from an annotated [`Corpus`] using the
//! error-driven procedure of Brill (1995) §2.
//!
//! # The token contract
//!
//! **This crate never tokenizes.** A token is a non-empty string containing no
//! scalar with the Unicode `White_Space` property, and the caller decides what
//! produced them. That matters more than it sounds: the bundled dictionaries are
//! keyed by whitespace-delimited corpus tokens, so `well-known` and `A.A.U.` are
//! single keys, and a tokenizer that splits inside them cannot reach them —
//! measured at **15,543 of 92,538 English keys (16.8%)** for a [UAX #29] word
//! tokenizer such as `verbora_tokenizers::WordTokenizer`. [`Lexicon`]'s module
//! documentation gives the full table and the guidance that follows from it;
//! `tests/tokenization.rs` walks every bundled key and pins the numbers.
//!
//! Nothing here rewrites a token. Case folding, trimming and normalisation are
//! the caller's explicit choice; tokens come out of the tagger byte-identical to
//! the ones that went in.
//!
//! # The text unit
//!
//! Tagging is a whole-token operation. Only two things look inside a token, and
//! both are defined on **Unicode scalar values**: the capitalisation test reads
//! the first scalar, and [`Condition::CurrentWordEndsWith`] is `str::ends_with`,
//! which for well-formed UTF-8 is a scalar-sequence suffix match. Nothing counts
//! UTF-16 code units and nothing indexes a token numerically, so an astral
//! scalar is one thing everywhere.
//!
//! # Failure
//!
//! Tagging cannot fail. [`BrillTagger::tag`] returns a value, not a `Result`:
//! every condition is total, every out-of-range position simply does not match,
//! and every token gets a tag.
//!
//! What is fallible is building the inputs. There are five such operations, and
//! only the first two are *parsing* — the other three are validation, which
//! rejects a value that was never text to begin with. Each reports precisely
//! what it rejected rather than repairing it.
//!
//! | Operation | Error | Rejects |
//! |---|---|---|
//! | [`Rule`] and [`RuleSet`] from text, via `FromStr` or [`RuleSet::parse_lines`] | [`RuleParseError`], [`RuleSetParseError`] | too few fields, an unknown condition name, the wrong argument count for the condition named, a field that is not a valid tag or word, and a boolean argument that is neither `YES` nor `NO` |
//! | [`Corpus::parse_brown`] | [`CorpusParseError`] | a `token_TAG` pair with no tag, an empty token, an empty tag, or the wildcard tag `*` |
//! | [`Corpus::build_lexicon`] | [`LexiconError`] | a corpus token that is not a conforming lexicon key — which [`Corpus::from_sentences`] can produce and [`Corpus::parse_brown`] cannot |
//! | [`Lexicon::insert`] | [`LexiconError`] | an empty key, a key containing whitespace, or an empty tag list |
//! | [`Tag::new`] and [`Word::new`], and their `FromStr` and `TryFrom<&'static str>` | [`LiteralError`] | the empty string, and any Unicode `White_Space` scalar |
//!
//! The wildcard rule is worth stating on its own, because it reaches two of
//! those rows. [`Tag::new`] rejects `"*"` with [`LiteralError::Wildcard`]: the
//! rule language spells its wildcard pattern that way, so a tag named `*` could
//! be written into a rule but never read back out of one. [`Corpus::parse_brown`]
//! inherits it — a corpus line tagging a token `*` is rejected as
//! [`CorpusParseError::WildcardTag`], not silently accepted. [`Word::new`] accepts
//! `"*"`, because there it is an ordinary token — the bundled English lexicon
//! keys it.
//!
//! # Bundled data
//!
//! | Language | Lexicon entries | Rules |
//! |---|---|---|
//! | [`Language::English`] | 92,538 | 18 |
//! | [`Language::Dutch`] | 11,699 | 273 |
//!
//! Plus [`brill_paper_rule_strings`], the ten rules of Brill (1992), Table 1.
//! All of it is packed at build time and read in place from the executable, so
//! constructing a [`Lexicon`] parses nothing and allocates nothing.
//!
//! The English source JSON holds 92,662 entries and 124 of them are not
//! bundled. Its keys are written in the escaped notation of the tagged corpus
//! they came from, where a `/` inside a token appears as `\/` because a bare `/`
//! separates a token from its tag, and an asterisk appears as `\*` because a
//! bare `*` marks a null element; `build.rs` decodes that, so `Asia\/Pacific`
//! ships as the key `Asia/Pacific` and `\*` as the key `*`. 123 keys turn out to
//! be markup rather than tokens and are dropped, and one (the key `""`, whose
//! tag list was also empty) violates the entry contract. Each count is a
//! constant `data::tests` asserts, per language and per reason.
//!
//! # References
//!
//! * Eric Brill, *A Simple Rule-Based Part of Speech Tagger*, ANLP-92, 152–155.
//! * Eric Brill, *Some Advances in Transformation-Based Part of Speech Tagging*,
//!   AAAI-94.
//! * Eric Brill, *Transformation-Based Error-Driven Learning and Natural
//!   Language Processing: A Case Study in Part-of-Speech Tagging*, Computational
//!   Linguistics 21(4), 1995, 543–565.
//!
//! [UAX #29]: https://www.unicode.org/reports/tr29/

#![cfg_attr(doctest, doc = include_str!("../README.md"))]
#![cfg_attr(docsrs, feature(doc_cfg))]
// The docs above link to the `parallel`-gated batch helper. That link
// resolves on docs.rs, which builds all features; without the feature the
// target does not exist and the lint would fire on every plain `cargo doc`.
// It stays armed in the all-features build, which is the one that ships.
#![cfg_attr(not(feature = "parallel"), allow(rustdoc::broken_intra_doc_links))]
#![forbid(unsafe_code)]

mod condition;
mod corpus;
mod data;
mod language;
mod lexicon;
mod parse;
mod rule;
mod ruleset;
mod tag;
mod tagger;
mod template;
mod text;
mod trainer;

pub use condition::Condition;
pub use corpus::{Corpus, CorpusParseError};
pub use language::{Language, brill_paper_rule_strings};
pub use lexicon::{Entries, Lexicon, LexiconError, Tags};
pub use parse::RuleParseError;
pub use rule::{Rule, TagPattern};
pub use ruleset::{RuleSet, RuleSetParseError};
pub use tag::{LiteralError, Tag, TaggedToken, Word};
pub use tagger::{BrillTagger, Evaluation, TagStream};
pub use template::Template;
pub use trainer::{Trainer, Training, TrainingStep};
