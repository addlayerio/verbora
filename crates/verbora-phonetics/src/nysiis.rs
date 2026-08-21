//! NYSIIS (Taft, 1970).

/// The fielded NYSIIS key length. Taft's rules themselves produce a key of
/// whatever length the name needs; the New York State system stored six
/// characters, and that is what [`Nysiis::new`] returns.
/// [`Nysiis::with_strict`]`(false)` returns the untruncated key.
const MAX_STRICT_LEN: usize = 6;

/// NYSIIS — the New York State Identification and Intelligence System code.
///
/// # Publication
///
/// Robert L. Taft, *Name Search Techniques*, New York State Identification and
/// Intelligence System, Special Report No. 1, Albany, 1970. Taft designed it as
/// a higher-accuracy replacement for Soundex over American surnames, and the
/// rule sequence below is his.
///
/// # The contract
///
/// * **The text unit is one Unicode scalar**, and only the twenty-six letters
///   `A`–`Z` are read, after simple ASCII case folding. Every other scalar is
///   skipped. Taft's rules are stated over the Roman alphabet and assign no
///   class to `é`, `ß` or `語`: carrying such a character through to the code
///   would put a letter in the key that no rule mentions, and would make the
///   key's length depend on the input's script.
/// * The code is ASCII uppercase letters only, so a strict truncation is a
///   character truncation and a byte truncation at once.
/// * A token with no `A`–`Z` letter encodes to `""`. So can a token that has
///   one: Taft's tail trims cascade, and `"AZ"` trims to nothing. The empty
///   code is therefore genuinely "this name has no NYSIIS key", not a
///   sentinel.
/// * **Total**: no input panics, and there is no error type.
///
/// # The rules, in order
///
/// 1. **Prefix rewrites**, applied to the *evolving* string:
///    `MAC`→`MCC`, `KN`→`NN`, `K`→`C`, `PH`/`PF`→`FF`, `SCH`→`SSS`.
/// 2. **Suffix rewrites**: `EE`/`IE`→`Y`, then `DT`/`RT`/`RD`/`NT`/`ND`→`D`.
/// 3. **The first character is retained verbatim** and never transcoded,
///    which is why `"AEIOU"` encodes to `"A"` and `"Um"` to `"UN"`.
/// 4. **Rolling transcode** from the second character on, writing its output
///    *back into the buffer* so a multi-character output becomes the next
///    iteration's input: `EV`→`AF`; vowels (`AEIOU`)→`A`; `Q`→`G`; `Z`→`S`;
///    `M`→`N`; `KN`→`NN`; `K`→`C`; `SCH`→`SSS`; `PH`→`FF`; `H` becomes the
///    previous character unless both its neighbours are vowels; `W` after a
///    vowel becomes that vowel. A transcoded character equal to its
///    (transcoded) predecessor is not appended to the key.
/// 5. **Tail trims**, only when the key is longer than one character: drop a
///    trailing `S`; rewrite a trailing `AY` to `Y` (only when longer than
///    two); drop a trailing `A`.
/// 6. **Strict truncation**: when [`Nysiis::is_strict`] — the default — the
///    key is cut to at most 6 characters. [`Nysiis::with_strict`]`(false)`
///    returns the untruncated key.
///
/// ```
/// use verbora_phonetics::Nysiis;
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
    /// The strict encoder — six-character keys, as fielded.
    ///
    /// ```
    /// use verbora_phonetics::Nysiis;
    ///
    /// assert_eq!(Nysiis::default(), Nysiis::new());
    /// assert!(Nysiis::default().is_strict());
    /// ```
    fn default() -> Self {
        Self::new()
    }
}

/// The vowel class every NYSIIS rule uses: `AEIOU`.
#[inline]
const fn is_ascii_vowel(b: u8) -> bool {
    matches!(b, b'A' | b'E' | b'I' | b'O' | b'U')
}

