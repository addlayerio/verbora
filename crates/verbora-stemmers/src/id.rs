//! The Indonesian stemmer — Nazief–Adriani, as refined by Sastrawi.
//!
//! # One rule, one [`RuleResult`]
//!
//! Each affix-removal rule reports two things: the removal it recorded, if any,
//! and the word as it left it. [`RuleResult`] is that pair, and the driver reads
//! nothing else off a rule.
//!
//! # Per-call state is reset once, not per sub-word
//!
//! The removals list is cleared once per [`StemmerId::stem`] call. A
//! reduplicated word then runs the singular stemmer two or three times
//! **without** clearing it again, so the second sub-word inherits the first's
//! removals and the prefix-restore step will happily restore a prefix that was
//! stripped from a different half of the word. That is the specified behaviour:
//! one list per `stem` call, shared by every sub-word inside it.
//!
//! # Regexes that are not anchored
//!
//! Most prefix rules are `^…$`, but not all, and the exceptions change the
//! output rather than merely the match:
//!
//! * Rule 3 (`/ber([C])([a-z])er([aiueo])(.*)/`) has **neither** anchor, so
//!   `"xxberkaerat"` matches at offset 2 and everything before it is discarded:
//!   the rule returns `"kaerat"`.
//! * Rule 5 (`/be([C])(er[C])(.*)$/`) has no `^`.
//! * Rules 2 and 32 have no `$`, and `.` does not match a line terminator, so
//!   `(.*)` stops at the first `\n` and the tail is silently dropped.
//!
//! # `[g|h|q|k]`
//!
//! Rule 16's character class contains a literal `|`, so `"meng|foo"` matches it
//! and reduces to `"|foo"`. Verbora keeps the class as written: reading it as
//! the alternation it resembles would drop `|` from the class and change which
//! words rule 16 fires on.
//!
//! # Disambiguation keeps the LAST attempt, not the first that matched
//!
//! A disambiguated rule tries its sub-rules in order, keeping each sub-rule's
//! result and stopping early only when that result is in the dictionary. When
//! nothing hits, what survives is whatever the **final** sub-rule produced —
//! including "no result at all", which turns the whole rule into a no-op. That
//! makes rules 17 and 30 behave differently on the same shape:
//!
//! ```text
//! rule 17 ("mengasahxyz")  17a,17b miss; 17c (^menge) no result; 17d -> "ngasahxyz"
//! rule 30 ("pengasahxyz")  30a,30b miss; 30c (^penge) no result  -> no-op
//! ```
//!
//! Both are pinned by test. Treating "no sub-rule matched" as the only
//! empty-result case, or returning the first sub-rule that matched, loses this.
//!
//! # The text unit
//!
//! The working buffer holds one **Unicode scalar value** per position, the
//! unit [`crate::units`] states the crate's contract in, and every index in
//! this module — every `w[3]`, every `w[p + 8..]`, `MAX_ROOT_LEN`, and
//! `stem_singular`'s `len() > 3` gate — counts characters.
//!
//! Indonesian is the one stemmer here whose *output* the choice of unit cannot
//! move, and the reason is worth stating because it is a proof rather than an
//! absence of evidence. Two filters stand in front of everything:
//!
//! * [`find`] rejects any word carrying a non-ASCII character before the
//!   dictionary is consulted, and a word is only ever rewritten when some
//!   candidate is *found*.
//! * Every rule is anchored on an ASCII literal at a known offset — `lit_at(w,
//!   0, "ber")`, `lit_at(w, p + 5, "er")` — and every class it tests (`v`,
//!   `az`, `cons`, `cls16`, `cls19`, `cons_35`) is a set of ASCII characters.
//!
//! So a word containing an astral character reaches no rule that can fire and
//! no dictionary entry, and it is returned as it arrived under either reading.
//! The unit-indexed constants are all still here; they are merely unreachable
//! with an astral character present, which is why
//! `an_astral_character_cannot_move_an_indonesian_answer` enumerates the claim
//! instead of asserting it.
//!
//! `U+002D` is one character under either reading, so reduplication —
//! [`split_last_hyphen`], [`is_plural_units`], [`stem_plural`] — is
//! unit-independent too, and every cut those make lands *on* the hyphen, which
//! is a character boundary either way.

use std::borrow::Cow;

use crate::base::{Casing, TokenizeAndStem};
use crate::data::indonesian_dict;
use crate::stopwords::Language;
use crate::units::{eq_str, slen, starts_with, text};

/// The Indonesian stemmer.
///
/// ```
/// use verbora_stemmers::StemmerId;
/// let s = StemmerId::new();
/// assert_eq!(s.stem("hancurlah"), "hancur");
/// assert_eq!(s.stem("mempengaruhi"), "pengaruh");
/// assert_eq!(s.stem("buku-buku"), "buku");
/// assert_eq!(s.stem("malaikat-malaikat-Nya"), "malaikat");
/// ```
///
/// # Reduplication is one word, so `-` does not break a token
///
/// Indonesian marks the plural by reduplicating the root and joining the two
/// halves with `U+002D`, and its orthography treats the result as a single
/// word: `buku-buku` ("books"), `malaikat-malaikat-nya` ("his angels"). This
/// stemmer is built for that — [`Self::is_plural`] answers by looking for the
/// hyphen, and the plural branch of [`Self::stem`] splits on it, stems both
/// halves and keeps the common root.
///
/// The crate's data is built for it too: **335 of the 29,932 roots** in
/// [`Self::dictionary`] and **22 of the 809 Indonesian stop words** are single
/// lexemes spelled with a hyphen — `abal-abal`, `alai-belai`, `tiba-tiba`,
/// `masing-masing`.
///
/// Untailored UAX #29 breaks at `U+002D`, which would hand `stem` two
/// fragments instead of one lexeme and leave every one of those entries
/// unreachable through [`TokenizeAndStem::tokenize_and_stem`] — the plural
/// branch could never run there at all. So this is the one stemmer in the
/// crate that sets [`TokenizeAndStem::HYPHEN_JOINS_LETTERS`]:
///
/// ```
/// use verbora_stemmers::{StemmerId, TokenizeAndStem};
/// let s = StemmerId::new();
/// assert_eq!(s.tokenize_and_stem("buku-buku itu", true), ["buku", "itu"]);
/// // Only between letters: a hyphenated date keeps the default boundaries.
/// assert_eq!(s.tokenize_and_stem("12-05-2020", true), ["12", "05", "2020"]);
/// ```
///
/// A hyphenated word that is *not* a reduplication is not damaged by this:
/// the plural branch stems both halves, finds they disagree, and returns the
/// token exactly as it arrived.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StemmerId;

/// Which kind of affix a [`Removal`] recorded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RemovalKind {
    /// `DP` — a derivational **prefix**, produced by the prefix rules.
    DerivationalPrefix,
    /// `DS` — a derivational suffix (`is`, `isme`, `isasi`, `i`, `kan`, `an`).
    DerivationalSuffix,
    /// `PP` — an inflectional possessive pronoun (`ku`, `mu`, `nya`).
    PossessivePronoun,
    /// `P` — an inflectional particle (`lah`, `kah`, `tah`, `pun`).
    Particle,
}

impl RemovalKind {
    /// The two-letter code Nazief–Adriani names this affix class by, as it
    /// appears in the literature: `DP`, `DS`, `PP`, `P`.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::DerivationalPrefix => "DP",
            Self::DerivationalSuffix => "DS",
            Self::PossessivePronoun => "PP",
            Self::Particle => "P",
        }
    }

    /// Whether this is a suffix removal: everything except a derivational prefix.
    #[must_use]
    pub const fn is_suffix(self) -> bool {
        !matches!(self, Self::DerivationalPrefix)
    }
}

/// One recorded affix removal — `indonesian/removal`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Removal {
    original: Vec<char>,
    result: Vec<char>,
    removed: Vec<char>,
    kind: RemovalKind,
}

impl Removal {
    /// The word as it stood before this removal.
    #[must_use]
    pub fn original_word(&self) -> String {
        text(&self.original)
    }
    /// The word as this removal left it.
    #[must_use]
    pub fn result(&self) -> String {
        text(&self.result)
    }
    /// The part this removal took out.
    ///
    /// Computed as `word.replace(result, '')` — the **first** occurrence of the
    /// result *anywhere* in the word, deleted. For a prefix rule that rewrites
    /// rather than truncates this is rarely the affix: `"mengeboran" ->
    /// "ngeboran"` records a removed part of `"me"` only because `"ngeboran"`
    /// does not occur in `"mengeboran"` at all, so nothing is deleted and the
    /// whole word is reported.
    #[must_use]
    pub fn removed_part(&self) -> String {
        text(&self.removed)
    }
    /// Which kind of affix this removal took out.
    #[must_use]
    pub const fn affix_type(&self) -> RemovalKind {
        self.kind
    }
}

