//! Language and script detection, and phonetic-strategy recommendation.
//!
//! This is a purpose-built extension: it answers one question and stops
//! there — no `docs/FEATURE_MATRIX.md` entry claims more,
//! and it must never be reported as one. It builds on
//! [`verbora_phonetics`]' [`PhoneticEncoder`](verbora_phonetics::PhoneticEncoder)/
//! [`PhoneticIndex`](verbora_phonetics::PhoneticIndex) (a prior Verbora
//! extension), so it answers a question that module cannot: *given a word
//! or document, which of Verbora's phonetic encoders should I even use?*
//!
//! # Three layers, kept separate on purpose
//!
//! 1. **Script detection** ([`detect_script`]) — Unicode-range
//!    classification, no model, no allocation. More reliable than
//!    language detection on short input, and cheap enough to always run
//!    first.
//! 2. **Language detection** ([`LanguageDetector`], [`WhatlangDetector`]
//!    behind the `language-detection` feature) — statistical, real
//!    confidence values, honestly uncertain on short input.
//! 3. **Phonetic strategy** ([`recommend`]) — a small, closed lookup table
//!    from [`Language`] to which encoder(s) actually work for it. Not
//!    statistical at all: once the language is known, the recommendation
//!    is a fact about Verbora's own encoders, not a guess.
//!
//! Composing all three is [`AutoPhoneticStrategy`] — but reach for the
//! layer you actually need. A caller who already knows the language calls
//! [`recommend`] directly and pays nothing for detection.
//!
//! # Names are not language
//!
//! This crate detects **language**, not nationality, ethnicity, or name
//! origin. `Language::Italian` means "this text's linguistic signal
//! matches Italian," never "this name sounds Italian" — a surname can
//! have Italian origins and appear in an English sentence with no
//! contradiction. Nothing in this crate infers anything about a person
//! from their name; see [`LanguageDetector`]'s own doc comment on short
//! inputs for why a single name is close to un-detectable as a *language*
//! signal in the first place, independent of origin questions this crate
//! does not attempt to answer.
//!
//! # Short inputs are genuinely ambiguous — this is not a corner case
//!
//! `"Das ist ein deutscher Satz"` and `"Müller"` are different problems.
//! The second is a single word, plausibly a name, and no statistical
//! detector — `whatlang` included — claims reliable single-word accuracy
//! (see [`WhatlangDetector`]'s own doc comment for the real numbers this
//! project measured). [`LanguageDetection`] models this honestly:
//! `candidates` can be empty, [`LanguageDetection::best_above`] requires
//! *you* to decide what "confident enough" means, and
//! [`AutoPhoneticStrategy`] never silently recommends a strategy below
//! your own threshold. See `tests/ambiguity.rs` for the specific short
//! inputs (`"hotel"`, `"radio"`, `"piano"`, `"normal"`, `"color"`, short
//! proper names) this crate's own test suite asserts must **not** resolve
//! to a single confident language.
//!
//! # Feature-gating: automatic detection is optional, the rest is not
//!
//! [`Language`], [`Script`]/[`detect_script`], [`recommend`],
//! [`LanguageDetector`] (the trait), and [`AutoPhoneticStrategy`] (generic
//! over any [`LanguageDetector`]) all compile and work with **zero**
//! extra dependencies. Only [`WhatlangDetector`] — a real, working
//! detector — needs the `language-detection` feature, which is the only
//! thing that pulls in `whatlang`. A caller who wants explicit-language
//! phonetic strategy selection, or plugs in their own detector, pays
//! nothing for automatic detection they never asked for.
//!
//! A second, opt-in detector exists behind `fast-language-detection`:
//! [`HashedLinearDetector`], a latency-first alternative with compiled-in
//! models and no extra dependencies. It trades coverage and hard-input
//! judgement for a several-hundred-fold latency reduction — see
//! `src/hashed_linear.rs`'s own doc comment for exactly what it does and
//! does not claim. [`WhatlangDetector`] stays the accuracy reference;
//! neither feature enables the other.

mod auto;
mod detect;
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
pub use detect::{LanguageCandidate, LanguageDetection, LanguageDetector};
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
    PhoneticRecommendation, PhoneticStrategy, TransliterationAdvice, apply_transliteration,
    recommend, recommend_for_script,
};
#[cfg(feature = "language-detection")]
pub use whatlang_detector::WhatlangDetector;
