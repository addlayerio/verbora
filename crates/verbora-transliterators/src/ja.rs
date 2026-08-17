//! `TransliterateJa` — kana to modified-Hepburn romaji.
//!
//! Port of the reference `index`, a 584-line module that is
//! one exported function and three tables.
//!
//! # The pipeline
//!
//! Five phases, strictly ordered. Each one runs over the *whole* string before
//! the next begins, and several of them depend on that:
//!
//! | [`Phase`] | Reference | What it does |
//! |---|---|---|
//! | [`Phase::CompoundKana`] | `replace1(str)` | the 20 u/vu digraphs: `ウァ` -> `wa`, `ヴュ` -> `vyu`, `ウー` -> `ū` |
//! | [`Phase::SokuonAndN`] | 30 chained `.replace(/X(?=[…])/g, …)` | the geminate consonant and the two faces of `ン` |
//! | [`Phase::Kana`] | `replace2(str)` | the main 382-entry kana table |
//! | [`Phase::LongVowels`] | `replace3(str)` | long vowels and the small-vowel fallback, over the now-mixed latin+kana text |
//! | [`Phase::FinalSokuon`] | `.replace(/(ッ\|っ)\B/g, 't')` | whatever small tsu is left |
//!
//! Order is not a stylistic choice. `ハイジャッンプ` becomes `haijanmpu` only
//! because `ッ` before `ン` is rewritten before `ン` before a labial gets a look
//! at the same `ン`; swap those two and the word comes out `haijammpu`.
//! Similarly, [`Phase::LongVowels`] has to run after [`Phase::Kana`] because its
//! keys begin with the latin vowels [`Phase::Kana`] just produced — `カァ` is
//! `ka` + `ァ` before it is `kā`.
//!
//! # Iterator first
//!
//! [`Phase::rewrites`] is the single implementation of every phase's behaviour:
//! a lazy iterator of [`Rewrite`]s, each naming a byte range of the input and the
//! text that replaces it. [`Phase::apply`], [`transliterate`] and
//! [`transliterate_into`] are all built on it, so there is one description of
//! what this module does and no second copy to drift.
//!
//! ```
//! use verbora_transliterators::ja::{Phase, transliterate};
//!
//! assert_eq!(transliterate("こんにちは"), "konnichiha");
//!
//! // The phases are individually addressable and individually lazy.
//! let mid = Phase::CompoundKana.apply("ヴァイオリン");
//! assert_eq!(mid, "vaイオリン");
//!
//! let first = Phase::Kana.rewrites("イオリン").next().unwrap();
//! assert_eq!((first.from, first.to), ("イ", "i"));
//! ```
//!
//! # What it does not do
//!
//! Halfwidth katakana is left completely alone: `ｱｲｳｴｵ` comes back unchanged,
//! because no table key is a halfwidth character. That is not an oversight in the
//! reference so much as a division of labour — `stemmer_ja` and
//! `noun_inflector` both run `normalizeJa` first. [`transliterate_normalized`]
//! is that composition, spelled out.
//!
//! Iteration marks (`々`), kanji, Latin text and punctuation also pass through
//! untouched, and the module's own header lists iteration marks as a `@todo`.

use std::borrow::Cow;

pub use crate::scan::Rewrite;
use crate::scan::{apply, apply_into, map_cow};

mod tables;

/// The UTF-8 lead byte shared by every character in U+3000..U+3FFF.
///
/// Every key of every table, and every source character of both hand-written
/// scanners, lies in U+3040..U+30FF — machine-checked at derivation time, and
/// re-checked in
/// [`tests::every_key_is_reachable_only_through_the_kana_gate`]. A document with
/// no `0xE3` byte anywhere therefore cannot match anything, at any phase, and is
/// returned untouched after one vectorised scan.
///
/// The test is exact rather than heuristic: `0xE3` is a lead byte only — UTF-8
/// continuation bytes are `0x80..=0xBF` — so its absence really does prove the
/// absence of the whole block.
const KANA_LEAD_BYTE: u8 = 0xE3;

/// Whether any phase could possibly change `text`.
///
/// One pass, no decoding. `slice::contains` on `u8` compiles to a vectorised
/// scan, which is why this is worth doing once before five character-wise passes.
///
/// Applying the same idea *per character* — stepping over any byte that is
/// neither `0xE3` nor an ASCII key start, using only the lead byte, so kanji and
/// Latin never reach a `char` decode — was implemented and measured. It came out
/// **6% slower on kana-dense text and 12% slower on mixed text**: resuming the
/// scan needs `&text[i..]`, which performs a character-boundary check, and
/// paying that per candidate costs more than `char_indices` costs per character.
/// The scanners therefore decode every character and reject on the index.
#[inline]
fn may_contain_kana(text: &str) -> bool {
    text.as_bytes().contains(&KANA_LEAD_BYTE)
}

