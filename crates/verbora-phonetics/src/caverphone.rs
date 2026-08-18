//! Caverphone 1.0 and 2.0, David Hood's New Zealand English matchers.
//!
//! # Provenance
//!
//! Caverphone was created by David Hood at the University of Otago for the
//! Caversham Project, to match spelling variants of names in historical
//! Dunedin electoral rolls:
//!
//! * **Caverphone 1.0** — "Caverphone: Phonetic Matching Algorithm",
//!   Technical Paper CTP060902, University of Otago, 2002. Codes are **six**
//!   characters.
//! * **Caverphone 2.0** — "Caverphone Revisited", Technical Paper CTP150804,
//!   University of Otago, 2004. Codes are **ten** characters, final `e` is
//!   dropped, and `y`, word-final `w`/`r`/`l` and trailing vowels are handled
//!   differently.
//!
//! Both papers were implemented by Apache commons-codec, which is in turn the
//! source of the [rphonetic](https://docs.rs/rphonetic) Rust port. This
//! algorithm is **not** part of the JS reference the rest of this crate ports;
//! following the crate's convention for such extensions ([`BeiderMorse`]
//! [`crate::BeiderMorse`] is the precedent), its behaviour is pinned to one
//! canonical implementation: **rphonetic 3.0.6** (`src/caverphone.rs`), byte
//! for byte, on every input rphonetic accepts. The single divergence — inputs
//! on which rphonetic *panics* — is documented below.
//!
//! # The algorithm
//!
//! Both versions are one long **ordered** cascade of rewrites over a
//! normalized word: lowercase it, drop non-letters, then apply several dozen
//! substitutions in a fixed sequence (`c`→`k`, `dg`→`2g`, vowels→`3`, …).
//! The digits are markers, not output: `2` means "delete", `3` means "vowel".
//! Runs of `s t p k f m n` compact to one uppercase letter; `w r l y h` are
//! kept only where a vowel follows (uppercased) and deleted otherwise. At the
//! end all `2`s are removed, all `3`s are removed (Caverphone 2.0 first turns
//! a *trailing* `3` into `A`), and the result is padded with `1`s and cut to
//! the fixed code length. **The ordering of the cascade is load-bearing** —
//! e.g. `tch`→`2ch` must precede `c`→`k`, and the `w3`→`W3` marking must
//! follow the `stpkfmn` compaction — so [`Caverphone1::process`] and
//! [`Caverphone2::process`] reproduce rphonetic's exact sequence, step for
//! step, in a single reused buffer instead of rphonetic's one freshly
//! allocated `String` per step.
//!
//! # Behavioural decisions (all pinned to rphonetic 3.0.6)
//!
//! * **Normalization keeps every Unicode-lowercase character, not just
//!   `a`–`z`.** rphonetic lowercases with `str::to_lowercase` and then keeps
//!   the characters for which `char::is_lowercase` is true. Accented and
//!   non-Latin *cased* letters (`é`, `ß`, `м`, …) therefore survive
//!   normalization, pass through the cascade untouched, and appear verbatim
//!   in the code, while digits, punctuation, whitespace, and uncased scripts
//!   (CJK, emoji) are dropped. `process("Москва")` really is `"мос"` + no
//!   room for padding — see the next point.
//! * **The code length is measured in bytes.** rphonetic appends the padding
//!   (`"111111"` / `"1111111111"`) and then slices `&txt[0..6]` /
//!   `&txt[0..10]` — a *byte* slice. A surviving multi-byte character counts
//!   as more than one position: `Caverphone1` maps `"café"` to `"KF\u{e9}11"`,
//!   six bytes but five characters. Every code this module returns is exactly
//!   6 (v1) or 10 (v2) bytes.
//! * **Divergence — the one place we do not copy rphonetic:** when that byte
//!   slice would split a multi-byte character (the character straddles byte
//!   index 6/10 of the padded rewrite result), rphonetic **panics** ("byte
//!   index is not a char boundary"). A text library must not panic on
//!   punctuation-adjacent input, so this module instead drops the straddling
//!   character and pads with `1`s to the exact code length:
//!   `Caverphone1::process("péééé")` is `"P\u{e9}\u{e9}1"` where rphonetic
//!   aborts. Affected inputs are exactly those whose *rewrite result* places
//!   a multi-byte (necessarily non-ASCII, Unicode-lowercase) character across
//!   the 6- or 10-byte boundary; they are excluded from the benchmark domain
//!   per the crate's fairness pattern. ASCII input can never reach this path.
//! * **Empty and letter-free input yields the all-`1` code** (`"111111"` /
//!   `"1111111111"`). rphonetic special-cases `""`; input that merely
//!   *normalizes* to empty (digits, spaces, emoji) falls through its cascade
//!   to the same value, so one uniform path here is exact.
//! * **Final sigma:** normalization is `str::to_lowercase`, not per-`char`
//!   lowercasing, so `"ΑΣ"` keeps Unicode's final form `ς` exactly as
//!   rphonetic does.
//!
//! ```
//! use verbora_phonetics::caverphone::{Caverphone1, Caverphone2};
//!
//! assert_eq!(Caverphone1::new().process("Thompson"), "TMPSN1");
//! assert_eq!(Caverphone2::new().process("Thompson"), "TMPSN11111");
//! // The 2004 revision keeps a trailing vowel (as `A`) that 1.0 discards:
//! assert_eq!(Caverphone1::new().process("ready"), "RT1111");
//! assert_eq!(Caverphone2::new().process("ready"), "RTA1111111");
//! ```

