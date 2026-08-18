//! NYSIIS — the New York State Identification and Intelligence System code.
//!
//! Devised by Robert L. Taft for the New York State Identification and
//! Intelligence System ("Name Search Technique", special report, 1970) as a
//! higher-accuracy replacement for SoundEx in matching surnames. It reached
//! Apache commons-codec as `org.apache.commons.codec.language.Nysiis`, and
//! [rphonetic](https://crates.io/crates/rphonetic) ports that class to Rust.
//!
//! # Provenance and pinning
//!
//! This module is a **Verbora-native extension**: the JS reference the rest of
//! this crate ports has no NYSIIS, so behavior is pinned to **rphonetic 3.0.6**
//! (`src/nysiis.rs`), the commons-codec lineage implementation this encoder is
//! benchmarked against. Output is byte-identical to rphonetic's on rphonetic's
//! full accepted input domain, with a single documented divergence (below).
//!
//! # The algorithm, exactly as rphonetic runs it
//!
//! 1. **Clean**: keep only Unicode-alphabetic characters (`char::is_alphabetic`)
//!    and uppercase them with the full Unicode mapping — so `ß` becomes `SS`,
//!    accents and CJK letters survive, and digits, punctuation, whitespace and
//!    emoji are dropped. An input that cleans to nothing encodes to `""`.
//! 2. **Prefix rewrites**, applied in order to the *evolving* string:
//!    `MAC`→`MCC`, `KN`→`NN`, `K`→`C`, `PH`/`PF`→`FF`, `SCH`→`SSS`.
//! 3. **Suffix rewrites**: `EE`/`IE`→`Y`, then `DT`/`RT`/`RD`/`NT`/`ND`→`D`.
//! 4. **First character retained verbatim** — it is never transcoded, which is
//!    why `"AEIOU"` encodes to `"A"` and `"Um"` to `"UN"`.
//! 5. **Rolling transcode** from the second character on, *writing its output
//!    back into the character buffer* so multi-character outputs overwrite the
//!    following input characters (rphonetic does exactly this, and it is
//!    observable — e.g. `PH`'s second output `F` becomes the next iteration's
//!    current character):
//!    `EV`→`AF`; vowels (`AEIOU` only — accented vowels are *not* vowels
//!    here, matching rphonetic)→`A`; `Q`→`G`; `Z`→`S`; `M`→`N`; `KN`→`NN`;
//!    `K`→`C`; `SCH`→`SSS`; `PH`→`FF`; `H` becomes the previous character
//!    unless both its neighbours are vowels; `W` after a vowel becomes that
//!    vowel (i.e. `A`, since vowels have just been transcoded).
//!    A transcoded character equal to its (transcoded) predecessor is not
//!    appended to the key.
//! 6. **Tail trims**, only when the key is longer than one byte: drop a
//!    trailing `S`; rewrite trailing `AY` to `Y` (only when longer than two
//!    bytes); drop a trailing `A`. These can cascade to the empty string:
//!    `"AZ"` encodes to `""`.
//! 7. **Strict truncation**: when [`Nysiis::is_strict`] (the default, as in
//!    commons-codec), the code is cut to at most 6 bytes.
//!
//! The trim gates in step 6 measure *bytes*, exactly like rphonetic's `String`
//! operations. This is provably equivalent to counting characters: every
//! trimmed pattern is ASCII, and any key ending in `AY` with a multi-byte
//! character elsewhere is already longer than two by both measures.
//!
//! # Divergence from rphonetic (excluded from the benchmark domain)
//!
//! rphonetic's strict truncation is `result[..min(len, 6)].to_string()` — a raw
//! byte slice. When the code is longer than 6 bytes **and** byte offset 6 falls
//! inside a multi-byte character, that slice **panics** in rphonetic. Example:
//! `"BCDFGÉX"` cleans and transcodes to the code `"BCDFGÉX"`, whose `É` spans
//! bytes 5..7, so `result[..6]` is not a character boundary. This
//! implementation never panics: it backs the cut off to the last character
//! boundary at or before 6, so `Nysiis::new().process("BCDFGÉX")` is
//! `"BCDFG"`. Affected inputs are exactly: strict mode, code longer than
//! 6 bytes, and `!code.is_char_boundary(6)` — reachable only with non-ASCII
//! letters. Whenever byte 6 *is* a boundary (all ASCII input; `"日本語"` →
//! `"日本"`), output is byte-identical to rphonetic's.
//!
//! # Performance
//!
//! ASCII input (the entire classical domain of the algorithm) runs a single
//! forward byte scan over one buffer that is mutated in place — prefix/suffix
//! rewrites, the transcode write-back, key compaction and trims all happen in
//! that buffer, which is then handed to the returned `String` without copying:
//! exactly one allocation per call. Non-ASCII input takes a `char`-level slow
//! path that mirrors rphonetic literally.