/// One stage of the transliteration pipeline.
///
/// Named after what it does rather than after the reference's `replace1` /
/// `replace2` / `replace3`, which say only what order they were written in. The
/// mapping is in the [module documentation](self).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Phase {
    /// The 20 two-kana digraphs built on `ウ` / `ヴ` (`replace1`).
    ///
    /// Runs first so that `ウァ` -> `wa` beats the `ウ` -> `u` of
    /// [`Phase::Kana`], and `ウー` -> `ū` beats `ウ` -> `u` followed by an
    /// untouched prolonged sound mark.
    CompoundKana,
    /// The 30 lookahead rules: `ッ`/`っ` before a consonant row, and `ン`/`ん`
    /// before a labial or a vowel.
    ///
    /// The reference writes these as 30 separate whole-string passes of
    /// `/X(?=[…])/g`. See [`Phase::rewrites`] for why one scan is equivalent.
    SokuonAndN,
    /// The main kana table: 191 katakana entries and their 191 hiragana mirrors
    /// (`replace2`).
    ///
    /// Also holds the module's one non-kana key, `・` KATAKANA MIDDLE DOT, which
    /// becomes a plain ASCII space — the hiragana half has no counterpart, so
    /// `TransliterateJa("・・")` is two spaces.
    Kana,
    /// Long vowels and the small-vowel fallback (`replace3`).
    ///
    /// Runs on the mixed latin+kana text the previous phase left behind: eleven
    /// of its 21 keys begin with a latin vowel. Only **lowercase** `a i u e o`
    /// participate, so `aー` becomes `ā` while `Aー` is left exactly as it is.
    LongVowels,
    /// The closing `/(ッ|っ)\B/g` pass, which turns any surviving small tsu into
    /// `t`.
    ///
    /// The condition is the reference's ASCII-only word boundary; see
    /// [`Phase::rewrites`].
    FinalSokuon,
}

impl Phase {
    /// Every phase, in the order [`transliterate`] runs them.
    pub const ALL: [Self; 5] = [
        Self::CompoundKana,
        Self::SokuonAndN,
        Self::Kana,
        Self::LongVowels,
        Self::FinalSokuon,
    ];

    /// The lazy primitive: every replacement this phase would make, in
    /// left-to-right order, without touching the input.
    ///
    /// Matches are non-overlapping and their output is never rescanned — the
    /// semantics of a `g`-flagged `String#replace`, which is what all five phases
    /// are built from.
    ///
    /// # The three table phases
    ///
    /// [`Phase::CompoundKana`], [`Phase::Kana`] and [`Phase::LongVowels`] are
    /// leftmost-longest scans over a static table. That is exactly what the
    /// reference's leftmost-*first* alternation computes, because in all three
    /// tables no key is a proper prefix of a *later* key — so whenever two keys
    /// match at one position, the longer one is the one listed first. The
    /// generator refuses to emit a table that breaks the invariant.
    ///
    /// # Why 30 lookahead passes become one scan
    ///
    /// The Rust `regex` crate has no lookahead, so [`Phase::SokuonAndN`] is a
    /// hand-written scan. Collapsing 30 ordered whole-string passes into one
    /// left-to-right pass is not valid in general — a pass can consume a
    /// character that a later pass was going to look ahead *at*, and then the
    /// later pass sees text the fused version never does. It is valid here, for
    /// two specific reasons:
    ///
    /// * every replacement is ASCII, and no ASCII character is a source or a
    ///   member of any lookahead class, so a rewritten position is inert for
    ///   every subsequent pass; and
    /// * the only source characters that also appear in some lookahead class are
    ///   `ン` and `ん`, and the rules that look ahead at them (`ッ`/`っ` before
    ///   the n-row) run *before* any rule that could rewrite them.
    ///
    /// The equivalence was re-proved at derivation time against the 30 real
    /// passes, over 120,000 random strings. Do not
    /// generalise the argument to other lookahead rewrites without redoing it.
    ///
    /// # Why the final pass is not `\B`
    ///
    /// The reference's `\w` is `[A-Za-z0-9_]` with no Unicode semantics. `ッ` is
    /// therefore a non-word character, so `\B` after it holds exactly when the
    /// next code unit is absent or is itself non-word:
    ///
    /// ```text
    /// "ッ漢" -> "t漢"     "ッ." -> "t."      "ッ " -> "t "     "ッ" -> "t"
    /// "ッ_"  -> "ッ_"     "ッ1" -> "ッ1"     "ッA" -> "ッA"
    /// ```
    ///
    /// Rust's `\B` is Unicode-aware: it considers `ッ` and `漢` both to be word
    /// characters, so `ッ漢` would have no boundary between them and the small
    /// tsu would be *kept* — the exact opposite of the reference on the exact
    /// input this crate is for. The port tests the next byte against the ASCII
    /// word set directly, which needs no regex engine and cannot drift.
    #[must_use]
    pub fn rewrites(self, text: &str) -> Rewrites<'_> {
        Rewrites {
            text,
            // The whole-input gate, applied once per phase rather than per
            // character. When it fires the iterator is empty by construction.
            pos: if may_contain_kana(text) {
                0
            } else {
                text.len()
            },
            phase: self,
        }
    }

    /// Runs this phase, borrowing when it changed nothing.
    ///
    /// ```
    /// # use verbora_transliterators::ja::Phase;
    /// assert_eq!(Phase::Kana.apply("カタカナ"), "katakana");
    /// assert_eq!(Phase::Kana.apply("hello"), "hello");
    /// ```
    #[must_use]
    pub fn apply(self, text: &str) -> Cow<'_, str> {
        apply(text, self.rewrites(text))
    }

    /// Runs this phase, appending to `out` instead of allocating.
    ///
    /// `out` is appended to, not cleared, so a caller can build one buffer from
    /// several inputs.
    pub fn apply_into(self, text: &str, out: &mut String) {
        apply_into(text, self.rewrites(text), out);
    }
}

