//! Metaphone (Philips, 1990).

use crate::letters::{is_ascii_vowel, letters_into};

/// Original Metaphone.
///
/// # Publication
///
/// Lawrence Philips, "Hanging on the Metaphone", *Computer Language* 7(12),
/// December 1990, pp. 39–43. The article specifies the algorithm as a table of
/// per-letter rewrite rules plus four word-initial exceptions; Verbora
/// implements that table and nothing else. Where the article is silent —
/// notably on what to do with a character outside `A`–`Z` — Verbora states its
/// own rule below rather than importing one from another implementation.
///
/// # The contract
///
/// * **The text unit is one Unicode scalar**, and only the twenty-six letters
///   `A`–`Z` are read, after simple ASCII case folding. Every other scalar is
///   skipped: it produces no code and does not break a cluster, so `"o'clock"`
///   and `"oclock"` encode identically. Metaphone's rules are stated over
///   English orthography and define nothing for `é`, `Ж` or `語`;
///   transliterate first if you want those to code as their Latin spelling.
/// * **The key is not truncated.** `process` returns the complete key, however
///   long the word makes it. Fixed-width keys are an indexing decision, not a
///   property of Philips's rules: truncate at the call site, or let
///   [`PhoneticIndex`](crate::PhoneticIndex) do it — that type documents the
///   bounded prefix it keys on.
/// * `""` is returned when, and only when, the input holds no `A`–`Z` letter.
///   No non-empty key is reachable from such an input, so the empty key is the
///   absence of a key rather than a sentinel value.
/// * **Total.** No input panics, and there is no error type.
///
/// # The rule table
///
/// Vowels are kept only as the first letter of the word. A doubled letter
/// other than `C` is read once. `N` below is the next letter, `P` the
/// previous.
///
/// | Letter | Rule |
/// |---|---|
/// | `B` | `B`, unless it ends the word after `M` (*dumb*) |
/// | `C` | `X` in `-CIA-` and `-CH-`; `S` before `I` `E` `Y`; silent in `-SCI-` `-SCE-` `-SCY-`; otherwise `K`, including `-SCH-` |
/// | `D` | `J` in `-DGE-` `-DGY-` `-DGI-` (consuming the `G`); otherwise `T` |
/// | `F` | `F` |
/// | `G` | silent in `-GH-` when the `H` neither ends the word nor precedes a vowel, and in word-final `-GN` `-GNED` `-GNS`; `J` before `I` `E` `Y`; otherwise `K` |
/// | `H` | silent after a vowel with no vowel following, and after `C` `S` `P` `T` `G`; otherwise `H` |
/// | `J` | `J` |
/// | `K` | silent after `C`; otherwise `K` |
/// | `L` `M` `N` `R` | themselves |
/// | `P` | `F` before `H`; otherwise `P` |
/// | `Q` | `K` |
/// | `S` | `X` before `H` and in `-SIO-` `-SIA-`; otherwise `S` |
/// | `T` | `X` in `-TIA-` `-TIO-`; `0` before `H`; silent in `-TCH-`; otherwise `T` |
/// | `V` | `F` |
/// | `W` | `W` before a vowel; otherwise silent |
/// | `X` | `KS` |
/// | `Y` | `Y` before a vowel; otherwise silent |
/// | `Z` | `S` |
///
/// Word-initial exceptions, applied to the letter sequence before the table
/// runs: `AE-` `GN-` `KN-` `PN-` `WR-` lose their first letter, `X-` becomes
/// `S-`, and `WH-` becomes `W-`.
///
/// `0` is the digit zero, standing for the "th" sound; it is the one non-letter
/// character a key can contain.
///
/// # Examples
///
/// ```
/// use verbora_phonetics::Metaphone;
///
/// let metaphone = Metaphone::new();
/// assert_eq!(metaphone.process("phonetics"), "FNTKS");
/// assert_eq!(metaphone.process("fonetix"), "FNTKS");
/// assert!(metaphone.compare("phonetics", "fonetix"));
///
/// // `ch` is `X`, the "sh" sound, exactly as the 1990 table specifies.
/// assert_eq!(metaphone.process("chemical"), "XMKL");
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Metaphone;

