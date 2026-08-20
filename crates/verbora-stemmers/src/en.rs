//! The English Porter stemmer, ported from the reference `porter_stemmer`.
//!
//! # Three things a careful reading gets wrong
//!
//! **`measure` is a floating-point number.** It is `categorizeGroups(token)`
//! with a leading `C` and a trailing `V` stripped, divided by two — and that
//! string can have odd length, because the `V` the second pass injects is not
//! itself in `[aeiou]` and so cannot merge with a later vowel run. `measure("sya")`
//! is **0.5**, `measure("syaing")` is 1.5, `measure("fifugyed")` is 3.5. Every
//! guard in the algorithm is `> 0`, `> 1` or `=== 1`, and an integer port changes
//! all of them.
//!
//! **`attemptReplacePatterns` is not "first match wins".** It walks the *whole*
//! table with no early exit, evaluates each rule's measure guard against the
//! **original** token (with the suffix stripped, which is classic Porter), and
//! applies the replacement to an **accumulator**. So `step3("formalizeful")` is
//! `"formalize"`: the `alize` rule is skipped because its guard is tested against
//! `"formalizeful"`, even though the accumulator ends in `alize` by the time the
//! rule is reached.
//!
//! **Empty strings are falsy.** `attemptReplace` skips its callback when the
//! result is `""`, and `|| replacement` discards an empty result, so
//! `step1b("ed")` is `"ed"` rather than `""`.
//!
//! The `\1` backreference in `endsWithDoublCons` has no equivalent in the Rust
//! `regex` crate; it is a two-code-unit comparison here. Note that `y` counts as
//! a consonant for *that* test but as a vowel-former in `categorizeGroups`.

use std::borrow::Cow;
use std::sync::LazyLock;

use crate::among::{AmongTable, Buf, UnionTable};
use crate::base::{Casing, TokenizeAndStem};
use crate::units::{at, ends_with, push_str, slen, text, truncate_by, u, units};

/// The English Porter stemmer.
///
/// ```
/// use verbora_stemmers::PorterStemmer;
/// let s = PorterStemmer::new();
/// assert_eq!(s.stem("running"), "run");
/// // Tokens shorter than three UTF-16 code units are returned WITHOUT
/// // lowercasing — the fold happens only on the other branch.
/// assert_eq!(s.stem("AB"), "AB");
/// assert_eq!(s.stem("ABC"), "abc");
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PorterStemmer;

#[inline]
const fn is_aeiou(c: u16) -> bool {
    matches!(c, 0x61 | 0x65 | 0x69 | 0x6F | 0x75) // a e i o u
}

#[inline]
const fn is_aeiouy(c: u16) -> bool {
    is_aeiou(c) || c == 0x79 // y
}

/// The characters the reference's `.` refuses to match.
#[inline]
const fn is_line_terminator(c: u16) -> bool {
    matches!(c, 0x0A | 0x0D | 0x2028 | 0x2029)
}

const C: u16 = 0x43;
const V: u16 = 0x56;

/// `token.replace(/[^aeiouy]+y/g,'CV').replace(/[aeiou]+/g,'V').replace(/[^V]+/g,'C')`
///
/// The three passes run in order over each other's output, which is why the
/// result can contain adjacent `VV`: the `V` written by pass two is outside
/// `[aeiou]`, so a following vowel run starts a *new* group.
fn categorize_groups(w: &[u16]) -> Vec<u16> {
    // Pass 1: a run of non-[aeiouy] followed by a literal `y` becomes "CV".
    let mut a: Vec<u16> = Vec::with_capacity(w.len());
    let mut i = 0;
    while i < w.len() {
        if is_aeiouy(w[i]) {
            a.push(w[i]);
            i += 1;
            continue;
        }
        // Greedy run; `y` is excluded from the class, so a shorter run could
        // never be followed by `y` either — no backtracking is possible.
        let start = i;
        while i < w.len() && !is_aeiouy(w[i]) {
            i += 1;
        }
        if at(w, i) == Some(u('y')) {
            a.push(C);
            a.push(V);
            i += 1;
        } else {
            a.extend_from_slice(&w[start..i]);
        }
    }

    // Pass 2: runs of [aeiou] become a single `V`.
    let mut b: Vec<u16> = Vec::with_capacity(a.len());
    let mut i = 0;
    while i < a.len() {
        if is_aeiou(a[i]) {
            while i < a.len() && is_aeiou(a[i]) {
                i += 1;
            }
            b.push(V);
        } else {
            b.push(a[i]);
            i += 1;
        }
    }

    // Pass 3: runs of non-`V` become a single `C`.
    let mut out: Vec<u16> = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == V {
            out.push(V);
            i += 1;
        } else {
            while i < b.len() && b[i] != V {
                i += 1;
            }
            out.push(C);
        }
    }
    out
}

/// `token.replace(/[^aeiouy]y/g,'CV').replace(/[aeiou]/g,'V').replace(/[^V]/g,'C')`
///
/// Length preserving, unlike [`categorize_groups`]: every input code unit maps to
/// exactly one output unit.
fn categorize_chars(w: &[u16]) -> Vec<u16> {
    let mut out: Vec<u16> = Vec::with_capacity(w.len());
    let mut i = 0;
    while i < w.len() {
        if !is_aeiouy(w[i]) && at(w, i + 1) == Some(u('y')) {
            out.push(C);
            out.push(V);
            i += 2;
        } else if is_aeiou(w[i]) {
            out.push(V);
            i += 1;
        } else {
            out.push(C);
            i += 1;
        }
    }
    out
}

