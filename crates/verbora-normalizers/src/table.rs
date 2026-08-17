//! The replacement engine behind every Japanese conversion table.
//!
//! # What the reference does
//!
//! `util/utils` exposes `replacer(table)`, which joins the table's keys into
//! one alternation, compiles it with the `g` flag, and hands back
//! `str => str.replace(re, m => table[m])`. That is a single non-overlapping
//! left-to-right pass whose replacements are never rescanned, and whose match
//! preference is the reference's **leftmost-first**: the earliest-listed alternative
//! that matches at the leftmost possible position wins.
//!
//! # Why this is leftmost-longest instead
//!
//! Leftmost-first depends on the *order* of the alternation, which for the
//! derived tables (`flip`/`merge` products) is an emergent property of insertion
//! order rather than something written down anywhere. Reproducing it faithfully
//! would mean preserving that order through the port.
//!
//! It is not necessary. In every table of this cluster, no key is a proper
//! prefix of a **later** key — machine-checked when `ja/tables.rs` was generated.
//! Whenever two keys both match at a position, the longer one is therefore listed
//! first, so leftmost-first picks it: exactly leftmost-longest, which is
//! order-free. If a table is ever extended, that invariant must be re-checked.
//!
//! # Why not a regex or an Aho–Corasick automaton
//!
//! Every key is at most two `char`s long, so "longest match at this position" is
//! decided by looking at the current character and at most the next one. That
//! collapses to two binary searches over sorted static slices, behind an exact
//! two-level bitmap gate so that text made of characters no key starts with —
//! Latin prose against a katakana table, kanji against the symbol table — never
//! touches the tables at all. See [`Table::starts`] for why the gate has to be
//! exact rather than merely conservative.

use std::borrow::Cow;

/// One conversion table, in the form the scanner wants it.
///
/// Both key slices are sorted; `two` holds the keys that are two `char`s long.
/// Splitting by length rather than storing `&str` keys keeps the comparison a
/// pair of scalar compares instead of a byte-slice memcmp.
pub(crate) struct Table {
    /// Single-`char` keys and their replacements, sorted by key.
    pub(crate) one: &'static [(char, &'static str)],
    /// Two-`char` keys and their replacements, sorted by key.
    pub(crate) two: &'static [([char; 2], &'static str)],
    /// Inline bitmap over `codepoint >> 8`: which 256-codepoint blocks hold the
    /// first `char` of some key.
    ///
    /// Coarse and conservative, but it lives in the struct itself, so rejecting a
    /// character costs one shift and one test with no pointer to follow. This is
    /// the filter almost every character of almost every input hits.
    pub(crate) blocks: [u64; 4],
    /// Exact membership test for "some key starts with this `char`", as a sorted
    /// list of `(high byte, bitmap over the low byte)`, consulted only when
    /// [`Table::blocks`] does not already say no.
    ///
    /// The coarse bitmap alone was the obvious design, and it is a bad one for
    /// exactly the input this crate cares about: `NORMALIZE` has a key at U+3000,
    /// which sets the whole 0x30 block, and *every* hiragana and katakana
    /// character then falls through to two binary searches that always miss.
    /// Splitting the low byte out makes the gate exact, so a character reaches
    /// the key tables only when it genuinely begins a key.
    ///
    /// All keys are BMP, so astral characters are rejected before either test.
    /// No table has keys in more than four blocks, which is why a linear scan
    /// beats a binary search here.
    pub(crate) starts: &'static [(u8, [u64; 4])],
    /// Whether any key starts with an ASCII character.
    ///
    /// When false, pure-ASCII input cannot match and [`Table::translate`] can
    /// return it untouched after a single vectorised scan.
    pub(crate) ascii_keys: bool,
}

impl Table {
    /// Whether some key in this table starts with `c`.
    ///
    /// `pub(crate)` (not just an internal detail of [`Table::translate`])
    /// so a caller composing several tables can pre-check all of them in one
    /// pass — see `ja.rs`'s `hiragana_to_katakana`/`katakana_to_hiragana`
    /// for why that is worth doing.
    #[inline]
    pub(crate) fn may_start(&self, c: char) -> bool {
        let cp = c as u32;
        if cp > 0xFFFF {
            return false;
        }
        let hi = (cp >> 8) as u8;
        if (self.blocks[(hi >> 6) as usize] >> (hi & 63)) & 1 == 0 {
            return false;
        }
        let lo = (cp & 0xFF) as usize;
        for &(block, ref bits) in self.starts {
            if block == hi {
                return (bits[lo >> 6] >> (lo & 63)) & 1 != 0;
            }
            if block > hi {
                break; // `starts` is sorted by block.
            }
        }
        false
    }

