//! Text normalization and diacritic folding for Rust.
//!
//! A port of the reference `normalizers` module: five independent normalizers that share
//! nothing but a habit of doing surprising things to Unicode.
//!
//! | Rust | the reference | Job |
//! |---|---|---|
//! | [`normalize`] | `normalize(tokens)` (`normalizeTokens`) | expand English contractions |
//! | [`normalize_token`] | `normalize(string)` | the same, for the bare-string call |
//! | [`remove_diacritics`] | `removeDiacritics` | fold Latin diacritics to base letters |
//! | [`normalize_no`] | `normalizeNo` | fold Norwegian diacritics |
//! | [`normalize_sv`] | `normalizeSv.removeDiacritics` | fold Swedish diacritics |
//! | [`normalize_ja`] | `normalizeJa` | normalize Japanese widths, kana and symbols |
//! | [`ja::converters`] | `Converters` | the seventeen individual Japanese conversions |
//!
//! # Everything returns a [`Cow`](std::borrow::Cow)
//!
//! These functions are usually called on text that needs no change at all — a
//! Latin sentence handed to the katakana converter, an ASCII token handed to the
//! diacritic folder. Every single-string API therefore returns
//! `Cow::Borrowed` when it changed nothing, and allocates only at the first
//! replacement. The multi-stage pipelines ([`normalize_ja`],
//! [`ja::converters::hiragana_to_katakana`]) carry the borrow through every
//! stage rather than allocating once per stage as the reference does.
//!
//! # Deliberate divergences from the reference
//!
//! Three, each argued where it lives:
//!
//! * **`normalizeSv` is callable here.** In the reference it is the module *object*
//!   and calling it throws; see [`normalize_sv`].
//! * **`normalize(["constructor"])` does not panic.** The reference's conversion
//!   table is a plain object literal, so `"constructor"` and `"__proto__"` find
//!   `Object.prototype` members and throw `TypeError: ....split is not a
//!   function`. A Rust lookup has no prototype chain, so both come back as
//!   ordinary unmatched tokens. See [`normalize`].
//! * **`normalize_ja` cannot emit a lone surrogate.** Its first stage matches
//!   UTF-16 code units and can split a surrogate pair; the reference renders the
//!   result as an unpaired surrogate, which a Rust `String` cannot hold, so it
//!   becomes U+FFFD. See [`normalize_ja`].
//!
//! Everything else is byte-exact against `fixtures/normalizers.json`, which
//! records 127,902 calls into the real library, replayed by `tests/parity.rs`.
//!
//! # Generated data
//!
//! `src/ja/tables.rs` and `src/diacritics/table.rs` were machine-derived by
//! dumping the reference's own tables at runtime rather than transcribing them —
//! several of them do not exist in the reference source at all, being built at
//! load time by `flip()` and `merge()` whose collision rules are observable.
//! Derivation also re-proved,
//! on every run, the two properties the scanners depend on: that no key is a
//! proper prefix of a later key, and that the 86-pass diacritics algorithm is
//! equivalent to a single per-character lookup.

mod table;

pub mod diacritics;
pub mod english;
pub mod ja;
pub mod nordic;

#[cfg(feature = "parallel")]
pub use diacritics::par_remove_diacritics_batch;
pub use diacritics::remove_diacritics;
pub use english::{normalize, normalize_token};
pub use ja::normalize_ja;
pub use nordic::{normalize_no, normalize_sv};
