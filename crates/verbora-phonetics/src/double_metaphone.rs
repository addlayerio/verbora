//! Double Metaphone (Philips, 2000).

use std::fmt;

use crate::letters::name_letters_into;

/// The maximum length of either key, in characters.
///
/// Philips's algorithm is specified with this cap: the encoding loop stops as
/// soon as both keys have reached it, and both are truncated to it before they
/// are returned. It is part of the algorithm, not a Verbora convenience — a
/// key of unbounded length would defeat the purpose of a blocking key.
const MAX_KEY_LEN: usize = 4;

/// The keys Double Metaphone produced for one name.
///
/// The algorithm always yields a primary key. It yields a **second** key only
/// for a spelling whose pronunciation genuinely forks — `Richard` is `RXRT` in
/// English and `RKRT` under a Germanic reading, so both are recorded; `Smith`
/// forks into `SM0`/`XMT`; `Thompson` does not fork at all. Where the
/// algorithm never took an alternate branch, [`alternate`](Self::alternate) is
/// `None` rather than a duplicate of the primary key: "there is no second
/// pronunciation" and "the second pronunciation happens to be spelled the same"
/// are different facts, and a caller building an index wants to know which one
/// it has.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DoubleMetaphoneCode {
    primary: String,
    alternate: Option<String>,
}

impl DoubleMetaphoneCode {
    /// The primary key. At most four characters; empty only for
    /// a name with no `A`–`Z` letter.
    #[must_use]
    pub fn primary(&self) -> &str {
        &self.primary
    }

    /// The alternate key, or `None` when the name has only one pronunciation
    /// under Philips's rules.
    #[must_use]
    pub fn alternate(&self) -> Option<&str> {
        self.alternate.as_deref()
    }

    /// Whether `key` is one of this name's keys.
    #[must_use]
    pub fn contains(&self, key: &str) -> bool {
        self.primary == key || self.alternate.as_deref() == Some(key)
    }

    /// Whether `self` and `other` share at least one key — the relation
    /// [`DoubleMetaphone::compare`] tests.
    #[must_use]
    pub fn shares_key_with(&self, other: &Self) -> bool {
        other.contains(&self.primary)
            || self
                .alternate
                .as_deref()
                .is_some_and(|alt| other.contains(alt))
    }

    /// Consumes the code, returning `(primary, alternate)`.
    #[must_use]
    pub fn into_parts(self) -> (String, Option<String>) {
        (self.primary, self.alternate)
    }
}

impl fmt::Display for DoubleMetaphoneCode {
    /// The primary key alone, so `to_string()` never silently hides the fork.
    /// Read [`alternate`](Self::alternate) when you need both.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.primary)
    }
}

/// Double Metaphone — one key per plausible pronunciation of a name.
///
/// # Publication
///
/// Lawrence Philips, "The Double Metaphone Search Algorithm", *C/C++ Users
/// Journal* 18(6), June 2000. Verbora implements the rule set that article
/// specifies, including its four-character key cap and its branch conditions
/// for Slavo-Germanic, Romance, Greek and Chinese spellings.
///
/// # The contract
///
/// * **The text unit is one Unicode scalar.** Only the twenty-six letters
///   `A`–`Z` are read, after simple ASCII case folding — plus **one space**
///   wherever whitespace separates two letters. The space is not an accident
///   of tokenization: Philips's rules test for it by name (`VAN `, `VON `,
///   `SAN `, the French `-IER ` ending, and the word-boundary alternative in
///   the `CH` rule), because the algorithm was specified over whole personal
///   names. Every other scalar — accents, digits, punctuation, non-Latin
///   scripts — is skipped without leaving a gap, so `"O'Brien"` and
///   `"OBrien"` encode identically while `"Van Der Berg"` and `"VanDerBerg"`
///   do not.
/// * **Two keys, or one.** See [`DoubleMetaphoneCode`]: the alternate is
///   `Option`, not a duplicated primary.
/// * **Both keys are at most four characters**, per the algorithm.
/// * A name with no `A`–`Z` letter yields an empty primary key and no
///   alternate.
/// * **Total.** No input panics, and there is no error type.
///
/// # Examples
///
/// ```
/// use verbora_phonetics::DoubleMetaphone;
///
/// let dm = DoubleMetaphone::new();
///
/// let smith = dm.process("Smith");
/// assert_eq!(smith.primary(), "SM0");
/// assert_eq!(smith.alternate(), Some("XMT"));
///
/// // Schmidt's primary is Smith's alternate, which is the whole point.
/// assert!(dm.compare("Smith", "Schmidt"));
///
/// // Thompson has one pronunciation, so there is no alternate key at all.
/// assert_eq!(dm.process("Thompson").alternate(), None);
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DoubleMetaphone;

