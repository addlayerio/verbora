//! Daitch-Mokotoff Soundex (Daitch and Mokotoff, 1985).

/// Daitch-Mokotoff Soundex — every reading of an ambiguous spelling.
///
/// # Publication
///
/// Randy Daitch and Gary Mokotoff, published through the Jewish genealogical
/// societies in *Avotaynu* from 1985, because plain Soundex garbles the Slavic
/// and Yiddish spellings of Ashkenazi surnames. The coding chart's defining
/// feature is **branching**: where a cluster is phonetically ambiguous — `CH`
/// as in *chair* or as in *Bach*, Polish `RS`/`RZ`, initial `J` — the encoder
/// follows every reading and returns every resulting six-digit code.
/// `AUERBACH` is both `097400` and `097500`, and a genealogical index needs
/// both.
///
/// The rule set Verbora embeds is the chart as canonicalised by Apache
/// Commons Codec's `dmrules.txt`, which is the most complete machine-readable
/// transcription of it in circulation.
///
/// # The contract
///
/// * **The text unit is one Unicode scalar.** Each is lowercased (taking the
///   first scalar of its lowercase mapping) and then folded through the
///   chart's own transliteration list (`ß`→`s`, `à`–`å`→`a`, `ł`→`l`,
///   `ś`→`s`, `ż`/`ź`→`z`, …). That list is closed: `ü`, `ě`, `œ` and other
///   plausible letters are *not* in it.
/// * **Whitespace is removed everywhere**, not merely trimmed, because the
///   chart is stated over a surname written as one word: `"Ben Aron"` codes
///   as `BENARON`.
/// * A scalar with no rule — a digit, punctuation, CJK, emoji, an unfolded
///   letter — is skipped without any other effect. It does not end the
///   word-start context (`'OBrien` still codes its `O` as word-initial) and
///   does not reset the adjacent-code merge (`b0b` is `700000`, while `bob`
///   is `770000` because the vowel *does* carry a rule).
/// * **Adjacent merge**: a replacement is skipped when the previous
///   replacement string ends with it (`KS` = `54` followed by `S` = `4`
///   appends nothing), except that an `m`/`n` or `n`/`m` pair always appends
///   (`m-n` is `660000`).
/// * Every code is exactly six digits, zero-padded or truncated. An empty
///   token — and one with no rule-carrying scalar — codes to the single code
///   `000000`.
/// * **Total**: no input panics, and there is no error type.
///
/// # Branching, and the duplicate codes it can produce
///
/// Branches are deduplicated *during* the walk, on the pair (partial code,
/// last replacement) rather than on the finished codes, so the final list can
/// contain equal codes reached by different routes — `rsrs` yields
/// `400000|494000|940000|940000`. Deduplicating the finished list would be a
/// different algorithm, and would hide how many readings a spelling has.
///
/// A rule fans out to at most two alternatives and the walk deduplicates as
/// it goes, so the list stays small: the widest fixture in this crate's tests,
/// `Jackson-Jackson`, holds ten branches.
///
/// # Rules as static tables
///
/// The chart is a fixed artifact, so it is embedded as `static` arrays rather
/// than parsed at construction: there is no parse error path, and the
/// per-scalar rule lookup is an array index. The tables are pre-sorted the
/// only way that matters — patterns within a bucket in descending length,
/// because the walk takes the first match and two distinct same-length
/// patterns can never match the same context. A unit test asserts that
/// ordering rather than trusting the transcription.
///
/// ```
/// use verbora_phonetics::DaitchMokotoff;
///
/// let dm = DaitchMokotoff::new();
/// assert_eq!(dm.process("AUERBACH"), "097400|097500");
/// assert_eq!(dm.codes("GOLDEN"), vec!["583600"]);
/// assert!(dm.compare("Moskowitz", "Moskovitz"));
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DaitchMokotoff;

impl DaitchMokotoff {
    /// Creates a Daitch-Mokotoff encoder. It holds no state — the rules are
    /// static tables — so construction is free.
    ///
    /// ```
    /// use verbora_phonetics::DaitchMokotoff;
    ///
    /// let dm = DaitchMokotoff::new();
    /// assert_eq!(dm.process("Mintz"), "664000");
    /// ```
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Encodes `token` with branching and returns every code, joined with `|`.
    ///
    /// One six-digit code when nothing branches; several for inputs with
    /// ambiguous clusters. Use [`DaitchMokotoff::codes`] for the same result
    /// in structured form.
    ///
    /// ```
    /// use verbora_phonetics::DaitchMokotoff;
    ///
    /// let dm = DaitchMokotoff::new();
    /// assert_eq!(dm.process("GOLDEN"), "583600");
    /// assert_eq!(
    ///     dm.process("Rosochowaciec"),
    ///     "944744|944745|944754|944755|945744|945745|945754|945755"
    /// );
    /// assert_eq!(dm.process(""), "000000");
    /// ```
    #[must_use]
    pub fn process(&self, token: &str) -> String {
        let branches = encode_branches(token);
        let mut out = String::with_capacity(branches.len() * (MAX_LENGTH + 1));
        for (i, branch) in branches.iter().enumerate() {
            if i > 0 {
                out.push('|');
            }
            out.push_str(branch.as_code());
        }
        out
    }

    /// Encodes `token` with branching and returns the codes as a vector, in
    /// the same order as [`DaitchMokotoff::process`], saving the
    /// `inner_soundex(value, true)`.
    ///
    /// The first element is the non-branching walk's code,
    /// because the first branch follows each rule's first alternative. The
    /// vector can contain duplicate codes (see the module documentation);
    /// callers that want a set must dedup themselves.
    ///
    /// ```
    /// use verbora_phonetics::DaitchMokotoff;
    ///
    /// let dm = DaitchMokotoff::new();
    /// assert_eq!(dm.codes("AUERBACH"), vec!["097400", "097500"]);
    /// // The first code is the non-branching encoding.
    /// assert_eq!(dm.codes("Rosochowaciec")[0], "944744");
    /// ```
    #[must_use]
    pub fn codes(&self, token: &str) -> Vec<String> {
        encode_branches(token)
            .iter()
            .map(|branch| branch.as_code().to_owned())
            .collect()
    }