/// What [`categorize_groups`] would produce, as the four facts every caller
/// actually reads: how long it is, what its first and last symbols are, and
/// how many `V`s it contains.
///
/// # Why this exists
///
/// `measure` is the innermost loop of the whole English stemmer — step 2
/// alone evaluates it twenty-two times per word, once per rewrite rule — and
/// `categorizeGroups` is three chained passes each building a string. Three
/// allocations times thirty-odd calls dominated the per-word cost. Nothing
/// downstream ever indexes the categorisation, though: `measure` needs its
/// length and its two ends, step 1b asks whether it contains a `V`, and step
/// 1c asks whether its *head* does. All four fall out of a single streaming
/// scan, so the strings never need to exist.
///
/// The scan reproduces the three passes exactly. Pass 1 is the loop below
/// (a maximal non-`[aeiouy]` run followed by a literal `y` becomes `CV`;
/// anything else passes through). Passes 2 and 3 are the two merge rules in
/// [`GroupScan::feed`]: consecutive `[aeiou]` collapse into one `V`, and
/// consecutive non-`V` symbols collapse into one `C`. `groups_agree_with_the
/// _string_form` fuzzes the two forms against each other.
#[derive(Default)]
struct GroupScan {
    len: usize,
    first: u16,
    last: u16,
    vowels: usize,
    /// Whether the previous pass-1 symbol was an `[aeiou]` character, which
    /// is pass 2's run state.
    vowel_run: bool,
}

impl GroupScan {
    /// Feeds one symbol of the pass-1 stream.
    #[inline]
    fn feed(&mut self, s: u16) {
        if is_aeiou(s) {
            // Pass 2: a maximal `[aeiou]` run becomes a single `V`.
            if !self.vowel_run {
                self.vowel_run = true;
                self.push(V);
            }
            return;
        }
        self.vowel_run = false;
        if s == V {
            self.push(V);
        } else if self.last == C && self.len > 0 {
            // Pass 3: this symbol joins the `C` already emitted.
        } else {
            self.push(C);
        }
    }

    #[inline]
    fn push(&mut self, sym: u16) {
        if self.len == 0 {
            self.first = sym;
        }
        self.last = sym;
        self.len += 1;
        if sym == V {
            self.vowels += 1;
        }
    }

    /// Runs pass 1 over `w`, feeding every symbol it produces.
    fn scan(w: &[u16]) -> GroupScan {
        let mut g = GroupScan::default();
        let mut i = 0;
        while i < w.len() {
            if is_aeiouy(w[i]) {
                g.feed(w[i]);
                i += 1;
                continue;
            }
            // Greedy run; `y` is excluded from the class, so a shorter run
            // could never be followed by `y` either — no backtracking.
            let start = i;
            while i < w.len() && !is_aeiouy(w[i]) {
                i += 1;
            }
            if at(w, i) == Some(u('y')) {
                g.feed(C);
                g.feed(V);
                i += 1;
            } else {
                for &c in &w[start..i] {
                    g.feed(c);
                }
            }
        }
        g
    }
}

/// The Porter measure `m` of `w`.
///
/// # The empty word
///
/// `measure_units(&[])` is `0.0`. Porter (1980) §2 writes any word as
/// `[C](VC){m}[V]`, so the empty word is the `m = 0` case with every part
/// empty; there is no such thing as "no measure", and the function has no
/// sentinel to return.
///
/// It used to return `-1.0`, which is not a measure any word has. That value
/// was inert rather than merely unused: the algorithm compares `m` against
/// exactly three predicates — `> 0.0`, `> 1.0` and `== 1.0` — and `-1.0` and
/// `0.0` answer all three identically, which
/// `the_empty_word_measures_zero_and_nothing_else_changes` enumerates rather
/// than asserts.
fn measure_units(w: &[u16]) -> f64 {
    if w.is_empty() {
        return 0.0;
    }
    let g = GroupScan::scan(w);
    let start = usize::from(g.first == C && g.len > 0);
    let end = g.len - usize::from(g.last == V && g.len > start);
    (end - start) as f64 / 2.0
}

/// `token.match(/([^aeiou])\1$/)` — the hand-written backreference.
fn ends_with_double_cons(w: &[u16]) -> bool {
    w.len() >= 2 && w[w.len() - 1] == w[w.len() - 2] && !is_aeiou(w[w.len() - 1])
}

/// `attemptReplace` for a literal-string pattern.
///
/// Returns `None` when the pattern is not a suffix, mirroring the reference
/// `null`. An empty *result* is returned as `Some(vec![])`; callers apply the
/// falsy check themselves, because the two call sites treat it differently.
fn attempt_replace(w: &[u16], pattern: &str, replacement: &str) -> Option<Vec<u16>> {
    if !ends_with(w, pattern) {
        return None;
    }
    let mut out = w[..w.len() - slen(pattern)].to_vec();
    out.extend(replacement.encode_utf16());
    Some(out)
}

/// `attemptReplacePatterns`: walk the entire table, guard against the original,
/// apply to the accumulator, never break.
fn attempt_replace_patterns(
    token: &[u16],
    rules: &[(&str, &str, &str)],
    threshold: Option<f64>,
) -> Vec<u16> {
    let mut replacement = token.to_vec();
    let table = if std::ptr::eq(rules, STEP2) {
        Some(&TABLES.step2)
    } else if std::ptr::eq(rules, STEP3) {
        Some(&TABLES.step3)
    } else {
        None
    };
    apply_patterns(token, &mut replacement, rules, threshold, table);
    replacement
}