/// Code length of Caverphone 1.0, in bytes.
const LEN_V1: usize = 6;
/// Code length of Caverphone 2.0, in bytes.
const LEN_V2: usize = 10;

/// Caverphone 1.0 (Hood 2002); six-byte codes.
///
/// Pinned byte-for-byte to rphonetic 3.0.6 — see the [module docs](self) for
/// the algorithm, the normalization rules, and the single divergence.
///
/// ```
/// use verbora_phonetics::caverphone::Caverphone1;
///
/// let caverphone = Caverphone1::new();
/// assert_eq!(caverphone.process("Thompson"), "TMPSN1");
/// assert_eq!(caverphone.process("Lee"), "L11111");
/// assert!(caverphone.compare("Peter", "Peady"));
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Caverphone1;

/// Caverphone 2.0 (Hood 2004, "Caverphone Revisited"); ten-byte codes.
///
/// Pinned byte-for-byte to rphonetic 3.0.6 — see the [module docs](self) for
/// the algorithm, the normalization rules, and the single divergence.
///
/// ```
/// use verbora_phonetics::caverphone::Caverphone2;
///
/// let caverphone = Caverphone2::new();
/// assert_eq!(caverphone.process("Thompson"), "TMPSN11111");
/// assert_eq!(caverphone.process("Stevenson"), "STFNSN1111");
/// assert!(caverphone.compare("Peter", "Peady"));
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Caverphone2;

impl Caverphone1 {
    /// Creates a Caverphone 1.0 encoder. It holds no state; the type exists
    /// to mirror rphonetic's `Caverphone1`.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Encodes `token` as a six-byte Caverphone 1.0 code.
    ///
    /// ```
    /// use verbora_phonetics::caverphone::Caverphone1;
    ///
    /// let caverphone = Caverphone1::new();
    /// assert_eq!(caverphone.process("David"), "TFT111");
    /// assert_eq!(caverphone.process("Whittle"), "WTL111");
    /// assert_eq!(caverphone.process(""), "111111");
    /// ```
    #[must_use]
    pub fn process(&self, token: &str) -> String {
        encode(token, Version::V1)
    }

    /// Whether two words share a Caverphone 1.0 code.
    ///
    /// ```
    /// use verbora_phonetics::caverphone::Caverphone1;
    ///
    /// let caverphone = Caverphone1::new();
    /// assert!(caverphone.compare("Peter", "Peady"));
    /// assert!(!caverphone.compare("Peter", "Stevenson"));
    /// ```
    #[must_use]
    pub fn compare(&self, a: &str, b: &str) -> bool {
        self.process(a) == self.process(b)
    }
}

impl Caverphone2 {
    /// Creates a Caverphone 2.0 encoder. It holds no state; the type exists
    /// to mirror rphonetic's `Caverphone2`.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Encodes `token` as a ten-byte Caverphone 2.0 code.
    ///
    /// ```
    /// use verbora_phonetics::caverphone::Caverphone2;
    ///
    /// let caverphone = Caverphone2::new();
    /// assert_eq!(caverphone.process("Stevenson"), "STFNSN1111");
    /// assert_eq!(caverphone.process("ready"), "RTA1111111");
    /// assert_eq!(caverphone.process(""), "1111111111");
    /// ```
    #[must_use]
    pub fn process(&self, token: &str) -> String {
        encode(token, Version::V2)
    }

    /// Whether two words share a Caverphone 2.0 code.
    ///
    /// ```
    /// use verbora_phonetics::caverphone::Caverphone2;
    ///
    /// let caverphone = Caverphone2::new();
    /// assert!(caverphone.compare("Peter", "Peady"));
    /// assert!(!caverphone.compare("Peter", "Stevenson"));
    /// ```
    #[must_use]
    pub fn compare(&self, a: &str, b: &str) -> bool {
        self.process(a) == self.process(b)
    }
}

impl verbora_core::Phonetic for Caverphone1 {
    fn process(&self, token: &str) -> String {
        Self::process(self, token)
    }

    fn compare(&self, a: &str, b: &str) -> bool {
        Self::compare(self, a, b)
    }
}

impl verbora_core::Phonetic for Caverphone2 {
    fn process(&self, token: &str) -> String {
        Self::process(self, token)
    }

    fn compare(&self, a: &str, b: &str) -> bool {
        Self::compare(self, a, b)
    }
}

/// Which cascade to run. The two versions share most of their steps but
/// interleave their differences, so one ordered function with version guards
/// keeps the load-bearing sequence in a single place, in rphonetic's order.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Version {
    V1,
    V2,
}

