//! The stemmer working buffer, and the text unit every stemmer measures in.
//!
//! # The unit is the Unicode scalar value
//!
//! These algorithms are specified over **letters**, not over an encoding.
//! Every rule they are published with is written that way: R1 is *"the region
//! after the first non-vowel following a vowel"*, Spanish RV is *"if the second
//! letter is a consonant, the region after the next following vowel"*, Porter's
//! own measure counts consonant and vowel *sequences*, and every short-word
//! gate this crate inherits is stated as a count of letters. Not one of those
//! sentences names a code unit, a byte, or any other unit of storage.
//!
//! The UTF-16 indices this crate used to carry are therefore a property of the
//! *host language* of the port these algorithms were transcribed through, not
//! a property of the algorithm. Reading every position as a Unicode scalar
//! value is the faithful reading; reading it as a UTF-16 code unit is the
//! transcription artefact. This module states the faithful one, so the choice
//! is a correction rather than a trade-off.
//!
//! # Why the scalar value and not the grapheme cluster
//!
//! "Letter" admits a third reading — the user-perceived character — and it is
//! rejected deliberately. Every letter class these algorithms test is a set of
//! scalar values (`[aeiouyæåø]`, `[A-Za-z0-9_æøåÆØÅäÄöÖüÜ]`, the Cyrillic
//! vowels), every rule table entry is a sequence of scalar values, and the
//! lowercasing every prelude opens with is `str::to_lowercase`, which is also
//! per scalar value. A grapheme-cluster reading would have to answer what the
//! vowel class says about `e` followed by a combining acute, which no published
//! Snowball rule does. The scalar value is also the unit every other crate in
//! this workspace measures in, so it is the reading that makes one text measure
//! the same wherever it is handled.
//!
//! Concretely, and inherited by every stemmer in this crate:
//!
//! * **R1, R2 and RV are positions in scalar values**, as is every other
//!   cursor, limit and region bound.
//! * **A length compared against a constant counts scalar values** — `if rv >
//!   3`, `if r1 < 3 { r1 = 3 }`, the Italian and Portuguese `length < 3`
//!   gates, the French `length == 1` gate, Porter's own three-letter gate.
//! * **A cut at any such position is a cut at a character boundary**, by
//!   construction rather than by audit.
//!
//! For the Basic Multilingual Plane the scalar and the code unit coincide
//! exactly, so this changes an answer only for astral-plane text — emoji,
//! mathematical alphanumerics, historic scripts — where the two readings
//! disagree about how long a word is. `PorterStemmer::stem("😀s")` is the
//! canonical case: two scalar values, so Porter's three-letter gate returns
//! the token untouched, where three code units let the algorithm run and cut
//! the `s` off.
//!
//! # What the scalar unit removes, beyond the inconsistency
//!
//! Every other crate in this workspace measures text in scalar values, so the
//! same string no longer measures differently depending on which crate touched
//! it. Two more consequences are structural rather than cosmetic:
//!
//! * **A working buffer can no longer hold half a character.** A `char` is a
//!   whole scalar value, so no cut — by a suffix length, by a region
//!   constant, or by position arithmetic that relates to neither — can split
//!   one. Decoding a buffer back to a `String` is total: it cannot invent a
//!   `U+FFFD` the caller never supplied. Under the code-unit reading that
//!   safety rested on an argument about *table* contents, which did not cover
//!   cuts made by *region arithmetic*.
//! * **The sort order and the search comparator agree everywhere.** [`char`]'s
//!   `Ord` is its code-point order, which is exactly what
//!   [`crate::among`]'s binary search subtracts. UTF-16 orders surrogates
//!   (`U+D800`–`U+DFFF`) below `U+E000`–`U+FFFF` while the characters they
//!   encode sort above both, so a table mixing astral entries with high-BMP
//!   ones would sort one way and be searched another. That hazard is gone
//!   rather than merely unreached.
//!
//! # The second implementation, and why it survives
//!
//! [`Unit`] has two implementations, but only one is a unit. [`char`] is the
//! contract: every stemmer in this crate measures scalar values, and no
//! production path can reach anything else.
//!
//! `u16` is compiled only under `cfg(test)`, as a differential oracle. The
//! tables in [`crate::among`] are built in both and swept across the whole BMP
//! asserting they answer identically — the standing proof that reading these
//! algorithms as scalars, which is what their publications specify, changed
//! nothing for text below `U+10000`. Keeping the oracle keeps the proof
//! runnable; `cfg(test)` is what stops it from being mistaken for a supported
//! alternative.
//!
//! # Cost
//!
//! One buffer per stemmed word, built in a single pass. Suffix tests compare
//! against `&str` literals *without* materialising them into a buffer first —
//! the literal is walked as an iterator of units — so the static rule tables
//! stay plain `&'static str` and no table is ever converted at run time.