/// [`attempt_replace_patterns`] with the accumulator supplied by the caller.
///
/// `token` is the string the measure guards are evaluated against — the
/// reference evaluates every guard against the step's *input*, never against
/// the accumulator — and `acc` starts as a copy of it. Splitting the two lets
/// `stem` keep one working buffer for the whole word and take the guard
/// token as a stack snapshot, instead of allocating a fresh accumulator per
/// step.
fn apply_patterns(
    token: &[u16],
    replacement: &mut Vec<u16>,
    rules: &[(&str, &str, &str)],
    threshold: Option<f64>,
    table: Option<&UnionTable>,
) {
    // Two suffix questions per rule — "is this rule's pattern a suffix of the
    // step's input?" (the guard) and "of the accumulator?" (the rewrite) —
    // and the reference asks both by walking the whole table. One merged
    // search answers all twenty-two of the first kind at once, and a second
    // answers all of the second; the accumulator's answers only go stale when
    // a rule actually rewrites it, which happens at most once or twice per
    // word. `union_masks_agree_with_ends_with` pins the equivalence.
    let mut guard_hit = [0u32; MAX_RULES];
    let mut acc_hit = [0u32; MAX_RULES];
    if let Some(t) = table {
        t.length_masks(token, token.len(), &mut guard_hit);
        acc_hit = guard_hit;
    }
    for (i, &(pattern, guard_replacement, real_replacement)) in rules.iter().enumerate() {
        let guard_matches = table.map_or_else(|| ends_with(token, pattern), |_| guard_hit[i] != 0);
        let passes = match threshold {
            None => true,
            Some(t) => {
                // `measure(null)` is -1, and so is `measure('')`. When the
                // guard replacement is empty — which it is for every shipped
                // rule — the guarded string is a *prefix slice* of the token,
                // so the measure can be taken without building it. Only a
                // future non-empty guard would need the allocation.
                let m: Option<f64> = if !guard_matches {
                    None
                } else if guard_replacement.is_empty() {
                    Some(measure_units(&token[..token.len() - slen(pattern)]))
                } else {
                    attempt_replace(token, pattern, guard_replacement)
                        .as_deref()
                        .map(measure_units)
                };
                m.is_some_and(|m| m > t)
            }
        };
        // The replacement is applied in place: `attemptReplace` returning a
        // fresh string per rule was one allocation per rule, on a
        // twenty-two-rule table, for a rewrite that fires at most once.
        let acc_matches =
            table.map_or_else(|| ends_with(replacement, pattern), |_| acc_hit[i] != 0);
        if passes && acc_matches {
            let keep = replacement.len() - slen(pattern);
            // `if (result)` — an empty result is falsy and is discarded.
            if keep > 0 || !real_replacement.is_empty() {
                replacement.truncate(keep);
                push_str(replacement, real_replacement);
                if let Some(t) = table {
                    acc_hit = [0u32; MAX_RULES];
                    t.length_masks(replacement, replacement.len(), &mut acc_hit);
                }
            }
        }
    }
}

/// The largest rule table's length, and so the size of the mask arrays
/// [`apply_patterns`] fills.
const MAX_RULES: usize = 22;

/// The sorted search tables, built once from the rule tables below — those
/// stay the single source of truth. Each rule contributes its pattern as a
/// one-entry table, so a rule's slot in the mask array is its index in the
/// chain and `mask != 0` is exactly "this rule's pattern is a suffix".
struct EnTables {
    step2: UnionTable,
    step3: UnionTable,
    /// Step 4's alternation, which is a plain longest-match.
    step4: AmongTable,
    /// The `(s|t)ion` alternation of step 4's second rule.
    sion_tion: AmongTable,
}

static TABLES: LazyLock<EnTables> = LazyLock::new(|| EnTables {
    step2: UnionTable::build(STEP2_PATTERNS),
    step3: UnionTable::build(STEP3_PATTERNS),
    step4: AmongTable::build(STEP4),
    sion_tion: AmongTable::build(SION_TION),
});

/// `replaceRegex(token, /^(.+?)(alt|…)$/, [1], min)` — the step-4 shape.
///
/// Lazy `.+?` plus a `$`-anchored alternation means "the shortest non-empty
/// prefix whose remainder is one of the alternatives", i.e. the **longest**
/// matching suffix. `.` cannot cross a line terminator, so a prefix containing
/// one blocks the match entirely.
fn strip_longest_alternative(w: &[u16], table: &AmongTable, keep_extra: usize) -> Option<usize> {
    // `.+?` is non-empty, so an alternative as long as the whole token cannot
    // match — hence the strict `n < w.len()` the link walk applies.
    let n = table.longest_where(w, w.len(), 0, |n| n < w.len());
    if n == 0 {
        return None;
    }
    let cut = w.len() - n;
    if w[..cut].iter().copied().any(is_line_terminator) {
        return None;
    }
    // The kept length, not the kept string: every caller truncates.
    Some(cut + keep_extra)
}

impl PorterStemmer {
    /// Creates the stemmer. It is stateless and zero-sized.
    #[inline]
    pub const fn new() -> Self {
        Self
    }

    /// The consonant/vowel group string of `token`: `C` for each maximal
    /// consonant group, `V` for each vowel group.
    ///
    /// This is the `[C](VC){m}[V]` decomposition of Porter (1980) §2, and
    /// [`Self::measure`] is derived from it. Exposed because the measure is
    /// exposed and this is what explains it — including the adjacent `VV`
    /// that makes the measure fractional (see [`Self::measure`]).
    pub fn categorize_groups(token: &str) -> String {
        text(&categorize_groups(&units(token)))
    }

    /// The Porter measure `m` of `token` — the `m` of `[C](VC){m}[V]`,
    /// Porter (1980) §2.
    ///
    /// `measure("")` is `0.0`, the empty word's own measure; the function has
    /// no sentinel and returns no value outside the range a word can have.
    ///
    /// # This `m` is not always an integer
    ///
    /// Porter's `m` is a non-negative integer. This implementation's is a
    /// multiple of `0.5`, because the grouping it is derived from can emit two
    /// adjacent vowel groups: a consonant run followed by a literal `y` is
    /// rewritten to `CV` before vowel runs are collapsed, and the `V` that
    /// rewrite writes is not itself a vowel, so a vowel immediately after it
    /// starts a *second* group. `measure("sya")` is `0.5`, `measure("canyon")`
    /// is `2.5`, `measure("syaing")` is `1.5`.
    ///
    /// This is a divergence from the published algorithm rather than a
    /// feature, and it is not yet resolved: every guard in the stemmer is
    /// `> 0`, `> 1` or `== 1`, so correcting the grouping changes those guards
    /// for every word in the affected class at once, and this crate carries no
    /// vocabulary to validate the result against. Until it does, the value is
    /// documented as what it is rather than described as Porter's.
    #[must_use]
    pub fn measure(token: &str) -> f64 {
        measure_units(&units(token))
    }