/// The whole cascade, in rphonetic's exact step order.
///
/// rphonetic allocates a fresh `String` for every one of its ~40 steps. Every
/// step here except normalization, compaction, marker removal, and padding is
/// **length-preserving**, so the entire cascade runs in place in one buffer:
/// the shrinking steps use a read/write cursor, and padding reuses the spare
/// capacity reserved up front. One allocation per call on ASCII input (the
/// buffer, which becomes the returned `String`), two on non-ASCII input
/// (`str::to_lowercase` needs an intermediate — see [`normalize`]).
fn encode(token: &str, version: Version) -> String {
    let v2 = version == Version::V2;
    let target = if v2 { LEN_V2 } else { LEN_V1 };
    let mut buf = normalize(token, target);

    // v2 only: drop one final `e` (helper::replace_end(txt, "e", "")).
    if v2 && buf.last() == Some(&b'e') {
        buf.pop();
    }

    // Leading irregularities. Each rphonetic step is `starts_with` guarded
    // `replacen(pat, rep, 1)`, i.e. a prefix rewrite; all are length-preserving.
    replace_prefix(&mut buf, b"cough", b"cou2f");
    replace_prefix(&mut buf, b"rough", b"rou2f");
    replace_prefix(&mut buf, b"tough", b"tou2f");
    replace_prefix(&mut buf, b"enough", b"enou2f");
    if v2 {
        replace_prefix(&mut buf, b"trough", b"trou2f");
    }
    replace_prefix(&mut buf, b"gn", b"2n");

    // Trailing `mb` → `m2` (helper::replace_end).
    if buf.ends_with(b"mb") {
        let last = buf.len() - 1;
        buf[last] = b'2';
    }

    // The consonant cascade, shared verbatim by both versions.
    replace_pair(&mut buf, b"cq", b"2q");
    replace_pair(&mut buf, b"ci", b"si");
    replace_pair(&mut buf, b"ce", b"se");
    replace_pair(&mut buf, b"cy", b"sy");
    replace_run(&mut buf, b"tch", b"2ch");
    map_byte(&mut buf, b'c', b'k');
    map_byte(&mut buf, b'q', b'k');
    map_byte(&mut buf, b'x', b'k');
    map_byte(&mut buf, b'v', b'f');
    replace_pair(&mut buf, b"dg", b"2g");
    replace_run(&mut buf, b"tio", b"sio");
    replace_run(&mut buf, b"tia", b"sia");
    map_byte(&mut buf, b'd', b't');
    replace_pair(&mut buf, b"ph", b"fh");
    map_byte(&mut buf, b'b', b'p');
    replace_pair(&mut buf, b"sh", b"s2");
    map_byte(&mut buf, b'z', b's');

    // Initial vowel → `A`, every other vowel → `3`. rphonetic runs two
    // char-map passes; fused here, first byte special-cased. A multi-byte
    // first character has a lead byte >= 0x80, which no vowel test matches,
    // exactly as `helper::is_vowel` rejects any non-ASCII char.
    if let Some(first) = buf.first_mut()
        && is_vowel(*first)
    {
        *first = b'A';
    }
    for b in &mut buf {
        if is_vowel(*b) {
            *b = b'3';
        }
    }

    // v2 only: `y` is vowel-like. `j`→`y` first, then a *prefix* `y3`→`Y3`
    // or bare leading `y`→`A`, then every remaining `y`→`3`.
    if v2 {
        map_byte(&mut buf, b'j', b'y');
        if buf.starts_with(b"y3") {
            buf[0] = b'Y';
        } else if buf.first() == Some(&b'y') {
            buf[0] = b'A';
        }
        map_byte(&mut buf, b'y', b'3');
    }

    replace_run(&mut buf, b"3gh3", b"3kh3");
    replace_pair(&mut buf, b"gh", b"22");
    map_byte(&mut buf, b'g', b'k');

    // Runs of `s t p k f m n` compact to a single uppercase letter
    // (helper::replace_compact_all_to_uppercase). Shrinking; done in place.
    compact_stpkfmn(&mut buf);

    // `w`, `h`, `r`, `l` (and, in v1, `y`) survive as uppercase only before a
    // vowel; otherwise they become deletion markers. Order is rphonetic's.
    replace_pair(&mut buf, b"w3", b"W3");
    if !v2 {
        replace_pair(&mut buf, b"wy", b"Wy");
    }
    replace_run(&mut buf, b"wh3", b"Wh3");
    if !v2 {
        replace_run(&mut buf, b"why", b"Why");
    }
    if v2 && buf.last() == Some(&b'w') {
        let last = buf.len() - 1;
        buf[last] = b'3';
    }
    map_byte(&mut buf, b'w', b'2');

    if buf.first() == Some(&b'h') {
        buf[0] = b'A';
    }
    map_byte(&mut buf, b'h', b'2');

    replace_pair(&mut buf, b"r3", b"R3");
    if v2 {
        if buf.last() == Some(&b'r') {
            let last = buf.len() - 1;
            buf[last] = b'3';
        }
    } else {
        replace_pair(&mut buf, b"ry", b"Ry");
    }
    map_byte(&mut buf, b'r', b'2');

    replace_pair(&mut buf, b"l3", b"L3");
    if v2 {
        if buf.last() == Some(&b'l') {
            let last = buf.len() - 1;
            buf[last] = b'3';
        }
    } else {
        replace_pair(&mut buf, b"ly", b"Ly");
    }
    map_byte(&mut buf, b'l', b'2');

    // v1 only: `y` is consonant-like and handled last.
    if !v2 {
        map_byte(&mut buf, b'j', b'y');
        replace_pair(&mut buf, b"y3", b"Y3");
        map_byte(&mut buf, b'y', b'2');
    }

    // Strip the markers. v2 keeps a trailing vowel as `A` first.
    remove_byte(&mut buf, b'2');
    if v2 && buf.last() == Some(&b'3') {
        let last = buf.len() - 1;
        buf[last] = b'A';
    }
    remove_byte(&mut buf, b'3');

    // Pad with `1`s and cut to the fixed BYTE length, as rphonetic's
    // `(txt + "111111")[0..6]` does. Where that byte slice would split a
    // multi-byte character rphonetic panics; we drop the straddling character
    // and let the padding fill back to exactly `target` bytes (the module
    // docs call out this sole divergence).
    if buf.len() > target {
        let mut cut = target;
        while cut > 0 && buf[cut] & 0xC0 == 0x80 {
            cut -= 1;
        }
        buf.truncate(cut);
    }
    buf.resize(target, b'1');

    debug_assert_eq!(buf.len(), target);
    String::from_utf8(buf).expect("cascade edits are ASCII-for-ASCII on valid UTF-8")
}

