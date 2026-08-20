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
/// # Where the rules come from
///
/// The coding chart is the specification, and the static tables in this
/// module are that chart transcribed row for row. The transcription is not
/// taken on trust: `the_embedded_table_is_the_published_coding_chart`
/// transcribes the chart a *second* time, independently of the tables the
/// encoder consults, and asserts the two agree on all 124 rows and all three
/// context columns — so the publication, not this file, is what defines the
/// encoder, and a row that quietly stops saying what the chart says fails a
/// test rather than silently shrinking the encoder.
///
/// Two things the chart does not itself fix are taken from the
/// machine-readable rule file distributed with Apache Commons Codec
/// (`dmrules.txt`), and are cited here rather than dressed up as chart rules:
/// the closed ASCII **folding** list applied before matching, and the two
/// Romanian rows `ţ`/`ț`.
///
/// A third inheritance from that lineage is a **defect**, not a rule: the
/// four non-ASCII rule keys are one character each but two bytes, and the
/// walk advances and probes the following-vowel column by *bytes*, so each
/// swallows the character after it. `"ąk"` codes `000000`, where the chart
/// codes the `k` and gives `500000`. It is documented at `Rule::replacements`
/// and pinned — explicitly as a recording of a defect rather than as
/// specification — by `non_ascii_rule_keys_reproduce_an_inherited_byte_index_defect`.
///
/// # The contract
///
/// * **The text unit is one Unicode scalar.** Each is lowercased (taking the
///   first scalar of its lowercase mapping) and then folded through the
///   transliteration list described under "Where the rules come from"
///   (`ß`→`s`, `à`–`å`→`a`, `ł`→`l`, `ś`→`s`, `ż`/`ź`→`z`, …). That list is
///   closed: `ü`, `ě`, `œ` and other plausible letters are *not* in it.
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

/// One row of the published coding chart.
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
    /// The chart's own rule is "the character after the pattern": if it is a
    /// vowel, take the before-a-vowel column. What this implementation does
    /// is use the pattern's **byte** length as a **character** index into
    /// `context`. The two coincide for every ASCII pattern — which is every
    /// row of the chart proper — and there the probed character may itself be
    /// non-ASCII without harm, since it is then correctly not a vowel either
    /// way.
    ///
    /// For the four two-byte patterns `ą`, `ę`, `ţ` and `ț` the two do *not*
    /// coincide: the probe lands one character too far, and the matching walk
    /// in `encode_branches` likewise advances one character too many, so the
    /// rule swallows the letter after it. **This is not chart behaviour** —
    /// it is inherited from the rule file these tables were transcribed
    /// against, and it changes results (`"bąbel"` codes `780000` here, where
    /// the chart gives `778000|767800`). It is kept because changing it is a
    /// behaviour change with consumers outside this crate, and it is recorded
    /// rather than hidden: the tests that pin it say in their own names that
    /// they are pinning a defect.
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
        // Every byte of `code` is an ASCII digit or the `\0` it was
        // initialised with, because the only writes are `finish`'s `b'0'`
        // padding and `push`'s copy from a `&'static str` replacement — and
        // every replacement literal in this file's rule tables is a digit
        // string of one or two ASCII bytes. (The non-ASCII literals here are
        // rule *patterns*, never replacements.) That is what keeps this total,
        // including before `finish` has run. It is also the invariant that
        // makes `push`'s `MAX_LENGTH - len` truncation safe: a multi-byte
        // replacement added to the tables could be cut mid-character, and
        // nothing but this comment says it must not be.
        std::str::from_utf8(&self.code).expect("codes are ASCII digits")
    }
}

