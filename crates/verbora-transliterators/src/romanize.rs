//! The scanner and the four public entry points.

use std::borrow::Cow;

use crate::scan::{Mora, Rewrite, apply, apply_into, longest_at, map_cow};
use crate::syllabary::{NASAL, PROLONGED_SOUND_MARK, SOKUON};

/// The UTF-8 lead byte shared by every character in U+3000..U+3FFF.
///
/// Every key of the generated index, and every mark the scanner handles
/// itself, lies in U+3041..U+30FF — `build.rs` refuses to emit anything else,
/// and `every_key_is_reachable_only_through_the_kana_gate` re-checks the
/// consequence. A document with no `0xE3` byte anywhere therefore cannot match
/// anything and is returned untouched after one vectorised scan.
///
/// The test is exact rather than heuristic: `0xE3` is a lead byte only — UTF-8
/// continuation bytes are `0x80..=0xBF` — so its absence really does prove the
/// absence of the whole block.
const KANA_LEAD_BYTE: u8 = 0xE3;

/// Whether the scanner could possibly change `text`.
///
/// One pass, no decoding. `slice::contains` on `u8` compiles to a vectorised
/// scan, which is why it is worth doing once before a character-wise pass.
#[inline]
fn may_contain_kana(text: &str) -> bool {
    text.as_bytes().contains(&KANA_LEAD_BYTE)
}

/// The romanization of the mora at byte offset `at`, for the two rules that
/// need to look at their neighbour.
///
/// The syllabic nasal is not an index key — its own romanization depends on
/// *its* neighbour — but it is a mora, and both callers only need its initial
/// letter, which is always `n`. Everything else the scanner handles itself
/// (the sokuon, the prolonged sound mark) is deliberately absent, so a sokuon
/// before a sokuon and a sokuon before a prolonged sound mark both see `None`.
#[inline]
fn peek(text: &str, at: usize) -> Option<&'static str> {
    let c = text.get(at..)?.chars().next()?;
    if NASAL.contains(&c) {
        return Some("n");
    }
    longest_at(text, at).map(|(_, mora)| mora.short)
}

/// What a sokuon romanizes to, given the mora that follows it.
///
/// ALA-LC: the sokuon doubles the initial consonant of the following syllable,
/// except before `ch`, where it is written `t` — `まっちゃ` is `matcha`, not
/// `cchacha`. Applied to the romanization rather than to the kana, the rule
/// reproduces every column of the syllabary without a table of its own,
/// including the two that look like exceptions when written in kana: `っし` is
/// `sshi` because `し` is `shi`, and `っふ` is `ffu` because `ふ` is `fu`.
///
/// With no following mora, or before one whose romanization begins with a
/// vowel, there is no consonant to double. Verbora romanizes that as nothing
/// rather than inventing a letter for it: no romanization standard assigns the
/// sokuon a segment of its own, and leaving the kana in the output would put a
/// character in romanized text that the caller asked to have romanized.
#[inline]
fn gemination(following: Option<&str>) -> &'static str {
    match following.map(str::as_bytes) {
        Some([b'c', b'h', ..]) => "t",
        Some([b, ..]) => consonant(*b),
        Some([]) | None => "",
    }
}

/// The doubled consonant letter, or `""` for anything that is not one.
///
/// A `match` rather than an index into the input, because [`Rewrite::to`] is
/// `&'static str` and the letter must outlive the borrow of the text it came
/// from.
#[inline]
const fn consonant(byte: u8) -> &'static str {
    match byte {
        b'b' => "b",
        b'c' => "c",
        b'd' => "d",
        b'f' => "f",
        b'g' => "g",
        b'h' => "h",
        b'j' => "j",
        b'k' => "k",
        b'm' => "m",
        b'n' => "n",
        b'p' => "p",
        b'r' => "r",
        b's' => "s",
        b't' => "t",
        b'v' => "v",
        b'w' => "w",
        b'y' => "y",
        b'z' => "z",
        // A vowel, or the ASCII space `・` romanizes to: nothing to double.
        _ => "",
    }
}

/// What the syllabic nasal romanizes to, given the mora that follows it.
///
/// ALA-LC: `m` before `b`, `m` and `p`; `n'` before a vowel or `y`; `n`
/// everywhere else, including at the end of the input. So `ばんび` is `bambi`,
/// `ほんや` is `hon'ya` and `でんわ` is `denwa`.
#[inline]
fn nasal(following: Option<&str>) -> &'static str {
    match following.map(str::as_bytes) {
        Some([b'b' | b'm' | b'p', ..]) => "m",
        Some([b'a' | b'e' | b'i' | b'o' | b'u' | b'y', ..]) => "n'",
        _ => "n",
    }
}

/// Whether `next` lengthens a romanization ending in `vowel`.
///
/// ALA-LC writes a long vowel with a macron where kana spells it as a vowel
/// repeated (`ああ` → `ā`, `ええ` → `ē`, `うう` → `ū`, `おお` → `ō`) and where
/// it spells long `o` as `おう` (`とうきょう` → `Tōkyō`).
///
/// Two sequences are deliberately absent, because ALA-LC does not treat them
/// as long:
///
/// * **`いい` is `ii`, not `ī`** — a long `i` is written doubled. `おいしい`
///   is `oishii`.
/// * **`えい` is `ei`, not `ē`** — `せんせい` is `sensei`.
///
/// The prolonged sound mark is not handled here: it lengthens *any* vowel,
/// `i` included, so `シー` is `shī` while `しい` is `shii`.
#[inline]
fn lengthens(vowel: u8, next: char) -> bool {
    match vowel {
        b'a' => matches!(next, 'あ' | 'ア'),
        b'u' => matches!(next, 'う' | 'ウ'),
        b'e' => matches!(next, 'え' | 'エ'),
        b'o' => matches!(next, 'う' | 'ウ' | 'お' | 'オ'),
        _ => false,
    }
}

/// The byte offset past a run of prolonged sound marks starting at `at`.
#[inline]
fn skip_prolonged(text: &str, at: usize) -> usize {
    let mut end = at;
    for c in text[end..].chars() {
        if c != PROLONGED_SOUND_MARK {
            break;
        }
        end += c.len_utf8();
    }
    end
}

/// The lazy stream of replacements that romanizing `text` would make.
///
/// This is the crate's single implementation of what romanization *is*:
/// [`transliterate_ja`], [`transliterate_ja_into`] and
/// [`transliterate_ja_normalized`] are all built on it, so there is one
/// description of the behaviour and no second copy to drift.
///
/// Yields ascending, non-overlapping [`Rewrite`]s and borrows the input for as
/// long as it lives. Nothing is allocated: every replacement is a `&'static
/// str` laid out at build time.
///
/// ```
/// use verbora_transliterators::Rewrites;
///
/// let hits: Vec<_> = Rewrites::new("かんぱい").collect();
/// let spelled: Vec<_> = hits.iter().map(|r| (r.from, r.to)).collect();
/// assert_eq!(spelled, [("か", "ka"), ("ん", "m"), ("ぱ", "pa"), ("い", "i")]);
/// ```
#[derive(Debug, Clone)]
pub struct Rewrites<'a> {
    /// The text being scanned.
    text: &'a str,
    /// Byte offset to resume from; `text.len()` when the scan is finished.
    pos: usize,
}

impl<'a> Rewrites<'a> {
    /// Starts a scan over `text`.
    ///
    /// The whole-input gate is applied here, once, rather than per character:
    /// text with no `0xE3` byte in it cannot match anything, so the iterator is
    /// empty by construction and never decodes a character.
    #[must_use]
    pub fn new(text: &'a str) -> Self {
        Self {
            text,
            pos: if may_contain_kana(text) {
                0
            } else {
                text.len()
            },
        }
    }

    /// The rewrite for a mora found in the index, plus whatever lengthens it.
    ///
    /// Kept separate from [`Iterator::next`] so that the three mark cases
    /// there read as three cases rather than as one nested expression.
    #[inline]
    fn mora_rewrite(&self, start: usize, len: usize, mora: Mora) -> Rewrite<'a> {
        let mut end = start + len;
        let mut to = mora.short;