    /// [`Table::may_start`], exposed so the table tests can assert the generated
    /// gate is exact — admitting every key's first character and nothing else.
    #[cfg(test)]
    pub(crate) fn gate_admits(&self, c: char) -> bool {
        self.may_start(c)
    }

    /// The replacement for the single-`char` key `c`, if the table has one.
    #[inline]
    fn lookup1(&self, c: char) -> Option<&'static str> {
        self.one
            .binary_search_by_key(&c, |&(k, _)| k)
            .ok()
            .map(|i| self.one[i].1)
    }

    /// The replacement for the two-`char` key `a`,`b`, if the table has one.
    #[inline]
    fn lookup2(&self, a: char, b: char) -> Option<&'static str> {
        self.two
            .binary_search_by_key(&[a, b], |&(k, _)| k)
            .ok()
            .map(|i| self.two[i].1)
    }

    /// Runs one leftmost-longest, non-overlapping replacement pass over `s`.
    ///
    /// Returns [`Cow::Borrowed`] when no key matched, which is the common case
    /// for text that is not in the script the table targets. The output buffer is
    /// only allocated at the first match, and unmatched runs are copied in bulk.
    pub(crate) fn translate<'a>(&self, s: &'a str) -> Cow<'a, str> {
        // Eleven of the fifteen tables — every fullwidth-to-halfwidth one, the
        // composite normalizer and both kana fixers — have no ASCII key at all,
        // so pure-ASCII input cannot match. `str::is_ascii` is vectorised and
        // stops at the first non-ASCII byte, so this costs almost nothing when it
        // does not fire and skips a whole character walk when it does. It matters
        // because a pipeline runs `normalize_ja` over Latin text constantly.
        if !self.ascii_keys && s.is_ascii() {
            return Cow::Borrowed(s);
        }

        let mut out: Option<String> = None;
        // Byte offset of the first character not yet copied into `out`.
        let mut copied = 0usize;
        let mut chars = s.char_indices().peekable();

        while let Some((i, c)) = chars.next() {
            if !self.may_start(c) {
                continue;
            }

            // Longest first: a two-character key beats the one-character key that
            // is its prefix. `ｳﾞ` -> `ヴ` must win over `ｳ` -> `ウ`.
            let two_hit = if self.two.is_empty() {
                None
            } else {
                chars.peek().and_then(|&(_, d)| self.lookup2(c, d))
            };
            let hit = match two_hit {
                Some(v) => Some((v, true)),
                None => self.lookup1(c).map(|v| (v, false)),
            };

            let Some((replacement, took_two)) = hit else {
                continue;
            };

            let buf = out.get_or_insert_with(|| String::with_capacity(s.len()));
            buf.push_str(&s[copied..i]);
            buf.push_str(replacement);
            copied = i + c.len_utf8();
            if took_two {
                let (j, d) = chars.next().expect("lookup2 only fires on a peeked char");
                copied = j + d.len_utf8();
            }
        }

        match out {
            Some(mut buf) => {
                buf.push_str(&s[copied..]);
                Cow::Owned(buf)
            }
            None => Cow::Borrowed(s),
        }
    }
}

/// Rewrites `s` character by character, borrowing when nothing changes.
///
/// `f` returns the replacement for a character, or `None` to keep it. Used for
/// the arithmetic kana shifts, which need no table at all.
pub(crate) fn map_chars(s: &str, f: impl Fn(char) -> Option<char>) -> Cow<'_, str> {
    let mut out: Option<String> = None;
    let mut copied = 0usize;

    for (i, c) in s.char_indices() {
        if let Some(r) = f(c) {
            let buf = out.get_or_insert_with(|| String::with_capacity(s.len()));
            buf.push_str(&s[copied..i]);
            buf.push(r);
            copied = i + c.len_utf8();
        }
    }

    match out {
        Some(mut buf) => {
            buf.push_str(&s[copied..]);
            Cow::Owned(buf)
        }
        None => Cow::Borrowed(s),
    }
}