/// Maximum code length in strict mode, as in commons-codec (`TRUE_LENGTH`).
const MAX_STRICT_LEN: usize = 6;

/// The NYSIIS phonetic encoder, pinned to rphonetic 3.0.6.
///
/// The `Default` (and [`Nysiis::new`]) configuration is **strict** — codes are
/// truncated to 6 characters — matching commons-codec's and rphonetic's
/// defaults. [`Nysiis::with_strict`] configures it explicitly:
/// `Nysiis::with_strict(false)` corresponds to rphonetic's
/// `Nysiis::new(false)`.
///
/// ```
/// use verbora_phonetics::nysiis::Nysiis;
///
/// let strict = Nysiis::new();
/// assert_eq!(strict.process("Westerlund"), "WASTAR");
///
/// let full = Nysiis::with_strict(false);
/// assert_eq!(full.process("Westerlund"), "WASTARLAD");
/// ```
// `Default` is implemented by hand because the default `strict` is `true`,
// which `#[derive(Default)]` cannot express.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Nysiis {
    strict: bool,
}

impl Default for Nysiis {
    /// The strict encoder, like commons-codec's zero-argument constructor.
    ///
    /// ```
    /// use verbora_phonetics::nysiis::Nysiis;
    ///
    /// assert_eq!(Nysiis::default(), Nysiis::new());
    /// assert!(Nysiis::default().is_strict());
    /// ```
    fn default() -> Self {
        Self::new()
    }
}

/// The vowel class every NYSIIS rule uses: `AEIOU`, ASCII only.
///
/// rphonetic lowercases and checks `aeiou`; the buffer here holds uppercase, so
/// this is the same predicate. Accented vowels are deliberately *not* vowels.
#[inline]
const fn is_ascii_vowel(b: u8) -> bool {
    matches!(b, b'A' | b'E' | b'I' | b'O' | b'U')
}

/// `is_ascii_vowel` for the non-ASCII slow path, with rphonetic's exact
/// `to_ascii_lowercase` framing.
#[inline]
fn is_vowel_char(c: char) -> bool {
    matches!(c.to_ascii_lowercase(), 'a' | 'e' | 'i' | 'o' | 'u')
}

impl Nysiis {
    /// Creates a strict NYSIIS encoder (codes truncated to 6 characters), the
    /// commons-codec default.
    ///
    /// ```
    /// use verbora_phonetics::nysiis::Nysiis;
    ///
    /// assert_eq!(Nysiis::new().process("Brian"), "BRAN");
    /// ```
    #[must_use]
    pub const fn new() -> Self {
        Self { strict: true }
    }

    /// Creates an encoder with explicit strictness.
    ///
    /// `strict == true` truncates codes to 6 characters; `false` leaves them
    /// full length. Mirrors rphonetic's `Nysiis::new(strict)`.
    ///
    /// ```
    /// use verbora_phonetics::nysiis::Nysiis;
    ///
    /// assert_eq!(Nysiis::with_strict(true).process("Phillipson"), "FALAPS");
    /// assert_eq!(Nysiis::with_strict(false).process("Phillipson"), "FALAPSAN");
    /// ```
    #[must_use]
    pub const fn with_strict(strict: bool) -> Self {
        Self { strict }
    }

    /// Whether codes are truncated to 6 characters.
    ///
    /// ```
    /// use verbora_phonetics::nysiis::Nysiis;
    ///
    /// assert!(Nysiis::new().is_strict());
    /// assert!(!Nysiis::with_strict(false).is_strict());
    /// ```
    #[must_use]
    pub const fn is_strict(&self) -> bool {
        self.strict
    }

    /// Encodes `token` to its NYSIIS code.
    ///
    /// Non-letters are dropped, letters are uppercased, and an input with no
    /// letters at all encodes to the empty string — exactly rphonetic's
    /// handling. Never panics, including on non-ASCII input (see the module
    /// documentation for the one divergence that buys).
    ///
    /// ```
    /// use verbora_phonetics::nysiis::Nysiis;
    ///
    /// let nysiis = Nysiis::new();
    /// assert_eq!(nysiis.process("MACINTOSH"), "MCANT");
    /// assert_eq!(nysiis.process("KNUTH"), "NAT");
    /// assert_eq!(nysiis.process("o'daniel"), "ODANAL");
    /// assert_eq!(nysiis.process("12345"), "");
    /// ```
    #[must_use]
    pub fn process(&self, token: &str) -> String {
        if token.is_ascii() {
            self.process_ascii(token.as_bytes())
        } else {
            self.process_unicode(token)
        }
    }