        if let (Some(long), Some(vowel)) = (mora.long, mora.final_vowel())
            && let Some(next) = self.text[end..].chars().next()
        {
            if next == PROLONGED_SOUND_MARK {
                // A run counts once: `あーー` is one long `ā`, not `ā` plus a
                // mark with nothing left to lengthen.
                to = long;
                end = skip_prolonged(self.text, end + next.len_utf8());
            } else if lengthens(vowel, next) && !begins_a_mora(self.text, end) {
                // A scalar that could lengthen the vowel is only a lengthener
                // when it is not the start of a mora of its own. `ウィ` after a
                // mora reading `-u` is the case that separates the two: `ロウ`
                // then `ィン` reads `rowin`, not `rō` swallowing the `ィ` and
                // dropping the syllable. Leftmost-longest is the rule the scan
                // documents, and consuming past a key here broke it.
                to = long;
                end += next.len_utf8();
            }
        }

        Rewrite {
            start,
            end,
            from: &self.text[start..end],
            to,
        }
    }
}

/// Whether a mora key starts at `at` — the guard that keeps vowel lengthening
/// from consuming the first scalar of a two-scalar mora.
fn begins_a_mora(text: &str, at: usize) -> bool {
    crate::scan::longest_at(text, at).is_some_and(|(len, _)| {
        // A one-scalar match is the scalar we were about to treat as a
        // lengthener, which is exactly what lengthening is for. Only a longer
        // key means a syllable would be lost.
        text[at..]
            .chars()
            .next()
            .is_some_and(|c| len > c.len_utf8())
    })
}

impl<'a> Iterator for Rewrites<'a> {
    type Item = Rewrite<'a>;

    fn next(&mut self) -> Option<Rewrite<'a>> {
        for (offset, c) in self.text[self.pos..].char_indices() {
            let start = self.pos + offset;
            let after = start + c.len_utf8();

            let rewrite = if SOKUON.contains(&c) {
                Rewrite {
                    start,
                    end: after,
                    from: &self.text[start..after],
                    to: gemination(peek(self.text, after)),
                }
            } else if NASAL.contains(&c) {
                Rewrite {
                    start,
                    end: after,
                    from: &self.text[start..after],
                    to: nasal(peek(self.text, after)),
                }
            } else if c == PROLONGED_SOUND_MARK {
                // Reached here, the mark follows nothing this scanner
                // lengthened — a mora that ends in a vowel consumes its own
                // marks in `mora_rewrite`. There is no vowel to lengthen, so
                // the run romanizes as nothing.
                let end = skip_prolonged(self.text, after);
                Rewrite {
                    start,
                    end,
                    from: &self.text[start..end],
                    to: "",
                }
            } else if let Some((len, mora)) = longest_at(self.text, start) {
                self.mora_rewrite(start, len, mora)
            } else {
                continue;
            };

            self.pos = rewrite.end;
            return Some(rewrite);
        }

        self.pos = self.text.len();
        None
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        // Every rewrite consumes at least one byte, so the remaining input
        // bounds their number. Nothing is guaranteed to match, so the lower
        // bound is zero.
        (0, Some(self.text.len().saturating_sub(self.pos)))
    }
}

/// Romanizes Japanese kana into modified-Hepburn romaji.
///
/// One left-to-right pass. Kana are replaced mora by mora; everything else —
/// kanji, Latin text, digits, punctuation, halfwidth katakana, emoji — is
/// passed through byte for byte, and [`Cow::Borrowed`] is returned when
/// nothing changed at all.
///
/// ```
/// # use verbora_transliterators::transliterate_ja;
/// assert_eq!(transliterate_ja("あいうえお かきくけこ"), "aiueo kakikukeko");
/// assert_eq!(transliterate_ja("とうきょう"), "tōkyō");
/// assert_eq!(transliterate_ja("まっか ざっし たった はっぱ"), "makka zasshi tatta happa");
/// assert_eq!(transliterate_ja("まんと ばんび ほんや"), "manto bambi hon'ya");
/// assert_eq!(transliterate_ja("アヴァンギャルド"), "avangyarudo");
/// assert_eq!(transliterate_ja("ボージョレー・ヌーヴォー"), "bōjorē nūvō");
///
/// // Untouched: kanji, Latin, halfwidth katakana, iteration marks.
/// assert_eq!(transliterate_ja("abc ABC 漢字 (.)"), "abc ABC 漢字 (.)");
/// assert_eq!(transliterate_ja("ｱｲｳｴｵ"), "ｱｲｳｴｵ");
/// assert_eq!(transliterate_ja("時々"), "時々");
/// ```
///
/// # Contract
///
/// * **Total.** Every `&str` is accepted and no input panics.
/// * **Idempotent.** `transliterate_ja(transliterate_ja(t)) ==
///   transliterate_ja(t)` for every `t`: the output contains no mora the
///   scanner would rewrite again.
/// * **Borrowed exactly when unchanged.** [`Cow::Borrowed`] is returned if and
///   only if no mora was found, which makes matching on the `Cow` a correct way
///   to ask "did this contain kana?" rather than a fast path that might stop
///   working.
/// * **Nothing is invented.** The output contains no `U+FFFD` unless the input
///   did, and no character that was not either a romanization from the
///   syllabary or a byte copied from the input.
///
/// # It expects composed text
///
/// The syllabary is spelled in NFC: `が` is U+304C, not `か` U+304B followed by
/// U+3099. Decomposed kana, halfwidth katakana and fullwidth Latin therefore do
/// not match, and pass through. [`transliterate_ja_normalized`] is the pairing
/// that folds them first, and is what to reach for when the input's spelling is
/// not under your control.
///
/// # Choosing the right API
///
/// | Call | Returns | Allocates | Reach for it when |
/// |---|---|---|---|
/// | [`transliterate_ja`] | `Cow<str>` | one `String`, and only if some mora matched | you want the romaji, and the input is already NFC. **The right default.** |
/// | [`transliterate_ja_into`] | `()` | nothing of its own | you are romanizing many strings into one buffer and want to keep its capacity |
/// | [`Rewrites::new`] | an iterator | nothing at all | you need the byte ranges — highlighting, alignment, or counting morae — or want to stop early |
/// | [`transliterate_ja_normalized`] | `Cow<str>` | up to three `String`s | the input's spelling is not under your control |
/// | [`par_transliterate_ja_batch`] | `Vec<Cow<str>>` | one `Vec`, plus the per-input strings | you have many document-sized inputs and the `parallel` feature |
///
/// ```text
/// need byte offsets, or to stop early?  ── yes ─► Rewrites::new
///                  │ no
/// input might be halfwidth/decomposed?  ── yes ─► transliterate_ja_normalized
///                  │ no
/// romanizing into one shared buffer?    ── yes ─► transliterate_ja_into
///                  │ no
/// many multi-KB inputs, `parallel` on?  ── yes ─► par_transliterate_ja_batch
///                  │ no
///                  └───────────────────────────► transliterate_ja
/// ```
///
/// None of the four is faster than `transliterate_ja` at what
/// `transliterate_ja` does: they all drive the same [`Rewrites`] scan, and the
/// only cost they can remove is the output `String`. Relative cost is
/// **not currently measured** — the crate's Criterion suite exists
/// (`benches/transliterators.rs`) but has not been run against this
/// implementation, so no figures are published here.
#[must_use]
pub fn transliterate_ja(text: &str) -> Cow<'_, str> {
    apply(text, Rewrites::new(text))
}

