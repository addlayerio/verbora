//! Rule-based clause analysis over part-of-speech-tagged English sentences.
//!
//! Give it a tagged sentence; it reports the prepositional phrases, splits
//! subject from predicate, and classifies the clause as declarative,
//! interrogative, imperative or exclamative.
//!
//! ```
//! use verbora_analyzers::{ImpliedSubject, SentenceType, TaggedWord as W, analyze};
//!
//! let sentence = [
//!     W::new("The", "DT"),
//!     W::new("angry", "JJ"),
//!     W::new("bear", "NN"),
//!     W::new("chased", "VBD"),
//!     W::new("the", "DT"),
//!     W::new("squirrel", "NN"),
//!     W::new(".", "."),
//! ];
//! let analysis = analyze(&sentence);
//!
//! assert_eq!(analysis.subject_to_string(), "The angry bear");
//! assert_eq!(analysis.predicate_to_string(), "chased the squirrel");
//! assert_eq!(analysis.sentence_type(), Some(SentenceType::Declarative));
//! assert_eq!(analysis.implied_subject(), None);
//! ```
//!
//! # What this crate assumes about its input
//!
//! This crate has no upstream stage of its own: it consumes tags, it does not
//! produce them. Everything it can do therefore rests on assumptions about who
//! produced those tags, and leaving them implicit is how a downstream stage
//! ends up silently disagreeing with the stage that feeds it. They are:
//!
//! 1. **The tag set is Penn Treebank.** Santorini, *Part-of-Speech Tagging
//!    Guidelines for the Penn Treebank Project*, 3rd revision, 1990; Marcus,
//!    Santorini & Marcinkiewicz, *Building a Large Annotated Corpus of English:
//!    The Penn Treebank*, Computational Linguistics 19(2), 1993. A tag outside
//!    that set is [`TagClass::Other`] and takes part in no rule. See
//!    [`TagClass`] for the complete mapping.
//! 2. **One tag per word.** Ambiguity classes such as `NN|IN` — which a Brill
//!    lexicon carries by the hundred — are *not* tags, and match nothing. If
//!    your tagger emits them, resolve them before calling.
//! 3. **The words are one clause, in order.** Nothing here finds clause
//!    boundaries, so a two-clause sentence is analysed as if it were one.
//! 4. **Tags are spelled exactly.** Comparison is byte-for-byte and
//!    case-sensitive. `"nn"` and `" NN"` are not `"NN"`.
//! 5. **The language is English.** Every rule below is a rule of English
//!    syntax; none generalises to another language by relabelling tags.
//!
//! The crate validates none of this. Bad tags do not raise an error — they
//! simply match no rule, and the analysis degrades to what the remaining
//! evidence supports.
//!
//! # Text units
//!
//! * **Structure** is analysed in **tagged words**. A token is opaque: nothing
//!   splits it, folds its case, trims it or normalises it, and every index and
//!   range this crate reports is a word index into the sentence you supplied.
//! * **Terminator recognition** is the single exception, and it works in
//!   **Unicode scalar values**: a token is terminal punctuation when it is
//!   exactly one scalar and that scalar appears in the table [`Terminator`]
//!   specifies. A token of two scalars is not a terminator, and neither is a
//!   token that merely contains one.
//!
//! So no combining mark, astral scalar or bidi control can change how a
//! sentence is split — only which single-scalar token ends it.
//!
//! # The pipeline
//!
//! [`analyze`] runs four stages in this order. Each stage sees the output of
//! the ones before it, so the order is part of the contract.
//!
//! **Stage 0 — terminator.** If the sentence's last word's *token* is one of
//! the scalars [`Terminator`] specifies, that word is the terminator and the
//! remaining words are the **body**. Otherwise the body is the whole sentence.
//! The word's tag is not consulted: taggers disagree about how to tag
//! punctuation, while the token itself is unambiguous.
//! [`analyze_with_terminator`] supplies the terminator out of band instead, and
//! never removes a word from the body.
//!
//! **Stage 1 — prepositional phrases.** Left to right over the body. A word
//! tagged `IN` opens a phrase unless one is already open. The first following
//! noun (`NN`, `NNS`, `NNP`, `NNPS`) closes it, **inclusive** — the noun heads
//! the phrase's object, so it belongs inside. A phrase with no following noun
//! runs to the end of the body. Phrases never overlap and never contain the
//! terminator.
//!
//! **Stage 2 — imperative test.** The body is an imperative clause when its
//! first word that is neither an adverb (`RB`, `RBR`, `RBS`) nor an
//! interjection (`UH`) is tagged `VB`. English imperatives take the base form
//! of the verb and have no overt subject, and an adverbial or interjection may
//! precede the verb — *Always look both ways*, *Please vote* (Quirk, Greenbaum,
//! Leech & Svartvik, *A Comprehensive Grammar of the English Language*, 1985,
//! §11.24). Only `VB` qualifies: `MD`, `VBD`, `VBP` and `VBZ` are finite and
//! cannot head an imperative.
//!
//! *Limit worth knowing:* only adverbs and interjections are skipped, so
//! *Please, vote* — with a comma between — is not detected, because the comma's
//! tag is neither.
//!
//! **Stage 3 — subject and predicate.** In an imperative clause the predicate
//! starts at the first body word: there is no overt subject, and
//! [`SentenceAnalysis::implied_subject`] reports the understood *you*.
//! Otherwise the predicate starts at the first body word that is a verb
//! (`MD`, `VB`, `VBD`, `VBG`, `VBN`, `VBP`, `VBZ`) and is **not inside a
//! prepositional phrase** — so *the bear in the running water drank* opens its
//! predicate at *drank*, not at *running*. Everything before that word is the
//! subject; if there is no such word, the whole body is subject. Words inside a
//! phrase take [`Role::PrepositionalPhrase`] whichever side they fall on, and
//! the terminator takes [`Role::Terminator`].
//!
//! **Stage 4 — clause type.** Terminal punctuation decides when it is present,
//! because it is the strongest signal available:
//!
//! | Terminator | Imperative clause | Otherwise |
//! |---|---|---|
//! | question mark | [`Interrogative`] | [`Interrogative`] |
//! | exclamation mark | [`Imperative`] | [`Exclamative`] |
//! | full stop | [`Imperative`] | [`Declarative`] |
//!
//! With no terminator, the clause itself is the only evidence, in this order:
//! an imperative clause is [`Imperative`]; a body starting with a wh-word
//! (`WDT`, `WP`, `WP$`, `WRB`) is [`Interrogative`]; a body starting with a
//! finite verb is [`Interrogative`], since neither a declarative nor an
//! imperative English clause can begin with one; a body ending in a personal
//! pronoun preceded — past any adverbs — by a finite verb is a tag question and
//! so [`Interrogative`] (*…, isn't it*). Failing all of those the type is
//! `None`: no evidence, rather than a guess or an `Unknown` sentinel.
//!
//! [`Interrogative`]: SentenceType::Interrogative
//! [`Imperative`]: SentenceType::Imperative
//! [`Exclamative`]: SentenceType::Exclamative
//! [`Declarative`]: SentenceType::Declarative
//!
//! # Nothing here rewrites your sentence
//!
//! [`analyze`] borrows a `&[TaggedWord]` and returns a [`SentenceAnalysis`]
//! that borrows it too. No token is copied, no word is appended, no word is
//! removed, and no field of a word is annotated in place. Analysing the same
//! slice twice gives equal results, and the implied subject of an imperative is
//! reported as a value ([`ImpliedSubject`]) rather than inserted as a synthetic
//! word.
//!
//! # Choosing the right API
//!
//! Three pairs of entry points cover different shapes of the same job.
//!
//! | You have | Call | Cost |
//! |---|---|---|
//! | One sentence, punctuation kept as a token | [`analyze`] | two allocations |
//! | One sentence, punctuation stripped by the tokenizer | [`analyze_with_terminator`] | two allocations |
//! | Hundreds of independent sentences, `parallel` enabled | `par_analyze_batch` | one `Vec` plus the per-sentence cost |
//!
//! * **[`analyze`] is the right call for the large majority of programs.** It
//!   is the only one that can report which word supplied the terminator
//!   ([`SentenceAnalysis::terminator_index`]), and it needs nothing from the
//!   caller beyond the sentence.
//! * **[`analyze_with_terminator`]** exists for tokenizers that discard
//!   punctuation. It never consumes a word, so `analyze_with_terminator(s,
//!   None)` is also the way to force a trailing full stop to be analysed as an
//!   ordinary word.
//! * **`par_analyze_batch`** is a `rayon` fan-out over [`analyze`], behind
//!   the `parallel` feature — it exists only when that feature is enabled. It
//!   calls [`analyze`] and does not re-derive it, so
//!   the two cannot drift. One sentence's analysis is small — often smaller than
//!   the cost of scheduling a `rayon` task — so a plain
//!   `sentences.iter().map(|s| analyze(s)).collect()` loop is simpler and
//!   usually at least as fast for small batches. Reach for the parallel form
//!   with hundreds of sentences or more, and measure your own workload.
//!
//! Within a [`SentenceAnalysis`], the lazy `*_tokens` iterators and the owned
//! `*_to_string` methods answer the same question at different costs; see
//! [`SentenceAnalysis`] for that table.
//!
//! # Performance
//!
//! [`analyze`] is a constant number of linear passes over the sentence — at
//! most six, none nested — with no work that depends on token length, and two
//! allocations: one `Vec<Role>` of exactly `sentence.len()` elements and one
//! `Vec<Range<usize>>` sized to the number of prepositional phrases (not
//! allocated when there are none). **Not yet benchmarked**:
//! `benches/analyzers.rs` measures this pipeline, but no run against it exists,
//! so this crate publishes no timing figures.
//!
//! # What this crate does not do
//!
//! It does not tokenize, tag, find clause or sentence boundaries, resolve tag
//! ambiguity, parse a full constituency or dependency tree, or handle any
//! language but English. A prepositional phrase here is the flat `IN … noun`
//! span above, not a nested constituent, and attachment is never resolved.

mod analysis;
mod sentence_type;
mod tag;
mod terminator;
mod word;

#[cfg(feature = "parallel")]
mod par;

pub use analysis::{ImpliedSubject, Role, SentenceAnalysis, analyze, analyze_with_terminator};
pub use sentence_type::{ParseSentenceTypeError, SentenceType};
pub use tag::TagClass;
pub use terminator::Terminator;
pub use word::TaggedWord;

#[cfg(feature = "parallel")]
pub use par::par_analyze_batch;