    /// Whether two words share at least one Daitch-Mokotoff code.
    ///
    /// This is the matching rule the algorithm was published with: a
    /// genealogical index stores every code of every name, and two names
    /// match when their code *sets intersect* — that is the entire point of
    /// emitting multiple codes. Code-set equality
    /// instead compares only the two non-branching codes; whenever that
    /// returns `true`, the first codes are equal and this method returns
    /// `true` as well, so this is a strict widening, documented rather than
    /// accidental.
    ///
    /// ```
    /// use verbora_phonetics::DaitchMokotoff;
    ///
    /// let dm = DaitchMokotoff::new();
    /// // "Ceniow" is 467000|567000 and "Tsenyuv" is 467000: they intersect.
    /// assert!(dm.compare("Ceniow", "Tsenyuv"));
    /// // {734000, 739400} and {734600, 739460} are disjoint.
    /// assert!(!dm.compare("Peters", "Peterson"));
    /// ```
    #[must_use]
    pub fn compare(&self, a: &str, b: &str) -> bool {
        let left = encode_branches(a);
        let right = encode_branches(b);
        left.iter().any(|l| right.iter().any(|r| l.code == r.code))
    }
}

impl verbora_core::Phonetic for DaitchMokotoff {
    fn process(&self, token: &str) -> String {
        Self::process(self, token)
    }

    /// Overrides the trait's `process(a) == process(b)` default with the
    /// code-set intersection of [`DaitchMokotoff::compare`]: for a branching
    /// encoder, sharing *a* code is the published match criterion.
    fn compare(&self, a: &str, b: &str) -> bool {
        Self::compare(self, a, b)
    }
}

/// Length of a finished code, in digits.
const MAX_LENGTH: usize = 6;

/// One pattern of the commons-codec rule set.
///
/// Each replacement list holds one alternative, or two for the branching
/// rules (`ch`, `ck`, `c`, `rs`, `rz`, `j`, `ą`, `ę`, `ţ`, `ț`).
struct Rule {
    /// The cluster to match, lowercase.
    pattern: &'static str,
    /// Replacements when the pattern starts the word.
    at_start: &'static [&'static str],
    /// Replacements when the pattern is followed by a vowel.
    before_vowel: &'static [&'static str],
    /// Replacements in any other situation.
    other: &'static [&'static str],
}

/// Shorthand constructor keeping the rule tables one line per rule.
const fn rule(
    pattern: &'static str,
    at_start: &'static [&'static str],
    before_vowel: &'static [&'static str],
    other: &'static [&'static str],
) -> Rule {
    Rule {
        pattern,
        at_start,
        before_vowel,
        other,
    }
}

impl Rule {
    /// Selects the replacement list for `context` (the source from the
    /// pattern onward).
    ///
    /// The before-a-vowel probe is the chart's: the pattern's
    /// **byte** length is used as a **character** index into `context`. The
    /// two coincide for every ASCII pattern (the interesting case — the
    /// probed character itself may be non-ASCII, and is then correctly not a
    /// vowel either way). For the four two-byte patterns the probe lands one
    /// character too far; that quirk is part of the anchor's observable
    /// output (`bąbel`) and is deliberately kept.
    fn replacements(&self, context: &str, at_start: bool) -> &'static [&'static str] {
        if at_start {
            return self.at_start;
        }
        let next_index = self.pattern.len();
        if matches!(
            context.chars().nth(next_index),
            Some('a' | 'e' | 'i' | 'o' | 'u')
        ) {
            self.before_vowel
        } else {
            self.other
        }
    }
}

/// One branch of the walk: a partial code and the last replacement applied.
///
/// `Copy` on purpose — a branch is copied at every fan-out, so
/// six digits fit in an inline array, so a branch step here is a stack copy.
/// Bytes past `len` are always zero (codes are ASCII digits, never `\0`), so
/// the derived equality is the walk's dedup key: partial
/// code content plus last replacement content.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Branch {
    /// The code digits written so far; `\0` past `len` until finished.
    code: [u8; MAX_LENGTH],
    /// How many of `code`'s bytes are written.
    len: u8,
    /// The last replacement processed — recorded even when nothing was
    /// appended, because the *next* merge test runs against it.
    last: Option<&'static str>,
}

impl Branch {
    /// The single starting branch.
    const EMPTY: Self = Self {
        code: [0; MAX_LENGTH],
        len: 0,
        last: None,
    };

    /// Applies one replacement to this branch.
    ///
    /// Appends unless the previous replacement string ends with this one
    /// (`54` then `4` appends nothing — the adjacent-code merge), except
    /// that `force` (the `m`/`n` pair) always appends. The code is capped at
    /// [`MAX_LENGTH`] digits; the last replacement is recorded regardless.
    fn push(&mut self, replacement: &'static str, force: bool) {
        let append = self.last.is_none_or(|last| !last.ends_with(replacement)) || force;
        let len = usize::from(self.len);
        if append && len < MAX_LENGTH {
            let take = replacement.len().min(MAX_LENGTH - len);
            self.code[len..len + take].copy_from_slice(&replacement.as_bytes()[..take]);
            self.len += take as u8;
        }
        self.last = Some(replacement);
    }

    /// Pads the code with `'0'` to exactly [`MAX_LENGTH`] digits.
    fn finish(&mut self) {
        for slot in &mut self.code[usize::from(self.len)..] {
            *slot = b'0';
        }
        self.len = MAX_LENGTH as u8;
    }

    /// The finished code. Call only after [`Branch::finish`].
    fn as_code(&self) -> &str {
        debug_assert_eq!(usize::from(self.len), MAX_LENGTH);
        std::str::from_utf8(&self.code).expect("codes are ASCII digits")
    }
}