/// [`transliterate_ja`], appending to a caller-owned buffer.
///
/// # The problem it solves
///
/// [`transliterate_ja`] allocates one `String` per call that found a mora.
/// Romanizing a corpus into a single document therefore allocates once per
/// input and immediately copies each result into the destination. This
/// function writes directly into `out`, so a loop over `n` inputs performs the
/// growth of one buffer instead of `n` allocations and `n` copies.
///
/// # What it costs the caller
///
/// `out` is **appended to, not cleared** — which is what makes building one
/// document from many inputs possible, and a silent correctness bug when a
/// caller meant to reuse the buffer for unrelated results and forgot to
/// [`String::clear`] it.
///
/// ```
/// # use verbora_transliterators::transliterate_ja_into;
/// let mut buf = String::new();
/// for word in ["こんにちは", " ", "せかい"] {
///     transliterate_ja_into(word, &mut buf);
/// }
/// assert_eq!(buf, "konnichiha sekai");
/// ```
///
/// # How it differs from the other shapes
///
/// Unlike [`transliterate_ja`] it never allocates an intermediate `String`,
/// and unlike [`Rewrites`] it does the splicing for you. If you want neither
/// the copy nor the splice — because you are measuring, highlighting or
/// aligning rather than producing text — iterate [`Rewrites::new`] directly.
pub fn transliterate_ja_into(text: &str, out: &mut String) {
    apply_into(text, Rewrites::new(text), out);
}

/// [`transliterate_ja`], fanned out across a `rayon` thread pool. Requires the
/// `parallel` feature.
///
/// # Why this exists
///
/// [`transliterate_ja`] is a pure function of one `&str` with no shared state,
/// so romanizing many independent documents is embarrassingly parallel with no
/// coordination between them. This function is exactly
/// `inputs.par_iter().map(transliterate_ja).collect()` — a thin fan-out over
/// the sequential primitive, not a second implementation of it. If you need a
/// different shape in parallel (a shared output buffer built with
/// [`transliterate_ja_into`], for instance), apply the same
/// `par_iter().map(…)` pattern at your own call site.
///
/// # When to reach for it
///
/// A `rayon` task costs on the order of a microsecond to schedule, so a batch
/// of short strings can easily cost more to distribute than to romanize.
/// Reach for this when the inputs are document-scale and there are more than a
/// handful of them; a plain `inputs.iter().map(transliterate_ja).collect()`
/// loop is the better answer otherwise.
///
/// The crossover point on this implementation is **not currently measured**.
/// The crate's `par_transliterate_ja_batch` Criterion group exists
/// (`benches/transliterators.rs`) but has not been run against it, so no
/// speedup figures are published here. Measure your own workload rather than
/// assuming the win.
///
/// # Allocation behaviour
///
/// One `Vec<Cow<str>>` sized to `inputs.len()`, plus whatever
/// [`transliterate_ja`] itself allocates per input — nothing for text that
/// contains no kana, and otherwise one `String`. No additional buffering, no
/// locking, no per-call thread-pool construction: this uses whichever global
/// `rayon` pool is already installed, so pool configuration stays the caller's
/// responsibility.
///
/// # Order and errors
///
/// Output order matches input order — `results[i]` is
/// `transliterate_ja(inputs[i])` — via `rayon`'s order-preserving `map` +
/// `collect`. [`transliterate_ja`] never errors, so there is no error shape to
/// preserve.
///
/// ```
/// use verbora_transliterators::par_transliterate_ja_batch;
///
/// let inputs = ["あいうえお", "ざっし", "plain ascii"];
/// let got = par_transliterate_ja_batch(&inputs);
/// assert_eq!(got, ["aiueo", "zasshi", "plain ascii"]);
/// ```
#[cfg(feature = "parallel")]
#[cfg_attr(docsrs, doc(cfg(feature = "parallel")))]
#[must_use]
pub fn par_transliterate_ja_batch<'a>(inputs: &[&'a str]) -> Vec<Cow<'a, str>> {
    use rayon::prelude::*;
    inputs.par_iter().map(|s| transliterate_ja(s)).collect()
}

/// The two **spacing** voiced sound marks, and the combining mark each one is
/// the spacing form of.
///
/// `UnicodeData.txt` gives U+309B `KATAKANA-HIRAGANA VOICED SOUND MARK` the
/// compatibility mapping `<compat> 0020 3099` and U+309C
/// `KATAKANA-HIRAGANA SEMI-VOICED SOUND MARK` the mapping `<compat> 0020 309A`
/// (The Unicode Standard §3.7, D65: a compatibility decomposition, which NFKD
/// and NFKC both apply). The pairing below is those mappings with the U+0020
/// dropped; [`transliterate_ja_normalized`] documents why the space is what
/// breaks NFKC on this input.
const SPACING_VOICED_MARKS: [(char, &str); 2] =
    [('\u{309B}', "\u{3099}"), ('\u{309C}', "\u{309A}")];

/// Re-spells the spacing voiced sound marks U+309B `゛` and U+309C `゜` as
/// their combining counterparts U+3099 and U+309A.
///
/// One character for one character, unconditionally, per
/// [`SPACING_VOICED_MARKS`]. The derivation — why NFKC alone strands these two
/// marks on an invented space, and why the re-spelling is unconditional rather
/// than gated on the preceding character — is on
/// [`transliterate_ja_normalized`], the one caller, so that it is readable
/// without `--document-private-items`.
///
/// Borrows when neither mark is present, which is every input that is not
/// legacy Shift-JIS-derived text.
fn respell_spacing_voiced_marks(text: &str) -> Cow<'_, str> {
    // An *exact* gate, not the scanner's `may_contain_kana` superset. This
    // pass has two keys rather than hundreds, and `str::contains(char)` is a
    // `memchr`-accelerated substring search, so ordinary Japanese — which has
    // the 0xE3 lead byte on every kana and the legacy spelling on none of it —
    // is rejected without decoding a single character.
    if !SPACING_VOICED_MARKS
        .iter()
        .any(|&(mark, _)| text.contains(mark))
    {
        return Cow::Borrowed(text);
    }
    apply(
        text,
        text.char_indices().filter_map(|(start, c)| {
            let &(_, to) = SPACING_VOICED_MARKS.iter().find(|&&(k, _)| k == c)?;
            let end = start + c.len_utf8();
            Some(Rewrite {
                start,
                end,
                from: &text[start..end],
                to,
            })
        }),
    )
}