    /// Step 1a: plural `s` removal.
    pub fn step1a(token: &str) -> String {
        let mut w = units(token);
        Self::step1a_units(&mut w);
        text(&w)
    }

    fn step1a_units(w: &mut Vec<u16>) {
        // `/(ss|i)es$/ -> '$1'` reduces to "drop the trailing `es`" for both
        // alternatives, because the captured group is what precedes it.
        if STEP1A.iter().any(|p| ends_with(w, p)) {
            truncate_by(w, 2);
            return;
        }
        if w.len() > 2 && w[w.len() - 1] == u('s') && w[w.len() - 2] != u('s') {
            truncate_by(w, 1);
        }
    }

    /// Step 1b: `eed`, `ed` and `ing`.
    pub fn step1b(token: &str) -> String {
        let mut w = units(token);
        Self::step1b_units(&mut w);
        text(&w)
    }

    fn step1b_units(w: &mut Vec<u16>) {
        // `token.substr(-3) === 'eed'` needs the token to be at least three units
        // long — a shorter `substr(-3)` returns the whole token, which can never
        // equal a three-character literal.
        if ends_with(w, "eed") {
            if measure_units(&w[..w.len() - 3]) > 0.0 {
                truncate_by(w, 1); // eed -> ee
            }
            return;
        }
        let stripped = if ends_with(w, "ing") {
            Some(w[..w.len() - 3].to_vec())
        } else if ends_with(w, "ed") {
            Some(w[..w.len() - 2].to_vec())
        } else {
            None
        };
        let Some(t) = stripped else { return };
        // The callback returns null when the stripped stem has no vowel group.
        let Some(result) = Self::step1b_callback(&t) else {
            return;
        };
        // `if (result)` — an empty result is falsy and leaves the token alone.
        if !result.is_empty() {
            *w = result;
        }
    }

    fn step1b_callback(t: &[u16]) -> Option<Vec<u16>> {
        if GroupScan::scan(t).vowels == 0 {
            return None;
        }
        // No measure threshold here: every rule fires unconditionally.
        let r = attempt_replace_patterns(t, STEP1B, None);
        if r.as_slice() != t {
            return Some(r);
        }
        if ends_with_double_cons(t)
            && t.last()
                .is_some_and(|&c| c != u('l') && c != u('s') && c != u('z'))
        {
            return Some(t[..t.len() - 1].to_vec());
        }
        let cc = categorize_chars(t);
        let tail3 = &cc[cc.len().saturating_sub(3)..];
        if measure_units(t) == 1.0
            && tail3 == [C, V, C]
            && t.last()
                .is_some_and(|&c| c != u('w') && c != u('x') && c != u('y'))
        {
            let mut out = t.to_vec();
            out.push(u('e'));
            return Some(out);
        }
        Some(t.to_vec())
    }

    /// Step 1c: terminal `y` becomes `i` when the stem has a vowel group.
    pub fn step1c(token: &str) -> String {
        let mut w = units(token);
        Self::step1c_units(&mut w);
        text(&w)
    }

    fn step1c_units(w: &mut [u16]) {
        let g = GroupScan::scan(w);
        // `g.substr(0, g.length - 1)` drops one character of the COMPRESSED
        // categorisation, whose length has nothing to do with the token's.
        // That head contains a `V` unless the only `V` was the dropped last
        // symbol.
        let head_has_v = g.vowels > usize::from(g.last == V);
        if w.last() == Some(&u('y')) && head_has_v {
            let n = w.len();
            w[n - 1] = u('i');
        }
    }

    /// Step 2: the twenty-two long-suffix rewrites.
    pub fn step2(token: &str) -> String {
        text(&Self::step2_units(&units(token)))
    }

    fn step2_units(w: &[u16]) -> Vec<u16> {
        attempt_replace_patterns(w, STEP2, Some(0.0))
    }

    /// Step 3: the seven `-icate`/`-ful`/`-ness` rewrites.
    pub fn step3(token: &str) -> String {
        text(&Self::step3_units(&units(token)))
    }

    fn step3_units(w: &[u16]) -> Vec<u16> {
        attempt_replace_patterns(w, STEP3, Some(0.0))
    }

    /// Step 4: suffix removal for stems of measure greater than one.
    pub fn step4(token: &str) -> String {
        text(&Self::step4_units(&units(token)))
    }

    fn step4_units(w: &[u16]) -> Vec<u16> {
        let mut out = w.to_vec();
        Self::step4_in_place(&mut out);
        out
    }

    /// Step 4 as a truncation, which is all it ever is: both alternations
    /// keep a prefix of the token.
    fn step4_in_place(w: &mut Vec<u16>) {
        if let Some(keep) = strip_longest_alternative(w, &TABLES.step4, 0)
            && measure_units(&w[..keep]) > 1.0
        {
            w.truncate(keep);
            return;
        }
        // `/^(.+?)(s|t)(ion)$/` keeps groups 1 AND 2, i.e. everything but `ion`.
        if let Some(keep) = strip_longest_alternative(w, &TABLES.sion_tion, 1)
            && measure_units(&w[..keep]) > 1.0
        {
            w.truncate(keep);
        }
    }

    /// Step 5a: terminal `e` removal.
    pub fn step5a(token: &str) -> String {
        let mut w = units(token);
        Self::step5a_units(&mut w);
        text(&w)
    }

