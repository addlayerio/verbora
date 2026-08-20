//! Script detection, language detection, and phonetic-strategy
//! recommendation.
//!
//! This is a purpose-built extension: it answers one question and stops
//! there. It builds on [`verbora_phonetics`]' encoders and
//! [`PhoneticIndex`](verbora_phonetics::PhoneticIndex), so it answers a
//! question those cannot: *given a word or a document, which of Verbora's
//! twelve phonetic encoders should I even use?*
//!
//! # Three layers, kept separate on purpose
//!
//! 1. **Script detection** ([`detect_script`], [`Script::of`]) — Unicode
//!    block classification. No model, no allocation, no dependency. More
//!    reliable than language detection on short input, and cheap enough to
//!    always run first.
//! 2. **Language detection** ([`LanguageDetector`], and
//!    [`WhatlangDetector`] behind the `language-detection` feature) —
//!    statistical, with a real [`Confidence`] and an honest abstention for
//!    input it cannot judge.
//! 3. **Phonetic strategy** ([`recommend`]) — a closed lookup table from
//!    [`Language`] to which encoder(s) actually work for it, and how firmly
//!    that is grounded ([`StrategyBasis`]). Not statistical at all: once
//!    the language is known, the recommendation is a fact about Verbora's
//!    own encoders, not a guess.
//!
//! There is deliberately **no** `auto_phonetic_encode(text) -> String`
//! anywhere in this crate. [`recommend`] takes a [`Language`], never a
//! `&str` and never a detector, so a statistical guess can never be
//! laundered into a phonetic key without the caller seeing it happen.
//! [`AutoPhoneticStrategy`] composes all three at the edge, and even then
//! only above a threshold the caller chose.
//!
//! # Choosing the right entry point
//!
//! | You have | Call | What it costs |
//! |---|---|---|
//! | the [`Language`] already | [`recommend`] | a closed `match` over 22 arms; no detector, no feature, no model. **Prefer this whenever you can.** |
//! | text, and only need the writing system | [`detect_script`] | one pass, no allocation |
//! | a [`Script`] and no language guess | [`recommend_for_script`] | a `match`; coarser answers, never a language-level claim |
//! | text and no idea | [`AutoPhoneticStrategy::detect_and_recommend`] | a full statistical detection on top of the above |
//! | a large corpus | [`par_detect_batch`] (feature `parallel`) | one detection per text, fanned out across threads |
//!
//! # Names are not language
//!
//! This crate detects **language**, not nationality, ethnicity, or name
//! origin. [`Language::Italian`] means "this text's linguistic signal
//! matches Italian," never "this name sounds Italian" — a surname can have
//! Italian origins and appear in an English sentence with no
//! contradiction. Nothing here infers anything about a person from their
//! name.
//!
//! # Short inputs are genuinely ambiguous — this is not a corner case
//!
//! `"Das ist ein deutscher Satz"` and `"Müller"` are different problems.
//! The second is a single word, plausibly a name, and no statistical
//! detector — `whatlang` included — claims reliable single-word accuracy.
//! [`LanguageDetection`] models this honestly: it can be empty,
//! [`LanguageDetection::best_above`] requires *you* to decide what
//! "confident enough" means, and [`AutoPhoneticStrategy`] never silently
//! recommends a strategy below your own threshold. `tests/ambiguity.rs`
//! asserts the specific short inputs (`"hotel"`, `"radio"`, `"piano"`,
//! `"normal"`, `"color"`, short proper names) that must **not** resolve to
//! a single confident language.
//!
//! # Feature-gating: automatic detection is optional, the rest is not
//!
//! [`Language`], [`Script`]/[`detect_script`], [`recommend`],
//! [`LanguageDetector`] (the trait), [`FallbackDetector`] and
//! [`AutoPhoneticStrategy`] (generic over any [`LanguageDetector`]) all
//! compile and work with **zero** extra dependencies. Only
//! [`WhatlangDetector`] needs `language-detection`, which is the only thing
//! that pulls in `whatlang`; only `HashedLinearDetector` needs
//! `fast-language-detection` (which adds no dependency at all, just
//! compiled-in weight tables); only [`par_detect_batch`] needs `parallel`.
//! Neither detector feature enables the other.
//!
//! # Which detector to reach for
//!
//! [`DefaultDetector`] is [`WhatlangDetector`] — the detector every
//! unqualified "Verbora language detection" claim in this workspace refers
//! to. Measured on the published 13-language × 4-tier UDHR evaluation set
//! (`benchmarks/competitive/datasets/language-accuracy`; the same set
//! `benchmarks/competitive/rust-competitors` reports on, and the same one
//! `tests/default_detector.rs` re-scores as an executed test rather than a
//! quoted claim):
//!
//! | | short_word | short_phrase | sentence | paragraph | overall |
//! |---|---|---|---|---|---|
//! | [`WhatlangDetector`] (**default**) | 10/13 | 13/13 | 13/13 | 13/13 | **49/52** |
//! | `HashedLinearDetector` | 7/13 | 12/13 | 13/13 | 13/13 | 45/52 |
//! | [`FallbackDetector`]`<Hashed, Whatlang>` | 10/13 | 13/13 | 13/13 | 13/13 | **49/52** |
//! | `whichlang` 0.1.1, for scale | 9/13 | 13/13 | 13/13 | 13/13 | 48/52 |
//!
//! **Why the fastest detector is deliberately not the default.**
//! `HashedLinearDetector` beats every third-party Rust detector this
//! workspace benchmarks — `whichlang` included — at every input tier, and
//! that is exactly why its accuracy number has to travel with it: it costs
//! 4 of those 52 items, all on input shorter than a sentence, which leaves
//! it *less* accurate than the `whichlang` it outruns. Shipping it as the
//! default would make this crate's headline claim "faster than whichlang"
//! and its unstated fine print "and wronger than whichlang on the only tier
//! where the three detectors differ at all" — the
//! speed-quoted-without-correctness presentation this workspace's benchmark
//! charter forbids outright. So it stays opt-in behind
//! `fast-language-detection`, first-class and separately labelled in both
//! the speed and accuracy tables, never folded into the default's row.
//!
//! [`FallbackDetector`] is the composition that buys the speed back without
//! the trade: it matches the default tier for tier (its own doc comment
//! states precisely what that does and does not prove) and is only as slow
//! as the default on the input the fast model declines to judge.
//!
//! # Performance
//!
//! The relative-speed figures this crate previously published for its
//! detectors, its `recommend` path and its parallel batch all pre-date the
//! Rust-native migration's behaviour changes and are therefore stale. They
//! are not restated here: measurement is pending, and this crate publishes
//! no number it has not measured against the code as it now stands.

