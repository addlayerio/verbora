//! `normalizeJa` and `Converters` — the Japanese width and kana normalizer.
//!
//! Port of the reference `normalizer_ja`.
//!
//! The reference exposes a `Converters` class used purely as a namespace: it has
//! no constructor body, no fields and no state. Six prototype methods convert
//! fullwidth to halfwidth, six convert back, two convert between the kana
//! syllabaries, and three are `static`. In Rust they are all free functions in
//! [`converters`]; the prototype/static split has no observable meaning here.

use std::borrow::Cow;

use crate::table::map_cow;

mod tables;

/// U+3005 IDEOGRAPHIC ITERATION MARK, the `々` that stage one expands.
const ITERATION_MARK: char = '々';

/// Normalizes Japanese text: iteration marks, widths, kana and unit symbols.
///
/// Four stages, in this order:
///
/// 1. Expand iteration marks — `時々刻々` becomes `時時刻刻`.
/// 2. [`converters::normalize`] — fullwidth alphanumerics and symbols to
///    halfwidth, halfwidth punctuation and katakana to fullwidth.
/// 3. [`converters::fix_fullwidth_kana`] — compose base kana with a standalone
///    voiced mark, and rewrite small tsu before the n-row.
/// 4. [`converters::fix_composite_symbols`] — `㍼` becomes `昭和`.
///
/// ```
/// # use verbora_normalizers::normalize_ja;
/// assert_eq!(normalize_ja("時々刻々"), "時時刻刻");
/// assert_eq!(normalize_ja("ｶﾀｶﾅ"), "カタカナ");
/// assert_eq!(normalize_ja("ＡＢＣ１２３"), "ABC123");
/// assert_eq!(normalize_ja("まっなか"), "まんなか");
/// assert_eq!(normalize_ja("㍼54年㋃㏪"), "昭和54年4月11日");
/// ```
///
/// # Divergence on astral input
///
/// Stage one matches UTF-16 code units, so on a surrogate pair it can capture
/// half of one. `normalizeJa("😀々")` really does return `"😀"` followed by a
/// **lone low surrogate** in the reference. A Rust `String` cannot hold that, so the
/// matching is reproduced exactly over UTF-16 and only the final conversion back
/// is lossy: the unpaired surrogate becomes U+FFFD. Positions, lengths and every
/// well-formed result are unaffected. (Recorded as divergence D2 in
/// `docs/PARITY.md`, for the same reason and with the same shape.)
#[must_use]
pub fn normalize_ja(s: &str) -> Cow<'_, str> {
    let out = expand_iteration_marks(s);
    let out = map_cow(out, converters::normalize);
    let out = map_cow(out, converters::fix_fullwidth_kana);
    map_cow(out, converters::fix_composite_symbols)
}

/// Runs the two iteration-mark passes, in the order the reference runs them.
///
/// ```text
/// str.replace(/(..)々々/g, '$1$1').replace(/(.)々/g, '$1$1')
/// ```
///
/// # Why the order is load-bearing
///
/// The two-character pass fires **first**, and its output is then eligible for
/// the one-character pass. That is what makes `あ々々々` become `ああああ`:
/// the first pass rewrites it to `あ々あ々`, and the second then doubles each
/// `あ`. A single fused pass, or a recursive expansion, gives a different answer,
/// and so does running them the other way round.
///
/// It also explains the less obvious results:
/// `あい々々々々` -> `あいあいい々` (the first pass consumes the leading four
/// characters and leaves two marks that only the second pass can reach),
/// `々々` -> `々々` (the mark doubles *itself*), and `あ々々` -> `ああ々`.
///
/// # Why UTF-16
///
/// The reference's `.` matches one UTF-16 code unit, not one scalar value, and
/// excludes exactly four line terminators — `\n`, `\r`, U+2028, U+2029. It does
/// match `\t`, `\v`, `\f`, U+0085, U+00A0, U+3000 and U+FEFF, all of which a
/// Rust `char::is_whitespace`-based test would get wrong in one direction or the
/// other. Working over code units also reproduces the surrogate behaviour: for
/// `a😀々々` the two-character pass captures the whole pair and yields `a😀😀`,
/// whereas a `char`-based port would capture `a` and `😀` and yield `a😀a😀`.
fn expand_iteration_marks(s: &str) -> Cow<'_, str> {
    // U+3005 is rare, and this substring test is a memchr-class scan. Skipping
    // the UTF-16 round trip keeps the stage free for the overwhelming majority
    // of inputs, including all pure-ASCII ones.
    if !s.contains(ITERATION_MARK) {
        return Cow::Borrowed(s);
    }
    let units: Vec<u16> = s.encode_utf16().collect();
    let after_pairs = expand_pass::<2>(&units);
    match expand_pass::<1>(&after_pairs) {
        // Containing a `々` is not the same as being changed by one: `々` on its
        // own, and `時\n々`, both come through untouched. When neither pass
        // matched, hand back the original `&str` rather than re-encoding it.
        Cow::Borrowed(_) if matches!(after_pairs, Cow::Borrowed(_)) => Cow::Borrowed(s),
        changed => Cow::Owned(String::from_utf16_lossy(&changed)),
    }
}

/// One `/(.{N})々{N}/g`-shaped pass: `N` free code units followed by `N` marks,
/// replaced by the free units repeated.
///
/// Scanning is left to right and non-overlapping, and the emitted text is never
/// rescanned — the semantics of a `g`-flagged `String#replace`. Borrows when the
/// pass found nothing, which is the usual outcome for the two-unit pass: text
/// like `時々刻々` has no `々々` in it at all.
fn expand_pass<const N: usize>(units: &[u16]) -> Cow<'_, [u16]> {
    let mut out: Option<Vec<u16>> = None;
    // Index of the first code unit not yet copied into `out`.
    let mut copied = 0usize;
    let mut i = 0usize;

    while i < units.len() {
        let matched = i + 2 * N <= units.len()
            && units[i..i + N].iter().all(|&u| is_dot(u))
            && units[i + N..i + 2 * N]
                .iter()
                .all(|&u| u == ITERATION_MARK as u16);
        if !matched {
            i += 1;
            continue;
        }
        let buf = out.get_or_insert_with(|| Vec::with_capacity(units.len()));
        buf.extend_from_slice(&units[copied..i]);
        buf.extend_from_slice(&units[i..i + N]);
        buf.extend_from_slice(&units[i..i + N]);
        i += 2 * N;
        copied = i;
    }

    match out {
        Some(mut buf) => {
            buf.extend_from_slice(&units[copied..]);
            Cow::Owned(buf)
        }
        None => Cow::Borrowed(units),
    }
}

/// Whether the reference's `.` matches this code unit.
///
/// `.` matches everything except the four LineTerminators. Note that U+0085 and
/// U+FEFF *are* matched, which is where a `\s`-based approximation would drift.
#[inline]
const fn is_dot(u: u16) -> bool {
    !matches!(u, 0x000A | 0x000D | 0x2028 | 0x2029)
}

/// The width, kana and symbol converters — the reference's `Converters` class.
///
/// Every function here is a single non-overlapping left-to-right pass whose
/// replacements are never rescanned — the semantics of the reference's
/// `replacer(table)`. Its alternation is leftmost-*first*, which this crate
/// implements as leftmost-*longest*; `src/table.rs` records why the two coincide
/// for every table here and what would have to be re-checked if one changed.
pub mod converters {
    use std::borrow::Cow;

