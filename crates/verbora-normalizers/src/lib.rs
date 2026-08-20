//! Rewrites text, and says in each function's name exactly what the rewrite is.
//!
//! Five functions. Four are the Unicode normalization forms of [UAX #15]; the
//! fifth is a Verbora-defined fold built out of them.
//!
//! | Function | Rewrite | Annex |
//! |---|---|---|
//! | [`nfd`] | Canonical Decomposition | [UAX #15] §1.2 |
//! | [`nfc`] | Canonical Decomposition, then Canonical Composition | [UAX #15] §1.2 |
//! | [`nfkd`] | Compatibility Decomposition | [UAX #15] §1.2 |
//! | [`nfkc`] | Compatibility Decomposition, then Canonical Composition | [UAX #15] §1.2 |
//! | [`remove_diacritics`] | NFD, drop every scalar with `Canonical_Combining_Class != 0`, NFC | Verbora, defined on the item |
//!
//! This is the one crate in Verbora's text-shaping group whose job *is*
//! rewriting. `verbora-tokenizers` and `verbora-ngrams` never alter the text
//! they are given; every function here does, and none of them hides it behind
//! a name that suggests otherwise.
//!
//! ```
//! use verbora_normalizers::{nfc, nfkc, remove_diacritics};
//!
//! assert_eq!(nfc("e\u{0301}"), "é");            // compose
//! assert_eq!(nfkc("ｶﾞ"), "ガ");                 // width- and mark-fold
//! assert_eq!(remove_diacritics("résumé"), "resume");
//! ```
//!
//! # The unit of text is the Unicode scalar value
//!
//! [UAX #15] defines all four forms as maps over sequences of code points, and
//! `Canonical_Combining_Class` is a per-code-point property, so this is the
//! unit the operation is written in rather than a choice this crate makes.
//! There is no smaller unit at which normalization is defined, and a grapheme
//! cluster would be strictly larger than the sequences being reordered.
//!
//! There is no UTF-16 anywhere in this crate — no public type, no internal
//! buffer, no index. The direct consequence is that **nothing here can emit
//! `U+FFFD` unless `U+FFFD` was in the input**: an astral scalar has no halves
//! to split, so `nfkc("😀々")` is `"😀々"`.
//!
//! # Every function returns `Cow`, and the borrow means something
//!
//! Each of the five returns [`Cow::Borrowed`](std::borrow::Cow::Borrowed) **if
//! and only if** the result is byte-identical to the input. That is a
//! guarantee, not a description of a fast path: branching on
//!
//! ```
//! # use std::borrow::Cow;
//! # use verbora_normalizers::nfc;
//! # let text = "already composed";
//! let key = match nfc(text) {
//!     Cow::Borrowed(s) => s.to_owned(), // the input was already in NFC
//!     Cow::Owned(s) => s,               // it was not, and `s` is the NFC form
//! };
//! # assert_eq!(key, "already composed");
//! ```
//!
//! is correct code, not an optimization that might stop working. The
//! quick-check properties of [UAX #15] §9 decide it without materialising the
//! result in the common case; where a quick check answers `Maybe` the
//! implementation compares and still returns `Borrowed` when it can.
//!
//! # The Unicode version is part of the contract
//!
//! Normalization is defined by UCD properties — `Decomposition_Mapping`,
//! `Canonical_Combining_Class`, `Composition_Exclusion` — so this crate cannot
//! promise results frozen for all time. A frozen table would be wrong for
//! every character encoded after the freeze, which is worse than a moving one.
//! Instead:
//!
//! * The Unicode version is whichever version `unicode-normalization` ships,
//!   pinned in `Cargo.lock`. At the version this crate is built against that is
//!   **Unicode 17.0.0**; [`unicode_version`] reports it at runtime, and
//!   `tests/conformance.rs` replays the UCD's own `NormalizationTest.txt` for
//!   it, so a dependency bump that moves any mapping fails loudly there.
//! * A UCD upgrade is a **semver-visible behaviour change** for this crate and
//!   is released as one.
//! * **Any structure that persists normalizer-derived keys must stamp the
//!   Unicode version and refuse to load across a change.** A search index, a
//!   trained model or an interned term table built before an upgrade does not
//!   match one built after it, and nothing else will tell you.
//!
//! Within one Unicode version the crate is fully deterministic: same input,
//! same output, on every platform and every build. There is no global mutable
//! state, no hash-order dependence, no interior mutability, and no floating
//! point.
//!
//! # Choosing the right API
//!
//! All five functions have the same shape — `&str` in, `Cow<str>` out — so the
//! choice is entirely about which rewrite you want.
//!
//! | Call | Use when | Loses | Allocates |
//! |---|---|---|---|
//! | [`nfc`] | you want one canonical spelling per abstract character, for storage, comparison or hashing. **The default for text you will show a human again.** | nothing: the result is canonically equivalent to the input, so only the spelling changes | one `String`, and only if the input was not already NFC |
//! | [`nfd`] | you are about to inspect or filter combining marks yourself | nothing, for the same reason | as `nfc` |
//! | [`nfkc`] | you want a *matching* key that ignores width, ligation and circling — halfwidth katakana, fullwidth Latin, `ﬁ`, `Ⅻ` | formatting distinctions, irreversibly | as `nfc` |
//! | [`nfkd`] | the same, and you will inspect the marks yourself | as `nfkc` | as `nfc` |
//! | [`remove_diacritics`] | you want accent-insensitive matching of Latin-script text | every combining mark, and for Thai and Devanagari that changes the word rather than de-accenting it — read the function's own table before using it on non-Latin text | nothing for ASCII; one `String` otherwise |
//! | `par_remove_diacritics_batch` (feature `parallel`) | many independent documents at once | the same as `remove_diacritics` | one `Vec` plus the per-input cost |
//!
//! A decision tree, for the common question "which one do I store?":
//!
//! ```text
//! Do you need the text back, readable, exactly as written?
//!  ├─ yes → nfc
//!  └─ no, it is a lookup key
//!      ├─ must "ｱ" match "ア" and "Ａ" match "A"?
//!      │   ├─ yes → nfkc
//!      │   └─ no  → nfc
//!      └─ must "resume" match "résumé"?
//!          └─ yes → remove_diacritics (after nfkc, if you also wanted that)
//! ```
//!
//! The forms compose in the obvious way and nothing here does it for you:
//! `remove_diacritics(&nfkc(text))` is the width-, ligature- and
//! accent-insensitive key, spelled out so the two rewrites are both visible at
//! the call site.
//!
//! There is deliberately no `par_nfc_batch` and no sibling for the other three
//! forms. They are adapters over `unicode-normalization` with no Verbora-side
//! per-item work to fan out, so `inputs.par_iter().map(nfc).collect()` at the
//! call site is the same code with one fewer name to learn.
//!
//! # Performance
//!
//! **Unmeasured.** Every function in this crate was reimplemented in the
//! Rust-native migration, and no benchmark has been run against the new code.
//! The allocation behaviour above is a property of the implementation and is
//! stated as such; no timing claim is made, and none should be inferred.
//!
//! [UAX #15]: https://www.unicode.org/reports/tr15/