/// The branching walk, returning
/// finished (padded) branches in their order.
fn encode_branches(token: &str) -> Vec<Branch> {
    // Preprocessing: drop whitespace anywhere, keep the first character of
    // each lowercase mapping, then apply the ASCII folding list (see `fold`,
    // which documents where that list comes from).
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

/// The ASCII folding list, applied after lowercasing.
///
/// The coding chart is stated over unaccented Latin letters and does not
/// define a fold; this list is taken verbatim from the machine-readable rule
/// file distributed with Apache Commons Codec (`dmrules.txt`), which is cited
/// here as the reference of record for it rather than presented as a chart
/// rule. It is closed and deliberately not extended: `ü`, `ě`, `œ` and other
/// plausible candidates are absent there and therefore absent here, encoding
/// as rule-less (skipped) characters. `every_folded_scalar_reaches_a_bucket`
/// checks that every entry actually lands on a chart row.
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

// The published Daitch-Mokotoff coding chart, one bucket per first character,
// patterns in descending byte length so the first match is the longest match.
// Same-length order is immaterial — two distinct same-length patterns cannot
// match one context.
//
// `the_embedded_table_is_the_published_coding_chart` holds a second,
// independent transcription of the chart and asserts these tables against it
// row by row; `rule_tables_hold_the_invariants_the_walk_relies_on` pins the
// ordering the first-match walk depends on.

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

// Where every expected value in this module comes from.
//
// The specification for this encoder is the Daitch-Mokotoff coding chart —
// Randy Daitch and Gary Mokotoff, published from 1985 through the Jewish
// genealogical societies (*Avotaynu*) and distributed since as the
// "Daitch-Mokotoff Soundex Coding Chart". Three layers of test stand between
// that chart and this file, and it is worth knowing which layer any given
// assertion belongs to:
//
// 1. `the_embedded_table_is_the_published_coding_chart` transcribes the chart
//    a second time, independently of the static tables the encoder consults,
//    and asserts the two agree on every one of the 124 rows and all three
//    context columns. That is the test that makes the *publication*
//    normative — a row quietly losing a letter (this crate has had exactly
//    that happen once, in the Beider-Morse Romanian rules) fails here rather
//    than silently shrinking the encoder.
// 2. `every_rule_is_reachable_through_its_witness` and
//    `every_rule_pattern_encodes_through_its_own_replacements` walk *every*
//    rule and prove the table is what `process` actually consults.
// 3. The named fixtures below. Their codes are derived by walking the input
//    through the chart, and the derivation is written out beside each one so
//    a reader can check the arithmetic rather than trust it.
//
// A trace reads left to right, one entry per chart row that fired:
//
//     GOLDEN: G^5 O-NC L-8 D>3 E-NC N-6  =  5 8 3 6  ->  583600
//
// where `^` marks the "start of name" column, `>` the "before a vowel"
// column, `-` the "any other" column, `NC` is the chart's "not coded", and
// `a|b` is a branching row that follows both readings. The digits are then
// merged where the chart says adjacent same-sounding letters code once, and
// the result is padded to six digits.
//
// Every value below was additionally checked against a reference written
// from the chart alone — a separate transcription, walking by *characters*,
// with no access to this file. Of the fixtures in this module it reproduces
// all but the fourteen listed in `NOT_CHART_BEHAVIOUR` below, which is the
// honest boundary of what "derived from the publication" can claim here.
//
// ---------------------------------------------------------------------
// NOT_CHART_BEHAVIOUR — the one place a fixture pins something the chart
// does not say
//
// The four non-ASCII rule keys `ą`, `ę`, `ţ` and `ț` are each ONE character,
// but the walk advances by a pattern's **byte** length and probes the
// following-vowel column at a **byte** offset (see `Rule::replacements` and
// the `iter.nth` call in `encode_branches`). All four are two bytes in UTF-8,
// so each of them swallows the character after it and probes one character
// too far. Nothing in the coding chart produces that; it is inherited from
// the machine-readable rule file this module's tables were transcribed
// against, and it changes results:
//
//     input     this encoder        the chart, walking by characters
//     "ąk"      000000              500000      (the k is not swallowed)
//     "kęs"     500000|560000       540000|564000
//     "bąb"     700000|760000       770000|767000
//     "bąbel"   780000              778000|767800
//     "ţka"     300000|400000       350000|450000
//
// The fixtures that pin this behaviour are grouped in
// `non_ascii_rule_keys_reproduce_an_inherited_byte_index_defect` and
// `non_ascii_rule_key_chains_reproduce_the_same_defect`, are labelled as
// recordings there rather than as derivations, and each carries the code the
// chart would give instead. They are change detectors for a defect, not a
// specification of it. Deciding whether to keep the defect is a behaviour
// change that reaches outside this crate — `docs/COMPETITIVE_BENCHMARKS.md`
// records the quirk as deliberately reproduced, and
// `benchmarks/competitive/rust-competitors/tests/phonetics_correctness.rs`
// asserts byte-for-byte agreement with another implementation on it — so it
// is documented here rather than quietly changed.
//
// `ţamas`/`țamas` are NOT in that group: the character each swallows codes
// `NC` anyway, so their codes do follow from the chart.
// ---------------------------------------------------------------------
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

    // ------------------------------------------------------------------
    // Layer 1: the embedded table *is* the published chart
    // ------------------------------------------------------------------

    /// The coding chart, transcribed here a second time and independently of
    /// the `static` tables above, and asserted against them row for row.
    ///
    /// This is the test that makes the publication — not this file — the
    /// specification. It is deliberately a duplicate transcription: the
    /// failure it exists to catch is a table row that has quietly stopped
    /// saying what the chart says, which no amount of encoding fixtures can
    /// find, because a corrupted row simply encodes some other way and every
    /// fixture that does not happen to use it still passes. `verbora-phonetics`
    /// has had precisely that defect before, in the Beider-Morse Romanian
    /// rules, where four rows carried `U+FFFD` where accented vowels belonged.
    ///
    /// Both directions are asserted, so a row cannot be added to the encoder
    /// without appearing in the chart either.
    #[test]
    fn the_embedded_table_is_the_published_coding_chart() {
        /// One row of the chart: its pattern, then the codes for the three
        /// context columns — start of name, before a vowel, any other. `""`
        /// is the chart's "NC" (not coded); two entries are a row the chart
        /// marks as having more than one possible code, and the encoder
        /// follows both.
        type ChartRow = (
            &'static str,
            &'static [&'static str],
            &'static [&'static str],
            &'static [&'static str],
        );

        let chart: &[ChartRow] = &[
            // A
            ("ai", &["0"], &["1"], &[""]),
            ("aj", &["0"], &["1"], &[""]),
            ("ay", &["0"], &["1"], &[""]),
            ("au", &["0"], &["7"], &[""]),
            ("a", &["0"], &[""], &[""]),
            // B
            ("b", &["7"], &["7"], &["7"]),
            // C
            ("chs", &["5"], &["54"], &["54"]),
            ("csz", &["4"], &["4"], &["4"]),
            ("czs", &["4"], &["4"], &["4"]),
            ("cz", &["4"], &["4"], &["4"]),
            ("cs", &["4"], &["4"], &["4"]),
            ("ch", &["4", "5"], &["4", "5"], &["4", "5"]),
            ("ck", &["5", "45"], &["5", "45"], &["5", "45"]),
            ("c", &["4", "5"], &["4", "5"], &["4", "5"]),
            // D
            ("drz", &["4"], &["4"], &["4"]),
            ("drs", &["4"], &["4"], &["4"]),
            ("dsh", &["4"], &["4"], &["4"]),
            ("dsz", &["4"], &["4"], &["4"]),
            ("dzh", &["4"], &["4"], &["4"]),
            ("dzs", &["4"], &["4"], &["4"]),
            ("ds", &["4"], &["4"], &["4"]),
            ("dz", &["4"], &["4"], &["4"]),
            ("dt", &["3"], &["3"], &["3"]),
            ("d", &["3"], &["3"], &["3"]),
            // E
            ("ei", &["0"], &["1"], &[""]),
            ("ej", &["0"], &["1"], &[""]),
            ("ey", &["0"], &["1"], &[""]),
            ("eu", &["1"], &["1"], &[""]),
            ("e", &["0"], &[""], &[""]),
            // F
            ("fb", &["7"], &["7"], &["7"]),
            ("f", &["7"], &["7"], &["7"]),
            // G
            ("g", &["5"], &["5"], &["5"]),
            // H
            ("h", &["5"], &["5"], &[""]),
            // I
            ("ia", &["1"], &[""], &[""]),
            ("ie", &["1"], &[""], &[""]),
            ("io", &["1"], &[""], &[""]),
            ("iu", &["1"], &[""], &[""]),
            ("i", &["0"], &[""], &[""]),
            // J
            ("j", &["1", "4"], &["", "4"], &["", "4"]),
            // K
            ("ks", &["5"], &["54"], &["54"]),
            ("kh", &["5"], &["5"], &["5"]),
            ("k", &["5"], &["5"], &["5"]),
            // L
            ("l", &["8"], &["8"], &["8"]),
            // M
            ("mn", &["66"], &["66"], &["66"]),
            ("m", &["6"], &["6"], &["6"]),
            // N
            ("nm", &["66"], &["66"], &["66"]),
            ("n", &["6"], &["6"], &["6"]),
            // O
            ("oi", &["0"], &["1"], &[""]),
            ("oj", &["0"], &["1"], &[""]),
            ("oy", &["0"], &["1"], &[""]),
            ("o", &["0"], &[""], &[""]),
            // P
            ("pf", &["7"], &["7"], &["7"]),
            ("ph", &["7"], &["7"], &["7"]),
            ("p", &["7"], &["7"], &["7"]),
            // Q
            ("q", &["5"], &["5"], &["5"]),
            // R
            ("rs", &["4", "94"], &["4", "94"], &["4", "94"]),
            ("rz", &["4", "94"], &["4", "94"], &["4", "94"]),
            ("r", &["9"], &["9"], &["9"]),
            // S
            ("schtsch", &["2"], &["4"], &["4"]),
            ("schtsh", &["2"], &["4"], &["4"]),
            ("schtch", &["2"], &["4"], &["4"]),
            ("shtch", &["2"], &["4"], &["4"]),
            ("shtsh", &["2"], &["4"], &["4"]),
            ("stsch", &["2"], &["4"], &["4"]),
            ("shch", &["2"], &["4"], &["4"]),
            ("scht", &["2"], &["43"], &["43"]),
            ("schd", &["2"], &["43"], &["43"]),
            ("stch", &["2"], &["4"], &["4"]),
            ("strz", &["2"], &["4"], &["4"]),
            ("strs", &["2"], &["4"], &["4"]),
            ("stsh", &["2"], &["4"], &["4"]),
            ("szcz", &["2"], &["4"], &["4"]),
            ("szcs", &["2"], &["4"], &["4"]),
            ("sch", &["4"], &["4"], &["4"]),
            ("sht", &["2"], &["43"], &["43"]),
            ("szt", &["2"], &["43"], &["43"]),
            ("shd", &["2"], &["43"], &["43"]),
            ("szd", &["2"], &["43"], &["43"]),
            ("sh", &["4"], &["4"], &["4"]),
            ("sc", &["2"], &["4"], &["4"]),
            ("st", &["2"], &["43"], &["43"]),
            ("sd", &["2"], &["43"], &["43"]),
            ("sz", &["4"], &["4"], &["4"]),
            ("s", &["4"], &["4"], &["4"]),
            // T
            ("ttsch", &["4"], &["4"], &["4"]),
            ("ttch", &["4"], &["4"], &["4"]),
            ("tsch", &["4"], &["4"], &["4"]),
            ("ttsz", &["4"], &["4"], &["4"]),
            ("tch", &["4"], &["4"], &["4"]),
            ("trz", &["4"], &["4"], &["4"]),
            ("trs", &["4"], &["4"], &["4"]),
            ("tsh", &["4"], &["4"], &["4"]),
            ("tts", &["4"], &["4"], &["4"]),
            ("ttz", &["4"], &["4"], &["4"]),
            ("tzs", &["4"], &["4"], &["4"]),
            ("tsz", &["4"], &["4"], &["4"]),
            ("th", &["3"], &["3"], &["3"]),
            ("ts", &["4"], &["4"], &["4"]),
            ("tc", &["4"], &["4"], &["4"]),
            ("tz", &["4"], &["4"], &["4"]),
            ("t", &["3"], &["3"], &["3"]),
            // U
            ("ui", &["0"], &["1"], &[""]),
            ("uj", &["0"], &["1"], &[""]),
            ("uy", &["0"], &["1"], &[""]),
            ("ue", &["0"], &["1"], &[""]),
            ("u", &["0"], &[""], &[""]),
            // V
            ("v", &["7"], &["7"], &["7"]),
            // W
            ("w", &["7"], &["7"], &["7"]),
            // X
            ("x", &["5"], &["54"], &["54"]),
            // Y
            ("y", &["1"], &[""], &[""]),
            // Z
            ("zhdzh", &["2"], &["4"], &["4"]),
            ("zdzh", &["2"], &["4"], &["4"]),
            ("zsch", &["4"], &["4"], &["4"]),
            ("zdz", &["2"], &["4"], &["4"]),
            ("zhd", &["2"], &["43"], &["43"]),
            ("zsh", &["4"], &["4"], &["4"]),
            ("zd", &["2"], &["43"], &["43"]),
            ("zh", &["4"], &["4"], &["4"]),
            ("zs", &["4"], &["4"], &["4"]),
            ("z", &["4"], &["4"], &["4"]),
            // Ą
            ("ą", &[""], &[""], &["", "6"]),
            // Ę
            ("ę", &[""], &[""], &["", "6"]),
            // Ţ
            ("ţ", &["3", "4"], &["3", "4"], &["3", "4"]),
            // Ț
            ("ț", &["3", "4"], &["3", "4"], &["3", "4"]),
        ];

        // Every embedded rule appears in the chart, with the same columns.
        let mut embedded = 0usize;
        for (key, bucket) in every_bucket() {
            for r in bucket {
                embedded += 1;
                let row = chart
                    .iter()
                    .find(|(pattern, ..)| *pattern == r.pattern)
                    .unwrap_or_else(|| {
                        panic!(
                            "bucket {key:?} has rule {:?}, absent from the chart",
                            r.pattern
                        )
                    });
                assert_eq!(row.1, r.at_start, "{:?}: start-of-name column", r.pattern);
                assert_eq!(
                    row.2, r.before_vowel,
                    "{:?}: before-a-vowel column",
                    r.pattern
                );
                assert_eq!(row.3, r.other, "{:?}: any-other column", r.pattern);
            }
        }

        // ... and every chart row is reachable in the encoder, so the chart
        // cannot lose a row without this failing either.
        for (pattern, at_start, before_vowel, other) in chart {
            let first = pattern.chars().next().expect("no empty pattern");
            let bucket = rules_for(first)
                .unwrap_or_else(|| panic!("chart row {pattern:?} reaches no bucket"));
            let r = bucket
                .iter()
                .find(|r| r.pattern == *pattern)
                .unwrap_or_else(|| panic!("chart row {pattern:?} is not in the embedded table"));
            assert_eq!(&r.at_start, at_start, "{pattern:?}");
            assert_eq!(&r.before_vowel, before_vowel, "{pattern:?}");
            assert_eq!(&r.other, other, "{pattern:?}");
        }

        assert_eq!(chart.len(), 124, "the chart holds 124 rows");
        assert_eq!(embedded, 124, "the embedded table holds 124 rules");
    }

    // ------------------------------------------------------------------
    // Layer 2: the table is what the encoder consults
    // ------------------------------------------------------------------

    /// **Enumeration, not sampling.** Walks *every* rule of *every* bucket
    /// through the documented pipeline and asserts the rule is reachable —
    /// that is, that a word beginning with its pattern selects that rule and
    /// no earlier one.
    ///
    /// The failure it exists to catch is a rule shadowed by an earlier entry
    /// in its own bucket: the walk takes the **first** match, so a rule whose
    /// pattern is a strict extension of an earlier rule's can never fire, and
    /// would sit in the table forever looking like specified behaviour while
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

    #[test]
    fn rule_tables_hold_the_invariants_the_walk_relies_on() {
        for (key, rules) in every_bucket() {
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

    // ------------------------------------------------------------------
    // Layer 3: the chart's own worked examples, each derived
    // ------------------------------------------------------------------

    /// The names the coding chart itself works through, with the derivation
    /// of each written out.
    ///
    /// The pairs are the chart's point: two spellings of one name reaching
    /// one code. The four spelling pairs at the end are its "letters that may
    /// have more than one possible code" examples, where the ambiguous
    /// spelling produces several codes and the unambiguous one produces the
    /// single code that must be among them.
    #[test]
    fn encodes_the_published_chart_examples() {
        let d = dm();
        for (input, want) in [
            // G^5 O-NC L-8 D>3 E-NC N-6
            ("GOLDEN", "583600"),
            // A^0 L-8 P>7 E-NC R-9 T-3
            ("Alpert", "087930"),
            // B^7 R>9 EU>1 E-NC R-9
            ("Breuer", "791900"),
            // H^5 A-NC B>7 E-NC R-9
            ("Haber", "579000"),
            // M^6 A-NC N-6 N-6 H>5 EI-NC M-6 — the second N merges into the
            // first (same sound, coded once), so 6 6 5 6
            ("Mannheim", "665600"),
            // M^6 I-NC N-6 TZ-4
            ("Mintz", "664000"),
            // T^3 O-NC PF-7
            ("Topf", "370000"),
            // K^5 L>8 EI-NC NM>66 A-NC N-6 N-6 — NM is the chart's 66 row,
            // and the trailing NN merges to one 6
            ("Kleinmann", "586660"),
            // B^7 E-NC N>6 A-NC R>9 O-NC N-6, the space removed first
            ("Ben Aron", "769600"),
            // AU^0 E-NC R-9 B>7 A-NC CH-4|5 — CH is the chart's branching
            // row: read as TCH (4) or as KH (5)
            ("AUERBACH", "097400|097500"),
            // O^0 H-NC R-9 B>7 A-NC CH-4|5 — H is NC other than at the start
            // or before a vowel, so OHRBACH lands on AUERBACH's codes
            ("OHRBACH", "097400|097500"),
            // U^0 H-NC R-9 B>7 A-NC CH-4|5 — the third spelling of the same
            ("Uhrbach", "097400|097500"),
            // L^8 I-NC P-7 SH>4 I-NC TZ-4
            ("LIPSHITZ", "874400"),
            // L^8 I-NC P-7 P-7 SZ-4 Y-NC C-4|5 — the second P merges away
            ("LIPPSZYC", "874400|874500"),
            // L^8 I-NC P-7 S>4 I-NC TZ-4 — a third spelling of LIPSHITZ
            ("Lipsitz", "874400"),
            // L^8 E-NC W>7 I-NC N-6 S-4 K-5 Y-NC
            ("LEWINSKY", "876450"),
            // L^8 E-NC V>7 I-NC N-6 S-4 K>5 I-NC — W and V both code 7
            ("LEVINSKI", "876450"),
            // SZ^4 L>8 A-NC M>6 A-NC W>7 I-NC CZ-4
            ("SZLAMAWICZ", "486740"),
            // SH^4 L>8 A-NC M>6 O-NC V>7 I-NC TZ-4
            ("SHLAMOVITZ", "486740"),
            // C^4|5 E-NC N>6 IO-NC W-7 — the chart's first alternate-code
            // pair: C reads as TZ (4) or as K (5) ...
            ("Ceniow", "467000|567000"),
            // ... and TS^4 E-NC N-6 Y>NC U-NC V-7 spells out the 4 reading
            ("Tsenyuv", "467000"),
            // H^5 O-NC L>8 U-NC B>7 I-NC C>4|5 A-NC
            ("Holubica", "587400|587500"),
            // G^5 O-NC L>8 U-NC B>7 I-NC TS>4 A-NC
            ("Golubitsa", "587400"),
            // P^7 RZ>4|94 E-NC M-6 Y-NC S-4 L-8 — RZ reads as Polish (4) or
            // as German (94), and the 94 pushes the code one digit along
            ("Przemysl", "746480|794648"),
            // P^7 SH>4 E-NC M>6 E-NC SH>4 I-NC L-8
            ("Pshemeshil", "746480"),
            // R^9 O-NC S>4 O-NC CH>4|5 O-NC W>7 A-NC C>4|5 IE-NC C-4|5 —
            // three independent branching rows, hence 2x2x2 = 8 codes
            (
                "Rosochowaciec",
                "944744|944745|944754|944755|945744|945745|945754|945755",
            ),
            // R^9 O-NC S>4 O-NC KH>5 O-NC V>7 A-NC TS>4 E-NC TS-4 — the
            // unambiguous spelling, one of the eight above
            ("Rosokhovatsets", "945744"),
            // P^7 E-NC T>3 E-NC RS-4|94
            ("Peters", "734000|739400"),
            // P^7 E-NC T>3 E-NC RS>4|94 O-NC N-6
            ("Peterson", "734600|739460"),
            // M^6 O-NC S-4 K>5 O-NC W>7 I-NC TZ-4
            ("Moskowitz", "645740"),
            // M^6 O-NC S-4 K>5 O-NC V>7 I-NC TZ-4
            ("Moskovitz", "645740"),
        ] {
            assert_eq!(d.process(input), want, "for {input:?}");
        }
    }

    /// The chart specifies a **set** of codes for a name, not an ordered list
    /// and not a multiset: "the name may be coded 097400 or 097500". So the
    /// chart's own examples are pinned here as sets, which is all the
    /// publication actually claims.
    ///
    /// The order those codes come out in, and the duplicates the walk can
    /// emit, are Verbora's own contract rather than the chart's — they are
    /// pinned separately, in `branch_order_and_duplicates_are_verboras_own_contract`,
    /// so that a change to branch ordering fails there (where the contract
    /// lives) instead of here (where the publication does).
    #[test]
    fn published_examples_are_pinned_as_code_sets() {
        use std::collections::BTreeSet;

        let d = dm();
        let set = |name: &str| -> BTreeSet<String> { d.codes(name).into_iter().collect() };
        let expected = |codes: &[&str]| -> BTreeSet<String> {
            codes.iter().map(|c| (*c).to_owned()).collect()
        };

        for (name, codes) in [
            ("GOLDEN", &["583600"][..]),
            ("Alpert", &["087930"]),
            ("Breuer", &["791900"]),
            ("Haber", &["579000"]),
            ("Mannheim", &["665600"]),
            ("Mintz", &["664000"]),
            ("Topf", &["370000"]),
            ("Kleinmann", &["586660"]),
            ("Ben Aron", &["769600"]),
            ("AUERBACH", &["097400", "097500"]),
            ("OHRBACH", &["097400", "097500"]),
            ("Uhrbach", &["097400", "097500"]),
            ("LIPSHITZ", &["874400"]),
            ("LIPPSZYC", &["874400", "874500"]),
            ("Lipsitz", &["874400"]),
            ("LEWINSKY", &["876450"]),
            ("LEVINSKI", &["876450"]),
            ("SZLAMAWICZ", &["486740"]),
            ("SHLAMOVITZ", &["486740"]),
            ("Ceniow", &["467000", "567000"]),
            ("Tsenyuv", &["467000"]),
            ("Holubica", &["587400", "587500"]),
            ("Golubitsa", &["587400"]),
            ("Przemysl", &["746480", "794648"]),
            ("Pshemeshil", &["746480"]),
            (
                "Rosochowaciec",
                &[
                    "944744", "944745", "944754", "944755", "945744", "945745", "945754", "945755",
                ],
            ),
            ("Rosokhovatsets", &["945744"]),
            ("Peters", &["734000", "739400"]),
            ("Peterson", &["734600", "739460"]),
            ("Moskowitz", &["645740"]),
            ("Moskovitz", &["645740"]),
        ] {
            assert_eq!(set(name), expected(codes), "for {name:?}");
        }

        // The chart's pairs are pairs because their code sets meet. That is
        // the claim the publication actually makes about them, and it is what
        // `compare` implements.
        for (a, b) in [
            ("AUERBACH", "OHRBACH"),
            ("AUERBACH", "Uhrbach"),
            ("LIPSHITZ", "LIPPSZYC"),
            ("LIPSHITZ", "Lipsitz"),
            ("LEWINSKY", "LEVINSKI"),
            ("SZLAMAWICZ", "SHLAMOVITZ"),
            ("Ceniow", "Tsenyuv"),
            ("Holubica", "Golubitsa"),
            ("Przemysl", "Pshemeshil"),
            ("Rosochowaciec", "Rosokhovatsets"),
            ("Moskowitz", "Moskovitz"),
        ] {
            assert!(
                !set(a).is_disjoint(&set(b)),
                "the chart pairs {a:?} with {b:?}, but their code sets are disjoint"
            );
            assert!(d.compare(a, b), "{a:?} vs {b:?}");
        }
    }

    #[test]
    fn first_code_matches_the_non_branching_encode() {
        // Where a chart row offers more than one reading, the alternatives
        // are followed in the order the row lists them, so the first code a
        // name produces is the one its primary reading assigns — the answer
        // a single-code reading of the chart gives. The chart's own examples
        // are listed in that same order (AUERBACH "097400 and 097500"), which
        // is what fixes which reading is primary.
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
            ("Ceniow", "467000"),
            ("Holubica", "587400"),
            ("Przemysl", "746480"),
            ("Rosochowaciec", "944744"),
            ("Peters", "734000"),
        ] {
            assert_eq!(d.codes(input)[0], want, "for {input:?}");
        }
    }

    // ------------------------------------------------------------------
    // Preprocessing: what is removed before the chart is consulted
    // ------------------------------------------------------------------

    #[test]
    fn apostrophes_are_ignored() {
        // O^0 B-7 R>9 IE-NC N-6 in every variant: an apostrophe carries no
        // chart row, so it is skipped — and skipping it does not end the
        // start-of-name context, which is why a leading "'O" still codes its
        // O as 0 rather than NC.
        let d = dm();
        for input in [
            "OBrien", "'OBrien", "O'Brien", "OB'rien", "OBr'ien", "OBri'en", "OBrie'n", "OBrien'",
        ] {
            assert_eq!(d.process(input), "079600", "for {input:?}");
        }
    }

    #[test]
    fn hyphens_are_ignored() {
        // K^5 I-NC N-6 G-5 S-4 M>6 I-NC TH-3, wherever the hyphen falls.
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
        // The chart is stated over a surname written as one word, so a space
        // is removed rather than treated as a word boundary:
        // W^7 A-NC SH>4 I-NC N-6 G-5 T>3 O-NC N-6.
        let d = dm();
        assert_eq!(d.process(" \t\n\r Washington \t\n\r "), "746536");
        assert_eq!(d.process("Washington"), "746536");
        // "Ben Aron" is the chart's own example of this: it codes as BENARON.
        assert_eq!(d.process("Ben Aron"), d.process("BENARON"));
    }

    #[test]
    fn ascii_folding_matches_the_rules_file() {
        // The chart is stated over unaccented Latin letters and does not
        // itself define a folding list; this one comes from the
        // machine-readable rule file the tables were transcribed against, and
        // is closed — `ü`, `ě` and `œ` are simply absent from it. Every code
        // below is then derived from the chart, applied to the folded text.
        let d = dm();
        for (input, want) in [
            // ß folds to s: ST^2 R>9 A-NC S-4 B>7 U-NC R-9 G-5
            ("Straßburg", "294795"),
            ("Strasburg", "294795"),
            // É folds to e: E^0 R>9 E-NC G>5 O-NC N-6
            ("Éregon", "095600"),
            ("Eregon", "095600"),
            // ST^2 R>9 A-NC S>4 E-NC
            ("straße", "294000"),
            // S^4 alone
            ("ß", "400000"),
            // ẞ lowercases to ß, which folds to s — the two cases of one
            // letter must not disagree
            ("ẞ", "400000"),
            // ł folds to l, ó to o, ź to z: L^8 O-NC DZ-4 (the chart's DZ row
            // claims both letters)
            ("Łódź", "840000"),
            // ç folds to c and à to a: C^4|5 A-NC E-NC T-3 L>8 A-NC
            ("çà-et-là", "438000|538000"),
            // œ is NOT in the folding list: it is skipped rather than folded
            // to oe, so this codes as UVRE — U^0 V-7 R>9 E-NC
            ("ŒUVRE", "079000"),
            // ... and here as SUR — S^4 U-NC R-9
            ("sœur", "490000"),
            // ü is NOT in the list either (ù, ú and û are), so this codes as
            // BERALL, and the B is still start-of-name: B^7 E-NC R>9 A-NC L-8 L-8
            ("überall", "798000"),
            // F^7 RS-4|94 T-3, the ü skipped between F and R
            ("Fürst", "743000|794300"),
        ] {
            assert_eq!(d.process(input), want, "for {input:?}");
        }
    }

    // ------------------------------------------------------------------
    // The chart's merge rule: adjacent same-sounding letters code once
    // ------------------------------------------------------------------

    #[test]
    fn adjacent_identical_codes_collapse() {
        let d = dm();
        // A^0 KS-54 S>4 O-NC L-8. The chart's own worked example of the merge:
        // KS already ends in the S sound, so the following S is not coded
        // again. 0 54 8 -> 054800.
        assert_eq!(d.process("AKSSOL"), "054800");
        // G^5 E-NC RS-4|94 CH-4|5 F>7 E-NC L-8 D-3. Two branching rows meet,
        // and in the branch where RS is 4 the following CH's 4 merges into it
        // — which is why the four codes are not simply the 2x2 product with
        // one digit each.
        assert_eq!(d.process("GERSCHFELD"), "547830|545783|594783|594578");
        // The merge compares the previous replacement, and a character with
        // no chart row does not reset it: in "b0b" the two Bs are still
        // adjacent and code once.
        assert_eq!(d.process("b0b"), "700000"); // B^7 B-7 (merged)
        // A vowel does carry a row — coded NC — and that NC is what separates
        // them, so "bob" codes both Bs.
        assert_eq!(d.process("bob"), "770000"); // B^7 O-NC B-7
    }

    #[test]
    fn m_n_pairs_always_append() {
        // The chart gives MN and NM their own row, 66 — an M followed by an N
        // is *not* the merge case, it is two coded sounds. This is the one
        // exception to the merge rule above.
        let d = dm();
        assert_eq!(d.process("mn"), "660000"); // MN^66
        assert_eq!(d.process("nm"), "660000"); // NM^66
        // Separated by a character with no chart row, the pair is still an
        // m/n pair and still appends twice rather than merging to one 6.
        assert_eq!(d.process("m1n"), "660000"); // M^6 N-6, forced
        assert_eq!(d.process("m-n"), "660000");
        // MN^66 then M-6, whose 6 merges into the 66's trailing 6.
        assert_eq!(d.process("mnm"), "660000");
    }

    #[test]
    fn force_append_versus_merge_on_nasal_chains() {
        let d = dm();
        // The MN row matches greedily, so a run of alternating letters is a
        // run of MN clusters, each coding 66 — and 66 merges into 66, because
        // the merge compares replacement strings.
        assert_eq!(d.process("mnmnmnmnmn"), "660000");
        assert_eq!(d.process("nmnmnm"), "660000");
        // "mnnm" is MN^66 then NM-66, and the m/n exception fires at the
        // boundary between the two clusters, so both are appended: 66 66.
        assert_eq!(d.process("mnnm"), "666600");
    }

    // ------------------------------------------------------------------
    // Branching, and the ordering contract around it
    // ------------------------------------------------------------------

    /// Branch order and duplicate codes are **Verbora's** contract, not the
    /// chart's: the chart says a name "may be coded X or Y" and stops there.
    /// This module's documentation states the rest — branches fan out in rule
    /// order and are deduplicated mid-walk on (partial code, last
    /// replacement) rather than on finished codes, which is exactly why equal
    /// codes can reach the output by different routes.
    ///
    /// These fixtures are derived from the chart's rows plus that stated
    /// contract, and were checked against a reference implementing both.
    /// They belong here, and not in the published-example tests, because the
    /// publication does not make this claim.
    #[test]
    fn branch_order_and_duplicates_are_verboras_own_contract() {
        let d = dm();
        // RS^4|94 RS-4|94. Four routes, three distinct codes: the branch that
        // takes 4 then 4 merges the second away and lands on 400000, while
        // 94-then-4 and 4-then-94 both reach 940000 by different routes and
        // both survive, because the dedup key includes the last replacement.
        assert_eq!(d.process("rsrs"), "400000|494000|940000|940000");
        // The same shape on the chart's CK row (5 or 45), four rows deep.
        assert_eq!(
            d.process("ckckckck"),
            "500000|545000|545000|545450|450000|454500|454500|450000"
        );
        // J^1|4 then five J-NC|4: the NC alternative appends nothing, so the
        // branch count stays bounded while the codes saturate.
        assert_eq!(
            d.process("jjjjjj"),
            "100000|140000|140000|144000|144000|144400|400000|440000|440000|444000|444000|400000"
        );
        // Eight CH rows, each 4 or 5.
        assert_eq!(
            d.process("chchchchchchchch"),
            "400000|450000|454000|454500|454540|454545|454545|540000|545000|545400|545450|545454|545454|500000"
        );
        // The widest fan-out the chart's rows allow in this suite: alternating
        // CH (4|5) and CK (5|45) keeps two-way branching alive while six-digit
        // saturation makes ever more branches collide.
        assert_eq!(
            d.process("chckchck"),
            "454500|454450|450000|454500|445450|445445|445000|445450|545000|544500|500000|545000|545450|545445|545450"
        );
    }

    #[test]
    fn branching_j_meets_the_vowel_rows() {
        // The chart's J row reads as Y (1 at the start, NC elsewhere) or as
        // DZH (4), and its NC alternative then merges with whatever precedes.
        let d = dm();
        // AU^0 J>NC|4 AU-NC
        assert_eq!(d.process("aujau"), "000000|040000");
        // EY^0 EY>1 E-NC — no J at all, so no branching: 0 1.
        assert_eq!(d.process("eyeye"), "010000");
        // UE^0 J>NC|4 UE-NC — the chart's UE row behaves like the other
        // U diphthongs
        assert_eq!(d.process("uejue"), "000000|040000");
        // J^1|4 M-6 J-NC|4 — two independent J rows, four codes
        assert_eq!(d.process("jmj"), "160000|164000|460000|464000");
    }

    #[test]
    fn multi_letter_clusters_win_over_their_prefixes() {
        // SCHTSCH is the chart's longest row; the walk must prefer it over
        // the SCH, SHT, SCH... rows whose patterns it begins with.
        let d = dm();
        // SCHTSCH^2 SCHTSCH-4: the row matches twice, and the second
        // occurrence's 4 does not merge with the first's 2.
        assert_eq!(d.process("schtschschtsch"), "240000");
        assert_eq!(d.codes("SCHTSCH"), vec!["200000"]);
        // Likewise STRZ, which begins with the ST row's pattern.
        assert_eq!(d.codes("STRZ"), vec!["200000"]);
    }

    #[test]
    fn jackson_branches_on_j_and_ck() {
        // Not a chart example — derived here, because it is the widest
        // fan-out this suite uses. J^1|4 A-NC CK-5|45 S>4 O-NC N-6: the J row
        // gives 1 or 4, the CK row 5 or 45, and 2x2 = 4 codes. In the 45
        // branch the following S's 4 does *not* merge, because "45" ends in
        // 5, not 4.
        let d = dm();
        assert_eq!(d.process("Jackson"), "154600|145460|454600|445460");
        // Doubling the name doubles the branching before saturation caps it
        // at ten distinct routes.
        assert_eq!(
            d.process("Jackson-Jackson"),
            "154654|154645|154644|145465|145464|454654|454645|454644|445465|445464"
        );
        // Fifty of them still collapse to those same ten, because codes
        // saturate at six digits and the dedup works on that bounded state.
        let long = "jackson".repeat(50);
        assert_eq!(
            d.process(&long),
            "154654|154645|154644|145465|145464|454654|454645|454644|445465|445464"
        );
        let codes = d.codes(&long);
        assert_eq!(codes.len(), 10);
        assert!(codes.iter().all(|c| c.len() == MAX_LENGTH));
    }

    // ------------------------------------------------------------------
    // The four non-ASCII rule keys: a recording, not a derivation
    // ------------------------------------------------------------------

    /// **These expected values do not follow from the coding chart.** See
    /// this module's `NOT_CHART_BEHAVIOUR` note above for the full statement;
    /// in short, `ą`, `ę`, `ţ` and `ț` are one character each but two bytes,
    /// and the walk advances and probes by bytes, so each swallows the
    /// character after it and reads the following-vowel column one character
    /// too far.
    ///
    /// Each fixture therefore carries the code the chart gives, alongside the
    /// code this encoder gives, so the divergence is written down rather than
    /// implied. They are kept as change detectors for the defect — removing
    /// them would leave the behaviour unpinned — and are deliberately not
    /// presented as specification.
    #[test]
    fn non_ascii_rule_keys_reproduce_an_inherited_byte_index_defect() {
        // (input, what this encoder gives, what the chart gives)
        let fixtures: &[(&str, &str, &str)] = &[
            // These two agree with the chart: Ţ^3|4 A-NC M>6 A-NC S-4. The
            // character the rule swallows is an `a`, which codes NC anyway,
            // so the defect happens not to change the answer here.
            ("ţamas", "364000|464000", "364000|464000"),
            ("țamas", "364000|464000", "364000|464000"),
            // The k after ţ is swallowed. The chart codes it: K-5.
            ("ţka", "300000|400000", "350000|450000"),
            // The i after ț is swallowed. The chart: Ț^3|4 I-NC T-3.
            ("țit", "300000|430000", "330000|430000"),
            // The trailing b is swallowed, and the vowel probe lands past the
            // end. The chart: B^7 Ą-NC|6 B-7.
            ("bąb", "700000|760000", "770000|767000"),
            // The probe lands on the `e` of "bel" and reads a vowel, so ą
            // does not branch, and the b it swallowed never codes. The chart:
            // B^7 Ą-NC|6 B-7 E-NC L-8.
            ("bąbel", "780000", "778000|767800"),
            // Word-initial ą codes nothing and swallows the k. The chart
            // codes the k: K-5.
            ("ąk", "000000", "500000"),
            ("ęk", "000000", "500000"),
            // The s after ę is swallowed. The chart: K^5 Ę-NC|6 S-4.
            ("kęs", "500000|560000", "540000|564000"),
        ];

        let d = dm();
        for &(input, want, _chart) in fixtures {
            assert_eq!(d.process(input), want, "for {input:?}");
        }

        // Seven of the nine diverge from the chart; the two `ţamas` spellings
        // do not, because the character their rule swallows codes NC either
        // way. Asserting the count means that fixing the defect fails *here*,
        // loudly, pointing at the note above — rather than silently turning
        // these fixtures into ordinary chart derivations while the comments
        // still call them recordings.
        let diverging = fixtures
            .iter()
            .filter(|(_, want, chart)| want != chart)
            .count();
        assert_eq!(
            diverging, 7,
            "the byte-index defect is documented as affecting exactly these seven"
        );
    }

    /// The same defect, chained — the swallow, the branching and the dedup
    /// interacting. Recordings for the same reason as the test above, and
    /// derived from nothing: the chart does not produce these.
    #[test]
    fn non_ascii_rule_key_chains_reproduce_the_same_defect() {
        let d = dm();
        for (input, want, chart_would_give) in [
            (
                "ţţţ",
                "300000|340000|430000|400000",
                "300000|340000|343000|430000|434000|400000",
            ),
            (
                "ţaţaţa",
                "300000|340000|343000|430000|434000|400000",
                "333000|334000|343000|344000|433000|434000|443000|444000",
            ),
            ("ąęą", "000000|600000", "000000|600000|600000"),
            ("ąą", "000000", "000000|600000"),
            ("ęę", "000000", "000000|600000"),
            (
                "bąbąb",
                "700000|760000|760000",
                "777000|776700|767700|767670",
            ),
            (
                "kęskęs",
                "550000|556000|565000|565600",
                "545400|545640|564540|564564",
            ),
        ] {
            assert_eq!(d.process(input), want, "for {input:?}");
            assert_ne!(
                want, chart_would_give,
                "{input:?}: this fixture is only interesting while it diverges"
            );
        }
    }

    // ------------------------------------------------------------------
    // Totality, Unicode, and the API's own shape
    // ------------------------------------------------------------------

    #[test]
    fn edge_and_unicode_inputs() {
        // A scalar with no chart row is skipped without any other effect, and
        // an input with no coded character at all still yields one code, the
        // six-digit padding by itself.
        let d = dm();
        for (input, want) in [
            ("", "000000"),
            ("   ", "000000"),
            ("123", "000000"),
            // A^0 B-7 C-4|5 D>3 E-NC F-7, the digits skipped
            ("abc123def", "074370|075370"),
            ("日本語", "000000"),
            ("😀", "000000"),
            // A^0 B-7, the kana skipped
            ("aあb", "070000"),
            // U+01C6 is not folded and has no row of its own
            ("ǆ", "000000"),
            ("x", "500000"), // X^5
            ("q", "500000"), // Q^5
            // İ lowercases to i + a combining dot; the dot has no row and is
            // skipped, so this is ISTANBUL: I^0 ST>43 A-NC N-6 B>7 U-NC L-8,
            // which fills all six digits exactly
            ("İstanbul", "043678"),
        ] {
            assert_eq!(d.process(input), want, "for {input:?}");
        }
        assert_eq!(d.codes(""), vec!["000000"]);
        // Every A after the first codes NC, so a long vowel run is padding.
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
        // Multi vs single: {467000, 567000} meets {467000}.
        assert!(d.compare("Ceniow", "Tsenyuv"));
        assert!(d.compare("Holubica", "Golubitsa"));
        assert!(d.compare("Przemysl", "Pshemeshil"));
        // Multi vs multi with full overlap.
        assert!(d.compare("AUERBACH", "OHRBACH"));
        // Intersection is symmetric.
        assert!(d.compare("Tsenyuv", "Ceniow"));
        // Disjoint sets: {734000, 739400} and {734600, 739460} share nothing.
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
}