    /// Whether two strings share a NYSIIS code (at this encoder's strictness).
    ///
    /// ```
    /// use verbora_phonetics::nysiis::Nysiis;
    ///
    /// let nysiis = Nysiis::new();
    /// assert!(nysiis.compare("Smith", "Schmit"));
    /// assert!(nysiis.compare("Trueman", "Truman"));
    /// assert!(!nysiis.compare("Smith", "Jones"));
    /// ```
    #[must_use]
    pub fn compare(&self, a: &str, b: &str) -> bool {
        self.process(a) == self.process(b)
    }

    /// The fast path: everything in one mutable byte buffer.
    ///
    /// The key is compacted into the front of the same buffer the transcode
    /// loop reads. That is sound because the key write position `k` never
    /// exceeds the loop index `i` (both start at 1 and `k` only advances when
    /// `i` does), so a key write at `k < i` lands strictly below every cell the
    /// loop still reads (`i - 1` and up), and a write at `k == i` stores the
    /// value already there.
    fn process_ascii(&self, bytes: &[u8]) -> String {
        let mut buf: Vec<u8> = Vec::with_capacity(bytes.len());
        for &b in bytes {
            if b.is_ascii_alphabetic() {
                buf.push(b.to_ascii_uppercase());
            }
        }
        if buf.is_empty() {
            return String::new();
        }

        // Prefix rewrites, in rphonetic's order, each testing the buffer as
        // already mutated by the previous one.
        if buf.starts_with(b"MAC") {
            buf[1] = b'C'; // MAC -> MCC
        }
        if buf.starts_with(b"KN") {
            buf[0] = b'N'; // KN -> NN
        }
        if buf.first() == Some(&b'K') {
            buf[0] = b'C'; // K -> C
        }
        if buf.starts_with(b"PH") || buf.starts_with(b"PF") {
            buf[0] = b'F'; // PH/PF -> FF
            buf[1] = b'F';
        }
        if buf.starts_with(b"SCH") {
            buf[1] = b'S'; // SCH -> SSS
            buf[2] = b'S';
        }

        // Suffix rewrites: EE/IE -> Y, then DT/RT/RD/NT/ND -> D. After the
        // first fires the string ends in Y, so the second never also fires —
        // but they are tested sequentially, like rphonetic's two `if`s.
        let n = buf.len();
        if n >= 2 && matches!([buf[n - 2], buf[n - 1]], [b'E' | b'I', b'E']) {
            buf[n - 2] = b'Y';
            buf.truncate(n - 1);
        }
        let n = buf.len();
        if n >= 2
            && matches!(
                [buf[n - 2], buf[n - 1]],
                [b'D' | b'R', b'T'] | [b'R' | b'N', b'D'] | [b'N', b'T']
            )
        {
            buf[n - 2] = b'D';
            buf.truncate(n - 1);
        }

        // Rolling transcode with write-back, compacting the key in place.
        // buf[0] is the retained first character and is never transcoded.
        let len = buf.len();
        let mut k = 1;
        for i in 1..len {
            transcode_ascii(&mut buf, i);
            if buf[i - 1] != buf[i] {
                buf[k] = buf[i];
                k += 1;
            }
        }
        buf.truncate(k);

        // Tail trims, gated on a key longer than one byte (rphonetic returns a
        // one-character key untouched, even if it is "A" or "S").
        if buf.len() > 1 {
            if buf.last() == Some(&b'S') {
                buf.pop();
            }
            let n = buf.len();
            if n > 2 && buf[n - 2] == b'A' && buf[n - 1] == b'Y' {
                buf.remove(n - 2);
            }
            if buf.last() == Some(&b'A') {
                buf.pop();
            }
        }

        if self.strict {
            buf.truncate(MAX_STRICT_LEN);
        }
        String::from_utf8(buf).expect("buffer holds only ASCII uppercase letters")
    }