/// The lazy stream of replacements one [`Phase`] would make.
///
/// Created by [`Phase::rewrites`]. Yields ascending, non-overlapping
/// [`Rewrite`]s and borrows the input for as long as it lives.
#[derive(Debug, Clone)]
pub struct Rewrites<'a> {
    /// The text being scanned.
    text: &'a str,
    /// Byte offset to resume from; `text.len()` when the scan is finished.
    pos: usize,
    /// Which phase's rules to apply.
    phase: Phase,
}

impl<'a> Rewrites<'a> {
    /// The next `ッ`/`っ`/`ン`/`ん` whose following character triggers a rule.
    ///
    /// Only the source character is consumed: The reference's lookahead is
    /// zero-width, so the character that *caused* the rewrite stays in the
    /// output and is itself re-examined on the next step. That is what makes
    /// `ッンプ` -> `nmプ` rather than `nプ`.
    fn next_lookahead(&self, from: usize) -> Option<(usize, usize, &'static str)> {
        for (offset, c) in self.text[from..].char_indices() {
            // Four source characters, so a `matches!` beats any lookup: it
            // rejects every other character in at most four compares and without
            // touching memory. It duplicates the table's key set, so
            // `the_lookahead_fast_path_matches_the_generated_table` asserts the
            // two agree — a regenerated table with a different source set would
            // otherwise be silently ignored here.
            if !matches!(c, 'ッ' | 'っ' | 'ン' | 'ん') {
                continue;
            }
            let start = from + offset;
            let after = start + c.len_utf8();
            let Ok(i) = tables::LOOKAHEAD.binary_search_by_key(&c, |&(k, _)| k) else {
                continue;
            };
            let next = self.text[after..].chars().next()?;
            let class = tables::LOOKAHEAD[i].1;
            if let Ok(j) = class.binary_search_by_key(&next, |&(k, _)| k) {
                return Some((start, after, class[j].1));
            }
        }
        None
    }

    /// The next small tsu that the ASCII-only `\B` leaves exposed.
    fn next_final_sokuon(&self, from: usize) -> Option<(usize, usize, &'static str)> {
        let (chars, to) = tables::FINAL_SOKUON;
        for (offset, c) in self.text[from..].char_indices() {
            if !chars.contains(&c) {
                continue;
            }
            let start = from + offset;
            let after = start + c.len_utf8();
            // `\B` holds when the next code unit is absent or not `[A-Za-z0-9_]`.
            // Every ASCII word character is a single byte and every non-ASCII
            // byte is >= 0x80, so one byte test decides it exactly.
            let next_is_word = self
                .text
                .as_bytes()
                .get(after)
                .is_some_and(|&b| b.is_ascii_alphanumeric() || b == b'_');
            if !next_is_word {
                return Some((start, after, to));
            }
        }
        None
    }
}

impl<'a> Iterator for Rewrites<'a> {
    type Item = Rewrite<'a>;