/// Lowercases `token` and keeps only its Unicode-lowercase characters, as
/// rphonetic's `to_lowercase()` + `helper::remove_all_non_letter` do.
///
/// ASCII input — the overwhelmingly common case — runs byte-wise with no
/// intermediate allocation. Non-ASCII input takes `str::to_lowercase` (whose
/// word-final-sigma rule is observable in the output, so a per-`char`
/// lowercase would be wrong) and then the same `char::is_lowercase` filter.
fn normalize(token: &str, target: usize) -> Vec<u8> {
    // The buffer only ever shrinks until padding, so `max(len, target)`
    // capacity guarantees zero reallocation for the whole cascade.
    let mut buf = Vec::with_capacity(token.len().max(target));
    if token.is_ascii() {
        for &b in token.as_bytes() {
            if b.is_ascii_alphabetic() {
                buf.push(b | 0x20);
            }
        }
    } else {
        for c in token.to_lowercase().chars() {
            if c.is_lowercase() {
                let mut utf8 = [0u8; 4];
                buf.extend_from_slice(c.encode_utf8(&mut utf8).as_bytes());
            }
        }
    }
    buf
}

/// `[aeiou]`, lowercase ASCII only — rphonetic's `helper::is_vowel(_, false)`.
#[inline]
const fn is_vowel(b: u8) -> bool {
    matches!(b, b'a' | b'e' | b'i' | b'o' | b'u')
}

/// `String::replace(char, char)`: an unconditional per-byte map.
///
/// All mapped bytes are ASCII, and no ASCII byte can occur inside a UTF-8
/// multi-byte sequence, so a byte map on the buffer is exactly a char map on
/// the string.
#[inline]
fn map_byte(buf: &mut [u8], from: u8, to: u8) {
    for b in buf {
        if *b == from {
            *b = to;
        }
    }
}

/// The `starts_with(pat)`-guarded `replacen(pat, rep, 1)` steps: rewrites a
/// prefix in place. `pat` and `rep` have equal length everywhere they occur.
#[inline]
fn replace_prefix(buf: &mut [u8], pat: &[u8], rep: &[u8]) {
    debug_assert_eq!(pat.len(), rep.len());
    if buf.starts_with(pat) {
        buf[..rep.len()].copy_from_slice(rep);
    }
}

/// `String::replace(pat, rep)` for equal-length `pat`/`rep`, in place.
///
/// `str::replace` collects the non-overlapping matches of `pat` left to
/// right over the *original* string and splices `rep` in. Because `rep` is
/// the same length as `pat`, a forward scan that rewrites each match in place
/// and resumes *after* it sees original bytes everywhere ahead of the cursor
/// and final bytes only behind it — the same matches, the same result, no
/// second buffer. (Patterns are all-ASCII, so as in [`map_byte`] they can
/// never match inside a multi-byte character.)
#[inline]
fn replace_run(buf: &mut [u8], pat: &[u8], rep: &[u8]) {
    debug_assert_eq!(pat.len(), rep.len());
    if buf.len() < pat.len() {
        return;
    }
    let mut i = 0;
    while i + pat.len() <= buf.len() {
        if buf[i..].starts_with(pat) {
            buf[i..i + rep.len()].copy_from_slice(rep);
            i += pat.len();
        } else {
            i += 1;
        }
    }
}

/// [`replace_run`] specialized to the cascade's many two-byte rewrites, whose
/// window fits one comparison.
#[inline]
fn replace_pair(buf: &mut [u8], pat: &[u8; 2], rep: &[u8; 2]) {
    let mut i = 0;
    while i + 2 <= buf.len() {
        if buf[i] == pat[0] && buf[i + 1] == pat[1] {
            buf[i] = rep[0];
            buf[i + 1] = rep[1];
            i += 2;
        } else {
            i += 1;
        }
    }
}

/// rphonetic's `helper::replace_compact_all_to_uppercase(txt, [s,t,p,k,f,m,n])`:
/// each maximal run of one of those letters collapses to a single uppercase
/// copy; any other character (multi-byte ones included, byte by byte) passes
/// through and breaks the run. Shrinking, so read/write cursors in place.
fn compact_stpkfmn(buf: &mut Vec<u8>) {
    let mut write = 0;
    let mut previous = 0u8; // never a valid letter, so "no previous"
    for read in 0..buf.len() {
        let b = buf[read];
        if matches!(b, b's' | b't' | b'p' | b'k' | b'f' | b'm' | b'n') {
            if previous != b {
                buf[write] = b.to_ascii_uppercase();
                write += 1;
                previous = b;
            }
        } else {
            buf[write] = b;
            write += 1;
            previous = 0;
        }
    }
    buf.truncate(write);
}