    /// The non-ASCII slow path: a literal mirror of rphonetic over
    /// `Vec<char>`, with the key accumulated in a `String` so the byte-measured
    /// trim gates read exactly like rphonetic's.
    fn process_unicode(&self, token: &str) -> String {
        // rphonetic's `soundex_clean`: Unicode-alphabetic filter, then the full
        // (possibly multi-character) uppercase mapping.
        let mut chars: Vec<char> = token
            .chars()
            .filter(|c| c.is_alphabetic())
            .flat_map(char::to_uppercase)
            .collect();
        if chars.is_empty() {
            return String::new();
        }

        if chars.starts_with(&['M', 'A', 'C']) {
            chars[1] = 'C';
        }
        if chars.starts_with(&['K', 'N']) {
            chars[0] = 'N';
        }
        if chars.first() == Some(&'K') {
            chars[0] = 'C';
        }
        if chars.starts_with(&['P', 'H']) || chars.starts_with(&['P', 'F']) {
            chars[0] = 'F';
            chars[1] = 'F';
        }
        if chars.starts_with(&['S', 'C', 'H']) {
            chars[1] = 'S';
            chars[2] = 'S';
        }

        let n = chars.len();
        if n >= 2 && matches!([chars[n - 2], chars[n - 1]], ['E' | 'I', 'E']) {
            chars[n - 2] = 'Y';
            chars.truncate(n - 1);
        }
        let n = chars.len();
        if n >= 2
            && matches!(
                [chars[n - 2], chars[n - 1]],
                ['D' | 'R', 'T'] | ['R' | 'N', 'D'] | ['N', 'T']
            )
        {
            chars[n - 2] = 'D';
            chars.truncate(n - 1);
        }

        let len = chars.len();
        let mut key = String::with_capacity(token.len());
        key.push(chars[0]);
        for i in 1..len {
            transcode_char(&mut chars, i);
            if chars[i - 1] != chars[i] {
                key.push(chars[i]);
            }
        }

        // Byte-measured gates, exactly like rphonetic's String operations.
        if key.len() > 1 {
            if key.ends_with('S') {
                key.pop();
            }
            if key.len() > 2 && key.ends_with("AY") {
                key.remove(key.len() - 2);
            }
            if key.ends_with('A') {
                key.pop();
            }
        }

        if self.strict && key.len() > MAX_STRICT_LEN {
            // rphonetic slices at byte 6 and panics off a boundary; we back off
            // to the previous character boundary instead (see module docs).
            let mut cut = MAX_STRICT_LEN;
            while !key.is_char_boundary(cut) {
                cut -= 1;
            }
            key.truncate(cut);
        }
        key
    }
}

/// One step of the rolling transcode, writing its output back into `buf` at
/// `i..` exactly as rphonetic writes `transcode`'s output into its `chars`
/// vector. Multi-character outputs never overrun: each one is guarded by the
/// existence of the lookahead characters it overwrites.
fn transcode_ascii(buf: &mut [u8], i: usize) {
    let prev = buf[i - 1];
    let cur = buf[i];
    let next = buf.get(i + 1).copied();
    let next2 = buf.get(i + 2).copied();

    if cur == b'E' && next == Some(b'V') {
        buf[i] = b'A'; // EV -> AF
        buf[i + 1] = b'F';
        return;
    }
    if is_ascii_vowel(cur) {
        buf[i] = b'A';
        return;
    }
    match (cur, next) {
        (b'Q', _) => {
            buf[i] = b'G';
            return;
        }
        (b'Z', _) => {
            buf[i] = b'S';
            return;
        }
        (b'M', _) => {
            buf[i] = b'N';
            return;
        }
        (b'K', Some(b'N')) => {
            buf[i] = b'N'; // KN -> NN
            buf[i + 1] = b'N';
            return;
        }
        (b'K', _) => {
            buf[i] = b'C';
            return;
        }
        _ => {}
    }
    if cur == b'S' && next == Some(b'C') && next2 == Some(b'H') {
        buf[i + 1] = b'S'; // SCH -> SSS (buf[i] is already S)
        buf[i + 2] = b'S';
        return;
    }
    if cur == b'P' && next == Some(b'H') {
        buf[i] = b'F'; // PH -> FF
        buf[i + 1] = b'F';
        return;
    }
    // H stays H only between two vowels; W after a vowel echoes that vowel.
    if (cur == b'H' && (!is_ascii_vowel(prev) || !next.is_some_and(is_ascii_vowel)))
        || (cur == b'W' && is_ascii_vowel(prev))
    {
        buf[i] = prev;
    }
}

/// [`transcode_ascii`] for the `char`-level slow path.
fn transcode_char(chars: &mut [char], i: usize) {
    let prev = chars[i - 1];
    let cur = chars[i];
    let next = chars.get(i + 1).copied();
    let next2 = chars.get(i + 2).copied();

    if cur == 'E' && next == Some('V') {
        chars[i] = 'A';
        chars[i + 1] = 'F';
        return;
    }
    if is_vowel_char(cur) {
        chars[i] = 'A';
        return;
    }
    match (cur, next) {
        ('Q', _) => {
            chars[i] = 'G';
            return;
        }
        ('Z', _) => {
            chars[i] = 'S';
            return;
        }
        ('M', _) => {
            chars[i] = 'N';
            return;
        }
        ('K', Some('N')) => {
            chars[i] = 'N';
            chars[i + 1] = 'N';
            return;
        }
        ('K', _) => {
            chars[i] = 'C';
            return;
        }
        _ => {}
    }
    if cur == 'S' && next == Some('C') && next2 == Some('H') {
        chars[i + 1] = 'S';
        chars[i + 2] = 'S';
        return;
    }
    if cur == 'P' && next == Some('H') {
        chars[i] = 'F';
        chars[i + 1] = 'F';
        return;
    }
    if (cur == 'H' && (!is_vowel_char(prev) || !next.is_some_and(is_vowel_char)))
        || (cur == 'W' && is_vowel_char(prev))
    {
        chars[i] = prev;
    }
}