    use super::tables;
    use crate::table::{map_chars, map_cow};

    /// Fullwidth Latin letters, and U+3000, to halfwidth.
    ///
    /// ```
    /// # use verbora_normalizers::ja::converters::alphabet_fh;
    /// assert_eq!(alphabet_fh("ＡＢＣ　ＡＢＣ"), "ABC ABC");
    /// ```
    #[must_use]
    pub fn alphabet_fh(s: &str) -> Cow<'_, str> {
        tables::FH_ALPHABET.translate(s)
    }

    /// Fullwidth digits to ASCII digits.
    #[must_use]
    pub fn numbers_fh(s: &str) -> Cow<'_, str> {
        tables::FH_NUMBERS.translate(s)
    }

    /// Fullwidth symbols to their ASCII equivalents.
    ///
    /// Both U+FF0D FULLWIDTH HYPHEN-MINUS and U+2500 BOX DRAWINGS LIGHT
    /// HORIZONTAL map to `-`, which is what makes the reverse table lossy.
    ///
    /// ```
    /// # use verbora_normalizers::ja::converters::symbol_fh;
    /// assert_eq!(symbol_fh("－─"), "--");
    /// ```
    #[must_use]
    pub fn symbol_fh(s: &str) -> Cow<'_, str> {
        tables::FH_SYMBOL.translate(s)
    }

    /// Fullwidth CJK punctuation `、。・「」` to the halfwidth forms.
    #[must_use]
    pub fn pure_punctuation_fh(s: &str) -> Cow<'_, str> {
        tables::FH_PURE_PUNCTUATION.translate(s)
    }

    /// [`symbol_fh`] and [`pure_punctuation_fh`] in one pass.
    #[must_use]
    pub fn punctuation_fh(s: &str) -> Cow<'_, str> {
        tables::FH_PUNCTUATION.translate(s)
    }

    /// Fullwidth katakana to halfwidth, expanding voiced kana into two characters.
    ///
    /// ```
    /// # use verbora_normalizers::ja::converters::katakana_fh;
    /// assert_eq!(katakana_fh("ヴガパー゛゜"), "ｳﾞｶﾞﾊﾟｰﾞﾟ");
    /// ```
    #[must_use]
    pub fn katakana_fh(s: &str) -> Cow<'_, str> {
        tables::FH_KATAKANA.translate(s)
    }

    /// Halfwidth Latin letters, and the ASCII space, to fullwidth.
    ///
    /// The space really is widened: the table is the flip of [`alphabet_fh`]'s,
    /// which contains U+3000 -> U+0020.
    ///
    /// ```
    /// # use verbora_normalizers::ja::converters::alphabet_hf;
    /// assert_eq!(alphabet_hf("ABC "), "ＡＢＣ　");
    /// ```
    #[must_use]
    pub fn alphabet_hf(s: &str) -> Cow<'_, str> {
        tables::HF_ALPHABET.translate(s)
    }

    /// ASCII digits to fullwidth digits.
    #[must_use]
    pub fn numbers_hf(s: &str) -> Cow<'_, str> {
        tables::HF_NUMBERS.translate(s)
    }

    /// ASCII symbols to their fullwidth equivalents.
    ///
    /// `-` maps to U+2500, **not** U+FF0D. The reference derives this table with
    /// `flip()`, which is last-writer-wins, and U+2500 is listed second.
    ///
    /// ```
    /// # use verbora_normalizers::ja::converters::symbol_hf;
    /// assert_eq!(symbol_hf("-"), "─");
    /// ```
    #[must_use]
    pub fn symbol_hf(s: &str) -> Cow<'_, str> {
        tables::HF_SYMBOL.translate(s)
    }

    /// Halfwidth CJK punctuation `､｡･｢｣` to the fullwidth forms.
    #[must_use]
    pub fn pure_punctuation_hf(s: &str) -> Cow<'_, str> {
        tables::HF_PURE_PUNCTUATION.translate(s)
    }

    /// [`symbol_hf`] and [`pure_punctuation_hf`] in one pass.
    #[must_use]
    pub fn punctuation_hf(s: &str) -> Cow<'_, str> {
        tables::HF_PUNCTUATION.translate(s)
    }

    /// First code point of the halfwidth katakana block the dense arrays
    /// below are indexed against: U+FF66 `ｦ`. The block runs through U+FF9F
    /// `ﾟ`, 58 code points in all.
    const KHF_FIRST: u32 = 0xFF66;

    /// Fullwidth form of each halfwidth katakana, indexed by
    /// `codepoint - KHF_FIRST`. Row layout is ten entries per line, so entry
    /// `i` of line `n` is U+FF66 + 10n + i.
    ///
    /// These three arrays are the dense-array form of `HF_KATAKANA`: every
    /// code point in the block is a one-char key of that table, so the
    /// general gate-plus-binary-search scanner degenerates to an array index
    /// here. A test (`katakana_hf_dense_arrays_match_the_table`) derives all
    /// three arrays from `HF_KATAKANA` itself, so editing the table without
    /// updating them cannot drift silently.
    pub(crate) static KHF_PLAIN: [char; 58] = [
        'ヲ', 'ァ', 'ィ', 'ゥ', 'ェ', 'ォ', 'ャ', 'ュ', 'ョ', 'ッ', // U+FF66..
        'ー', 'ア', 'イ', 'ウ', 'エ', 'オ', 'カ', 'キ', 'ク', 'ケ', // U+FF70..
        'コ', 'サ', 'シ', 'ス', 'セ', 'ソ', 'タ', 'チ', 'ツ', 'テ', // U+FF7A..
        'ト', 'ナ', 'ニ', 'ヌ', 'ネ', 'ノ', 'ハ', 'ヒ', 'フ', 'ヘ', // U+FF84..
        'ホ', 'マ', 'ミ', 'ム', 'メ', 'モ', 'ヤ', 'ユ', 'ヨ', 'ラ', // U+FF8E..
        'リ', 'ル', 'レ', 'ロ', 'ワ', 'ン', '゛', '゜', // U+FF98..=U+FF9F
    ];

    /// Composed form when the halfwidth katakana is followed by `ﾞ`, or
    /// `'\0'` when no such two-char key exists (the two never compose and the
    /// mark is converted on its own). Same indexing as [`KHF_PLAIN`].
    pub(crate) static KHF_DAKUTEN: [char; 58] = [
        '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0', // U+FF66..
        '\0', '\0', '\0', 'ヴ', '\0', '\0', 'ガ', 'ギ', 'グ', 'ゲ', // U+FF70..
        'ゴ', 'ザ', 'ジ', 'ズ', 'ゼ', 'ゾ', 'ダ', 'ヂ', 'ヅ', 'デ', // U+FF7A..
        'ド', '\0', '\0', '\0', '\0', '\0', 'バ', 'ビ', 'ブ', 'ベ', // U+FF84..
        'ボ', '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0', // U+FF8E..
        '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0', // U+FF98..=U+FF9F
    ];

    /// Composed form when followed by `ﾟ` — only the five in the h-row.
    /// Same indexing and `'\0'` convention as [`KHF_DAKUTEN`].
    pub(crate) static KHF_HANDAKUTEN: [char; 58] = [
        '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0', // U+FF66..
        '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0', // U+FF70..
        '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0', // U+FF7A..
        '\0', '\0', '\0', '\0', '\0', '\0', 'パ', 'ピ', 'プ', 'ペ', // U+FF84..
        'ポ', '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0', // U+FF8E..
        '\0', '\0', '\0', '\0', '\0', '\0', '\0', '\0', // U+FF98..=U+FF9F
    ];

    /// Halfwidth katakana to fullwidth, composing base + voiced mark.
    ///
    /// Every code point of the halfwidth block U+FF66..=U+FF9F is a one-char
    /// key of `HF_KATAKANA`, so the general `Table` scanner — an exact bitmap
    /// gate plus up to two binary searches per admitted character — is pure
    /// overhead here. Instead, each block character indexes the three dense
    /// arrays above directly, and one peeked character decides whether a
    /// following `ﾞ`/`ﾟ` composes. Leftmost-longest is reproduced by
    /// construction: the composed entry is non-`'\0'` exactly where the table
    /// has a two-char key, and the plain entry is taken otherwise. Measured
    /// ~4x faster than the table walk on halfwidth katakana prose (and ~3x
    /// faster than the `kana-converter` crate's `HashMap`-per-char design).
    ///
    /// `Cow::Borrowed` is returned under exactly the table's condition: no
    /// character in the block (every block character is a key, so presence
    /// implies a rewrite).
    ///
    /// ```
    /// # use verbora_normalizers::ja::converters::katakana_hf;
    /// assert_eq!(katakana_hf("ｳﾞｶﾞﾊﾟｰﾞﾟ"), "ヴガパー゛゜");
    /// ```
    #[must_use]
    pub fn katakana_hf(s: &str) -> Cow<'_, str> {
        // Borrow until the first block character; none at all is the common
        // case for text in any other script.
        let Some((first, _)) = s
            .char_indices()
            .find(|&(_, c)| (c as u32).wrapping_sub(KHF_FIRST) < 58)
        else {
            return Cow::Borrowed(s);
        };
        // `s.len()` is exact for the common no-composition case (both widths
        // are three bytes in UTF-8) and an overestimate when pairs compose.
        let mut out = String::with_capacity(s.len());
        out.push_str(&s[..first]);
        let mut chars = s[first..].chars().peekable();
        while let Some(c) = chars.next() {
            let k = (c as u32).wrapping_sub(KHF_FIRST);
            if k < 58 {
                let idx = k as usize;
                match chars.peek() {
                    Some('ﾞ') if KHF_DAKUTEN[idx] != '\0' => {
                        out.push(KHF_DAKUTEN[idx]);
                        chars.next();
                    }
                    Some('ﾟ') if KHF_HANDAKUTEN[idx] != '\0' => {
                        out.push(KHF_HANDAKUTEN[idx]);
                        chars.next();
                    }
                    _ => out.push(KHF_PLAIN[idx]),
                }
            } else {
                out.push(c);
            }
        }
        Cow::Owned(out)
    }

    /// The composite normalization pass: stage two of [`super::normalize_ja`].
    ///
    /// Fullwidth alphanumerics, the ideographic space and fullwidth symbols go
    /// halfwidth; halfwidth punctuation and halfwidth katakana go fullwidth. It
    /// deliberately contains neither the fullwidth punctuation table nor the
    /// halfwidth-to-fullwidth alphabet, so `、。・「」` stay fullwidth and ASCII
    /// stays ASCII.
    ///
    /// ```
    /// # use verbora_normalizers::ja::converters::normalize;
    /// assert_eq!(normalize("ＡＢｶﾞ"), "ABガ");
    /// assert_eq!(normalize("ﾞ"), "゛");
    /// ```
    #[must_use]
    pub fn normalize(s: &str) -> Cow<'_, str> {
        tables::NORMALIZE.translate(s)
    }

    /// Composes base kana with a standalone voiced or semi-voiced mark.
    ///
    /// The marks handled are the **spacing** ones, U+309B `゛` and U+309C `゜`,
    /// not the combining U+3099/U+309A — the reference does not touch those.
    ///
    /// Five entries per syllabary are not diacritic fixes at all: `っな` becomes
    /// `んな` and so on through the n-row, rewriting a small tsu as an `n`. That
    /// is a phonetic change, and it is what makes `normalize_ja("まっなか")`
    /// return `"まんなか"`. It also fires inside [`hiragana_to_katakana`].
    ///
    /// ```
    /// # use verbora_normalizers::ja::converters::fix_fullwidth_kana;
    /// assert_eq!(fix_fullwidth_kana("か゛"), "が");
    /// assert_eq!(fix_fullwidth_kana("ゝ゛ヽ゛"), "ゞヾ");
    /// assert_eq!(fix_fullwidth_kana("っな"), "んな");
    /// ```
    #[must_use]
    pub fn fix_fullwidth_kana(s: &str) -> Cow<'_, str> {
        tables::FIX_FULLWIDTH_KANA.translate(s)
    }

    /// Expands single-codepoint CJK compatibility symbols to their spelled-out
    /// forms: era names, month and day numerals, point scores and unit words.
    ///
    /// ```
    /// # use verbora_normalizers::ja::converters::fix_composite_symbols;
    /// assert_eq!(fix_composite_symbols("㍼㍿㋀"), "昭和株式会社1月");
    /// assert_eq!(fix_composite_symbols("100㌫"), "100パーセント");
    /// ```
    #[must_use]
    pub fn fix_composite_symbols(s: &str) -> Cow<'_, str> {
        tables::FIX_COMPOSITE_SYMBOLS.translate(s)
    }

    /// Whether `s` has any character either [`katakana_hf`] or
    /// [`fix_fullwidth_kana`] would actually touch — halfwidth katakana or a
    /// standalone voiced/semi-voiced mark.
    ///
    /// A single combined pass over both tables' gates, used by the staged
    /// fallbacks below to skip straight to the shift stage when neither of
    /// the first two stages has anything to do. The fast paths bail to the
    /// staged form on a *superset* of the conditions this checks exactly —
    /// text whose only trigger is halfwidth punctuation (U+FF61..=U+FF65,
    /// no key starts there), or a standalone mark next to nothing voiceable
    /// — so the check still pays for itself on the fallback path. This is
    /// one `O(n)` scan; it cannot make the whole function sub-linear, but it
    /// replaces two separate [`crate::table::Table::translate`] walks (each
    /// running its own gate check, and one of them allocating a `String`
    /// buffer up front to hold nothing) with one combined gate-only scan.
    #[inline]
    fn needs_hf_or_voiced_mark_fix(s: &str) -> bool {
        s.chars()
            .any(|c| tables::HF_KATAKANA.may_start(c) || tables::FIX_FULLWIDTH_KANA.may_start(c))
    }

    /// The full three-stage pipeline behind [`hiragana_to_katakana`]: fold
    /// halfwidth katakana to fullwidth, fix standalone voiced marks
    /// (small-tsu rewrites included), then shift.
    ///
    /// [`hiragana_to_katakana`] proper is an optimistic single pass; it
    /// bails to this staged form — on the *original* input, so the answer is
    /// identical by construction — the moment it sees anything the first two
    /// stages could touch. The differential tests also use this shape as the
    /// oracle the fast path is checked against.
    ///
    /// The reference writes the shift and the two iteration marks as three
    /// separate global replaces. Fusing them is exact rather than merely
    /// convenient: `ゝ` U+309D and `ゞ` U+309E lie outside the shifted range,
    /// and their outputs `ヽ` U+30FD and `ヾ` U+30FE lie outside it too, so
    /// no pass can see another's output.
    fn hiragana_to_katakana_staged(s: &str) -> Cow<'_, str> {
        let out = if needs_hf_or_voiced_mark_fix(s) {
            let out = katakana_hf(s);
            map_cow(out, fix_fullwidth_kana)
        } else {
            Cow::Borrowed(s)
        };
        map_cow(out, |t| {
            map_chars(t, |c| match c {
                'ゝ' => Some('ヽ'),
                'ゞ' => Some('ヾ'),
                '\u{3041}'..='\u{3096}' => Some(shift(c, 0x60)),
                _ => None,
            })
        })
    }

    /// The staged pipeline behind [`katakana_to_hiragana`]; the mirror of
    /// [`hiragana_to_katakana_staged`], see there for the shape and why.
    fn katakana_to_hiragana_staged(s: &str) -> Cow<'_, str> {
        let out = if needs_hf_or_voiced_mark_fix(s) {
            let out = katakana_hf(s);
            map_cow(out, fix_fullwidth_kana)
        } else {
            Cow::Borrowed(s)
        };
        map_cow(out, |t| {
            map_chars(t, |c| match c {
                'ヽ' => Some('ゝ'),
                'ヾ' => Some('ゞ'),
                '\u{30A1}'..='\u{30F6}' => Some(shift(c, -0x60)),
                _ => None,
            })
        })
    }

    /// Converts hiragana to fullwidth katakana.
    ///
    /// Semantically three stages, exactly as the reference: fold halfwidth
    /// katakana to fullwidth, fix standalone voiced marks (small-tsu
    /// rewrites included), then shift U+3041..=U+3096 up by 0x60 and map the
    /// two iteration marks `ゝゞ` to `ヽヾ` (which are exactly 0x60 below
    /// `ヽヾ` too, so they fold into the same arithmetic).
    ///
    /// # The optimistic single pass
    ///
    /// On ordinary prose the first two stages never change anything, but
    /// confirming that used to cost two full table walks before the one pass
    /// that does real work. Instead, this runs a single pass that shifts as
    /// it goes — a prefix loop that allocates nothing until the first
    /// shiftable character, then a build loop that pushes every character
    /// into an exact-capacity `String` — while watching for the only
    /// conditions under which stage one or two could fire:
    ///
    /// * any character in U+FF61..=U+FF9F (a superset of the halfwidth
    ///   katakana keys, which all live in U+FF66..=U+FF9F);
    /// * a standalone `゛` U+309B or `゜` U+309C (every voiced-mark key ends
    ///   in one);
    /// * `っ` directly before `な`..`の`, or `ッ` before `ナ`..`ノ` (the
    ///   small-tsu keys, tracked exactly with one byte of state).
    ///
    /// On the first trigger the optimistic build is discarded and the staged
    /// pipeline runs on the original input, so the result — including
    /// whether the `Cow` borrows — is identical by construction. A test
    /// derives the trigger set from the tables themselves, so extending
    /// `HF_KATAKANA` or `FIX_FULLWIDTH_KANA` without widening the triggers
    /// cannot silently break parity.
    ///
    /// # The measured trade-off
    ///
    /// Every common shape wins: pure kana, small-tsu prose with non-n-row
    /// followers, mixed ASCII and kana are 4-6x faster than the staged
    /// pipeline (and faster than a bare `+0x60` map, because the exact
    /// `with_capacity` avoids the reallocation a `collect` undershoots
    /// into); kanji-only borrows either way; an early trigger bails
    /// immediately at staged cost. The one bounded regression is a trigger
    /// appearing only near the *end* of otherwise pure kana: the nearly
    /// complete build is discarded, costing up to ~1.24x the staged pipeline
    /// (never more than one extra scan-and-build, since the pass abandons at
    /// the first trigger).
    ///
    /// ```
    /// # use verbora_normalizers::ja::converters::hiragana_to_katakana;
    /// assert_eq!(hiragana_to_katakana("ぁゖゝゞっなあいうえお"), "ァヶヽヾンナアイウエオ");
    /// assert_eq!(hiragana_to_katakana("abc123 漢字 ｶ"), "abc123 漢字 カ");
    /// ```
    #[must_use]
    pub fn hiragana_to_katakana(s: &str) -> Cow<'_, str> {
        // Prefix loop: trigger-checks only, no allocation, until the first
        // shiftable character decides the output cannot be borrowed.
        // `prev_tsu` is 1 after `っ`, 2 after `ッ`, 0 otherwise.
        let mut prev_tsu = 0u8;
        let mut it = s.char_indices();
        let (start, first) = loop {
            let Some((i, c)) = it.next() else {
                return Cow::Borrowed(s);
            };
            let cp = c as u32;
            if cp.wrapping_sub(0x3041) <= 0x55 {
                // U+3041..=U+3096, shiftable — unless it completes a
                // small-tsu pair, which is stage two's job.
                if prev_tsu == 1 && cp.wrapping_sub(0x306A) <= 4 {
                    return hiragana_to_katakana_staged(s);
                }
                break (i, c);
            }
            if cp.wrapping_sub(0x309B) <= 3 {
                if cp <= 0x309C {
                    return hiragana_to_katakana_staged(s); // ゛ or ゜
                }
                break (i, c); // ゝ or ゞ: shiftable by the same +0x60.
            }
            if cp.wrapping_sub(0xFF61) <= 0x3E {
                return hiragana_to_katakana_staged(s); // halfwidth block
            }
            if prev_tsu == 2 && cp.wrapping_sub(0x30CA) <= 4 {
                return hiragana_to_katakana_staged(s); // ッ + ナ..ノ
            }
            prev_tsu = match cp {
                0x3063 => 1, // っ
                0x30C3 => 2, // ッ
                _ => 0,
            };
        };
        // Build loop: push *every* character (run-copy bookkeeping measured
        // slower than the straight-line push), still watching for triggers.
        // Kana in and out are three bytes each, so `s.len()` is exact.
        let mut out = String::with_capacity(s.len());
        out.push_str(&s[..start]);
        prev_tsu = u8::from(first == 'っ');
        out.push(shift(first, 0x60));
        for (_, c) in it {
            let cp = c as u32;
            if cp.wrapping_sub(0x3041) <= 0x55 {
                if prev_tsu == 1 && cp.wrapping_sub(0x306A) <= 4 {
                    return hiragana_to_katakana_staged(s);
                }
                prev_tsu = u8::from(cp == 0x3063);
                out.push(shift(c, 0x60));
                continue;
            }
            if cp.wrapping_sub(0x309B) <= 3 {
                if cp <= 0x309C {
                    return hiragana_to_katakana_staged(s);
                }
                prev_tsu = 0;
                out.push(shift(c, 0x60));
                continue;
            }
            if cp.wrapping_sub(0xFF61) <= 0x3E {
                return hiragana_to_katakana_staged(s);
            }
            if prev_tsu == 2 && cp.wrapping_sub(0x30CA) <= 4 {
                return hiragana_to_katakana_staged(s);
            }
            prev_tsu = if cp == 0x30C3 { 2 } else { 0 };
            out.push(c);
        }
        Cow::Owned(out)
    }

    /// Converts katakana to hiragana: the mirror of [`hiragana_to_katakana`].
    ///
    /// The shifted range is U+30A1..=U+30F6, so `ヷヸヹヺ` (U+30F7..=U+30FA),
    /// `ー` U+30FC and the halfwidth forms all fall outside it and pass through.
    ///
    /// Same optimistic single pass, same trigger set and same bounded
    /// worst case as [`hiragana_to_katakana`] — see that function's own doc
    /// comment.
    ///
    /// ```
    /// # use verbora_normalizers::ja::converters::katakana_to_hiragana;
    /// assert_eq!(katakana_to_hiragana("ァヶヽヾッナアイウエオ"), "ぁゖゝゞんなあいうえお");
    /// assert_eq!(katakana_to_hiragana("ヷヸヹヺー"), "ヷヸヹヺー");
    /// ```
    #[must_use]
    pub fn katakana_to_hiragana(s: &str) -> Cow<'_, str> {
        let mut prev_tsu = 0u8;
        let mut it = s.char_indices();
        let (start, first) = loop {
            let Some((i, c)) = it.next() else {
                return Cow::Borrowed(s);
            };
            let cp = c as u32;
            if cp.wrapping_sub(0x30A1) <= 0x55 {
                // U+30A1..=U+30F6, shiftable — unless it completes ッ+ナ..ノ.
                if prev_tsu == 2 && cp.wrapping_sub(0x30CA) <= 4 {
                    return katakana_to_hiragana_staged(s);
                }
                break (i, c);
            }
            if cp.wrapping_sub(0x30FD) <= 1 {
                break (i, c); // ヽ or ヾ: shiftable by the same -0x60.
            }
            if cp == 0x309B || cp == 0x309C {
                return katakana_to_hiragana_staged(s); // ゛ or ゜
            }
            if cp.wrapping_sub(0xFF61) <= 0x3E {
                return katakana_to_hiragana_staged(s); // halfwidth block
            }
            if prev_tsu == 1 && cp.wrapping_sub(0x306A) <= 4 {
                return katakana_to_hiragana_staged(s); // っ + な..の
            }
            prev_tsu = match cp {
                0x3063 => 1, // っ
                0x30C3 => 2, // ッ
                _ => 0,
            };
        };
        let mut out = String::with_capacity(s.len());
        out.push_str(&s[..start]);
        prev_tsu = if first == 'ッ' { 2 } else { 0 };
        out.push(shift(first, -0x60));
        for (_, c) in it {
            let cp = c as u32;
            if cp.wrapping_sub(0x30A1) <= 0x55 {
                if prev_tsu == 2 && cp.wrapping_sub(0x30CA) <= 4 {
                    return katakana_to_hiragana_staged(s);
                }
                prev_tsu = if cp == 0x30C3 { 2 } else { 0 };
                out.push(shift(c, -0x60));
                continue;
            }
            if cp.wrapping_sub(0x30FD) <= 1 {
                prev_tsu = 0;
                out.push(shift(c, -0x60));
                continue;
            }
            if cp == 0x309B || cp == 0x309C {
                return katakana_to_hiragana_staged(s);
            }
            if cp.wrapping_sub(0xFF61) <= 0x3E {
                return katakana_to_hiragana_staged(s);
            }
            if prev_tsu == 1 && cp.wrapping_sub(0x306A) <= 4 {
                return katakana_to_hiragana_staged(s);
            }
            prev_tsu = u8::from(cp == 0x3063);
            out.push(c);
        }
        Cow::Owned(out)
    }

    /// Shifts a kana by `delta` code points.
    ///
    /// The caller has already range-checked `c`; both kana blocks are fully
    /// assigned, so the conversion back to `char` cannot fail.
    #[inline]
    fn shift(c: char, delta: i32) -> char {
        let shifted = (c as i32 + delta) as u32;
        char::from_u32(shifted).expect("both kana blocks contain only valid scalar values")
    }
}