impl DoubleMetaphone {
    /// Creates a Double Metaphone encoder.
    ///
    /// The encoder is stateless and zero-sized.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Encodes `name`.
    #[must_use]
    pub fn process(&self, name: &str) -> DoubleMetaphoneCode {
        let mut word = Vec::new();
        name_letters_into(name, &mut word);
        Encoder::new(&word).run()
    }

    /// Whether `a` and `b` share at least one key.
    ///
    /// This is the relation Double Metaphone exists to provide, and it is
    /// deliberately *not* primary-key equality: `Smith` and `Schmidt` have
    /// different primary keys and match on `Smith`'s alternate.
    #[must_use]
    pub fn compare(&self, a: &str, b: &str) -> bool {
        self.process(a).shares_key_with(&self.process(b))
    }
}

impl verbora_core::DoubleKeyPhonetic for DoubleMetaphone {
    /// `(primary, alternate)`, with `None` when the name has no second
    /// pronunciation.
    ///
    /// The same values [`DoubleMetaphone::process`] returns, flattened out of
    /// [`DoubleMetaphoneCode`]. Until 2026-08 the trait's second element was a
    /// `String` and this impl repeated the primary into it, which made "one
    /// pronunciation" indistinguishable from "two that encode alike" and
    /// doubled every index built from both keys.
    fn process_double(&self, token: &str) -> (String, Option<String>) {
        self.process(token).into_parts()
    }
}

/// The single left-to-right pass.
struct Encoder<'a> {
    /// The prepared name: uppercase `A`–`Z` plus interior spaces.
    word: &'a [u8],
    /// `word.len()`, cached because every rule reads it.
    len: usize,
    primary: String,
    alternate: String,
    /// Set the first time a rule proposes different text for the two keys.
    forked: bool,
    /// True when the name contains `W`, `K` or `CZ` — Philips's test for a
    /// Slavo-Germanic spelling.
    slavo_germanic: bool,
    pos: usize,
}

impl<'a> Encoder<'a> {
    fn new(word: &'a [u8]) -> Self {
        let slavo_germanic = word
            .iter()
            .enumerate()
            .any(|(i, &c)| c == b'W' || c == b'K' || (c == b'C' && word.get(i + 1) == Some(&b'Z')));
        Self {
            word,
            len: word.len(),
            primary: String::with_capacity(MAX_KEY_LEN),
            alternate: String::with_capacity(MAX_KEY_LEN),
            forked: false,
            slavo_germanic,
            pos: 0,
        }
    }

    // -- reading the name ----------------------------------------------------

    /// The letter at `index`, treating everything past the end as the space
    /// Philips's padded buffer holds there, and everything before the start as
    /// absent.
    ///
    /// The padding is load-bearing: rules such as `CH` before
    /// `L R N M B H F V W` **or a space** use it to mean "or the name ends
    /// here". Without it those clauses would be unreachable.
    #[inline]
    fn at(&self, index: isize) -> Option<u8> {
        if index < 0 {
            return None;
        }
        let index = index as usize;
        if index < self.len {
            Some(self.word[index])
        } else if index < self.len + MAX_KEY_LEN {
            Some(b' ')
        } else {
            None
        }
    }

    /// Whether the `n` characters starting at `index` equal one of `terms`.
    ///
    /// `false` for a negative `index` or one past the padded buffer, which is
    /// how Philips's `StringAt` behaves and how every `pos - 2` style lookup in
    /// the rules stays in range near the start of a name.
    #[inline]
    fn eq_at(&self, index: isize, n: usize, terms: &[&[u8]]) -> bool {
        if index < 0 || self.at(index).is_none() {
            return false;
        }
        let mut window = [b' '; 8];
        let n = n.min(window.len());
        for (k, slot) in window[..n].iter_mut().enumerate() {
            match self.at(index + k as isize) {
                Some(c) => *slot = c,
                None => return false,
            }
        }
        terms.iter().any(|t| *t == &window[..n])
    }

    /// Whether the one character at `index` is any of `letters`.
    #[inline]
    fn one_of(&self, index: isize, letters: &[u8]) -> bool {
        self.at(index).is_some_and(|c| letters.contains(&c))
    }

    #[inline]
    fn is(&self, index: isize, letter: u8) -> bool {
        self.at(index) == Some(letter)
    }

    /// Philips's `IsVowel`, which counts `Y`.
    #[inline]
    fn vowel(&self, index: isize) -> bool {
        self.one_of(index, b"AEIOUY")
    }