/// [`nfkc`](verbora_normalizers::nfkc) then [`transliterate_ja`], with the two
/// spacing voiced sound marks re-spelled first.
///
/// The syllabary holds no halfwidth-katakana key, so `ｱｲｳｴｵ` survives
/// [`transliterate_ja`] unchanged. NFKC is what brings it into range:
/// compatibility decomposition maps halfwidth kana onto their fullwidth forms
/// and the halfwidth voiced sound mark U+FF9E onto the combining U+3099, which
/// canonical composition then folds into the preceding kana — so `ｶ` + `ﾞ`
/// arrives at the index as `ガ`. Fullwidth Latin and digits collapse to ASCII
/// in the same step, and NFD-spelled kana (`か` + U+3099) recompose.
///
/// # The one thing this does beyond NFKC
///
/// All **three** spellings of each voiced sound mark reach the index, not the
/// two NFKC handles on its own — because NFKC does not merely leave the third
/// alone, it damages it.
///
/// | Spelling | `UnicodeData.txt` mapping | `nfkc("か" + mark)` |
/// |---|---|---|
/// | U+3099, combining | — (it *is* the combining mark) | `が` |
/// | U+FF9E, halfwidth | `<narrow> 3099` | `が` |
/// | U+309B, spacing | `<compat> 0020 3099` | `か` `U+0020` `U+3099` |
///
/// The halfwidth mark decomposes to the bare combining mark, so NFKC's
/// Canonical Composition Algorithm (The Unicode Standard §3.11, D117) pairs it
/// with the preceding kana. The **spacing** mark U+309B `゛` — the legacy
/// Shift-JIS spelling, along with its semi-voiced twin U+309C `゜` —
/// decomposes to `SPACE` + combining mark instead, and U+0020 is a starter
/// (`Canonical_Combining_Class = 0`), so the Canonical Ordering Algorithm
/// (§3.11, D108–D109) cannot reorder U+3099 across it. Composition never sees
/// the kana and the mark adjacent, and the mark is stranded on **a space that
/// was not in the input** — the one character `・` romanizes to, and the one
/// any downstream whitespace tokenizer splits on. Left to NFKC alone,
/// `か゛っき` came out `"ka \u{3099}kki"`.
///
/// So U+309B and U+309C are re-spelled as U+3099 and U+309A *before*
/// normalization: one character for one character, unconditionally, which is
/// the standard's own compatibility mapping for them minus its `U+0020`. All
/// three spellings then converge — after a composable kana the mark composes
/// (`か゛` → `が` → `ga`), and anywhere else it stays a bare combining mark,
/// which is exactly what the halfwidth U+FF9E already did in that position. No
/// space is ever invented.
///
/// The re-spelling is not gated on what precedes the mark. A rule that fired
/// only before a kana with a canonical composition would need a composition
/// table this crate does not have, and would leave the spacing and halfwidth
/// spellings disagreeing in precisely the positions where neither composes.
///
/// # What this does not do
///
/// Iteration marks are **not** expanded: `時々` romanizes as `時々`, not
/// `時時`. Expanding them is an orthographic rewrite with no Unicode
/// definition, and it is not idempotent — applying it to `あ々々` twice gives
/// two different answers — so it cannot be used to canonicalise text for
/// comparison, which is what a normalizer is for. Callers who need
/// iteration-mark expansion must do it themselves, before calling this.
///
/// # Cost, and when not to pay it
///
/// Three stages instead of one, each returning [`Cow::Borrowed`] when it
/// changed nothing, so text that needs none of them is still borrowed all the
/// way through. When the input is known to be NFC — because it came from a
/// source you normalize yourself — [`transliterate_ja`] does strictly less
/// work for the same answer.
///
/// ```
/// # use verbora_transliterators::{transliterate_ja, transliterate_ja_normalized};
/// assert_eq!(transliterate_ja("ｱｲｳｴｵ"), "ｱｲｳｴｵ");
/// assert_eq!(transliterate_ja_normalized("ｱｲｳｴｵ"), "aiueo");
/// // Fullwidth Latin comes along in the same step.
/// assert_eq!(transliterate_ja_normalized("ＡＢＣ"), "ABC");
/// // All three spellings of the voiced mark agree.
/// assert_eq!(transliterate_ja_normalized("ｶﾞｯｷ"), "gakki");           // U+FF9E, halfwidth
/// assert_eq!(transliterate_ja_normalized("か\u{3099}っき"), "gakki"); // U+3099, combining
/// assert_eq!(transliterate_ja_normalized("か\u{309B}っき"), "gakki"); // U+309B, spacing
/// // With no kana to attach to, the mark stays a bare combining mark —
/// // NFKC's `0020 3099` expansion would have put a space here.
/// assert_eq!(transliterate_ja_normalized("\u{309B}"), "\u{3099}");
/// // Iteration marks pass through.
/// assert_eq!(transliterate_ja_normalized("時々"), "時々");
/// ```
#[must_use]
pub fn transliterate_ja_normalized(text: &str) -> Cow<'_, str> {
    let respelled = respell_spacing_voiced_marks(text);
    let normalized = map_cow(respelled, verbora_normalizers::nfkc);
    map_cow(normalized, transliterate_ja)
}

#[cfg(test)]
mod tests {
    /// Vowel lengthening must not consume the first scalar of a mora.
    ///
    /// `ロウ` reads `-u`, and `ウ` lengthens an `o`, so a lengthener that does
    /// not check for a longer key eats the `ィ` of `ウィ` and drops the whole
    /// syllable: `ハロウィン` came out `harōin` rather than `harowin`. Six keys
    /// collide this way (`うぃ うぇ うぉ ウィ ウェ ウォ`), after any of the
    /// morae whose reading ends in `o` or `u`.
    ///
    /// The crate's own `no_romanization_leaves_a_romanizable_kana_behind` could
    /// not catch it: no kana survived, the reading was simply wrong.
    #[test]
    fn a_lengthener_never_swallows_the_start_of_a_mora() {
        assert_eq!(transliterate_ja("ハロウィン"), "harowin");
        assert_eq!(transliterate_ja("スウェーデン"), "suwēden");
        assert_eq!(transliterate_ja("クウェート"), "kuwēto");
        assert_eq!(transliterate_ja("ミルウォーキー"), "miruwōkī");
    }

    /// The guard above must not cost legitimate lengthening, where the next
    /// scalar begins no key of its own.
    #[test]
    fn a_lengthener_still_lengthens_when_no_mora_starts_there() {
        assert_eq!(transliterate_ja("とうきょう"), "tōkyō");
        assert_eq!(transliterate_ja("コーヒー"), "kōhī");
        assert_eq!(transliterate_ja("おかあさん"), "okāsan");
    }

    use super::*;
    use crate::syllabary::{HIRAGANA, HIRAGANA_ONLY, KATAKANA_ONLY};

    /// Distance from a hiragana kana to its katakana counterpart, per The
    /// Unicode Standard §18.4.
    const KATAKANA_OFFSET: u32 = 0x60;

    /// Every `(kana, romaji)` pair the index is supposed to hold, rebuilt here
    /// from the syllabary source.
    ///
    /// The whole point of the enumeration tests is to walk the *index* the
    /// scanner actually reads, so the expected values come from
    /// `src/syllabary.rs` and the katakana half is re-derived by the same
    /// §18.4 offset `build.rs` uses. Recording what the scanner emits would
    /// test nothing.
    fn syllabary() -> Vec<(String, &'static str)> {
        let mut out = Vec::with_capacity(HIRAGANA.len() * 2 + 8);
        for &(kana, romaji) in HIRAGANA {
            out.push((kana.to_owned(), romaji));
            let katakana: String = kana
                .chars()
                .map(|c| char::from_u32(c as u32 + KATAKANA_OFFSET).expect("katakana"))
                .collect();
            out.push((katakana, romaji));
        }
        for &(kana, romaji) in HIRAGANA_ONLY.iter().chain(KATAKANA_ONLY) {
            out.push((kana.to_owned(), romaji));
        }
        out
    }

    // ---------------------------------------------------------------------
    // Enumeration: every entry of the table, walked through the pipeline.
    // ---------------------------------------------------------------------

    #[test]
    fn the_syllabary_holds_the_number_of_morae_it_is_supposed_to() {
        // 5 vowels + 10 か/が + 10 さ/ざ + 10 た/だ + 5 な + 15 は/ば/ぱ
        // + 5 ま + 3 や + 5 ら + 4 わ-row-and-archaic + 1 `ゔ` + 11 small
        // = 84 gojūon; plus 36 yōon, 11 from 「外来語の表記」第1表, 19 from
        // 第2表 and 5 voiced counterparts Verbora adds = 155 hiragana
        // entries. Each is mirrored into katakana, and `ゟ` plus the six
        // katakana-only characters have no mirror.
        assert_eq!(HIRAGANA.len(), 84 + 36 + 11 + 19 + 5);
        assert_eq!(HIRAGANA_ONLY.len(), 1);
        assert_eq!(KATAKANA_ONLY.len(), 6);
        assert_eq!(syllabary().len(), 155 * 2 + 7);
    }

    #[test]
    fn every_syllabary_entry_romanizes_to_its_own_reading() {
        for (kana, romaji) in syllabary() {
            assert_eq!(
                transliterate_ja(&kana),
                romaji,
                "{kana:?} (U+{:04X}…)",
                kana.chars().next().expect("non-empty") as u32
            );
        }
    }

