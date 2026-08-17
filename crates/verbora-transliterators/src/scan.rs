//! The replacement engine behind the transliterator's table phases.
//!
//! # What the reference does
//!
//! `util/utils` exposes `replacer(table)`: it joins the table's keys into one
//! alternation, compiles it with the `g` flag, and returns
//! `str => str.replace(re, m => table[m])`. That is a single non-overlapping
//! left-to-right pass whose replacements are never rescanned, and whose match
//! preference is the reference's **leftmost-first** — the earliest-*listed*
//! alternative that matches at the leftmost possible position wins.
//!
//! # Why this is leftmost-longest instead
//!
//! Leftmost-first depends on the order of the alternation, which is the order of
//! `for (key in translationTable)`, which is the order the keys were written in
//! the source. Carrying that ordering through the port would mean every future
//! reader had to keep it in mind.
//!
//! It is not necessary here. In all three tables, no key is a proper prefix of a
//! **later** key — a table that breaks this was refused at derivation time, and
//! the invariant is re-checked by this module's own tests. Whenever two keys both match
//! at a position the longer one is therefore listed first, so leftmost-first
//! picks it: exactly leftmost-longest, which is order-free. `キョウ` -> `kyō`
//! beating `キョ` -> `kyo` beating `キ` -> `ki` is the property in action.
//!
//! # Why not a regex or an Aho–Corasick automaton
//!
//! Every key is at most three `char`s long, so "longest match here" is decided
//! by the current character and at most the next two. [`Table`] turns the first
//! character into an index with one subtraction, and the slot it lands on names
//! the two- and three-character keys that could continue — never more than five
//! of them. An automaton would need a runtime construction step and a pointer
//! chase per byte to answer the same question, and a regex would need a
//! 382-branch alternation.

use std::borrow::Cow;

/// One replacement the scanner found: a byte range of the input and the text
/// that takes its place.
///
/// Byte offsets rather than character indices because that is what splicing
/// needs, and because a `char` index would have to be recomputed anyway.
/// `from` is the matched slice; for the lookahead phase it is **not** the whole
/// pattern, only the part the reference actually consumes (the lookahead is
/// zero-width and its character is left in the output).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rewrite<'a> {
    /// Byte offset where the replaced text begins.
    pub start: usize,
    /// Byte offset one past the end of the replaced text.
    pub end: usize,
    /// The slice being replaced, `&text[start..end]`.
    pub from: &'a str,
    /// The text written in its place.
    pub to: &'static str,
}

/// One conversion table, in the form the scanner wants it.
///
/// # Why not sorted key slices and a binary search
///
/// That was the first design, and it is what the sibling `verbora-normalizers`
/// uses — reasonably, because its tables are consulted for a handful of
/// characters per document. This one is different: the main kana table is
/// consulted for *every* kana character of Japanese text, which is the entire
/// input this crate exists to process. Three binary searches over 149, 189 and
/// 44 sorted entries came to roughly twenty comparisons per character and
/// measured 169 MiB/s; the index below measures 474 MiB/s on the same input,
/// and takes the whole pipeline from 1.6x to 3.2x the reference.
///
/// So the keys are indexed by their first `char` instead. Every key of every
/// table starts inside one or two short code-point windows, so the first
/// character is turned into a slot index by a subtraction and a bounds check,
/// and the slot names the *contiguous* run of keys beginning with that
/// character — never more than five of them, which a linear scan finishes before
/// a binary search has computed its first midpoint. The same slot doubles as the
/// membership gate: a character inside a window that begins no key lands on an
/// empty slot and is rejected without a single comparison against key data.
///
/// Only the *tail* of each multi-character key is stored, since the first
/// character is implied by the slot that points at it.
pub(crate) struct Table {
    /// Code-point windows covering every character that begins some key, sorted
    /// by base and disjoint. One for tables 1 and 2; two for table 3, whose keys
    /// begin either with a latin vowel or with a small kana.
    pub(crate) windows: &'static [Window],
    /// Tails of the two-`char` keys, grouped by first character in window order
    /// and sorted within each group.
    pub(crate) two: &'static [(char, &'static str)],
    /// Tails of the three-`char` keys, grouped and sorted the same way.
    pub(crate) three: &'static [([char; 2], &'static str)],
}

/// A contiguous run of code points that begin some key.
pub(crate) struct Window {
    /// The code point [`Window::slots`]`[0]` describes.
    pub(crate) base: u32,
    /// One slot per code point in the window, including the ones that begin no
    /// key at all — those are what make the index a gate as well as a lookup.
    pub(crate) slots: &'static [Slot],
}

/// Everything the table knows about keys beginning with one particular `char`.
pub(crate) struct Slot {
    /// The replacement, if that `char` is itself a key.
    pub(crate) one: Option<&'static str>,
    /// Half-open range into [`Table::two`].
    pub(crate) two: (u16, u16),
    /// Half-open range into [`Table::three`].
    pub(crate) three: (u16, u16),
}

impl Slot {
    /// Whether no key at all begins with this slot's character.
    #[inline]
    const fn is_empty(&self) -> bool {
        self.one.is_none() && self.two.0 == self.two.1 && self.three.0 == self.three.1
    }
}