    #[inline]
    fn cur(&self) -> isize {
        self.pos as isize
    }

    #[inline]
    fn at_last(&self) -> bool {
        self.len > 0 && self.pos == self.len - 1
    }

    // -- writing the keys ----------------------------------------------------

    /// Appends the same text to both keys.
    #[inline]
    fn add(&mut self, both: &str) {
        self.primary.push_str(both);
        self.alternate.push_str(both);
    }

    /// Appends different text to each key, recording that the name forked.
    #[inline]
    fn add_split(&mut self, primary: &str, alternate: &str) {
        self.primary.push_str(primary);
        self.alternate.push_str(alternate);
        if primary != alternate {
            self.forked = true;
        }
    }

    fn run(mut self) -> DoubleMetaphoneCode {
        // Word-initial silent letters.
        if self.eq_at(0, 2, &[b"GN", b"KN", b"PN", b"WR", b"PS"]) {
            self.pos = 1;
        }
        // Initial X, as in the Chinese "Xu": pronounced "S".
        if self.is(0, b'X') {
            self.add("S");
            self.pos = 1;
        }

        while self.pos < self.len
            && (self.primary.len() < MAX_KEY_LEN || self.alternate.len() < MAX_KEY_LEN)
        {
            self.step();
        }

        self.primary.truncate(MAX_KEY_LEN);
        self.alternate.truncate(MAX_KEY_LEN);
        let alternate = (self.forked && self.alternate != self.primary).then_some(self.alternate);
        DoubleMetaphoneCode {
            primary: self.primary,
            alternate,
        }
    }

    fn step(&mut self) {
        let c = self.word[self.pos];
        match c {
            b'A' | b'E' | b'I' | b'O' | b'U' | b'Y' => {
                // Every word-initial vowel is `A`; every other vowel is silent.
                if self.pos == 0 {
                    self.add("A");
                }
                self.pos += 1;
            }
            b'B' => {
                self.add("P");
                self.pos += if self.is(self.cur() + 1, b'B') { 2 } else { 1 };
            }
            b'C' => self.handle_c(),
            b'D' => self.handle_d(),
            b'F' => {
                self.add("F");
                self.pos += if self.is(self.cur() + 1, b'F') { 2 } else { 1 };
            }
            b'G' => self.handle_g(),
            b'H' => {
                // Kept only word-initially before a vowel, or between vowels.
                if (self.pos == 0 || self.vowel(self.cur() - 1)) && self.vowel(self.cur() + 1) {
                    self.add("H");
                    self.pos += 2;
                } else {
                    self.pos += 1;
                }
            }
            b'J' => self.handle_j(),
            b'K' => {
                self.add("K");
                self.pos += if self.is(self.cur() + 1, b'K') { 2 } else { 1 };
            }
            b'L' => self.handle_l(),
            b'M' => self.handle_m(),
            b'N' => {
                self.add("N");
                self.pos += if self.is(self.cur() + 1, b'N') { 2 } else { 1 };
            }
            b'P' => {
                if self.is(self.cur() + 1, b'H') {
                    self.add("F");
                    self.pos += 2;
                } else {
                    self.add("P");
                    // "campbell", "raspberry".
                    self.pos += if self.one_of(self.cur() + 1, b"PB") {
                        2
                    } else {
                        1
                    };
                }
            }
            b'Q' => {
                self.add("K");
                self.pos += if self.is(self.cur() + 1, b'Q') { 2 } else { 1 };
            }
            b'R' => self.handle_r(),
            b'S' => self.handle_s(),
            b'T' => self.handle_t(),
            b'V' => {
                self.add("F");
                self.pos += if self.is(self.cur() + 1, b'V') { 2 } else { 1 };
            }
            b'W' => self.handle_w(),
            b'X' => self.handle_x(),
            b'Z' => self.handle_z(),
            // A space, and nothing else: `name_letters_into` yields no other
            // character.
            _ => self.pos += 1,
        }
    }

