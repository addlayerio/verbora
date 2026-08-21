//! Phonex (Lait and Randell, 1996).

/// Phonex — a Soundex refinement for British surnames.
///
/// # Publication
///
/// A. J. Lait and Brian Randell, "An Assessment of Name Matching Algorithms",
/// Technical Report Series, University of Newcastle upon Tyne Department of
/// Computing Science, 1996. Phonex adds a preprocessing stage (trailing-`S`
/// removal, leading-pair and leading-letter substitutions) and
/// context-sensitive digit rules to Soundex, tuned to reduce false negatives
/// on British surname data.
///
/// The paper fixes the preprocessing, the digit table and the three context
/// rules; it does not settle every detail of how repeated digits are
/// suppressed. Verbora specifies that below, in
/// [The algorithm](#the-algorithm)'s step 2, rather than leaving it to be
/// read off the code — the two fixtures that turn on it (`Ghosh` and a run
/// of same-class letters) are pinned in this module's tests.
///
/// # The contract
///
/// * **The text unit is one Unicode scalar**, and only the twenty-six letters
///   `A`–`Z` are read, after simple ASCII case folding. Every other scalar is
///   skipped. Phonex, like the Soundex it refines, is stated over the Roman
///   alphabet.
/// * Because every key character is ASCII, the configured maximum length is a
///   character count and a byte count at once — there is no input for which
///   they differ.
/// * A token with no `A`–`Z` letter — and a token whose letters are all
///   trailing `S`, which the preprocessing removes — encodes to all-`0`
///   padding at the configured length.
/// * **Total**: no input panics, and there is no error type.
///
/// # The algorithm
///
/// 1. **Preprocess** the letter sequence:
///    * remove every trailing `S` (`JONES` → `JONE`, `SSS` → empty);
///    * rewrite a leading pair by replacing only its **first** letter:
///      `KN…` → `NN…`, `PH…` → `FH…`, `WR…` → `RR…`;
///    * remove one leading `H` (`HARRINGTON` → `ARRINGTON`; a second `H` is
///      not removed: `HHART` → `HART`);
///    * substitute the (new) first letter: `E I O U Y` → `A`, `P` → `B`,
///      `V` → `F`, `K Q` → `C`, `J` → `G`, `Z` → `S`.
/// 2. **Transcode**: emit the first preprocessed character verbatim, then walk
///    the rest against the digit table (`BPFV`→1, `CSKGJQXZ`→2, `DT`→3,
///    `L`→4, `MN`→5, `R`→6, other→0) with three context rules: `D`/`T` before
///    `C` is silent; `L` and `R` code only before a vowel (`AEIOUY`, so `Y`
///    counts) or at word end; `M`/`N` swallow a following `D` or `G`. A
///    silenced letter emits nothing *and leaves the carried digit as it was*,
///    so the next letter is compared against the digit before it.
///
///    `0` is never emitted. Any other digit is emitted unless it equals **the
///    character the code most recently gained** — the head letter included.
///    Three consequences follow, and each is pinned by a test:
///    * a digit right after a non-emitting step is always emitted, because
///      the comparison falls back to a *letter* and no digit equals a letter
///      (`Ghosh` → `G200`, not `G000`);
///    * a run of same-class letters therefore emits on its third member
///      (`ssssb` → `S210`);
///    * the head letter already stands for its own digit, so that digit is
///      not emitted — except when the head triggered the `M`/`N` swallow,
///      which has consumed a letter, so the digit is emitted after all
///      (`Ng` → `N500`, where `Na` → `N000`).
/// 3. **Pad** with `0` to the configured length (default 4).
///
/// The only configuration is that maximum length
/// ([`Phonex::with_max_code_length`]); [`Phonex::new`] and
/// [`Phonex::default`] use the conventional 4.
///
/// ```
/// use verbora_phonetics::Phonex;
///
/// let phonex = Phonex::new();
/// assert_eq!(phonex.process("KNUTH"), "N300");
/// assert_eq!(phonex.process("Wright"), "R623");
/// assert!(phonex.compare("Schmidt", "Schmit"));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Phonex {
    max_code_length: usize,
}

/// The conventional Phonex code length: one letter plus three digits.
const DEFAULT_MAX_CODE_LENGTH: usize = 4;

impl Phonex {
    /// Creates a Phonex encoder with the conventional maximum code length
    /// of 4.
    ///
    /// ```
    /// use verbora_phonetics::Phonex;
    ///
    /// assert_eq!(Phonex::new().process("Sinatra"), "S536");
    /// ```
    #[must_use]
    pub const fn new() -> Self {
        Self {
            max_code_length: DEFAULT_MAX_CODE_LENGTH,
        }
    }

    /// Creates a Phonex encoder with a custom maximum code length, mirroring
    /// the code length, in characters.
    ///
    /// The length is measured in **bytes** and codes shorter than it are
    /// zero-padded up to it — see the type
    /// documentation for the byte-length quirks this implies on non-ASCII
    /// input.
    ///
    /// ```
    /// use verbora_phonetics::Phonex;
    ///
    /// assert_eq!(Phonex::with_max_code_length(6).process("Sinatra"), "S53600");
    /// assert_eq!(Phonex::with_max_code_length(2).process("Sinatra"), "S5");
    /// assert_eq!(Phonex::with_max_code_length(1).process("Sinatra"), "S");
    /// assert_eq!(Phonex::with_max_code_length(0).process("Sinatra"), "");
    /// ```
    #[must_use]
    pub const fn with_max_code_length(max_code_length: usize) -> Self {
        Self { max_code_length }
    }