#[cfg(test)]
mod tests {
    use super::converters::*;
    use super::*;

    #[test]
    fn empty_and_single_characters() {
        assert_eq!(normalize_ja(""), "");
        assert_eq!(normalize_ja("a"), "a");
        assert_eq!(normalize_ja("々"), "々");
        assert_eq!(alphabet_fh(""), "");
        assert_eq!(katakana_hf(""), "");
        assert_eq!(hiragana_to_katakana(""), "");
    }

    #[test]
    fn iteration_marks_expand_in_the_documented_order() {
        assert_eq!(normalize_ja("時々刻々"), "時時刻刻");
        assert_eq!(normalize_ja("甲斐々々しい"), "甲斐甲斐しい");
        assert_eq!(normalize_ja("々"), "々");
        assert_eq!(normalize_ja("々々"), "々々");
        assert_eq!(normalize_ja("々々々"), "々々々");
        assert_eq!(normalize_ja("々々々々"), "々々々々");
        assert_eq!(normalize_ja("あ々々"), "ああ々");
        assert_eq!(normalize_ja("あ々々々"), "ああああ");
        assert_eq!(normalize_ja("あ々々々々"), "ああああ々");
        assert_eq!(normalize_ja("あい々々"), "あいあい");
        assert_eq!(normalize_ja("あいう々々"), "あいういう");
        assert_eq!(normalize_ja("あい々々々々"), "あいあいい々");
        assert_eq!(normalize_ja("々々a々々"), "々々aaa");
    }

