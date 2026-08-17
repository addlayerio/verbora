//! Double Metaphone, as the reference implements it.
//!
//! Ports the reference `double_metaphone`: a single left-to-right
//! scan over the uppercased token that builds two encodings at once.
//!
//! # This is a transcription, not an implementation
//!
//! The reference contains a family of the reference-truthiness accidents that are
//! *observable*, so they are reproduced here rather than corrected. Each is
//! marked at its site; the catalogue is:
//!
//! | Site | What the source says | What it means |
//! |---|---|---|
//! | `handleC` | `token.substring(pos-2, pos+1) !== 'WICZ'` | a 3-character slice compared with a 4-character literal — **always true** |
//! | `handleC` | `['C','Q','G'].indexOf(token[pos+2])` used as a condition | `indexOf` returns `0` for `C`, which is **falsy** — the test is inverted |
//! | `handleC` | `pos === 1 && …` inside a branch guarded by `pos !== 1` | **unreachable** |
//! | `handleR` | `!subMatch(-4, -3, ['ME','MA'])` | a 1-character slice against 2-character literals — **always true** |
//! | `handleT` | `token.substring(1, 2) === 'TH'` | a 1-character slice against a 2-character literal — **always false** |
//! | `handleL` | `subMatch(-2, -1, ['AS','OS']) > -1` | a boolean compared with `-1` — **always true**, in an unreachable branch |
//! | `handleJ` | `add('J', 'H')` | `add` takes one argument, so **both** encodings get `J` |
//! | `handleG` | `token[pos+1] !== 'Y'` inside a branch requiring `token[pos+1] === 'N'` | **always true** |
//! | `handleH`, `handleW`, `handleZ` | a `pos++` in a branch that consumed nothing | the next character is **silently skipped** |
//!
//! "Fixing" any of them changes the encoding of real words — `markiewicz`,
//! `papier`, `lebowski`, `MAC GAT` — so each is transcribed as written and
//! pinned by a test.

use crate::units::{
    Unit, any_ascii_slice, clamp_take, coerce_or_default, eq_ascii_slice, utf16_to_uppercase,
};

/// Double Metaphone: a primary and an alternate encoding.
///
/// ```
/// use verbora_phonetics::DoubleMetaphone;
///
/// let dm = DoubleMetaphone::new();
/// assert_eq!(dm.process("Matrix"), ("MTRKS".into(), "MTRKS".into()));
/// assert_eq!(dm.process("astromech"), ("ATRMX".into(), "ATRMK".into()));
/// assert!(dm.compare("love", "luv"));
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DoubleMetaphone;

impl DoubleMetaphone {
    /// Creates a Double Metaphone encoder. It holds no state; the type exists to
    /// mirror the reference `DoubleMetaphone`.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Encodes `token` with the default maximum length of 32.
    #[must_use]
    pub fn process(&self, token: &str) -> (String, String) {
        self.process_with(token, None)
    }

    /// Encodes `token`, honouring the reference's `maxLength` coercion.
    ///
    /// Both encodings are always returned, and both are always strings — the
    /// reference returns a two-element array unconditionally.
    #[must_use]
    pub fn process_with(&self, token: &str, max_length: Option<f64>) -> (String, String) {
        let upper = utf16_to_uppercase(token);
        if upper.is_ascii() {
            encode(upper.as_bytes(), max_length)
        } else {
            let units: Vec<u16> = upper.encode_utf16().collect();
            encode(&units, max_length)
        }
    }

    /// Whether two strings share **either** encoding.
    ///
    /// This overrides the inherited `Phonetic#compare`, which would compare the
    /// two returned arrays by reference and therefore always be `false`.
    #[must_use]
    pub fn compare(&self, a: &str, b: &str) -> bool {
        let (pa, sa) = self.process(a);
        let (pb, sb) = self.process(b);
        pa == pb || sa == sb
    }

    /// `c && c.match(/[aeiouy]/i)` — note that **`y` counts as a vowel**.
    ///
    /// The reference uses `.match`, which searches anywhere in the string, so
    /// this reports `true` for any string *containing* a vowel; every internal
    /// call site passes a single code unit. Accented letters are **not** vowels
    /// here (`É` does not match), even though the main switch treats `É`, `Ê` and
    /// `À` as word-initial vowels.
    #[must_use]
    pub fn is_vowel(&self, s: &str) -> bool {
        s.bytes().any(|b| {
            matches!(
                b.to_ascii_uppercase(),
                b'A' | b'E' | b'I' | b'O' | b'U' | b'Y'
            )
        })
    }
}