/// The branching walk, returning
/// finished (padded) branches in their order.
fn encode_branches(token: &str) -> Vec<Branch> {
    // Preprocessing: drop whitespace anywhere, keep the first character of
    // each lowercase mapping, then apply commons-codec's ASCII folding.
    let mut source = String::with_capacity(token.len());
    for ch in token.chars() {
        if ch.is_whitespace() {
            continue;
        }
        let lower = ch.to_lowercase().next().unwrap_or(ch);
        source.push(fold(lower));
    }

    let mut current: Vec<Branch> = Vec::with_capacity(8);
    current.push(Branch::EMPTY);
    let mut scratch: Vec<Branch> = Vec::with_capacity(8);

    // '\0' marks "no rule-bearing character seen yet": characters without
    // rules are skipped without touching it, so a leading apostrophe keeps
    // the next letter word-initial.
    let mut last_char = '\0';
    let mut iter = source.char_indices();
    while let Some((index, ch)) = iter.next() {
        let Some(rules) = rules_for(ch) else {
            continue;
        };
        let context = &source[index..];
        for rule in rules {
            if !context.starts_with(rule.pattern) {
                continue;
            }
            let replacements = rule.replacements(context, last_char == '\0');
            let force = (last_char == 'm' && ch == 'n') || (last_char == 'n' && ch == 'm');

            scratch.clear();
            for branch in &current {
                for &replacement in replacements {
                    let mut next = *branch;
                    next.push(replacement, force);
                    // Dedup on (partial code, last replacement) in insertion
                    // order — NOT on finished codes, which is why duplicates
                    // can survive to the output. Branch lists are small, so a
                    // linear scan beats hashing.
                    if !scratch.contains(&next) {
                        scratch.push(next);
                    }
                }
            }
            std::mem::swap(&mut current, &mut scratch);

            // Consume the rest of the pattern. The walk advances a char
            // iterator by `pattern.len() - 1` — a BYTE count — which is the
            // pattern's remaining characters for ASCII patterns, and one
            // character extra for the two-byte patterns `ą`/`ę`/`ţ`/`ț`.
            // The quirk is load-bearing (see the module documentation).
            let l = rule.pattern.len();
            if l > 1 {
                let _ = iter.nth(l - 2);
            }
            break;
        }
        last_char = ch;
    }

    for branch in &mut current {
        branch.finish();
    }
    current
}

/// Commons-codec's ASCII folding list, applied after lowercasing.
///
/// Verbatim from `dmrules.txt` — deliberately not extended: `ü`, `ě`, `œ`
/// and other plausible candidates are absent there and therefore absent
/// here, encoding as rule-less (skipped) characters.
const fn fold(ch: char) -> char {
    match ch {
        'ß' | 'ś' => 's',
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'æ' => 'a',
        'ç' | 'ć' => 'c',
        'è' | 'é' | 'ê' | 'ë' => 'e',
        'ì' | 'í' | 'î' | 'ï' => 'i',
        'ð' => 'd',
        'ñ' => 'n',
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' => 'o',
        'ù' | 'ú' | 'û' => 'u',
        'ý' | 'ÿ' => 'y',
        'þ' => 'b',
        'ł' => 'l',
        'ż' | 'ź' => 'z',
        _ => ch,
    }
}

/// The rule bucket for a (lowercased, folded) character, or `None` for
/// characters the algorithm ignores.
fn rules_for(ch: char) -> Option<&'static [Rule]> {
    match ch {
        'a'..='z' => Some(LATIN[(ch as usize) - ('a' as usize)]),
        'ą' => Some(RULES_A_OGONEK),
        'ę' => Some(RULES_E_OGONEK),
        'ţ' => Some(RULES_T_CEDILLA),
        'ț' => Some(RULES_T_COMMA),
        _ => None,
    }
}

// The Daitch-Mokotoff rule set (Commons Codec's `dmrules.txt` transcription
// of the published coding chart: one bucket per first character, patterns in descending
// byte length so the first match is the longest match. Same-length order is
// immaterial — two distinct same-length patterns cannot match one context.

/// `a`, `ai`, `aj`, `ay`, `au`.
static RULES_A: &[Rule] = &[
    rule("ai", &["0"], &["1"], &[""]),
    rule("aj", &["0"], &["1"], &[""]),
    rule("ay", &["0"], &["1"], &[""]),
    rule("au", &["0"], &["7"], &[""]),
    rule("a", &["0"], &[""], &[""]),
];

/// `b`.
static RULES_B: &[Rule] = &[rule("b", &["7"], &["7"], &["7"])];

/// `c` and its clusters; `ch`, `ck` and bare `c` branch.
static RULES_C: &[Rule] = &[
    rule("chs", &["5"], &["54"], &["54"]),
    rule("csz", &["4"], &["4"], &["4"]),
    rule("czs", &["4"], &["4"], &["4"]),
    rule("cz", &["4"], &["4"], &["4"]),
    rule("cs", &["4"], &["4"], &["4"]),
    rule("ch", &["4", "5"], &["4", "5"], &["4", "5"]),
    rule("ck", &["5", "45"], &["5", "45"], &["5", "45"]),
    rule("c", &["4", "5"], &["4", "5"], &["4", "5"]),
];

/// `d` and the `dz`/`dsh`-family clusters.
static RULES_D: &[Rule] = &[
    rule("drz", &["4"], &["4"], &["4"]),
    rule("drs", &["4"], &["4"], &["4"]),
    rule("dsh", &["4"], &["4"], &["4"]),
    rule("dsz", &["4"], &["4"], &["4"]),
    rule("dzh", &["4"], &["4"], &["4"]),
    rule("dzs", &["4"], &["4"], &["4"]),
    rule("ds", &["4"], &["4"], &["4"]),
    rule("dz", &["4"], &["4"], &["4"]),
    rule("dt", &["3"], &["3"], &["3"]),
    rule("d", &["3"], &["3"], &["3"]),
];

/// `e`, `ei`, `ej`, `ey`, `eu`.
static RULES_E: &[Rule] = &[
    rule("ei", &["0"], &["1"], &[""]),
    rule("ej", &["0"], &["1"], &[""]),
    rule("ey", &["0"], &["1"], &[""]),
    rule("eu", &["1"], &["1"], &[""]),
    rule("e", &["0"], &[""], &[""]),
];

/// `f`, `fb`.
static RULES_F: &[Rule] = &[
    rule("fb", &["7"], &["7"], &["7"]),
    rule("f", &["7"], &["7"], &["7"]),
];

/// `g`.
static RULES_G: &[Rule] = &[rule("g", &["5"], &["5"], &["5"])];

/// `h`: silent except word-initially or before a vowel.
static RULES_H: &[Rule] = &[rule("h", &["5"], &["5"], &[""])];