    #[test]
    fn the_dot_excludes_exactly_four_line_terminators() {
        for terminator in ["\n", "\r", "\u{2028}", "\u{2029}"] {
            let input = format!("{terminator}々");
            assert_eq!(normalize_ja(&input), input, "for {terminator:?}");
        }
        // …and matches these, which a `\s`-based test would get wrong.
        for matched in ["\t", "\u{0B}", "\u{0C}", "\u{0085}", "\u{00A0}", "\u{FEFF}"] {
            let input = format!("{matched}々");
            assert_eq!(normalize_ja(&input), format!("{matched}{matched}"));
        }
    }

    #[test]
    fn astral_input_follows_utf16_matching() {
        // The pair is captured whole by the two-character pass.
        assert_eq!(normalize_ja("😀々々"), "😀😀");
        assert_eq!(normalize_ja("a😀々々"), "a😀😀");
        // The one-character pass captures only the low half; the reference emits a
        // lone surrogate there, which Rust necessarily renders as U+FFFD.
        assert_eq!(normalize_ja("😀々"), "😀\u{FFFD}");
    }

    #[test]
    fn spec_fixtures_from_the_shipped_jasmine_suite() {
        assert_eq!(
            normalize_ja("う゛か゛き゛く゛は゜ひ゜ふ゜"),
            "ゔがぎぐぱぴぷ"
        );
        assert_eq!(normalize_ja("うﾞかﾞきﾞくﾞはﾟひﾟふﾟ"), "ゔがぎぐぱぴぷ");
        assert_eq!(normalize_ja("まっなか"), "まんなか");
        assert_eq!(
            normalize_ja("ウ゛カ゛キ゛ク゛ハ゜ヒ゜フ゜"),
            "ヴガギグパピプ"
        );
        assert_eq!(normalize_ja("ｳ゛ｶ゛ｷ゛ｸ゛ﾊ゜ﾋ゜ﾌ゜"), "ヴガギグパピプ");
        assert_eq!(normalize_ja("ｳﾞｶﾞｷﾞｸﾞﾊﾟﾋﾟﾌﾟ"), "ヴガギグパピプ");
        assert_eq!(normalize_ja("ｶﾀｶﾅ"), "カタカナ");
        assert_eq!(normalize_ja("ＡＢＣ１２３"), "ABC123");
        assert_eq!(normalize_ja("空　空　空"), "空 空 空");
        assert_eq!(
            normalize_ja("〜 ・ ･ 、､ 。｡ 「｢ 」｣"),
            "〜 ・ ・ 、、 。。 「「 」」"
        );
        assert_eq!(normalize_ja("㍼54年㋃㏪"), "昭和54年4月11日");
        assert_eq!(normalize_ja("㍧〜㍬"), "15点〜20点");
        assert_eq!(normalize_ja("カンパニー㍿"), "カンパニー株式会社");
        assert_eq!(normalize_ja("100㌫"), "100パーセント");
        assert_eq!(normalize_ja("70㌔"), "70キロ");
        assert_eq!(normalize_ja("㍇"), "マンション");
    }

