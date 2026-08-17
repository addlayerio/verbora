//! A UTF-16 working buffer, because the reference's string indices are code units.
//!
//! # Why not `&str` or `Vec<char>`
//!
//! Every Snowball stemmer in the reference is written in terms of `word.length`,
//! `word[i]`, `word.slice(-n)` and `word.substring(i)` — all of which count
//! UTF-16 **code units**. For the Basic Multilingual Plane a code unit is a
//! character and the distinction is invisible, but an astral-plane character
//! (an emoji, a mathematical alphanumeric) is two units to the reference and one
//! `char` to Rust. That difference is observable at every place a length or a
//! position is compared against a *constant*, and these algorithms are full of
//! such comparisons:
//!
//! ```text
//! PorterStemmer.stem("😀s")   the reference   "😀"    length 3, so the algorithm runs
//!                             char-port       "😀s"   length 2, returned unchanged
//! ```
//!
//! Region marking (`if (rv > 3)`, `if (r1 < 3) r1 = 3`), the Italian and
//! Portuguese `length < 3` gates, and the French `length === 1` gate are all in
//! this class. Working in code units removes the entire family of divergences
//! rather than arguing about which members of it are reachable.
//!
//! # Cost
//!
//! One `Vec<u16>` per stemmed word, built in a single pass. Suffix tests compare
//! against `&str` literals *without* encoding them into a buffer first — the
//! literal is walked as an iterator of code units — so the static tables stay
//! plain `&'static str` and no table is ever converted at run time.
//!
//! # Lone surrogates
//!
//! [`text`] decodes with `String::from_utf16_lossy`, so a buffer that was cut
//! between the halves of a surrogate pair renders the orphan as `U+FFFD`. Rust's
//! `String` cannot hold an unpaired surrogate at all; this is divergence **D2**
//! in `docs/PARITY.md`. No shipped rule can produce such a cut — every table
//! entry is BMP text, so every cut lands on a character boundary — which is why
//! the lossy decode is safe in practice rather than merely convenient.

/// The UTF-16 code unit of a Basic Multilingual Plane character.
///
/// Casting `char as u16` truncates, so this is only correct below `U+10000`.
/// Every literal in every rule table in this crate is BMP text, and the tables
/// are generated from the reference rather than typed by hand, so the constraint
/// holds by construction.
#[inline]
pub(crate) const fn u(c: char) -> u16 {
    debug_assert!((c as u32) < 0x1_0000, "u() is for BMP characters only");
    c as u16
}

/// Encodes `s` as UTF-16 code units.
#[inline]
pub(crate) fn units(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

/// Decodes code units back to a `String`, replacing unpaired surrogates.
#[inline]
pub(crate) fn text(w: &[u16]) -> String {
    String::from_utf16_lossy(w)
}

/// The number of UTF-16 code units in `s` — the reference's `s.length`.
#[inline]
pub(crate) fn slen(s: &str) -> usize {
    // ASCII is the overwhelmingly common case for rule-table literals, and for
    // it `len()` is already the answer.
    if s.is_ascii() {
        s.len()
    } else {
        s.encode_utf16().count()
    }
}

/// Whether `w` ends with `suffix`.
///
/// Note the asymmetry the reference inherits from `slice(-0) === slice(0)`: an empty
/// suffix compares the *whole* string against `""`, so it matches only the empty
/// token. Callers that go through this helper get that behaviour for free.
///
/// # Why the last unit is checked first
///
/// `longest_suffix`/`first_suffix` call this once per candidate in a table —
/// `docs/PERFORMANCE_GAPS.md` entry 34 measured this as the dominant cost
/// behind Verbora losing to `rust-stemmers`/`snowball_stemmers_rs` on 7 of 9
/// Snowball languages, since a real word rejects most of a step's candidate
/// suffixes (only one, if any, ever actually fires). Comparing `w`'s last
/// unit against `suffix`'s last unit is a single, branch-cheap check that
/// rejects a mismatching candidate without ever building
/// `suffix.encode_utf16()`'s iterator — real savings on the common case
/// (the two units differ) that costs nothing extra on the rare case that
/// reaches the full comparison anyway (the last units matching is a
/// necessary condition for the full comparison to succeed, so this branch
/// is never wasted work on a true match). `suffix` is `&'static str` table
/// data, never caller input, so `chars().next_back()` — `O(1)` via `Chars`'
/// `DoubleEndedIterator` impl, not a second full scan — is safe to call on
/// every invocation.
pub(crate) fn ends_with(w: &[u16], suffix: &str) -> bool {
    if suffix.is_empty() {
        return w.is_empty();
    }
    let n = slen(suffix);
    if n > w.len() {
        return false;
    }
    let tail = &w[w.len() - n..];
    if let Some(last) = suffix.chars().next_back() {
        debug_assert!((last as u32) < 0x1_0000, "suffix literals are BMP-only");
        if tail[n - 1] != last as u16 {
            return false;
        }
    }
    suffix.encode_utf16().eq(tail.iter().copied())
}

/// Whether `w` begins with `prefix`.
pub(crate) fn starts_with(w: &[u16], prefix: &str) -> bool {
    let n = slen(prefix);
    if n > w.len() {
        return false;
    }
    prefix.encode_utf16().eq(w[..n].iter().copied())
}

/// Whether `w` equals `s`.
pub(crate) fn eq_str(w: &[u16], s: &str) -> bool {
    s.encode_utf16().eq(w.iter().copied())
}

/// Appends the code units of `s` to `w`.
#[inline]
pub(crate) fn push_str(w: &mut Vec<u16>, s: &str) {
    w.extend(s.encode_utf16());
}

/// The code unit at `i`, or `None` past the end.
///
/// The reference yields `undefined` for an out-of-range index, and every predicate
/// the stemmers apply to it (`isVowel`, `=== 'e'`) is false for `undefined`, so
/// `None` behaves identically at every call site.
#[inline]
pub(crate) fn at(w: &[u16], i: usize) -> Option<u16> {
    w.get(i).copied()
}

/// The longest suffix of `w` drawn from `suffixes`, or `None`.
///
/// This is the **longest-match** policy used by the Spanish, French and Dutch
/// `endsinArr` helpers. Italian and Portuguese deliberately use first-match
/// instead ([`first_suffix`]); sharing one helper between them would silently
/// change two languages, which is why there are two.
///
/// # Two rewrites tried here, both measured and rejected
///
/// This function's dominant cost, per `docs/PERFORMANCE_GAPS.md` entry 34,
/// is calling [`ends_with`] on every candidate in a table even after the
/// true answer is already known. Two attempts at avoiding that were
/// implemented and benchmarked (`cargo bench -p verbora-stemmers`,
/// `stem-per-word/es` and `stem-per-word/fr-porter` — the two languages
/// this function serves most heavily) before being reverted in favour of
/// the plain loop below:
///
/// 1. **Sort candidates by descending length, then early-exit on first
///    match.** Provably correct (a length-descending scan's first hit is
///    the longest one by construction), but `Vec::to_vec()` allocates on
///    every call — Spanish regressed 484%, French 266%.
/// 2. **No allocation: skip calling [`ends_with`] for any candidate whose
///    length cannot beat the running best.** Still provably correct (a
///    candidate that could not improve `best` never needs checking), and
///    genuinely helped most other languages tried alongside it — but
///    Spanish and French *still* regressed (27%/21%), meaning the extra
///    per-iteration branch itself cost more than it saved for these two
///    tables' specific shape (their real matches tend to appear early, so
///    the skip rarely fires, leaving only the branch's own overhead).
///
/// [`ends_with`]'s own last-unit fast-path (this module's other real
/// optimization from the same pass) already improves every caller of this
/// function, `longest_suffix` included, without touching the iteration
/// strategy at all — the safe, working part of the intended fix.
pub(crate) fn longest_suffix<'s>(w: &[u16], suffixes: &[&'s str]) -> Option<&'s str> {
    let mut best: Option<&'s str> = None;
    for s in suffixes {
        if ends_with(w, s) && best.is_none_or(|b| slen(s) > slen(b)) {
            best = Some(s);
        }
    }
    best
}