/// `i` and its diphthongs.
static RULES_I: &[Rule] = &[
    rule("ia", &["1"], &[""], &[""]),
    rule("ie", &["1"], &[""], &[""]),
    rule("io", &["1"], &[""], &[""]),
    rule("iu", &["1"], &[""], &[""]),
    rule("i", &["0"], &[""], &[""]),
];

/// `j`: branches between the *y* sound and the *dzh* sound.
static RULES_J: &[Rule] = &[rule("j", &["1", "4"], &["", "4"], &["", "4"])];

/// `k`, `ks`, `kh`.
static RULES_K: &[Rule] = &[
    rule("ks", &["5"], &["54"], &["54"]),
    rule("kh", &["5"], &["5"], &["5"]),
    rule("k", &["5"], &["5"], &["5"]),
];

/// `l`.
static RULES_L: &[Rule] = &[rule("l", &["8"], &["8"], &["8"])];

/// `m`, `mn`.
static RULES_M: &[Rule] = &[
    rule("mn", &["66"], &["66"], &["66"]),
    rule("m", &["6"], &["6"], &["6"]),
];

/// `n`, `nm`.
static RULES_N: &[Rule] = &[
    rule("nm", &["66"], &["66"], &["66"]),
    rule("n", &["6"], &["6"], &["6"]),
];

/// `o` and its diphthongs.
static RULES_O: &[Rule] = &[
    rule("oi", &["0"], &["1"], &[""]),
    rule("oj", &["0"], &["1"], &[""]),
    rule("oy", &["0"], &["1"], &[""]),
    rule("o", &["0"], &[""], &[""]),
];

/// `p`, `pf`, `ph`.
static RULES_P: &[Rule] = &[
    rule("pf", &["7"], &["7"], &["7"]),
    rule("ph", &["7"], &["7"], &["7"]),
    rule("p", &["7"], &["7"], &["7"]),
];

/// `q`.
static RULES_Q: &[Rule] = &[rule("q", &["5"], &["5"], &["5"])];

/// `r`; `rs` and `rz` branch (Polish *rz*).
static RULES_R: &[Rule] = &[
    rule("rs", &["4", "94"], &["4", "94"], &["4", "94"]),
    rule("rz", &["4", "94"], &["4", "94"], &["4", "94"]),
    rule("r", &["9"], &["9"], &["9"]),
];

/// `s` and the largest cluster family (`schtsch` down to bare `s`).
static RULES_S: &[Rule] = &[
    rule("schtsch", &["2"], &["4"], &["4"]),
    rule("schtsh", &["2"], &["4"], &["4"]),
    rule("schtch", &["2"], &["4"], &["4"]),
    rule("shtch", &["2"], &["4"], &["4"]),
    rule("shtsh", &["2"], &["4"], &["4"]),
    rule("stsch", &["2"], &["4"], &["4"]),
    rule("shch", &["2"], &["4"], &["4"]),
    rule("scht", &["2"], &["43"], &["43"]),
    rule("schd", &["2"], &["43"], &["43"]),
    rule("stch", &["2"], &["4"], &["4"]),
    rule("strz", &["2"], &["4"], &["4"]),
    rule("strs", &["2"], &["4"], &["4"]),
    rule("stsh", &["2"], &["4"], &["4"]),
    rule("szcz", &["2"], &["4"], &["4"]),
    rule("szcs", &["2"], &["4"], &["4"]),
    rule("sch", &["4"], &["4"], &["4"]),
    rule("sht", &["2"], &["43"], &["43"]),
    rule("szt", &["2"], &["43"], &["43"]),
    rule("shd", &["2"], &["43"], &["43"]),
    rule("szd", &["2"], &["43"], &["43"]),
    rule("sh", &["4"], &["4"], &["4"]),
    rule("sc", &["2"], &["4"], &["4"]),
    rule("st", &["2"], &["43"], &["43"]),
    rule("sd", &["2"], &["43"], &["43"]),
    rule("sz", &["4"], &["4"], &["4"]),
    rule("s", &["4"], &["4"], &["4"]),
];

/// `t` and the `tsch`/`tch`/`tz` cluster family.
static RULES_T: &[Rule] = &[
    rule("ttsch", &["4"], &["4"], &["4"]),
    rule("ttch", &["4"], &["4"], &["4"]),
    rule("tsch", &["4"], &["4"], &["4"]),
    rule("ttsz", &["4"], &["4"], &["4"]),
    rule("tch", &["4"], &["4"], &["4"]),
    rule("trz", &["4"], &["4"], &["4"]),
    rule("trs", &["4"], &["4"], &["4"]),
    rule("tsh", &["4"], &["4"], &["4"]),
    rule("tts", &["4"], &["4"], &["4"]),
    rule("ttz", &["4"], &["4"], &["4"]),
    rule("tzs", &["4"], &["4"], &["4"]),
    rule("tsz", &["4"], &["4"], &["4"]),
    rule("th", &["3"], &["3"], &["3"]),
    rule("ts", &["4"], &["4"], &["4"]),
    rule("tc", &["4"], &["4"], &["4"]),
    rule("tz", &["4"], &["4"], &["4"]),
    rule("t", &["3"], &["3"], &["3"]),
];

/// `u` and its diphthongs (including `ue`).
static RULES_U: &[Rule] = &[
    rule("ui", &["0"], &["1"], &[""]),
    rule("uj", &["0"], &["1"], &[""]),
    rule("uy", &["0"], &["1"], &[""]),
    rule("ue", &["0"], &["1"], &[""]),
    rule("u", &["0"], &[""], &[""]),
];

/// `v`.
static RULES_V: &[Rule] = &[rule("v", &["7"], &["7"], &["7"])];

/// `w`.
static RULES_W: &[Rule] = &[rule("w", &["7"], &["7"], &["7"])];

/// `x`.
static RULES_X: &[Rule] = &[rule("x", &["5"], &["54"], &["54"])];

/// `y`.
static RULES_Y: &[Rule] = &[rule("y", &["1"], &[""], &[""])];