    fn handle_c(&mut self) {
        let p = self.cur();

        // Germanic "-ACH-", as in "Bach", but not "Bacher"/"Macher".
        if self.pos > 1
            && !self.vowel(p - 2)
            && self.eq_at(p - 1, 3, &[b"ACH"])
            && !self.is(p + 2, b'I')
            && (!self.is(p + 2, b'E') || self.eq_at(p - 2, 6, &[b"BACHER", b"MACHER"]))
        {
            self.add("K");
            self.pos += 2;
            return;
        }
        // "Caesar".
        if self.pos == 0 && self.eq_at(p, 6, &[b"CAESAR"]) {
            self.add("S");
            self.pos += 2;
            return;
        }
        // Italian "chianti".
        if self.eq_at(p, 4, &[b"CHIA"]) {
            self.add("K");
            self.pos += 2;
            return;
        }
        if self.eq_at(p, 2, &[b"CH"]) {
            // "Michael".
            if self.pos > 0 && self.eq_at(p, 4, &[b"CHAE"]) {
                self.add_split("K", "X");
                self.pos += 2;
                return;
            }
            // Greek roots: "chemistry", "chorus", but not "chore".
            if self.pos == 0
                && (self.eq_at(p + 1, 5, &[b"HARAC", b"HARIS"])
                    || self.eq_at(p + 1, 3, &[b"HOR", b"HYM", b"HIA", b"HEM"]))
                && !self.eq_at(0, 5, &[b"CHORE"])
            {
                self.add("K");
                self.pos += 2;
                return;
            }
            // Germanic or Greek "ch" read as "kh".
            if self.eq_at(0, 4, &[b"VAN ", b"VON "])
                || self.eq_at(0, 3, &[b"SCH"])
                || self.eq_at(p - 2, 6, &[b"ORCHES", b"ARCHIT", b"ORCHID"])
                || self.one_of(p + 2, b"TS")
                || ((self.one_of(p - 1, b"AOUE") || self.pos == 0)
                    && self.one_of(p + 2, b"LRNMBHFVW "))
            {
                self.add("K");
            } else if self.pos > 0 {
                if self.eq_at(0, 2, &[b"MC"]) {
                    // "McHugh".
                    self.add("K");
                } else {
                    self.add_split("X", "K");
                }
            } else {
                self.add("X");
            }
            self.pos += 2;
            return;
        }
        // "Czerny", but not the Polish "-wicz" ending.
        if self.eq_at(p, 2, &[b"CZ"]) && !self.eq_at(p - 2, 4, &[b"WICZ"]) {
            self.add_split("S", "X");
            self.pos += 2;
            return;
        }
        // "focaccia".
        if self.eq_at(p + 1, 3, &[b"CIA"]) {
            self.add("X");
            self.pos += 3;
            return;
        }
        // Double C, except in "McClellan".
        if self.eq_at(p, 2, &[b"CC"]) && !(self.pos == 1 && self.is(0, b'M')) {
            // "bellocchio", but not "bacchus".
            if self.one_of(p + 2, b"IEH") && !self.eq_at(p + 2, 2, &[b"HU"]) {
                if (self.pos == 1 && self.is(p - 1, b'A'))
                    || self.eq_at(p - 1, 5, &[b"UCCEE", b"UCCES"])
                {
                    // "accident", "accede", "succeed".
                    self.add("KS");
                } else {
                    // "bacci", "bertucci", other Italian.
                    self.add("X");
                }
                self.pos += 3;
            } else {
                self.add("K");
                self.pos += 2;
            }
            return;
        }
        if self.eq_at(p, 2, &[b"CK", b"CG", b"CQ"]) {
            self.add("K");
            self.pos += 2;
            return;
        }
        if self.eq_at(p, 2, &[b"CI", b"CE", b"CY"]) {
            // Italian vs English.
            if self.eq_at(p, 3, &[b"CIO", b"CIE", b"CIA"]) {
                self.add_split("S", "X");
            } else {
                self.add("S");
            }
            self.pos += 2;
            return;
        }

        self.add("K");
        if self.eq_at(p + 1, 2, &[b" C", b" Q", b" G"]) {
            // "Mac Caffrey", "Mac Gregor".
            self.pos += 3;
        } else if self.one_of(p + 1, b"CKQ") && !self.eq_at(p + 1, 2, &[b"CE", b"CI"]) {
            self.pos += 2;
        } else {
            self.pos += 1;
        }
    }

    fn handle_d(&mut self) {
        let p = self.cur();
        if self.eq_at(p, 2, &[b"DG"]) {
            if self.one_of(p + 2, b"IEY") {
                // "edge".
                self.add("J");
                self.pos += 3;
            } else {
                // "Edgar".
                self.add("TK");
                self.pos += 2;
            }
            return;
        }
        if self.eq_at(p, 2, &[b"DT", b"DD"]) {
            self.add("T");
            self.pos += 2;
            return;
        }
        self.add("T");
        self.pos += 1;
    }