    fn step5a_units(w: &mut Vec<u16>) {
        // `token.replace(/e$/, '')` — a no-op when the token does not end in `e`,
        // which is why this step can fire on tokens that have no `e` to remove.
        let keep = w.len() - usize::from(w.last() == Some(&u('e')));
        let m = measure_units(&w[..keep]);
        // `categorizeChars(token).substr(-4, 3)`: a negative start clamps to 0,
        // so a three-unit token yields its whole categorisation.
        let cc = categorize_chars(w);
        let start = cc.len().saturating_sub(4);
        let end = (start + 3).min(cc.len());
        let cvc = cc[start..end] == [C, V, C];
        // `/[^wxy].$/` needs two units, and `.` refuses a line terminator.
        let short_e = w.len() >= 2
            && !matches!(w[w.len() - 2], c if c == u('w') || c == u('x') || c == u('y'))
            && !is_line_terminator(w[w.len() - 1]);
        if m > 1.0 || (m == 1.0 && !(cvc && short_e)) {
            w.truncate(keep);
        }
    }

    /// Step 5b: `ll` becomes `l` for stems of measure greater than one.
    pub fn step5b(token: &str) -> String {
        let mut w = units(token);
        Self::step5b_units(&mut w);
        text(&w)
    }

    fn step5b_units(w: &mut Vec<u16>) {
        if measure_units(w) > 1.0 && ends_with(w, STEP5B[0]) {
            truncate_by(w, 1);
        }
    }

    /// Stems one token.
    ///
    /// # Short tokens keep their case
    ///
    /// A token shorter than three UTF-16 code units is returned **exactly as
    /// it arrived**, uncased: the length gate is tested before the fold, so
    /// `stem("AB")` is `"AB"` while `stem("ABC")` is `"abc"`. The asymmetry is
    /// visible through this method and not through
    /// [`TokenizeAndStem::tokenize_and_stem`], which lowercases the whole
    /// document before tokenizing.
    pub fn stem<'a>(&self, token: &'a str) -> Cow<'a, str> {
        if slen(token) < 3 {
            return Cow::Borrowed(token);
        }
        let mut w = Vec::with_capacity(token.len());
        crate::units::for_each_lowercase_unit(token, |unit| w.push(unit));
        Self::step1a_units(&mut w);
        Self::step1b_units(&mut w);
        Self::step1c_units(&mut w);
        // Steps 2 and 3 rewrite `w` in place; their measure guards read the
        // step's input, taken here as a stack snapshot rather than as the
        // fresh accumulator the reference allocates.
        let snapshot = Buf::from_slice(&w);
        apply_patterns(
            snapshot.as_slice(),
            &mut w,
            STEP2,
            Some(0.0),
            Some(&TABLES.step2),
        );
        let snapshot = Buf::from_slice(&w);
        apply_patterns(
            snapshot.as_slice(),
            &mut w,
            STEP3,
            Some(0.0),
            Some(&TABLES.step3),
        );
        Self::step4_in_place(&mut w);
        Self::step5a_units(&mut w);
        Self::step5b_units(&mut w);
        Cow::Owned(text(&w))
    }
}

/// Step 2's rewrite table, in the reference's exact order.
///
/// Each entry is `(suffix, guard replacement, real replacement)`. The guard
/// replacement is always `''`: the measure test is `m(stem without the suffix) > 0`.
/// One one-entry table per rule of [`STEP2`], in the same order, so a
/// rule's index in the chain is its slot in the merged mask array.
/// `rule_pattern_tables_track_their_rule_tables` keeps the two in step.
/// Step 1a's two four/three-unit plural patterns; both reduce to "drop the
/// trailing `es`", because the captured group is what precedes it.
static STEP1A: &[&str] = &["sses", "ies"];

/// Step 1b's unconditional rewrites, in table order.
static STEP1B: &[(&str, &str, &str)] = &[("at", "", "ate"), ("bl", "", "ble"), ("iz", "", "ize")];

/// The `(s|t)ion` alternation of step 4's second rule.
static SION_TION: &[&str] = &["sion", "tion"];

/// Step 5b's doubled `l`.
static STEP5B: &[&str] = &["ll"];

static STEP2_PATTERNS: &[&[&str]] = &[
    &["ational"],
    &["tional"],
    &["enci"],
    &["anci"],
    &["izer"],
    &["abli"],
    &["bli"],
    &["alli"],
    &["entli"],
    &["eli"],
    &["ousli"],
    &["ization"],
    &["ation"],
    &["ator"],
    &["alism"],
    &["iveness"],
    &["fulness"],
    &["ousness"],
    &["aliti"],
    &["iviti"],
    &["biliti"],
    &["logi"],
];

static STEP2: &[(&str, &str, &str)] = &[
    ("ational", "", "ate"),
    ("tional", "", "tion"),
    ("enci", "", "ence"),
    ("anci", "", "ance"),
    ("izer", "", "ize"),
    ("abli", "", "able"),
    ("bli", "", "ble"),
    ("alli", "", "al"),
    ("entli", "", "ent"),
    ("eli", "", "e"),
    ("ousli", "", "ous"),
    ("ization", "", "ize"),
    ("ation", "", "ate"),
    ("ator", "", "ate"),
    ("alism", "", "al"),
    ("iveness", "", "ive"),
    ("fulness", "", "ful"),
    ("ousness", "", "ous"),
    ("aliti", "", "al"),
    ("iviti", "", "ive"),
    ("biliti", "", "ble"),
    ("logi", "", "log"),
];

/// Step 3's rewrite table, in the reference's exact order.
/// One one-entry table per rule of [`STEP3`], in the same order, so a
/// rule's index in the chain is its slot in the merged mask array.
/// `rule_pattern_tables_track_their_rule_tables` keeps the two in step.
static STEP3_PATTERNS: &[&[&str]] = &[
    &["icate"],
    &["ative"],
    &["alize"],
    &["iciti"],
    &["ical"],
    &["ful"],
    &["ness"],
];

static STEP3: &[(&str, &str, &str)] = &[
    ("icate", "", "ic"),
    ("ative", "", ""),
    ("alize", "", "al"),
    ("iciti", "", "ic"),
    ("ical", "", "ic"),
    ("ful", "", ""),
    ("ness", "", ""),
];

