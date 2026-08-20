//! Japanese kana romanized into modified-Hepburn romaji.
//!
//! ```
//! use verbora_transliterators::transliterate_ja;
//!
//! assert_eq!(transliterate_ja("とうきょう"), "tōkyō");
//! assert_eq!(transliterate_ja("ざっし"), "zasshi");
//! assert_eq!(transliterate_ja("ほんや"), "hon'ya");
//! ```
//!
//! # What this crate does, exactly
//!
//! It rewrites **kana**, mora by mora, and copies everything else through. It
//! is a *romanization*: a mapping from one script to Latin letters that
//! approximates how the text is pronounced. It is not a translation, not a
//! phonetic key, and not a normalizer — see `AGENTS.md` and the crate list
//! below for which Verbora crate answers which of those questions.
//!
//! It is also **grapheme-driven**: every decision is made from the kana on the
//! page and its immediate neighbour. It has no dictionary, no morphological
//! analyser and no notion of a word, and three consequences of that are
//! visible in the output and are part of the contract rather than defects to
//! be reported:
//!
//! * **Particles are romanized by their kana value.** `こんにちは` is
//!   `konnichiha`, not `konnichiwa`. The rule that spells the topic particle
//!   `は` as `wa` is syntactic — `は` is `ha` in `はな` — and nothing here
//!   knows what a particle is. The same goes for `へ` (`he`, never `e`).
//! * **Kanji are copied through.** `これは日本語のテストです。` is
//!   `koreha日本語notesutodesu。`. Reading kanji needs a dictionary; this
//!   crate has none, and inventing a reading would be worse than leaving the
//!   character alone.
//! * **`おう` is always long.** `とうきょう` is `tōkyō`, and so `おもう` is
//!   `omō` where a morphological analyser would say `omou`. ALA-LC resolves
//!   this by word element; a grapheme-driven romanizer cannot.
//!
//! # Where the readings come from
//!
//! **Modified Hepburn**, as codified in the *ALA-LC Romanization Tables:
//! Japanese* (American Library Association / Library of Congress), which
//! follows ANSI Z39.11-1972 and BS 4812:1972 — plus 内閣告示第二号
//! 「外来語の表記」 (Cabinet of Japan, Notification No. 2 of 1991) for the
//! extended syllables Japanese writes foreign sounds with (`ファ`, `ティ`,
//! `ヴァ`, `クォ`, …).
//!
//! Every mora, its reading and the citation for it are in `src/syllabary.rs`,
//! which is the crate's single source of truth: `build.rs` derives the
//! katakana half, the long-vowel forms and the lookup index from that one
//! file. Six characters take their reading from the Unicode Character Database
//! instead, because their character names *are* their readings — the four
//! `KATAKANA LETTER V*` (U+30F7..U+30FA) and the digraphs `ゟ` U+309F
//! `HIRAGANA DIGRAPH YORI` and `ヿ` U+30FF `KATAKANA DIGRAPH KOTO`.
//!
//! Verbora adds five entries of its own to that list, each the voiced
//! counterpart of a syllable the notification does list (`でぃ` beside `てぃ`,
//! and so on), and makes one decision the standards do not cover: `・` U+30FB
//! `KATAKANA MIDDLE DOT` romanizes as a single ASCII space. Both are argued
//! for in `src/syllabary.rs`.
//!
//! # The unit is the mora
//!
//! Not the byte, not the scalar value, not the grapheme cluster. A mora is
//! spelled in kana as **one or two Unicode scalar values**, optionally
//! followed by one more that lengthens it:
//!
//! | Spelling | Scalars | Romaji |
//! |---|---|---|
//! | `か` | 1 | `ka` |
//! | `きょ` | 2 (base + small `ょ`) | `kyo` |
//! | `かー` | 1 + prolonged sound mark | `kā` |
//! | `こう` | 1 + lengthening vowel kana | `kō` |
//!
//! The scalar value is the wrong unit here because `きょ` is one mora written
//! with two of them, and the grapheme cluster is the wrong unit because `かー`
//! is two clusters and one mora. [`Rewrites`] reports each mora's extent in
//! **bytes**, which is what splicing a `&str` needs.
//!
//! Three marks carry a mora but have no reading of their own, because what
//! they romanize as depends on their neighbour. They are resolved by the
//! scanner rather than by a table:
//!
//! | Mark | Rule | Example |
//! |---|---|---|
//! | sokuon `っ` `ッ` | doubles the following consonant; `t` before `ch` | `ざっし` → `zasshi`, `まっちゃ` → `matcha` |
//! | syllabic nasal `ん` `ン` | `m` before `b`/`m`/`p`, `n'` before a vowel or `y`, else `n` | `ばんび` → `bambi`, `ほんや` → `hon'ya` |
//! | prolonged sound mark `ー` | macron over the preceding vowel | `スーパー` → `sūpā` |
//!
//! Applied to the *romanization* rather than to the kana, the sokuon rule
//! reproduces the columns that look like exceptions when written in kana:
//! `っし` is `sshi` because `し` is `shi`, and `っふ` is `ffu` because `ふ` is
//! `fu`.
//!
//! # What is deliberately not romanized
//!
//! Twelve scalar values in the Hiragana and Katakana blocks are passed through
//! unchanged, and `every_kana_block_scalar_is_romanized_or_deliberately_not`
//! pins the list exactly:
//!
//! * **U+3040, U+3097, U+3098** — unassigned.
//! * **U+3099, U+309A** — combining voiced and semi-voiced sound marks. They
//!   are diacritics, not morae; [`transliterate_ja_normalized`] composes them
//!   onto the kana they belong to.
//! * **U+309B, U+309C** — the spacing forms of those two marks. Same reason,
//!   and the same function handles them.
//! * **U+309D, U+309E, U+30FD, U+30FE** — iteration marks. Expanding them is
//!   an orthographic rewrite with no Unicode definition, and it is not
//!   idempotent (`あ々々` gives a different answer applied twice), so Verbora
//!   ships none.
//! * **U+30A0 `゠` KATAKANA-HIRAGANA DOUBLE HYPHEN** — punctuation.
//!
//! A sokuon with no consonant after it, and a prolonged sound mark with no
//! vowel before it, romanize as **nothing**: they are modifiers with nothing
//! to modify. No romanization standard assigns either a segment of its own,
//! and leaving the kana in place would put a character in romanized text that
//! the caller asked to have romanized.
//!
//! # Lazy first
//!
//! [`Rewrites`] is the crate's single implementation of what romanization is —
//! a lazy iterator of [`Rewrite`]s, each naming a byte range of the input and
//! the `&'static str` that replaces it. [`transliterate_ja`],
//! [`transliterate_ja_into`] and [`transliterate_ja_normalized`] are all built
//! on it, so there is one description of the behaviour and no second copy to
//! drift. Nothing is allocated until a replacement is actually spliced, and
//! text that needs no change comes back as [`Cow::Borrowed`](std::borrow::Cow)
//! after a single vectorised scan.
//!
//! [`transliterate_ja`]'s own documentation carries the comparison table and
//! decision tree for choosing between the five call shapes.
//!
//! # Relationship to `verbora-normalizers`
//!
//! Halfwidth katakana is ignored completely, because no key of the syllabary
//! is a halfwidth character. Folding width — and folding the halfwidth voiced
//! sound mark onto the kana it belongs to — is Unicode compatibility
//! normalization, [`verbora_normalizers::nfkc`], not romanization.
//! [`transliterate_ja_normalized`] is that pairing, so this crate carries no
//! second copy of a width table.
//!
//! It is that pairing plus exactly one two-scalar re-spelling. The **spacing**
//! voiced sound marks U+309B `゛` and U+309C `゜` — the legacy Shift-JIS
//! spelling — carry the compatibility mappings `<compat> 0020 3099` and
//! `<compat> 0020 309A`, and that `U+0020` is a starter, so NFKC on its own
//! strands the mark on an invented space instead of composing it onto the
//! preceding kana. [`transliterate_ja_normalized`] re-spells those two scalars
//! as the bare combining marks first, which is the standard's own mapping
//! without the space and is what the halfwidth U+FF9E already decomposes to.
//! The derivation is on that function.

#![cfg_attr(doctest, doc = include_str!("../README.md"))]

mod romanize;
mod scan;
mod syllabary;

/// The generated lookup index. Written by `build.rs` from `src/syllabary.rs`.
mod tables {
    #![allow(missing_docs)]
    include!(concat!(env!("OUT_DIR"), "/index.rs"));
}

#[cfg(feature = "parallel")]
pub use romanize::par_transliterate_ja_batch;
pub use romanize::{
    Rewrites, transliterate_ja, transliterate_ja_into, transliterate_ja_normalized,
};
pub use scan::Rewrite;