    /// The maximum code length this encoder pads and truncates to, in bytes.
    ///
    /// ```
    /// use verbora_phonetics::Phonex;
    ///
    /// assert_eq!(Phonex::new().max_code_length(), 4);
    /// assert_eq!(Phonex::with_max_code_length(10).max_code_length(), 10);
    /// ```
    #[must_use]
    pub const fn max_code_length(&self) -> usize {
        self.max_code_length
    }

    /// Encodes `token`, returning its Phonex code.
    ///
    /// The code, at this encoder's configured length, for every `&str`
    /// input. Never panics; input that cleans to nothing (empty strings,
    /// digits, punctuation, emoji, all-`S` words) encodes to all zeros.
    ///
    /// ```
    /// use verbora_phonetics::Phonex;
    ///
    /// let phonex = Phonex::new();
    /// assert_eq!(phonex.process("Phonex"), "F520");
    /// assert_eq!(phonex.process("Ashcraft"), "A261");
    /// assert_eq!(phonex.process("Meyer-Lansky"), "M452");
    /// assert_eq!(phonex.process(""), "0000");
    /// assert_eq!(phonex.process("12345"), "0000");
    /// ```
    #[must_use]
    pub fn process(&self, token: &str) -> String {
        let mut rest = PreChars::new(token);
        let mut curr = rest.next();
        let mut next = rest.next();

        // The transcode loop, including the index bookkeeping the `M`/`N`
        // swallow rule needs: a swallow advances past the swallowed letter,
        // so an iteration that started as the first stops counting as one.
        // Only "is this the first iteration" is ever observable, which
        // `first_iter`/`treat_as_first` carry.
        let mut result = String::with_capacity(self.max_code_length.clamp(4, 16));
        let mut code = '0';
        let mut last = '0';
        let mut last_push = '0';
        let mut first_iter = true;

        while let Some(c) = curr {
            // Every key character is ASCII and the code grows one character
            // at a time, so equality here is the same test as `>=`.
            if result.len() == self.max_code_length {
                break;
            }

            // The first preprocessed character is always emitted verbatim.
            if first_iter {
                result.push(c);
                last_push = c;
            }

            let (new_code, skip_next) = transcode(c, next, next.is_none());
            if let Some(new_code) = new_code {
                code = new_code;
            }

            // The swallow advances past the D/G, so an iteration that
            // started as the first stops counting as it.
            let treat_as_first = first_iter && !skip_next;
            if skip_next {
                next = rest.next();
            }

            if last != code && code != '0' && !treat_as_first {
                result.push(code);
                last_push = code;
            }

            // `last` rewinds to the last *pushed* character — possibly the
            // head letter — after every iteration, except that the first
            // iteration records its own code instead.
            last = last_push;
            if treat_as_first {
                last = code;
            }

            curr = next;
            next = rest.next();
            first_iter = false;
        }

        // Pad to the configured length. The code is ASCII, so this is a
        // character count.
        while result.len() < self.max_code_length {
            result.push('0');
        }

        result
    }

    /// Whether two strings share a Phonex code at this encoder's length.
    ///
    /// Both inputs are encoded and the codes compared for equality.
    ///
    /// ```
    /// use verbora_phonetics::Phonex;
    ///
    /// let phonex = Phonex::new();
    /// assert!(phonex.compare("Knuth", "Nuth"));
    /// assert!(phonex.compare("Dalitz", "Duhlitz"));
    /// assert!(!phonex.compare("Wilson", "Worms"));
    /// ```
    #[must_use]
    pub fn compare(&self, a: &str, b: &str) -> bool {
        self.process(a) == self.process(b)
    }
}

impl Default for Phonex {
    /// Equivalent to [`Phonex::new`]: maximum code length 4, matching
    /// the conventional length of 4.
    fn default() -> Self {
        Self::new()
    }
}

impl verbora_core::Phonetic for Phonex {
    fn process(&self, token: &str) -> String {
        Self::process(self, token)
    }

    fn compare(&self, a: &str, b: &str) -> bool {
        Self::compare(self, a, b)
    }
}

/// One step of the digit table and the three context rules, from Lait and
/// Randell's Phonex description.
///
/// Returns the digit for `curr` (or `None` when a context rule silences it —
/// in which case the caller *keeps its previous code* — a carried-context rule
/// the encode loop depends on) and whether the following character must be
/// swallowed (`M`/`N` before `D`/`G`).
#[inline]
fn transcode(curr: char, next: Option<char>, is_last_char: bool) -> (Option<char>, bool) {
    match curr {
        'B' | 'P' | 'F' | 'V' => (Some('1'), false),
        'C' | 'S' | 'K' | 'G' | 'J' | 'Q' | 'X' | 'Z' => (Some('2'), false),
        'D' | 'T' => match next {
            Some('C') => (None, false),
            _ => (Some('3'), false),
        },
        'L' => {
            if is_vowel(next) || is_last_char {
                (Some('4'), false)
            } else {
                (None, false)
            }
        }
        'M' | 'N' => (Some('5'), matches!(next, Some('D') | Some('G'))),
        'R' => {
            if is_vowel(next) || is_last_char {
                (Some('6'), false)
            } else {
                (None, false)
            }
        }
        _ => (Some('0'), false),
    }
}