/// `isVowel` over a single code unit, with the reference's falsy-on-`undefined`.
#[inline]
fn is_vowel<U: Unit>(u: Option<U>) -> bool {
    matches!(
        u.and_then(Unit::to_ascii).map(|b| b.to_ascii_uppercase()),
        Some(b'A' | b'E' | b'I' | b'O' | b'U' | b'Y')
    )
}

/// The one-pass encoder. `pos` and the two accumulators are locals of
/// `process()` in the reference; nothing survives the call.
struct Encoder<'a, U: Unit> {
    token: &'a [U],
    primary: String,
    secondary: String,
    pos: usize,
    /// `token.substring(0, 3) === 'SAN'`
    san: bool,
    /// `isVowel(token[0])`
    starts_with_vowel: bool,
    /// `token.match(/(W|K|CZ|WITZ)/)` — truthy when `W`, `K` or `CZ` occurs
    /// **anywhere**. `WITZ` is subsumed by `W`.
    slavo_germanic: bool,
}

fn encode<U: Unit>(token: &[U], max_length: Option<f64>) -> (String, String) {
    let max = coerce_or_default(max_length, 32.0);
    let mut e = Encoder {
        token,
        primary: String::with_capacity(16),
        secondary: String::with_capacity(16),
        pos: 0,
        san: eq_ascii_slice(&token[..token.len().min(3)], b"SAN"),
        starts_with_vowel: is_vowel(token.first().copied()),
        slavo_germanic: token.iter().enumerate().any(|(i, u)| {
            u.eq_ascii(b'W')
                || u.eq_ascii(b'K')
                || (u.eq_ascii(b'C') && token.get(i + 1).is_some_and(|n| n.eq_ascii(b'Z')))
        }),
    };

    if any_ascii_slice(
        &token[..token.len().min(2)],
        &[b"GN", b"KN", b"PN", b"WR", b"PS"],
    ) {
        e.pos += 1;
    }

    while e.pos < e.token.len() {
        e.step();
        // The early break is a pure optimisation — the encodings are
        // append-only, so it cannot change the truncated result — but it is
        // transcribed because `maxLength = -1` makes it fire immediately.
        if e.primary.len() as f64 >= max && e.secondary.len() as f64 >= max {
            break;
        }
        e.pos += 1;
    }

    (truncate(e.primary, max), truncate(e.secondary, max))
}

/// `truncate(string, length)`
fn truncate(mut s: String, max: f64) -> String {
    if s.len() as f64 >= max {
        s.truncate(clamp_take(max, s.len()));
    }
    s
}