    fn handle_g(&mut self) {
        let p = self.cur();
        if self.is(p + 1, b'H') {
            if self.pos > 0 && !self.vowel(p - 1) {
                self.add("K");
                self.pos += 2;
                return;
            }
            if self.pos == 0 {
                // "Ghislane", "Ghiradelli".
                if self.is(p + 2, b'I') {
                    self.add("J");
                } else {
                    self.add("K");
                }
                self.pos += 2;
                return;
            }
            // Parker's rule: "hugh", "Bough".
            if (self.pos > 1 && self.one_of(p - 2, b"BHD"))
                || (self.pos > 2 && self.one_of(p - 3, b"BHD"))
                || (self.pos > 3 && self.one_of(p - 4, b"BH"))
            {
                self.pos += 2;
                return;
            }
            // "laugh", "McLaughlin", "cough", "gough", "rough", "tough".
            if self.pos > 2 && self.is(p - 1, b'U') && self.one_of(p - 3, b"CGLRT") {
                self.add("F");
            } else if self.pos > 0 && !self.is(p - 1, b'I') {
                self.add("K");
            }
            self.pos += 2;
            return;
        }
        if self.is(p + 1, b'N') {
            if self.pos == 1 && self.vowel(0) && !self.slavo_germanic {
                self.add_split("KN", "N");
            } else if !self.eq_at(p + 2, 2, &[b"EY"])
                && !self.is(p + 1, b'Y')
                && !self.slavo_germanic
            {
                // Not "Cagney".
                self.add_split("N", "KN");
            } else {
                self.add("KN");
            }
            self.pos += 2;
            return;
        }
        // "Tagliaro".
        if self.eq_at(p + 1, 2, &[b"LI"]) && !self.slavo_germanic {
            self.add_split("KL", "L");
            self.pos += 2;
            return;
        }
        // "-ges-", "-gep-", "-gel-", "-gie-" at the start of a name.
        if self.pos == 0
            && (self.is(p + 1, b'Y')
                || self.eq_at(
                    p + 1,
                    2,
                    &[
                        b"ES", b"EP", b"EB", b"EL", b"EY", b"IB", b"IL", b"IN", b"IE", b"EI", b"ER",
                    ],
                ))
        {
            self.add_split("K", "J");
            self.pos += 2;
            return;
        }
        // "-ger-", "-gy-".
        if (self.eq_at(p + 1, 2, &[b"ER"]) || self.is(p + 1, b'Y'))
            && !self.eq_at(0, 6, &[b"DANGER", b"RANGER", b"MANGER"])
            && !self.one_of(p - 1, b"EI")
            && !self.eq_at(p - 1, 3, &[b"RGY", b"OGY"])
        {
            self.add_split("K", "J");
            self.pos += 2;
            return;
        }
        // Italian "biaggi".
        if self.one_of(p + 1, b"EIY") || self.eq_at(p - 1, 4, &[b"AGGI", b"OGGI"]) {
            if self.eq_at(0, 4, &[b"VAN ", b"VON "])
                || self.eq_at(0, 3, &[b"SCH"])
                || self.eq_at(p + 1, 2, &[b"ET"])
            {
                // Obviously Germanic.
                self.add("K");
            } else if self.eq_at(p + 1, 4, &[b"IER "]) {
                // Always soft with a French ending.
                self.add("J");
            } else {
                self.add_split("J", "K");
            }
            self.pos += 2;
            return;
        }
        self.add("K");
        self.pos += if self.is(p + 1, b'G') { 2 } else { 1 };
    }

    fn handle_j(&mut self) {
        let p = self.cur();
        // Spanish "Jose", "San Jacinto".
        if self.eq_at(p, 4, &[b"JOSE"]) || self.eq_at(0, 4, &[b"SAN "]) {
            if (self.pos == 0 && self.is(p + 4, b' ')) || self.eq_at(0, 4, &[b"SAN "]) {
                self.add("H");
            } else {
                self.add_split("J", "H");
            }
            self.pos += 1;
            return;
        }
        if self.pos == 0 {
            self.add_split("J", "A");
        } else if self.vowel(p - 1) && !self.slavo_germanic && self.one_of(p + 1, b"AO") {
            // Spanish pronunciation of "bajador".
            self.add_split("J", "H");
        } else if self.at_last() {
            self.add_split("J", "");
        } else if !self.one_of(p + 1, b"LTKSNMBZ") && !self.one_of(p - 1, b"SKL") {
            self.add("J");
        }
        self.pos += if self.is(p + 1, b'J') { 2 } else { 1 };
    }