impl Metaphone {
    /// Creates a Metaphone encoder.
    ///
    /// The encoder is stateless and zero-sized; the type exists so that
    /// [`verbora_core::Phonetic`] can be implemented for it.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Encodes `token` as its complete Metaphone key.
    ///
    /// Returns `""` — and only then — when `token` contains no `A`–`Z`
    /// letter. See the [type documentation](Self) for the rule table.
    #[must_use]
    pub fn process(&self, token: &str) -> String {
        let mut out = String::new();
        self.process_into(token, &mut out);
        out
    }

    /// Appends `token`'s Metaphone key to `out`.
    ///
    /// Appends nothing when `token` has no `A`–`Z` letter. `out` is never
    /// cleared.
    ///
    /// # Choosing the right API
    ///
    /// | | [`process`](Self::process) | `process_into` |
    /// |---|---|---|
    /// | Use case | one word, one key | encoding a dictionary into a buffer you already own |
    /// | Allocation | one `String` per call | none, once `out` has grown |
    /// | Trade-off | none | you manage `out`, including clearing it |
    /// | Recommendation | **the default** | reach for it only when a profile shows the per-call `String` matters |
    pub fn process_into(&self, token: &str, out: &mut String) {
        let mut word = Vec::new();
        letters_into(token, &mut word);
        reduce_doubled_letters(&mut word);
        apply_initial_exceptions(&mut word);
        encode(&word, out);
    }

    /// Whether `a` and `b` share a Metaphone key.
    #[must_use]
    pub fn compare(&self, a: &str, b: &str) -> bool {
        self.process(a) == self.process(b)
    }
}

/// "Doubled letters except `C` are reduced to a single letter."
///
/// A transformation of the word, not a filter on the emitted key: the letter
/// after a reduction sees the reduced spelling, which is why `MISSION` reaches
/// the `-SIO-` rule (`M X N`) instead of coding its second `S` as a plain `S`.
fn reduce_doubled_letters(word: &mut Vec<u8>) {
    let mut previous = None;
    word.retain(|&letter| {
        let doubled = previous == Some(letter) && letter != b'C';
        previous = Some(letter);
        !doubled
    });
}

/// `AE- GN- KN- PN- WR-` lose their first letter, `X-` becomes `S-`, `WH-`
/// becomes `W-`.
///
/// Applied to the letter sequence, so the rest of the table then sees a word
/// whose *first letter* is the transformed one — which is what makes the vowel
/// rule fire for `AEbersold` and not for `Xavier`.
fn apply_initial_exceptions(word: &mut Vec<u8>) {
    let head: &[u8] = &word[..word.len().min(2)];
    match head {
        b"AE" | b"GN" | b"KN" | b"PN" | b"WR" => {
            word.remove(0);
        }
        b"WH" => {
            word.remove(1);
        }
        _ => {
            if word.first() == Some(&b'X') {
                word[0] = b'S';
            }
        }
    }
}

/// The letter at `index`, or `None` past either end.
#[inline]
fn at(word: &[u8], index: usize) -> Option<u8> {
    word.get(index).copied()
}

#[inline]
fn is(word: &[u8], index: usize, letter: u8) -> bool {
    at(word, index) == Some(letter)
}

#[inline]
fn is_any(word: &[u8], index: usize, letters: &[u8]) -> bool {
    at(word, index).is_some_and(|c| letters.contains(&c))
}

#[inline]
fn vowel_at(word: &[u8], index: usize) -> bool {
    at(word, index).is_some_and(is_ascii_vowel)
}

