//! Brill transformation-based part-of-speech tagging.
//!
//! # No dictionary ships with this crate. Bring your own.
//!
//! This is a tagger **engine**, not a tagger. Versions before 0.3 embedded
//! English and Dutch dictionaries; those files could not be redistributed under
//! this crate's licence and were removed (`data/NOTICE.md` records which and
//! why). What ships now is the algorithm, the rule language, the
//! [`Trainer`] — and [`RuleSet::brill_1992`], the ten published rules of Brill
//! (1992), Table 1.
//!
//! The lexicon is yours to supply, from any source you have the right to use:
//!
//! ```
//! use verbora_tagger::{BrillTagger, Corpus, RuleSet, Tag};
//!
//! // An annotated corpus in Brown `token_TAG` form — a few lines here, a real
//! // corpus in practice.
//! let corpus = Corpus::parse_brown(
//!     "the_DT dog_NN barks_VBZ\n\
//!      the_DT book_NN is_VBZ good_JJ\n\
//!      I_PRP would_MD book_VB a_DT flight_NN",
//! )?;
//!
//! // Tag frequencies become the initial-state annotator, most frequent first.
//! let lexicon = corpus.build_lexicon(Tag::new("NN")?)?;
//! assert_eq!(lexicon.tag_of("book").as_str(), "NN"); // noun twice, verb once
//!
//! // One rule, in the tag set the corpus itself uses.
//! let rules: RuleSet = "NN VB PREV-TAG MD".parse()?;
//! let tagger = BrillTagger::new(&lexicon, &rules);
//!
//! let tagged = tagger.tag("I would book a flight".split(' '));
//! let tags: Vec<&str> = tagged.iter().map(|w| w.tag().as_str()).collect();
//! assert_eq!(tags, ["PRP", "MD", "VB", "DT", "NN"]);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! `book` is a noun in the lexicon; the rule `NN VB PREV-TAG MD` is what makes
//! it a verb here. [`Lexicon::new`] plus [`Lexicon::insert`] is the other way
//! in, when the entries are yours to write down rather than to count.
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
//! error-driven procedure of Brill (1995) §2 — which, with no bundled rules to
//! fall back on, is the main way to get a rule set that fits your tag set.
//!
//! # The tag set is whatever your data says it is
//!
//! Verbora attaches no meaning to a tag beyond string identity, so a lexicon and
//! a rule set agree only if they were written against the same tag set. That is
//! not a formality: a rule whose condition names a tag nothing produces is not
//! an error, it simply never fires, and a tagger built from a mismatched pair
//! runs to completion and returns the initial-state annotation unchanged.
//!
//! [`RuleSet::brill_1992`] is the one place this crate can trip you up, and it
//! says so in its own documentation: those rules are written in **Brown** corpus
//! tags (`AT`, `PPS`, `PPO`, `HVD`, `NP`), so they do nothing to a Penn Treebank
//! lexicon.
//!
//! # The token contract
//!
//! **This crate never tokenizes.** A token is a non-empty string containing no
//! scalar with the Unicode `White_Space` property, and the caller decides what
//! produced them. Which keys a program can reach therefore depends on the
//! tokenizer: a dictionary keyed by whitespace-delimited corpus tokens holds
//! `well-known` and `A.A.U.` as single keys, and a [UAX #29] word tokenizer such
//! as `verbora_tokenizers::WordTokenizer` splits inside both, so those entries
//! can never be looked up. Key the lexicon with the producer that will tokenize
//! the text — [`Corpus::build_lexicon`] does that for you — and the question
//! does not arise. [`Lexicon`]'s own documentation states the rule in full;
//! `tests/tokenization.rs` demonstrates both the matched pair and the mismatch.
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
//! `"*"`, because there it is an ordinary token.
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
pub use lexicon::{Entries, Lexicon, LexiconError, Tags};
pub use parse::RuleParseError;
pub use rule::{Rule, TagPattern};
pub use ruleset::{RuleSet, RuleSetParseError};
pub use tag::{LiteralError, Tag, TaggedToken, Word};
pub use tagger::{BrillTagger, Evaluation, TagStream};
pub use template::Template;
pub use trainer::{Trainer, Training, TrainingStep};