impl Nysiis {
    /// Creates a strict NYSIIS encoder: keys truncated to the fielded six
    /// characters. This is the default.
    ///
    /// ```
    /// use verbora_phonetics::Nysiis;
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
    /// full length.
    ///
    /// ```
    /// use verbora_phonetics::Nysiis;
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
    /// use verbora_phonetics::Nysiis;
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
    /// Scalars outside `A`–`Z` are skipped; see the [type
    /// documentation](Self) for the rules and the text unit.
    ///
    /// ```
    /// use verbora_phonetics::Nysiis;
    ///
    /// let nysiis = Nysiis::new();
    /// assert_eq!(nysiis.process("MACINTOSH"), "MCANT");
    /// assert_eq!(nysiis.process("KNUTH"), "NAT");
    /// assert_eq!(nysiis.process("o'daniel"), "ODANAL");
    /// assert_eq!(nysiis.process("12345"), "");
    /// ```
    #[must_use]
    pub fn process(&self, token: &str) -> String {
        self.encode(token)
    }

    /// Whether two strings share a NYSIIS code (at this encoder's strictness).
    ///
    /// ```
    /// use verbora_phonetics::Nysiis;
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
    fn encode(&self, token: &str) -> String {
        let mut buf: Vec<u8> = Vec::with_capacity(token.len());
        buf.extend(crate::letters::Letters::new(token));
        if buf.is_empty() {
            return String::new();
        }

        // Prefix rewrites, in Taft's order, each testing the buffer as
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
        // but they are tested sequentially, as two independent rules.
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

        // Tail trims, gated on a key longer than one character: a
        // one-character key is returned untouched, even if it is "A" or "S".
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
}

/// One step of the rolling transcode, writing its output back into `buf` at
/// `i..`, so a multi-character output becomes the next iteration's input.
/// Multi-character outputs never overrun: each one is guarded by the
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

    // -- an independent transcription of Taft's rules, used as the oracle --

    /// A deliberately naive second transcription of Taft's rules, over
    /// `Vec<char>` with the key accumulated in a separate `String`.
    ///
    /// [`Nysiis::encode`] compacts the key into the *same* buffer its
    /// transcode loop reads, relying on the invariant that the write position
    /// never exceeds the read position. That is the kind of optimisation that
    /// is right until it is not — and it is also the kind that makes a
    /// fixture unfalsifiable, since an expected value read off that code
    /// would pin the code rather than Taft's rules.
    ///
    /// This mirror allocates a second buffer and therefore cannot have the
    /// aliasing bug, and it is written from the numbered rule list in
    /// [`Nysiis`]'s own documentation rather than from `encode`. **Every**
    /// fixture below goes through [`encoded`], which asserts the two agree,
    /// so no expected value in this module is true of `encode` alone.
    fn reference_encode(strict: bool, token: &str) -> String {
        let mut chars: Vec<char> = crate::letters::Letters::new(token)
            .map(char::from)
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
            reference_transcode(&mut chars, i);
            if chars[i - 1] != chars[i] {
                key.push(chars[i]);
            }
        }

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