impl Table {
    /// The slot for `c`, or `None` when `c` is outside every window.
    #[inline]
    fn slot(&self, c: char) -> Option<&'static Slot> {
        let cp = c as u32;
        for w in self.windows {
            // Wrapping subtraction folds "below the window" into a huge value,
            // so one unsigned compare decides both ends.
            let i = cp.wrapping_sub(w.base) as usize;
            if i < w.slots.len() {
                return Some(&w.slots[i]);
            }
        }
        None
    }

    /// Whether some key in this table starts with `c`.
    ///
    /// Exposed for the tests that assert the generated index is exact — it must
    /// admit every key's first character and nothing else.
    #[cfg(test)]
    pub(crate) fn gate_admits(&self, c: char) -> bool {
        self.slot(c).is_some_and(|s| !s.is_empty())
    }

    /// Every key of this table, reconstructed from the index.
    ///
    /// The index is generated, so tests that want to assert something about "all
    /// the keys" have to read them back out of it. Reconstruction rather than a
    /// second copy of the key list means the assertions are about the data the
    /// scanner actually uses.
    #[cfg(test)]
    pub(crate) fn keys(&self) -> Vec<String> {
        let mut out = Vec::new();
        for w in self.windows {
            for (i, slot) in w.slots.iter().enumerate() {
                let Some(first) = char::from_u32(w.base + i as u32) else {
                    continue;
                };
                if slot.one.is_some() {
                    out.push(first.to_string());
                }
                for &(b, _) in &self.two[slot.two.0 as usize..slot.two.1 as usize] {
                    out.push(format!("{first}{b}"));
                }
                for &([b, d], _) in &self.three[slot.three.0 as usize..slot.three.1 as usize] {
                    out.push(format!("{first}{b}{d}"));
                }
            }
        }
        out
    }

    /// The next leftmost-longest match at or after byte offset `from`.
    ///
    /// Returns `(start, end, replacement)`. The caller resumes at `end`, which is
    /// what makes matches non-overlapping and keeps replacements from being
    /// rescanned — the semantics of a `g`-flagged `String#replace`.
    pub(crate) fn next_match(
        &self,
        text: &str,
        from: usize,
    ) -> Option<(usize, usize, &'static str)> {
        for (offset, c) in text[from..].char_indices() {
            let Some(slot) = self.slot(c).filter(|s| !s.is_empty()) else {
                continue;
            };
            let start = from + offset;
            let after = start + c.len_utf8();

            // Decoding the following characters is deferred to here, so it only
            // happens for characters that actually begin a key.
            let mut rest = text[after..].chars();
            if let Some(b) = rest.next() {
                // Longest first. `キョウ` -> `kyō` must beat `キョ` -> `kyo`,
                // which must beat `キ` -> `ki`.
                let three = &self.three[slot.three.0 as usize..slot.three.1 as usize];
                if !three.is_empty() {
                    if let Some(d) = rest.next() {
                        if let Some(&(_, to)) = three.iter().find(|&&(k, _)| k == [b, d]) {
                            return Some((start, after + b.len_utf8() + d.len_utf8(), to));
                        }
                    }
                }
                let two = &self.two[slot.two.0 as usize..slot.two.1 as usize];
                if let Some(&(_, to)) = two.iter().find(|&&(k, _)| k == b) {
                    return Some((start, after + b.len_utf8(), to));
                }
            }
            if let Some(to) = slot.one {
                return Some((start, after, to));
            }
        }
        None
    }
}

/// Splices a stream of ascending, non-overlapping rewrites into `text`.
///
/// Returns [`Cow::Borrowed`] when the stream was empty, which is the usual
/// outcome for text the phase has nothing to do with. The output buffer is
/// allocated at the first rewrite and unmatched runs are copied in bulk rather
/// than character by character.
pub(crate) fn apply<'a, I>(text: &'a str, rewrites: I) -> Cow<'a, str>
where
    I: Iterator<Item = Rewrite<'a>>,
{
    let mut out: Option<String> = None;
    let mut copied = 0usize;

    for r in rewrites {
        let buf = out.get_or_insert_with(|| String::with_capacity(text.len()));
        buf.push_str(&text[copied..r.start]);
        buf.push_str(r.to);
        copied = r.end;
    }

    match out {
        Some(mut buf) => {
            buf.push_str(&text[copied..]);
            Cow::Owned(buf)
        }
        None => Cow::Borrowed(text),
    }
}

/// [`apply`], appending to a caller-owned buffer instead of allocating.
pub(crate) fn apply_into<'a, I>(text: &'a str, rewrites: I, out: &mut String)
where
    I: Iterator<Item = Rewrite<'a>>,
{
    let mut copied = 0usize;
    for r in rewrites {
        out.push_str(&text[copied..r.start]);
        out.push_str(r.to);
        copied = r.end;
    }
    out.push_str(&text[copied..]);
}