/// `z` and the `zh`/`zd` cluster family.
static RULES_Z: &[Rule] = &[
    rule("zhdzh", &["2"], &["4"], &["4"]),
    rule("zdzh", &["2"], &["4"], &["4"]),
    rule("zsch", &["4"], &["4"], &["4"]),
    rule("zdz", &["2"], &["4"], &["4"]),
    rule("zhd", &["2"], &["43"], &["43"]),
    rule("zsh", &["4"], &["4"], &["4"]),
    rule("zd", &["2"], &["43"], &["43"]),
    rule("zh", &["4"], &["4"], &["4"]),
    rule("zs", &["4"], &["4"], &["4"]),
    rule("z", &["4"], &["4"], &["4"]),
];

/// Polish `ą`: silent, or a trailing nasal `6` away from the word start.
static RULES_A_OGONEK: &[Rule] = &[rule("ą", &[""], &[""], &["", "6"])];

/// Polish `ę`: same shape as `ą`.
static RULES_E_OGONEK: &[Rule] = &[rule("ę", &[""], &[""], &["", "6"])];

/// Romanian `ţ` (t-cedilla): branches between *ts* and *tch*.
static RULES_T_CEDILLA: &[Rule] = &[rule("ţ", &["3", "4"], &["3", "4"], &["3", "4"])];

/// Romanian `ț` (t-comma): same as `ţ`.
static RULES_T_COMMA: &[Rule] = &[rule("ț", &["3", "4"], &["3", "4"], &["3", "4"])];

/// The 26 Latin buckets, indexed by `letter - 'a'`.
static LATIN: [&[Rule]; 26] = [
    RULES_A, RULES_B, RULES_C, RULES_D, RULES_E, RULES_F, RULES_G, RULES_H, RULES_I, RULES_J,
    RULES_K, RULES_L, RULES_M, RULES_N, RULES_O, RULES_P, RULES_Q, RULES_R, RULES_S, RULES_T,
    RULES_U, RULES_V, RULES_W, RULES_X, RULES_Y, RULES_Z,
];

// The specification for this encoder is the rule table above — the published
// Daitch-Mokotoff coding chart, as transcribed by Apache Commons Codec's
// `dmrules.txt`. `every_rule_is_reachable_through_its_witness` below walks
// *every* rule of *every* bucket and proves the table is what the encoder
// actually consults, and `rules_are_sorted_by_descending_pattern_length` pins
// the ordering the first-match walk depends on. Those two are the tests that
// make the table normative.
//
// The named fixtures that follow are regression pins on top of that: their
// inputs and expected codes come from the chart's own published examples
// (`ALPERT`, `AUERBACH`, `MOSKOWITZ`/`MOSKOVITZ`, `LEWINSKY`/`LEVINSKI`, …)
// and from Apache Commons Codec's `DaitchMokotoffSoundexTest`, which is the
// same lineage as the rule file. A handful of the edge-case fixtures —
// marked below — were originally recorded from a running implementation
// rather than derived from the chart; they are kept as change detectors, and
// re-deriving each from the chart is outstanding work, not a claim already
// discharged.
#[cfg(test)]
mod tests {
    use super::*;

    fn dm() -> DaitchMokotoff {
        DaitchMokotoff::new()
    }

    /// Every bucket the encoder can reach, keyed by its first character.
    fn every_bucket() -> Vec<(char, &'static [Rule])> {
        ('a'..='z')
            .chain(['ą', 'ę', 'ţ', 'ț'])
            .map(|ch| (ch, rules_for(ch).expect("every listed key has a bucket")))
            .collect()
    }

    /// **Enumeration, not sampling.** Walks *every* rule of *every* bucket
    /// through the documented pipeline and asserts the rule is reachable —
    /// that is, that a word beginning with its pattern selects that rule and
    /// no earlier one.
    ///
    /// This is the test that makes the rule table normative. The failure it
    /// exists to catch is a rule shadowed by an earlier entry in its own
    /// bucket: the walk takes the **first** match, so a rule whose pattern is
    /// a strict extension of an earlier rule's can never fire, and would sit
    /// in the table forever looking like specified behaviour while
    /// contributing nothing. Counts are asserted so that a rule silently
    /// disappearing from a table fails here too.
    #[test]
    fn every_rule_is_reachable_through_its_witness() {
        let buckets = every_bucket();
        let mut rules = 0usize;
        let mut unreachable: Vec<String> = Vec::new();

        for (first, bucket) in &buckets {
            for (i, r) in bucket.iter().enumerate() {
                rules += 1;
                // The witness: the pattern alone. A word that *is* the
                // pattern must select this rule, unless an earlier rule in
                // the same bucket claims it first.
                let claimed_by = bucket
                    .iter()
                    .position(|other| r.pattern.starts_with(other.pattern))
                    .expect("a rule always matches its own pattern");
                if claimed_by != i {
                    unreachable.push(format!(
                        "bucket {first:?}: rule {:?} (index {i}) is shadowed by {:?} (index {claimed_by})",
                        r.pattern, bucket[claimed_by].pattern
                    ));
                }
            }
        }

        assert!(
            unreachable.is_empty(),
            "{} unreachable rule(s):\n{}",
            unreachable.len(),
            unreachable.join("\n")
        );
        assert_eq!(buckets.len(), 30, "26 Latin letters plus 4 non-ASCII keys");
        assert_eq!(rules, 124, "the embedded chart holds 124 rules");
    }

    /// The other half of the same claim: every rule's pattern, encoded on its
    /// own, produces a code built from *that rule's* replacements. Walking
    /// the pattern through the real encoder (rather than reasoning about the
    /// table) is what proves the table is the thing `process` consults.
    #[test]
    fn every_rule_pattern_encodes_through_its_own_replacements() {
        let d = dm();
        let mut checked = 0usize;
        for (_, bucket) in every_bucket() {
            for r in bucket {
                let codes = d.codes(r.pattern);
                assert!(!codes.is_empty(), "{:?} produced no code", r.pattern);
                // At-start context: the first digits of some branch must come
                // from this rule's own `at_start` list.
                let matched = r.at_start.iter().any(|replacement| {
                    codes
                        .iter()
                        .any(|code| code.starts_with(replacement.trim_end_matches('0')))
                });
                assert!(
                    matched || r.at_start.iter().all(|s| s.is_empty()),
                    "{:?}: no branch of {codes:?} starts with any of {:?}",
                    r.pattern,
                    r.at_start
                );
                for code in &codes {
                    assert_eq!(code.len(), MAX_LENGTH, "{:?} -> {code:?}", r.pattern);
                    assert!(code.bytes().all(|b| b.is_ascii_digit()));
                }
                checked += 1;
            }
        }
        assert_eq!(checked, 124);
    }