/// One position of a stemmer's working buffer.
///
/// The buffer is a flat, randomly-indexable sequence, because that is what the
/// algorithms are: `w[i]`, `w[..n]`, `w.len()`, and comparisons of positions
/// against region bounds. [`char`] is the implementation this crate's contract
/// is written in (see the module documentation); `u16` is the transitional
/// one.
///
/// # Why a trait rather than two modules
///
/// The alternative during the migration was a second copy of this module and
/// of [`crate::among`] spelled in `char`, with the per-language files moved
/// across one at a time. One implementation behind a trait keeps the binary
/// search, the rejection filter and the lowercasing fast paths as a single
/// piece of code that both instantiations monomorphise from, so the
/// transitional `u16` behaviour is preserved *by construction* rather than by
/// keeping two copies in step by hand.
pub(crate) trait Unit: Copy + Eq + Ord + core::fmt::Debug + 'static {
    /// Filler for the unused tail of an inline buffer. Never read as text.
    const PAD: Self;

    /// The numeric value the sort order and the search comparator use.
    ///
    /// For [`char`] this is the code point; for `u16` the code unit. Both are
    /// consistent with the type's own `Ord`, which is what makes a table
    /// sorted by `Ord` searchable by numeric subtraction.
    fn code(self) -> u32;

    /// The unit for an ASCII byte.
    fn from_ascii(b: u8) -> Self;

    /// The units of `s`, in order.
    fn of(s: &str) -> impl Iterator<Item = Self> + '_;

    /// The number of units in `s`.
    fn len_of(s: &str) -> usize;

    /// The last unit of `s`, or `None` when `s` is empty.
    fn last_of(s: &str) -> Option<Self>;

    /// The units decoded back to a `String`.
    fn decode(w: &[Self]) -> String;

    /// The lowercase of `c` when it is a single unit drawn from
    /// [`fast_lower`]'s ranges, or `None` when the caller must fall back to
    /// `char::to_lowercase`.
    fn lower(c: char) -> Option<Self>;

    /// Writes the units of one character into `out`, returning how many.
    ///
    /// Two slots is the maximum either implementation needs: one scalar value
    /// is one [`char`], or one or two UTF-16 code units. The array form rather
    /// than a callback is so a caller writing into a fixed inline buffer can
    /// bail out of its own loop when it runs out of room.
    fn encode_char(c: char, out: &mut [Self; 2]) -> usize;
}

impl Unit for char {
    const PAD: Self = '\0';

    #[inline]
    fn code(self) -> u32 {
        self as u32
    }

    #[inline]
    fn from_ascii(b: u8) -> Self {
        char::from(b)
    }

    #[inline]
    fn of(s: &str) -> impl Iterator<Item = Self> + '_ {
        s.chars()
    }

    #[inline]
    fn len_of(s: &str) -> usize {
        // ASCII is the overwhelmingly common case for rule-table literals, and
        // for it `len()` is already the answer.
        if s.is_ascii() {
            s.len()
        } else {
            s.chars().count()
        }
    }

    #[inline]
    fn last_of(s: &str) -> Option<Self> {
        // `O(1)` via `Chars`' `DoubleEndedIterator` impl, not a second scan.
        s.chars().next_back()
    }

    #[inline]
    fn decode(w: &[Self]) -> String {
        // Total: a `char` is a whole scalar value, so there is no replacement
        // case and no failure case. One byte per unit is exact for ASCII and
        // the floor for everything else.
        let mut out = String::with_capacity(w.len());
        out.extend(w.iter().copied());
        out
    }

    #[inline]
    fn lower(c: char) -> Option<Self> {
        if (c as u32) >= 0x1_0000 {
            return None;
        }
        // Every value `fast_lower` returns lies in `U+0000..=U+045F`, which
        // holds no surrogate, so `from_u32` always succeeds; `and_then` states
        // that without a panicking branch in the per-character loop, and a
        // `None` would only mean "take the standard library path", which is
        // correct anyway.
        fast_lower(c as u16).and_then(|l| char::from_u32(u32::from(l)))
    }

    #[inline]
    fn encode_char(c: char, out: &mut [Self; 2]) -> usize {
        out[0] = c;
        1
    }
}