/// What one affix-removal rule produced: the removal it recorded, if any,
/// and the word as the rule left it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleResult {
    /// The removal this rule recorded, or `None` when it matched nothing.
    pub removal: Option<Removal>,
    /// The word as the rule left it.
    pub current_word: String,
}

// ---------------------------------------------------------------------------
// Dictionary
// ---------------------------------------------------------------------------

/// `dictionary.has(word)`.
///
/// Every root is ASCII, so a word carrying any non-ASCII character cannot be in
/// the set and never reaches the binary search. This runs on every rule
/// application — `stemming_process` calls it after each suffix and prefix rule —
/// so it is one of the hottest paths in the stemmer.
///
/// The longest dictionary entry is 20 characters, so a stack buffer covers every
/// word that could possibly be found; longer inputs fall back to a heap buffer
/// purely for correctness (an unfindable word must still be looked up, not just
/// assumed absent) rather than for speed.
fn find(w: &[char]) -> bool {
    // The longest root is 20 characters (`ekstrateritorialitas`), so anything
    // longer is absent without a lookup — and, since every root is ASCII, so
    // is anything carrying a non-ASCII character. Both are pure filters on a
    // function that has no side effects, so an early `false` is exactly what
    // the search would have returned.
    //
    // The two filters are also what makes the narrowing below sound: past
    // them every character of `w` is ASCII, so it is one byte and the byte is
    // its own code point.
    if w.len() > MAX_ROOT_LEN || w.iter().any(|&c| !c.is_ascii()) {
        return false;
    }
    let mut bytes = [0u8; MAX_ROOT_LEN];
    for (dst, &c) in bytes.iter_mut().zip(w) {
        *dst = c as u8;
    }
    DICT.contains(&bytes[..w.len()])
}

/// The length of the longest dictionary root, pinned by
/// `the_dictionary_is_the_expected_size`.
///
/// Every root is ASCII, so this one number is the entry's length in bytes and
/// in characters alike — which is what lets [`find`] compare it against a
/// character count and then index a byte array with the same value.
const MAX_ROOT_LEN: usize = 20;

/// An open-addressed hash index over [`indonesian_dict::SORTED`].
///
/// # Why not `binary_search`
///
/// Nazief–Adriani is *driven* by dictionary membership rather than merely
/// checked against it: every affix-removal candidate, every disambiguator
/// alternative and every restored prefix is looked up, which measured at
/// **28.2 lookups per stemmed word** over the bench corpus. Against 29,932
/// entries a binary search is ~15 probes, and each probe is two dependent
/// loads — one into the 480 KB pointer array, one into the scattered string
/// data — so a single word chased roughly 420 dependent cache misses. That,
/// and not the rule engine, was the entire gap to `sastrawi` (which reaches
/// for a `HashMap`): the stemmer ran 6.8× slower than it and the rules
/// themselves were never the problem.
///
/// # Shape
///
/// One `u32` per slot, packing a 16-bit hash tag above a 16-bit
/// `index + 1` into `SORTED` (`0` marks an empty slot, and an occupied one
/// is never `0` because the index is biased). The tag settles almost every
/// probe without touching the string data at all, so a hit costs one load
/// from the 256 KB slot array plus one string compare, and a miss usually
/// costs just the one load. 65,536 slots for 29,932 entries keeps the load
/// factor at 0.46, where linear probing stays short.
struct DictIndex {
    slots: Vec<u32>,
}

/// Slots in [`DictIndex`]; a power of two so the modulus is a mask.
const DICT_SLOTS: usize = 1 << 16;
const DICT_MASK: usize = DICT_SLOTS - 1;

