//! Kana-to-romaji transliteration for Rust.
//!
//! A port of the reference `transliterators` module, which has exactly one export:
//! `TransliterateJa`, kana to modified-Hepburn romaji.
//!
//! ```
//! use verbora_transliterators::transliterate_ja;
//!
//! assert_eq!(transliterate_ja("とうきょう"), "tōkyō");
//! assert_eq!(transliterate_ja("ザッシ"), "zasshi");
//! assert_eq!(transliterate_ja("ほんや"), "hon'ya");
//! ```
//!
//! # What makes this hard to port
//!
//! The function is 584 lines of table plus twelve lines of pipeline, and almost
//! every line of the pipeline is a trap:
//!
//! * **Thirty of its rules use lookahead**, `/X(?=[…])/g`, which the Rust `regex`
//!   crate cannot express at all. They are a hand-written scan here, and the
//!   argument for why 30 ordered passes fuse into one is in
//!   [`ja::Phase::rewrites`] — with a generator that re-proves it against the
//!   real passes on every run.
//! * **The closing pass keys on `\B`**, and the reference's `\w` is ASCII-only.
//!   Rust's `\B` is Unicode-aware and gets `ッ漢` exactly backwards.
//! * **The phases are order-dependent.** `ハイジャッンプ` is `haijanmpu` only
//!   because the small-tsu rules run before the `ン` rules, and `カァ` is `kā`
//!   only because the long-vowel table runs after the kana table has produced the
//!   `a` it keys on.
//! * **The tables have deliberate holes.** `ジ` is excluded from the `ッ` -> `z`
//!   class and gets its own `j` rule; `フ` is excluded from `ッ` -> `h` and gets
//!   `f`. Writing out "the whole ざ row" breaks `ざっし` and `バッファ`.
//! * **`・` KATAKANA MIDDLE DOT maps to an ASCII space**, and only in the
//!   katakana half of the table.
//!
//! None of the tables are transcribed. They were machine-derived: the reference
//! module was loaded, what it actually built was dumped, the 30 lookahead rules
//! were parsed out of the source text, and three properties were re-proved
//! before anything was emitted: the prefix invariant that makes leftmost-longest
//! matching
//! equal to the reference's leftmost-first alternation, the fusion of the 30 passes,
//! and the equivalence of the whole model to `TransliterateJa` over 160,401
//! inputs.
//!
//! # Iterator first
//!
//! [`ja::Phase::rewrites`] is a lazy iterator of [`ja::Rewrite`]s and is the only
//! implementation of any phase's behaviour; [`ja::Phase::apply`],
//! [`transliterate_ja`] and [`ja::transliterate_into`] are all built on top of
//! it. Nothing is allocated until a replacement is actually found, and text that
//! needs no change is returned as [`Cow::Borrowed`](std::borrow::Cow) after a
//! single vectorised scan.
//!
//! # Deliberate divergences from the reference
//!
//! One, and it is a consequence of the type system:
//!
//! * **This cannot throw.** `TransliterateJa(null)` and `TransliterateJa(42)`
//!   raise a `TypeError` from inside `String#replace`; a `&str` parameter makes
//!   those calls unrepresentable. The thrown messages are recorded in
//!   `fixtures/transliterators.json` and asserted by `tests/parity.rs`, so the
//!   claim that this is the only difference is checked rather than asserted.
//!
//! Everything else is byte-exact against that fixture, which records 143,060
//! calls into the real library — including every ordered pair of kana, which is
//! where leftmost-longest matching and all 30 lookahead rules are decided.
//!
//! # Relationship to `verbora-normalizers`
//!
//! `TransliterateJa` ignores halfwidth katakana completely, because none of its
//! keys are halfwidth; the reference's own Japanese stemmer and noun inflector
//! run `normalizeJa` first. [`ja::transliterate_normalized`] is that pairing,
//! built on the parity-verified `verbora-normalizers` rather than on a second
//! copy of its tables.

mod scan;

pub mod ja;

#[cfg(feature = "parallel")]
pub use ja::par_transliterate_ja_batch;
pub use ja::{Phase, Rewrite, Rewrites, transliterate as transliterate_ja, transliterate_into};