    #[test]
    fn every_key_is_reachable_only_through_the_kana_gate() {
        // `may_contain_kana` rejects a document with no `0xE3` lead byte
        // without decoding anything. That is only sound if every key has one.
        for (kana, _) in syllabary() {
            assert!(
                kana.as_bytes().contains(&KANA_LEAD_BYTE),
                "key {kana:?} is reachable without a 0xE3 byte"
            );
            assert!(may_contain_kana(&kana), "gate rejects key {kana:?}");
        }
        for c in SOKUON.iter().chain(&NASAL).chain(&[PROLONGED_SOUND_MARK]) {
            assert!(may_contain_kana(&c.to_string()), "gate rejects {c:?}");
        }
    }

    #[test]
    fn katakana_is_the_hiragana_half_shifted_by_0x60() {
        // §18.4's parallel encoding, checked against the built index rather
        // than assumed: `build.rs` derives the katakana half from this offset,
        // so if the offset were wrong every katakana mora would be wrong.
        for &(hiragana, romaji) in HIRAGANA {
            let katakana: String = hiragana
                .chars()
                .map(|c| {
                    let shifted = c as u32 + KATAKANA_OFFSET;
                    assert!(
                        (0x30A1..=0x30F6).contains(&shifted),
                        "{c:?} shifts out of the Katakana block"
                    );
                    char::from_u32(shifted).expect("katakana")
                })
                .collect();
            assert_eq!(transliterate_ja(hiragana), romaji, "{hiragana:?}");
            assert_eq!(transliterate_ja(&katakana), romaji, "{katakana:?}");
        }
    }

    #[test]
    fn normalization_never_changes_a_key_s_romanization() {
        // The transform-then-lookup check, enumerated rather than sampled:
        // `transliterate_ja_normalized` runs NFKC *before* the index is
        // consulted, so a key whose NFKC form is spelled differently would be
        // unreachable through that entry point. Two keys are in fact
        // rewritten by NFKC; both must still land on the same romanization.
        let mut rewritten = Vec::new();
        for (kana, romaji) in syllabary() {
            let folded = verbora_normalizers::nfkc(&kana);
            if folded != kana {
                rewritten.push((kana.clone(), folded.to_string()));
            }
            assert_eq!(
                transliterate_ja_normalized(&kana),
                romaji,
                "NFKC changed the romanization of {kana:?}"
            );
        }
        // `<vertical> 3088 308A` and `<square> 30B3 30C8` from
        // `UnicodeData.txt`: the only two keys with a compatibility mapping.
        rewritten.sort();
        assert_eq!(
            rewritten,
            [
                ("ゟ".to_owned(), "より".to_owned()),
                ("ヿ".to_owned(), "コト".to_owned()),
            ]
        );
        // And the readings the UCD names gave them agree with the readings of
        // the decompositions, which is why the rewrite is harmless.
        assert_eq!(transliterate_ja("より"), transliterate_ja("ゟ"));
        assert_eq!(transliterate_ja("コト"), transliterate_ja("ヿ"));
    }

    #[test]
    fn every_key_is_already_in_nfc() {
        // `transliterate_ja` does not normalize, so a key spelled in NFD
        // would be unreachable for callers whose text is composed — which is
        // the overwhelmingly common spelling.
        for (kana, _) in syllabary() {
            assert_eq!(verbora_normalizers::nfc(&kana), kana, "{kana:?} is not NFC");
        }
    }

    #[test]
    fn the_sokuon_doubles_the_following_consonant_for_every_mora() {
        // ALA-LC: double the initial consonant of the following syllable,
        // except before `ch`, where the sokuon is written `t`. Restated here
        // over the romanization so that the assertion is the rule and not a
        // copy of the implementation's table.
        for (kana, romaji) in syllabary() {
            let expected_doubling = if romaji.starts_with("ch") {
                "t".to_owned()
            } else {
                match romaji.chars().next().expect("non-empty") {
                    'a' | 'e' | 'i' | 'o' | 'u' | ' ' => String::new(),
                    c => c.to_string(),
                }
            };
            for sokuon in SOKUON {
                let input = format!("{sokuon}{kana}");
                assert_eq!(
                    transliterate_ja(&input),
                    format!("{expected_doubling}{romaji}"),
                    "{input:?}"
                );
            }
        }
    }

    #[test]
    fn the_syllabic_nasal_follows_ala_lc_for_every_mora() {
        // ALA-LC: `m` before b/m/p, `n'` before a vowel or `y`, else `n`.
        for (kana, romaji) in syllabary() {
            let expected = match romaji.chars().next().expect("non-empty") {
                'b' | 'm' | 'p' => "m",
                'a' | 'e' | 'i' | 'o' | 'u' | 'y' => "n'",
                _ => "n",
            };
            for nasal in NASAL {
                let input = format!("{nasal}{kana}");
                assert_eq!(
                    transliterate_ja(&input),
                    format!("{expected}{romaji}"),
                    "{input:?}"
                );
            }
        }
    }

    #[test]
    fn the_prolonged_sound_mark_lengthens_every_mora_that_ends_in_a_vowel() {
        // ALA-LC writes a long vowel with a macron: U+0101 ā, U+0113 ē,
        // U+012B ī, U+014D ō, U+016B ū. Unlike the repeated-kana spellings,
        // the mark lengthens `i` too.
        for (kana, romaji) in syllabary() {
            let want = match romaji.chars().next_back().expect("non-empty") {
                'a' => Some('ā'),
                'e' => Some('ē'),
                'i' => Some('ī'),
                'o' => Some('ō'),
                'u' => Some('ū'),
                _ => None,
            };
            let input = format!("{kana}{PROLONGED_SOUND_MARK}");
            let expected = match want {
                Some(macron) => format!("{}{macron}", &romaji[..romaji.len() - 1]),
                // `・` romanizes to a space: no vowel, so the mark that
                // follows has nothing to lengthen and disappears.
                None => romaji.to_owned(),
            };
            assert_eq!(transliterate_ja(&input), expected, "{input:?}");
            // A run counts once.
            let run = format!("{kana}{PROLONGED_SOUND_MARK}{PROLONGED_SOUND_MARK}");
            assert_eq!(transliterate_ja(&run), expected, "{run:?}");
        }
    }

    #[test]
    fn every_kana_block_scalar_is_romanized_or_deliberately_not() {
        // Walks U+3040..=U+30FF and partitions it. The pass-through half is
        // the list `lib.rs` publishes; anything that drifts into or out of it
        // is a silent change to what "romanized" means.
        let mut untouched = Vec::new();
        for cp in 0x3040u32..=0x30FF {
            let c = char::from_u32(cp).expect("BMP scalar");
            let text = c.to_string();
            if transliterate_ja(&text) == text {
                untouched.push(cp);
            }
        }
        assert_eq!(
            untouched,
            [
                0x3040, // unassigned
                0x3097, 0x3098, // unassigned
                0x3099, 0x309A, // combining voiced / semi-voiced sound mark
                0x309B, 0x309C, // spacing voiced / semi-voiced sound mark
                0x309D, 0x309E, // hiragana iteration marks
                0x30A0, // katakana-hiragana double hyphen
                0x30FD, 0x30FE, // katakana iteration marks
            ]
        );
    }

    #[test]
    fn no_romanization_leaves_a_romanizable_kana_behind() {
        // The defect this pins: a romanizer whose output still contains the
        // script it was asked to convert. Every mora of the syllabary, every
        // mark, and every ordered pair of the two.
        let all: Vec<String> = syllabary()
            .into_iter()
            .map(|(kana, _)| kana)
            .chain(SOKUON.iter().map(char::to_string))
            .chain(NASAL.iter().map(char::to_string))
            .chain(std::iter::once(PROLONGED_SOUND_MARK.to_string()))
            .collect();
        for a in &all {
            for b in &all {
                let input = format!("{a}{b}");
                let out = transliterate_ja(&input);
                assert!(
                    !out.chars().any(|c| ('\u{3041}'..='\u{30FF}').contains(&c)),
                    "{input:?} romanized to {out:?}, which still holds kana"
                );
            }
        }
    }