#![cfg_attr(doctest, doc = include_str!("../README.md"))]

mod auto;
mod detect;
mod fallback;
#[cfg(feature = "fast-language-detection")]
mod hashed_linear;
mod language;
#[cfg(feature = "parallel")]
mod parallel;
mod script;
mod strategy;
#[cfg(feature = "language-detection")]
mod whatlang_detector;

pub use auto::{AutoPhoneticStrategy, AutoResult};
pub use detect::{Confidence, LanguageCandidate, LanguageDetection, LanguageDetector};
pub use fallback::FallbackDetector;
#[cfg(feature = "fast-language-detection")]
pub use hashed_linear::HashedLinearDetector;
#[doc(hidden)]
#[cfg(feature = "fast-language-detection")]
pub use hashed_linear::train_support;
pub use language::{Language, ParseLanguageError};
#[cfg(feature = "parallel")]
pub use parallel::par_detect_batch;
pub use script::{Script, detect_script};
pub use strategy::{
    PhoneticRecommendation, PhoneticStrategy, StrategyBasis, TransliterationAdvice,
    apply_transliteration, recommend, recommend_for_script,
};
#[cfg(feature = "language-detection")]
pub use whatlang_detector::WhatlangDetector;

/// The detector this crate means by "Verbora language detection" when no
/// detector is named — [`WhatlangDetector`].
///
/// This alias exists so "which one is the default?" is a fact in the code
/// with exactly one definition, rather than a convention re-stated in
/// prose across a crate, a benchmark, and a docs site that can drift
/// apart. `tests/default_detector.rs` pins it two ways: this alias must
/// behave identically to [`WhatlangDetector`] on every item of the
/// published evaluation corpus, and that corpus's per-tier accuracy
/// counts are asserted exactly — so promoting a faster detector here is
/// possible, but not *quietly* possible.
///
/// It is not a re-export a caller must use. Name [`WhatlangDetector`],
/// `HashedLinearDetector`, or [`FallbackDetector`] directly whenever
/// the choice matters to you; reach for this only when you want whatever
/// this crate currently considers the right default, and are content for
/// that to be re-settled on future evidence.
///
/// See the crate-level docs' comparison table for the measurement that
/// settles it, and why the ~50-400x faster `HashedLinearDetector` is
/// not what this points at.
#[cfg(feature = "language-detection")]
pub type DefaultDetector = WhatlangDetector;
