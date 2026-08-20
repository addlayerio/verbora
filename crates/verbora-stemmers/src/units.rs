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
//! between the halves of a surrogate pair renders the orphan as `U+FFFD`.
//! Rust's `String` cannot hold an unpaired surrogate at all, so some
//! substitution is unavoidable. No shipped rule can produce such a cut — every
//! table entry is BMP text, pinned by `data::table_audit`, so every cut lands
//! on a character boundary — which is why the lossy decode is safe in practice
//! rather than merely convenient.

/// The UTF-16 code unit of a Basic Multilingual Plane character.
///
/// Casting `char as u16` truncates, so this is only correct below `U+10000`.
/// Every literal in every rule table in this crate is BMP text — pinned by
/// `data::table_audit`, which walks all of them — so the constraint holds by
/// construction.
#[inline]
pub(crate) const fn u(c: char) -> u16 {
    debug_assert!((c as u32) < 0x1_0000, "u() is for BMP characters only");
    c as u16
}

/// The low half of a Latin-1-range character set, as a bitmask over
/// U+0000-U+007F. See [`in_set`].
pub(crate) const fn set_lo(chars: &[u16]) -> u128 {
    let mut mask = 0u128;
    let mut i = 0;
    while i < chars.len() {
        if chars[i] < 128 {
            mask |= 1u128 << chars[i];
        }
        i += 1;
    }
    mask
}

/// The high half of a Latin-1-range character set, as a bitmask over
/// U+0080-U+00FF. See [`in_set`].
pub(crate) const fn set_hi(chars: &[u16]) -> u128 {
    let mut mask = 0u128;
    let mut i = 0;
    while i < chars.len() {
        assert!(chars[i] < 256, "in_set only covers the Latin-1 range");
        if chars[i] >= 128 {
            mask |= 1u128 << (chars[i] - 128);
        }
        i += 1;
    }
    mask
}

/// Membership of `c` in the Latin-1-range set described by `lo`/`hi`.
///
/// # Why not `matches!`
///
/// The vowel and word-character predicates are the innermost loop of region
/// marking: every stemmer scans every character of every word through one or
/// two of them. Written as a `matches!` over scattered code points they
/// compile to a chain of comparisons — sixteen of them for Norwegian's R1
/// class — evaluated per character. Every set these algorithms test lives
/// entirely below U+0100, so two 128-bit masks cover it exactly and the test
/// becomes a shift and an and. The masks are built by [`set_lo`]/[`set_hi`]
/// from the same code-point lists the `matches!` arms held, at compile time,
/// and each converted predicate keeps its original form as a `#[cfg(test)]`
/// oracle checked over every `u16`.
#[inline]
pub(crate) const fn in_set(c: u16, lo: u128, hi: u128) -> bool {
    if c < 128 {
        (lo >> c) & 1 != 0
    } else if c < 256 {
        (hi >> (c - 128)) & 1 != 0
    } else {
        false
    }
}