/// Step 4's alternation, in the reference's exact order (which does not matter:
/// the `$` anchor makes the longest suffix win regardless).
static STEP4: &[&str] = &[
    "al", "ance", "ence", "er", "ic", "able", "ible", "ant", "ement", "ment", "ent", "ou", "ism",
    "ate", "iti", "ous", "ive", "ize",
];

impl TokenizeAndStem for PorterStemmer {
    const FILTER_ON: Casing = Casing::Raw;
    const STEM_ON: Casing = Casing::Raw;

    /// The whole document is lowercased before tokenizing, so the stop-word test
    /// and `stem` both see an already-folded token.
    fn prepare(t: &str) -> Cow<'_, str> {
        Cow::Owned(t.to_lowercase())
    }

    fn is_stop_word(word: &str) -> bool {
        verbora_core::is_global_stopword(word)
    }

    fn stem_token(&self, token: &str) -> String {
        self.stem(token).into_owned()
    }
}

/// What [`crate::data::table_audit`] needs to walk this language's tables.
#[cfg(test)]
pub(crate) mod audit {
    /// Every rule table, named. The three rewrite tables are flattened to
    /// their patterns, which is the half a word is matched against.
    pub(crate) fn tables() -> Vec<(&'static str, Vec<&'static str>)> {
        vec![
            ("STEP1A", super::STEP1A.to_vec()),
            (
                "STEP1B",
                super::STEP1B.iter().map(|r| r.0).collect::<Vec<_>>(),
            ),
            (
                "STEP2",
                super::STEP2.iter().map(|r| r.0).collect::<Vec<_>>(),
            ),
            (
                "STEP3",
                super::STEP3.iter().map(|r| r.0).collect::<Vec<_>>(),
            ),
            ("STEP4", super::STEP4.to_vec()),
            ("SION_TION", super::SION_TION.to_vec()),
            ("STEP5B", super::STEP5B.to_vec()),
        ]
    }

    /// The prelude `stem` runs before any table is consulted: lowercasing,
    /// and nothing else.
    pub(crate) fn prelude(token: &str) -> String {
        token.to_lowercase()
    }

    /// The prelude writes no marker unit.
    pub(crate) static MARKERS: &[(&str, &str)] = &[];
}

impl verbora_core::Stemmer for PorterStemmer {
    fn stem<'a>(&self, token: &'a str) -> Cow<'a, str> {
        Self::stem(self, token)
    }
}

impl PorterStemmer {
    /// Appends a stop word to the **process-global English list**, which
    /// `LancasterStemmer` and the phonetics helpers also read.
    pub fn add_stop_word(&self, word: impl Into<String>) {
        verbora_core::add_global_stopword(word);
    }