/// FNV-1a over the ASCII bytes of a root.
///
/// Chosen over the standard library's `SipHash` because the keys are three
/// to twenty bytes and are hashed tens of times per word: at that size the
/// per-byte multiply-xor beats SipHash's setup, and the quality needed is
/// only "spreads 29,932 short lowercase Latin strings", which FNV does.
#[inline]
fn dict_hash(bytes: &[u8]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325_u64;
    for &c in bytes {
        h ^= u64::from(c);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

impl DictIndex {
    fn build() -> DictIndex {
        // The real bound is 16 bits, not 32. A slot packs a 16-bit hash tag in
        // its high half and the biased index in its low half, and the probe
        // loop below terminates only while some slot is still free — so the
        // dictionary must satisfy `len + 1 < DICT_SLOTS == 1 << 16` on both
        // counts. Checked once per process, always (not `debug_assert!`),
        // because the release-build failure modes are silent: a 17-bit index
        // would corrupt the tag it shares the slot with and make `contains`
        // answer wrongly, and a full table would spin here forever. The
        // shipped dictionary is 29,932 roots, less than half the ceiling.
        assert!(
            indonesian_dict::SORTED.len() + 1 < DICT_SLOTS,
            "the Indonesian root dictionary outgrew its 16-bit index: {} roots, ceiling {}",
            indonesian_dict::SORTED.len(),
            DICT_SLOTS - 2
        );
        let mut slots = vec![0u32; DICT_SLOTS];
        for (i, word) in indonesian_dict::SORTED.iter().enumerate() {
            let h = dict_hash(word.as_bytes());
            let tag = (h >> 48) as u32;
            // Lossless by the assertion above: `i + 1 <= SORTED.len() + 1`,
            // which is below `1 << 16`.
            let biased = (i + 1) as u32;
            let mut p = (h as usize) & DICT_MASK;
            while slots[p] != 0 {
                p = (p + 1) & DICT_MASK;
            }
            slots[p] = (tag << 16) | biased;
        }
        DictIndex { slots }
    }

    /// Whether `bytes` is a dictionary root.
    #[inline]
    fn contains(&self, bytes: &[u8]) -> bool {
        let h = dict_hash(bytes);
        let tag = (h >> 48) as u32;
        let mut p = (h as usize) & DICT_MASK;
        loop {
            let slot = self.slots[p];
            if slot == 0 {
                return false;
            }
            if slot >> 16 == tag
                && indonesian_dict::SORTED[(slot & 0xFFFF) as usize - 1].as_bytes() == bytes
            {
                return true;
            }
            p = (p + 1) & DICT_MASK;
        }
    }
}

static DICT: std::sync::LazyLock<DictIndex> = std::sync::LazyLock::new(DictIndex::build);

// ---------------------------------------------------------------------------
// Small scanners
// ---------------------------------------------------------------------------

/// `[aiueo]`.
#[inline]
fn v(c: char) -> bool {
    matches!(c, 'a' | 'i' | 'u' | 'e' | 'o')
}
/// `[a-z]`.
#[inline]
fn az(c: char) -> bool {
    c.is_ascii_lowercase()
}
/// `[bcdfghjklmnpqrstvwxyz]` — every lowercase consonant.
#[inline]
fn cons(c: char) -> bool {
    az(c) && !v(c)
}
/// `[bcdfghjklmnpqstvwxyz]` — rule 5's class, which omits `r`.
#[inline]
fn cons_no_r(c: char) -> bool {
    cons(c) && c != 'r'
}
/// `[bcdfghjkpqstvxz]` — rules 35 and 36.
#[inline]
fn cons_35(c: char) -> bool {
    matches!(
        c,
        'b' | 'c' | 'd' | 'f' | 'g' | 'h' | 'j' | 'k' | 'p' | 'q' | 's' | 't' | 'v' | 'x' | 'z'
    )
}
/// `[abcdfghijklmopqrstuvwxyz]` — rule 19's class, which omits `e` and `n`.
#[inline]
fn cls19(c: char) -> bool {
    az(c) && c != 'e' && c != 'n'
}
/// `[g|h|q|k]` — the literal `|` is part of the class, not an alternation.
#[inline]
fn cls16(c: char) -> bool {
    matches!(c, 'g' | '|' | 'h' | 'q' | 'k')
}

/// How far `.` can run from `at`: to the first line terminator.
fn dot_end(w: &[char], at: usize) -> usize {
    (at..w.len())
        .find(|&i| matches!(w[i], '\n' | '\r' | '\u{2028}' | '\u{2029}'))
        .unwrap_or(w.len())
}

/// Whether `(.*)$` can consume `w[at..]` in one piece.
fn dot_to_end(w: &[char], at: usize) -> bool {
    dot_end(w, at) == w.len()
}

/// Whether `w[at..]` begins with the ASCII literal `lit`.
fn lit_at(w: &[char], at: usize, lit: &str) -> bool {
    at <= w.len() && starts_with(&w[at..], lit)
}

/// Concatenates ASCII literals and slices into one buffer.
macro_rules! cat {
    ($($part:expr),+ $(,)?) => {{
        let mut out: Vec<char> = Vec::new();
        $( $part.append_to(&mut out); )+
        out
    }};
}

/// Lets `cat!` take `&str`, `&[char]` and a bare [`char`].
trait Append {
    fn append_to(&self, out: &mut Vec<char>);
}
impl Append for &str {
    fn append_to(&self, out: &mut Vec<char>) {
        out.extend(self.chars());
    }
}
impl Append for &[char] {
    fn append_to(&self, out: &mut Vec<char>) {
        out.extend_from_slice(self);
    }
}
impl Append for char {
    fn append_to(&self, out: &mut Vec<char>) {
        out.push(*self);
    }
}

/// `word.replace(needle, '')`: delete the **first** occurrence of a literal.
///
/// An empty needle is found at index 0 and deletes nothing, so the word comes
/// back unchanged — which is how a rule that reduces a word to `""` still
/// records the whole word as its removed part.
fn delete_first(w: &[char], needle: &[char]) -> Vec<char> {
    if needle.is_empty() || needle.len() > w.len() {
        return w.to_vec();
    }
    for i in 0..=w.len() - needle.len() {
        if &w[i..i + needle.len()] == needle {
            let mut out = w[..i].to_vec();
            out.extend_from_slice(&w[i + needle.len()..]);
            return out;
        }
    }
    w.to_vec()
}

// ---------------------------------------------------------------------------
// Suffix rules
// ---------------------------------------------------------------------------

/// `/-*(alt)$/`: the start of the leftmost match, dashes included.
fn dashed_suffix(w: &[char], alts: &[&str]) -> Option<usize> {
    let mut best: Option<usize> = None;
    for a in alts {
        let n = slen(a);
        if n <= w.len() && lit_at(w, w.len() - n, a) {
            let mut start = w.len() - n;
            while start > 0 && w[start - 1] == '-' {
                start -= 1;
            }
            if best.is_none_or(|b| start < b) {
                best = Some(start);
            }
        }
    }
    best
}

/// `/(alt)$/` with no dashes: the longest listed suffix.
fn plain_suffix(w: &[char], alts: &[&str]) -> Option<usize> {
    let mut best: Option<usize> = None;
    for a in alts {
        let n = slen(a);
        if n <= w.len() && lit_at(w, w.len() - n, a) {
            let start = w.len() - n;
            if best.is_none_or(|b| start < b) {
                best = Some(start);
            }
        }
    }
    best
}

/// A suffix rule's result: no removal when nothing changed.
///
/// Takes the cut point rather than an already-built result so that the
/// unchanged case — by far the common one, since each rule is tried against
/// every word — costs no allocation at all. `word[..cut]` equals `word` exactly
/// when `cut` is `word.len()`, so comparing the cut point against the length is
/// the same test as comparing the two words.
fn suffix_result(
    cut: usize,
    word: &[char],
    kind: RemovalKind,
) -> (Option<Removal>, Option<Vec<char>>) {
    if cut == word.len() {
        return (None, None);
    }
    let result = word[..cut].to_vec();
    let removed = delete_first(word, &result);
    (
        Some(Removal {
            original: word.to_vec(),
            result: result.clone(),
            removed,
            kind,
        }),
        Some(result),
    )
}

/// `/-*(lah|kah|tah|pun)$/` — the inflectional particles.
///
/// Named rather than written inline so that the affix tables have exactly one
/// spelling each and a test can walk them; see
/// `every_affix_table_entry_measures_the_same_as_the_buffer`.
static PARTICLES: &[&str] = &["lah", "kah", "tah", "pun"];
/// `/-*(ku|mu|nya)$/` — the inflectional possessive pronouns.
static POSSESSIVES: &[&str] = &["ku", "mu", "nya"];
/// `/(is|isme|isasi|i|kan|an)$/` — the derivational suffixes.
static DERIVATIONAL_SUFFIXES: &[&str] = &["is", "isme", "isasi", "i", "kan", "an"];
/// `/^(di|ke|se)/` — `RemovePlainPrefix`'s alternation.
static PLAIN_PREFIXES: &[&str] = &["di", "ke", "se"];

fn remove_particle(w: &[char]) -> (Option<Removal>, Option<Vec<char>>) {
    let cut = dashed_suffix(w, PARTICLES).unwrap_or(w.len());
    suffix_result(cut, w, RemovalKind::Particle)
}

fn remove_possessive(w: &[char]) -> (Option<Removal>, Option<Vec<char>>) {
    let cut = dashed_suffix(w, POSSESSIVES).unwrap_or(w.len());
    suffix_result(cut, w, RemovalKind::PossessivePronoun)
}

fn remove_derivational_suffix(w: &[char]) -> (Option<Removal>, Option<Vec<char>>) {
    let cut = plain_suffix(w, DERIVATIONAL_SUFFIXES).unwrap_or(w.len());
    suffix_result(cut, w, RemovalKind::DerivationalSuffix)
}

/// The three suffix rules, in `SuffixRules.rules` order. `None` for the new
/// word means the rule left it alone; see [`suffix_result`].
type SuffixRule = fn(&[char]) -> (Option<Removal>, Option<Vec<char>>);
static SUFFIX_RULES: &[SuffixRule] = &[
    remove_particle,
    remove_possessive,
    remove_derivational_suffix,
];

// ---------------------------------------------------------------------------
// Prefix rules
// ---------------------------------------------------------------------------

/// One disambiguation attempt: `undefined` when its pattern does not match.
type SubRule = fn(&[char]) -> Option<Vec<char>>;

/// A prefix rule, as `PrefixRules.rules` holds them.
enum PrefixRule {
    /// `RemovePlainPrefix` — the only one that can record *no* removal.
    Plain,
    /// A disambiguated rule over an ordered list of attempts.
    Dis(&'static [SubRule]),
}

fn r1a(w: &[char]) -> Option<Vec<char>> {
    (lit_at(w, 0, "ber") && w.len() > 3 && v(w[3]) && dot_to_end(w, 4)).then(|| w[3..].to_vec())
}
fn r1b(w: &[char]) -> Option<Vec<char>> {
    (lit_at(w, 0, "ber") && w.len() > 3 && v(w[3]) && dot_to_end(w, 4)).then(|| cat!("r", &w[3..]))
}
/// Rule 2. No `$`, so `(.*)` stops at the first line terminator.
fn r2(w: &[char]) -> Option<Vec<char>> {
    if !(lit_at(w, 0, "ber") && w.len() > 4 && cons(w[3]) && az(w[4])) {
        return None;
    }
    let g3 = &w[5..dot_end(w, 5)];
    // `P != 'er'`
    if lit_at(g3, 0, "er") {
        return None;
    }
    Some(cat!(w[3], w[4], g3))
}
/// Rule 3. Neither anchor, so anything before the match is discarded.
fn r3(w: &[char]) -> Option<Vec<char>> {
    let p = (0..w.len()).find(|&p| {
        lit_at(w, p, "ber")
            && p + 7 < w.len()
            && cons(w[p + 3])
            && az(w[p + 4])
            && lit_at(w, p + 5, "er")
            && v(w[p + 7])
    })?;
    // `C != 'r'` is checked *after* the match, so a `berr…` word yields nothing
    // rather than retrying at a later offset.
    if w[p + 3] == 'r' {
        return None;
    }
    let g4 = &w[p + 8..dot_end(w, p + 8)];
    Some(cat!(w[p + 3], w[p + 4], "er", w[p + 7], g4))
}
fn r4(w: &[char]) -> Option<Vec<char>> {
    eq_str(w, "belajar").then(|| "ajar".chars().collect())
}
/// Rule 5. No `^`, so it may start anywhere; `$` forces it to reach the end.
fn r5(w: &[char]) -> Option<Vec<char>> {
    let p = (0..w.len()).find(|&p| {
        lit_at(w, p, "be")
            && p + 5 < w.len()
            && cons_no_r(w[p + 2])
            && lit_at(w, p + 3, "er")
            && cons(w[p + 5])
            && dot_to_end(w, p + 6)
    })?;
    Some(w[p + 2..].to_vec())
}
fn r6a(w: &[char]) -> Option<Vec<char>> {
    (lit_at(w, 0, "ter") && w.len() > 3 && v(w[3]) && dot_to_end(w, 4)).then(|| w[3..].to_vec())
}
fn r6b(w: &[char]) -> Option<Vec<char>> {
    (lit_at(w, 0, "ter") && w.len() > 3 && v(w[3]) && dot_to_end(w, 4)).then(|| cat!("r", &w[3..]))
}
fn r7(w: &[char]) -> Option<Vec<char>> {
    if !(lit_at(w, 0, "ter")
        && w.len() > 6
        && cons(w[3])
        && lit_at(w, 4, "er")
        && v(w[6])
        && dot_to_end(w, 7))
    {
        return None;
    }
    if w[3] == 'r' {
        return None;
    }
    Some(cat!(w[3], "er", &w[6..]))
}
fn r8(w: &[char]) -> Option<Vec<char>> {
    if !(lit_at(w, 0, "ter") && w.len() > 3 && cons(w[3]) && dot_to_end(w, 4)) {
        return None;
    }
    if w[3] == 'r' || lit_at(w, 4, "er") {
        return None;
    }
    Some(w[3..].to_vec())
}
fn r9(w: &[char]) -> Option<Vec<char>> {
    if !(lit_at(w, 0, "te")
        && w.len() > 5
        && cons(w[2])
        && lit_at(w, 3, "er")
        && cons(w[5])
        && dot_to_end(w, 6))
    {
        return None;
    }
    if w[2] == 'r' {
        return None;
    }
    Some(w[2..].to_vec())
}
fn r10(w: &[char]) -> Option<Vec<char>> {
    (lit_at(w, 0, "me")
        && w.len() > 3
        && matches!(w[2], 'l' | 'r' | 'w' | 'y')
        && v(w[3])
        && dot_to_end(w, 4))
    .then(|| w[2..].to_vec())
}
fn r11(w: &[char]) -> Option<Vec<char>> {
    (lit_at(w, 0, "mem") && w.len() > 3 && matches!(w[3], 'b' | 'f' | 'v') && dot_to_end(w, 4))
        .then(|| w[3..].to_vec())
}
fn r12(w: &[char]) -> Option<Vec<char>> {
    (lit_at(w, 0, "mempe") && dot_to_end(w, 5)).then(|| cat!("pe", &w[5..]))
}
fn r13a(w: &[char]) -> Option<Vec<char>> {
    (lit_at(w, 0, "mem") && w.len() > 3 && v(w[3]) && dot_to_end(w, 4)).then(|| cat!("m", &w[3..]))
}
fn r13b(w: &[char]) -> Option<Vec<char>> {
    (lit_at(w, 0, "mem") && w.len() > 3 && v(w[3]) && dot_to_end(w, 4)).then(|| cat!("p", &w[3..]))
}
fn r14(w: &[char]) -> Option<Vec<char>> {
    (lit_at(w, 0, "men")
        && w.len() > 3
        && matches!(w[3], 'c' | 'd' | 'j' | 's' | 't' | 'z')
        && dot_to_end(w, 4))
    .then(|| w[3..].to_vec())
}
fn r15a(w: &[char]) -> Option<Vec<char>> {
    (lit_at(w, 0, "men") && w.len() > 3 && v(w[3]) && dot_to_end(w, 4)).then(|| cat!("n", &w[3..]))
}
fn r15b(w: &[char]) -> Option<Vec<char>> {
    (lit_at(w, 0, "men") && w.len() > 3 && v(w[3]) && dot_to_end(w, 4)).then(|| cat!("t", &w[3..]))
}
fn r16(w: &[char]) -> Option<Vec<char>> {
    (lit_at(w, 0, "meng") && w.len() > 4 && cls16(w[4]) && dot_to_end(w, 5))
        .then(|| w[4..].to_vec())
}
fn r17a(w: &[char]) -> Option<Vec<char>> {
    (lit_at(w, 0, "meng") && w.len() > 4 && v(w[4]) && dot_to_end(w, 5)).then(|| w[4..].to_vec())
}
fn r17b(w: &[char]) -> Option<Vec<char>> {
    (lit_at(w, 0, "meng") && w.len() > 4 && v(w[4]) && dot_to_end(w, 5)).then(|| cat!("k", &w[4..]))
}
fn r17c(w: &[char]) -> Option<Vec<char>> {
    (lit_at(w, 0, "menge") && dot_to_end(w, 5)).then(|| w[5..].to_vec())
}
fn r17d(w: &[char]) -> Option<Vec<char>> {
    (lit_at(w, 0, "meng") && w.len() > 4 && v(w[4]) && dot_to_end(w, 5))
        .then(|| cat!("ng", &w[4..]))
}
fn r18a(w: &[char]) -> Option<Vec<char>> {
    (lit_at(w, 0, "meny") && w.len() > 4 && v(w[4]) && dot_to_end(w, 5))
        .then(|| cat!("ny", &w[4..]))
}
fn r18b(w: &[char]) -> Option<Vec<char>> {
    (lit_at(w, 0, "meny") && w.len() > 4 && v(w[4]) && dot_to_end(w, 5)).then(|| cat!("s", &w[4..]))
}
fn r19(w: &[char]) -> Option<Vec<char>> {
    (lit_at(w, 0, "memp") && w.len() > 4 && cls19(w[4]) && dot_to_end(w, 5))
        .then(|| cat!("p", &w[4..]))
}
fn r20(w: &[char]) -> Option<Vec<char>> {
    (lit_at(w, 0, "pe") && w.len() > 3 && matches!(w[2], 'w' | 'y') && v(w[3]) && dot_to_end(w, 4))
        .then(|| w[2..].to_vec())
}
fn r21a(w: &[char]) -> Option<Vec<char>> {
    (lit_at(w, 0, "per") && w.len() > 3 && v(w[3]) && dot_to_end(w, 4)).then(|| w[3..].to_vec())
}
fn r21b(w: &[char]) -> Option<Vec<char>> {
    (lit_at(w, 0, "pe") && w.len() > 3 && w[2] == 'r' && v(w[3]) && dot_to_end(w, 4))
        .then(|| w[2..].to_vec())
}
fn r23(w: &[char]) -> Option<Vec<char>> {
    if !(lit_at(w, 0, "per") && w.len() > 4 && cons(w[3]) && az(w[4]) && dot_to_end(w, 5)) {
        return None;
    }
    if lit_at(w, 5, "er") {
        return None;
    }
    Some(w[3..].to_vec())
}
fn r24(w: &[char]) -> Option<Vec<char>> {
    if !(lit_at(w, 0, "per")
        && w.len() > 7
        && cons(w[3])
        && az(w[4])
        && lit_at(w, 5, "er")
        && v(w[7])
        && dot_to_end(w, 8))
    {
        return None;
    }
    if w[3] == 'r' {
        return None;
    }
    Some(w[3..].to_vec())
}
fn r25(w: &[char]) -> Option<Vec<char>> {
    (lit_at(w, 0, "pem") && w.len() > 3 && matches!(w[3], 'b' | 'f' | 'v') && dot_to_end(w, 4))
        .then(|| w[3..].to_vec())
}
fn r26a(w: &[char]) -> Option<Vec<char>> {
    (lit_at(w, 0, "pem") && w.len() > 3 && v(w[3]) && dot_to_end(w, 4)).then(|| cat!("m", &w[3..]))
}
fn r26b(w: &[char]) -> Option<Vec<char>> {
    (lit_at(w, 0, "pem") && w.len() > 3 && v(w[3]) && dot_to_end(w, 4)).then(|| cat!("p", &w[3..]))
}
fn r27(w: &[char]) -> Option<Vec<char>> {
    (lit_at(w, 0, "pen")
        && w.len() > 3
        && matches!(w[3], 'c' | 'd' | 'j' | 's' | 't' | 'z')
        && dot_to_end(w, 4))
    .then(|| w[3..].to_vec())
}
fn r28a(w: &[char]) -> Option<Vec<char>> {
    (lit_at(w, 0, "pen") && w.len() > 3 && v(w[3]) && dot_to_end(w, 4)).then(|| cat!("n", &w[3..]))
}
fn r28b(w: &[char]) -> Option<Vec<char>> {
    (lit_at(w, 0, "pen") && w.len() > 3 && v(w[3]) && dot_to_end(w, 4)).then(|| cat!("t", &w[3..]))
}
fn r29(w: &[char]) -> Option<Vec<char>> {
    (lit_at(w, 0, "peng") && w.len() > 4 && cons(w[4]) && dot_to_end(w, 5)).then(|| w[4..].to_vec())
}
fn r30a(w: &[char]) -> Option<Vec<char>> {
    (lit_at(w, 0, "peng") && w.len() > 4 && v(w[4]) && dot_to_end(w, 5)).then(|| w[4..].to_vec())
}
fn r30b(w: &[char]) -> Option<Vec<char>> {
    (lit_at(w, 0, "peng") && w.len() > 4 && v(w[4]) && dot_to_end(w, 5)).then(|| cat!("k", &w[4..]))
}
fn r30c(w: &[char]) -> Option<Vec<char>> {
    (lit_at(w, 0, "penge") && dot_to_end(w, 5)).then(|| w[5..].to_vec())
}
fn r31a(w: &[char]) -> Option<Vec<char>> {
    (lit_at(w, 0, "peny") && w.len() > 4 && v(w[4]) && dot_to_end(w, 5))
        .then(|| cat!("ny", &w[4..]))
}
fn r31b(w: &[char]) -> Option<Vec<char>> {
    (lit_at(w, 0, "peny") && w.len() > 4 && v(w[4]) && dot_to_end(w, 5)).then(|| cat!("s", &w[4..]))
}
/// Rule 32. The `pelajar` special case, then `^pe(l[aiueo])(.*)` — no `$`.
fn r32(w: &[char]) -> Option<Vec<char>> {
    if eq_str(w, "pelajar") {
        return Some("ajar".chars().collect());
    }
    (lit_at(w, 0, "pe") && w.len() > 3 && w[2] == 'l' && v(w[3]))
        .then(|| w[2..dot_end(w, 4)].to_vec())
}
fn r34(w: &[char]) -> Option<Vec<char>> {
    if !(lit_at(w, 0, "pe") && w.len() > 2 && cons(w[2]) && dot_to_end(w, 3)) {
        return None;
    }
    if lit_at(w, 3, "er") {
        return None;
    }
    Some(w[2..].to_vec())
}
fn r35(w: &[char]) -> Option<Vec<char>> {
    (lit_at(w, 0, "ter")
        && w.len() > 6
        && cons_35(w[3])
        && lit_at(w, 4, "er")
        && cons(w[6])
        && dot_to_end(w, 7))
    .then(|| w[3..].to_vec())
}
fn r36(w: &[char]) -> Option<Vec<char>> {
    (lit_at(w, 0, "pe")
        && w.len() > 5
        && cons_35(w[2])
        && lit_at(w, 3, "er")
        && cons(w[5])
        && dot_to_end(w, 6))
    .then(|| w[2..].to_vec())
}

/// `^(C)(XY[aiueo])(.*)$` for the four `CerV`/`CelV`/`CemV`/`CinV` pairs.
///
/// The `a` variant re-emits the word unchanged; the `b` variant drops the
/// infix. Returning the input unchanged still records a `Removal` — a prefix
/// rule records one unconditionally — whose removed part is `""`, and that
/// non-empty removals list is what stops the prefix-rule walk.
fn infix(w: &[char], infix: &str, keep: bool) -> Option<Vec<char>> {
    if !(w.len() > 3 && cons(w[0]) && lit_at(w, 1, infix) && v(w[3]) && dot_to_end(w, 4)) {
        return None;
    }
    Some(if keep {
        w.to_vec()
    } else {
        cat!(w[0], &w[3..])
    })
}
fn r37a(w: &[char]) -> Option<Vec<char>> {
    infix(w, "er", true)
}
fn r37b(w: &[char]) -> Option<Vec<char>> {
    infix(w, "er", false)
}
fn r38a(w: &[char]) -> Option<Vec<char>> {
    infix(w, "el", true)
}
fn r38b(w: &[char]) -> Option<Vec<char>> {
    infix(w, "el", false)
}
fn r39a(w: &[char]) -> Option<Vec<char>> {
    infix(w, "em", true)
}
fn r39b(w: &[char]) -> Option<Vec<char>> {
    infix(w, "em", false)
}
fn r40a(w: &[char]) -> Option<Vec<char>> {
    infix(w, "in", true)
}
fn r40b(w: &[char]) -> Option<Vec<char>> {
    infix(w, "in", false)
}
fn r41(w: &[char]) -> Option<Vec<char>> {
    (lit_at(w, 0, "ku") && dot_to_end(w, 2)).then(|| w[2..].to_vec())
}
fn r42(w: &[char]) -> Option<Vec<char>> {
    (lit_at(w, 0, "kau") && dot_to_end(w, 3)).then(|| w[3..].to_vec())
}

/// The prefix rules, in order. Rules 22, 33 and 43 do not exist — the
/// published numbering has gaps.
static PREFIX_RULES: &[PrefixRule] = &[
    PrefixRule::Plain,
    PrefixRule::Dis(&[r1a, r1b]),
    PrefixRule::Dis(&[r2]),
    PrefixRule::Dis(&[r3]),
    PrefixRule::Dis(&[r4]),
    PrefixRule::Dis(&[r5]),
    PrefixRule::Dis(&[r6a, r6b]),
    PrefixRule::Dis(&[r7]),
    PrefixRule::Dis(&[r8]),
    PrefixRule::Dis(&[r9]),
    PrefixRule::Dis(&[r10]),
    PrefixRule::Dis(&[r11]),
    PrefixRule::Dis(&[r12]),
    PrefixRule::Dis(&[r13a, r13b]),
    PrefixRule::Dis(&[r14]),
    PrefixRule::Dis(&[r15a, r15b]),
    PrefixRule::Dis(&[r16]),
    PrefixRule::Dis(&[r17a, r17b, r17c, r17d]),
    PrefixRule::Dis(&[r18a, r18b]),
    PrefixRule::Dis(&[r19]),
    PrefixRule::Dis(&[r20]),
    PrefixRule::Dis(&[r21a, r21b]),
    PrefixRule::Dis(&[r23]),
    PrefixRule::Dis(&[r24]),
    PrefixRule::Dis(&[r25]),
    PrefixRule::Dis(&[r26a, r26b]),
    PrefixRule::Dis(&[r27]),
    PrefixRule::Dis(&[r28a, r28b]),
    PrefixRule::Dis(&[r29]),
    PrefixRule::Dis(&[r30a, r30b, r30c]),
    PrefixRule::Dis(&[r31a, r31b]),
    PrefixRule::Dis(&[r32]),
    PrefixRule::Dis(&[r34]),
    PrefixRule::Dis(&[r35]),
    PrefixRule::Dis(&[r36]),
    PrefixRule::Dis(&[r37a, r37b]),
    PrefixRule::Dis(&[r38a, r38b]),
    PrefixRule::Dis(&[r39a, r39b]),
    PrefixRule::Dis(&[r40a, r40b]),
    PrefixRule::Dis(&[r41]),
    PrefixRule::Dis(&[r42]),
];

impl PrefixRule {
    /// Applies the rule, returning the removal it recorded (if any) and the
    /// new word — `None` when the rule left the word alone.
    ///
    /// # Why the word is optional
    ///
    /// The prefix-rule walk runs every rule in turn until one records a
    /// removal, so a word is tried against dozens of rules that do not match
    /// it. Returning the unchanged word by value made each of those a full
    /// copy of the buffer, and they dominated: the stemmer allocated 40.4
    /// times per word where the Snowball stemmers here allocate 0.1 to 0.4.
    /// `None` says "keep what you have", and the driver then simply does not
    /// assign.
    fn apply(&self, w: &[char]) -> (Option<Removal>, Option<Vec<char>>) {
        match self {
            Self::Plain => {
                // `word.replace(/^(di|ke|se)/, '')`. A matched prefix always
                // shortens the word by two characters, so "the word changed"
                // and "a prefix matched" are the same question.
                if !PLAIN_PREFIXES.iter().any(|p| lit_at(w, 0, p)) {
                    return (None, None);
                }
                let result = w[2..].to_vec();
                let removed = delete_first(w, &result);
                (
                    Some(Removal {
                        original: w.to_vec(),
                        result: result.clone(),
                        removed,
                        kind: RemovalKind::DerivationalPrefix,
                    }),
                    Some(result),
                )
            }
            Self::Dis(rules) => {
                // `result` keeps whatever the LAST attempt returned unless one
                // hit the dictionary first — including `undefined`.
                let mut result: Option<Vec<char>> = None;
                for r in *rules {
                    result = r(w);
                    if result.as_deref().is_some_and(find) {
                        break;
                    }
                }
                let Some(result) = result else {
                    return (None, None);
                };
                // A prefix rule records a removal unconditionally: it records a
                // removal even when the result equals the input.
                let removed = delete_first(w, &result);
                (
                    Some(Removal {
                        original: w.to_vec(),
                        result: result.clone(),
                        removed,
                        kind: RemovalKind::DerivationalPrefix,
                    }),
                    Some(result),
                )
            }
        }
    }
}

// ---------------------------------------------------------------------------
// The driver
// ---------------------------------------------------------------------------

/// The removals list, the original word and the current word, per `stem` call.
struct State {
    removals: Vec<Removal>,
    original: Vec<char>,
    current: Vec<char>,
}

impl State {
    fn found(&self) -> bool {
        find(&self.current)
    }

    /// Suffix removal: particle, then possessive, then derivational suffix.
    fn remove_suffixes(&mut self) {
        for rule in SUFFIX_RULES {
            let (removal, next) = rule(&self.current);
            if let Some(r) = removal {
                self.removals.push(r);
            }
            if let Some(next) = next {
                self.current = next;
            }
            if self.found() {
                return;
            }
        }
    }

    /// Prefix removal: three passes of [`Self::check_prefix_rules`].
    fn remove_prefixes(&mut self) {
        for _ in 0..3 {
            self.check_prefix_rules();
            if self.found() {
                return;
            }
        }
    }

    /// Stops at the first rule that records a removal.
    fn check_prefix_rules(&mut self) {
        let before = self.removals.len();
        for rule in PREFIX_RULES {
            let (removal, next) = rule.apply(&self.current);
            if let Some(r) = removal {
                self.removals.push(r);
            }
            if let Some(next) = next {
                self.current = next;
            }
            if self.found() || self.removals.len() > before {
                return;
            }
        }
    }

    /// Rewinds to the last removal's original word, then forgets
    /// every prefix removal.
    fn restore_prefix(&mut self) {
        // The rewind assigns from every removal in turn, so the LAST one wins.
        if let Some(last) = self.removals.last() {
            self.current = last.original.clone();
        }
        self.removals
            .retain(|r| r.kind != RemovalKind::DerivationalPrefix);
    }

    /// The ECS restore loop.
    fn loop_restore_prefixes(&mut self) {
        self.restore_prefix();
        self.removals.reverse();
        let temp = self.current.clone();

        // `for (const i in reversedRemovals)` snapshots the index keys, so the
        // removals that [`Self::remove_prefixes`] appends below are never visited.
        let n = self.removals.len();
        for i in 0..n {
            // The kind is `Copy`, so the skip is decided before anything is
            // cloned — most entries are prefix removals and never needed the
            // three buffers a `Removal` carries.
            if !self.removals[i].kind.is_suffix() {
                continue;
            }
            let removal = self.removals[i].clone();
            if eq_str(&removal.removed, "kan") {
                self.current = cat!(removal.result.as_slice(), "k");
                self.remove_prefixes();
                if self.found() {
                    return;
                }
                self.current = cat!(removal.result.as_slice(), "kan");
            } else {
                self.current = removal.original.clone();
            }
            self.remove_prefixes();
            if self.found() {
                return;
            }
            self.current.clone_from(&temp);
        }
    }

    /// The stemming process — steps 2 through 5.
    fn stemming_process(&mut self) {
        if self.found() {
            return;
        }
        if precedence_adjustment(&self.original) {
            self.remove_prefixes();
            if self.found() {
                return;
            }
            self.remove_suffixes();
            if self.found() {
                return;
            }
            self.current.clone_from(&self.original);
            self.removals.clear();
        }
        self.remove_suffixes();
        if self.found() {
            return;
        }
        self.remove_prefixes();
        if self.found() {
            return;
        }
        self.loop_restore_prefixes();
    }

    /// Stems one non-reduplicated word.
    ///
    /// The `> 3` gate is the module's only absolute length, and it counts
    /// **characters**: a three-letter word is a root or nothing, so there is
    /// no affix left to strip. It is unobservable either way — see the module
    /// documentation's "The text unit" — because a word short enough for the
    /// two readings to disagree must contain an astral character, and no rule
    /// behind the gate can fire on one.
    fn stem_singular(&mut self, word: &[char]) -> Vec<char> {
        self.original = word.to_vec();
        self.current = word.to_vec();
        if self.current.len() > 3 {
            self.stemming_process();
        }
        if self.found() {
            self.current.clone()
        } else {
            self.original.clone()
        }
    }
}

/// The precedence-adjustment specification: six `^X(.*)Y$` probes.
fn precedence_adjustment(w: &[char]) -> bool {
    [
        ("be", "lah"),
        ("be", "an"),
        ("me", "i"),
        ("di", "i"),
        ("pe", "i"),
        ("ter", "i"),
    ]
    .into_iter()
    .any(|(head, tail)| {
        let (h, t) = (slen(head), slen(tail));
        w.len() >= h + t
            && lit_at(w, 0, head)
            && lit_at(w, w.len() - t, tail)
            // The `(.*)` between them cannot cross a line terminator.
            && dot_end(w, h) >= w.len() - t
    })
}

/// `/^(.*)-(.*)$/`: the index of the LAST hyphen, or `None`.
///
/// `(.*)` is greedy, so the split lands on the last hyphen; and because both
/// groups together must cover the whole string, a line terminator anywhere makes
/// the pattern unmatchable.
fn split_last_hyphen(w: &[char]) -> Option<usize> {
    if dot_end(w, 0) != w.len() {
        return None;
    }
    (0..w.len()).rev().find(|&i| w[i] == '-')
}

/// [`StemmerId::is_plural`] over an already-built working buffer, so `stem`
/// does not walk the token a second time.
///
/// `U+002D` is one character and was one code unit, so the hyphen this reads
/// is exactly where it always was; changing the buffer's unit moves nothing
/// here. See [`StemmerId`]'s "Reduplication is one word".
fn is_plural_units(w: &[char]) -> bool {
    // `/^(.*)-(ku|mu|nya|lah|kah|tah|pun)$/` — `(.*)` is greedy, so the
    // shortest possessive wins the tie and the hyphen sits as late as it can.
    let head = if dot_end(w, 0) == w.len() {
        [
            ["ku", "mu"].as_slice(),
            ["nya", "lah", "kah", "tah", "pun"].as_slice(),
        ]
        .into_iter()
        .find_map(|group| {
            group.iter().find_map(|a| {
                let n = slen(a);
                (w.len() > n && lit_at(w, w.len() - n, a) && w[w.len() - n - 1] == '-')
                    .then(|| w.len() - n - 1)
            })
        })
    } else {
        None
    };
    match head {
        Some(cut) => w[..cut].contains(&'-'),
        None => w.contains(&'-'),
    }
}

impl StemmerId {
    /// Creates the stemmer. It is stateless and zero-sized.
    #[inline]
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// The 29,932 root words, in file order.
    #[allow(
        clippy::unused_self,
        reason = "every stemmer is zero-sized; the dictionary is exposed as a \
                  method so it shares the call shape of the rest of the API"
    )]
    #[must_use]
    pub const fn dictionary(&self) -> &'static [&'static str] {
        indonesian_dict::WORDS
    }

    /// Whether the token is a reduplicated (hyphenated) plural.
    #[allow(
        clippy::unused_self,
        reason = "every stemmer is zero-sized; `stem` is a method so the \
                  sixteen of them share one call shape"
    )]
    #[must_use]
    pub fn is_plural(&self, token: &str) -> bool {
        is_plural_units(&token.chars().collect::<Vec<char>>())
    }
    /// Stems one token.
    #[allow(
        clippy::unused_self,
        reason = "every stemmer is zero-sized; `stem` is a method so the \
                  sixteen of them share one call shape"
    )]
    #[must_use]
    pub fn stem<'a>(&self, token: &'a str) -> Cow<'a, str> {
        // The lowered characters land straight in the working vector: no
        // intermediate lowercased `String` is materialised, and `is_plural`
        // reads the buffer rather than walking the token again.
        // `token.len()` is a byte count and so an upper bound on the number
        // of characters, which is all a capacity hint needs to be.
        let mut w: Vec<char> = Vec::with_capacity(token.len());
        crate::units::for_each_lowercase_unit(token, |c| w.push(c));
        let mut st = State {
            // Reset here, and — deliberately — nowhere else.
            removals: Vec::new(),
            original: Vec::new(),
            current: Vec::new(),
        };
        let out = if is_plural_units(&w) {
            stem_plural(&w, &mut st)
        } else {
            st.stem_singular(&w)
        };
        Cow::Owned(text(&out))
    }

    /// The inflectional-particle rule: strips a trailing `lah`, `kah`, `tah`
    /// or `pun`, along with any hyphens immediately before it.
    ///
    /// Returns the removal it recorded and the word as it left it. See
    /// [`RuleResult`].
    #[allow(
        clippy::unused_self,
        reason = "every stemmer is zero-sized; `stem` is a method so the \
                  sixteen of them share one call shape"
    )]
    #[must_use]
    pub fn remove_inflectional_particle(&self, word: &str) -> RuleResult {
        let w: Vec<char> = word.chars().collect();
        let (removal, current) = remove_particle(&w);
        RuleResult {
            removal,
            // `None` is the rule reporting that it left the word alone.
            current_word: text(current.as_deref().unwrap_or(&w)),
        }
    }

    /// Appends a stop word to the **process-global Indonesian list**.
    pub fn add_stop_word(&self, word: impl Into<String>) {
        Language::Id.add(word);
    }

    /// Appends several stop words to the process-global Indonesian list.
    pub fn add_stop_words<I, S>(&self, words: I)
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Language::Id.add_all(words);
    }

    /// Removes the first occurrence of `word` from the Indonesian list.
    pub fn remove_stop_word(&self, word: &str) {
        Language::Id.remove(word);
    }

    /// Removes the first occurrence of each of `words`.
    pub fn remove_stop_words<'a, I>(&self, words: I)
    where
        I: IntoIterator<Item = &'a str>,
    {
        Language::Id.remove_all(words);
    }
}