    fn handle_l(&mut self) {
        let p = self.cur();
        if self.is(p + 1, b'L') {
            let last = self.len as isize - 1;
            // Spanish "cabrillo", "gallegos".
            let spanish = (self.len >= 3
                && self.pos == self.len - 3
                && self.eq_at(p - 1, 4, &[b"ILLO", b"ILLA", b"ALLE"]))
                || ((self.eq_at(last - 1, 2, &[b"AS", b"OS"]) || self.one_of(last, b"AO"))
                    && self.eq_at(p - 1, 4, &[b"ALLE"]));
            if spanish {
                self.add_split("L", "");
                self.pos += 2;
                return;
            }
            self.pos += 2;
        } else {
            self.pos += 1;
        }
        self.add("L");
    }

    fn handle_m(&mut self) {
        let p = self.cur();
        // "-umb-" as in "dumb", "thumb", "number".
        let silent_b = self.eq_at(p - 1, 3, &[b"UMB"])
            && (self.pos + 1 == self.len.saturating_sub(1) || self.eq_at(p + 2, 2, &[b"ER"]));
        self.add("M");
        self.pos += if silent_b || self.is(p + 1, b'M') {
            2
        } else {
            1
        };
    }

    fn handle_r(&mut self) {
        let p = self.cur();
        // French "Rogier", but not "Hochmeier".
        if self.at_last()
            && !self.slavo_germanic
            && self.eq_at(p - 2, 2, &[b"IE"])
            && !self.eq_at(p - 4, 2, &[b"ME", b"MA"])
        {
            self.add_split("", "R");
        } else {
            self.add("R");
        }
        self.pos += if self.is(p + 1, b'R') { 2 } else { 1 };
    }

    fn handle_s(&mut self) {
        let p = self.cur();
        // "island", "isle", "Carlisle", "Carlysle".
        if self.eq_at(p - 1, 3, &[b"ISL", b"YSL"]) {
            self.pos += 1;
            return;
        }
        if self.pos == 0 && self.eq_at(p, 5, &[b"SUGAR"]) {
            self.add_split("X", "S");
            self.pos += 1;
            return;
        }
        if self.eq_at(p, 2, &[b"SH"]) {
            if self.eq_at(p + 1, 4, &[b"HEIM", b"HOEK", b"HOLM", b"HOLZ"]) {
                // Germanic.
                self.add("S");
            } else {
                self.add("X");
            }
            self.pos += 2;
            return;
        }
        // Italian and Armenian.
        if self.eq_at(p, 3, &[b"SIO", b"SIA"]) || self.eq_at(p, 4, &[b"SIAN"]) {
            if self.slavo_germanic {
                self.add("S");
            } else {
                self.add_split("S", "X");
            }
            self.pos += 3;
            return;
        }
        // German anglicisations: "Smith"/"Schmidt", "Snider"/"Schneider"; and
        // "-sz-" in Slavic spellings.
        if (self.pos == 0 && self.one_of(p + 1, b"MNLW")) || self.is(p + 1, b'Z') {
            self.add_split("S", "X");
            self.pos += if self.is(p + 1, b'Z') { 2 } else { 1 };
            return;
        }
        if self.eq_at(p, 2, &[b"SC"]) {
            // Schlesinger's rule.
            if self.is(p + 2, b'H') {
                if self.eq_at(p + 3, 2, &[b"OO", b"ER", b"EN", b"UY", b"ED", b"EM"]) {
                    // Dutch: "school", "schooner", "Schermerhorn", "Schenker".
                    if self.eq_at(p + 3, 2, &[b"ER", b"EN"]) {
                        self.add_split("X", "SK");
                    } else {
                        self.add("SK");
                    }
                } else if self.pos == 0 && !self.vowel(3) && !self.is(3, b'W') {
                    self.add_split("X", "S");
                } else {
                    self.add("X");
                }
                self.pos += 3;
                return;
            }
            if self.one_of(p + 2, b"IEY") {
                self.add("S");
            } else {
                self.add("SK");
            }
            self.pos += 3;
            return;
        }
        // French "Resnais", "Artois".
        if self.at_last() && self.eq_at(p - 2, 2, &[b"AI", b"OI"]) {
            self.add_split("", "S");
        } else {
            self.add("S");
        }
        self.pos += if self.one_of(p + 1, b"SZ") { 2 } else { 1 };
    }

    fn handle_t(&mut self) {
        let p = self.cur();
        if self.eq_at(p, 4, &[b"TION"]) {
            self.add("X");
            self.pos += 3;
            return;
        }
        if self.eq_at(p, 3, &[b"TIA", b"TCH"]) {
            self.add("X");
            self.pos += 3;
            return;
        }
        if self.eq_at(p, 2, &[b"TH"]) || self.eq_at(p, 3, &[b"TTH"]) {
            // "Thomas", "Thames", or Germanic.
            if self.eq_at(p + 2, 2, &[b"OM", b"AM"])
                || self.eq_at(0, 4, &[b"VAN ", b"VON "])
                || self.eq_at(0, 3, &[b"SCH"])
            {
                self.add("T");
            } else {
                self.add_split("0", "T");
            }
            self.pos += 2;
            return;
        }
        self.add("T");
        self.pos += if self.one_of(p + 1, b"TD") { 2 } else { 1 };
    }

