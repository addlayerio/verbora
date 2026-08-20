//! The lookup index and the splice helpers the scanner is built from.
//!
//! Everything here is private to the crate. The data these types describe is
//! written by `build.rs` from `src/syllabary.rs`; the shapes are here so that
//! the generated file has something to name.

use std::borrow::Cow;

/// One mora's romanization, short and lengthened.
///
/// Both forms are `&'static str` because both are laid out at build time, so a
/// rewrite never allocates to name its replacement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Mora {
    /// The romanization on its own: `か` → `"ka"`.
    pub(crate) short: &'static str,
    /// `short` with its final vowel macronned — `Some("kā")` for `か` — or
    /// `None` when `short` does not end in a vowel and so cannot be lengthened.
    pub(crate) long: Option<&'static str>,
}

impl Mora {
    /// The ASCII vowel `short` ends in, if it ends in one.
    ///
    /// Reads the last byte directly: `build.rs` rejects any romanization that
    /// is not ASCII, so the last byte is the last character.
    #[inline]
    pub(crate) fn final_vowel(self) -> Option<u8> {
        match self.short.as_bytes().last() {
            Some(&b @ (b'a' | b'e' | b'i' | b'o' | b'u')) => Some(b),
            _ => None,
        }
    }
}

/// Everything the index knows about keys beginning with one particular scalar.
#[derive(Debug)]
pub(crate) struct Slot {
    /// The mora for the one-scalar key that is exactly this character.
    pub(crate) one: Option<Mora>,
    /// Half-open range into [`tables::TWO`](crate::tables::TWO) naming the
    /// two-scalar keys that begin with this character.
    pub(crate) two: (usize, usize),
}

impl Slot {
    /// Whether no key at all begins with this slot's character.
    #[inline]
    pub(crate) const fn is_empty(&self) -> bool {
        self.one.is_none() && self.two.0 == self.two.1
    }
}

/// The longest key matching at byte offset `at`, with its length in bytes.
///
/// Leftmost-longest by construction rather than by list order: the two-scalar
/// keys are tried before the one-scalar key, and there are no longer keys.
///
/// Returns `None` when `at` is not a character boundary of a key start, when
/// the character there begins no key, or when `at >= text.len()`.
#[inline]
pub(crate) fn longest_at(text: &str, at: usize) -> Option<(usize, Mora)> {
    let first = text.get(at..)?.chars().next()?;
    let slot = slot(first)?;
    let after = at + first.len_utf8();

    // Decoding the second character is deferred to here, so it only happens
    // for characters that actually begin a key.
    if slot.two.0 != slot.two.1 {
        if let Some(second) = text[after..].chars().next() {
            let group = &crate::tables::TWO[slot.two.0..slot.two.1];
            if let Some(&(_, mora)) = group.iter().find(|&&(k, _)| k == second) {
                return Some((after + second.len_utf8() - at, mora));
            }
        }
    }
    slot.one.map(|mora| (first.len_utf8(), mora))
}

/// The slot for `c`, or `None` when `c` begins no key at all.
#[inline]
fn slot(c: char) -> Option<&'static Slot> {
    // Wrapping subtraction folds "below the base" into a huge value, so one
    // unsigned compare decides both ends of the span.
    let i = (c as u32).wrapping_sub(crate::tables::SLOT_BASE) as usize;
    crate::tables::SLOTS.get(i).filter(|s| !s.is_empty())
}

/// One replacement the scanner found: a byte range of the input and the text
/// that takes its place.
///
/// Byte offsets rather than character indices because that is what splicing
/// needs, and because a character index would have to be recomputed anyway.
///
/// `to` is `&'static str` and may be empty: a mora that romanizes to nothing —
/// a sokuon with no consonant to double, a prolonged sound mark with no vowel
/// to lengthen — is reported as a rewrite to `""` rather than skipped, so that
/// the stream describes the whole transformation and not merely its visible
/// half.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rewrite<'a> {
    /// Byte offset where the replaced text begins.
    pub start: usize,
    /// Byte offset one past the end of the replaced text.
    pub end: usize,
    /// The slice being replaced, `&text[start..end]`.
    pub from: &'a str,
    /// The text written in its place, possibly empty.
    pub to: &'static str,
}

/// Splices a stream of ascending, non-overlapping rewrites into `text`.
///
/// Returns [`Cow::Borrowed`] when the stream was empty, which is the usual
/// outcome for text the scanner has nothing to do with. The output buffer is
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
/// `transliterate_ja_normalized` is three stages over text that usually needs
/// none of them; chaining naively would allocate a `String` per stage.
pub(crate) fn map_cow<'a>(
    input: Cow<'a, str>,
    f: impl for<'b> FnOnce(&'b str) -> Cow<'b, str>,
) -> Cow<'a, str> {
    match input {
        Cow::Borrowed(s) => f(s),
        Cow::Owned(owned) => {
            // Re-borrowing inside the match keeps `f`'s temporary alive only
            // for the statement, so `owned` can be handed back untouched when
            // `f` made no change.
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

    #[test]
    fn longest_at_prefers_the_two_scalar_key() {
        // `きょ` is a key and so is `き`; the two-scalar one must win.
        assert_eq!(
            longest_at("きょ", 0).map(|(n, m)| (n, m.short)),
            Some((6, "kyo"))
        );
        assert_eq!(
            longest_at("きあ", 0).map(|(n, m)| (n, m.short)),
            Some((3, "ki"))
        );
        assert_eq!(
            longest_at("き", 0).map(|(n, m)| (n, m.short)),
            Some((3, "ki"))
        );
    }

    #[test]
    fn longest_at_rejects_everything_that_begins_no_key() {
        for (text, at) in [("abc", 0), ("漢", 0), ("😀", 0), ("", 0), ("ki", 1)] {
            assert!(longest_at(text, at).is_none(), "{text:?} at {at}");
        }
        // Inside the slot span but begins no key: `ー`, `ん`, `っ`.
        for text in ["ー", "ん", "っ", "ン", "ッ"] {
            assert!(longest_at(text, 0).is_none(), "{text:?}");
        }
        // Past the end of the string.
        assert!(longest_at("き", 3).is_none());
    }

    #[test]
    fn final_vowel_reads_the_last_byte() {
        let ka = longest_at("か", 0).expect("か").1;
        assert_eq!(ka.short, "ka");
        assert_eq!(ka.long, Some("kā"));
        assert_eq!(ka.final_vowel(), Some(b'a'));

        let dot = longest_at("・", 0).expect("・").1;
        assert_eq!(dot.short, " ");
        assert_eq!(dot.long, None);
        assert_eq!(dot.final_vowel(), None);
    }

    #[test]
    fn apply_borrows_when_the_stream_is_empty() {
        assert!(matches!(
            apply("zzz", std::iter::empty()),
            Cow::Borrowed("zzz")
        ));
        assert!(matches!(apply("", std::iter::empty()), Cow::Borrowed("")));
    }

    #[test]
    fn apply_splices_in_order_including_empty_replacements() {
        let hits = [
            Rewrite {
                start: 0,
                end: 3,
                from: "か",
                to: "ka",
            },
            Rewrite {
                start: 3,
                end: 6,
                from: "ー",
                to: "",
            },
        ];
        assert_eq!(apply("かー", hits.into_iter()), "ka");
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