/// Stems a reduplicated (hyphenated) word.
fn stem_plural(w: &[char], st: &mut State) -> Vec<char> {
    let Some(at) = split_last_hyphen(w) else {
        return w.to_vec();
    };
    let mut first = w[..at].to_vec();
    let mut second = w[at + 1..].to_vec();

    // `malaikat-malaikat-nya` -> `malaikat` + `malaikat-nya`
    let is_pronoun = ["ku", "mu", "nya", "lah", "kah", "tah", "pun"]
        .iter()
        .any(|s| second.len() == slen(s) && lit_at(&second, 0, s));
    if is_pronoun && let Some(inner) = split_last_hyphen(&first) {
        let head = first[..inner].to_vec();
        let tail = first[inner + 1..].to_vec();
        second = cat!(tail.as_slice(), "-", second.as_slice());
        first = head;
    }

    let root1 = st.stem_singular(&first);
    let mut root2 = st.stem_singular(&second);

    // `meniru-nirukan` -> `tiru`
    if !find(&second) && root2 == second {
        root2 = st.stem_singular(&cat!("me", second.as_slice()));
    }

    if root1 == root2 { root1 } else { w.to_vec() }
}

impl TokenizeAndStem for StemmerId {
    const FILTER_ON: Casing = Casing::Raw;
    const STEM_ON: Casing = Casing::Raw;