/// The differential oracle, not a supported unit.
///
/// Every stemmer in this crate measures scalar values. This second
/// implementation is retained under `cfg(test)` for one purpose: the tables in
/// [`crate::among`] are built in both units and swept across the whole BMP to
/// assert they answer identically, which is the standing proof that reading
/// these algorithms as scalars changed nothing for text below `U+10000`.
/// Deleting it would delete that proof, so it stays — but no production path
/// may reach it, which `cfg(test)` enforces rather than merely asks.
#[cfg(test)]
impl Unit for u16 {
    const PAD: Self = 0;

    #[inline]
    fn code(self) -> u32 {
        u32::from(self)
    }

    #[inline]
    fn from_ascii(b: u8) -> Self {
        u16::from(b)
    }

    #[inline]
    fn of(s: &str) -> impl Iterator<Item = Self> + '_ {
        s.encode_utf16()
    }

    #[inline]
    fn len_of(s: &str) -> usize {
        if s.is_ascii() {
            s.len()
        } else {
            s.encode_utf16().count()
        }
    }

    #[inline]
    fn last_of(s: &str) -> Option<Self> {
        let c = s.chars().next_back()?;
        // Every rule-table literal is BMP text, so this is a compare and a
        // truncating cast — the shape the last-unit rejection in `ends_with`
        // was written for. The cold arm yields the *low* surrogate, which is
        // what a UTF-16 buffer's own last position holds for an astral
        // character.
        if (c as u32) < 0x1_0000 {
            return Some(c as u16);
        }
        let mut b = [0u16; 2];
        c.encode_utf16(&mut b).last().copied()
    }

    #[inline]
    fn decode(w: &[Self]) -> String {
        text_utf16(w)
    }

    #[inline]
    fn lower(c: char) -> Option<Self> {
        if (c as u32) < 0x1_0000 {
            fast_lower(c as u16)
        } else {
            None
        }
    }

    #[inline]
    fn encode_char(c: char, out: &mut [Self; 2]) -> usize {
        c.encode_utf16(out).len()
    }
}

/// A BMP character as its single UTF-16 code unit.
///
/// **Test-only**, like [`Unit`]'s `u16` implementation it serves. `char as u16`
/// truncates, so it is correct only below `U+10000` — which every rule-table
/// literal in this crate satisfies, pinned by `data::table_audit`.
#[cfg(test)]
#[inline]
pub(crate) const fn u(c: char) -> u16 {
    debug_assert!((c as u32) < 0x1_0000, "u() is for BMP characters only");
    c as u16
}

/// The low half of a Latin-1-range character set, as a bitmask over
/// U+0000-U+007F. See [`in_set`].
///
/// The argument is a list of **code points**, not of units: the two coincide
/// below U+0100, which is the whole range these masks cover.
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
/// oracle checked over every code point.
///
/// # Unit independence
///
/// Every set is below U+0100, and an astral scalar value is neither below
/// U+0100 nor — under the transitional UTF-16 reading — either half of a
/// surrogate pair below it. So this predicate answers the same for a word
/// under both units, character for character, and converting a caller from
/// `u16` to [`char`] cannot change an answer.
#[inline]
pub(crate) fn in_set<U: Unit>(c: U, lo: u128, hi: u128) -> bool {
    let c = c.code();
    if c < 128 {
        (lo >> c) & 1 != 0
    } else if c < 256 {
        (hi >> (c - 128)) & 1 != 0
    } else {
        false
    }
}