    #[test]
    fn tildes_are_deliberately_never_converted() {
        assert_eq!(normalize_ja("～〜"), "～〜");
        assert_eq!(symbol_fh("～〜"), "～〜");
    }

    #[test]
    fn the_flip_collision_is_reproduced() {
        assert_eq!(symbol_hf("-"), "─");
        assert_eq!(punctuation_hf("."), "．");
    }

    #[test]
    fn alphabet_hf_widens_the_ascii_space() {
        assert_eq!(alphabet_hf("a b"), "ａ　ｂ");
    }

    #[test]
    fn kana_shifts_respect_their_range_bounds() {
        assert_eq!(hiragana_to_katakana("ゕゖ"), "ヵヶ");
        assert_eq!(hiragana_to_katakana("ゔ"), "ヴ");
        assert_eq!(hiragana_to_katakana("カタカナ"), "カタカナ");
        assert_eq!(katakana_to_hiragana("ヴ"), "ゔ");
        assert_eq!(katakana_to_hiragana("かたかな"), "かたかな");
        assert_eq!(katakana_to_hiragana("ヷヸヹヺ"), "ヷヸヹヺ");
        assert_eq!(katakana_to_hiragana("ー"), "ー");
        // U+309F ゟ and U+30FF ヿ are outside both ranges.
        assert_eq!(hiragana_to_katakana("ゟ"), "ゟ");
        assert_eq!(katakana_to_hiragana("ヿ"), "ヿ");
    }