impl verbora_core::Phonetic for Nysiis {
    fn process(&self, token: &str) -> String {
        Self::process(self, token)
    }

    fn compare(&self, a: &str, b: &str) -> bool {
        Self::compare(self, a, b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Asserts every value encodes to `expected` under the strict default,
    /// mirroring rphonetic's `encode_all` helper.
    fn strict_all(values: &[&str], expected: &str) {
        let nysiis = Nysiis::default();
        for v in values {
            assert_eq!(nysiis.process(v), expected, "strict encoding of {v:?}");
        }
    }

    /// Asserts each pair under the non-strict encoder, mirroring rphonetic's
    /// `encode` helper.
    fn full(pairs: &[(&str, &str)]) {
        let nysiis = Nysiis::with_strict(false);
        for (value, expected) in pairs {
            assert_eq!(
                nysiis.process(value),
                *expected,
                "non-strict encoding of {value:?}"
            );
        }
    }

    // ------------------------------------------------------------------
    // Fixtures ported verbatim from rphonetic 3.0.6 src/nysiis.rs tests
    // (themselves mirroring commons-codec's NysiisTest.java).
    // ------------------------------------------------------------------

    #[test]
    fn rphonetic_bran() {
        strict_all(&["Brian", "Brown", "Brun"], "BRAN");
    }

    #[test]
    fn rphonetic_cap() {
        strict_all(&["Capp", "Cope", "Copp", "Kipp"], "CAP");
    }

    #[test]
    fn rphonetic_dad() {
        strict_all(&["Dent"], "DAD");
    }

    #[test]
    fn rphonetic_dan() {
        strict_all(&["Dane", "Dean", "Dionne"], "DAN");
    }

    #[test]
    fn rphonetic_fal() {
        strict_all(&["Phil"], "FAL");
    }

    #[test]
    fn rphonetic_drop_by() {
        full(&[
            ("MACINTOSH", "MCANT"),
            ("KNUTH", "NAT"),
            ("KOEHN", "CAN"),
            ("PHILLIPSON", "FALAPSAN"),
            ("PFEISTER", "FASTAR"),
            ("SCHOENHOEFT", "SANAFT"),
            ("MCKEE", "MCY"),
            ("MACKIE", "MCY"),
            ("HEITSCHMIDT", "HATSNAD"),
            ("BART", "BAD"),
            ("HURD", "HAD"),
            ("HUNT", "HAD"),
            ("WESTERLUND", "WASTARLAD"),
            ("CASSTEVENS", "CASTAFAN"),
            ("VASQUEZ", "VASG"),
            ("FRAZIER", "FRASAR"),
            ("BOWMAN", "BANAN"),
            ("MCKNIGHT", "MCNAGT"),
            ("RICKERT", "RACAD"),
            ("DEUTSCH", "DAT"),
            ("WESTPHAL", "WASTFAL"),
            ("SHRIVER", "SRAVAR"),
            ("KUHL", "CAL"),
            ("RAWSON", "RASAN"),
            ("JILES", "JAL"),
            ("CARRAWAY", "CARY"),
            ("YAMADA", "YANAD"),
        ]);
    }

    #[test]
    fn rphonetic_others() {
        full(&[
            ("O'Daniel", "ODANAL"),
            ("O'Donnel", "ODANAL"),
            ("Cory", "CARY"),
            ("Corey", "CARY"),
            ("Kory", "CARY"),
            ("FUZZY", "FASY"),
        ]);
    }

    #[test]
    fn rphonetic_rule1() {
        full(&[
            ("MACX", "MCX"),
            ("KNX", "NX"),
            ("KX", "CX"),
            ("PHX", "FX"),
            ("PFX", "FX"),
            ("SCHX", "SX"),
        ]);
    }

    #[test]
    fn rphonetic_rule2() {
        full(&[
            ("XEE", "XY"),
            ("XIE", "XY"),
            ("XDT", "XD"),
            ("XRT", "XD"),
            ("XRD", "XD"),
            ("XNT", "XD"),
            ("XND", "XD"),
        ]);
    }

    #[test]
    fn rphonetic_rule4_dot1() {
        full(&[
            ("XEV", "XAF"),
            ("XAX", "XAX"),
            ("XEX", "XAX"),
            ("XIX", "XAX"),
            ("XOX", "XAX"),
            ("XUX", "XAX"),
        ]);
    }

    #[test]
    fn rphonetic_rule4_dot2() {
        full(&[("XQ", "XG"), ("XZ", "X"), ("XM", "XN")]);
    }

    #[test]
    fn rphonetic_rule5() {
        full(&[("XS", "X"), ("XSS", "X")]);
    }

    #[test]
    fn rphonetic_rule6() {
        full(&[("XAY", "XY"), ("XAYS", "XY")]);
    }

    #[test]
    fn rphonetic_rule7() {
        full(&[("XA", "X"), ("XAS", "X")]);
    }

    #[test]
    fn rphonetic_snad() {
        strict_all(&["Schmidt"], "SNAD");
    }

    #[test]
    fn rphonetic_snat() {
        strict_all(&["Smith", "Schmit"], "SNAT");
    }

    #[test]
    fn rphonetic_special_branches() {
        strict_all(&["Kobwick"], "CABWAC");
        strict_all(&["Kocher"], "CACAR");
        strict_all(&["Fesca"], "FASC");
        strict_all(&["Shom"], "SAN");
        strict_all(&["Ohlo"], "OL");
        strict_all(&["Uhu"], "UH");
        strict_all(&["Um"], "UN");
    }

    #[test]
    fn rphonetic_tranan() {
        strict_all(&["Trueman", "Truman"], "TRANAN");
    }

    #[test]
    fn rphonetic_true_variant() {
        let nysiis = Nysiis::default();
        let result = nysiis.process("WESTERLUND");
        assert!(result.len() <= 6);
        assert_eq!(result, "WASTAR");
    }

    // ------------------------------------------------------------------
    // Hand-written edge cases.
    // ------------------------------------------------------------------

    #[test]
    fn empty_and_letterless_inputs_encode_to_empty() {
        for nysiis in [Nysiis::new(), Nysiis::with_strict(false)] {
            for input in ["", "12345", "!!!", "   ", "'", "-_-", "😀", "😀🎉"] {
                assert_eq!(nysiis.process(input), "", "for {input:?}");
            }
        }
    }

    #[test]
    fn single_letters() {
        let nysiis = Nysiis::new();
        // The first character is retained verbatim, so no rolling rule ever
        // applies to it — but the K -> C *prefix* rule does.
        assert_eq!(nysiis.process("a"), "A");
        assert_eq!(nysiis.process("K"), "C");
        assert_eq!(nysiis.process("M"), "M");
        assert_eq!(nysiis.process("Z"), "Z");
        // One-byte keys skip the tail trims entirely.
        assert_eq!(nysiis.process("S"), "S");
        assert_eq!(nysiis.process("A"), "A");
    }

    #[test]
    fn prefixes_alone() {
        full(&[
            ("PH", "F"),   // PH -> FF, then the two Fs collapse
            ("KN", "N"),   // KN -> NN, collapse
            ("SCH", "S"),  // SCH -> SSS, collapse
            ("MAC", "MC"), // MAC -> MCC, collapse
            ("PF", "F"),
        ]);
    }

    #[test]
    fn tail_trims_can_empty_the_key() {
        // Key "AS": the S trim then the A trim leave nothing.
        full(&[("AZ", ""), ("AAS", "")]);
        assert_eq!(Nysiis::new().process("AZ"), "");
        // But a key that *collapses* to one byte skips the trims: "AA" dedups
        // to the key "A" before the gate is consulted.
        full(&[("AA", "A"), ("AHA", "AH")]);
    }

    #[test]
    fn first_char_is_never_transcoded() {
        full(&[
            ("AEIOU", "A"), // vowels collapse into the retained A
            ("EVE", "EV"),  // EV rule does not fire at position 0
            ("EEVE", "EAF"),
            ("QQ", "QG"), // Q -> G fires only at position 1; no dedup since Q != G
        ]);
    }

    #[test]
    fn ev_and_write_back_shapes() {
        full(&[
            ("XEV", "XAF"),
            ("XEVS", "XAF"),
            // PH's written-back second F becomes the next current character
            // and collapses: PPHH -> P + F(F) + dups -> "PF".
            ("PPHH", "PF"),
            ("XSCHX", "XSX"),
            ("XKN", "XN"),
            ("SCHSCH", "S"),
            ("KNKN", "N"),
            ("MACMAC", "MCNAC"),
        ]);
    }

    #[test]
    fn h_and_w_context_rules() {
        full(&[
            ("PHISH", "F"),    // FF + A + S + H->S, then S and A trims
            ("AHAB", "AHAB"),  // H between two vowels survives
            ("ABHAB", "ABAB"), // H after consonant echoes it, then dedups
            ("AWA", "A"),      // W after vowel echoes it; everything dedups away
            ("XWX", "XWX"),    // W after consonant stays W
            ("KOBWICK", "CABWAC"),
        ]);
    }

    #[test]
    fn digits_and_separators_are_dropped_before_everything() {
        let nysiis = Nysiis::new();
        assert_eq!(nysiis.process("K1N2"), nysiis.process("KN"));
        assert_eq!(nysiis.process("mac intosh"), "MCANT");
        assert_eq!(nysiis.process("Mac-Intosh"), "MCANT");
        assert_eq!(nysiis.process("K9N"), "N");
    }

    #[test]
    fn mixed_case_is_normalized() {
        let nysiis = Nysiis::new();
        assert_eq!(nysiis.process("wEsTeRlUnD"), "WASTAR");
        assert_eq!(nysiis.process("MacIntosh"), nysiis.process("MACINTOSH"));
        assert_eq!(nysiis.process("knuth"), "NAT");
    }

    #[test]
    fn non_ascii_letters_pass_through_untranscoded() {
        let nysiis = Nysiis::new();
        // Accented letters are alphabetic (kept, uppercased) but are neither
        // vowels nor any consonant class, so they ride along verbatim.
        assert_eq!(nysiis.process("café"), "CAFÉ");
        assert_eq!(nysiis.process("Müller"), "MÜLAR");
        assert_eq!(nysiis.process("É"), "É");
        assert_eq!(nysiis.process("ÉÉÉ"), "É");
        // ß uppercases to SS, which collapses to a single-character key.
        assert_eq!(nysiis.process("ß"), "S");
        assert_eq!(nysiis.process("Straße"), "STRAS");
        // Letters mixed with emoji: the emoji is dropped, letters remain.
        assert_eq!(nysiis.process("a😀b"), "AB");
    }

    #[test]
    fn cjk_and_strict_truncation_on_a_boundary() {
        // Each CJK char is 3 bytes; the 6-byte strict cut lands on a character
        // boundary, so this is byte-identical to rphonetic.
        assert_eq!(Nysiis::new().process("日本語"), "日本");
        assert_eq!(Nysiis::with_strict(false).process("日本語"), "日本語");
        // 2-byte chars alternating with ASCII: the cut at byte 6 is again a
        // boundary ("ÉXÉX" is 6 bytes), identical to rphonetic.
        assert_eq!(Nysiis::new().process("ÉXÉXÉX"), "ÉXÉX");
    }

    #[test]
    fn documented_divergence_no_panic_when_byte_6_splits_a_char() {
        // rphonetic panics here: the code "BCDFGÉX" is 8 bytes and É spans
        // bytes 5..7, so its `result[..6]` slice is off a char boundary. We
        // back off to the boundary at byte 5 instead. See module docs.
        assert_eq!(Nysiis::new().process("BCDFGÉX"), "BCDFG");
        // Non-strict mode never truncates, so it never diverges.
        assert_eq!(Nysiis::with_strict(false).process("BCDFGÉX"), "BCDFGÉX");
    }

    #[test]
    fn very_long_inputs() {
        let nysiis = Nysiis::with_strict(false);
        assert_eq!(nysiis.process(&"a".repeat(500)), "A");
        let long = "ab".repeat(250);
        let code = nysiis.process(&long);
        assert_eq!(code.len(), 500);
        assert!(code.starts_with("ABAB"));
        assert_eq!(Nysiis::new().process(&long), "ABABAB");
    }

    #[test]
    fn strict_is_a_pure_truncation_of_non_strict() {
        let strict = Nysiis::new();
        let full = Nysiis::with_strict(false);
        for input in [
            "MACINTOSH",
            "WESTERLUND",
            "PHILLIPSON",
            "SCHOENHOEFT",
            "CASSTEVENS",
            "supercalifragilisticexpialidocious",
        ] {
            let code = full.process(input);
            let want = &code[..code.len().min(6)];
            assert_eq!(strict.process(input), want, "for {input:?}");
        }
    }

    #[test]
    fn ascii_fast_path_matches_unicode_slow_path() {
        // Every ASCII input must encode identically through both paths.
        for nysiis in [Nysiis::new(), Nysiis::with_strict(false)] {
            for input in [
                "",
                "A",
                "K",
                "XS",
                "XAYS",
                "MACINTOSH",
                "KNUTH",
                "PHILLIPSON",
                "SCHOENHOEFT",
                "HEITSCHMIDT",
                "CASSTEVENS",
                "CARRAWAY",
                "O'Daniel",
                "PPHH",
                "SCHSCH",
                "AZ",
                "Uhu",
                "Um",
                "FUZZY",
                "mac intosh",
            ] {
                assert_eq!(
                    nysiis.process_ascii(input.as_bytes()),
                    nysiis.process_unicode(input),
                    "paths disagree on {input:?} (strict={})",
                    nysiis.is_strict()
                );
            }
        }
    }

    /// The ASCII fast path compacts the key into the same buffer the
    /// transcode loop reads (`k <= i` invariant). Exhaustively comparing it
    /// against the literal char-level mirror over short dense strings pins
    /// that in-place write-back can never corrupt a read the loop still
    /// depends on — for every string, not just picked fixtures.
    #[test]
    fn in_place_write_back_matches_char_mirror_exhaustively() {
        // Letters chosen to hit every transcode branch: EV, vowels, Q/Z/M,
        // KN/K, SCH, PH, H (vowel and consonant neighbours), W-after-vowel.
        const HOT: &[u8] = b"AEVHWKNSCPFZ";
        let strict = Nysiis::new();
        let full = Nysiis::with_strict(false);
        let mut buf = [0u8; 4];
        for len in 0..=4usize {
            let mut idx = [0usize; 4];
            loop {
                for (slot, &i) in buf[..len].iter_mut().zip(idx.iter()) {
                    *slot = HOT[i];
                }
                let s = std::str::from_utf8(&buf[..len]).unwrap();
                for nysiis in [strict, full] {
                    assert_eq!(
                        nysiis.process_ascii(s.as_bytes()),
                        nysiis.process_unicode(s),
                        "paths disagree on {s:?} (strict={})",
                        nysiis.is_strict()
                    );
                }
                // Odometer increment.
                let mut pos = 0;
                loop {
                    if pos == len {
                        break;
                    }
                    idx[pos] += 1;
                    if idx[pos] < HOT.len() {
                        break;
                    }
                    idx[pos] = 0;
                    pos += 1;
                }
                if pos == len {
                    break;
                }
            }
        }
    }

    /// Prefix/EV/AY chains recorded from rphonetic 3.0.6 (`Nysiis::encode`),
    /// beyond the shapes the ported fixtures cover: repeated prefixes whose
    /// rewritten output immediately re-feeds the rolling transcode, and the
    /// AY trim interacting with the S trim.
    #[test]
    fn recorded_prefix_and_write_back_chains() {
        let cases: &[(&str, &str, &str)] = &[
            // (input, strict, non-strict) — recorded from rphonetic 3.0.6.
            ("KNKNKNKN", "N", "N"),
            ("SCHSCHSCHSCH", "S", "S"),
            ("PFPFPF", "FPFPF", "FPFPF"),
            ("EVEVEV", "EVAFAF", "EVAFAF"),
            ("HHHHH", "H", "H"),
            ("WAWAWA", "W", "W"),
            ("MACMACMAC", "MCNACN", "MCNACNAC"),
            ("KNIGHTSSCH", "NAGT", "NAGT"),
            ("AAYAAY", "AYY", "AYY"),
            ("SCHWARZKN", "SWARSN", "SWARSN"),
        ];
        for &(input, strict, full) in cases {
            assert_eq!(Nysiis::new().process(input), strict, "strict {input:?}");
            assert_eq!(
                Nysiis::with_strict(false).process(input),
                full,
                "non-strict {input:?}"
            );
        }
    }

    #[test]
    fn compare_matches_code_equality() {
        let nysiis = Nysiis::new();
        assert!(nysiis.compare("Brian", "Brown"));
        assert!(nysiis.compare("Capp", "Kipp"));
        assert!(nysiis.compare("Smith", "Schmit"));
        assert!(!nysiis.compare("Smith", "Jones"));
        assert!(!nysiis.compare("Brian", "Dent"));
        // Strictness changes comparison: these agree only on the 6-char prefix.
        let full = Nysiis::with_strict(false);
        assert_eq!(full.process("WESTERLUND"), "WASTARLAD");
        assert_eq!(full.process("WESTERLAND"), "WASTARLAD");
    }

    #[test]
    fn constructors_and_default() {
        assert_eq!(Nysiis::default(), Nysiis::new());
        assert_eq!(Nysiis::new(), Nysiis::with_strict(true));
        assert!(Nysiis::new().is_strict());
        assert!(!Nysiis::with_strict(false).is_strict());
    }

    #[test]
    fn phonetic_trait_delegates() {
        fn as_phonetic(p: &dyn verbora_core::Phonetic) -> (String, bool) {
            (p.process("KNUTH"), p.compare("Smith", "Schmit"))
        }
        let (code, eq) = as_phonetic(&Nysiis::new());
        assert_eq!(code, "NAT");
        assert!(eq);
    }
}