/// Applies `f` to a [`Cow`] without giving up the borrow when neither step
/// changed anything.
///
/// The normalizers are pipelines of four or five passes over text that usually
/// needs none of them; naively chaining would allocate a `String` per stage.
pub(crate) fn map_cow<'a>(
    input: Cow<'a, str>,
    f: impl for<'b> FnOnce(&'b str) -> Cow<'b, str>,
) -> Cow<'a, str> {
    match input {
        Cow::Borrowed(s) => f(s),
        Cow::Owned(owned) => {
            // Re-borrowing `owned` inside the match keeps `f`'s temporary alive
            // only for the statement, so `owned` can be handed back untouched
            // when `f` made no change.
            let next = match f(&owned) {
                Cow::Borrowed(_) => None,
                Cow::Owned(v) => Some(v),
            };
            Cow::Owned(next.unwrap_or(owned))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Keys `a`, `b`, `ｳ`, `ab`, `ｳﾞ`.
    static SAMPLE: Table = Table {
        one: &[('a', "A"), ('b', "BB"), ('ｳ', "u")],
        two: &[(['a', 'b'], "X"), (['ｳ', 'ﾞ'], "V")],
        blocks: [1, 0, 0, 0x8000_0000_0000_0000],
        starts: &[
            // 'a' = 0x61 and 'b' = 0x62: word 0x61 >> 6 = 1, bits 33 and 34.
            (0x00, [0, (1 << 33) | (1 << 34), 0, 0]),
            // 'ｳ' = U+FF73: word 0x73 >> 6 = 1, bit 0x73 & 63 = 51.
            (0xFF, [0, 1 << 51, 0, 0]),
        ],
        ascii_keys: true,
    };

    #[test]
    fn borrows_when_nothing_matches() {
        assert!(matches!(SAMPLE.translate("zzz"), Cow::Borrowed("zzz")));
        assert!(matches!(SAMPLE.translate(""), Cow::Borrowed("")));
    }

    #[test]
    fn prefers_the_longer_key() {
        assert_eq!(SAMPLE.translate("ab"), "X");
        assert_eq!(SAMPLE.translate("ｳﾞ"), "V");
        assert_eq!(SAMPLE.translate("ｳ"), "u");
    }

    #[test]
    fn replacements_are_not_rescanned() {
        // 'b' -> "BB" must not then be re-read as two more 'b' keys.
        assert_eq!(SAMPLE.translate("b"), "BB");
    }

    #[test]
    fn matches_are_non_overlapping() {
        // "aab": the leading 'a' has no 'b' after it, so it takes the 1-char key;
        // the second 'a' then pairs with 'b'.
        assert_eq!(SAMPLE.translate("aab"), "AX");
    }

    #[test]
    fn ascii_input_short_circuits_only_when_it_is_sound() {
        static NO_ASCII_KEYS: Table = Table {
            one: &[('ｳ', "u")],
            two: &[],
            blocks: [0, 0, 0, 0x8000_0000_0000_0000],
            starts: &[(0xFF, [0, 1 << 51, 0, 0])],
            ascii_keys: false,
        };
        // The fast path fires and must not change the answer.
        assert!(matches!(NO_ASCII_KEYS.translate("plain"), Cow::Borrowed(_)));
        // SAMPLE does have ASCII keys, so it must not take the fast path.
        assert_eq!(SAMPLE.translate("plain"), "plAin");
    }

    #[test]
    fn astral_characters_are_left_alone() {
        assert_eq!(SAMPLE.translate("😀a😀"), "😀A😀");
    }

    #[test]
    fn map_cow_keeps_the_borrow_through_a_no_op_stage() {
        let out = map_cow(Cow::Borrowed("xyz"), |s| Cow::Borrowed(s));
        assert!(matches!(out, Cow::Borrowed("xyz")));
    }

    #[test]
    fn map_cow_reuses_the_owned_buffer_when_the_stage_is_a_no_op() {
        let out = map_cow(Cow::Owned(String::from("xyz")), |s| Cow::Borrowed(s));
        assert_eq!(out, "xyz");
    }

    #[test]
    fn map_chars_borrows_when_unchanged() {
        assert!(matches!(map_chars("abc", |_| None), Cow::Borrowed("abc")));
        assert_eq!(map_chars("abc", |c| (c == 'b').then_some('B')), "aBc");
    }
}