#![cfg_attr(docsrs, feature(doc_cfg))]

mod diacritics;
mod forms;

#[cfg(feature = "parallel")]
pub use diacritics::par_remove_diacritics_batch;
pub use diacritics::remove_diacritics;
pub use forms::{nfc, nfd, nfkc, nfkd};

/// The Unicode version whose `Decomposition_Mapping`,
/// `Canonical_Combining_Class` and `Composition_Exclusion` assignments this
/// crate's results are computed from, as `(major, minor, update)`.
///
/// Stamp this into anything that persists normalizer-derived keys, and refuse
/// to load an artifact whose stamp differs: mappings move between Unicode
/// versions, so an index built under one and queried under another silently
/// mismatches rather than failing.
///
/// ```
/// // The version is a fact about the build, not a constant to assert against
/// // in application code — record it, compare it, do not hardcode it.
/// let (major, _minor, _update) = verbora_normalizers::unicode_version();
/// assert!(major >= 17);
/// ```
#[must_use]
pub fn unicode_version() -> (u64, u64, u64) {
    let (major, minor, update) = unicode_normalization::UNICODE_VERSION;
    // Widened so that this and `verbora_tokenizers::unicode_version` have the
    // same type: a consumer stamping both into one persisted artifact should
    // not have to reconcile two integer widths that mean the same thing.
    (u64::from(major), u64::from(minor), u64::from(update))
}