    fn next(&mut self) -> Option<Rewrite<'a>> {
        if self.pos >= self.text.len() {
            return None;
        }
        // Exhaustive on purpose: a sixth phase must be given a scanner here
        // rather than falling into a catch-all that silently does the wrong one.
        let (start, end, to) = match self.phase {
            Phase::CompoundKana => tables::TABLE1.next_match(self.text, self.pos)?,
            Phase::SokuonAndN => self.next_lookahead(self.pos)?,
            Phase::Kana => tables::TABLE2.next_match(self.text, self.pos)?,
            Phase::LongVowels => tables::TABLE3.next_match(self.text, self.pos)?,
            Phase::FinalSokuon => self.next_final_sokuon(self.pos)?,
        };
        self.pos = end;
        Some(Rewrite {
            start,
            end,
            from: &self.text[start..end],
            to,
        })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        // Every match consumes at least one byte, so the remaining input bounds
        // the number of rewrites. Nothing is guaranteed to match, so the lower
        // bound is zero.
        (0, Some(self.text.len().saturating_sub(self.pos)))
    }
}

/// Transliterates Japanese kana into modified-Hepburn romaji.
///
/// The exact behaviour of the reference `TransliterateJa`, including the parts that
/// are arguably wrong. Returns [`Cow::Borrowed`] when nothing changed, which
/// covers Latin text, kanji and halfwidth katakana.
///
/// ```
/// # use verbora_transliterators::transliterate_ja;
/// assert_eq!(transliterate_ja("あいうえお かきくけこ"), "aiueo kakikukeko");
/// assert_eq!(transliterate_ja("まっか ざっし たった はっぱ"), "makka zasshi tatta happa");
/// assert_eq!(transliterate_ja("まんと ばんび ほんや"), "manto bambi hon'ya");
/// assert_eq!(transliterate_ja("アヴァンギャルド"), "avangyarudo");
/// assert_eq!(transliterate_ja("ボージョレー・ヌーヴォー"), "bōjorē nūvō");
///
/// // Untouched: kanji, Latin, halfwidth katakana, iteration marks.
/// assert_eq!(transliterate_ja("abc ABC 漢字 (.)"), "abc ABC 漢字 (.)");
/// assert_eq!(transliterate_ja("ｱｲｳｴｵ"), "ｱｲｳｴｵ");
/// ```
///
/// # Divergence: this cannot throw
///
/// The reference does no argument checking, so `TransliterateJa(null)` and
/// `TransliterateJa(42)` both raise a `TypeError` from inside `String#replace`.
/// A `&str` parameter makes those calls unrepresentable. The thrown messages are
/// still recorded in `fixtures/transliterators.json` and asserted by
/// `tests/parity.rs`, so the claim that this is the *whole* difference stays
/// checkable.
#[must_use]
pub fn transliterate(text: &str) -> Cow<'_, str> {
    // No character in U+3000..U+3FFF means no phase can match; skip all five.
    if !may_contain_kana(text) {
        return Cow::Borrowed(text);
    }
    let mut out = Cow::Borrowed(text);
    for phase in Phase::ALL {
        out = map_cow(out, |s| phase.apply(s));
    }
    out
}

/// [`transliterate`], appending to a caller-owned buffer.
///
/// Useful when transliterating many strings into one document: `out` is appended
/// to rather than cleared, and no intermediate `String` survives the call.
///
/// ```
/// # use verbora_transliterators::ja::transliterate_into;
/// let mut buf = String::new();
/// for word in ["こんにちは", " ", "せかい"] {
///     transliterate_into(word, &mut buf);
/// }
/// assert_eq!(buf, "konnichiha sekai");
/// ```
pub fn transliterate_into(text: &str, out: &mut String) {
    match transliterate(text) {
        Cow::Borrowed(s) => out.push_str(s),
        // Taking the buffer the pipeline already built saves a copy — but only
        // when there is nothing to lose by dropping `out`. An empty `String` that
        // a caller deliberately reserved capacity in must be kept and appended
        // to, or `transliterate_into` would quietly undo their reservation on
        // every first call after a `clear()`.
        Cow::Owned(s) if out.is_empty() && out.capacity() == 0 => *out = s,
        Cow::Owned(s) => out.push_str(&s),
    }
}