    #[test]
    fn romanization_is_idempotent() {
        for (kana, _) in syllabary() {
            let once = transliterate_ja(&kana).into_owned();
            assert_eq!(transliterate_ja(&once), once, "{kana:?}");
        }
        for input in [
            "とうきょう",
            "ボージョレー・ヌーヴォー",
            "こんにちは世界",
            "か\u{3099}",
            "あ々々",
            "ッ",
            "ー",
        ] {
            let once = transliterate_ja(input).into_owned();
            assert_eq!(transliterate_ja(&once), once, "{input:?}");
        }
    }

    // ---------------------------------------------------------------------
    // Fixtures from the standard.
    // ---------------------------------------------------------------------

    #[test]
    fn ala_lc_worked_examples() {
        // Each row is a spelling whose modified-Hepburn romanization is
        // settled by ALA-LC's own rules, named beside it.
        for (kana, romaji, rule) in [
            ("とうきょう", "tōkyō", "long o written おう"),
            ("おおさか", "ōsaka", "long o written おお"),
            ("おかあさん", "okāsan", "long a written ああ"),
            ("くうき", "kūki", "long u written うう"),
            ("おいしい", "oishii", "long i is doubled, not macronned"),
            ("せんせい", "sensei", "えい is ei, not ē"),
            ("スーパー", "sūpā", "the prolonged sound mark is a macron"),
            ("コーヒー", "kōhī", "…including over i"),
            ("ざっし", "zasshi", "sokuon doubles s, because し is shi"),
            ("がっこう", "gakkō", "sokuon doubles k"),
            ("きって", "kitte", "sokuon doubles t"),
            ("まっちゃ", "matcha", "sokuon before ch is t"),
            ("いっしょ", "issho", "sokuon before a yōon"),
            ("はっぴょう", "happyō", "sokuon plus a long yōon"),
            ("しんぶん", "shimbun", "ん is m before b"),
            ("ぐんま", "gumma", "ん is m before m"),
            ("こんぺいとう", "kompeitō", "ん is m before p"),
            ("ほんや", "hon'ya", "ん is n' before y"),
            ("あんない", "annai", "ん is n before n"),
            ("でんわ", "denwa", "ん is n before w"),
            ("ぎんこう", "ginkō", "ん is n before k"),
            ("にほん", "nihon", "ん at the end of the input"),
            ("ふじさん", "fujisan", "ふ is fu and じ is ji"),
            ("ちず", "chizu", "ち is chi and ず is zu"),
            ("つなみ", "tsunami", "つ is tsu"),
        ] {
            assert_eq!(transliterate_ja(kana), romaji, "{kana:?}: {rule}");
        }
    }

    #[test]
    fn gairaigo_worked_examples() {
        // 「外来語の表記」's extended syllables, in the words they exist for.
        for (kana, romaji) in [
            ("ファイル", "fairu"),
            ("ヴァイオリン", "vaiorin"),
            ("ディズニー", "dizunī"),
            ("チェス", "chesu"),
            ("ジェット", "jetto"),
            ("シェフ", "shefu"),
            ("ツアー", "tsuā"),
            ("パーティー", "pātī"),
            ("コンピュータ", "kompyūta"),
            ("バッファ", "baffa"),
            ("クォーツ", "kwōtsu"),
            ("イエス", "iesu"),
            ("ボージョレー・ヌーヴォー", "bōjorē nūvō"),
            ("アヴァンギャルド", "avangyarudo"),
        ] {
            assert_eq!(transliterate_ja(kana), romaji, "{kana:?}");
        }
    }

    #[test]
    fn the_grapheme_driven_limits_this_crate_documents() {
        // Not defects: consequences of having no dictionary and no parser,
        // stated in `lib.rs` and pinned here so they cannot change silently.
        // The topic particle `は` keeps its kana value.
        assert_eq!(transliterate_ja("こんにちは"), "konnichiha");
        // …and so does the direction particle `へ`.
        assert_eq!(transliterate_ja("がっこうへ"), "gakkōhe");
        // Kanji are copied through.
        assert_eq!(
            transliterate_ja("これは日本語のテストです。"),
            "koreha日本語notesutodesu。"
        );
        // `おう` is always read as a long o, even in a verb where the `う` is
        // an inflection rather than a lengthening.
        assert_eq!(transliterate_ja("おもう"), "omō");
    }

    #[test]
    fn the_middle_dot_is_the_one_punctuation_key() {
        assert_eq!(transliterate_ja("・"), " ");
        assert_eq!(transliterate_ja("・・"), "  ");
        assert_eq!(transliterate_ja("あ・い"), "a i");
        // It carries no vowel, so nothing lengthens it and nothing geminates
        // against it.
        assert_eq!(transliterate_ja("・ー"), " ");
        assert_eq!(transliterate_ja("っ・"), " ");
        assert_eq!(transliterate_ja("ん・"), "n ");
    }

    #[test]
    fn a_modifier_with_nothing_to_modify_romanizes_to_nothing() {
        for (input, want) in [
            ("っ", ""),
            ("ッ", ""),
            ("ー", ""),
            ("ーー", ""),
            ("っっ", ""),
            // Before a vowel there is no consonant to double.
            ("っあ", "a"),
            ("ッア", "a"),
            // Before Latin, a digit or a kanji there is no following mora at
            // all — the surrounding text is untouched either way.
            ("ッA", "A"),
            ("ッ1", "1"),
            ("ッ漢", "漢"),
            ("ッ😀", "😀"),
            ("ー漢", "漢"),
            // A prolonged sound mark after a consonant-final mora.
            ("んー", "n"),
        ] {
            assert_eq!(transliterate_ja(input), want, "{input:?}");
        }
    }

    // ---------------------------------------------------------------------
    // Total function: edges, other scripts, borrowing.
    // ---------------------------------------------------------------------

    #[test]
    fn empty_and_single_character_inputs() {
        assert_eq!(transliterate_ja(""), "");
        assert!(matches!(transliterate_ja(""), Cow::Borrowed("")));
        assert_eq!(transliterate_ja("あ"), "a");
        assert_eq!(transliterate_ja("ア"), "a");
        assert_eq!(transliterate_ja(" "), " ");
        assert_eq!(transliterate_ja("\t\n\r "), "\t\n\r ");
    }