/// `String::replace(char, "")`: deletes every occurrence of `b`, in place.
fn remove_byte(buf: &mut Vec<u8>, byte: u8) {
    let mut write = 0;
    for read in 0..buf.len() {
        let b = buf[read];
        if b != byte {
            buf[write] = b;
            write += 1;
        }
    }
    buf.truncate(write);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v1(token: &str) -> String {
        Caverphone1::new().process(token)
    }

    fn v2(token: &str) -> String {
        Caverphone2::new().process(token)
    }

    // -- ports of rphonetic 3.0.6's caverphone.rs test suite (which is in
    //    turn commons-codec's CaverphoneTest/Caverphone2Test) ---------------

    /// rphonetic `test_caverphone1_revisited_common_code_at1111`.
    #[test]
    fn caverphone1_common_code_at1111() {
        for input in [
            "add", "aid", "at", "art", "eat", "earth", "head", "hit", "hot", "hold", "hard",
            "heart", "it", "out", "old",
        ] {
            assert_eq!(v1(input), "AT1111", "for {input:?}");
        }
    }

    /// rphonetic `test_end_mb_caverphone1`.
    #[test]
    fn caverphone1_end_mb() {
        assert_eq!(v1("mb"), "M11111");
        assert_eq!(v1("mbmb"), "MPM111");
    }

    /// rphonetic `test_is_caverphone1_equals`.
    #[test]
    fn caverphone1_compare() {
        let caverphone = Caverphone1::new();
        assert!(!caverphone.compare("Peter", "Stevenson"));
        assert!(caverphone.compare("Peter", "Peady"));
    }

    /// rphonetic `test_specification_v1examples`.
    #[test]
    fn caverphone1_specification_examples() {
        assert_eq!(v1("David"), "TFT111");
        assert_eq!(v1("Whittle"), "WTL111");
    }

    /// rphonetic `test_wikipedia_examples`.
    #[test]
    fn caverphone1_wikipedia_examples() {
        assert_eq!(v1("Lee"), "L11111");
        assert_eq!(v1("Thompson"), "TMPSN1");
    }

    /// rphonetic `test_caverphone_revisited_common_code_at11111111`.
    #[test]
    fn caverphone2_common_code_at11111111() {
        for input in [
            "add", "aid", "at", "art", "eat", "earth", "head", "hit", "hot", "hold", "hard",
            "heart", "it", "out", "old",
        ] {
            assert_eq!(v2(input), "AT11111111", "for {input:?}");
        }
    }

    /// rphonetic `test_caverphone_revisited_examples`.
    #[test]
    fn caverphone2_revisited_examples() {
        assert_eq!(v2("Stevenson"), "STFNSN1111");
        assert_eq!(v2("Peter"), "PTA1111111");
    }

    /// rphonetic `test_caverphone_revisited_random_name_kln1111111`.
    #[test]
    fn caverphone2_names_encoding_to_kln1111111() {
        let names = [
            "Cailean", "Calan", "Calen", "Callahan", "Callan", "Callean", "Carleen", "Carlen",
            "Carlene", "Carlin", "Carline", "Carlyn", "Carlynn", "Carlynne", "Charlean",
            "Charleen", "Charlene", "Charline", "Cherlyn", "Chirlin", "Clein", "Cleon", "Cline",
            "Cohleen", "Colan", "Coleen", "Colene", "Colin", "Colleen", "Collen", "Collin",
            "Colline", "Colon", "Cullan", "Cullen", "Cullin", "Gaelan", "Galan", "Galen", "Garlan",
            "Garlen", "Gaulin", "Gayleen", "Gaylene", "Giliane", "Gillan", "Gillian", "Glen",
            "Glenn", "Glyn", "Glynn", "Gollin", "Gorlin", "Kalin", "Karlan", "Karleen", "Karlen",
            "Karlene", "Karlin", "Karlyn", "Kaylyn", "Keelin", "Kellen", "Kellene", "Kellyann",
            "Kellyn", "Khalin", "Kilan", "Kilian", "Killen", "Killian", "Killion", "Klein",
            "Kleon", "Kline", "Koerlin", "Kylen", "Kylynn", "Quillan", "Quillon", "Qulllon",
            "Xylon",
        ];
        for name in names {
            assert_eq!(v2(name), "KLN1111111", "{name} caused the error");
        }
    }

    /// rphonetic `test_caverphone_revisited_random_name_tn11111111`.
    #[test]
    fn caverphone2_names_encoding_to_tn11111111() {
        let names = [
            "Dan", "Dane", "Dann", "Darn", "Daune", "Dawn", "Ddene", "Dean", "Deane", "Deanne",
            "DeeAnn", "Deeann", "Deeanne", "Deeyn", "Den", "Dene", "Denn", "Deonne", "Diahann",
            "Dian", "Diane", "Diann", "Dianne", "Diannne", "Dine", "Dion", "Dione", "Dionne",
            "Doane", "Doehne", "Don", "Donn", "Doone", "Dorn", "Down", "Downe", "Duane", "Dun",
            "Dunn", "Duyne", "Dyan", "Dyane", "Dyann", "Dyanne", "Dyun", "Tan", "Tann", "Teahan",
            "Ten", "Tenn", "Terhune", "Thain", "Thaine", "Thane", "Thanh", "Thayne", "Theone",
            "Thin", "Thorn", "Thorne", "Thun", "Thynne", "Tien", "Tine", "Tjon", "Town", "Towne",
            "Turne", "Tyne",
        ];
        for name in names {
            assert_eq!(v2(name), "TN11111111", "{name} caused the error");
        }
    }

    /// rphonetic `test_caverphone_revisited_random_name_tta1111111`.
    #[test]
    fn caverphone2_names_encoding_to_tta1111111() {
        let names = [
            "Darda", "Datha", "Dedie", "Deedee", "Deerdre", "Deidre", "Deirdre", "Detta", "Didi",
            "Didier", "Dido", "Dierdre", "Dieter", "Dita", "Ditter", "Dodi", "Dodie", "Dody",
            "Doherty", "Dorthea", "Dorthy", "Doti", "Dotti", "Dottie", "Dotty", "Doty", "Doughty",
            "Douty", "Dowdell", "Duthie", "Tada", "Taddeo", "Tadeo", "Tadio", "Tati", "Teador",
            "Tedda", "Tedder", "Teddi", "Teddie", "Teddy", "Tedi", "Tedie", "Teeter", "Teodoor",
            "Teodor", "Terti", "Theda", "Theodor", "Theodore", "Theta", "Thilda", "Thordia",
            "Tilda", "Tildi", "Tildie", "Tildy", "Tita", "Tito", "Tjader", "Toddie", "Toddy",
            "Torto", "Tuddor", "Tudor", "Turtle", "Tuttle", "Tutto",
        ];
        for name in names {
            assert_eq!(v2(name), "TTA1111111", "{name} caused the error");
        }
    }

    /// rphonetic `test_caverphone_revisited_random_words`.
    #[test]
    fn caverphone2_random_words() {
        assert_eq!(v2("rather"), "RTA1111111");
        assert_eq!(v2("ready"), "RTA1111111");
        assert_eq!(v2("writer"), "RTA1111111");
        assert_eq!(v2("social"), "SSA1111111");
        assert_eq!(v2("able"), "APA1111111");
        assert_eq!(v2("appear"), "APA1111111");
    }

    /// rphonetic `test_end_mb_caverphone2`.
    #[test]
    fn caverphone2_end_mb() {
        assert_eq!(v2("mb"), "M111111111");
        assert_eq!(v2("mbmb"), "MPM1111111");
    }

    /// rphonetic `test_is_caverphone2_equals`.
    #[test]
    fn caverphone2_compare() {
        let caverphone = Caverphone2::new();
        assert!(!caverphone.compare("Peter", "Stevenson"));
        assert!(caverphone.compare("Peter", "Peady"));
    }

    /// rphonetic `test_specification_examples` (the 2.0 paper's own vectors).
    #[test]
    fn caverphone2_specification_examples() {
        assert_eq!(v2("Peter"), "PTA1111111");
        assert_eq!(v2("ready"), "RTA1111111");
        assert_eq!(v2("social"), "SSA1111111");
        assert_eq!(v2("able"), "APA1111111");
        assert_eq!(v2("Tedder"), "TTA1111111");
        assert_eq!(v2("Karleen"), "KLN1111111");
        assert_eq!(v2("Dyun"), "TN11111111");
    }

    // -- hand-written edge cases, all verified against rphonetic 3.0.6 ------

    #[test]
    fn empty_input_is_all_ones() {
        assert_eq!(v1(""), "111111");
        assert_eq!(v2(""), "1111111111");
    }

    #[test]
    fn input_that_normalizes_to_empty_is_all_ones() {
        // rphonetic short-circuits only `""`; these fall through its cascade
        // to the identical all-padding code.
        for input in [
            "12345",
            "0",
            "  \t\n",
            "!@#$%^&*()",
            "'-_'",
            "日本語",
            "😀🎉",
            "中文",
        ] {
            assert_eq!(v1(input), "111111", "for {input:?}");
            assert_eq!(v2(input), "1111111111", "for {input:?}");
        }
    }

    #[test]
    fn single_letters() {
        assert_eq!(v1("a"), "A11111");
        assert_eq!(v1("b"), "P11111");
        assert_eq!(v1("e"), "A11111");
        assert_eq!(v1("h"), "A11111");
        assert_eq!(v1("j"), "111111");
        assert_eq!(v1("w"), "111111");
        assert_eq!(v1("y"), "111111");
        assert_eq!(v1("z"), "S11111");
        assert_eq!(v2("a"), "A111111111");
        assert_eq!(v2("b"), "P111111111");
        // v2 drops a final `e` before anything else, so `e` normalizes away.
        assert_eq!(v2("e"), "1111111111");
        assert_eq!(v2("h"), "A111111111");
        // v2 treats `j` as `y`, and a leading `y` becomes `A`.
        assert_eq!(v2("j"), "A111111111");
        assert_eq!(v2("y"), "A111111111");
        // v2 turns a final `w`/`r`/`l` into a vowel marker; alone, it becomes
        // the trailing-vowel `A`.
        assert_eq!(v2("w"), "A111111111");
        assert_eq!(v2("r"), "A111111111");
        assert_eq!(v2("l"), "A111111111");
    }

    #[test]
    fn case_and_embedded_junk_are_ignored() {
        assert_eq!(v1("THOMPSON"), v1("thompson"));
        assert_eq!(v1("ThOmPsOn"), v1("thompson"));
        assert_eq!(v2("STEVENSON"), v2("stevenson"));
        assert_eq!(v1("O'Brien"), v1("obrien"));
        assert_eq!(v2("O'Brien"), v2("obrien"));
        assert_eq!(v1("Th om ps on"), v1("Thompson"));
        assert_eq!(v2("Th-om-ps-on!!"), v2("Thompson"));
        assert_eq!(v1("Thompson123"), v1("Thompson"));
    }

    #[test]
    fn leading_irregular_prefixes() {
        // cough/rough/tough/enough keep their `f` sound; `gn` drops the `g`.
        assert_eq!(v1("cough"), "KF1111");
        assert_eq!(v1("rough"), "RF1111");
        assert_eq!(v1("tough"), "TF1111");
        assert_eq!(v1("enough"), "ANF111");
        assert_eq!(v1("gnome"), "NM1111");
        assert_eq!(v2("cough"), "KF11111111");
        assert_eq!(v2("rough"), "RF11111111");
        assert_eq!(v2("tough"), "TF11111111");
        assert_eq!(v2("enough"), "ANF1111111");
        assert_eq!(v2("gnome"), "NM11111111");
        // `trough` is special-cased only in v2 (keeping its `f` sound); v1's
        // generic `gh` → `22` rule deletes the `gh` outright instead.
        assert_eq!(v1("trough"), "TR1111");
        assert_eq!(v2("trough"), "TRF1111111");
        // The prefix checks run against the NORMALIZED string.
        assert_eq!(v1("  Cough"), v1("cough"));
        // ...and only against the prefix: an interior `rough` is untouched.
        assert_eq!(v1("borough"), "PR1111");
    }

    #[test]
    fn version_differences_on_the_same_words() {
        // Trailing vowels: v1 deletes them, v2 keeps one as `A`.
        assert_eq!(v1("Peter"), "PT1111");
        assert_eq!(v2("Peter"), "PTA1111111");
        assert_eq!(v1("ready"), "RT1111");
        assert_eq!(v2("ready"), "RTA1111111");
        // v2 drops the final `e` BEFORE the cascade, so `Lee` becomes `l` +
        // vowel, and the trailing vowel then surfaces as `A`; v1 keeps both
        // `e`s as vowels and deletes them at the end.
        assert_eq!(v1("Lee"), "L11111");
        assert_eq!(v2("Lee"), "LA11111111");
        // `y` between consonants: consonant-like (kept as `Y` before a
        // vowel) in v1, vowel (deleted) in v2.
        assert_eq!(v1("Dyun"), "TYN111");
        assert_eq!(v2("Dyun"), "TN11111111");
    }

    #[test]
    fn code_is_a_pure_truncation_of_longer_words() {
        // The cascade output exceeds the code length and is simply cut.
        assert_eq!(v1("Stevenson"), "STFNSN");
        assert_eq!(v1("supercalifragilisticexpialidocious"), "SPKLFR");
        assert_eq!(v2("supercalifragilisticexpialidocious"), "SPKLFRKLST");
        assert_eq!(v1("mississippi"), "MSSP11");
        assert_eq!(v2("mississippi"), "MSSPA11111");
    }

    #[test]
    fn very_long_input_does_not_overflow_anything() {
        assert_eq!(v1(&"a".repeat(10_000)), "A11111");
        // v2's trailing-vowel rule turns the LAST `3` into `A` too.
        assert_eq!(v2(&"a".repeat(10_000)), "AA11111111");
        // The vowels between the `p`s prevent compaction, so every `b`
        // survives as its own `P`.
        assert_eq!(v1(&"ab".repeat(5_000)), "APPPPP");
        assert_eq!(v2(&"ab".repeat(5_000)), "APPPPPPPPP");
        assert_eq!(v2(&"stevenson".repeat(2_000)), "STFNSNSTFN");
    }

    #[test]
    fn marker_letters_never_leak_into_the_code() {
        // `2` and `3` are internal markers; whole-alphabet input exercises
        // every rule and must produce only [A-Z1] output.
        for input in ["abcdefghijklmnopqrstuvwxyz", "zyxwvutsrqponmlkjihgfedcba"] {
            for code in [v1(input), v2(input)] {
                assert!(
                    code.bytes().all(|b| b == b'1' || b.is_ascii_uppercase()),
                    "marker leaked in {code:?}"
                );
            }
        }
        assert_eq!(v1("abcdefghijklmnopqrstuvwxyz"), "APKTFK");
        assert_eq!(v2("abcdefghijklmnopqrstuvwxyz"), "APKTFKMNPK");
        assert_eq!(v1("zyxwvutsrqponmlkjihgfedcba"), "SKFTSK");
        assert_eq!(v2("zyxwvutsrqponmlkjihgfedcba"), "SKFTSKPNMK");
    }

    // -- non-ASCII behaviour, pinned to rphonetic (see module docs) ---------

    #[test]
    fn unicode_lowercase_letters_survive_into_the_code() {
        // rphonetic keeps every Unicode-lowercase char, and slices BYTES.
        assert_eq!(v1("é"), "é1111"); // 6 bytes, 5 chars
        assert_eq!(v2("é"), "é11111111");
        assert_eq!(v1("café"), "KFé11");
        assert_eq!(v2("café"), "KFé111111");
        assert_eq!(v1("ß"), "ß1111");
        assert_eq!(v1("straße"), "STRß1");
        assert_eq!(v2("straße"), "STRß11111");
        // Uppercase non-ASCII is lowercased first, then kept.
        assert_eq!(v1("É"), "é1111");
        // Cyrillic is cased, so it passes straight through; the 2-byte chars
        // fill the whole 6-/10-byte code, leaving no room for padding.
        assert_eq!(v1("Москва"), "мос");
        assert_eq!(v2("Москва"), "москв");
        assert_eq!(v1("пятница"), "пят");
        // Word-final sigma follows str::to_lowercase, like rphonetic. Note
        // Greek alpha is not ASCII `a`, so it is not treated as a vowel.
        assert_eq!(v1("ΑΣ"), "ας11");
        assert_eq!(v2("ΑΣ"), "ας111111");
    }

    #[test]
    fn char_straddling_the_cut_is_dropped_not_panicked() {
        // DOCUMENTED DIVERGENCE: rphonetic panics on these ("byte index 6/10
        // is not a char boundary"); we drop the straddling char and pad back
        // to the exact byte length. `p` compacts to `P` and the four/eight
        // `é`s pass through, so the third/fifth `é` straddles the cut.
        assert_eq!(v1("péééé"), "Péé1");
        assert_eq!(v1("péééé").len(), LEN_V1);
        assert_eq!(v2("péééééééé"), "Péééé1");
        assert_eq!(v2("péééééééé").len(), LEN_V2);
        // Four-byte mathematical-script letters: `𝓢` has no lowercase
        // mapping and is dropped; the Ll ones survive and straddle both cuts.
        assert_eq!(v1("𝓢𝓶𝓲𝓽𝓱"), "𝓶11");
        assert_eq!(v1("𝓢𝓶𝓲𝓽𝓱").len(), LEN_V1);
        assert_eq!(v2("𝓢𝓶𝓲𝓽𝓱"), "𝓶𝓲11");
        assert_eq!(v2("𝓢𝓶𝓲𝓽𝓱").len(), LEN_V2);
        // Where the boundary happens to fall cleanly, rphonetic accepts the
        // input and we match it exactly (see the previous test).
        assert_eq!(v2("péééé"), "Péééé1");
    }

    /// A four-byte character sitting *across* one cut but cleanly *inside*
    /// the other: the truncation backoff must step over several continuation
    /// bytes, and each version must diverge (drop + re-pad) exactly where
    /// rphonetic panics while matching it byte-for-byte where it does not.
    #[test]
    fn four_byte_char_backoff_at_each_cut() {
        // "stp" compacts to "STP" (3 bytes); 𝓶 (U+1D4F6, 4 bytes) then spans
        // bytes 3..7. v1's cut at 6 lands on 𝓶's last byte — rphonetic
        // panics; we back off three continuation bytes and re-pad.
        assert_eq!(v1("stp𝓶"), "STP111");
        assert_eq!(v1("stp𝓶").len(), LEN_V1);
        // v2's cut at 10 is beyond the 7-byte result: no divergence, and the
        // recorded rphonetic 3.0.6 output matches exactly.
        assert_eq!(v2("stp𝓶"), "STP𝓶111");
        // "stpkfmn" compacts to "STPKFMN" (7 bytes): now v1's cut at byte 6
        // falls between N and 𝓶 — clean, recorded from rphonetic — while
        // v2's cut at 10 splits 𝓶 and diverges.
        assert_eq!(v1("stpkfmn𝓶"), "STPKFM"); // recorded from rphonetic 3.0.6
        assert_eq!(v2("stpkfmn𝓶"), "STPKFMN111");
        assert_eq!(v2("stpkfmn𝓶").len(), LEN_V2);
    }

    /// Mid-word `wy`/`why` clusters: kept as `W` in v1 (its `wy`→`Wy`,
    /// `why`→`Why` rules), while v2 reaches the same letters through its
    /// y-as-vowel path. Recorded from rphonetic 3.0.6.
    #[test]
    fn wy_and_why_clusters_mid_word() {
        assert_eq!(v1("wywywy"), "WWW111");
        assert_eq!(v2("wywywy"), "WWWA111111");
        assert_eq!(v1("whywhy"), "WW1111");
        assert_eq!(v2("whywhy"), "WWA1111111");
        // j → y feeds those rules only at the very end of v1's cascade.
        assert_eq!(v1("jyjy"), "111111");
        assert_eq!(v2("jyjy"), "AA11111111");
        assert_eq!(v1("ymb"), "M11111");
        assert_eq!(v2("ymb"), "AM11111111");
    }

    #[test]
    fn astral_and_uncased_chars_are_dropped_without_panic() {
        assert_eq!(v1("Smith😀"), v1("Smith"));
        assert_eq!(v2("日本Smith語"), v2("Smith"));
        assert_eq!(v1("Smith"), "SMT111");
        assert_eq!(v2("Smith"), "SMT1111111");
    }

    #[test]
    fn compare_is_code_equality() {
        let c1 = Caverphone1::new();
        let c2 = Caverphone2::new();
        assert!(c1.compare("", "12345"));
        assert!(c2.compare("rather", "writer"));
        assert!(!c1.compare("Thompson", "Peady"));
        // v1 collides where v2 distinguishes (the point of the revision):
        // v1 deletes the trailing vowel of `ready`, v2 keeps it as `A`.
        assert!(c1.compare("ready", "rat"));
        assert!(!c2.compare("ready", "rat"));
    }
}