    fn handle_w(&mut self) {
        let p = self.cur();
        if self.eq_at(p, 2, &[b"WR"]) {
            self.add("R");
            self.pos += 2;
            return;
        }
        if self.pos == 0 && (self.vowel(p + 1) || self.eq_at(p, 2, &[b"WH"])) {
            if self.vowel(p + 1) {
                // "Wasserman" should match "Vasserman".
                self.add_split("A", "F");
            } else {
                // "Uomo" should match "Womo".
                self.add("A");
            }
        }
        // "Arnow" should match "Arnoff".
        if (self.at_last() && self.vowel(p - 1))
            || self.eq_at(p - 1, 5, &[b"EWSKI", b"EWSKY", b"OWSKI", b"OWSKY"])
            || self.eq_at(0, 3, &[b"SCH"])
        {
            self.add_split("", "F");
            self.pos += 1;
            return;
        }
        // Polish "Filipowicz".
        if self.eq_at(p, 4, &[b"WICZ", b"WITZ"]) {
            self.add_split("TS", "FX");
            self.pos += 4;
            return;
        }
        self.pos += 1;
    }

    fn handle_x(&mut self) {
        let p = self.cur();
        // French "Breaux": the X is silent at the end after these endings.
        let silent = self.at_last()
            && (self.eq_at(p - 3, 3, &[b"IAU", b"EAU"]) || self.eq_at(p - 2, 2, &[b"AU", b"OU"]));
        if !silent {
            self.add("KS");
        }
        self.pos += if self.one_of(p + 1, b"CX") { 2 } else { 1 };
    }