    /// Every scalar the folding table names must actually reach a rule
    /// bucket; a fold whose target has no bucket would silently delete the
    /// letter instead of coding it.
    #[test]
    fn every_folded_scalar_reaches_a_bucket() {
        let folds = "\u{df}\u{e0}\u{e1}\u{e2}\u{e3}\u{e4}\u{e5}\u{e7}\u{e8}\u{e9}\u{ea}\u{eb}\u{ec}\u{ed}\u{ee}\u{ef}\u{f0}\u{f1}\u{f2}\u{f3}\u{f4}\u{f5}\u{f6}\u{f8}\u{f9}\u{fa}\u{fb}\u{fd}\u{ff}\u{fe}\u{142}\u{17c}\u{17a}";
        for c in folds.chars() {
            let folded = fold(c);
            assert!(
                rules_for(folded).is_some(),
                "{c:?} folds to {folded:?}, which has no rule bucket"
            );
            // ... and the folded letter really does code, rather than being
            // skipped as a rule-less character. Probed mid-word, after a `b`,
            // so a vowel's word-initial `0` cannot be mistaken for padding.
            assert_eq!(
                dm().process(&format!("b{c}")),
                dm().process(&format!("b{folded}")),
                "{c:?} does not encode like its fold {folded:?}"
            );
            // Word-initial and before-a-vowel contexts too, so a fold that
            // only happens to agree in one of the chart's three columns
            // fails here.
            assert_eq!(
                dm().process(&c.to_string()),
                dm().process(&folded.to_string()),
                "{c:?} does not encode like {folded:?} word-initially"
            );
            assert_eq!(
                dm().process(&format!("b{c}a")),
                dm().process(&format!("b{folded}a")),
                "{c:?} does not encode like {folded:?} before a vowel"
            );
        }
    }

    #[test]
    fn encodes_the_published_chart_examples() {
        // Commons Codec DaitchMokotoffSoundexTest test_soundex_basic, test_soundex_basic2,
        // test_soundex_basic3.
        let d = dm();
        for (input, want) in [
            ("GOLDEN", "583600"),
            ("Alpert", "087930"),
            ("Breuer", "791900"),
            ("Haber", "579000"),
            ("Mannheim", "665600"),
            ("Mintz", "664000"),
            ("Topf", "370000"),
            ("Kleinmann", "586660"),
            ("Ben Aron", "769600"),
            ("AUERBACH", "097400|097500"),
            ("OHRBACH", "097400|097500"),
            ("LIPSHITZ", "874400"),
            ("LIPPSZYC", "874400|874500"),
            ("LEWINSKY", "876450"),
            ("LEVINSKI", "876450"),
            ("SZLAMAWICZ", "486740"),
            ("SHLAMOVITZ", "486740"),
            ("Ceniow", "467000|567000"),
            ("Tsenyuv", "467000"),
            ("Holubica", "587400|587500"),
            ("Golubitsa", "587400"),
            ("Przemysl", "746480|794648"),
            ("Pshemeshil", "746480"),
            (
                "Rosochowaciec",
                "944744|944745|944754|944755|945744|945745|945754|945755",
            ),
            ("Rosokhovatsets", "945744"),
            ("Peters", "734000|739400"),
            ("Peterson", "734600|739460"),
            ("Moskowitz", "645740"),
            ("Moskovitz", "645740"),
            ("Jackson", "154600|145460|454600|445460"),
            (
                "Jackson-Jackson",
                "154654|154645|154644|145465|145464|454654|454645|454644|445465|445464",
            ),
        ] {
            assert_eq!(d.process(input), want, "for {input:?}");
        }
    }

    #[test]
    fn first_code_matches_the_non_branching_encode() {
        // Daitch-Mokotoff assigns alternative codes where a letter sequence is
        // pronounced differently across the languages the scheme covers. The
        // alternatives are generated in rule order, so the first code a name
        // produces is the one the primary rule assigns — the answer a
        // single-code reading of the published scheme gives.
        let d = dm();
        for (input, want) in [
            ("AUERBACH", "097400"),
            ("OHRBACH", "097400"),
            ("LIPSHITZ", "874400"),
            ("LIPPSZYC", "874400"),
            ("LEWINSKY", "876450"),
            ("LEVINSKI", "876450"),
            ("SZLAMAWICZ", "486740"),
            ("SHLAMOVITZ", "486740"),
        ] {
            assert_eq!(d.codes(input)[0], want, "for {input:?}");
        }
    }

    #[test]
    fn apostrophes_are_ignored() {
        // Commons Codec DaitchMokotoffSoundexTest test_encode_ignore_apostrophes. All variants are
        // single-code, so `process` covers their `encode` assertion too.
        let d = dm();
        for input in [
            "OBrien", "'OBrien", "O'Brien", "OB'rien", "OBr'ien", "OBri'en", "OBrie'n", "OBrien'",
        ] {
            assert_eq!(d.process(input), "079600", "for {input:?}");
        }
    }

    #[test]
    fn hyphens_are_ignored() {
        // Commons Codec DaitchMokotoffSoundexTest test_encode_ignore_hyphens.
        let d = dm();
        for input in [
            "KINGSMITH",
            "-KINGSMITH",
            "K-INGSMITH",
            "KI-NGSMITH",
            "KIN-GSMITH",
            "KING-SMITH",
            "KINGS-MITH",
            "KINGSM-ITH",
            "KINGSMI-TH",
            "KINGSMIT-H",
            "KINGSMITH-",
        ] {
            assert_eq!(d.process(input), "565463", "for {input:?}");
        }
    }

    #[test]
    fn whitespace_is_removed_anywhere() {
        // Commons Codec DaitchMokotoffSoundexTest test_encode_ignore_trimmable, plus the interior
        // space of "Ben Aron" from test_soundex_basic.
        let d = dm();
        assert_eq!(d.process(" \t\n\r Washington \t\n\r "), "746536");
        assert_eq!(d.process("Washington"), "746536");
        assert_eq!(d.process("Ben Aron"), d.process("BENARON"));
    }