    /// Indonesian writes reduplication as one hyphenated word, so the hyphen
    /// is word-internal here. See [`StemmerId`]'s "Reduplication is one word"
    /// and [`TokenizeAndStem::HYPHEN_JOINS_LETTERS`].
    const HYPHEN_JOINS_LETTERS: bool = true;

    /// The whole document is lowercased before tokenizing, as in English.
    fn prepare(text: &str) -> Cow<'_, str> {
        Cow::Owned(text.to_lowercase())
    }

    fn is_stop_word(word: &str) -> bool {
        Language::Id.contains(word)
    }

    fn stem_token(&self, token: &str) -> String {
        self.stem(token).into_owned()
    }
}

impl verbora_core::Stemmer for StemmerId {
    fn stem<'a>(&self, token: &'a str) -> Cow<'a, str> {
        Self::stem(self, token)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(t: &str) -> String {
        StemmerId::new().stem(t).into_owned()
    }

    #[test]
    fn documented_vectors() {
        for (input, want) in [
            ("hancurlah", "hancur"),
            ("bukumukah", "buku"),
            ("berikanku", "beri"),
            ("dibuang", "buang"),
            ("belajar", "ajar"),
            ("pelajar", "ajar"),
            ("mempengaruhi", "pengaruh"),
            ("mengkritik", "kritik"),
            ("buku-buku", "buku"),
            ("malaikat-malaikat-Nya", "malaikat"),
            ("", ""),
        ] {
            assert_eq!(s(input), want, "stem({input})");
        }
    }

    /// The hash index replaced a `binary_search` over the same sorted table,
    /// and that search is still the definition of the answer — so it is kept
    /// here as the oracle and checked over every root (each of which must be
    /// *found*, which is what catches a probe sequence that terminates early)
    /// and over a corpus of near-misses built by mutating real roots, which
    /// is where a bad hash or a bad tag comparison would show up.
    #[test]
    fn the_hash_index_agrees_with_a_binary_search() {
        let oracle = |s: &str| indonesian_dict::SORTED.binary_search(&s).is_ok();
        for word in indonesian_dict::SORTED {
            assert!(DICT.contains(word.as_bytes()), "missing root {word:?}");
        }
        let mut rng = Rng(0xD1C7_1DEA_5EED_0011);
        for _ in 0..80_000 {
            // Mutations of real roots: truncations, extensions and single
            // character edits all land near occupied slots.
            let base = indonesian_dict::SORTED[rng.below(indonesian_dict::SORTED.len())];
            let mut w = base.to_owned();
            match rng.below(4) {
                0 => w.truncate(rng.below(w.len().max(1))),
                1 => w.push((b'a' + rng.below(26) as u8) as char),
                2 => {
                    let at = rng.below(w.len().max(1));
                    if w.is_char_boundary(at) {
                        w.insert(at, (b'a' + rng.below(26) as u8) as char);
                    }
                }
                _ => {}
            }
            assert_eq!(DICT.contains(w.as_bytes()), oracle(&w), "{w:?}");
        }
    }

    /// `find` skips the lookup for anything longer than the longest root, so
    /// that constant must really be the longest root.
    #[test]
    fn max_root_len_is_the_longest_root() {
        let longest = indonesian_dict::SORTED
            .iter()
            .map(|w| w.len())
            .max()
            .expect("the dictionary is not empty");
        assert_eq!(longest, MAX_ROOT_LEN);
        assert!(
            indonesian_dict::SORTED.iter().all(|w| w.is_ascii()),
            "`find`'s ASCII rejection assumes every root is ASCII"
        );
    }

    /// A deterministic xorshift, so the fuzz above needs no dev-dependency.
    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            let mut x = self.0;
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            self.0 = x;
            x
        }
        fn below(&mut self, n: usize) -> usize {
            (self.next() % n as u64) as usize
        }
    }

    #[test]
    fn is_plural_reads_the_hyphen_before_the_pronoun() {
        let st = StemmerId::new();
        assert!(st.is_plural("buku-buku"));
        assert!(st.is_plural("malaikat-malaikat-nya"));
        assert!(!st.is_plural("buku-nya"));
        assert!(!st.is_plural("buku"));
        assert!(!st.is_plural(""));
    }

    #[test]
    fn the_dictionary_is_the_expected_size() {
        assert_eq!(StemmerId::new().dictionary().len(), 29932);
    }

    /// The cross-cutting battery every stemmer in this crate answers: empty,
    /// one character, uppercase, accented Latin, Greek, Cyrillic, CJK, an
    /// astral pair, punctuation, digits, a line terminator, and a very long
    /// word.
    ///
    /// The expectations are the *identity* in every row but the case fold,
    /// which is the whole point: none of these is a word of this language, so
    /// a stemmer that changes one is reaching outside its own alphabet.
    #[test]
    fn cross_script_battery() {
        for (input, want) in [
            ("", ""),
            ("a", "a"),
            ("A", "a"),
            ("Ä", "ä"),
            ("café", "café"),
            ("ΟΔΟΣ", "οδος"),
            ("Ω", "ω"),
            ("мама", "мама"),
            ("日本語", "日本語"),
            ("😀", "😀"),
            ("😀ab", "😀ab"),
            ("!?,.", "!?,."),
            ("123", "123"),
            ("\n", "\n"),
        ] {
            assert_eq!(s(input), want, "stem({input:?})");
        }
        assert_eq!(s(&"x".repeat(1000)).len(), 1000);
    }

    #[test]
    fn a_bare_hyphen_run_is_not_a_plural() {
        assert_eq!(s("---"), "---");
        assert_eq!(s("МАМА"), "мама");
    }

    /// Reduplication is spelled with `-` and is **one** Indonesian word, so
    /// the whole lexeme has to reach [`StemmerId::stem`] through the pipeline.
    ///
    /// Every hyphenated entry the crate carries — 335 of the 29,932 roots and
    /// 22 of the 809 stop words — is a single lexeme, and untailored UAX #29
    /// breaks at `U+002D`, which made all of them unreachable through
    /// `tokenize_and_stem` and left [`stem_plural`] dead in that path.
    ///
    /// # The hyphen is unit-neutral
    ///
    /// `U+002D` is one character and was one UTF-16 code unit, so nothing the
    /// change of text unit did touches it: [`split_last_hyphen`] and
    /// [`is_plural_units`] find the same hyphen at the same position, and both
    /// cut *on* it, which is a character boundary under either reading. The
    /// counts below are the same before and after, and they are counted here
    /// rather than quoted so that they cannot go stale.
    #[test]
    fn hyphenated_lexemes_survive_tokenization() {
        let st = StemmerId::new();

        // Reduplication resolves to its root, exactly as `stem` does.
        assert_eq!(st.tokenize_and_stem("buku-buku itu", true), ["buku", "itu"]);
        assert_eq!(
            st.tokenize_and_stem("malaikat-malaikat-Nya", true),
            ["malaikat"]
        );

        // Every hyphenated stop word is still filtered.
        let hyphenated_stops: Vec<&str> = crate::stopwords::Language::Id
            .defaults()
            .iter()
            .copied()
            .filter(|w| w.contains('-'))
            .collect();
        assert_eq!(hyphenated_stops.len(), 22);
        for word in &hyphenated_stops {
            assert!(
                st.tokenize_and_stem(word, false).is_empty(),
                "{word:?} is not filtered through the pipeline"
            );
        }

        // Every hyphenated root reaches `stem` whole.
        let hyphenated_roots: Vec<&str> = indonesian_dict::WORDS
            .iter()
            .copied()
            .filter(|w| w.contains('-'))
            .collect();
        assert_eq!(hyphenated_roots.len(), 335);
        let mut split = Vec::new();
        for root in &hyphenated_roots {
            let got = st.tokenize_and_stem(root, true);
            if got != [s(root)] {
                split.push(*root);
            }
        }
        assert!(
            split.is_empty(),
            "{} hyphenated roots do not reach `stem` whole: {:?}",
            split.len(),
            &split[..split.len().min(8)]
        );
    }

    // -----------------------------------------------------------------------
    // The tables, walked through the documented pipeline
    // -----------------------------------------------------------------------

    /// Every one of the 29,932 roots reaches [`StemmerId::stem`] whole through
    /// [`TokenizeAndStem::tokenize_and_stem`], and every one of the 809 stop
    /// words is filtered by it.
    ///
    /// This is the failure this migration has produced eight times: a stage
    /// transforms the text before a later stage looks it up in a table spelled
    /// the old way. Indonesian is the shape most exposed to it, because it is
    /// the one stemmer here whose `prepare` really does transform — it
    /// lowercases the whole document — before `is_stop_word` and the
    /// dictionary are consulted. Both tables are ASCII lowercase, so the
    /// transform is the identity on them, and this walks **every** entry of
    /// both rather than a sample to say so.
    #[test]
    fn every_dictionary_root_and_stop_word_survives_the_pipeline() {
        let st = StemmerId::new();

        let stops = crate::stopwords::Language::Id.defaults();
        assert_eq!(stops.len(), 809);
        let unfiltered: Vec<&str> = stops
            .iter()
            .copied()
            .filter(|w| !st.tokenize_and_stem(w, false).is_empty())
            .collect();
        assert!(
            unfiltered.is_empty(),
            "{} of {} stop words are not filtered: {:?}",
            unfiltered.len(),
            stops.len(),
            &unfiltered[..unfiltered.len().min(8)]
        );

        let roots = indonesian_dict::WORDS;
        assert_eq!(roots.len(), 29_932);
        let mut lost = Vec::new();
        for root in roots {
            // The prelude is `to_lowercase`, and a root is already lowercase,
            // so the token the tokenizer yields must be the root itself.
            if st.tokenize_and_stem(root, true) != [s(root)] {
                lost.push(*root);
            }
        }
        assert!(
            lost.is_empty(),
            "{} of {} roots do not reach `stem` whole: {:?}",
            lost.len(),
            roots.len(),
            &lost[..lost.len().min(8)]
        );
    }

    /// Every affix table entry measures the same as text and as buffer, and a
    /// cut by its own length lands where the entry starts.
    ///
    /// The tables are `&'static str` and are never re-encoded, so the unit
    /// they are *measured* in is the only thing the migration could have
    /// moved. There are 16 entries across the four tables — 4 particles, 3
    /// possessives, 6 derivational suffixes, 3 plain prefixes — and every one
    /// is ASCII, which is asserted rather than assumed: it is what lets
    /// [`dashed_suffix`] and [`plain_suffix`] subtract a literal's length from
    /// a buffer's own count.
    #[test]
    fn every_affix_table_entry_measures_the_same_as_the_buffer() {
        let tables: &[(&str, &[&str])] = &[
            ("PARTICLES", PARTICLES),
            ("POSSESSIVES", POSSESSIVES),
            ("DERIVATIONAL_SUFFIXES", DERIVATIONAL_SUFFIXES),
            ("PLAIN_PREFIXES", PLAIN_PREFIXES),
        ];
        let mut entries = 0usize;
        for (name, table) in tables {
            for entry in *table {
                entries += 1;
                assert!(entry.is_ascii(), "{name} carries the non-ASCII {entry:?}");
                let probe: Vec<char> = format!("buku{entry}").chars().collect();
                let n = slen(entry);
                assert_eq!(n, entry.chars().count(), "{name} {entry:?}");
                assert!(
                    lit_at(&probe, probe.len() - n, entry),
                    "{name} {entry:?} is not found at the end of its own probe"
                );
                assert_eq!(
                    text(&probe[..probe.len() - n]),
                    "buku",
                    "{name} {entry:?} cuts in the wrong place"
                );
            }
        }
        assert_eq!(entries, 16);
    }

    // -----------------------------------------------------------------------
    // The text unit
    // -----------------------------------------------------------------------

    /// A character outside the Basic Multilingual Plane, and a character
    /// inside it that is its exact equal for every question this module asks.
    ///
    /// `U+1D7CE` (MATHEMATICAL BOLD DIGIT ZERO) and `U+4E2D` are both outside
    /// every character class the rules test ([`v`], [`az`], [`cons`],
    /// [`cls16`], [`cls19`], [`cons_35`]), are not `U+002D`, are not line
    /// terminators, are fixed points of `str::to_lowercase`, and are rejected
    /// by [`find`]'s ASCII filter. Under the crate's unit each is exactly
    /// **one** position of the working buffer.
    const ASTRAL: char = '\u{1D7CE}';
    /// See [`ASTRAL`].
    const BMP_TWIN: char = '\u{4E2D}';

    /// `word` with `c` inserted before its `i`th character.
    fn insert_at(word: &str, i: usize, c: char) -> String {
        let cs: Vec<char> = word.chars().collect();
        let mut out: String = cs[..i].iter().collect();
        out.push(c);
        out.extend(&cs[i..]);
        out
    }

    /// An astral character is one position, so replacing it with an inert
    /// Basic Multilingual Plane character cannot change a stem.
    ///
    /// # Indonesian is provably unmoved by the unit
    ///
    /// See the module documentation's "The text unit": [`find`] rejects any
    /// word carrying a non-ASCII character before the dictionary is consulted,
    /// and every rule is anchored on an ASCII literal at a known offset, so a
    /// word containing an astral character reaches no rule that can fire. The
    /// unit-indexed constants — `w.len() > 3`, `w[3]`, `MAX_ROOT_LEN` — are
    /// all still here and are all unreachable with one present.
    ///
    /// This test therefore passed before the conversion as well as after, and
    /// it is here as the *certification* of that argument rather than as its
    /// red-to-green gate: the gate for this group is `crate::uk`'s.
    #[test]
    fn an_astral_character_cannot_move_an_indonesian_answer() {
        let twin = BMP_TWIN.to_string();
        let astral = ASTRAL.to_string();
        let corpus = astral_corpus();
        for word in &corpus {
            let bmp = word.replace(ASTRAL, &twin);
            assert_eq!(
                s(word),
                s(&bmp).replace(BMP_TWIN, &astral),
                "stem({word:?}) does not agree with its BMP twin {bmp:?}"
            );
        }
        assert_eq!(corpus.len(), placements());
    }

    /// One character in, one character out: no stem may contain a character
    /// the caller did not supply.
    ///
    /// Every cut this module makes is at an ASCII literal's boundary, at a
    /// hyphen or at a line terminator, all of which are single characters, so
    /// a `char` buffer can never be left holding half of one.
    #[test]
    fn no_stem_invents_a_replacement_character() {
        let corpus = astral_corpus();
        for word in &corpus {
            let out = s(word);
            assert!(
                !out.contains('\u{FFFD}'),
                "stem({word:?}) returned {out:?}, which the caller never supplied"
            );
        }
        assert_eq!(corpus.len(), placements());
    }

    /// The size of [`astral_corpus`], derived from its own seeds rather than
    /// recorded: a seed of `n` characters has `n + 1` insertion points.
    fn placements() -> usize {
        astral_seeds().iter().map(|s| s.chars().count() + 1).sum()
    }

    /// What the enumerations walk: **every** Indonesian stop word, **every**
    /// affix table entry, **every** hyphenated root, the bench corpus, and
    /// seeded affixations of the dictionary.
    ///
    /// The composition is arithmetic: 809 stop words, the 16 affix entries,
    /// all 335 hyphenated roots, 16 bench words, and 8,000 seeded roots each
    /// contributing five shapes — the bare root, its reduplication, and the
    /// three affixation patterns the rules are written for.
    fn astral_seeds() -> Vec<String> {
        let mut seeds: Vec<String> = crate::stopwords::Language::Id
            .defaults()
            .iter()
            .map(|w| (*w).to_owned())
            .collect();
        for table in [
            PARTICLES,
            POSSESSIVES,
            DERIVATIONAL_SUFFIXES,
            PLAIN_PREFIXES,
        ] {
            seeds.extend(table.iter().map(|e| (*e).to_owned()));
        }
        seeds.extend(
            indonesian_dict::WORDS
                .iter()
                .filter(|w| w.contains('-'))
                .map(|w| (*w).to_owned()),
        );
        seeds.extend(crate::test_support::bench_words("id"));
        let mut rng = Rng(0x5EED_1234_ABCD_0001);
        for _ in 0..8_000 {
            let root = indonesian_dict::WORDS[rng.below(indonesian_dict::WORDS.len())];
            seeds.push(root.to_owned());
            seeds.push(format!("{root}-{root}"));
            seeds.push(format!("me{root}kan"));
            seeds.push(format!("di{root}i"));
            seeds.push(format!("peng{root}an"));
        }
        assert_eq!(seeds.len(), 809 + 16 + 335 + 16 + 5 * 8_000);
        seeds
    }

    /// [`astral_seeds`] with [`ASTRAL`] inserted at every position of every
    /// seed. Nothing here is sampled.
    fn astral_corpus() -> Vec<String> {
        let mut out = Vec::new();
        for seed in &astral_seeds() {
            for i in 0..=seed.chars().count() {
                out.push(insert_at(seed, i, ASTRAL));
            }
        }
        out
    }
}