    #[test]
    fn kana_round_trip() {
        assert_eq!(
            katakana_to_hiragana(&hiragana_to_katakana("こんにちは")),
            "こんにちは"
        );
    }

    #[test]
    fn other_scripts_pass_through_every_converter() {
        let probes = [
            "",
            "abc",
            "ALLCAPS",
            "café",
            "Москва",
            "Ελλάδα",
            "漢字",
            "😀",
            "123",
            "!?.",
        ];
        for p in probes {
            assert_eq!(katakana_fh(p), p, "katakana_fh({p:?})");
            assert_eq!(pure_punctuation_fh(p), p, "pure_punctuation_fh({p:?})");
            assert_eq!(fix_fullwidth_kana(p), p, "fix_fullwidth_kana({p:?})");
            assert_eq!(fix_composite_symbols(p), p, "fix_composite_symbols({p:?})");
        }
    }

    #[test]
    fn unchanged_input_is_returned_borrowed() {
        assert!(matches!(normalize_ja("plain ascii"), Cow::Borrowed(_)));
        assert!(matches!(normalize_ja("漢字だけ"), Cow::Borrowed(_)));
        assert!(matches!(katakana_hf("Москва"), Cow::Borrowed(_)));
    }

    #[test]
    fn very_long_input() {
        let input = "ｶﾀｶﾅ".repeat(5_000);
        assert_eq!(normalize_ja(&input), "カタカナ".repeat(5_000));
    }

    #[test]
    fn tables_are_sorted_and_within_the_bmp() {
        let all = [
            &tables::FH_ALPHABET,
            &tables::FH_NUMBERS,
            &tables::FH_SYMBOL,
            &tables::FH_PURE_PUNCTUATION,
            &tables::FH_PUNCTUATION,
            &tables::FH_KATAKANA,
            &tables::HF_ALPHABET,
            &tables::HF_NUMBERS,
            &tables::HF_SYMBOL,
            &tables::HF_PURE_PUNCTUATION,
            &tables::HF_PUNCTUATION,
            &tables::HF_KATAKANA,
            &tables::NORMALIZE,
            &tables::FIX_FULLWIDTH_KANA,
            &tables::FIX_COMPOSITE_SYMBOLS,
        ];
        for t in all {
            assert!(t.one.windows(2).all(|w| w[0].0 < w[1].0));
            assert!(t.two.windows(2).all(|w| w[0].0 < w[1].0));
            assert!(t.starts.windows(2).all(|w| w[0].0 < w[1].0));
            // The gate must admit every key's first character and — since it is
            // exact, not conservative — reject everything else. A generated gate
            // that is merely *nearly* right would show up as a silent parity
            // break rather than a compile error, so it is checked here.
            assert!(t.one.iter().all(|&(k, _)| t.gate_admits(k)));
            assert!(t.two.iter().all(|&(k, _)| t.gate_admits(k[0])));
            let key_starts: std::collections::BTreeSet<char> = t
                .one
                .iter()
                .map(|&(k, _)| k)
                .chain(t.two.iter().map(|&(k, _)| k[0]))
                .collect();
            let admitted = (0u32..=0xFFFF)
                .filter_map(char::from_u32)
                .filter(|&c| t.gate_admits(c))
                .count();
            assert_eq!(admitted, key_starts.len());
            assert_eq!(t.ascii_keys, key_starts.iter().any(char::is_ascii));
        }
        // The counts the spec records, including the two the `flip()` collision
        // shrinks (32 instead of 33, and 37 instead of 38).
        assert_eq!(tables::FH_SYMBOL.one.len(), 33);
        assert_eq!(tables::HF_SYMBOL.one.len(), 32);
        assert_eq!(tables::FH_PUNCTUATION.one.len(), 38);
        assert_eq!(tables::HF_PUNCTUATION.one.len(), 37);
        assert_eq!(
            tables::HF_KATAKANA.one.len() + tables::HF_KATAKANA.two.len(),
            84
        );
        assert_eq!(
            tables::NORMALIZE.one.len() + tables::NORMALIZE.two.len(),
            185
        );
        assert_eq!(tables::FIX_FULLWIDTH_KANA.two.len(), 64);
        assert_eq!(tables::FIX_COMPOSITE_SYMBOLS.one.len(), 161);
    }

    // -----------------------------------------------------------------------
    // The `needs_hf_or_voiced_mark_fix` pre-check vs. always running both
    // early stages unconditionally
    // -----------------------------------------------------------------------