/// Vowel test for the `L`/`R` context rules: ASCII-lowercase the character,
/// then match `a e i o u` **and `y`** — Phonex's vowel set for this one rule
/// includes `Y`, which is what makes `Ellery` → `A460` rather than `A400`.
/// A non-ASCII scalar never reaches here (the preprocessing skips it), and no
/// non-ASCII letter is a vowel for this rule in any case.
#[inline]
fn is_vowel(c: Option<char>) -> bool {
    match c {
        Some(c) => matches!(c.to_ascii_lowercase(), 'a' | 'e' | 'i' | 'o' | 'u' | 'y'),
        None => false,
    }
}

/// Streaming equivalent of the clean + trailing-`S`
/// removal: yields the input's alphabetic characters uppercased (full
/// Unicode case mapping, so one input char may yield several), with every
/// maximal run of `S` that touches the end of the stream dropped.
///
/// Trailing-`S` removal normally needs the whole string; here an `S` run is
/// only *counted* (`pending_s`) until a later non-`S` letter proves it was
/// not trailing, at which point the run is replayed. Memory use is O(1)
/// regardless of input length.
struct CleanChars<'a> {
    inner: std::str::Chars<'a>,
    /// `S` characters seen but not yet known to be non-trailing.
    pending_s: usize,
    /// The non-`S` character that proved a pending `S` run non-trailing;
    /// emitted after the run is replayed.
    held: Option<char>,
}

impl<'a> CleanChars<'a> {
    fn new(token: &'a str) -> Self {
        Self {
            inner: token.chars(),
            pending_s: 0,
            held: None,
        }
    }

    /// The next `A`-`Z` letter, uppercased. Every other scalar is skipped.
    fn next_upper(&mut self) -> Option<char> {
        self.inner
            .find(char::is_ascii_alphabetic)
            .map(|c| c.to_ascii_uppercase())
    }
}

impl Iterator for CleanChars<'_> {
    type Item = char;

    fn next(&mut self) -> Option<char> {
        loop {
            if self.held.is_some() {
                if self.pending_s > 0 {
                    self.pending_s -= 1;
                    return Some('S');
                }
                return self.held.take();
            }
            match self.next_upper() {
                // End of input: any pending `S` run was trailing — drop it.
                None => return None,
                Some('S') => self.pending_s += 1,
                Some(c) => {
                    if self.pending_s > 0 {
                        self.held = Some(c);
                        self.pending_s -= 1;
                        return Some('S');
                    }
                    return Some(c);
                }
            }
        }
    }
}

/// The full preprocessed character stream: [`CleanChars`] with the leading
/// transformations applied to the first one or two characters, in order:
///
/// 1. leading pair (`KN`→`NN`, `PH`→`FH`, `WR`→`RR` — only the first letter
///    is replaced);
/// 2. one leading `H` removed (checked once, after the pair rewrite);
/// 3. leading-letter substitution (`EIOUY`→`A`, `P`→`B`, `V`→`F`, `KQ`→`C`,
///    `J`→`G`, `Z`→`S`) — applied to the character exposed by the `H`
///    removal, which therefore never gets the *pair* treatment
///    (`HKNUTH` → `CNUTH`, not `NNUTH`).
struct PreChars<'a> {
    clean: CleanChars<'a>,
    first: Option<char>,
    second: Option<char>,
}

impl<'a> PreChars<'a> {
    fn new(token: &'a str) -> Self {
        let mut clean = CleanChars::new(token);
        let mut first = clean.next();
        let mut second = clean.next();

        // Leading pair: replace only the first character.
        match (first, second) {
            (Some('K'), Some('N')) => first = Some('N'),
            (Some('P'), Some('H')) => first = Some('F'),
            (Some('W'), Some('R')) => first = Some('R'),
            _ => {}
        }

        // One leading `H` is dropped. (`second` can only be `None` here when
        // the stream is exhausted, so the shift below cannot lose a char.)
        if first == Some('H') {
            first = second;
            second = clean.next();
        }

        // Leading-letter substitution.
        first = match first {
            Some('E' | 'I' | 'O' | 'U' | 'Y') => Some('A'),
            Some('P') => Some('B'),
            Some('V') => Some('F'),
            Some('K' | 'Q') => Some('C'),
            Some('J') => Some('G'),
            Some('Z') => Some('S'),
            other => other,
        };

        Self {
            clean,
            first,
            second,
        }
    }
}