    fn handle_z(&mut self) {
        let p = self.cur();
        // Chinese pinyin "Zhao".
        if self.is(p + 1, b'H') {
            self.add("J");
            self.pos += 2;
            return;
        }
        if self.eq_at(p + 1, 2, &[b"ZO", b"ZI", b"ZA"])
            || (self.slavo_germanic && self.pos > 0 && !self.is(p - 1, b'T'))
        {
            self.add_split("S", "TS");
        } else {
            self.add("S");
        }
        self.pos += if self.is(p + 1, b'Z') { 2 } else { 1 };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use verbora_core::DoubleKeyPhonetic;

    fn dm() -> DoubleMetaphone {
        DoubleMetaphone::new()
    }

    fn keys(word: &str) -> (String, Option<String>) {
        dm().process(word).into_parts()
    }

    /// The worked examples Philips gives in the *C/C++ Users Journal* article
    /// as the motivating cases for a second key: two spellings of the same
    /// name whose primary keys differ and whose key **sets** intersect.
    #[test]
    fn philips_motivating_examples() {
        assert_eq!(keys("Smith"), ("SM0".into(), Some("XMT".into())));
        assert_eq!(keys("Schmidt"), ("XMT".into(), Some("SMT".into())));
        assert!(dm().compare("Smith", "Schmidt"));

        // Richard: English "RXRT", Germanic "RKRT".
        assert_eq!(keys("Richard"), ("RXRT".into(), Some("RKRT".into())));

        // The word-initial silent clusters.
        assert_eq!(keys("Knight"), ("NT".into(), None));
        assert_eq!(keys("Wright"), ("RT".into(), None));
        assert_eq!(keys("Gnome"), ("NM".into(), None));
        assert_eq!(keys("Psalm"), ("SLM".into(), None));

        // Initial X is the Chinese "Xu", pronounced S.
        assert_eq!(keys("Xu"), ("S".into(), None));
    }

    /// The alternate key is `None` exactly when the algorithm never forked,
    /// and `Some` exactly when it did — the distinction the old `(String,
    /// String)` trait shape could not express, because it duplicated the
    /// primary into the second slot as a sentinel for "absent".
    #[test]
    fn the_alternate_is_absent_rather_than_duplicated() {
        // No branch condition fires anywhere in "Thompson".
        let code = dm().process("Thompson");
        assert_eq!(code.alternate(), None);
        assert_eq!(code.primary(), "TMPS");

        // "Smith" forks at S (Germanic anglicisation) and at TH.
        assert_eq!(dm().process("Smith").alternate(), Some("XMT"));

        // The `DoubleKeyPhonetic` tuple says the same thing the struct does.
        assert_eq!(dm().process_double("Thompson"), ("TMPS".to_owned(), None));
        assert_eq!(
            dm().process_double("Smith"),
            ("SM0".to_owned(), Some("XMT".to_owned()))
        );
    }

    /// Both keys are capped at four characters, and the loop stops as soon as
    /// both have reached the cap.
    #[test]
    fn keys_are_capped_at_four_characters() {
        for word in [
            "supercalifragilisticexpialidocious",
            "Rosochowaciec",
            &"strzelczyk".repeat(20),
        ] {
            let code = dm().process(word);
            assert!(code.primary().len() <= 4, "{word:?} -> {code:?}");
            assert!(
                code.alternate().is_none_or(|a| a.len() <= 4),
                "{word:?} -> {code:?}"
            );
        }
    }

    /// The space is a character the rules read, not tokenizer residue: the
    /// `SAN ` and `VAN `/`VON ` clauses are unreachable without it, and
    /// `Mac Caffrey`'s three-character skip needs it too.
    #[test]
    fn the_space_is_load_bearing() {
        // The `SAN ` clause makes the J of "Jose"/"Jacinto" an unforked H.
        // Without the space the prefix is "SANJ", the clause cannot fire, and
        // the name forks instead: J primary, H alternate.
        assert_eq!(keys("San Jose"), ("SNHS".into(), None));
        assert_eq!(keys("SanJose"), ("SNJS".into(), Some("SNHS".into())));
        assert!(dm().process("San Jacinto").primary().starts_with("SNH"));

        // Everything that is not a letter and not whitespace is skipped
        // without leaving a gap.
        assert_eq!(keys("O'Brien"), keys("OBrien"));
        assert_eq!(keys("Jean-Luc"), keys("JeanLuc"));
        // ... which is exactly what makes the space the *only* non-letter the
        // rules can see.
        assert_eq!(keys("San\u{a0}Jose"), keys("San Jose")); // NBSP is whitespace
        assert_eq!(keys("San.Jose"), keys("SanJose")); // a full stop is not
    }

    /// One witness per branch-producing rule, so a rule that stops forking is
    /// caught rather than silently collapsing to a single key.
    #[test]
    fn each_forking_rule_has_a_witness() {
        // -CHAE- as in "Michael": K primary, X alternate.
        assert_eq!(keys("Michael"), ("MKL".into(), Some("MXL".into())));
        // ...and the "-ACH-" exception beside it, which does *not* fork:
        // "Bacher" is named in the rule itself.
        assert_eq!(keys("Bacher"), ("PKR".into(), None));
        // "Jose" alone is the Spanish H, with no fork.
        assert_eq!(keys("Jose"), ("HS".into(), None));
        // CZ: S primary, X alternate.
        assert_eq!(dm().process("Czerny").alternate(), Some("XRN"));
        // GN after an initial vowel: KN primary, N alternate.
        assert_eq!(dm().process("Agnes").alternate(), Some("ANS"));
        // -WICZ: TS primary, FX alternate.
        assert_eq!(dm().process("Filipowicz").alternate(), Some("FLPF"));
        // ZH is the pinyin J.
        assert_eq!(dm().process("Zhao").primary(), "J");
    }

    /// The text unit, enumerated over one scalar of every class.
    #[test]
    fn only_ascii_letters_and_spaces_are_read() {
        for empty in ["", "...", "1234", "日本語", "😀", "Москва", "\u{301}"] {
            let code = dm().process(empty);
            assert_eq!(code.primary(), "", "for {empty:?}");
            assert_eq!(code.alternate(), None, "for {empty:?}");
        }
        assert_eq!(keys("caf\u{e9}"), keys("caf"));
        assert_eq!(keys("na\u{ef}ve"), keys("nave"));
        assert_eq!(keys("Sm\u{1F600}ith"), keys("Smith"));
        assert_eq!(keys("SMITH"), keys("smith"));
    }

    #[test]
    fn compare_matches_on_either_key() {
        let dm = dm();
        assert!(dm.compare("Smith", "Schmidt"));
        assert!(dm.compare("Smith", "Smith"));
        assert!(!dm.compare("Smith", "Jones"));
        // Two names with no letters share the empty key.
        assert!(dm.compare("", "日本語"));
    }

    #[test]
    fn code_accessors_agree_with_each_other() {
        let code = dm().process("Smith");
        assert!(code.contains("SM0"));
        assert!(code.contains("XMT"));
        assert!(!code.contains("TMPS"));
        assert!(code.shares_key_with(&dm().process("Schmidt")));
        assert!(!code.shares_key_with(&dm().process("Jones")));
        assert_eq!(code.to_string(), "SM0");
    }
}