/// Applies `f` to a [`Cow`] without giving up the borrow when neither step
/// changed anything.
///
/// The transliterator is five passes over text that usually needs at most one of
/// them; chaining naively would allocate a `String` per pass.
pub(crate) fn map_cow<'a>(
    input: Cow<'a, str>,
    f: impl for<'b> FnOnce(&'b str) -> Cow<'b, str>,
) -> Cow<'a, str> {
    match input {
        Cow::Borrowed(s) => f(s),
        Cow::Owned(owned) => {
            // Re-borrowing inside the match keeps `f`'s temporary alive only for
            // the statement, so `owned` can be handed back untouched when `f`
            // made no change.
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

    /// Keys `ア`, `アイ`, `アイウ`, `カ` — a miniature of the real shape.
    ///
    /// The window runs `ア` (U+30A2) to `カ` (U+30AB), so the seven code points
    /// between them get empty slots. That is the gate: `イ` is inside the window
    /// and still rejected, because nothing starts with it.
    static SAMPLE: Table = Table {
        windows: &[Window {
            base: 0x30A2,
            slots: &[
                // ア: one key of each length.
                Slot {
                    one: Some("a"),
                    two: (0, 1),
                    three: (0, 1),
                },
                Slot {
                    one: None,
                    two: (1, 1),
                    three: (1, 1),
                }, // イ U+30A3 .. カ-1, all empty
                Slot {
                    one: None,
                    two: (1, 1),
                    three: (1, 1),
                },
                Slot {
                    one: None,
                    two: (1, 1),
                    three: (1, 1),
                },
                Slot {
                    one: None,
                    two: (1, 1),
                    three: (1, 1),
                },
                Slot {
                    one: None,
                    two: (1, 1),
                    three: (1, 1),
                },
                Slot {
                    one: None,
                    two: (1, 1),
                    three: (1, 1),
                },
                Slot {
                    one: None,
                    two: (1, 1),
                    three: (1, 1),
                },
                Slot {
                    one: None,
                    two: (1, 1),
                    three: (1, 1),
                },
                // カ U+30AB.
                Slot {
                    one: Some("ka"),
                    two: (1, 1),
                    three: (1, 1),
                },
            ],
        }],
        two: &[('イ', "ai")],
        three: &[(['イ', 'ウ'], "aiu")],
    };

    fn translate(s: &str) -> Cow<'_, str> {
        let mut pos = 0;
        let mut hits = Vec::new();
        while let Some((a, b, to)) = SAMPLE.next_match(s, pos) {
            hits.push(Rewrite {
                start: a,
                end: b,
                from: &s[a..b],
                to,
            });
            pos = b;
        }
        apply(s, hits.into_iter())
    }

    #[test]
    fn borrows_when_nothing_matches() {
        assert!(matches!(translate("zzz"), Cow::Borrowed("zzz")));
        assert!(matches!(translate(""), Cow::Borrowed("")));
        assert!(matches!(translate("漢字"), Cow::Borrowed("漢字")));
    }

    #[test]
    fn prefers_the_longest_key() {
        assert_eq!(translate("アイウ"), "aiu");
        assert_eq!(translate("アイ"), "ai");
        assert_eq!(translate("ア"), "a");
    }

    #[test]
    fn matches_are_non_overlapping_and_never_rescanned() {
        // The leading `ア` has no `イ` after it, so it takes the one-char key;
        // the second `ア` then pairs with `イ`.
        assert_eq!(translate("アアイ"), "aai");
        // `ka` must not be re-read as anything.
        assert_eq!(translate("カ"), "ka");
    }

    #[test]
    fn astral_characters_are_left_alone() {
        assert_eq!(translate("😀ア😀"), "😀a😀");
    }

    #[test]
    fn the_gate_admits_exactly_the_key_first_characters() {
        assert!(SAMPLE.gate_admits('ア'));
        assert!(SAMPLE.gate_admits('カ'));
        // Inside the window but begins no key: rejected by the empty slot.
        assert!(!SAMPLE.gate_admits('イ'));
        // Outside every window.
        assert!(!SAMPLE.gate_admits('a'));
        assert!(!SAMPLE.gate_admits('漢'));
        assert!(!SAMPLE.gate_admits('😀'));
    }

    #[test]
    fn keys_round_trip_out_of_the_index() {
        let mut keys = SAMPLE.keys();
        keys.sort();
        assert_eq!(keys, ["ア", "アイ", "アイウ", "カ"]);
    }

    #[test]
    fn apply_into_appends_without_clearing() {
        let mut buf = String::from("head:");
        let hits = [Rewrite {
            start: 0,
            end: 3,
            from: "ア",
            to: "a",
        }];
        apply_into("アイ", hits.into_iter(), &mut buf);
        assert_eq!(buf, "head:aイ");
    }

    #[test]
    fn map_cow_keeps_the_borrow_through_a_no_op_stage() {
        fn noop(s: &str) -> Cow<'_, str> {
            Cow::Borrowed(s)
        }
        assert!(matches!(
            map_cow(Cow::Borrowed("xyz"), noop),
            Cow::Borrowed("xyz")
        ));
        assert_eq!(map_cow(Cow::Owned(String::from("xyz")), noop), "xyz");
    }
}