    #[test]
    fn scripts_the_romanizer_does_not_touch_are_borrowed_unchanged() {
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
            "、。「」",
            "々〆〇",
        ] {
            assert!(
                matches!(transliterate_ja(s), Cow::Borrowed(_)),
                "{s:?} should be returned untouched"
            );
            assert_eq!(transliterate_ja(s), s);
        }
    }

    #[test]
    fn kana_mixed_with_everything_else() {
        // Astral scalars, punctuation, digits and other scripts adjacent to
        // kana that *is* rewritten — where a scanner that indexed wrongly
        // would corrupt the surrounding text rather than merely leave it
        // alone. Every expectation is the surrounding text byte for byte plus
        // the mora's own reading.
        for (input, want) in [
            ("😀あ😀", "😀a😀"),
            ("𝕳カ𝕳", "𝕳ka𝕳"),
            ("カ😀ー", "ka😀"),
            ("・😀・", " 😀 "),
            ("あ!い?う。", "a!i?u。"),
            ("abcあいう123", "abcaiu123"),
            ("漢っか", "漢kka"),
            ("あ\u{FEFF}ー", "a\u{FEFF}"),
            ("ｱあ", "ｱa"),
            ("时々ア", "时々a"),
        ] {
            assert_eq!(transliterate_ja(input), want, "input {input:?}");
        }
    }

    #[test]
    fn combining_marks_are_left_alone_without_normalization() {
        // `transliterate_ja` expects NFC. A decomposed `が` is `か` plus a
        // combining mark, and only the `か` is a key — the mark survives
        // untouched rather than being dropped or misread.
        assert_eq!(transliterate_ja("か\u{3099}"), "ka\u{3099}");
        assert_eq!(transliterate_ja("は\u{309A}"), "ha\u{309A}");
        assert_eq!(transliterate_ja("\u{3099}"), "\u{3099}");
        // …and the composed spelling is what the index holds.
        assert_eq!(transliterate_ja("が"), "ga");
        assert_eq!(transliterate_ja("ぱ"), "pa");
    }

    #[test]
    fn document_sized_inputs() {
        // 15 morae repeated: `tōkyōtokkyokyokakyoku`, the standard tongue
        // twister, 2,000 times over.
        let input = "とうきょうとっきょきょかきょく".repeat(2_000);
        let want = "tōkyōtokkyokyokakyoku".repeat(2_000);
        assert_eq!(transliterate_ja(&input), want);

        // And the rejection path on a document-sized Latin string.
        let latin = "the quick brown fox ".repeat(5_000);
        assert!(matches!(transliterate_ja(&latin), Cow::Borrowed(_)));
    }

    #[test]
    fn no_scalar_value_makes_it_panic() {
        // Total over every scalar, alone and next to each of the three marks
        // whose rules read their neighbour — the positions where a scanner
        // that sliced on a byte offset instead of a character boundary would
        // panic.
        let neighbours = ["", "っ", "ん", "ー", "か", "\u{3099}"];
        for cp in 0u32..=0x10FFFF {
            let Some(c) = char::from_u32(cp) else {
                continue;
            };
            for n in neighbours {
                let mut input = String::from(n);
                input.push(c);
                input.push_str(n);
                let _ = transliterate_ja(&input);
                let _ = Rewrites::new(&input).count();
            }
        }
    }

    // ---------------------------------------------------------------------
    // The lazy primitive.
    // ---------------------------------------------------------------------

    #[test]
    fn rewrites_report_what_they_replaced_and_where() {
        let hits: Vec<_> = Rewrites::new("カナ").collect();
        assert_eq!(hits.len(), 2);
        assert_eq!(
            (hits[0].start, hits[0].end, hits[0].from, hits[0].to),
            (0, 3, "カ", "ka")
        );
        assert_eq!(
            (hits[1].start, hits[1].end, hits[1].from, hits[1].to),
            (3, 6, "ナ", "na")
        );

        // A lengthened mora spans the mark it consumed.
        let hits: Vec<_> = Rewrites::new("カー").collect();
        assert_eq!(hits.len(), 1);
        assert_eq!(
            (hits[0].start, hits[0].end, hits[0].from, hits[0].to),
            (0, 6, "カー", "kā")
        );

        // A modifier with nothing to modify is reported, not skipped.
        let hits: Vec<_> = Rewrites::new("ッ").collect();
        assert_eq!(hits.len(), 1);
        assert_eq!(
            (hits[0].start, hits[0].end, hits[0].from, hits[0].to),
            (0, 3, "ッ", "")
        );

        // Non-kana between morae is not reported at all.
        let hits: Vec<_> = Rewrites::new("カ漢ナ").collect();
        assert_eq!(hits.len(), 2);
        assert_eq!((hits[0].start, hits[0].end), (0, 3));
        assert_eq!((hits[1].start, hits[1].end), (6, 9));
    }

    #[test]
    fn rewrites_are_ascending_and_non_overlapping_and_splice_back() {
        for input in [
            "とうきょうとっきょきょかきょく",
            "ボージョレー・ヌーヴォー",
            "abcあいう123",
            "漢っか",
            "",
        ] {
            let mut end = 0usize;
            let mut spliced = String::new();
            let mut copied = 0usize;
            for r in Rewrites::new(input) {
                assert!(r.start >= end, "{input:?} rewrites overlap");
                assert!(r.end > r.start, "{input:?} empty range");
                assert_eq!(r.from, &input[r.start..r.end]);
                spliced.push_str(&input[copied..r.start]);
                spliced.push_str(r.to);
                copied = r.end;
                end = r.end;
            }
            spliced.push_str(&input[copied..]);
            assert_eq!(spliced, transliterate_ja(input), "{input:?}");
        }
    }

    #[test]
    fn size_hint_bounds_the_stream() {
        let it = Rewrites::new("カタカナ");
        assert_eq!(it.size_hint(), (0, Some(12)));
        assert_eq!(it.count(), 4);
        // The gate makes the iterator empty without scanning.
        assert_eq!(Rewrites::new("abc").size_hint(), (0, Some(0)));
        assert_eq!(Rewrites::new("abc").count(), 0);
    }

    #[test]
    fn the_gate_rejects_only_what_it_should() {
        assert!(!may_contain_kana("plain ascii"));
        assert!(!may_contain_kana("café résumé"));
        assert!(!may_contain_kana("Москва"));
        assert!(!may_contain_kana("😀"));
        // Halfwidth katakana is U+FF61.., lead byte 0xEF: gated out, and
        // indeed the romanizer leaves it alone.
        assert!(!may_contain_kana("ｱｲｳ"));
        assert!(may_contain_kana("あ"));
        assert!(may_contain_kana("ア"));
        assert!(may_contain_kana("・"));
        // A false positive is allowed — the gate is a superset test — but it
        // must not change the answer.
        assert!(may_contain_kana("々"));
        assert_eq!(transliterate_ja("々"), "々");
    }

    // ---------------------------------------------------------------------
    // The buffer API.
    // ---------------------------------------------------------------------

    #[test]
    fn transliterate_ja_into_appends_and_keeps_the_buffer() {
        let mut buf = String::from("[");
        transliterate_ja_into("カナ", &mut buf);
        transliterate_ja_into("]", &mut buf);
        assert_eq!(buf, "[kana]");

        // A caller's reserved capacity survives, even on the first call into
        // an empty buffer.
        let mut reserved = String::with_capacity(4096);
        let addr = reserved.as_ptr();
        transliterate_ja_into("カナ", &mut reserved);
        assert_eq!(reserved, "kana");
        assert_eq!(reserved.capacity(), 4096);
        assert!(std::ptr::eq(reserved.as_ptr(), addr));
    }

    #[test]
    fn transliterate_ja_into_agrees_with_transliterate_ja() {
        for input in [
            "",
            "abc",
            "とうきょう",
            "ボージョレー・ヌーヴォー",
            "ッ",
            "漢っか",
        ] {
            let mut buf = String::new();
            transliterate_ja_into(input, &mut buf);
            assert_eq!(buf, transliterate_ja(input), "{input:?}");
        }
    }

    // ---------------------------------------------------------------------
    // Normalization.
    // ---------------------------------------------------------------------

    #[test]
    fn normalized_composition_reaches_halfwidth_and_decomposed_kana() {
        assert_eq!(transliterate_ja_normalized("ｱｲｳｴｵ"), "aiueo");
        assert_eq!(transliterate_ja_normalized("ｶﾞｯｷ"), "gakki");
        assert_eq!(transliterate_ja_normalized("ＡＢＣ"), "ABC");
        // Unchanged input still borrows all the way through.
        assert!(matches!(
            transliterate_ja_normalized("abc"),
            Cow::Borrowed("abc")
        ));
    }

    /// The *combining* voiced marks U+3099 and U+309A compose onto the
    /// preceding kana, so NFD-spelled Japanese reaches the index.
    /// Derivations from `UnicodeData.txt`: U+304C GA is `304B 3099` and
    /// U+3071 PA is `306F 309A`, both canonical, so canonical composition
    /// recombines them.
    #[test]
    fn normalization_composes_the_combining_voiced_marks() {
        assert_eq!(transliterate_ja_normalized("か\u{3099}"), "ga");
        assert_eq!(transliterate_ja_normalized("は\u{309A}"), "pa");
        assert_eq!(transliterate_ja_normalized("カ\u{3099}ッキ"), "gakki");
        assert_eq!(transliterate_ja_normalized("ｶﾞｯｷ"), "gakki");
    }

    /// The **spacing** voiced sound marks U+309B `゛` and U+309C `゜` — the
    /// legacy Shift-JIS spelling — reach the index too, because
    /// `respell_spacing_voiced_marks` runs before NFKC does.
    ///
    /// Without that pre-pass NFKC alone expands them per their
    /// `UnicodeData.txt` compatibility mappings, `<compat> 0020 3099` and
    /// `<compat> 0020 309A`, and the U+0020 in the middle is a starter that
    /// the Canonical Ordering Algorithm cannot reorder across — so the mark
    /// never reaches the kana and a **space** is injected into the middle of
    /// the word.
    #[test]
    fn normalization_respells_the_spacing_voiced_marks() {
        // After a kana the mark composes, exactly as the other two spellings
        // do.
        assert_eq!(transliterate_ja_normalized("か\u{309B}"), "ga");
        assert_eq!(transliterate_ja_normalized("は\u{309C}"), "pa");
        assert_eq!(transliterate_ja_normalized("カ\u{309B}"), "ga");
        assert_eq!(transliterate_ja_normalized("ハ\u{309C}"), "pa");
        assert_eq!(transliterate_ja_normalized("か\u{309B}っき"), "gakki");
        assert_eq!(
            transliterate_ja_normalized("は\u{309C}っは\u{309C}"),
            "pappa"
        );

        // All three spellings of each mark land on the same answer, for every
        // base kana that has a composition with it.
        for (spacing, halfwidth, combining) in [
            ('\u{309B}', '\u{FF9E}', '\u{3099}'),
            ('\u{309C}', '\u{FF9F}', '\u{309A}'),
        ] {
            for base in ['か', 'は', 'カ', 'ハ'] {
                let want = transliterate_ja_normalized(&format!("{base}{combining}")).into_owned();
                assert_eq!(
                    transliterate_ja_normalized(&format!("{base}{spacing}")),
                    want,
                    "spacing U+{:04X} after {base:?}",
                    spacing as u32
                );
                assert_eq!(
                    transliterate_ja_normalized(&format!("{base}{halfwidth}")),
                    want,
                    "halfwidth U+{:04X} after {base:?}",
                    halfwidth as u32
                );
            }
        }

        // In isolation there is nothing to attach to, so the mark stays a
        // bare combining mark — the same thing the halfwidth spelling already
        // gave, and *not* a space plus a combining mark.
        assert_eq!(transliterate_ja_normalized("\u{309B}"), "\u{3099}");
        assert_eq!(transliterate_ja_normalized("\u{309C}"), "\u{309A}");
        assert_eq!(
            transliterate_ja_normalized("\u{309B}"),
            transliterate_ja_normalized("\u{FF9E}")
        );
        assert_eq!(
            transliterate_ja_normalized("\u{309C}"),
            transliterate_ja_normalized("\u{FF9F}")
        );

        // The defect this pins, stated as the property that failed: the marks
        // never introduce a space that the input did not have.
        for input in [
            "か\u{309B}",
            "は\u{309C}",
            "カ\u{309B}キ",
            "\u{309B}",
            "\u{309C}",
            "あ\u{309B}",
            "a\u{309B}",
            "漢\u{309C}",
        ] {
            assert!(
                !transliterate_ja_normalized(input).contains(' '),
                "{input:?} gained a space"
            );
        }
    }

    #[test]
    fn nfkc_alone_would_strand_the_spacing_marks_on_an_invented_space() {
        // The reason the pre-pass exists, read out of the normalizer rather
        // than asserted: NFKC of `か゛` really does contain a U+0020 that the
        // input did not.
        assert_eq!(
            verbora_normalizers::nfkc("か\u{309B}"),
            "か\u{0020}\u{3099}"
        );
        assert_eq!(
            transliterate_ja(&verbora_normalizers::nfkc("か\u{309B}っき")),
            "ka \u{3099}kki"
        );
        // …and with the pre-pass, it does not.
        assert_eq!(transliterate_ja_normalized("か\u{309B}っき"), "gakki");
    }

    #[test]
    fn respelling_touches_only_the_two_spacing_marks() {
        assert_eq!(respell_spacing_voiced_marks("\u{309B}"), "\u{3099}");
        assert_eq!(respell_spacing_voiced_marks("\u{309C}"), "\u{309A}");
        assert_eq!(
            respell_spacing_voiced_marks("か\u{309B}き\u{309C}"),
            "か\u{3099}き\u{309A}"
        );
        for s in [
            "",
            "abc",
            "かき",
            "\u{3099}\u{309A}",
            "\u{FF9E}\u{FF9F}",
            "・",
            "\u{309A}\u{3099}",
            "\u{309D}\u{309E}",
            "😀漢字",
        ] {
            assert!(
                matches!(respell_spacing_voiced_marks(s), Cow::Borrowed(_)),
                "{s:?} should be borrowed"
            );
            assert_eq!(respell_spacing_voiced_marks(s), s);
        }

        // The pairing is the standard's own compatibility mapping with the
        // U+0020 dropped, so it must stay in step with what NFKD says. Read
        // back out of the normalizer rather than restated.
        for &(spacing, combining) in &SPACING_VOICED_MARKS {
            assert_eq!(
                verbora_normalizers::nfkd(&spacing.to_string()),
                format!(" {combining}"),
                "U+{:04X} no longer compatibility-decomposes to SPACE + U+{:04X}",
                spacing as u32,
                combining.chars().next().expect("one mark") as u32
            );
        }
    }

    #[test]
    fn normalization_neither_expands_iteration_marks_nor_invents_readings() {
        assert_eq!(transliterate_ja_normalized("時々"), "時々");
        assert_eq!(transliterate_ja_normalized("あ々々"), "a々々");
        // U+309E and U+30FE decompose canonically to the plain mark plus
        // U+3099, which composition puts straight back: normalization is the
        // identity on all four kana iteration marks.
        assert_eq!(
            transliterate_ja_normalized("ゝゞヽヾ"),
            transliterate_ja("ゝゞヽヾ")
        );
        assert_eq!(transliterate_ja_normalized("ゝゞヽヾ"), "ゝゞヽヾ");
    }

    /// Sequential-vs-parallel parity: `par_transliterate_ja_batch` must
    /// return exactly what a sequential `.iter().map(transliterate_ja)` loop
    /// returns, element for element, over the inputs this module's own
    /// sequential tests already exercise — not a fresh set of edge cases.
    #[cfg(feature = "parallel")]
    mod parallel_parity {
        use super::*;

        /// Runs both paths over `inputs` and asserts they agree, item by item.
        fn assert_parity(inputs: &[&str]) {
            let sequential: Vec<Cow<'_, str>> =
                inputs.iter().map(|s| transliterate_ja(s)).collect();
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
            let base = [
                "とうきょう",
                "おおさか",
                "まっちゃ",
                "しんぶん",
                "ほんや",
                "バッファ",
                "ディズニー",
                "スーパー",
                "ボージョレー・ヌーヴォー",
                "ッ",
                "ー",
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
            let long_kana = "とうきょうとっきょきょかきょく".repeat(2_000);
            let long_latin = "the quick brown fox jumps over the lazy dog ".repeat(5_000);
            let mixed = [
                "😀あ😀",
                "𝕳カ𝕳",
                "カ😀ー",
                "・😀・",
                "あ!い?う。",
                "漢っか",
                "か\u{3099}",
                "あ\u{FEFF}ー",
            ];
            let mut inputs: Vec<&str> = vec![long_kana.as_str(), long_latin.as_str()];
            inputs.extend(mixed);
            assert_parity(&inputs);
        }
    }

    /// Every entry of the syllabary, romanized in parallel, must equal the
    /// sequential answer — the enumeration the other parity tests sample.
    #[cfg(feature = "parallel")]
    #[test]
    fn parallel_batch_agrees_over_the_whole_syllabary() {
        let owned: Vec<String> = syllabary().into_iter().map(|(kana, _)| kana).collect();
        let inputs: Vec<&str> = owned.iter().map(String::as_str).collect();
        let sequential: Vec<Cow<'_, str>> = inputs.iter().map(|s| transliterate_ja(s)).collect();
        assert_eq!(par_transliterate_ja_batch(&inputs), sequential);
    }
}