impl Iterator for PreChars<'_> {
    type Item = char;

    fn next(&mut self) -> Option<char> {
        if let Some(c) = self.first.take() {
            return Some(c);
        }
        if let Some(c) = self.second.take() {
            return Some(c);
        }
        self.clean.next()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- Lait and Randell's two stages, transcribed separately -------------
    //
    // `Phonex::process` streams preprocessing and transcoding together
    // through `PreChars`/`CleanChars`, so neither stage ever materialises a
    // string, and it carries the bookkeeping the `M`/`N` swallow needs in two
    // `char` variables. Neither shape is what the paper describes, and an
    // expected value read off that code would pin the code rather than the
    // rules.
    //
    // `reference_preprocess` and `reference_encode` are therefore a second
    // transcription: two flat stages over an owned `Vec<char>`, written from
    // the rule list on [`Phonex`] itself — the preprocessing steps, the digit
    // table, the three context rules, and the emission rule that type's
    // documentation states in full. Every fixture in this module goes through
    // `preprocessed` or `encoded`, which assert the two agree, so no expected
    // value below can be true of `Phonex::process` alone.

    /// Stage 1: remove trailing `S`s, rewrite a leading pair, drop one
    /// leading `H`, substitute the resulting first letter.
    fn reference_preprocess(token: &str) -> Vec<char> {
        let mut cs: Vec<char> = token
            .chars()
            .filter(char::is_ascii_alphabetic)
            .map(|c| c.to_ascii_uppercase())
            .collect();
        while cs.last() == Some(&'S') {
            cs.pop();
        }
        if cs.len() >= 2 {
            match (cs[0], cs[1]) {
                ('K', 'N') => cs[0] = 'N',
                ('P', 'H') => cs[0] = 'F',
                ('W', 'R') => cs[0] = 'R',
                _ => {}
            }
        }
        if cs.first() == Some(&'H') {
            cs.remove(0);
        }
        if let Some(c) = cs.first_mut() {
            *c = match *c {
                'E' | 'I' | 'O' | 'U' | 'Y' => 'A',
                'P' => 'B',
                'V' => 'F',
                'K' | 'Q' => 'C',
                'J' => 'G',
                'Z' => 'S',
                other => other,
            };
        }
        cs
    }

    /// The digit table, then the two context rules that can silence a letter.
    /// `None` means "silent": the carried digit is left as it was.
    fn reference_digit(cur: char, next: Option<char>) -> Option<char> {
        let context_vowel = |c: Option<char>| {
            c.is_some_and(|c| matches!(c.to_ascii_lowercase(), 'a' | 'e' | 'i' | 'o' | 'u' | 'y'))
        };
        match (cur, next) {
            // `D`/`T` before `C` is silent.
            ('D' | 'T', Some('C')) => None,
            // `L` and `R` code only before a vowel or at word end.
            ('L' | 'R', n) if !(context_vowel(n) || n.is_none()) => None,
            _ => Some(match cur {
                'B' | 'P' | 'F' | 'V' => '1',
                'C' | 'S' | 'K' | 'G' | 'J' | 'Q' | 'X' | 'Z' => '2',
                'D' | 'T' => '3',
                'L' => '4',
                'M' | 'N' => '5',
                'R' => '6',
                _ => '0',
            }),
        }
    }

    /// Stage 2: the head verbatim, then one digit per preprocessed character
    /// under the emission rule stated on [`Phonex`] — a digit is emitted
    /// unless it equals the character the code most recently gained, the head
    /// letter included.
    fn reference_encode(token: &str, max_len: usize) -> String {
        let cs = reference_preprocess(token);
        let mut out = String::new();
        let mut carried = '0';
        let mut compare_to = '0';
        let mut i = 0;
        let mut head = true;
        while i < cs.len() {
            if out.len() == max_len {
                break;
            }
            let cur = cs[i];
            let next = cs.get(i + 1).copied();
            if head {
                out.push(cur);
            }
            if let Some(digit) = reference_digit(cur, next) {
                carried = digit;
            }
            // `M`/`N` swallow a following `D` or `G`.
            let swallow = matches!(cur, 'M' | 'N') && matches!(next, Some('D' | 'G'));
            // The head letter already stands for its own digit -- unless the
            // swallow consumed a letter, in which case the digit is emitted.
            let head_stands_for_it = head && !swallow;
            if swallow {
                i += 1;
            }
            if carried != '0' && carried != compare_to && !head_stands_for_it {
                out.push(carried);
            }
            compare_to = out.chars().next_back().unwrap_or('0');
            if head_stands_for_it {
                compare_to = carried;
            }
            i += 1;
            head = false;
        }
        while out.len() < max_len {
            out.push('0');
        }
        out
    }

    /// The preprocessed character stream, asserting the streaming `PreChars`
    /// and the flat [`reference_preprocess`] agree.
    fn preprocessed(token: &str) -> String {
        let streamed: String = PreChars::new(token).collect();
        let reference: String = reference_preprocess(token).into_iter().collect();
        assert_eq!(streamed, reference, "preprocessing {token:?}");
        streamed
    }

    /// Encodes with [`Phonex`] *and* with [`reference_encode`], asserting they
    /// agree before returning the code.
    fn encoded(token: &str, max_len: usize) -> String {
        let code = Phonex::with_max_code_length(max_len).process(token);
        assert_eq!(
            code,
            reference_encode(token, max_len),
            "the transcribed rule list disagrees on {token:?} at length {max_len}"
        );
        code
    }

    fn assert_encodes(cases: &[(&str, &str)]) {
        for &(input, expected) in cases {
            assert_eq!(encoded(input, 4), expected, "encoding {input:?}");
        }
    }

    /// Both transcriptions agree on *every* word up to four letters over the
    /// alphabet, at three code lengths. The two stages interlock in ways a
    /// fixture list cannot enumerate -- a swallow at the head, a silenced
    /// `L`/`R` carrying the previous digit into the next comparison, a
    /// trailing-`S` strip that changes which letter is last -- so the pairing
    /// is checked over every short input rather than sampled.
    #[test]
    fn both_transcriptions_agree_on_every_word_of_up_to_four_letters() {
        const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ";
        let mut buf = [0u8; 4];
        for len in 0..=4usize {
            let mut idx = [0usize; 4];
            loop {
                for (slot, &i) in buf[..len].iter_mut().zip(idx.iter()) {
                    *slot = ALPHABET[i];
                }
                let word = std::str::from_utf8(&buf[..len]).unwrap();
                for max_len in [1usize, 4, 8] {
                    // `encoded` carries the assertion.
                    let _ = encoded(word, max_len);
                }
                let mut pos = 0;
                loop {
                    if pos == len {
                        break;
                    }
                    idx[pos] += 1;
                    if idx[pos] < ALPHABET.len() {
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

    // ------------------------------------------------------------------
    // Fixtures. Their inputs are the names Phonex is customarily exercised
    // on -- Knuth's Soundex pairs, the German surnames the algorithm was
    // measured against, and Lait and Randell's own examples. Their expected
    // values are derived: each one is checked against the transcribed rule
    // list above on every run, and the ones that turn on a rule worth
    // spelling out carry the derivation in a comment.
    // ------------------------------------------------------------------

    /// Stage 1 on its own: one fixture per preprocessing rule.
    ///
    /// `TESTSSS` -> the trailing-`S` run goes, all three of it -> `TEST`.
    /// `SSS` is nothing but that run, so stage 1 consumes the whole word.
    /// `KNUTH`/`PHONETIC`/`WRIGHT` show the leading pair rewriting its
    /// *first* letter only; `HARRINGTON` the single leading `H`; the last
    /// six the leading-letter substitutions `E`->`A`, `P`->`B`, `V`->`F`,
    /// `K`->`C`, `J`->`G`, `Z`->`S`. `JONES` is both ends at once: the
    /// trailing `S` goes and the `J` becomes `G`.
    #[test]
    fn preprocessing_stage_fixtures() {
        for (input, expected) in [
            ("TESTSSS", "TEST"),
            ("SSS", ""),
            ("KNUTH", "NNUTH"),
            ("PHONETIC", "FHONETIC"),
            ("WRIGHT", "RRIGHT"),
            ("HARRINGTON", "ARRINGTON"),
            ("EIGER", "AIGER"),
            ("PERCIVAL", "BERCIVAL"),
            ("VERTIGAN", "FERTIGAN"),
            ("KELVIN", "CELVIN"),
            ("JONES", "GONE"),
            ("ZEPHYR", "SEPHYR"),
        ] {
            assert_eq!(preprocessed(input), expected, "preprocessing {input:?}");
        }
    }

    /// The digit table and the three context rules, one row per entry: the
    /// twelve letters of classes 1 and 2, `D`/`T` coding and then silenced
    /// before `C`, `L` coding before a vowel and at word end but not before a
    /// consonant, `M`/`N` coding and swallowing a following `D` or `G`, and
    /// `R` coding before a vowel and at word end.
    #[test]
    fn digit_table_and_context_rules() {
        for (curr, next, is_last_char, code, skip_next_char) in [
            ('B', None, false, Some('1'), false),
            ('P', None, false, Some('1'), false),
            ('F', None, false, Some('1'), false),
            ('V', None, false, Some('1'), false),
            ('C', None, false, Some('2'), false),
            ('S', None, false, Some('2'), false),
            ('K', None, false, Some('2'), false),
            ('G', None, false, Some('2'), false),
            ('J', None, false, Some('2'), false),
            ('Q', None, false, Some('2'), false),
            ('X', None, false, Some('2'), false),
            ('Z', None, false, Some('2'), false),
            ('D', None, false, Some('3'), false),
            ('T', None, false, Some('3'), false),
            ('D', Some('C'), false, None, false),
            ('T', Some('C'), false, None, false),
            ('L', Some('A'), false, Some('4'), false),
            ('L', Some('B'), true, Some('4'), false),
            ('L', Some('B'), false, None, false),
            ('M', None, false, Some('5'), false),
            ('N', None, false, Some('5'), false),
            ('M', Some('D'), false, Some('5'), true),
            ('M', Some('G'), false, Some('5'), true),
            ('R', Some('A'), false, Some('6'), false),
            ('R', None, true, Some('6'), false),
        ] {
            assert_eq!(
                transcode(curr, next, is_last_char),
                (code, skip_next_char),
                "transcoding {curr:?} before {next:?} (last: {is_last_char})"
            );
        }
    }

    /// Whole-name encodings. Worked derivations for six of them, each
    /// chosen for the rule it turns on:
    ///
    /// * `Ashcraft`: stage 1 leaves it alone -> `A` emitted, its own digit
    ///   `0` dropped; `S`->`2`; `H`->`0`, nothing emitted; `C`->`2`, equal to
    ///   the `2` already there, suppressed; `R` before `A` -> `6`; `A`->`0`;
    ///   `F`->`1`; the code is full at four -> `A261`.
    /// * `Wright`: stage 1 rewrites `WR`->`RR` -> `RRIGHT`. The head `R` is
    ///   followed by `R`, neither a vowel nor word end, so it is silent; the
    ///   second `R` is before `I` -> `6`; `G`->`2`; `H`->`0`; `T` at word end
    ///   -> `3` -> `R623`.
    /// * `Lee`: `L` before `E` codes `4`, but it is the head, which already
    ///   stands for its own digit -> `L000`. Nothing else in the word codes.
    /// * `Ellery`: stage 1 gives `ALLERY`. First `L` before `L` is silent;
    ///   second `L` before `E` -> `4`; `R` before **`Y`** -> `6`, because `Y`
    ///   counts as a vowel for this rule -> `A460`.
    /// * `Hilbert`: the leading `H` goes, the exposed `I` becomes `A` ->
    ///   `ALBERT`. `L` before `B` is silent; `B`->`1`; `R` before `T` is
    ///   silent; `T` at word end -> `3` -> `A130`.
    /// * `Ghosh`: `G`'s own digit `2` is dropped with the head, and after
    ///   `H`->`0` emits nothing the comparison falls back to the letter `G`,
    ///   so the later `S` emits `2` after all -> `G200`, not `G000`.
    #[test]
    fn whole_name_encodings() {
        assert_encodes(&[
            ("123 testsss", "T230"),
            ("24/7 test", "T230"),
            ("A", "A000"),
            ("Ashcraft", "A261"),
            ("Lee", "L000"),
            ("Kuhne", "C500"),
            ("Meyer-Lansky", "M452"),
            ("Oepping", "A150"),
            ("Daley", "D400"),
            ("Dalitz", "D432"),
            ("Duhlitz", "D432"),
            ("Dull", "D400"),
            ("De Ledes", "D430"),
            ("Sandemann", "S500"),
            ("Schmidt", "S530"),
            ("Sinatra", "S536"),
            ("Heinrich", "A562"),
            ("Hammerschlag", "A524"),
            ("Williams", "W450"),
            ("Wilms", "W500"),
            ("Wilson", "W250"),
            ("Worms", "W500"),
            ("Zedlitz", "S343"),
            ("Zotteldecke", "S320"),
            ("ZYX test", "S232"),
            ("Scherman", "S500"),
            ("Schurman", "S500"),
            ("Sherman", "S500"),
            ("Shermansss", "S500"),
            ("Shireman", "S650"),
            ("Shurman", "S500"),
            ("Euler", "A460"),
            ("Ellery", "A460"),
            ("Hilbert", "A130"),
            ("Heilbronn", "A165"),
            ("Gauss", "G000"),
            ("Ghosh", "G200"),
            ("Knuth", "N300"),
            ("Kant", "C530"),
            ("Lloyd", "L430"),
            ("Ladd", "L300"),
            ("Lukasiewicz", "L200"),
            ("Lissajous", "L200"),
            ("Philip", "F410"),
            ("Fripp", "F610"),
            ("Czarkowska", "C200"),
            ("Hornblower", "A514"),
            ("Looser", "L260"),
            ("Wright", "R623"),
            ("Phonic", "F520"),
            ("Quickening", "C250"),
            ("Kuickening", "C250"),
            ("Joben", "G150"),
            ("Zelda", "S300"),
        ]);
    }

    /// Input with no `A`-`Z` letter: stage 1 yields nothing, so the code is
    /// pure padding at the configured length.
    #[test]
    fn number_and_empty_input() {
        assert_encodes(&[("123456789", "0000"), ("", "0000")]);
    }

    // ------------------------------------------------------------------
    // Verbora edge cases: inputs no publication names -- empty and
    // letterless tokens, single letters, junk between letters, non-ASCII
    // scalars, very long words, every configured code length. Their expected
    // values are derived the same way, `encoded` checking each against the
    // transcribed rule list.
    // ------------------------------------------------------------------

    #[test]
    fn single_letters() {
        assert_encodes(&[
            ("A", "A000"),
            ("B", "B000"),
            // Preprocessing consumes the whole input.
            ("H", "0000"), // leading-H removal
            ("S", "0000"), // trailing-S removal
            ("s", "0000"),
            // Leading-letter substitution changes the emitted head.
            ("E", "A000"),
            ("Y", "A000"),
            ("P", "B000"),
            ("V", "F000"),
            ("K", "C000"),
            ("Q", "C000"),
            ("J", "G000"),
            ("Z", "S000"),
            // L and R are word-final here, so their digit is computed (but
            // never pushed for a first character).
            ("L", "L000"),
            ("R", "R000"),
            ("X", "X000"),
        ]);
    }

    #[test]
    fn non_letters_only() {
        assert_encodes(&[
            ("   ", "0000"),
            ("!!!", "0000"),
            ("-'\u{2019}", "0000"),
            ("\t\n", "0000"),
            ("0", "0000"),
            ("42", "0000"),
            ("😀🚀", "0000"),
        ]);
    }

    #[test]
    fn mixed_case_and_embedded_noise() {
        assert_eq!(encoded("knuth", 4), "N300");
        assert_eq!(encoded("KnUtH", 4), "N300");
        assert_eq!(encoded("k n u t h", 4), "N300");
        assert_eq!(encoded("K9N-U_T.H!", 4), "N300");
        assert_eq!(encoded("wright", 4), encoded("WRIGHT", 4));
    }

    /// The text unit, enumerated over one scalar of every class. A scalar
    /// outside `A`-`Z` is skipped, so a name codes exactly as its
    /// ASCII-letters-only spelling and every code is pure ASCII of the
    /// configured length.
    #[test]
    fn only_ascii_letters_are_read() {
        for input in [
            "",
            " ",
            "12345",
            "...",
            "\u{65e5}\u{672c}\u{8a9e}",
            "\u{1F600}",
            "\u{041c}\u{043e}",
        ] {
            assert_eq!(encoded(input, 4), "0000", "for {input:?}");
        }
        assert_eq!(encoded("caf\u{e9}", 4), encoded("caf", 4));
        assert_eq!(encoded("\u{e4}hnlich", 4), encoded("hnlich", 4));
        assert_eq!(encoded("stra\u{df}e", 4), encoded("strae", 4));
        assert_eq!(encoded("\u{10428}", 4), "0000");
        // The code is always exactly `max_code_length` ASCII characters, so
        // the byte length and the character length can never disagree.
        for input in ["caf\u{e9}", "\u{65e5}ba", "Sinatra", "", "\u{10428}x"] {
            for len in [2usize, 4, 6] {
                let code = encoded(input, len);
                assert!(code.is_ascii(), "{input:?} at {len}: {code:?}");
                assert_eq!(code.len(), len, "{input:?} at {len}: {code:?}");
                assert_eq!(code.chars().count(), len);
            }
        }
    }

    /// `\u{df}` is not an `A`-`Z` letter, so it is skipped rather than case-expanded
    /// into a pair of `S`es that the trailing-`S` strip would then eat.
    #[test]
    fn sharp_s_is_skipped_not_expanded() {
        assert_eq!(preprocessed("\u{df}"), "");
        assert_eq!(encoded("\u{df}", 4), "0000");
        assert_eq!(encoded("Stra\u{df}e", 4), encoded("Strae", 4));
        assert_ne!(encoded("Stra\u{df}e", 4), encoded("Strasse", 4));
        assert_eq!(encoded("a\u{df}", 4), "A000");
    }

    /// Trailing-`S` removal strips runs of any length, but only at the end.
    #[test]
    fn trailing_s_runs() {
        assert_eq!(preprocessed("ASAS"), "ASA");
        assert_eq!(encoded("asa s", 4), "A200");
        // A long trailing run exercises the streaming counter.
        let long_tail = format!("T{}", "s".repeat(4096));
        assert_eq!(encoded(&long_tail, 4), "T000");
        assert_eq!(encoded(&"s".repeat(4096), 4), "0000");
        // Interior runs are kept.
        assert_eq!(preprocessed("ASSSSA"), "ASSSSA");
    }

    /// The head normally does not emit its own digit, because the head
    /// letter already stands for it — but the `M`/`N` swallow consumes the
    /// following `D`/`G`, so a head that swallows *does* emit. `Ng` -> `N500`
    /// where `Na` -> `N000`, from the same head letter and the same digit.
    /// The control group below is the half of that pair the rule must not
    /// change.
    #[test]
    fn a_swallowing_head_emits_its_own_digit() {
        assert_encodes(&[
            ("Ng", "N500"),
            ("Nd", "N500"),
            ("Mg", "M500"),
            ("Md", "M500"),
            // Control group: no swallow, no digit from the head.
            ("Na", "N000"),
            ("Ma", "M000"),
            ("Nt", "N300"),
        ]);
    }

    /// A digit is compared against the character the code most recently
    /// gained, and after a step that emits nothing that character is a
    /// *letter* — which no digit equals.
    ///
    /// `Czarkowska`: the head `C` carries digit `2`; the `Z` also codes `2`
    /// and is suppressed against it; that step emitted nothing, so the
    /// comparison falls back to the letter `C`, and the later `K` emits its
    /// `2` -> `C200`.
    ///
    /// `Sandemann` is the control: `N` before `D` swallows the `D` and emits
    /// `5`; every later `M`/`N` codes `5` too and is suppressed against the
    /// `5` already in the code, which is a *digit* — so nothing more is
    /// emitted -> `S500`.
    #[test]
    fn a_digit_is_compared_against_the_last_character_emitted() {
        assert_encodes(&[("Czarkowska", "C200"), ("Sandemann", "S500")]);
    }

    /// Leading-`H` removal happens once, and the letter it exposes gets the
    /// substitution table but never the pair table.
    #[test]
    fn leading_h_cases() {
        assert_eq!(preprocessed("HHART"), "HART");
        assert_eq!(encoded("Hhart", 4), "H300");
        assert_eq!(encoded("Hh", 4), "H000");
        assert_eq!(encoded("Hhh", 4), "H000");
        // H-removal exposes K, which is *substituted* (K→C), not
        // pair-rewritten (KN→NN).
        assert_eq!(preprocessed("HKNUTH"), "CNUTH");
        assert_eq!(encoded("Hknuth", 4), "C530");
    }

    /// Leading pairs replace only their first letter, and interact with the
    /// trailing-S strip that runs before them.
    #[test]
    fn leading_pair_cases() {
        assert_eq!(preprocessed("KNS"), "NN");
        assert_eq!(encoded("kns", 4), "N000");
        assert_eq!(preprocessed("PHS"), "FH");
        assert_eq!(encoded("phs", 4), "F000");
        assert_eq!(preprocessed("WRS"), "RR");
        assert_eq!(encoded("wrs", 4), "R600");
        // Pair letters *not* at the head are ordinary.
        assert_eq!(encoded("Akn", 4), "A250");
    }

    /// `Y` counts as a vowel for the `L`/`R` context rules, which is the one
    /// place Phonex's vowel set differs from `AEIOU`: `R` before `Y` codes
    /// (`Ary` -> `A600`), `R` before a consonant does not (`Arb` -> `A100`,
    /// where only the `B` codes).
    #[test]
    fn y_is_a_context_vowel() {
        assert_encodes(&[("Ellery", "A460"), ("Ary", "A600"), ("Arb", "A100")]);
    }

    /// Longer chains where the three context rules and the emission rule
    /// interlock: repeated nasal swallows including one at the head,
    /// `D`-before-`C` silences that carry the previous digit forward, `L`/`R`
    /// flipping between silent and coding, and a pure same-class run.
    ///
    /// Two worked derivations, one per mechanism:
    ///
    /// * `dcdcdc` -> `D200`. Stage 1 leaves it. Head `D` is before `C`, so
    ///   it is silent and the carried digit stays `0`; the head emits
    ///   nothing. `C`->`2`, compared against the letter `D` -> emitted.
    ///   Every later `D` is silent (before `C`) and every later `C` codes
    ///   `2`, equal to the `2` already emitted -> nothing more.
    /// * `ssssb` -> `S210`. Head `S` carries `2`; the second `S` codes `2`
    ///   and is suppressed against the head's own digit; that step emitted
    ///   nothing, so the comparison falls back to the letter `S` and the
    ///   **third** `S` emits `2`; the fourth is suppressed against it; `B`
    ///   emits `1`.
    #[test]
    fn context_rule_chains() {
        assert_encodes(&[
            // Repeated M/N+D/G swallows, incl. the first-position quirk.
            ("ndgndgndg", "N525"),
            ("knknkn", "N252"),
            ("ngng", "N500"),
            ("mgmg", "M500"),
            // Swallow after other letters, with dedup reset in between.
            ("sandgmann", "S525"),
            ("mndgl", "M240"),
            // D-before-C silence carrying the previous code.
            ("dcdcdc", "D200"),
            ("tctc", "T200"),
            ("dcl", "D240"),
            ("ldc", "L200"),
            // L/R context: silenced (before consonant) versus coded (before
            // vowel/Y or at word end).
            ("rlrlrl", "R400"),
            ("lrlrlr", "L600"),
            ("rylr", "R600"),
            ("arlb", "A100"),
            // A pure same-digit run: the reset lets the THIRD S push the
            // digit the second suppressed (same mechanism as Czarkowska).
            ("ssssb", "S210"),
        ]);
    }

    #[test]
    fn configurable_length() {
        assert_eq!(encoded("Sinatra", 0), "");
        assert_eq!(encoded("", 0), "");
        assert_eq!(encoded("Sinatra", 1), "S");
        assert_eq!(encoded("", 1), "0");
        assert_eq!(encoded("Sinatra", 2), "S5");
        assert_eq!(encoded("Sinatra", 6), "S53600");
        assert_eq!(encoded("Sinatra", 10), "S536000000");
        assert_eq!(encoded("", 6), "000000");
    }

    /// The configured length is honoured exactly, for every input: with an
    /// all-ASCII code there is no scalar that can overshoot it.
    #[test]
    fn the_configured_length_is_never_overshot() {
        for len in [1usize, 2, 3, 4, 8] {
            for input in [
                "\u{65e5}ba",
                "\u{65e5}\u{672c}\u{8a9e}",
                "Sinatra",
                "",
                "caf\u{e9}",
            ] {
                assert_eq!(encoded(input, len).len(), len, "{input:?} at {len}");
            }
        }
        assert_eq!(encoded("\u{65e5}ba", 2), "B0");
    }

    #[test]
    fn very_long_input() {
        assert_eq!(encoded(&"a".repeat(10_000), 4), "A000");
        assert_eq!(encoded(&"ab".repeat(5_000), 4), "A100");
        assert_eq!(encoded(&"Czarkowska".repeat(1_000), 4), "C200");
    }

    #[test]
    fn compare_matches_code_equality() {
        let phonex = Phonex::new();
        assert!(phonex.compare("Knuth", "Nuth"));
        assert!(phonex.compare("Schmidt", "Schmit"));
        assert!(phonex.compare("Dalitz", "Duhlitz"));
        assert!(phonex.compare("", "123"));
        assert!(!phonex.compare("Wilson", "Worms"));
        // Length changes what collides.
        assert!(Phonex::with_max_code_length(1).compare("Sinatra", "Sherman"));
        assert!(!Phonex::new().compare("Sinatra", "Sherman"));
    }

    #[test]
    fn constructors_and_getters() {
        assert_eq!(Phonex::default(), Phonex::new());
        assert_eq!(Phonex::new().max_code_length(), 4);
        assert_eq!(Phonex::with_max_code_length(4), Phonex::new());
        assert_eq!(Phonex::with_max_code_length(7).max_code_length(), 7);
    }

    #[test]
    fn phonetic_trait_delegates() {
        let phonex: &dyn verbora_core::Phonetic = &Phonex::new();
        assert_eq!(phonex.process("Knuth"), encoded("Knuth", 4));
        assert_eq!(phonex.process("Knuth"), "N300");
        assert!(phonex.compare("Knuth", "Nuth"));
    }
}