impl<U: Unit> Encoder<'_, U> {
    // -- the reference string primitives ---------------------------------------

    /// `token[pos + offset]`, with `undefined` for out-of-range indices.
    #[inline]
    fn at(&self, offset: isize) -> Option<U> {
        self.at_abs(self.pos as isize + offset)
    }

    /// `token[index]`, with `undefined` for out-of-range indices.
    #[inline]
    fn at_abs(&self, index: isize) -> Option<U> {
        usize::try_from(index)
            .ok()
            .and_then(|i| self.token.get(i))
            .copied()
    }

    /// Whether `token[pos + offset]` is the ASCII byte `b`.
    #[inline]
    fn is(&self, offset: isize, b: u8) -> bool {
        self.at(offset).is_some_and(|u| u.eq_ascii(b))
    }

    /// Whether `token[pos + offset]` is one of `bytes`.
    #[inline]
    fn is_any(&self, offset: isize, bytes: &[u8]) -> bool {
        self.at(offset)
            .and_then(Unit::to_ascii)
            .is_some_and(|b| bytes.contains(&b))
    }

    /// `token.substring(start, end)`.
    ///
    /// `String#substring` **clamps negative indices to zero and swaps its
    /// arguments when start > end**. Every negative offset in the reference
    /// depends on that: at `pos == 0`, `substring(pos-1, pos)` is `""` and
    /// `substring(pos-2, pos+4)` is `substring(0, 4)`. Rust slicing would panic.
    #[inline]
    fn substring(&self, start: isize, end: isize) -> &[U] {
        let len = self.token.len() as isize;
        let a = start.clamp(0, len);
        let b = end.clamp(0, len);
        let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
        &self.token[lo as usize..hi as usize]
    }

    /// `token.substring(pos + start, pos + end)`.
    #[inline]
    fn sub(&self, start: isize, end: isize) -> &[U] {
        let pos = self.pos as isize;
        self.substring(pos + start, pos + end)
    }

    /// `subMatch(start, end, terms)`.
    #[inline]
    fn sub_match(&self, start: isize, end: isize, terms: &[&[u8]]) -> bool {
        any_ascii_slice(self.sub(start, end), terms)
    }

    /// `subMatchAbsolute(start, end, terms)`.
    #[inline]
    fn sub_match_abs(&self, start: isize, end: isize, terms: &[&[u8]]) -> bool {
        any_ascii_slice(self.substring(start, end), terms)
    }

    /// Whether `token.substring(pos + start, pos + end)` equals `term`.
    #[inline]
    fn sub_is(&self, start: isize, end: isize, term: &[u8]) -> bool {
        eq_ascii_slice(self.sub(start, end), term)
    }

    /// Whether `token.substring(start, end)` equals `term`.
    #[inline]
    fn sub_abs_is(&self, start: isize, end: isize, term: &[u8]) -> bool {
        eq_ascii_slice(self.substring(start, end), term)
    }

    /// `pos === token.length - n`
    #[inline]
    fn at_end_minus(&self, n: usize) -> bool {
        self.pos + n == self.token.len()
    }

    // -- accumulators --------------------------------------------------------

    /// `add(appendage)` — appends to both encodings.
    #[inline]
    fn add(&mut self, appendage: &str) {
        self.primary.push_str(appendage);
        self.secondary.push_str(appendage);
    }

    /// `addSecondary(primary, secondary)` — appends different text to each.
    #[inline]
    fn add_secondary(&mut self, primary: &str, secondary: &str) {
        self.primary.push_str(primary);
        self.secondary.push_str(secondary);
    }

    /// `addCompressedDouble(c)` — consumes a doubled letter, encoding it once.
    #[inline]
    fn add_compressed_double(&mut self, c: u8) {
        self.add_compressed_double_as(c, None);
    }

    /// `addCompressedDouble(c, encoded)`.
    fn add_compressed_double_as(&mut self, c: u8, encoded: Option<&str>) {
        if self.is(1, c) {
            self.pos += 1;
        }
        match encoded {
            Some(s) => self.add(s),
            // `c` is always an ASCII letter passed in by the switch in `step`,
            // so pushing it as a `char` is exact — and, unlike
            // `(c as char).to_string()`, allocates nothing.
            None => {
                let ch = c as char;
                self.primary.push(ch);
                self.secondary.push(ch);
            }
        }
    }

    // -- the switch ----------------------------------------------------------

    fn step(&mut self) {
        let c = self.token[self.pos];
        if let Some(b) = c.to_ascii() {
            match b {
                b'A' | b'E' | b'I' | b'O' | b'U' | b'Y' => {
                    if self.pos == 0 {
                        self.add("A");
                    }
                }
                b'B' => self.add_compressed_double_as(b'B', Some("P")),
                b'C' => self.handle_c(),
                b'D' => self.handle_d(),
                b'F' | b'K' | b'N' => self.add_compressed_double(b),
                b'G' => self.handle_g(),
                b'H' => self.handle_h(),
                b'J' => self.handle_j(),
                b'L' => self.handle_l(),
                b'M' => self.handle_m(),
                b'P' => self.handle_p(),
                b'Q' => self.add_compressed_double_as(b'Q', Some("K")),
                b'R' => self.handle_r(),
                b'S' => self.handle_s(),
                b'T' => self.handle_t(),
                b'V' => self.add_compressed_double_as(b'V', Some("F")),
                b'W' => self.handle_w(),
                b'X' => self.handle_x(),
                b'Z' => self.handle_z(),
                _ => {}
            }
            return;
        }
        // The only non-ASCII cases the switch names. Every other accented
        // letter — Á, Ô, Ü, … — falls through and contributes nothing.
        match c.to_utf16() {
            // Ê, É, À: word-initial vowels, even though `isVowel` rejects them.
            0x00CA | 0x00C9 | 0x00C0 => {
                if self.pos == 0 {
                    self.add("A");
                }
            }
            0x00C7 => self.add("S"), // Ç
            0x00D1 => self.add("N"), // Ñ
            _ => {}
        }
    }

    fn handle_c(&mut self) {
        if (self.pos >= 1
            && !is_vowel(self.at(-2))
            && self.is(-1, b'A')
            && self.is(1, b'H')
            && !self.is(2, b'I'))
            || self.sub_match(-2, 4, &[b"BACHER", b"MACHER"])
        {
            self.add("K");
            self.pos += 1;
        } else if self.pos == 0 && self.sub_abs_is(1, 6, b"EASAR") {
            self.add("S");
            self.add("S");
            self.add("R");
            self.pos += 6;
        } else if self.sub_is(1, 4, b"HIA") {
            self.add("K");
            self.pos += 1;
        } else if self.is(1, b'H') {
            if self.pos > 0 && self.sub_is(2, 4, b"AE") {
                self.add_secondary("K", "X");
                self.pos += 1;
            } else if self.pos == 0
                && (self.sub_match(1, 6, &[b"HARAC", b"HARIS"])
                    || self.sub_match(1, 4, &[b"HOR", b"HUM", b"HIA", b"HEM"]))
                && !self.sub_is(1, 5, b"HORE")
            {
                self.add("K");
                self.pos += 1;
            } else {
                if self.sub_match_abs(0, 3, &[b"VAN", b"VON"])
                    || self.sub_abs_is(0, 3, b"SCH")
                    || self.sub_match(-2, 4, &[b"ORCHES", b"ARCHIT", b"ORCHID"])
                    || self.sub_match(2, 3, &[b"T", b"S"])
                    || ((self.sub_match(-1, 0, &[b"A", b"O", b"U", b"E"]) || self.pos == 0)
                        && self.sub_match(
                            2,
                            3,
                            &[b"B", b"F", b"H", b"L", b"M", b"N", b"R", b"V", b"W"],
                        ))
                {
                    self.add("K");
                } else if self.pos > 0 {
                    if self.sub_abs_is(0, 2, b"MC") {
                        self.add("K");
                    } else {
                        self.add_secondary("X", "K");
                    }
                } else {
                    self.add("X");
                }
                self.pos += 1;
            }
        } else if self.sub_is(0, 2, b"CZ") {
            // The guard `token.substring(pos-2, pos+1) !== 'WICZ'` compares a
            // slice of at most three characters with a four-character literal:
            // always true. Restoring the intended offsets would change
            // `markiewicz`.
            self.add_secondary("S", "X");
            self.pos += 1;
        } else if self.sub_is(0, 3, b"CIA") {
            self.add("X");
            self.pos += 2;
        } else if self.is(1, b'C')
            && self.pos != 1
            && !self.at_abs(0).is_some_and(|u| u.eq_ascii(b'M'))
        {
            if self.is_any(2, b"IEH") && !self.sub_is(2, 4, b"HU") {
                // The reference also tests `pos === 1 && token[pos-1] === 'A'`,
                // which this branch has already ruled out.
                if self.sub_match(-1, 4, &[b"UCCEE", b"UCCES"]) {
                    self.add("KS");
                } else {
                    self.add("X");
                }
                self.pos += 2;
            } else {
                self.add("K");
                self.pos += 1;
            }
        } else if self.is_any(1, b"KGQ") {
            self.add("K");
            self.pos += 1;
        } else if self.is_any(1, b"EIY") {
            if self.sub_match(1, 3, &[b"IA", b"IE", b"IO"]) {
                self.add_secondary("S", "X");
            } else {
                self.add("S");
            }
            self.pos += 1;
        } else {
            self.add("K");
            // `['C','Q','G'].indexOf(x)` is used raw as a condition. It yields
            // 0 — falsy — for 'C', and -1 — truthy — for anything absent, so the
            // test fires for every character EXCEPT 'C'. Hence `MAC CAT` encodes
            // its second C while `MAC GAT` skips two characters.
            if self.is(1, b' ') && !self.is(2, b'C') {
                self.pos += 2;
            } else if self.is_any(1, b"CKQ") && !self.sub_match(1, 3, &[b"CE", b"CI"]) {
                self.pos += 1;
            }
        }
    }

    fn handle_d(&mut self) {
        if self.is(1, b'G') {
            if self.is_any(2, b"IEY") {
                self.add("J");
                self.pos += 2;
            } else {
                self.add("TK");
                self.pos += 1;
            }
        } else if self.is(1, b'T') {
            self.add("T");
            self.pos += 1;
        } else {
            self.add_compressed_double_as(b'D', Some("T"));
        }
    }

    fn handle_g(&mut self) {
        if self.is(1, b'H') {
            if self.pos > 0 && !is_vowel(self.at(-1)) {
                self.add("K");
                self.pos += 1;
            } else if self.pos == 0 {
                if self.is(2, b'I') {
                    self.add("J");
                } else {
                    self.add("K");
                }
                self.pos += 1;
            } else if self.pos > 1
                && (self.is_any(-2, b"BHD") || self.is_any(-3, b"BHD") || self.is_any(-4, b"BH"))
            {
                // Silent: nothing is added, and the H is consumed.
                self.pos += 1;
            } else {
                if self.pos > 2 && self.is(-1, b'U') && self.is_any(-3, b"CGLRT") {
                    self.add("F");
                } else if !self.is(-1, b'I') {
                    self.add("K");
                }
                self.pos += 1;
            }
        } else if self.is(1, b'N') {
            if self.pos == 1 && self.starts_with_vowel && !self.slavo_germanic {
                self.add_secondary("KN", "N");
            } else if !self.sub_is(2, 4, b"EY") && !self.slavo_germanic {
                // The reference also tests `token[pos+1] !== 'Y'`, which this
                // branch has already established to be 'N'.
                self.add_secondary("N", "KN");
            } else {
                self.add("KN");
            }
            self.pos += 1;
        } else if self.sub_is(1, 3, b"LI") && !self.slavo_germanic {
            self.add_secondary("KL", "L");
            self.pos += 1;
        } else if self.pos == 0
            && (self.is(1, b'Y')
                || self.sub_match(
                    1,
                    3,
                    &[
                        b"ES", b"EP", b"EB", b"EL", b"EY", b"IB", b"IL", b"IN", b"IE", b"EI", b"ER",
                    ],
                ))
        {
            self.add_secondary("K", "J");
        } else {
            self.add_compressed_double_as(b'G', Some("K"));
        }
    }

    fn handle_h(&mut self) {
        // Kept only when it starts a word or sits between two vowels. The
        // `pos++` consumes the FOLLOWING vowel, which is never encoded.
        if (self.pos == 0 || is_vowel(self.at(-1))) && is_vowel(self.at(1)) {
            self.add("H");
            self.pos += 1;
        }
    }

    fn handle_j(&mut self) {
        let jose = self.sub_is(1, 4, b"OSE");
        if self.san || jose {
            if (self.pos == 0 && self.is(4, b' ')) || self.san {
                self.add("H");
            } else {
                // `add('J', 'H')`: `add` takes one parameter, so the second
                // argument is discarded and BOTH encodings get 'J'.
                self.add("J");
            }
        } else if self.pos == 0 {
            self.add_secondary("J", "A");
        } else if is_vowel(self.at(-1))
            && !self.slavo_germanic
            && (self.is(1, b'A') || self.is(1, b'O'))
        {
            self.add_secondary("J", "H");
        } else if self.at_end_minus(1) {
            // A literal space in the secondary encoding.
            self.add_secondary("J", " ");
        } else {
            self.add_compressed_double(b'J');
        }
    }

    fn handle_l(&mut self) {
        if self.is(1, b'L') {
            // The reference's second disjunct requires the same four characters
            // the first list already contains, and compares a boolean with -1
            // (always true), so it can never add anything.
            if self.at_end_minus(3) && self.sub_match(-1, 3, &[b"ILLO", b"ILLA", b"ALLE"]) {
                self.add_secondary("L", "");
                self.pos += 1;
                return;
            }
            self.pos += 1;
        }
        self.add("L");
    }

    fn handle_m(&mut self) {
        self.add_compressed_double(b'M');
        // Evaluated against the pos `addCompressedDouble` may have just
        // advanced — the order of side effects is observable for `MM` clusters.
        if self.is(-1, b'U')
            && self.is(1, b'B')
            && (self.at_end_minus(2) || self.sub_is(2, 4, b"ER"))
        {
            self.pos += 1;
        }
    }

    fn handle_p(&mut self) {
        if self.is(1, b'H') {
            self.add("F");
            self.pos += 1;
        } else {
            self.add_compressed_double(b'P');
            if self.is(1, b'B') {
                self.pos += 1;
            }
        }
    }

    fn handle_r(&mut self) {
        // `!subMatch(-4, -3, ['ME','MA'])` compares a single character with
        // two-character literals: always true, so it is omitted here.
        if self.at_end_minus(1) && !self.slavo_germanic && self.sub_is(-2, 0, b"IE") {
            self.add_secondary("", "R");
        } else {
            self.add_compressed_double(b'R');
        }
    }

    fn handle_s(&mut self) {
        if self.pos == 0 && self.sub_abs_is(0, 5, b"SUGAR") {
            // Note: pos is NOT advanced, so the U is encoded as usual.
            self.add_secondary("X", "S");
        } else if self.is(1, b'H') {
            if self.sub_match(2, 5, &[b"EIM", b"OEK", b"OLM", b"OLZ"]) {
                self.add("S");
            } else {
                self.add("X");
            }
            self.pos += 1;
        } else if self.sub_match(1, 3, &[b"IO", b"IA"]) {
            if self.slavo_germanic {
                self.add("S");
            } else {
                self.add_secondary("S", "X");
            }
            self.pos += 1;
        } else if (self.pos == 0 && self.is_any(1, b"MNLW")) || self.is(1, b'Z') {
            self.add_secondary("S", "X");
            if self.is(1, b'Z') {
                self.pos += 1;
            }
        } else if self.sub_is(0, 2, b"SC") {
            if self.is(2, b'H') {
                if self.sub_match(3, 5, &[b"ER", b"EN"]) {
                    self.add_secondary("X", "SK");
                } else if self.sub_match(3, 5, &[b"OO", b"UY", b"ED", b"EM"]) {
                    self.add("SK");
                } else if self.pos == 0
                    && !is_vowel(self.at_abs(3))
                    && !self.at_abs(3).is_some_and(|u| u.eq_ascii(b'W'))
                {
                    self.add_secondary("X", "S");
                } else {
                    self.add("X");
                }
            } else if self.is_any(2, b"IEY") {
                self.add("S");
            } else {
                self.add("SK");
            }
            self.pos += 2;
        } else if self.at_end_minus(1) && self.sub_match(-2, 0, &[b"AI", b"OI"]) {
            self.add_secondary("", "S");
        } else if !self.is(1, b'L') && !self.is(-1, b'A') && !self.is(-1, b'I') {
            self.add_compressed_double(b'S');
            if self.is(1, b'Z') {
                self.pos += 1;
            }
        }
        // Otherwise the S is silently dropped: `ISLE` encodes as `AL`.
    }

    fn handle_t(&mut self) {
        if self.sub_is(1, 4, b"ION") {
            self.add("XN");
            self.pos += 3;
        } else if self.sub_match(1, 3, &[b"IA", b"CH"]) {
            self.add("X");
            self.pos += 2;
        } else if self.is(1, b'H') {
            // The reference's second disjunct, `token.substring(1,2) === 'TH'`,
            // compares one character with two and is always false.
            if self.sub_match(2, 4, &[b"OM", b"AM"])
                || self.sub_match_abs(0, 4, &[b"VAN ", b"VON "])
                || self.sub_abs_is(0, 3, b"SCH")
            {
                self.add("T");
            } else {
                self.add_secondary("0", "T");
            }
            self.pos += 1;
        } else {
            self.add_compressed_double(b'T');
            if self.is(1, b'D') {
                self.pos += 1;
            }
        }
    }

    fn handle_w(&mut self) {
        if self.pos == 0 {
            if self.at_abs(1).is_some_and(|u| u.eq_ascii(b'H')) {
                self.add("A");
            } else if is_vowel(self.at_abs(1)) {
                self.add_secondary("A", "F");
            }
        } else if self.sub_match(-1, 4, &[b"EWSKI", b"EWSKY", b"OWSKI", b"OWSKY"])
            || self.sub_abs_is(0, 3, b"SCH")
            || (self.at_end_minus(1) && is_vowel(self.at(-1)))
        {
            self.add_secondary("", "F");
            // This `pos++` consumes a character that was never part of a
            // digraph: in `LEBOWSKI` the S after `OWSKI` is never encoded.
            self.pos += 1;
        } else if any_ascii_slice(self.sub(1, 4), &[b"ICZ", b"ITZ"]) {
            self.add_secondary("TS", "FX");
            self.pos += 3;
        }
    }

    fn handle_x(&mut self) {
        if self.pos == 0 {
            self.add("S");
        } else if !(self.at_end_minus(1)
            && (any_ascii_slice(self.sub(-3, 0), &[b"IAU", b"EAU", b"IEU"])
                || any_ascii_slice(self.sub(-2, 0), &[b"AU", b"OU"])))
        {
            self.add("KS");
        }
    }

    fn handle_z(&mut self) {
        if self.is(1, b'H') {
            self.add("J");
            self.pos += 1;
        } else if self.sub_match(1, 3, &[b"ZO", b"ZI", b"ZA"])
            || (self.slavo_germanic && self.pos > 0 && !self.is(-1, b'T'))
        {
            self.add_secondary("S", "TS");
            // In the slavo-germanic case no `ZZ` was consumed, so this skips a
            // real letter.
            self.pos += 1;
        } else {
            self.add_compressed_double_as(b'Z', Some("S"));
        }
    }
}