        if strict {
            key.truncate(MAX_STRICT_LEN);
        }
        key
    }

    fn reference_vowel(c: char) -> bool {
        matches!(c, 'A' | 'E' | 'I' | 'O' | 'U')
    }

    /// One rolling-transcode step for [`reference_encode`].
    fn reference_transcode(chars: &mut [char], i: usize) {
        let prev = chars[i - 1];
        let cur = chars[i];
        let next = chars.get(i + 1).copied();
        let next2 = chars.get(i + 2).copied();

        if cur == 'E' && next == Some('V') {
            chars[i] = 'A';
            chars[i + 1] = 'F';
            return;
        }
        if reference_vowel(cur) {
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
        if (cur == 'H' && (!reference_vowel(prev) || !next.is_some_and(reference_vowel)))
            || (cur == 'W' && reference_vowel(prev))
        {
            chars[i] = prev;
        }
    }
    use super::*;

    /// Encodes with [`Nysiis`] *and* with [`reference_encode`], asserting the
    /// two agree before returning the key. Every fixture below is routed
    /// through here.
    fn encoded(strict: bool, token: &str) -> String {
        let code = Nysiis::with_strict(strict).process(token);
        assert_eq!(
            code,
            reference_encode(strict, token),
            "the transcribed rule list disagrees on {token:?} (strict={strict})"
        );
        code
    }

    /// Asserts every value encodes to `expected` under the strict default,
    /// over a whole fixture list at once.
    fn strict_all(values: &[&str], expected: &str) {
        for v in values {
            assert_eq!(encoded(true, v), expected, "strict encoding of {v:?}");
        }
    }

    /// Asserts each pair under the non-strict encoder.
    fn full(pairs: &[(&str, &str)]) {
        for (value, expected) in pairs {
            assert_eq!(
                encoded(false, value),
                *expected,
                "non-strict encoding of {value:?}"
            );
        }
    }

    // ------------------------------------------------------------------
    // Fixtures. The inputs are the American surnames NYSIIS is customarily
    // exercised on, plus one `X`-prefixed probe per numbered rule -- `X` is
    // in no rule's letter class, so it survives to the key untouched and
    // isolates the rule under test. The expected values are derived: each is
    // checked against the transcribed rule list on every run, and the ones
    // worth spelling out carry the derivation in a comment.
    // ------------------------------------------------------------------

    /// Three spellings that collide on `BRAN`.
    ///
    /// `Brian`: no prefix or suffix rule matches. `B` is retained. `R` is in
    /// no rolling rule's class, so it stays `R` and, differing from `B`, is
    /// appended. `I` is a vowel -> `A`, appended. `A` -> `A`, equal to the
    /// `A` before it, dropped. `N` stays, appended. Key `BRAN`; the tail
    /// trims want a trailing `S`, `AY` or `A`, and it ends in `N`.
    #[test]
    fn names_colliding_on_bran() {
        strict_all(&["Brian", "Brown", "Brun"], "BRAN");
    }

    /// Four spellings that collide on `CAP` — including `Kipp`, which the
    /// prefix rule `K`->`C` brings into the group.
    #[test]
    fn names_colliding_on_cap() {
        strict_all(&["Capp", "Cope", "Copp", "Kipp"], "CAP");
    }

    /// `Dent`: rule 2 rewrites the trailing `NT` to `D`, giving `DED`; the
    /// rolling transcode then turns the `E` into `A` -> `DAD`.
    #[test]
    fn name_colliding_on_dad() {
        strict_all(&["Dent"], "DAD");
    }

    /// Three spellings that collide on `DAN`.
    #[test]
    fn names_colliding_on_dan() {
        strict_all(&["Dane", "Dean", "Dionne"], "DAN");
    }

    /// `Phil`: the prefix rule `PH`->`FF` gives `FFIL`; the second `F` equals
    /// the retained first character and is dropped; `I` -> `A`; `L` stays.
    #[test]
    fn name_colliding_on_fal() {
        strict_all(&["Phil"], "FAL");
    }

    /// Twenty-seven surnames end to end, exercising every rule in
    /// combination. Three worked derivations:
    ///
    /// * `MACINTOSH`: rule 1 rewrites `MAC`->`MCC` -> `MCCINTOSH`. `M` is
    ///   retained; `C` appended; the second `C` equals it and is dropped;
    ///   `I`->`A`; `N`; `T`; `O`->`A`; `S`; the final `H` has a consonant
    ///   before it so it becomes that consonant, `S`, and is dropped as a
    ///   duplicate. Key `MCANTAS`; the tail trims drop the `S`, then the `A`
    ///   -> `MCANT`.
    /// * `KNUTH`: rule 1 rewrites `KN`->`NN`; the second `N` equals the
    ///   retained first character and is dropped; `U`->`A`; `T`; the final
    ///   `H` becomes the `T` before it and is dropped -> `NAT`.
    /// * `CARRAWAY`: `C` retained; `A`; `R`; the second `R` dropped; `A`;
    ///   `W` after a vowel becomes that vowel and is dropped; `A` dropped
    ///   likewise; `Y` appended. Key `CARAY` -> the `AY` trim gives `CARY`.
    #[test]
    fn surnames_end_to_end() {
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

    /// Apostrophes and mixed case are invisible: `O'Daniel` presents the
    /// same letters as `ODaniel`. `Cory`/`Corey`/`Kory` collide because the
    /// prefix rule `K`->`C` and the vowel rule between them erase the only
    /// differences.
    #[test]
    fn punctuation_and_prefix_collisions() {
        full(&[
            ("O'Daniel", "ODANAL"),
            ("O'Donnel", "ODANAL"),
            ("Cory", "CARY"),
            ("Corey", "CARY"),
            ("Kory", "CARY"),
            ("FUZZY", "FASY"),
        ]);
    }

    /// Rule 1, the five prefix rewrites, one probe each. `X` is in no
    /// rule's class, so what survives after it is exactly the prefix's
    /// output with its duplicate second character dropped: `MACX`->`MCCX`
    /// gives `MCX`, `KNX`->`NNX` gives `NX`, `KX`->`CX`, `PHX`/`PFX`->`FFX`
    /// give `FX`, `SCHX`->`SSSX` gives `SX`.
    #[test]
    fn rule_1_prefix_rewrites() {
        full(&[
            ("MACX", "MCX"),
            ("KNX", "NX"),
            ("KX", "CX"),
            ("PHX", "FX"),
            ("PFX", "FX"),
            ("SCHX", "SX"),
        ]);
    }

    /// Rule 2, the seven suffix rewrites: `EE` and `IE` become `Y`, and
    /// `DT`, `RT`, `RD`, `NT`, `ND` all become `D`.
    #[test]
    fn rule_2_suffix_rewrites() {
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

    /// Rule 4's first clause: `EV` becomes `AF`, and each of the five
    /// vowels becomes `A`.
    #[test]
    fn rule_4_vowels_and_ev() {
        full(&[
            ("XEV", "XAF"),
            ("XAX", "XAX"),
            ("XEX", "XAX"),
            ("XIX", "XAX"),
            ("XOX", "XAX"),
            ("XUX", "XAX"),
        ]);
    }

    /// Rule 4's single-letter substitutions: `Q`->`G`, `Z`->`S`, `M`->`N`.
    /// `XZ` shows both the substitution and rule 5 in one step — the `Z`
    /// becomes `S`, which the trailing-`S` trim then removes.
    #[test]
    fn rule_4_single_letter_substitutions() {
        full(&[("XQ", "XG"), ("XZ", "X"), ("XM", "XN")]);
    }

    /// Rule 5: a trailing `S` is dropped. It fires once, not repeatedly —
    /// `XSS` reaches the trim as the key `XS`, because the second `S`
    /// duplicated the first and never entered the key.
    #[test]
    fn rule_5_trailing_s() {
        full(&[("XS", "X"), ("XSS", "X")]);
    }

    /// Rule 6: a trailing `AY` becomes `Y`. `XAYS` shows the order — rule 5
    /// removes the `S` first, exposing the `AY` that rule 6 then rewrites.
    #[test]
    fn rule_6_trailing_ay() {
        full(&[("XAY", "XY"), ("XAYS", "XY")]);
    }

    /// Rule 7: a trailing `A` is dropped, again after rule 5 has removed a
    /// trailing `S`.
    #[test]
    fn rule_7_trailing_a() {
        full(&[("XA", "X"), ("XAS", "X")]);
    }

    /// `Schmidt`: rule 1 rewrites `SCH`->`SSS`, whose second and third `S`
    /// collapse into the retained first character; `M`->`N`; `I`->`A`; and
    /// rule 2 has already rewritten the trailing `DT` to `D`.
    #[test]
    fn name_colliding_on_snad() {
        strict_all(&["Schmidt"], "SNAD");
    }

    /// `Smith` and `Schmit` collide: `SCH`->`SSS` collapses to one `S`, so
    /// both present `S`, `M`->`N`, `I`->`A`, `T`, and a final `H` that
    /// becomes the `T` before it and is dropped.
    #[test]
    fn names_colliding_on_snat() {
        strict_all(&["Smith", "Schmit"], "SNAT");
    }

    /// The rolling transcode's remaining branches, one name each: `W` after
    /// a vowel (`Kobwick`), `H` after a consonant (`Kocher`), an `SC` that is
    /// not `SCH` and so is left alone (`Fesca`), `M`->`N` mid-word (`Shom`),
    /// `H` between two vowels surviving as `H` (`Ohlo`, `Uhu`), and the
    /// retained first character never being transcoded (`Um` -> `UN`, not
    /// `AN`).
    #[test]
    fn rolling_transcode_branches() {
        strict_all(&["Kobwick"], "CABWAC");
        strict_all(&["Kocher"], "CACAR");
        strict_all(&["Fesca"], "FASC");
        strict_all(&["Shom"], "SAN");
        strict_all(&["Ohlo"], "OL");
        strict_all(&["Uhu"], "UH");
        strict_all(&["Um"], "UN");
    }

    /// `Trueman` and `Truman` collide: the `E` becomes `A` and then
    /// duplicates the `A` before it, so it never enters the key.
    #[test]
    fn names_colliding_on_tranan() {
        strict_all(&["Trueman", "Truman"], "TRANAN");
    }

    /// The strict cut is at most six characters, on a name whose full key is
    /// nine (`WASTARLAD`).
    #[test]
    fn strict_keys_never_exceed_six_characters() {
        let result = encoded(true, "WESTERLUND");
        assert!(result.len() <= 6);
        assert_eq!(result, "WASTAR");
    }

    // ------------------------------------------------------------------
    // Hand-written edge cases.
    // ------------------------------------------------------------------

    #[test]
    fn empty_and_letterless_inputs_encode_to_empty() {
        for strict in [true, false] {
            for input in ["", "12345", "!!!", "   ", "'", "-_-", "😀", "😀🎉"] {
                assert_eq!(encoded(strict, input), "", "for {input:?}");
            }
        }
    }

    #[test]
    fn single_letters() {
        // The first character is retained verbatim, so no rolling rule ever
        // applies to it — but the K -> C *prefix* rule does.
        assert_eq!(encoded(true, "a"), "A");
        assert_eq!(encoded(true, "K"), "C");
        assert_eq!(encoded(true, "M"), "M");
        assert_eq!(encoded(true, "Z"), "Z");
        // One-byte keys skip the tail trims entirely.
        assert_eq!(encoded(true, "S"), "S");
        assert_eq!(encoded(true, "A"), "A");
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
        assert_eq!(encoded(true, "AZ"), "");
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
        assert_eq!(encoded(true, "K1N2"), encoded(true, "KN"));
        assert_eq!(encoded(true, "mac intosh"), "MCANT");
        assert_eq!(encoded(true, "Mac-Intosh"), "MCANT");
        assert_eq!(encoded(true, "K9N"), "N");
    }

    #[test]
    fn mixed_case_is_normalized() {
        assert_eq!(encoded(true, "wEsTeRlUnD"), "WASTAR");
        assert_eq!(encoded(true, "MacIntosh"), encoded(true, "MACINTOSH"));
        assert_eq!(encoded(true, "knuth"), "NAT");
    }

    /// The text unit, enumerated over one scalar of every class. A scalar
    /// outside `A`-`Z` is skipped -- it neither codes nor separates -- so an
    /// accented name codes exactly as its unaccented-letters-only spelling
    /// and every key is pure ASCII.
    #[test]
    fn only_ascii_letters_are_read() {
        for input in [
            "",
            " ",
            "12345",
            "...",
            "\u{65e5}\u{672c}\u{8a9e}",
            "\u{1F600}",
            "\u{301}",
        ] {
            assert_eq!(encoded(true, input), "", "for {input:?}");
        }
        assert_eq!(encoded(true, "caf\u{e9}"), encoded(true, "caf"));
        assert_eq!(encoded(true, "M\u{fc}ller"), encoded(true, "Mller"));
        assert_eq!(encoded(true, "stra\u{df}e"), encoded(true, "strae"));
        assert_eq!(encoded(true, "a\u{1F600}b"), "AB");
        assert_eq!(encoded(true, "O'Daniel"), encoded(true, "ODaniel"));
        assert_eq!(encoded(true, "mac intosh"), encoded(true, "macintosh"));
        // Every key is ASCII, so a strict cut is a character cut.
        for input in [
            "caf\u{e9}",
            "BCDFG\u{c9}X",
            "\u{65e5}\u{672c}\u{8a9e}",
            "Westerlund",
        ] {
            assert!(encoded(true, input).is_ascii(), "for {input:?}");
            assert!(encoded(true, input).chars().count() <= MAX_STRICT_LEN);
        }
    }

    /// Strict truncation cuts at six characters, and non-strict never cuts.
    /// Both are exercised on a key longer than six, the only place the flag
    /// is observable.
    #[test]
    fn strict_truncation_cuts_at_six_characters() {
        assert_eq!(encoded(false, "BCDFGX"), "BCDFGX");
        assert_eq!(encoded(true, "Westerlund"), "WASTAR");
        assert_eq!(encoded(false, "Westerlund"), "WASTARLAD");
    }

    #[test]
    fn very_long_inputs() {
        assert_eq!(encoded(false, &"a".repeat(500)), "A");
        let long = "ab".repeat(250);
        let code = encoded(false, &long);
        assert_eq!(code.len(), 500);
        assert!(code.starts_with("ABAB"));
        assert_eq!(encoded(true, &long), "ABABAB");
    }

    #[test]
    fn strict_is_a_pure_truncation_of_non_strict() {
        for input in [
            "MACINTOSH",
            "WESTERLUND",
            "PHILLIPSON",
            "SCHOENHOEFT",
            "CASSTEVENS",
            "supercalifragilisticexpialidocious",
        ] {
            let code = encoded(false, input);
            let want = &code[..code.len().min(6)];
            assert_eq!(encoded(true, input), want, "for {input:?}");
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
                    nysiis.encode(input),
                    reference_encode(nysiis.is_strict(), input),
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
                        nysiis.encode(s),
                        reference_encode(nysiis.is_strict(), s),
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

    /// Prefix, `EV` and `AY` chains beyond the shapes the surname fixtures
    /// reach: repeated prefixes whose rewritten output immediately re-feeds
    /// the rolling transcode, and the `AY` trim interacting with the `S`
    /// trim. No publication names these inputs, but their expected keys are
    /// still derived rather than recorded — `encoded` checks every one
    /// against the transcribed rule list.
    #[test]
    fn prefix_and_write_back_chains() {
        let cases: &[(&str, &str, &str)] = &[
            // (input, strict, non-strict).
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
            assert_eq!(encoded(true, input), strict, "strict {input:?}");
            assert_eq!(encoded(false, input), full, "non-strict {input:?}");
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
        assert_eq!(encoded(false, "WESTERLUND"), "WASTARLAD");
        assert_eq!(encoded(false, "WESTERLAND"), "WASTARLAD");
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
        assert_eq!(code, encoded(true, "KNUTH"));
        assert_eq!(code, "NAT");
        assert!(eq);
    }
}