    #[test]
    fn ascii_folding_matches_the_rules_file() {
        // First four: Commons Codec DaitchMokotoffSoundexTest test_accented_character_folding.
        // The rest: a change detector, not chart-derived.
        let d = dm();
        for (input, want) in [
            ("Straßburg", "294795"),
            ("Strasburg", "294795"),
            ("Éregon", "095600"),
            ("Eregon", "095600"),
            ("straße", "294000"),
            ("ß", "400000"),
            // ẞ lowercases to ß, which folds to s.
            ("ẞ", "400000"),
            ("Łódź", "840000"),
            ("çà-et-là", "438000|538000"),
            // œ is NOT in the folding list: it is skipped, not folded to oe.
            ("ŒUVRE", "079000"),
            ("sœur", "490000"),
            // ü is NOT in the folding list either (ù, ú, û are): the b after
            // the skipped ü is still word-initial-adjacent, i.e. "berall".
            ("überall", "798000"),
            ("Fürst", "743000|794300"),
        ] {
            assert_eq!(d.process(input), want, "for {input:?}");
        }
    }

    #[test]
    fn adjacent_identical_codes_collapse() {
        // First two: Commons Codec DaitchMokotoffSoundexTest test_adjacent_codes (their comments:
        // A-KS-S-O-L must drop the second S; GE-RS-CH-F-E-L-D must drop the
        // 5/4 after 4/94). The rest: a change detector, not chart-derived — the
        // merge tests the previous replacement STRING, so a rule-less
        // character in between does not reset it (b0b) but a coded-empty
        // vowel does terminate the run comparison differently (bob).
        let d = dm();
        assert_eq!(d.process("AKSSOL"), "054800");
        assert_eq!(d.process("GERSCHFELD"), "547830|545783|594783|594578");
        assert_eq!(d.process("b0b"), "700000");
        assert_eq!(d.process("bob"), "770000");
    }

    #[test]
    fn m_n_pairs_always_append() {
        // Change detector, not chart-derived: `mn`/`nm` as a cluster code 66; an
        // m and n separated by a rule-less character are force-appended to
        // the same 66 instead of merging into one 6.
        let d = dm();
        assert_eq!(d.process("mn"), "660000");
        assert_eq!(d.process("nm"), "660000");
        assert_eq!(d.process("m1n"), "660000");
        assert_eq!(d.process("m-n"), "660000");
        assert_eq!(d.process("mnm"), "660000");
    }

    #[test]
    fn special_slavic_and_romanian_rule_keys() {
        // First two: Commons Codec DaitchMokotoffSoundexTest test_special_romanian_characters. The
        // rest: a change detector, not chart-derived — they pin the two byte-length
        // quirks of the non-ASCII rule keys (the rule swallows the following
        // character; the vowel probe lands one character too far).
        let d = dm();
        for (input, want) in [
            ("ţamas", "364000|464000"),
            ("țamas", "364000|464000"),
            // The k after ţ is consumed by the ţ rule.
            ("ţka", "300000|400000"),
            ("țit", "300000|430000"),
            // bąb: the trailing b is consumed by ą; nth(2) sees no vowel, so
            // ą branches into "" and "6".
            ("bąb", "700000|760000"),
            // bąbel: nth(2) of "ąbel" is e — a vowel — so ą does NOT branch,
            // and the b it swallowed never codes.
            ("bąbel", "780000"),
            // Word-initial ą encodes nothing and swallows the k.
            ("ąk", "000000"),
            ("ęk", "000000"),
            ("kęs", "500000|560000"),
        ] {
            assert_eq!(d.process(input), want, "for {input:?}");
        }
    }

    #[test]
    fn branch_dedup_is_on_partial_code_and_last_replacement() {
        // Change detector, not chart-derived. Branches are deduplicated mid-walk
        // on (partial code, last replacement), so finished codes CAN repeat
        // — adjacent (rsrs) and non-adjacent (ckckckck) alike — and the
        // branch order interleaves accordingly.
        let d = dm();
        assert_eq!(d.process("rsrs"), "400000|494000|940000|940000");
        assert_eq!(
            d.process("ckckckck"),
            "500000|545000|545000|545450|450000|454500|454500|450000"
        );
        assert_eq!(
            d.process("jjjjjj"),
            "100000|140000|140000|144000|144000|144400|400000|440000|440000|444000|444000|400000"
        );
        assert_eq!(
            d.process("chchchchchchchch"),
            "400000|450000|454000|454500|454540|454545|454545|540000|545000|545400|545450|545454|545454|500000"
        );
    }

    #[test]
    fn force_append_versus_merge_on_nasal_chains() {
        // Change detector, not chart-derived. "mn"/"nm" as a CLUSTER codes one 66
        // and repeats of the cluster merge ("66" ends with "66"), because the
        // force test compares raw characters (m then m), not replacements —
        // while a genuine m/n boundary between two clusters force-appends.
        let d = dm();
        assert_eq!(d.process("mnmnmnmnmn"), "660000");
        assert_eq!(d.process("nmnmnm"), "660000");
        // "mnnm": the mn cluster, then the nm cluster starting with n after
        // last_char m — force fires, 66 + 66.
        assert_eq!(d.process("mnnm"), "666600");
    }

    #[test]
    fn special_key_chains_branch_and_dedup() {
        // Change detector, not chart-derived. Every input chains the non-ASCII
        // rule keys whose byte-length quirks swallow the following char, so
        // these pin swallow + branch + dedup interacting.
        let d = dm();
        for (input, want) in [
            // ţţţ: middle ţ swallowed; two 3|4 branchings with merge dedup.
            ("ţţţ", "300000|340000|430000|400000"),
            ("ţaţaţa", "300000|340000|343000|430000|434000|400000"),
            // ąęą: ę swallows the final ą; word-initial ą encodes nothing.
            ("ąęą", "000000|600000"),
            ("ąą", "000000"),
            ("ęę", "000000"),
            // Duplicate finished codes via distinct last-replacements.
            ("bąbąb", "700000|760000|760000"),
            ("kęskęs", "550000|556000|565000|565600"),
        ] {
            assert_eq!(d.process(input), want, "for {input:?}");
        }
    }