    /// Appends several stop words to the process-global English list.
    pub fn add_stop_words<I, S>(&self, words: I)
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        verbora_core::add_global_stopwords(words);
    }

    /// Removes the first occurrence of `word` from the process-global list.
    pub fn remove_stop_word(&self, word: &str) {
        verbora_core::remove_global_stopword(word);
    }

    /// Removes the first occurrence of each of `words`.
    pub fn remove_stop_words<'a, I>(&self, words: I)
    where
        I: IntoIterator<Item = &'a str>,
    {
        verbora_core::remove_global_stopwords(words);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measure_is_fractional() {
        assert_eq!(PorterStemmer::measure(""), 0.0);
        assert_eq!(PorterStemmer::measure("a"), 0.0);
        assert_eq!(PorterStemmer::measure("syllog"), 2.0);
        assert_eq!(PorterStemmer::measure("sya"), 0.5);
        assert_eq!(PorterStemmer::measure("syaing"), 1.5);
        assert_eq!(PorterStemmer::measure("oypvyegg"), 2.5);
        assert_eq!(PorterStemmer::measure("fifugyed"), 3.5);
    }

    /// The empty word measures 0, and switching it from `-1` to `0` changed
    /// nothing else — enumerated over every predicate the algorithm applies to
    /// a measure, not argued.
    ///
    /// The three predicates below are the *only* ways `m` is ever read: the
    /// step-2 and step-3 thresholds (`> 0`), step 4 and steps 5a/5b (`> 1`),
    /// and step 1b's `== 1`. `grep -n "measure_units" src/en.rs` finds no
    /// other shape, and this test walks all three against both values.
    #[test]
    fn the_empty_word_measures_zero_and_nothing_else_changes() {
        assert_eq!(measure_units(&[]), 0.0);
        assert_eq!(PorterStemmer::measure(""), 0.0);
        const OLD_SENTINEL: f64 = -1.0;
        for threshold in [0.0, 1.0] {
            assert_eq!(
                OLD_SENTINEL > threshold,
                measure_units(&[]) > threshold,
                "the `> {threshold}` guard changed for the empty word"
            );
        }
        assert_eq!(
            (OLD_SENTINEL - 1.0).abs() < f64::EPSILON,
            (measure_units(&[]) - 1.0).abs() < f64::EPSILON,
            "step 1b's `== 1` guard changed for the empty word"
        );
        // No word other than the empty one ever measured below zero, so no
        // other value moved: a measure is `(end - start) / 2` over unsigned
        // group counts.
        assert!(measure_units(&units("bcd")) >= 0.0);
    }

    #[test]
    fn categorize_groups_can_emit_adjacent_vv() {
        assert_eq!(PorterStemmer::categorize_groups("syllog"), "CVCVC");
        assert_eq!(PorterStemmer::categorize_groups("gypsy"), "CVCV");
        assert_eq!(PorterStemmer::categorize_groups(""), "");
        assert_eq!(PorterStemmer::categorize_groups("sya"), "CVV");
        assert_eq!(PorterStemmer::categorize_groups("xyz"), "CVC");
    }

    #[test]
    fn short_tokens_keep_their_case() {
        let s = PorterStemmer::new();
        assert_eq!(s.stem(""), "");
        assert_eq!(s.stem("a"), "a");
        assert_eq!(s.stem("AB"), "AB");
        assert_eq!(s.stem("IS"), "IS");
        assert_eq!(s.stem("ABC"), "abc");
    }

    #[test]
    fn astral_input_counts_as_two_code_units() {
        // "😀s".length is 3 in the reference, so the algorithm runs and step 1a
        // strips the `s`. A char-counting port would see 2 and bail out.
        assert_eq!(PorterStemmer::new().stem("😀s"), "😀");
    }

    #[test]
    fn the_accumulator_quirk_is_reproduced() {
        assert_eq!(PorterStemmer::step3("formalizeful"), "formalize");
        assert_eq!(PorterStemmer::step2("rationalization"), "rationalize");
    }

    #[test]
    fn empty_replacements_are_discarded() {
        assert_eq!(PorterStemmer::step1b("ed"), "ed");
        assert_eq!(PorterStemmer::step1b("ing"), "ing");
    }

    #[test]
    fn documented_step_vectors() {
        for (input, want) in [
            ("caresses", "caress"),
            ("ponies", "poni"),
            ("ties", "ti"),
            ("caress", "caress"),
            ("cats", "cat"),
            ("us", "us"),
        ] {
            assert_eq!(PorterStemmer::step1a(input), want, "step1a({input})");
        }
        for (input, want) in [
            ("feed", "feed"),
            ("agreed", "agree"),
            ("plastered", "plaster"),
            ("bled", "bled"),
            ("motoring", "motor"),
            ("sing", "sing"),
            ("hopping", "hop"),
            ("filing", "file"),
            ("falling", "fall"),
            ("hissing", "hiss"),
            ("fizzed", "fizz"),
        ] {
            assert_eq!(PorterStemmer::step1b(input), want, "step1b({input})");
        }
        assert_eq!(PorterStemmer::step1c("happy"), "happi");
        assert_eq!(PorterStemmer::step1c("sky"), "sky");
        assert_eq!(PorterStemmer::step5a("probate"), "probat");
        assert_eq!(PorterStemmer::step5a("rate"), "rate");
        assert_eq!(PorterStemmer::step5a("ace"), "ac");
        assert_eq!(PorterStemmer::step5b("controll"), "control");
        assert_eq!(PorterStemmer::step5b("roll"), "roll");
    }

    #[test]
    fn unicode_and_pathological_inputs_survive() {
        let s = PorterStemmer::new();
        assert_eq!(s.stem("café"), "café");
        assert_eq!(s.stem("Ångström"), "ångström");
        assert_eq!(s.stem("Москва"), "москва");
        assert_eq!(s.stem("日本語"), "日本語");
        assert_eq!(s.stem("123"), "123");
        assert_eq!(s.stem("..."), "...");
        let long = "a".repeat(500);
        assert_eq!(s.stem(&long), long);
    }

    // -----------------------------------------------------------------------
    // Differential oracles for the three allocation-free rewrites.
    //
    // `GroupScan` claims to compute the four facts callers read off
    // `categorizeGroups`'s *string*, and `attempt_replace_patterns` claims to
    // apply its table in place exactly as the per-rule fresh-string form did.
    // Both string forms are still present — `categorize_groups` because the
    // reference exports it, `attempt_replace` because a non-empty guard
    // replacement would still need it — so both claims are checkable
    // directly, and the third test replays whole words through a verbatim
    // copy of the pre-rewrite `stem`.
    // -----------------------------------------------------------------------
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

    fn random_word(rng: &mut Rng) -> String {
        const ALPHA: &[char] = &[
            'a', 'b', 'c', 'd', 'e', 'f', 'g', 'i', 'l', 'm', 'n', 'o', 'p', 'r', 's', 't', 'u',
            'v', 'y', 'z', 'w', 'x', 'k', 'h',
        ];
        const SUFFIXES: &[&str] = &[
            "ational", "tional", "enci", "anci", "izer", "abli", "alli", "entli", "eli", "ousli",
            "ization", "ation", "ator", "alism", "iveness", "fulness", "ousness", "aliti", "iviti",
            "biliti", "icate", "ative", "alize", "iciti", "ical", "ful", "ness", "sses", "ies",
            "eed", "ed", "ing", "ement", "ment", "ent", "ion", "sion", "tion", "ou", "ism", "ate",
            "iti", "ous", "ive", "ize", "e", "ll", "y", "s",
        ];
        let mut s = String::new();
        for _ in 0..rng.below(9) {
            s.push(ALPHA[rng.below(ALPHA.len())]);
        }
        if rng.below(10) < 8 {
            s.push_str(SUFFIXES[rng.below(SUFFIXES.len())]);
            if rng.below(3) == 0 {
                s.push_str(SUFFIXES[rng.below(SUFFIXES.len())]);
            }
        }
        match rng.below(40) {
            0 => s = s.to_uppercase(),
            1 => s.push('😀'),
            2 => s.insert(0, '日'),
            3 => s.push_str("123"),
            4 => s.push('\n'),
            _ => {}
        }
        s
    }

    /// The one-entry pattern tables the merged search is built from must
    /// stay in lockstep with the rule tables they mirror.
    #[test]
    fn rule_pattern_tables_track_their_rule_tables() {
        for (rules, patterns) in [(STEP2, STEP2_PATTERNS), (STEP3, STEP3_PATTERNS)] {
            assert_eq!(rules.len(), patterns.len());
            for (rule, pattern) in rules.iter().zip(patterns) {
                assert_eq!([rule.0], *pattern);
            }
            assert!(rules.len() <= MAX_RULES, "MAX_RULES is too small");
        }
    }

    /// The merged search must answer "is this rule's pattern a suffix?"
    /// exactly as the per-rule `ends_with` walk did.
    #[test]
    fn union_masks_agree_with_ends_with() {
        let check = |word: &str| {
            let w = units(word);
            for (rules, table) in [(STEP2, &TABLES.step2), (STEP3, &TABLES.step3)] {
                let mut hit = [0u32; MAX_RULES];
                table.length_masks(&w, w.len(), &mut hit);
                for (i, rule) in rules.iter().enumerate() {
                    assert_eq!(
                        hit[i] != 0,
                        ends_with(&w, rule.0),
                        "{word:?} rule {}",
                        rule.0
                    );
                }
            }
        };
        for w in [
            "",
            "rational",
            "formalizeful",
            "ational",
            "tional",
            "ness",
            "ful",
        ] {
            check(w);
        }
        let mut rng = Rng(0x3C79_AB61_1D2E_0F55);
        for _ in 0..40_000 {
            let w = random_word(&mut rng);
            check(&w);
        }
    }

    #[test]
    fn groups_agree_with_the_string_form() {
        let check = |word: &str| {
            let w = units(word);
            let g = categorize_groups(&w);
            let scan = GroupScan::scan(&w);
            assert_eq!(scan.len, g.len(), "len for {word:?} ({g:?})");
            assert_eq!(
                scan.vowels,
                g.iter().filter(|&&c| c == V).count(),
                "vowel count for {word:?}"
            );
            if !g.is_empty() {
                assert_eq!(scan.first, g[0], "first for {word:?}");
                assert_eq!(scan.last, g[g.len() - 1], "last for {word:?}");
            }
            // The two facts derived from the scan at its call sites.
            let start = usize::from(g.first() == Some(&C));
            let end = g.len() - usize::from(g.last() == Some(&V) && g.len() > start);
            // The empty word's groups are empty, so `end - start` is 0 and the
            // general formula already yields its measure; there is no special
            // case left to state.
            let want = (end - start) as f64 / 2.0;
            assert_eq!(measure_units(&w), want, "measure for {word:?}");
            assert_eq!(
                GroupScan::scan(&w).vowels > usize::from(scan.last == V),
                g[..g.len().saturating_sub(1)].contains(&V),
                "head-contains-V for {word:?}"
            );
        };
        for w in [
            "", "a", "y", "sya", "syllog", "gypsy", "xyz", "syaing", "fifugyed", "yy", "aey",
        ] {
            check(w);
        }
        let mut rng = Rng(0x51ED_270B_2FA4_9B1D);
        for _ in 0..80_000 {
            let w = random_word(&mut rng);
            check(&w);
        }
    }

    /// The pre-rewrite `attemptReplacePatterns`, allocating a fresh string
    /// per rule.
    fn oracle_replace_patterns(
        token: &[u16],
        rules: &[(&str, &str, &str)],
        threshold: Option<f64>,
    ) -> Vec<u16> {
        let mut replacement = token.to_vec();
        for &(pattern, guard_replacement, real_replacement) in rules {
            let passes = match threshold {
                None => true,
                Some(t) => {
                    let guarded = attempt_replace(token, pattern, guard_replacement);
                    guarded.as_deref().map_or(-1.0, measure_units) > t
                }
            };
            if passes
                && let Some(next) = attempt_replace(&replacement, pattern, real_replacement)
                && !next.is_empty()
            {
                replacement = next;
            }
        }
        replacement
    }

    #[test]
    fn in_place_rewrites_agree_with_the_fresh_string_form() {
        let check = |word: &str| {
            let w = units(word);
            for (rules, threshold) in [(STEP2, Some(0.0)), (STEP3, Some(0.0)), (STEP1B, None)] {
                assert_eq!(
                    attempt_replace_patterns(&w, rules, threshold),
                    oracle_replace_patterns(&w, rules, threshold),
                    "{word:?}"
                );
            }
        };
        for w in [
            "",
            "formalizeful",
            "rationalization",
            "at",
            "bl",
            "iz",
            "ed",
            "ing",
        ] {
            check(w);
        }
        let mut rng = Rng(0x7F4A_7C15_9E37_79B9);
        for _ in 0..80_000 {
            let w = random_word(&mut rng);
            check(&w);
        }
    }

    /// The pre-rewrite `stem`: `to_lowercase` through a `String`, and every
    /// step exactly as it was.
    fn oracle_stem(token: &str) -> String {
        if slen(token) < 3 {
            return token.to_owned();
        }
        let mut w = units(&token.to_lowercase());
        PorterStemmer::step1a_units(&mut w);
        PorterStemmer::step1b_units(&mut w);
        PorterStemmer::step1c_units(&mut w);
        w = oracle_replace_patterns(&w, STEP2, Some(0.0));
        w = oracle_replace_patterns(&w, STEP3, Some(0.0));
        w = PorterStemmer::step4_units(&w);
        PorterStemmer::step5a_units(&mut w);
        PorterStemmer::step5b_units(&mut w);
        text(&w)
    }

    #[test]
    fn differential_against_the_allocating_oracle() {
        let stemmer = PorterStemmer::new();
        let check = |input: &str| {
            assert_eq!(
                stemmer.stem(input).as_ref(),
                oracle_stem(input),
                "stem({input:?})"
            );
        };
        for w in crate::test_support::bench_words("en") {
            check(&w);
        }
        for w in [
            "",
            "a",
            "AB",
            "IS",
            "ABC",
            "😀s",
            "running",
            "formalizeful",
            "rationalization",
            "caresses",
            "ponies",
            "ties",
            "feed",
            "agreed",
            "plastered",
            "bled",
            "motoring",
            "sing",
            "conflated",
            "troubled",
            "sized",
            "hopping",
            "tanned",
            "falling",
            "hissing",
            "fizzed",
            "failing",
            "filing",
            "happy",
            "sky",
            "\n",
            "controll",
            "roll",
        ] {
            check(w);
        }
        let long = "nationalization".repeat(6);
        check(&long);
        let mut rng = Rng(0x2545_F491_4F6C_DD1D);
        for _ in 0..80_000 {
            let w = random_word(&mut rng);
            check(&w);
        }
    }
}