/// The single pass over the letter sequence.
fn encode(word: &[u8], out: &mut String) {
    let n = word.len();
    let mut i = 0;
    while i < n {
        let c = word[i];

        match c {
            // "Vowels are only kept when they are the first letter."
            b'A' | b'E' | b'I' | b'O' | b'U' => {
                if i == 0 {
                    out.push(char::from(c));
                }
            }
            // "B unless at the end of a word after M as in dumb."
            b'B' => {
                if !(i + 1 == n && i > 0 && word[i - 1] == b'M') {
                    out.push('B');
                }
            }
            b'C' => {
                if is(word, i + 1, b'I') && is(word, i + 2, b'A') {
                    out.push('X');
                } else if is(word, i + 1, b'H') {
                    // "-SCH-" is K; every other -CH- is X.
                    out.push(if i > 0 && word[i - 1] == b'S' {
                        'K'
                    } else {
                        'X'
                    });
                } else if is_any(word, i + 1, b"IEY") {
                    // Silent in -SCI-, -SCE-, -SCY-.
                    if !(i > 0 && word[i - 1] == b'S') {
                        out.push('S');
                    }
                } else {
                    out.push('K');
                }
            }
            b'D' => {
                if is(word, i + 1, b'G') && is_any(word, i + 2, b"EYI") {
                    out.push('J');
                    // The G belongs to the same sound and is consumed with it.
                    i += 1;
                } else {
                    out.push('T');
                }
            }
            b'F' => out.push('F'),
            b'G' => {
                let silent_gh = is(word, i + 1, b'H')
                    && i + 2 < n // the H does not end the word
                    && !vowel_at(word, i + 2);
                let silent_gn =
                    is(word, i + 1, b'N') && matches!(&word[i + 1..], b"N" | b"NED" | b"NS");
                if silent_gh || silent_gn {
                    // silent
                } else if is_any(word, i + 1, b"IEY") {
                    out.push('J');
                } else {
                    out.push('K');
                }
            }
            b'H' => {
                let after_digraph =
                    i > 0 && matches!(word[i - 1], b'C' | b'S' | b'P' | b'T' | b'G');
                let stranded = i > 0 && is_ascii_vowel(word[i - 1]) && !vowel_at(word, i + 1);
                if !(after_digraph || stranded) {
                    out.push('H');
                }
            }
            b'J' => out.push('J'),
            b'K' => {
                if !(i > 0 && word[i - 1] == b'C') {
                    out.push('K');
                }
            }
            b'L' => out.push('L'),
            b'M' => out.push('M'),
            b'N' => out.push('N'),
            b'P' => out.push(if is(word, i + 1, b'H') { 'F' } else { 'P' }),
            b'Q' => out.push('K'),
            b'R' => out.push('R'),
            b'S' => {
                if is(word, i + 1, b'H') || (is(word, i + 1, b'I') && is_any(word, i + 2, b"OA")) {
                    out.push('X');
                } else {
                    out.push('S');
                }
            }
            b'T' => {
                if is(word, i + 1, b'I') && is_any(word, i + 2, b"AO") {
                    out.push('X');
                } else if is(word, i + 1, b'H') {
                    out.push('0');
                } else if !(is(word, i + 1, b'C') && is(word, i + 2, b'H')) {
                    out.push('T');
                }
            }
            b'V' => out.push('F'),
            b'W' => {
                if vowel_at(word, i + 1) {
                    out.push('W');
                }
            }
            b'X' => out.push_str("KS"),
            b'Y' => {
                if vowel_at(word, i + 1) {
                    out.push('Y');
                }
            }
            b'Z' => out.push('S'),
            // `letters_into` yields nothing outside A-Z, so this is
            // unreachable by construction rather than by assumption; leaving
            // it as a no-op keeps `encode` total for a hand-built slice.
            _ => {}
        }
        i += 1;
    }
}

impl verbora_core::Phonetic for Metaphone {
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

    fn m() -> Metaphone {
        Metaphone::new()
    }