/// Encodes `s` as UTF-16 code units.
///
/// **Test-only.** Nothing in this crate measures text in UTF-16 any more; this
/// exists so the differential tests can build a `u16` table beside the `char`
/// one and assert the two agree across the BMP. See [`Unit`]'s second
/// implementation.
#[cfg(test)]
#[inline]
pub(crate) fn units(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

/// The units decoded back to a `String`.
///
/// For the scalar unit this is total — every `char` is a whole scalar value,
/// so there is nothing to replace and nothing to fail on. The transitional
/// `u16` instantiation still has to answer for buffers cut between the halves
/// of a surrogate pair, and does so through [`text_utf16`].
#[inline]
pub(crate) fn text<U: Unit>(w: &[U]) -> String {
    U::decode(w)
}

/// Decodes UTF-16 code units back to a `String`, replacing unpaired
/// surrogates. The transitional unit's [`Unit::decode`].
///
/// # Why this is hand-written rather than `String::from_utf16_lossy`
///
/// Every Snowball `stem` ends in a decode, so this is one of the two
/// allocations a stemmed word cannot avoid, and it was measured at 16-36 µs
/// per 1024 bench words — 15-20% of the whole per-word cost on the languages
/// `docs/PERFORMANCE_GAPS.md` entry 34 tracks. `from_utf16_lossy` drives
/// `char::decode_utf16`, which pays surrogate-pairing state and a
/// `char`-at-a-time `String::push` (each push re-checking the encoded length)
/// for every unit. Every table entry in this crate is BMP text and so is
/// essentially every input, so the loop below encodes the BMP cases directly
/// into a byte buffer sized once, and *defers to the standard library* the
/// moment it sees any surrogate — which is exactly the case where pairing
/// state and the U+FFFD substitution matter. The two are therefore equal by
/// construction on the fast path and equal by delegation on the slow one;
/// `text_agrees_with_from_utf16_lossy` fuzzes both, surrogates included.
///
/// The surrogate arm is why the scalar unit is a correction and not a
/// preference: a `u16` buffer can be cut between the halves of a pair — not
/// only by a suffix length, which the BMP rule tables do bound, but by region
/// arithmetic, which they do not — and the caller then receives a `U+FFFD`
/// it never supplied. A `char` buffer has no such state to be in.
#[cfg(test)]
fn text_utf16(w: &[u16]) -> String {
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
/// buffer's first `keep` units are literally `token`'s first `keep` bytes.
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
/// `keep` counts units. For ASCII input a unit is also a byte and a byte
/// index, so the slice is `token[..keep]` with no scan at all. For anything
/// else the byte offset of the `keep`-th unit has to be walked for, and the
/// scan costs what it saves on short words, so non-ASCII input keeps the
/// owned path.
///
/// Under the scalar unit that is the *whole* reason: `keep` is a count of
/// characters, so the offset it names is a character boundary by
/// construction and no boundary check is needed. The transitional UTF-16
/// unit additionally had to allow for a caller having truncated between the
/// halves of a surrogate pair. Whether removing that check makes the
/// non-ASCII scan worth taking is **UNMEASURED** — it trades a walk over
/// `keep` characters against one allocation, and this crate does not publish
/// unmeasured numbers — so the restriction stays as it is and the question is
/// recorded rather than answered.
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

/// The lowercase mapping of one BMP code point, or `None` when it needs the
/// standard library's per-character tables.
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
///
/// The domain is a code point rather than a unit because the mapping is the
/// same either way: every input and every output below `U+0460` is one
/// [`char`] and one `u16`, so [`Unit::lower`] is a thin re-typing of this
/// table for both implementations rather than two tables to keep in step.
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

/// Feeds `f` the units of `s.to_lowercase()`, in order.
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
pub(crate) fn for_each_lowercase_unit<U: Unit>(s: &str, mut f: impl FnMut(U)) {
    #[inline]
    fn emit<U: Unit>(c: char, f: &mut impl FnMut(U)) {
        let mut b = [U::PAD; 2];
        let n = U::encode_char(c, &mut b);
        for unit in &b[..n] {
            f(*unit);
        }
    }
    if s.contains('\u{03A3}') {
        for c in s.to_lowercase().chars() {
            emit(c, &mut f);
        }
        return;
    }
    for c in s.chars() {
        if let Some(l) = U::lower(c) {
            f(l);
            continue;
        }
        for l in c.to_lowercase() {
            emit(l, &mut f);
        }
    }
}

/// `text(w).to_lowercase()`, without the second allocation when `w` is ASCII.
///
/// The Italian and French stemmers end with a lowercase of the finished stem
/// (it folds the `I`/`U`/`Y` consonant marks their preludes wrote). For a
/// pure-ASCII buffer, `str::to_lowercase` is exactly the A–Z+32 fold —
/// Unicode's simple mappings agree, and the final-sigma context rule only
/// concerns `Σ`, which is not ASCII — so the fold can run in place on the
/// units and the `String` be built once. Anything non-ASCII takes the full
/// `to_lowercase` path unchanged.
pub(crate) fn text_lowercase<U: Unit>(w: &mut [U]) -> String {
    if w.iter().all(|c| c.code() < 0x80) {
        for c in w.iter_mut() {
            let v = c.code();
            if (0x41..=0x5A).contains(&v) {
                *c = U::from_ascii(v as u8 + 32);
            }
        }
        text(w)
    } else {
        text(w).to_lowercase()
    }
}

/// The length of `s` in the crate's text unit: its number of **Unicode scalar
/// values**.
///
/// This is the length the algorithms' own gates are written against — Porter's
/// three-letter minimum, the Italian and Portuguese `length < 3`, the French
/// `length == 1`, the Japanese katakana minimum — so it is the length a
/// stemmer asks for when it asks how long a word is.
///
/// # Using it as buffer arithmetic
///
/// Several call sites subtract `slen(literal)` from a working buffer's own
/// `len()`. That is only sound when the literal's scalar count equals its
/// count in the buffer's unit, which holds for **every** rule table in this
/// crate because every table entry is BMP text (pinned by
/// `data::table_audit`; the highest code point in any entry is `U+0457`), and
/// on the BMP the two units coincide exactly —
/// `on_the_basic_multilingual_plane_the_two_units_coincide` pins that. Once a
/// per-language file's buffer is `char`, the two are the same function and the
/// caveat evaporates. Inside this module and [`crate::among`] the question
/// never arises: buffer arithmetic goes through [`Unit::len_of`], which is
/// always the buffer's own unit.
#[inline]
pub(crate) fn slen(s: &str) -> usize {
    <char as Unit>::len_of(s)
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
/// rejects a mismatching candidate without ever building `suffix`'s unit
/// iterator — real savings on the common case (the two units differ) that
/// costs nothing extra on the rare case that reaches the full comparison
/// anyway (the last units matching is a necessary condition for the full
/// comparison to succeed, so this branch is never wasted work on a true
/// match). `suffix` is `&'static str` table data, never caller input, so
/// [`Unit::last_of`] — `O(1)` via `Chars`' `DoubleEndedIterator` impl, not a
/// second full scan — is safe to call on every invocation.
pub(crate) fn ends_with<U: Unit>(w: &[U], suffix: &str) -> bool {
    if suffix.is_empty() {
        return w.is_empty();
    }
    let n = U::len_of(suffix);
    if n > w.len() {
        return false;
    }
    let tail = &w[w.len() - n..];
    if let Some(last) = U::last_of(suffix)
        && tail[n - 1] != last
    {
        return false;
    }
    U::of(suffix).eq(tail.iter().copied())
}

/// Whether `w` begins with `prefix`.
pub(crate) fn starts_with<U: Unit>(w: &[U], prefix: &str) -> bool {
    let n = U::len_of(prefix);
    if n > w.len() {
        return false;
    }
    U::of(prefix).eq(w[..n].iter().copied())
}

/// Whether `w` equals `s`.
pub(crate) fn eq_str<U: Unit>(w: &[U], s: &str) -> bool {
    U::of(s).eq(w.iter().copied())
}

/// Appends the units of `s` to `w`.
#[inline]
pub(crate) fn push_str<U: Unit>(w: &mut Vec<U>, s: &str) {
    w.extend(U::of(s));
}

/// The unit at `i`, or `None` past the end.
///
/// Every predicate the stemmers apply to a position (`is_vowel`, `== 'e'`) is
/// false past the end, which is what makes the region scans safe at the
/// boundary without a length test at each call site.
#[inline]
pub(crate) fn at<U: Unit>(w: &[U], i: usize) -> Option<U> {
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
pub(crate) fn longest_suffix<'s, U: Unit>(w: &[U], suffixes: &[&'s str]) -> Option<&'s str> {
    let mut best: Option<&'s str> = None;
    for s in suffixes {
        if ends_with(w, s) && best.is_none_or(|b| U::len_of(s) > U::len_of(b)) {
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
pub(crate) fn first_suffix<'s, U: Unit>(w: &[U], suffixes: &[&'s str]) -> Option<&'s str> {
    suffixes.iter().copied().find(|s| ends_with(w, s))
}

/// Removes `n` units from the end of `w`.
#[inline]
pub(crate) fn truncate_by<U>(w: &mut Vec<U>, n: usize) {
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
pub(crate) fn replace_suffix<U: Unit>(w: &mut Vec<U>, suffix: &str, replacement: &str) {
    truncate_by(w, U::len_of(suffix));
    push_str(w, replacement);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scalar working buffer, the way a converted stemmer builds one.
    fn scalars(s: &str) -> Vec<char> {
        s.chars().collect()
    }

    #[test]
    fn empty_suffix_matches_only_the_empty_buffer() {
        assert!(!ends_with(&scalars("word"), ""));
        assert!(ends_with(&scalars(""), ""));
        assert!(!ends_with(&units("word"), ""));
        assert!(ends_with(&units(""), ""));
    }

    /// The contract: one astral character is **one** position, so `"😀s"` is a
    /// two-character word.
    ///
    /// The numbers are arithmetic, not measurement: `U+1F600` is a single
    /// Unicode scalar value, and a length is a count of scalar values.
    #[test]
    fn an_astral_character_is_one_position() {
        let w = scalars("😀s");
        assert_eq!(w.len(), 2);
        assert_eq!(w[0], '😀');
        assert_eq!(text(&w), "😀s");
        assert!(ends_with(&w, "s"));
        assert!(!ends_with(&w, "os"));
    }

    /// Decoding a scalar buffer is total: no cut can produce a character the
    /// caller did not supply, whatever position it is made at.
    ///
    /// This is the property the UTF-16 unit could not offer — there, a cut
    /// between the halves of a surrogate pair renders the orphan as `U+FFFD`.
    #[test]
    fn every_cut_of_a_scalar_buffer_decodes_to_supplied_text() {
        for s in [
            "😀s",
            "a😀b",
            "𝟎𝟏𝟐",
            "he😀ten",
            "kloke😀",
            "𐐀𐐁𐐂𐐃s",
            "coração",
            "",
        ] {
            let w = scalars(s);
            for keep in 0..=w.len() {
                let cut = text(&w[..keep]);
                assert!(
                    !cut.contains('\u{FFFD}') || s.contains('\u{FFFD}'),
                    "{s:?} cut at {keep} produced U+FFFD"
                );
                assert!(s.starts_with(&cut), "{s:?} cut at {keep} is not a prefix");
                assert_eq!(cut.chars().count(), keep);
            }
        }
    }

    #[test]
    fn suffix_tests_are_unit_exact() {
        let w = scalars("coração");
        assert!(ends_with(&w, "ção"));
        assert!(!ends_with(&w, "Ção"));
        assert!(starts_with(&w, "cora"));
        assert!(eq_str(&w, "coração"));

        let w = units("coração");
        assert!(ends_with(&w, "ção"));
        assert!(!ends_with(&w, "Ção"));
        assert!(starts_with(&w, "cora"));
    }

    /// The two implementations must agree on every BMP input, since that is
    /// the whole premise on which the per-language files are converted one at
    /// a time rather than all at once.
    #[test]
    fn the_two_units_agree_on_every_basic_multilingual_plane_input() {
        const TABLE: &[&str] = &["a", "e", "ção", "heten", "ost", "ère", "ь", "ий", ""];
        for cp in 0..=0xFFFFu32 {
            let Some(c) = char::from_u32(cp) else {
                continue;
            };
            let s = format!("x{c}ção");
            let (cs, us) = (scalars(&s), units(&s));
            assert_eq!(cs.len(), us.len(), "U+{cp:04X}");
            assert_eq!(text(&cs), text(&us), "U+{cp:04X}");
            for t in TABLE {
                assert_eq!(ends_with(&cs, t), ends_with(&us, t), "U+{cp:04X} {t:?}");
                assert_eq!(starts_with(&cs, t), starts_with(&us, t), "U+{cp:04X} {t:?}");
                assert_eq!(eq_str(&cs, t), eq_str(&us, t), "U+{cp:04X} {t:?}");
            }
            assert_eq!(
                longest_suffix(&cs, TABLE),
                longest_suffix(&us, TABLE),
                "U+{cp:04X}"
            );
            assert_eq!(
                first_suffix(&cs, TABLE),
                first_suffix(&us, TABLE),
                "U+{cp:04X}"
            );
        }
    }

    #[test]
    fn longest_and_first_differ() {
        let w = scalars("running");
        assert_eq!(longest_suffix(&w, &["ing", "ning"]), Some("ning"));
        assert_eq!(first_suffix(&w, &["ing", "ning"]), Some("ing"));
    }

    #[test]
    fn a_length_counts_scalar_values() {
        assert_eq!(slen(""), 0);
        assert_eq!(slen("abc"), 3);
        assert_eq!(slen("é"), 1);
        assert_eq!(slen("😀"), 1);
        assert_eq!(slen("😀s"), 2);
        assert_eq!(slen("𝟎𝟏𝟐"), 3);
        for s in ["", "a", "é", "日本語", "😀", "😀s", "𝟎𝟏𝟐", "a😀b𝟎c"] {
            assert_eq!(slen(s), s.chars().count(), "{s:?}");
        }
    }

    /// The lemma every transitional `slen(literal)` buffer subtraction rests
    /// on: on the Basic Multilingual Plane a scalar value *is* a code unit.
    #[test]
    fn on_the_basic_multilingual_plane_the_two_units_coincide() {
        for cp in 0..=0xFFFFu32 {
            let Some(c) = char::from_u32(cp) else {
                continue;
            };
            let s = c.to_string();
            assert_eq!(slen(&s), 1, "U+{cp:04X}");
            assert_eq!(slen(&s), s.encode_utf16().count(), "U+{cp:04X}");
        }
    }

    /// The unit is not an internal detail: it decides what a public stemmer
    /// returns.
    ///
    /// `en.rs`'s documented entry gate is `if length < 3 { return token }`.
    /// `"😀s"` is two scalar values, so the gate returns it untouched. Under
    /// the UTF-16 reading the same token measured three units, the algorithm
    /// ran, and the `s` was cut. The assertion lives here rather than in
    /// `en.rs` because it pins *this module's* contract, observed through the
    /// nearest caller, not English's algorithm.
    #[test]
    fn the_unit_is_observable_through_a_public_stemmer() {
        assert_eq!(crate::PorterStemmer::new().stem("😀s"), "😀s");
    }

    #[test]
    fn ends_with_fast_path_agrees_with_the_full_comparison() {
        let w = scalars("cab");
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
        assert!(!ends_with(&scalars(""), "b"));
        // An astral last character: `last_of` must report the whole scalar
        // under the scalar unit and the low surrogate under the transitional
        // one, so that both agree with their own buffer's last position.
        assert!(ends_with(&scalars("a😀"), "😀"));
        assert!(!ends_with(&scalars("a😀"), "🙂"));
        assert!(ends_with(&units("a😀"), "😀"));
        assert!(!ends_with(&units("a😀"), "🙂"));
    }

    #[test]
    fn push_and_truncate_move_whole_characters() {
        let mut w = scalars("cora");
        push_str(&mut w, "ção");
        assert_eq!(text(&w), "coração");
        truncate_by(&mut w, 3);
        assert_eq!(text(&w), "cora");
        replace_suffix(&mut w, "ra", "😀");
        assert_eq!(text(&w), "co😀");
        assert_eq!(at(&w, 2), Some('😀'));
        assert_eq!(at(&w, 3), None);
    }

    #[test]
    fn in_set_is_the_same_predicate_for_both_units() {
        const LO: u128 = set_lo(&[u('a'), u('e')]);
        const HI: u128 = set_hi(&[u('a'), u('e'), u('ø')]);
        assert!(in_set('a', LO, HI));
        assert!(in_set('ø', LO, HI));
        assert!(!in_set('b', LO, HI));
        // No astral character is admitted under either reading: it is neither
        // below U+0100 itself, nor are the surrogate halves it encodes to.
        assert!(!in_set('😀', LO, HI));
        for unit in "😀".encode_utf16() {
            assert!(!in_set(unit, LO, HI));
        }
    }

    /// `text_utf16`'s hand-rolled BMP encoder must be indistinguishable from
    /// the standard library's, including on the surrogate inputs that send it
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

    /// Decoding a scalar buffer round-trips every string exactly, for every
    /// scalar value there is.
    #[test]
    fn decoding_a_scalar_buffer_round_trips() {
        for cp in 0..=0x10_FFFFu32 {
            let Some(c) = char::from_u32(cp) else {
                continue;
            };
            let s = c.to_string();
            assert_eq!(text(&scalars(&s)), s, "U+{cp:04X}");
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
            // Both implementations of `Unit::lower` re-type the same table.
            assert_eq!(<u16 as Unit>::lower(c), Some(fast), "U+{cp:04X}");
            assert_eq!(
                <char as Unit>::lower(c),
                char::from_u32(u32::from(fast)),
                "U+{cp:04X}"
            );
        }
    }

    /// The callback form must reproduce `str::to_lowercase` exactly — over
    /// every single-scalar string, over the sigma cases the per-character
    /// mapping cannot decide, and over random mixed strings — in **both**
    /// units.
    #[test]
    fn for_each_lowercase_unit_agrees_with_to_lowercase() {
        let lower_c = |s: &str| {
            let mut v: Vec<char> = Vec::new();
            for_each_lowercase_unit(s, |c| v.push(c));
            v
        };
        let lower_u = |s: &str| {
            let mut v: Vec<u16> = Vec::new();
            for_each_lowercase_unit(s, |c| v.push(c));
            v
        };
        let check = |s: &str| {
            let want = s.to_lowercase();
            assert_eq!(lower_c(s), scalars(&want), "{s:?}");
            assert_eq!(lower_u(s), units(&want), "{s:?}");
        };
        for cp in 0..=0x10_FFFFu32 {
            let Some(c) = char::from_u32(cp) else {
                continue;
            };
            check(&c.to_string());
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
            check(s);
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
            check(&s);
        }
    }

    #[test]
    fn text_lowercase_folds_in_place_for_both_units() {
        for s in ["ABC", "AbC", "", "ÉCOLE", "aBcD", "ÀÖØ"] {
            let mut c = scalars(s);
            let mut u = units(s);
            assert_eq!(text_lowercase(&mut c), s.to_lowercase(), "{s:?}");
            assert_eq!(text_lowercase(&mut u), s.to_lowercase(), "{s:?}");
        }
    }

    /// The ASCII path must be unaffected by the unit: identical answers,
    /// through every helper, for every ASCII string of up to three bytes.
    #[test]
    fn ascii_answers_are_identical_under_both_units() {
        const TABLE: &[&str] = &["a", "s", "es", "ed", "ing", "z", ""];
        let mut buf = String::new();
        for a in 0..=0x7Fu8 {
            for b in 0..=0x7Fu8 {
                buf.clear();
                buf.push(char::from(a));
                buf.push(char::from(b));
                let (cs, us) = (scalars(&buf), units(&buf));
                assert_eq!(cs.len(), us.len());
                assert_eq!(slen(&buf), us.len());
                assert_eq!(text(&cs), text(&us));
                assert_eq!(text(&cs), buf);
                for t in TABLE {
                    assert_eq!(ends_with(&cs, t), ends_with(&us, t), "{buf:?} {t:?}");
                    assert_eq!(starts_with(&cs, t), starts_with(&us, t), "{buf:?} {t:?}");
                }
                assert_eq!(longest_suffix(&cs, TABLE), longest_suffix(&us, TABLE));
                assert_eq!(
                    borrowed_prefix(&buf, 1, true, false),
                    Some(&buf[..1]),
                    "{buf:?}"
                );
            }
        }
    }
}