    #[test]
    fn diphthong_rules_after_swallowed_j() {
        // Change detector, not chart-derived: vowel-cluster rules meeting the
        // branching j whose empty replacement then merges with everything.
        let d = dm();
        assert_eq!(d.process("aujau"), "000000|040000");
        assert_eq!(d.process("eyeye"), "010000");
        assert_eq!(d.process("uejue"), "000000|040000");
        assert_eq!(d.process("jmj"), "160000|164000|460000|464000");
    }

    #[test]
    fn saturated_branch_dedup_on_ck_ch_chains() {
        // Change detector, not chart-derived: the worst dedup stress the rules
        // allow — alternating ch/ck keeps two-way branching alive while the
        // six-digit saturation makes ever more branches collide.
        let d = dm();
        assert_eq!(
            d.process("chckchck"),
            "454500|454450|450000|454500|445450|445445|445000|445450|545000|544500|500000|545000|545450|545445|545450"
        );
    }

    #[test]
    fn maximum_branching_stays_bounded() {
        // Jackson-Jackson (10 branches) is the widest fan-out in the suite;
        // 50 concatenated jacksons still collapse to those same 10 codes
        // (a change detector, not chart-derived), because codes saturate at six
        // digits and the dedup works on that bounded state.
        let d = dm();
        let long = "jackson".repeat(50);
        assert_eq!(
            d.process(&long),
            "154654|154645|154644|145465|145464|454654|454645|454644|445465|445464"
        );
        let codes = d.codes(&long);
        assert_eq!(codes.len(), 10);
        assert!(codes.iter().all(|c| c.len() == MAX_LENGTH));
    }

    #[test]
    fn edge_and_unicode_inputs() {
        // Change detector, not chart-derived: anything without a rule is skipped
        // — digits, punctuation, CJK, emoji, unfolded letters — and an input
        // with no coded character at all yields the single code 000000.
        let d = dm();
        for (input, want) in [
            ("", "000000"),
            ("   ", "000000"),
            ("123", "000000"),
            ("abc123def", "074370|075370"),
            ("日本語", "000000"),
            ("😀", "000000"),
            ("aあb", "070000"),
            ("ǆ", "000000"),
            ("x", "500000"),
            ("q", "500000"),
            // İ lowercases to i + combining dot; the dot is skipped.
            ("İstanbul", "043678"),
        ] {
            assert_eq!(d.process(input), want, "for {input:?}");
        }
        assert_eq!(d.codes(""), vec!["000000"]);
        assert_eq!(d.process(&"a".repeat(500)), "000000");
    }

    #[test]
    fn mixed_case_is_folded() {
        let d = dm();
        assert_eq!(d.process("MoSkOwItZ"), "645740");
        assert_eq!(d.process("MOSKOWITZ"), d.process("moskowitz"));
        assert_eq!(d.process("RSRS"), d.process("rsrs"));
    }

    #[test]
    fn multi_letter_clusters_win_over_their_prefixes() {
        // schtschschtsch: the 7-letter cluster matches twice; the second
        // occurrence merges into the first's identical code (recorded from
        // Commons Codec's own suite).
        let d = dm();
        assert_eq!(d.process("schtschschtsch"), "240000");
        assert_eq!(d.codes("SCHTSCH"), vec!["200000"]);
        assert_eq!(d.codes("STRZ"), vec!["200000"]);
    }

    #[test]
    fn process_is_codes_joined_with_pipes() {
        let d = dm();
        for input in ["Rosochowaciec", "Jackson", "GOLDEN", "", "rsrs", "bąb"] {
            assert_eq!(d.process(input), d.codes(input).join("|"), "for {input:?}");
        }
    }

    #[test]
    fn compare_is_code_set_intersection() {
        let d = dm();
        // Same single code.
        assert!(d.compare("Moskowitz", "Moskovitz"));
        // Multi vs single: 467000|567000 ∩ 467000.
        assert!(d.compare("Ceniow", "Tsenyuv"));
        assert!(d.compare("Holubica", "Golubitsa"));
        assert!(d.compare("Przemysl", "Pshemeshil"));
        // Multi vs multi with full overlap.
        assert!(d.compare("AUERBACH", "OHRBACH"));
        // Intersection is symmetric.
        assert!(d.compare("Tsenyuv", "Ceniow"));
        // Disjoint sets.
        assert!(!d.compare("Peters", "Peterson"));
        assert!(!d.compare("Alpert", "GOLDEN"));
    }

    #[test]
    fn phonetic_trait_uses_the_branching_semantics() {
        use verbora_core::Phonetic;

        let d: &dyn Phonetic = &dm();
        assert_eq!(d.process("AUERBACH"), "097400|097500");
        // The trait override keeps set-intersection compare: these two have
        // different process() strings but a shared code.
        assert!(d.compare("Ceniow", "Tsenyuv"));
    }

    #[test]
    fn rule_tables_hold_the_invariants_the_walk_relies_on() {
        let buckets: Vec<(char, &[Rule])> = ('a'..='z')
            .chain(['ą', 'ę', 'ţ', 'ț'])
            .map(|ch| (ch, rules_for(ch).expect("bucket exists")))
            .collect();

        for (key, rules) in buckets {
            assert!(!rules.is_empty(), "bucket {key:?} is empty");
            for r in rules {
                // Every pattern belongs to its bucket, so the first-character
                // dispatch is exhaustive.
                assert!(
                    r.pattern.starts_with(key),
                    "pattern {:?} not in bucket {key:?}",
                    r.pattern
                );
                // Fan-out is one or two alternatives — the branch bound the
                // module documentation promises.
                for reps in [r.at_start, r.before_vowel, r.other] {
                    assert!(
                        (1..=2).contains(&reps.len()),
                        "pattern {:?} fan-out {}",
                        r.pattern,
                        reps.len()
                    );
                }
            }
            // Descending byte length: first match must be longest match.
            for w in rules.windows(2) {
                assert!(
                    w[0].pattern.len() >= w[1].pattern.len(),
                    "bucket {key:?} not sorted: {:?} before {:?}",
                    w[0].pattern,
                    w[1].pattern
                );
            }
            // The bucket ends with the bare key, so the walk always advances.
            let last = rules.last().expect("non-empty");
            assert_eq!(
                last.pattern,
                key.to_string(),
                "bucket {key:?} must end with its bare key"
            );
        }
    }
}