/// Encodes `s` as UTF-16 code units.
#[inline]
pub(crate) fn units(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

/// Decodes code units back to a `String`, replacing unpaired surrogates.
///
/// # Why this is hand-written rather than `String::from_utf16_lossy`
///
/// Every Snowball `stem` ends here, so this is one of the two allocations a
/// stemmed word cannot avoid, and it was measured at 16-36 µs per 1024
/// bench words — 15-20% of the whole per-word cost on the languages
/// `docs/PERFORMANCE_GAPS.md` entry 34 tracks. `from_utf16_lossy` drives
/// `char::decode_utf16`, which pays surrogate-pairing state and a
/// `char`-at-a-time `String::push` (each push re-checking the encoded
/// length) for every unit. Every table entry in this crate is BMP text and
/// so is essentially every input, so the loop below encodes the BMP cases
/// directly into a byte buffer sized once, and *defers to the standard
/// library* the moment it sees any surrogate — which is exactly the case
/// where pairing state and the U+FFFD substitution matter. The two are
/// therefore equal by construction on the fast path and equal by delegation
/// on the slow one; `text_agrees_with_from_utf16_lossy` fuzzes both,
/// surrogates included.
pub(crate) fn text(w: &[u16]) -> String {
    // Three bytes per unit is the BMP worst case; one `reserve` beats the
    // growth `decode_utf16().collect()` performs from a pessimistic hint.
    let mut out: Vec<u8> = Vec::with_capacity(w.len() * 3);
    for &c in w {
        match c {
            0x0000..=0x007F => out.push(c as u8),
            0x0080..=0x07FF => {
                out.push(0xC0 | (c >> 6) as u8);
                out.push(0x80 | (c & 0x3F) as u8);
            }
            // Any surrogate, paired or lone: hand the whole buffer back to
            // the standard library rather than reimplement its pairing and
            // replacement rules.
            0xD800..=0xDFFF => return String::from_utf16_lossy(w),
            _ => {
                out.push(0xE0 | (c >> 12) as u8);
                out.push(0x80 | ((c >> 6) & 0x3F) as u8);
                out.push(0x80 | (c & 0x3F) as u8);
            }
        }
    }
    String::from_utf8(out).expect("the loop above emits well-formed UTF-8 for BMP units")
}

/// The stemmed word as a slice of the caller's own `&str`, when the working
/// buffer's first `keep` code units are literally `token`'s first `keep`
/// bytes.
///
/// # Why this exists
///
/// Decoding the working buffer back through [`text`] is one of the two
/// allocations a stemmed word normally cannot avoid, and once the table
/// searches had been cut down it was the single largest remaining item in
/// the Norwegian budget — 17.4 µs per 1024 bench words, 38% of the whole
/// per-word cost. It is also avoidable far more often than it looks: every
/// Snowball step here only ever *shortens* the buffer, so when lowercasing
/// was itself a no-op the answer is a prefix of the input and no new string
/// is needed. That is precisely the condition on which `rust-stemmers` and
/// `snowball_stemmers_rs` return `Cow::Borrowed`, and it is why their
/// per-word cost has no allocation in it at all.
///
/// # Why ASCII only
///
/// `keep` counts UTF-16 code units. For ASCII input that is also a byte
/// count and a byte index, so the slice is `token[..keep]` with no scan. For
/// anything else the byte offset of the `keep`-th code unit has to be walked
/// for, and — because the caller may have truncated between the halves of a
/// surrogate pair — checked for landing on a character boundary. The scan
/// costs what it saves on short words, so non-ASCII input keeps the owned
/// path.
///
/// # The two conditions
///
/// `ascii_lower` is [`crate::among::Buf::fill_lowercase_tracked`]'s flag: the
/// input was ASCII with no uppercase letter, so the working buffer started
/// out as `token`'s bytes widened one-for-one. `rewrote` is the caller's own
/// record of whether any step *changed* units rather than only dropping
/// them (Norwegian's step 1c appends `er`, Swedish's paste arm moves units
/// down); only truncation preserves the prefix relation this relies on.
#[inline]
pub(crate) fn borrowed_prefix(
    token: &str,
    keep: usize,
    ascii_lower: bool,
    rewrote: bool,
) -> Option<&str> {
    if ascii_lower && !rewrote {
        // `keep` is a byte index into ASCII text, so it is always a
        // character boundary; `get` still carries the bound check.
        token.get(..keep)
    } else {
        None
    }
}

/// The lowercase mapping of one BMP code unit, or `None` when the unit needs
/// the standard library's per-character tables.
///
/// # Why a range table rather than `char::to_lowercase`
///
/// Nine of the twelve Snowball ports open with `token.to_lowercase()`, and
/// `char::to_lowercase` is a binary search through Unicode's conversion
/// tables on every character. That is invisible for ASCII (which the search
/// short-circuits) but not for Cyrillic: the Russian stemmer spent 82 µs per
/// 1024 bench words — over a third of its total — inside that search alone.
/// The ranges below are the ones this crate's languages actually live in
/// (ASCII, Latin-1 Supplement, Cyrillic U+0400-U+045F), each a fixed offset
/// or the identity, and anything outside them returns `None` so the caller
/// falls back. `fast_lower_agrees_with_the_standard_library` checks the
/// mapping against `char::to_lowercase` for **every** Unicode scalar value,
/// so a range that is subtly wrong cannot ship.
#[inline]
pub(crate) const fn fast_lower(c: u16) -> Option<u16> {
    match c {
        // A-Z, and the ASCII remainder which has no case mapping.
        0x0041..=0x005A => Some(c + 0x20),
        0x0000..=0x00BF => Some(c),
        // À-Ö and Ø-Þ fold by 0x20; × (0x00D7) and everything from ß on is
        // already lowercase or caseless.
        0x00C0..=0x00D6 | 0x00D8..=0x00DE => Some(c + 0x20),
        0x00D7 | 0x00DF..=0x00FF => Some(c),
        // Ѐ-Џ fold by 0x50, А-Я by 0x20, and а-џ are already lowercase.
        0x0400..=0x040F => Some(c + 0x50),
        0x0410..=0x042F => Some(c + 0x20),
        0x0430..=0x045F => Some(c),
        _ => None,
    }
}

/// Feeds `f` the code units of `s.to_lowercase()`, in order.
///
/// # Equivalence
///
/// `str::to_lowercase` is `char::to_lowercase` applied per character with a
/// single context-sensitive exception: a Greek capital sigma lowercases to
/// `ς` at the end of a word and `σ` elsewhere, which no per-character
/// mapping can decide. This function therefore hands any string containing
/// `Σ` straight to `str::to_lowercase`, and otherwise maps character by
/// character — through [`fast_lower`] where that applies and
/// `char::to_lowercase` where it does not.
///
/// The callback shape exists so a caller can write into a stack buffer:
/// nine `stem` entry points open by lowercasing, and routing that through
/// an intermediate `String` cost each of them an allocation the working
/// buffer then immediately re-encoded (`docs/PERFORMANCE_GAPS.md` entry 9's
/// allocation count).
pub(crate) fn for_each_lowercase_unit(s: &str, mut f: impl FnMut(u16)) {
    if s.contains('\u{03A3}') {
        for unit in s.to_lowercase().encode_utf16() {
            f(unit);
        }
        return;
    }
    for c in s.chars() {
        if (c as u32) < 0x1_0000 {
            if let Some(l) = fast_lower(c as u16) {
                f(l);
                continue;
            }
        }
        for l in c.to_lowercase() {
            let mut b = [0u16; 2];
            for unit in l.encode_utf16(&mut b) {
                f(*unit);
            }
        }
    }
}

/// `text(w).to_lowercase()`, without the second allocation when `w` is ASCII.
///
/// The Italian and French stemmers end with `.toLowerCase()` on the finished
/// stem (it folds the `I`/`U`/`Y` consonant marks their preludes wrote). For
/// a pure-ASCII buffer, `str::to_lowercase` is exactly the A–Z+32 fold —
/// Unicode's simple mappings agree, and the final-sigma context rule only
/// concerns `Σ`, which is not ASCII — so the fold can run in place on the
/// code units and the `String` be built once. Anything non-ASCII takes the
/// full `to_lowercase` path unchanged.
pub(crate) fn text_lowercase(w: &mut [u16]) -> String {
    if w.iter().all(|&c| c < 0x80) {
        for c in w.iter_mut() {
            if (0x41..=0x5A).contains(c) {
                *c += 32;
            }
        }
        text(w)
    } else {
        text(w).to_lowercase()
    }
}

/// The number of UTF-16 code units in `s`.
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
/// An empty suffix matches only the empty buffer, not every buffer. No rule
/// table in this crate carries an empty entry, so the case arises only through
/// a caller-supplied literal.
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
/// Every predicate the stemmers apply to a position (`is_vowel`, `== 'e'`) is
/// false past the end, which is what makes the region scans safe at the
/// boundary without a length test at each call site.
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
/// The Italian and Portuguese suffix tables both stop at the first hit, so they
/// are hand-ordered longest-first and that order is load-bearing.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "the Italian/Portuguese hot paths now go through `among::find_among`, \
                  whose first==longest equivalence their differential oracles — this \
                  function's remaining callers — exist to prove"
    )
)]
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
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "the Portuguese hot path now cuts by matched length; the verbatim \
                  pre-conversion oracle in `pt.rs`'s tests is the remaining caller"
    )
)]
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

    /// `text`'s hand-rolled BMP encoder must be indistinguishable from the
    /// standard library's, including on the surrogate inputs that send it
    /// down the delegation arm.
    #[test]
    fn text_agrees_with_from_utf16_lossy() {
        let mut state = 0x2545_F491_4F6C_DD1Du64;
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        for case in 0..40_000u32 {
            let n = (next() % 9) as usize;
            let w: Vec<u16> = (0..n)
                .map(|_| {
                    let r = next();
                    match r % 5 {
                        // Deliberately biased towards surrogates so both the
                        // paired and the lone case are hit often.
                        0 => (0xD800 + (r >> 8) % 0x0800) as u16,
                        1 => (0xDC00 + (r >> 8) % 0x0400) as u16,
                        2 => (r % 0x80) as u16,
                        3 => (0x80 + (r >> 8) % 0x0780) as u16,
                        _ => ((r >> 8) % 0xFFFF) as u16,
                    }
                })
                .collect();
            assert_eq!(text(&w), String::from_utf16_lossy(&w), "case {case}: {w:?}");
        }
    }

    /// Every scalar value's fast mapping must agree with the standard
    /// library — a range boundary off by one would otherwise only show up as
    /// a wrong stem for some rare letter.
    #[test]
    fn fast_lower_agrees_with_the_standard_library() {
        for cp in 0..=0x10_FFFFu32 {
            let Some(c) = char::from_u32(cp) else {
                continue;
            };
            if cp >= 0x1_0000 {
                continue;
            }
            let Some(fast) = fast_lower(cp as u16) else {
                continue;
            };
            let mut it = c.to_lowercase();
            let first = it.next().expect("to_lowercase yields at least one char");
            assert!(
                it.next().is_none(),
                "U+{cp:04X} expands to several characters, so no single-unit \
                 fast mapping can be right"
            );
            assert_eq!(
                u32::from(fast),
                first as u32,
                "U+{cp:04X}: fast mapping disagrees"
            );
        }
    }

    /// The callback form must reproduce `str::to_lowercase` exactly — over
    /// every single-scalar string, over the sigma cases the per-character
    /// mapping cannot decide, and over random mixed strings.
    #[test]
    fn for_each_lowercase_unit_agrees_with_to_lowercase() {
        let lower = |s: &str| {
            let mut v = Vec::new();
            for_each_lowercase_unit(s, |u| v.push(u));
            v
        };
        for cp in 0..=0x10_FFFFu32 {
            let Some(c) = char::from_u32(cp) else {
                continue;
            };
            let s = c.to_string();
            assert_eq!(lower(&s), units(&s.to_lowercase()), "U+{cp:04X}");
        }
        for s in [
            "ΟΔΟΣ",
            "ΣΣΣ",
            "Σ",
            "σ",
            "ς",
            "ὈΔΥΣΣΕΎΣ",
            "İ",
            "ǅ",
            "ẞ",
            "ß",
            "ΑΣ ΑΣ",
            "aΣb",
        ] {
            assert_eq!(lower(s), units(&s.to_lowercase()), "{s:?}");
        }
        let mut state = 0x853C_49E6_748F_EA9Bu64;
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        const POOL: &[char] = &[
            'a', 'Z', 'É', 'ß', 'Σ', 'σ', 'ς', 'Ж', 'ж', 'Ё', 'ё', 'İ', 'ǅ', '😀', '日', '1', '-',
            'Ø', 'ø', 'Ǆ', 'ᾈ',
        ];
        for _ in 0..20_000 {
            let n = (next() % 7) as usize;
            let s: String = (0..n)
                .map(|_| POOL[(next() % POOL.len() as u64) as usize])
                .collect();
            assert_eq!(lower(&s), units(&s.to_lowercase()), "{s:?}");
        }
    }
}