/// The first suffix of `w` drawn from `suffixes` **in array order**, or `None`.
///
/// The Italian tables and `Token#replaceSuffixInRegion` (which Portuguese is
/// built on) both stop at the first hit, so their tables are hand-ordered
/// longest-first and that order is load-bearing.
pub(crate) fn first_suffix<'s>(w: &[u16], suffixes: &[&'s str]) -> Option<&'s str> {
    suffixes.iter().copied().find(|s| ends_with(w, s))
}

/// Removes `n` code units from the end of `w`.
#[inline]
pub(crate) fn truncate_by(w: &mut Vec<u16>, n: usize) {
    let keep = w.len().saturating_sub(n);
    w.truncate(keep);
}

/// Replaces the trailing `suffix` with `replacement`.
#[inline]
pub(crate) fn replace_suffix(w: &mut Vec<u16>, suffix: &str, replacement: &str) {
    truncate_by(w, slen(suffix));
    push_str(w, replacement);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_suffix_matches_only_the_empty_buffer() {
        assert!(!ends_with(&units("word"), ""));
        assert!(ends_with(&units(""), ""));
    }

    #[test]
    fn astral_input_is_two_units() {
        let w = units("😀s");
        assert_eq!(w.len(), 3, "the reference reports 3 for this string");
        assert_eq!(text(&w), "😀s");
    }

    #[test]
    fn suffix_tests_are_unit_exact() {
        let w = units("coração");
        assert!(ends_with(&w, "ção"));
        assert!(!ends_with(&w, "Ção"));
        assert!(starts_with(&w, "cora"));
    }

    #[test]
    fn longest_and_first_differ() {
        let w = units("running");
        assert_eq!(longest_suffix(&w, &["ing", "ning"]), Some("ning"));
        assert_eq!(first_suffix(&w, &["ing", "ning"]), Some("ing"));
    }

    #[test]
    fn slen_matches_the_reference_length() {
        assert_eq!(slen(""), 0);
        assert_eq!(slen("abc"), 3);
        assert_eq!(slen("é"), 1);
        assert_eq!(slen("😀"), 2);
    }

    #[test]
    fn ends_with_fast_path_agrees_with_the_full_comparison() {
        let w = units("cab");
        // Last unit differs from every candidate below -- the fast path's
        // own rejection case.
        assert!(!ends_with(&w, "a"));
        assert!(!ends_with(&w, "bc"));
        // Last unit matches, and the full comparison also succeeds.
        assert!(ends_with(&w, "b"));
        assert!(ends_with(&w, "ab"));
        assert!(ends_with(&w, "cab"));
        // Last unit matches by coincidence, but the full comparison must
        // still reject once more of the suffix is compared -- this is the
        // case that would catch a fast path that short-circuits to `true`
        // instead of only ever short-circuiting to `false`.
        assert!(!ends_with(&w, "xb"));
        assert!(!ends_with(&units(""), "b"));
    }
}