    /// One witness per row of the published table, each with the derivation
    /// written out so the expected value comes from the rule and not from
    /// running this code.
    #[test]
    fn every_table_row_has_a_witness() {
        let m = m();

        // B: `B` normally; silent at the end after M.
        assert_eq!(m.process("bob"), "BB");
        assert_eq!(m.process("dumb"), "TM"); // D->T, U dropped, M, B silent
        assert_eq!(m.process("dumbo"), "TMB"); // the B no longer ends the word

        // C: X in -CIA-, X in -CH-, S before I/E/Y, silent in -SCI-, else K.
        assert_eq!(m.process("special"), "SPXL"); // S P (E) C+IA->X (I A) L
        assert_eq!(m.process("chemical"), "XMKL"); // CH->X, H silent, C->K
        assert_eq!(m.process("cent"), "SNT"); // C before E -> S
        assert_eq!(m.process("science"), "SNS"); // S, SCI C silent, N, C->S
        assert_eq!(m.process("cat"), "KT"); // C otherwise -> K
        assert_eq!(m.process("school"), "SKL"); // -SCH- -> K, H silent

        // D: J in -DGE-, T otherwise.
        assert_eq!(m.process("badge"), "BJ");
        assert_eq!(m.process("dodge"), "TJ");
        assert_eq!(m.process("dad"), "TT");

        // F.
        assert_eq!(m.process("fife"), "FF");

        // G: silent -GH-, silent -GN/-GNED/-GNS, J before I/E/Y, else K.
        assert_eq!(m.process("knight"), "NT"); // KN- drops K; GH silent; T
        assert_eq!(m.process("ghost"), "KST"); // GH before a vowel is not silent
        assert_eq!(m.process("tough"), "TK"); // GH ending the word is not silent
        assert_eq!(m.process("sign"), "SN");
        assert_eq!(m.process("signed"), "SNT");
        assert_eq!(m.process("signs"), "SNS");
        assert_eq!(m.process("gem"), "JM");
        assert_eq!(m.process("gap"), "KP");
        assert_eq!(m.process("egg"), "EK"); // E kept (first letter), GG -> G -> K

        // H: silent after a stranded vowel and after C/S/P/T/G.
        assert_eq!(m.process("aha"), "AH"); // vowel follows, so H survives
        assert_eq!(m.process("ah"), "A"); // nothing follows, so H is silent
        assert_eq!(m.process("hat"), "HT"); // word-initial H survives

        // J, K.
        assert_eq!(m.process("jaw"), "J"); // W not before a vowel is silent
        assert_eq!(m.process("kite"), "KT");
        assert_eq!(m.process("quick"), "KK"); // Q->K, CK: C->K, K silent after C

        // L, M, N, R.
        assert_eq!(m.process("lemon"), "LMN");
        assert_eq!(m.process("run"), "RN");

        // P.
        assert_eq!(m.process("phonetics"), "FNTKS");
        assert_eq!(m.process("pat"), "PT");

        // S: X before H and in -SIO-/-SIA-.
        assert_eq!(m.process("ship"), "XP");
        assert_eq!(m.process("tension"), "TNXN"); // T N (E) S+IO -> X, N
        assert_eq!(m.process("sat"), "ST");

        // T: X in -TIA-/-TIO-, 0 before H, silent in -TCH-.
        assert_eq!(m.process("nation"), "NXN");
        assert_eq!(m.process("thing"), "0NK");
        assert_eq!(m.process("watch"), "WX");
        assert_eq!(m.process("tot"), "TT");

        // V, X, Y, Z.
        assert_eq!(m.process("van"), "FN");
        assert_eq!(m.process("fox"), "FKS");
        assert_eq!(m.process("yes"), "YS");
        assert_eq!(m.process("sky"), "SK"); // Y with no vowel after it is silent
        assert_eq!(m.process("zoo"), "S");
    }

    /// The four word-initial exceptions, each with the un-excepted control
    /// beside it so the test fails if the exception stops firing *or* starts
    /// firing too widely.
    #[test]
    fn word_initial_exceptions() {
        let m = m();

        // AE-, GN-, KN-, PN-, WR- drop their first letter.
        assert_eq!(m.process("aegis"), m.process("egis"));
        assert_eq!(m.process("gnome"), m.process("nome"));
        assert_eq!(m.process("knight"), m.process("night"));
        assert_eq!(m.process("pneumatic"), m.process("neumatic"));
        assert_eq!(m.process("wright"), m.process("right"));
        // ... but only word-initially.
        assert_ne!(m.process("agnostic"), m.process("anostic"));

        // X- becomes S-.
        assert_eq!(m.process("xavier"), "SFR");
        // ... a medial X is still KS.
        assert_eq!(m.process("axe"), "AKS");

        // WH- becomes W-.
        assert_eq!(m.process("white"), "WT");
        assert_eq!(m.process("who"), "W"); // W before O; the H is gone
    }