/// [`transliterate`], fanned out across a `rayon` thread pool. Requires the
/// `parallel` feature.
///
/// # Why this exists
///
/// `transliterate` is a pure function of one `&str` with no shared state — the
/// whole-input kana gate and the five-phase pipeline both read only their
/// argument — so transliterating many independent documents is embarrassingly
/// parallel with zero coordination cost between them. This function is
/// exactly `inputs.par_iter().map(transliterate).collect()` — a thin fan-out
/// over the existing sequential primitive, not a second implementation of it.
/// The gate, the five ordered phases and the `Cow`-borrowing behaviour inside
/// `transliterate` are all untouched; if you need a different shape in
/// parallel (a shared output buffer built with [`transliterate_into`], for
/// instance), apply the same `par_iter().map(...)` pattern at your own call
/// site (see `site/performance/parallelism.md`).
///
/// # When to reach for it vs. the sequential loop
///
/// `transliterate` is already fast per call — this crate's own `transliterate`
/// benchmark (`cargo bench -p verbora-transliterators -- transliterate/kana`)
/// measures roughly 81 µs for a ~20 KB all-kana document, and under 100 ns for
/// the rejection path on Latin text, while a `rayon` task costs on the order
/// of a microsecond to schedule (`site/performance/parallelism.md`). This
/// crate's own `par_transliterate_ja_batch` Criterion group
/// (`benches/transliterators.rs`) measures the consequence directly, at a
/// fixed ~23.5 KB document per call: even a 4-document batch is already
/// **~4x faster** in parallel, and 32- and 256-document batches are
/// **6–10x faster**. A plain `inputs.iter().map(transliterate).collect()`
/// loop still wins for a handful of short (non-document-scale) inputs; reach
/// for this once inputs are document-scale (multi-kilobyte) and there is more
/// than a handful of them, and measure your own workload rather than
/// assuming the win.
///
/// # Allocation behaviour
///
/// One `Vec<Cow<str>>` sized to `inputs.len()` for the output, plus whatever
/// `transliterate` itself allocates per input — nothing for text the gate
/// rejects outright, or that no phase ends up changing (`Cow::Borrowed`), and
/// otherwise one `String` per phase that actually rewrites something (see
/// `transliterate`'s own doc comment for the phase-by-phase allocation
/// shape). No additional buffering, no locking, no per-call thread-pool
/// construction — this uses whichever global `rayon` pool is already
/// installed (or `rayon`'s default one), so pool configuration remains the
/// caller's responsibility, not this crate's.
///
/// # Order and errors
///
/// Output order matches input order — `results[i]` is
/// `transliterate(inputs[i])` — via `rayon`'s order-preserving `map` +
/// `collect`. `transliterate` never errors, so there is no error shape to
/// preserve.
///
/// # Examples
///
/// ```
/// use verbora_transliterators::ja::par_transliterate_ja_batch;
///
/// let inputs = ["あいうえお", "ざっし", "plain ascii"];
/// let got = par_transliterate_ja_batch(&inputs);
/// assert_eq!(got, ["aiueo", "zasshi", "plain ascii"]);
/// ```
#[cfg(feature = "parallel")]
#[must_use]
pub fn par_transliterate_ja_batch<'a>(inputs: &[&'a str]) -> Vec<Cow<'a, str>> {
    use rayon::prelude::*;
    inputs.par_iter().map(|s| transliterate(s)).collect()
}