impl verbora_core::DoubleKeyPhonetic for DoubleMetaphone {
    fn process_double(&self, token: &str) -> (String, String) {
        Self::process(self, token)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dm() -> DoubleMetaphone {
        DoubleMetaphone::new()
    }

    fn p(token: &str) -> (String, String) {
        dm().process(token)
    }

    fn want(a: &str, b: &str) -> (String, String) {
        (a.to_owned(), b.to_owned())
    }

    #[test]
    fn encodes_the_reference_vectors() {
        for (input, a, b) in [
            ("", "", ""),
            ("a", "A", "A"),
            ("ceasar", "SSR", "SSR"),
            ("ach", "AK", "AK"),
            ("chemical", "KMKL", "KMKL"),
            ("choral", "KRL", "KRL"),
            ("chore", "XR", "XR"),
            ("ptah", "PT", "PT"),
            ("Matrix", "MTRKS", "MTRKS"),
            ("intervention", "ANTRFNXN", "ANTRFNXN"),
            ("complete", "KMPLT", "KMPLT"),
            ("appropriate", "APRPRT", "APRPRT"),
            ("England", "ANKLNT", "ANKLNT"),
            ("thomas", "TM", "TM"),
            ("nation", "NXN", "NXN"),
            ("school", "SKL", "SKL"),
            ("wrong", "RNK", "RNK"),
            ("wheat", "AT", "AT"),
            ("tough", "TF", "TF"),
            ("gift", "KFT", "KFT"),
            ("hugh", "H", "H"),
            ("box", "PKS", "PKS"),
            ("zookeeper", "SKPR", "SKPR"),
        ] {
            assert_eq!(p(input), want(a, b), "for {input:?}");
        }
    }

    #[test]
    fn the_two_encodings_can_differ() {
        assert_eq!(p("astromech"), want("ATRMX", "ATRMK"));
        assert_eq!(p("Français"), want("FRNS", "FRNSS"));
        assert_eq!(p("germany"), want("KRMN", "JRMN"));
        assert_eq!(p("scherzando"), want("XRSNT", "SKRSNT"));
        assert_eq!(p("this"), want("0", "T"));
        assert_eq!(p("thatch"), want("0X", "TX"));
        assert_eq!(p("pizza"), want("PS", "PTS"));
        assert_eq!(p("sugar"), want("XKR", "SKR"));
    }

    #[test]
    fn accented_letters_only_where_the_switch_names_them() {
        assert_eq!(p("être"), want("ATR", "ATR"));
        assert_eq!(p("éte"), want("AT", "AT"));
        assert_eq!(p("leçon"), want("LSN", "LSN"));
        assert_eq!(p("jalapeño"), want("JLPN", "ALPN"));
    }

    #[test]
    fn san_and_jose_special_cases() {
        assert_eq!(p("san juan"), want("SNHN", "SNHN"));
        assert_eq!(p("jose"), want("JS", "JS")); // add('J','H') drops the 'H'
        assert_eq!(p("san jose"), want("SNHS", "SNHS"));
        assert_eq!(p("JOSE X"), want("HSKS", "HSKS"));
    }

    #[test]
    fn j_at_the_end_puts_a_space_in_the_secondary() {
        assert_eq!(p("hadj"), want("HTJ", "HT "));
        assert_eq!(p("J"), want("J", "A"));
        assert_eq!(p("JJ"), want("JJ", "A "));
    }

    #[test]
    fn the_indexof_truthiness_bug_skips_characters() {
        // 'C' is at index 0 — falsy — so the skip does NOT happen and the second
        // C is encoded. Every other letter skips two characters.
        assert_eq!(p("MAC CAT"), want("MKKT", "MKKT"));
        assert_eq!(p("MAC GAT"), want("MKT", "MKT"));
        assert_eq!(p("MAC QAT"), want("MKT", "MKT"));
        assert_eq!(p("MAC XAT"), want("MKT", "MKT"));
    }

    #[test]
    fn stray_pos_increments_swallow_letters() {
        // The S after OWSKI is never encoded.
        assert_eq!(p("lebowski"), want("LPK", "LPFK"));
        assert_eq!(p("Lowicz"), want("LTS", "LFX"));
        assert_eq!(p("markiewicz"), want("MRKTS", "MRKFX"));
    }

    #[test]
    fn always_true_and_always_false_conditions() {
        // WICZ guard is always true, so CZ always takes the S/X branch.
        assert_eq!(p("wicz"), want("AS", "FX"));
        // handleR's ME/MA guard is always true.
        assert_eq!(p("papier"), want("PP", "PPR"));
        // handleT's `substring(1,2) === 'TH'` is always false.
        assert_eq!(p("matta"), want("MT", "MT"));
    }

    #[test]
    fn silent_s_and_unreachable_ll_branch() {
        assert_eq!(p("isle"), want("AL", "AL"));
        assert_eq!(p("ISL"), want("AL", "AL"));
        assert_eq!(p("cabrillo"), want("KPRL", "KPR"));
        assert_eq!(p("caballe"), want("KPL", "KP"));
        assert_eq!(p("casallo"), want("KL", "KL"));
    }

    #[test]
    fn gh_and_gn_branches() {
        assert_eq!(p("ghislaine"), want("JLN", "JLN"));
        assert_eq!(p("consign"), want("KNSN", "KNSKN"));
        assert_eq!(p("agnosia"), want("AKNS", "ANX"));
        assert_eq!(p("ANGN"), want("ANN", "ANKN"));
        assert_eq!(p("YGN"), want("AKN", "AN"));
        assert_eq!(p("GNEY"), want("N", "N"));
        assert_eq!(p("taglianetti"), want("TKLNT", "TLNT"));
        assert_eq!(p("Afghani"), want("AFKN", "AFKN"));
    }

    #[test]
    fn c_branches() {
        assert_eq!(p("McHenry"), want("MKNR", "MKNR"));
        assert_eq!(p("mccarthy"), want("MKR0", "MKRT"));
        assert_eq!(p("accede"), want("AKST", "AKST"));
        assert_eq!(p("succeed"), want("SKST", "SKST"));
        assert_eq!(p("muccee"), want("MKS", "MKS"));
        assert_eq!(p("von Chor"), want("FNKR", "FNKR"));
        assert_eq!(p("orchestrational"), want("ARKSTRXNL", "ARKSTRXNL"));
        assert_eq!(p("chthonic"), want("K0NK", "KTNK"));
        assert_eq!(p("character"), want("KRKTR", "KRKTR"));
        assert_eq!(p("archaeology"), want("ARKLK", "ARXLK"));
        assert_eq!(p("chianti"), want("KNT", "KNT"));
        assert_eq!(p("indicia"), want("ANTX", "ANTX"));
        assert_eq!(p("bacci"), want("PX", "PX"));
        assert_eq!(p("racquetball"), want("RKTPL", "RKTPL"));
        assert_eq!(p("sociopath"), want("SSP0", "SXPT"));
        assert_eq!(p("Mac Ghille dhuibh"), want("MKLTP", "MKLTP"));
    }

    #[test]
    fn edge_and_unicode_inputs() {
        assert_eq!(p("ß"), want("S", "S")); // toUpperCase expands to SS
        assert_eq!(p("😀"), want("", ""));
        assert_eq!(p("1234"), want("", ""));
        assert_eq!(p("Москва"), want("", ""));
        assert_eq!(p("日本語"), want("", ""));
        assert_eq!(p(&"a".repeat(500)), want("A", "A"));
    }

    #[test]
    fn max_length_truncates_both_encodings() {
        let d = dm();
        assert_eq!(d.process_with("Matrix", Some(4.0)), want("MTRK", "MTRK"));
        assert_eq!(d.process_with("Français", Some(4.0)), want("FRNS", "FRNS"));
        assert_eq!(d.process_with("Français", Some(5.0)), want("FRNS", "FRNSS"));
        assert_eq!(d.process_with("Matrix", Some(32.0)), want("MTRKS", "MTRKS"));
        assert_eq!(d.process_with("Matrix", Some(-1.0)), want("", ""));
        assert_eq!(d.process_with("Matrix", Some(0.0)), want("MTRKS", "MTRKS"));
    }

    #[test]
    fn is_vowel_counts_y_and_rejects_accents() {
        let d = dm();
        assert!(d.is_vowel("a"));
        assert!(d.is_vowel("Y"));
        assert!(!d.is_vowel("é"));
        assert!(!d.is_vowel("É"));
        assert!(!d.is_vowel(""));
        assert!(!d.is_vowel("bc"));
        // `.match` searches anywhere, so a multi-character string can match.
        assert!(d.is_vowel("ay"));
    }

    #[test]
    fn compare_matches_on_either_encoding() {
        let d = dm();
        assert!(d.compare("love", "luv"));
        assert!(!d.compare("love", "hate"));
    }
}