    /// The one product fact this crate previously left unstated: `ch` is the
    /// "sh" sound `X`, not the `KSH` an earlier pipeline produced by running
    /// its `X` rule after its `C` rule. Every one of these words is one of the
    /// 41 in the 649-name corpus that differed.
    #[test]
    fn ch_is_x_not_ksh() {
        let m = m();
        for (word, want) in [
            ("chemical", "XMKL"),
            ("charles", "XRLS"),
            ("chicago", "XKK"),
            ("cheese", "XS"),
            ("chris", "XRS"),
            ("machine", "MXN"),
        ] {
            assert_eq!(m.process(word), want, "for {word:?}");
            assert!(
                !m.process(word).contains("KSH"),
                "{word:?} must not encode `ch` as KSH"
            );
        }
    }

    /// Doubled letters are read once, and `C` is the sole exemption, so `CC`
    /// runs both halves through the `C` row.
    #[test]
    fn doubled_letters_are_read_once_except_c() {
        let m = m();
        assert_eq!(m.process("ball"), "BL");
        assert_eq!(m.process("bal"), "BL");
        assert_eq!(m.process("bubble"), "BBL");
        // The reduction is a rewrite of the word, so the *next* rule sees the
        // reduced spelling: MISSION becomes MISION and reaches -SIO-.
        assert_eq!(m.process("mission"), "MXN");
        assert_eq!(m.process("mission"), m.process("mision"));
        // CC before a front vowel: first C is K (next letter is C), second C
        // is S (before E) -- two codes from one doubled letter.
        assert_eq!(m.process("accept"), "AKSPT");
        // CC before a back vowel: K then K.
        assert_eq!(m.process("account"), "AKKNT");
    }

    /// The text unit, enumerated over one scalar of every class.
    #[test]
    fn only_ascii_letters_are_read() {
        let m = m();
        for empty in ["", " ", "...", "1234", "日本語", "😀", "Москва", "\u{301}"] {
            assert_eq!(m.process(empty), "", "for {empty:?}");
        }
        assert_eq!(m.process("o'clock"), m.process("oclock"));
        assert_eq!(m.process("well-known"), m.process("wellknown"));
        assert_eq!(m.process("caf\u{e9}"), m.process("caf"));
        assert_eq!(m.process("na\u{ef}ve"), m.process("nave"));
        // An astral scalar is one unit; it can never be split.
        assert_eq!(m.process("f\u{1F600}ox"), m.process("fox"));
        assert_eq!(m.process("PHONETICS"), m.process("phonetics"));
    }

    /// The key is never truncated, so it grows with the word rather than
    /// silently capping — the property a caller relies on when they truncate
    /// it themselves.
    #[test]
    fn the_key_is_not_truncated() {
        let m = m();
        let long = "supercalifragilisticexpialidocious";
        assert_eq!(m.process(long), "SPRKLFRJLSTSKSPLTSS");
        // 250 `XY` pairs, so no letter is ever doubled. The leading X becomes
        // S (one character); each of the other 249 X's is KS (two); every Y is
        // followed by a consonant and is silent. 1 + 249 * 2 = 499.
        assert_eq!(m.process(&"xy".repeat(250)).len(), 499);
    }

    #[test]
    fn compare_is_key_equality() {
        let m = m();
        assert!(m.compare("phonetics", "fonetix"));
        assert!(m.compare("Smith", "Smyth")); // both SM0: Y before a consonant is silent
        assert!(!m.compare("phonetics", "soundex"));
        assert!(m.compare("", "日本語"));
    }

    #[test]
    fn process_into_appends_and_never_clears() {
        let m = m();
        let mut buf = String::from("keep:");
        m.process_into("fox", &mut buf);
        m.process_into("日本語", &mut buf);
        m.process_into("fox", &mut buf);
        assert_eq!(buf, "keep:FKSFKS");
    }
}