/// [`normalize_ja`](verbora_normalizers::normalize_ja) followed by
/// [`transliterate`] — the composition the reference's own callers use.
///
/// `TransliterateJa` has no halfwidth-katakana entries, so `ｱｲｳｴｵ` survives it
/// unchanged; both `stemmer_ja` and `inflectors/ja/noun_inflector`
/// therefore normalize first. Nothing in the reference exports that pairing,
/// which is why it is spelled out here rather than left to every caller to
/// rediscover.
///
/// ```
/// # use verbora_transliterators::ja::{transliterate, transliterate_normalized};
/// assert_eq!(transliterate("ｱｲｳｴｵ"), "ｱｲｳｴｵ");
/// assert_eq!(transliterate_normalized("ｱｲｳｴｵ"), "aiueo");
/// // Fullwidth Latin and composed voiced marks come along too.
/// assert_eq!(transliterate_normalized("ｶﾞｯｷ"), "gakki");
/// ```
#[must_use]
pub fn transliterate_normalized(text: &str) -> Cow<'_, str> {
    map_cow(verbora_normalizers::normalize_ja(text), transliterate)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every key of every table, read back out of the generated index, plus the
    /// source characters of the two hand-written scanners.
    fn all_keys() -> Vec<String> {
        let mut out = Vec::new();
        for table in [&tables::TABLE1, &tables::TABLE2, &tables::TABLE3] {
            out.extend(table.keys());
        }
        out.extend(tables::LOOKAHEAD.iter().map(|&(k, _)| k.to_string()));
        out.extend(tables::FINAL_SOKUON.0.iter().map(char::to_string));
        out
    }

    #[test]
    fn every_key_is_reachable_only_through_the_kana_gate() {
        // The generator asserts that every key contains a U+3040..U+30FF
        // character. This asserts the consequence the code actually relies on:
        // such a character always encodes with lead byte 0xE3, so a string with
        // no 0xE3 byte in it cannot match any key of any phase.
        //
        // Table 3 is why this is a per-key test and not a per-character one:
        // eleven of its keys *begin* with an ASCII vowel, and only the kana
        // second character keeps them out of reach of plain Latin text.
        for key in all_keys() {
            let gated = key.chars().any(|c| {
                let mut buf = [0u8; 4];
                ('\u{3040}'..='\u{30FF}').contains(&c)
                    && c.encode_utf8(&mut buf).as_bytes()[0] == KANA_LEAD_BYTE
            });
            assert!(gated, "key {key:?} is reachable without a 0xE3 byte");
        }
    }

    #[test]
    fn the_generated_tables_still_hold_what_the_reference_does() {
        // Spot checks against `transliterators/ja/index`, so a regenerated
        // table that lost or gained entries fails here rather than only in the
        // 143,060-case fixture replay.
        assert_eq!(tables::TABLE1.keys().len(), 20);
        assert_eq!(tables::TABLE2.keys().len(), 382);
        assert_eq!(tables::TABLE3.keys().len(), 21);
        assert_eq!(tables::LOOKAHEAD.len(), 4, "four source characters");
        assert_eq!(
            tables::LOOKAHEAD
                .iter()
                .map(|(_, class)| class.len())
                .sum::<usize>(),
            148,
            "the 30 rules flatten to 148 (source, next) pairs"
        );

        // The two deliberate holes: `ジ` is not in the ッ -> z class and `フ` is
        // not in the ッ -> h class. Transcribing "the whole ざ row" or "the whole
        // は row" breaks `ざっし` and `バッファ`.
        let katakana_tsu = tables::LOOKAHEAD
            .iter()
            .find(|(c, _)| *c == 'ッ')
            .expect("ッ rules")
            .1;
        let rule = |c: char| {
            katakana_tsu
                .iter()
                .find(|(k, _)| *k == c)
                .map(|&(_, v)| v)
                .unwrap_or("")
        };
        assert_eq!(rule('ズ'), "z");
        assert_eq!(rule('ジ'), "j");
        assert_eq!(rule('ハ'), "h");
        assert_eq!(rule('フ'), "f");
        assert_eq!(transliterate("ざっし"), "zasshi");
        assert_eq!(transliterate("バッファ"), "baffa");
    }

    #[test]
    fn the_gate_rejects_only_what_it_should() {
        assert!(!may_contain_kana("plain ascii"));
        assert!(!may_contain_kana("café résumé"));
        assert!(!may_contain_kana("Москва"));
        assert!(!may_contain_kana("😀"));
        // Halfwidth katakana is U+FF61.., lead byte 0xEF: gated out, and indeed
        // the transliterator leaves it alone.
        assert!(!may_contain_kana("ｱｲｳ"));
        assert!(may_contain_kana("あ"));
        assert!(may_contain_kana("ア"));
        assert!(may_contain_kana("・"));
        // A false positive is allowed (the gate is a superset test) but must not
        // change the answer.
        assert!(may_contain_kana("々"));
        assert_eq!(transliterate("々"), "々");
    }

    #[test]
    fn phases_compose_into_the_pipeline() {
        for input in ["ハイジャッンプ", "ボージョレー・ヌーヴォー", "ああっ", ""]
        {
            let mut staged = Cow::Borrowed(input);
            for phase in Phase::ALL {
                staged = map_cow(staged, |s| phase.apply(s));
            }
            assert_eq!(staged, transliterate(input), "input {input:?}");
        }
    }

    #[test]
    fn rewrites_report_what_they_replaced() {
        let hits: Vec<_> = Phase::Kana.rewrites("カナ").collect();
        assert_eq!(hits.len(), 2);
        assert_eq!(
            (hits[0].start, hits[0].end, hits[0].from, hits[0].to),
            (0, 3, "カ", "ka")
        );
        assert_eq!(
            (hits[1].start, hits[1].end, hits[1].from, hits[1].to),
            (3, 6, "ナ", "na")
        );

        // The lookahead phase consumes only its source character.
        let hits: Vec<_> = Phase::SokuonAndN.rewrites("ッカ").collect();
        assert_eq!(hits.len(), 1);
        assert_eq!(
            (hits[0].start, hits[0].end, hits[0].from, hits[0].to),
            (0, 3, "ッ", "k")
        );
    }

    #[test]
    fn size_hint_bounds_the_stream() {
        let it = Phase::Kana.rewrites("カタカナ");
        assert_eq!(it.size_hint(), (0, Some(12)));
        assert_eq!(it.count(), 4);
        // The gate makes the iterator empty without scanning.
        assert_eq!(Phase::Kana.rewrites("abc").size_hint(), (0, Some(0)));
    }

    #[test]
    fn empty_and_single_character_inputs() {
        assert_eq!(transliterate(""), "");
        assert!(matches!(transliterate(""), Cow::Borrowed("")));
        assert_eq!(transliterate("あ"), "a");
        assert_eq!(transliterate("ア"), "a");
        assert_eq!(transliterate("ッ"), "t");
        assert_eq!(transliterate("ン"), "n");
        assert_eq!(transliterate("ー"), "ー");
    }

    #[test]
    fn scripts_the_transliterator_does_not_touch() {
        for s in [
            "A",
            "Z",
            "ABC",
            "0123456789",
            "!?.,;:'\"",
            "café",
            "naïve",
            "ÜBER",
            "Москва",
            "Ελλάδα",
            "שלום",
            "العربية",
            "한국어",
            "中文测试",
            "漢字",
            "😀",
            "a😀b",
            "𝕳𝖊𝖑𝖑𝖔",
            "ｱｲｳｴｵ",
            "ｶﾞｷﾞ",
        ] {
            assert!(
                matches!(transliterate(s), Cow::Borrowed(_)),
                "{s:?} should be returned untouched"
            );
            assert_eq!(transliterate(s), s);
        }
    }

    #[test]
    fn uppercase_latin_is_excluded_from_the_long_vowel_table() {
        // Table 3 keys use lowercase vowels only; a port that case-folded first
        // would turn "Aー" into "Ā".
        assert_eq!(transliterate("aー"), "ā");
        assert_eq!(transliterate("Aー"), "Aー");
        assert_eq!(transliterate("iー"), "ī");
        assert_eq!(transliterate("Iー"), "Iー");
    }

    #[test]
    fn the_ascii_word_boundary_decides_the_final_small_tsu() {
        for (input, want) in [
            ("ッ", "t"),
            ("っ", "t"),
            ("ッ ", "t "),
            ("ッ漢", "t漢"),
            ("ッ.", "t."),
            ("ッ😀", "t😀"),
            ("ッ_", "ッ_"),
            ("ッ1", "ッ1"),
            ("ッA", "ッA"),
            ("ッz", "ッz"),
            ("ッッ", "tt"),
            ("ああっ", "aat"),
            ("アアッ", "aat"),
        ] {
            assert_eq!(transliterate(input), want, "input {input:?}");
        }
    }

    #[test]
    fn the_lookahead_fast_path_matches_the_generated_table() {
        // `next_lookahead` gates on a literal four-character `matches!`, which is
        // the one place in this crate where table data is written out by hand.
        // Pin it to the generated table so the two cannot drift apart.
        let from_table: Vec<char> = tables::LOOKAHEAD.iter().map(|&(c, _)| c).collect();
        assert_eq!(from_table, ['っ', 'ん', 'ッ', 'ン']);
        for c in from_table {
            assert!(matches!(c, 'ッ' | 'っ' | 'ン' | 'ん'), "{c:?} is gated out");
        }
    }

    #[test]
    fn kana_mixed_with_everything_else() {
        // Astral characters, punctuation, digits and text in other scripts, all
        // adjacent to kana that IS rewritten — the cases where a port that
        // indexes wrongly would corrupt the surrounding text rather than merely
        // leaving it alone. Recorded from the reference.
        for (input, want) in [
            ("😀あ😀", "😀a😀"),
            ("a😀ッ😀b", "a😀t😀b"),
            ("𝕳ッ𝕳", "𝕳t𝕳"),
            ("ッ😀ッ", "t😀t"),
            ("カ😀ー", "ka😀ー"),
            ("・😀・", " 😀 "),
            ("あ!い?う。", "a!i?u。"),
            // Digits are ASCII word characters, so they suppress the final rule.
            ("ッ12ン34", "ッ12n34"),
            ("か3っ4", "ka3っ4"),
            ("abcあいう123", "abcaiu123"),
            ("漢っか", "漢kka"),
            // Halfwidth marks and a zero-width no-break space are inert.
            ("ﾞあ", "ﾞa"),
            ("ンﾞ", "nﾞ"),
            ("あ\u{FEFF}ー", "a\u{FEFF}ー"),
            ("ｱあ", "ｱa"),
        ] {
            assert_eq!(transliterate(input), want, "input {input:?}");
        }
    }

    #[test]
    fn the_middle_dot_is_the_one_non_kana_key() {
        assert_eq!(transliterate("・"), " ");
        assert_eq!(transliterate("・・"), "  ");
        assert_eq!(transliterate("あ・い"), "a i");
    }

    #[test]
    fn very_long_input() {
        let input = "とうきょうとっきょきょかきょく".repeat(2_000);
        let want = "tōkyōtokkyokyokakyoku".repeat(2_000);
        assert_eq!(transliterate(&input), want);

        // And the rejection path on a document-sized Latin string.
        let latin = "the quick brown fox ".repeat(5_000);
        assert!(matches!(transliterate(&latin), Cow::Borrowed(_)));
    }

    #[test]
    fn transliterate_into_appends() {
        let mut buf = String::from("[");
        transliterate_into("カナ", &mut buf);
        transliterate_into("]", &mut buf);
        assert_eq!(buf, "[kana]");

        // A fresh buffer takes ownership of the pipeline's string outright.
        let mut fresh = String::new();
        transliterate_into("カナ", &mut fresh);
        assert_eq!(fresh, "kana");

        // But a buffer the caller reserved capacity in keeps its allocation,
        // even though it is empty at the moment of the call.
        let mut reserved = String::with_capacity(4096);
        let addr = reserved.as_ptr();
        transliterate_into("カナ", &mut reserved);
        assert_eq!(reserved, "kana");
        assert_eq!(reserved.capacity(), 4096);
        assert!(std::ptr::eq(reserved.as_ptr(), addr));
    }

    /// Sequential-vs-parallel parity: `par_transliterate_ja_batch` must return
    /// exactly what a sequential `.iter().map(transliterate)` loop returns,
    /// element for element, for every input this module's own sequential
    /// tests already exercise above — not a fresh set of edge cases.
    #[cfg(feature = "parallel")]
    mod parallel_parity {
        use super::*;

        /// Runs both paths over `inputs` and asserts they agree, item by item.
        fn assert_parity(inputs: &[&str]) {
            let sequential: Vec<Cow<'_, str>> = inputs.iter().map(|s| transliterate(s)).collect();
            let parallel = par_transliterate_ja_batch(inputs);
            assert_eq!(
                parallel,
                sequential,
                "batch of {} inputs diverged from the sequential loop",
                inputs.len()
            );
        }

        #[test]
        fn empty_input_produces_an_empty_output() {
            assert_parity(&[]);
        }

        #[test]
        fn one_item() {
            assert_parity(&["あいうえお"]);
            assert_parity(&[""]);
            assert_parity(&["plain ascii, no kana at all"]);
        }

        #[test]
        fn many_items_preserve_order() {
            // The exact inputs `transliterate`'s own tests assert by value
            // above: plain kana, the deliberate table holes (ざっし,
            // バッファ), long vowels, the ASCII-word-boundary small-tsu
            // cases, the middle dot, and text from scripts the gate must
            // reject — cycled well past any plausible rayon task count.
            let base = [
                "あいうえお かきくけこ",
                "まっか ざっし たった はっぱ",
                "まんと ばんび ほんや",
                "アヴァンギャルド",
                "ボージョレー・ヌーヴォー",
                "バッファ",
                "ハイジャッンプ",
                "aー",
                "Aー",
                "ッ",
                "ッ漢",
                "ッ_",
                "・・",
                "abc ABC 漢字 (.)",
                "ｱｲｳｴｵ",
                "café naïve ÜBER",
                "Москва",
                "😀",
                "",
            ];
            let inputs: Vec<&str> = base.iter().copied().cycle().take(500).collect();
            assert_parity(&inputs);
        }

        #[test]
        fn unicode_and_pathological_inputs() {
            // The 2,000-repeat kana document and the 5,000-repeat Latin
            // rejection case `transliterate`'s own `very_long_input` test
            // exercises, alongside the exotic mixed-script cases from
            // `kana_mixed_with_everything_else`.
            let long_kana = "とうきょうとっきょきょかきょく".repeat(2_000);
            let long_latin = "the quick brown fox jumps over the lazy dog ".repeat(5_000);
            let mixed = [
                "😀あ😀",
                "a😀ッ😀b",
                "𝕳ッ𝕳",
                "カ😀ー",
                "・😀・",
                "あ!い?う。",
                "漢っか",
                "ﾞあ",
                "あ\u{FEFF}ー",
            ];
            let mut inputs: Vec<&str> = vec![long_kana.as_str(), long_latin.as_str()];
            inputs.extend(mixed);
            assert_parity(&inputs);
        }
    }

    #[test]
    fn normalized_composition_reaches_halfwidth_kana() {
        assert_eq!(transliterate_normalized("ｱｲｳｴｵ"), "aiueo");
        assert_eq!(transliterate_normalized("ｶﾞｯｷ"), "gakki");
        // Unchanged input still borrows all the way through.
        assert!(matches!(
            transliterate_normalized("abc"),
            Cow::Borrowed("abc")
        ));
    }
}