    /// `hiragana_to_katakana` before the `docs/PERFORMANCE_GAPS.md` entry 30
    /// pre-check: always run both early stages, never skip them. Kept only
    /// as the reference the differential test below checks the real
    /// implementation against.
    fn hiragana_to_katakana_naive(s: &str) -> Cow<'_, str> {
        let out = katakana_hf(s);
        let out = crate::table::map_cow(out, fix_fullwidth_kana);
        crate::table::map_cow(out, |t| {
            crate::table::map_chars(t, |c| match c {
                'ゝ' => Some('ヽ'),
                'ゞ' => Some('ヾ'),
                '\u{3041}'..='\u{3096}' => char::from_u32(c as u32 + 0x60),
                _ => None,
            })
        })
    }

    #[test]
    fn hiragana_to_katakana_pre_check_matches_always_running_both_stages() {
        // Deliberately mixes: pure hiragana (the pre-check's own skip
        // target), halfwidth katakana (must still run `katakana_hf`),
        // standalone voiced marks (must still run `fix_fullwidth_kana`),
        // small-tsu-before-n-row (a `fix_fullwidth_kana` case that is also a
        // phonetic change, not just a diacritic fix), CJK/ASCII (inert
        // either way), and combinations of all of them.
        let cases = [
            "",
            "あいうえお",
            "かきくけこ",
            "まっなか",
            "ｶﾞｷﾞｸﾞｹﾞｺﾞ",
            "か゛き゛",
            "abc123 漢字",
            "あｶﾞい",
            "ゝゞ",
            "っなあ",
            "ぁゖゝゞっなあいうえお",
        ];
        for s in cases {
            assert_eq!(
                hiragana_to_katakana(s),
                hiragana_to_katakana_naive(s),
                "mismatch for {s:?}"
            );
        }
    }

    #[test]
    fn hiragana_to_katakana_pre_check_matches_always_running_both_stages_randomized() {
        // A tiny, dependency-free PRNG -- this crate has no `rand`
        // dependency and does not need one just for this.
        struct Xorshift64(u64);
        impl Xorshift64 {
            fn next_u64(&mut self) -> u64 {
                let mut x = self.0;
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                self.0 = x;
                x
            }
            fn next_range(&mut self, bound: usize) -> usize {
                (self.next_u64() % bound as u64) as usize
            }
        }

        const ALPHABET: [char; 14] = [
            'あ', 'か', 'さ', 'た', 'な', 'っ', 'ゝ', 'ゞ', 'ｶ', 'ﾞ', '゛', '゜', 'ー', 'a',
        ];
        let mut rng = Xorshift64(0xABCD_EF01_2345_6789);
        for round in 0..500 {
            let len = rng.next_range(8);
            let s: String = (0..len)
                .map(|_| ALPHABET[rng.next_range(ALPHABET.len())])
                .collect();
            assert_eq!(
                hiragana_to_katakana(&s),
                hiragana_to_katakana_naive(&s),
                "round {round}: {s:?}"
            );
        }
    }

    // -----------------------------------------------------------------------
    // The optimistic single-pass converters and the dense-array katakana_hf
    // vs. the table-driven pipeline as oracle
    // -----------------------------------------------------------------------

    /// The staged pipeline built from the tables alone — independent of both
    /// the optimistic fast paths and the dense-array `katakana_hf` — so the
    /// differential tests below check the shipped functions against the
    /// original table-driven implementation, not against themselves.
    fn h2k_oracle(s: &str) -> Cow<'_, str> {
        let out = tables::HF_KATAKANA.translate(s);
        let out = crate::table::map_cow(out, fix_fullwidth_kana);
        crate::table::map_cow(out, |t| {
            crate::table::map_chars(t, |c| match c {
                'ゝ' => Some('ヽ'),
                'ゞ' => Some('ヾ'),
                '\u{3041}'..='\u{3096}' => char::from_u32(c as u32 + 0x60),
                _ => None,
            })
        })
    }

    /// Mirror of [`h2k_oracle`] for the katakana-to-hiragana direction.
    fn k2h_oracle(s: &str) -> Cow<'_, str> {
        let out = tables::HF_KATAKANA.translate(s);
        let out = crate::table::map_cow(out, fix_fullwidth_kana);
        crate::table::map_cow(out, |t| {
            crate::table::map_chars(t, |c| match c {
                'ヽ' => Some('ゝ'),
                'ヾ' => Some('ゞ'),
                '\u{30A1}'..='\u{30F6}' => char::from_u32(c as u32 - 0x60),
                _ => None,
            })
        })
    }

    /// Asserts all three rewritten converters agree with their table-driven
    /// oracles on `input`: byte-exact output *and* the same `Cow` kind, since
    /// whether the input is borrowed back is observable behaviour.
    fn assert_parity(input: &str) {
        let checks = [
            (
                "hiragana_to_katakana",
                h2k_oracle(input),
                hiragana_to_katakana(input),
            ),
            (
                "katakana_to_hiragana",
                k2h_oracle(input),
                katakana_to_hiragana(input),
            ),
            (
                "katakana_hf",
                tables::HF_KATAKANA.translate(input),
                katakana_hf(input),
            ),
        ];
        for (name, oracle, fast) in checks {
            assert_eq!(fast, oracle, "{name} value for {input:?}");
            assert_eq!(
                matches!(fast, Cow::Borrowed(_)),
                matches!(oracle, Cow::Borrowed(_)),
                "{name} Cow kind for {input:?}"
            );
        }
    }

    /// A character can slip past the fast paths' trigger constants only if
    /// the tables gain a key those constants do not cover — which would be a
    /// silent parity break, not a compile error. This derives the required
    /// trigger set from the tables and asserts the constants are a superset,
    /// in the same spirit as the gate-exactness assertions in
    /// `tables_are_sorted_and_within_the_bmp`.
    #[test]
    fn fast_path_trigger_set_is_a_superset_of_the_tables() {
        // Mirrors of the constants in `hiragana_to_katakana` /
        // `katakana_to_hiragana`.
        let standalone = |c: char| {
            let cp = c as u32;
            cp.wrapping_sub(0xFF61) <= 0x3E || cp == 0x309B || cp == 0x309C
        };
        let pair = |a: char, b: char| {
            (a == 'っ' && ('な'..='の').contains(&b)) || (a == 'ッ' && ('ナ'..='ノ').contains(&b))
        };
        // Stage one fires only when an `HF_KATAKANA` key is present, so every
        // character of every key must itself be a standalone trigger.
        for &(k, _) in tables::HF_KATAKANA.one {
            assert!(standalone(k), "{k:?} escapes the trigger set");
        }
        for &(k, _) in tables::HF_KATAKANA.two {
            assert!(
                standalone(k[0]) && standalone(k[1]),
                "{k:?} escapes the trigger set"
            );
        }
        // Stage two has only two-char keys, and each must either end in a
        // mark (its mere presence anywhere triggers the fallback) or be one
        // of the small-tsu pairs the fast paths track exactly by adjacency.
        assert!(tables::FIX_FULLWIDTH_KANA.one.is_empty());
        for &(k, _) in tables::FIX_FULLWIDTH_KANA.two {
            assert!(
                standalone(k[1]) || pair(k[0], k[1]),
                "{k:?} escapes the trigger set"
            );
        }
    }

    /// The dense arrays must stay exactly derivable from `HF_KATAKANA`:
    /// one-char keys are the contiguous block U+FF66..=U+FF9F with one-char
    /// replacements, and the composed entries exist precisely where the
    /// table has a two-char key.
    #[test]
    fn katakana_hf_dense_arrays_are_derived_from_the_table() {
        assert_eq!(tables::HF_KATAKANA.one.len(), 58);
        for (i, &(k, v)) in tables::HF_KATAKANA.one.iter().enumerate() {
            assert_eq!(k as u32, 0xFF66 + i as u32, "one-char keys must be dense");
            let mut chars = v.chars();
            assert_eq!((chars.next(), chars.next()), (Some(KHF_PLAIN[i]), None));
        }
        let mut dakuten = ['\0'; 58];
        let mut handakuten = ['\0'; 58];
        for &(k, v) in tables::HF_KATAKANA.two {
            let idx = (k[0] as u32 - 0xFF66) as usize;
            let mut chars = v.chars();
            let composed = chars.next().expect("composed replacement is non-empty");
            assert_eq!(chars.next(), None, "composed replacements are single chars");
            match k[1] {
                'ﾞ' => dakuten[idx] = composed,
                'ﾟ' => handakuten[idx] = composed,
                other => panic!("unexpected second key char {other:?}"),
            }
        }
        assert_eq!(dakuten, KHF_DAKUTEN);
        assert_eq!(handakuten, KHF_HANDAKUTEN);
    }

    #[test]
    fn fast_paths_match_the_table_oracle_on_fixtures() {
        let mut cases: Vec<String> = [
            "",
            "ぁゖゝゞっなあいうえお",
            "abc123 漢字 ｶ",
            "ァヶヽヾッナアイウエオ",
            "ヷヸヹヺー",
            "ゕゖ",
            "ゔ",
            "カタカナ",
            "かたかな",
            "ー",
            "ゟ",
            "ヿ",
            "こんにちは",
            "ｳﾞｶﾞﾊﾟｰﾞﾟ",
            "ｼﾝｸﾞﾙﾊﾞｲﾄｶﾅ",
            "か゛き゛",
            "まっなか",
            "っな",
            "ッナ",
            "っ",
            "ッ",
            "゛",
            "゜",
            "ﾞ",
            "ﾟﾟﾞ",
            "ｦﾞﾜﾞ",
            "Москва",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect();
        // The bench pangrams, and the bounded worst case the doc comment
        // records: a trigger appearing only at the very end of otherwise
        // pure kana, which discards a nearly complete optimistic build.
        let iroha_hira = "いろはにほへとちりぬるをわかよたれそつねならむうゐのおくやまけふこえてあさきゆめみしゑひもせす";
        let iroha_kata = "イロハニホヘトチリヌルヲワカヨタレソツネナラムウヰノオクヤマケフコエテアサキユメミシヱヒモセス";
        cases.push(iroha_hira.repeat(2));
        cases.push(iroha_kata.repeat(2));
        cases.push("ｼﾝｸﾞﾙﾊﾞｲﾄｶﾅｶﾀｶﾅｶﾞｷﾞｸﾞｹﾞｺﾞｻﾞｼﾞｽﾞｾﾞｿﾞ".repeat(2));
        cases.push(format!("{}゛", iroha_hira.repeat(4)));
        cases.push(format!("{}ｶ", iroha_kata.repeat(4)));
        cases.push(format!("{}っな", iroha_hira.repeat(4)));
        for s in &cases {
            assert_parity(s);
        }
    }

    #[test]
    fn fast_paths_match_the_table_oracle_randomized() {
        struct Xorshift(u64);
        impl Xorshift {
            fn next_u64(&mut self) -> u64 {
                let mut x = self.0;
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                self.0 = x;
                x
            }
            fn next_range(&mut self, bound: usize) -> usize {
                (self.next_u64() % bound as u64) as usize
            }
        }

        // Deliberately rich in edge cases: hiragana incl. っ/ゝ/ゞ and the
        // n-row, katakana incl. ッ/ヽ/ヾ and the n-row, both shift-range
        // boundaries and their outside neighbours, the standalone spacing
        // marks AND the combining ones (which must pass through), halfwidth
        // katakana incl. ﾞ/ﾟ and the pair-less ｦ/ﾜ, halfwidth punctuation
        // (U+FF61..=U+FF65 — triggers that are not keys), ASCII, kanji and
        // astral.
        const ALPHABET: &[char] = &[
            'あ', 'い', 'か', 'は', 'ひ', 'ほ', 'っ', 'つ', 'な', 'に', 'の', 'ん', 'ゝ', 'ゞ',
            'ゔ', '\u{3041}', '\u{3096}', '\u{3040}', '\u{3097}', 'ゟ', 'ア', 'カ', 'ハ', 'ッ',
            'ツ', 'ナ', 'ノ', 'ン', 'ヽ', 'ヾ', 'ヴ', '\u{30A1}', '\u{30F6}', '\u{30F7}',
            '\u{30FA}', '\u{30FB}', '\u{30FC}', 'ヿ', '゛', '゜', '\u{3099}', '\u{309A}', 'ｱ', 'ｳ',
            'ｶ', 'ﾊ', 'ﾎ', 'ｦ', 'ﾜ', 'ﾝ', 'ﾞ', 'ﾟ', 'ｧ', 'ｯ', 'ｰ', '｡', '｢', '｣', '､', '･', 'a', 'Z',
            '1', ' ', '.', '漢', '字', '中', '😀', 'é', 'Ω',
        ];

        let mut rng = Xorshift(0x1234_5678_9ABC_DEF1);
        for _ in 0..6_000 {
            let len = rng.next_range(24);
            let s: String = (0..len)
                .map(|_| ALPHABET[rng.next_range(ALPHABET.len())])
                .collect();
            assert_parity(&s);
        }
        // Second seed, longer strings — crosses the with_capacity and
        // discarded-build paths that short strings cannot reach.
        let mut rng = Xorshift(0xFEED_FACE_CAFE_BEEF);
        for _ in 0..1_500 {
            let len = 24 + rng.next_range(200);
            let s: String = (0..len)
                .map(|_| ALPHABET[rng.next_range(ALPHABET.len())])
                .collect();
            assert_parity(&s);
        }
    }

    /// Every code point in the kana blocks and around the halfwidth block,
    /// alone and next to every pair-relevant leader and follower — the
    /// exhaustive form of the boundary conditions the fast paths encode as
    /// `wrapping_sub` range compares.
    #[test]
    fn fast_paths_match_the_table_oracle_on_exhaustive_block_sweep() {
        let followers = [
            '゛', '゜', 'な', 'の', 'に', 'ナ', 'ノ', 'ﾞ', 'ﾟ', 'あ', 'ア', 'a',
        ];
        let leaders = ['っ', 'ッ', 'つ', 'ツ', 'ゝ', 'ヽ', 'ｳ', 'ﾊ', 'a'];
        for cp in (0x3000u32..=0x3100).chain(0xFF5F..=0xFFA0) {
            let Some(c) = char::from_u32(cp) else {
                continue;
            };
            assert_parity(&c.to_string());
            for f in followers {
                assert_parity(&format!("{c}{f}"));
            }
            for l in leaders {
                assert_parity(&format!("{l}{c}"));
            }
            assert_parity(&format!("あ{c}ア{c}ｱ"));
        }
    }
}
