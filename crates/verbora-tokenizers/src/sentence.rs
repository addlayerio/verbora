//! `SentenceTokenizer` — splits text into sentences via placeholder substitution.
//!
//! The algorithm masks everything that must *not* be treated as a sentence
//! boundary (abbreviations, URIs, numbers), then masks the boundaries themselves,
//! splits on those, and finally unmasks. All four kinds of placeholder draw from
//! **one** counter, so the numbering interleaves across phases; that is
//! observable, because the placeholders end up in the output whenever the
//! unmasking cannot reach them.
//!
//! # Three behaviours that a "better" implementation would break
//!
//! * **Unmasking is a single pass in insertion order.** Abbreviations are masked
//!   before URIs, so a URI's stored text can contain `{{ABBREV_n}}` — and since
//!   `ABBREV_n` was already processed by the time `URI_m` reinserts it, it is
//!   never resolved. `tokenize("See https://a.b/i.e./x now. Done.")` with the
//!   abbreviation `i.e.` really does emit `{{ABBREV_0}}` in its output. A fixpoint
//!   loop would produce the *correct* text and fail parity.
//! * **Unmasking goes through `String.prototype.replace`, which interprets `$`
//!   in the replacement.** The replacement here is captured input, so a URL
//!   containing `$&` or ``$` `` rewrites itself. See
//!   `get_substitution`.
//! * **Array-trim runs before per-sentence trim.** `tokenize("   ")` is `[""]`,
//!   not `[]`, and `tokenize("Trailing space. ")` keeps a trailing empty
//!   sentence. Filtering empties globally, or trimming first, changes both.
//!
//! # The second constructor argument
//!
//! `index.d.ts` and the reference's own spec suggest a list of sentence
//! demarkers. The implementation names the parameter `trimSentences` and only
//! ever tests it for truthiness, so passing `['.', '!', '?']` merely means "yes,
//! trim". There is no demarker feature.

use std::borrow::Cow;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::LazyLock;

use regex::Regex;

use crate::Tokenize;
use crate::whitespace::{NON_SPACE_CLASS, ci_match_at, get_substitution, trim_units};

/// URIs, which must not be split even though they contain `.` and `/`.
///
/// Because `\S+` is greedy, a sentence-final period is swallowed into the URL:
/// `"Visit www.foo.com. Then go."` is **one** sentence. The e-mail alternative
/// does not use `\S+`, so `"mailto:a@b.co. Next."` is two.
static URI: LazyLock<Regex> = LazyLock::new(|| {
    let ns = NON_SPACE_CLASS;
    // Alternation order is the reference's, and both engines resolve it
    // leftmost-first, so the greedy `\S+` alternatives keep winning over the
    // e-mail one where they both apply.
    let pattern = format!(
        r"(?i-u:https?)://{ns}+|(?i-u:www)\.{ns}+|(?i-u:ftp)://{ns}+|(?:(?i-u:mailto):)?[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{{2,}}|(?i-u:file)://{ns}+"
    );
    Regex::new(&pattern).expect("static URI pattern")
});

/// Numbers, so that `3.14` and `1,234.50` do not end sentences.
///
/// `\b` and `\d` are ASCII in the reference. The trailing `\b` is load-bearing: a
/// bare run of four or more digits never matches at all, because `\d{1,3}` can
/// consume at most three and the next digit is still a word character. So
/// `"Number 12345 end."` leaves `12345` untouched.
///
/// It is also what protects the placeholders: `{{NUMBER_7}}` has `_` before the
/// digit, and `_` is a word character, so no boundary exists there.
static NUMBER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?-u:\b)[0-9]{1,3}(?:,[0-9]{3})*(?:\.[0-9]+)?(?-u:\b)")
        .expect("static number pattern")
});

/// Sentence delimiters, optionally preceded by more delimiters and spaces and
/// followed by one closing quote or bracket.
///
/// The greedy first group lets `"! !"`, `"..."` and `"...?"` be captured as a
/// single boundary.
static DELIMITER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new("([.?!\u{2026} ]*)([.?!\u{2026}])([\"'\u{201D}\u{2019})}\\]]?)")
        .expect("static delimiter pattern")
});

/// An insertion-ordered placeholder map. Order is observable, so this is a `Vec`
/// rather than a hash map.
type Placeholders = Vec<(String, String)>;

/// Splits text into sentences.
///
/// ```
/// use verbora_tokenizers::{SentenceTokenizer, Tokenize};
/// let t = SentenceTokenizer::new();
/// assert_eq!(t.tokenize("Hi. There!"), ["Hi.", "There!"]);
/// assert_eq!(t.tokenize("Numbers like 3.14 and 1,000.50 should not split."),
///            ["Numbers like 3.14 and 1,000.50 should not split."]);
/// // A whitespace-only input is one empty sentence, not zero sentences.
/// assert_eq!(t.tokenize("   "), [""]);
/// ```
///
/// With abbreviations, matching is case-insensitive and ordered — the first
/// listed abbreviation that matches at a position wins:
///
/// ```
/// use verbora_tokenizers::{SentenceTokenizer, Tokenize};
/// let t = SentenceTokenizer::with_abbreviations(["Dr.", "Mr."]);
/// assert_eq!(t.tokenize("Dr. Smith went home. He slept."),
///            ["Dr. Smith went home.", "He slept."]);
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SentenceTokenizer {
    abbreviations: Vec<String>,
    trim_sentences: bool,
}

/// The name the reference also exports this class under.
///
/// `SentenceTokenizer === SentenceTokenizerNew` is literally
/// true — one constructor, two names — so this is an alias rather than a second
/// type.
pub type SentenceTokenizerNew = SentenceTokenizer;

impl SentenceTokenizer {
    /// A tokenizer with no abbreviations that trims each sentence.
    #[inline]
    pub fn new() -> Self {
        Self {
            abbreviations: Vec::new(),
            trim_sentences: true,
        }
    }

    /// A tokenizer that protects the given abbreviations from being read as
    /// sentence boundaries.
    pub fn with_abbreviations<I, S>(abbreviations: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            abbreviations: abbreviations.into_iter().map(Into::into).collect(),
            trim_sentences: true,
        }
    }

    /// Sets whether each sentence is trimmed of surrounding whitespace.
    ///
    /// With trimming off, `"This is a sentence. This is another."` keeps the
    /// leading space on the second sentence.
    #[must_use]
    pub fn trimming(mut self, trim: bool) -> Self {
        self.trim_sentences = trim;
        self
    }

    /// The abbreviations this tokenizer protects.
    #[inline]
    pub fn abbreviations(&self) -> &[String] {
        &self.abbreviations
    }

    /// Runs the whole pipeline.
    fn split(&self, text: &str) -> Vec<String> {
        let mut counter = 0usize;
        let mut replacements: Placeholders = Vec::new();
        let mut delimiters: Placeholders = Vec::new();

        let phase1 = self.mask_abbreviations(text, &mut counter, &mut replacements);
        let phase2 = mask(&phase1, &URI, "URI", &mut counter, &mut replacements);
        let phase3 = mask(&phase2, &NUMBER, "NUMBER", &mut counter, &mut replacements);
        let phase4 = mask(&phase3, &DELIMITER, "DELIM", &mut counter, &mut delimiters);

        // Built once per document, not per sentence — see `unmask`'s own doc
        // comment for what this buys.
        let replacements_index = code_positions(&replacements);
        let delimiters_index = code_positions(&delimiters);

        let mut sentences: Vec<String> = split_on_delimiters(&phase4, &delimiters)
            .into_iter()
            .map(|s| {
                let unmasked = unmask(&s, &replacements, &replacements_index);
                unmask(&unmasked, &delimiters, &delimiters_index)
            })
            .collect();

        // Order matters: the array-level trim removes only leading and trailing
        // *exactly empty* sentences, and it runs before the per-sentence trim
        // that can create new ones.
        verbora_core::trim_edge_empties(&mut sentences);
        if self.trim_sentences {
            for s in &mut sentences {
                let trimmed = trim_units(s);
                if trimmed.len() != s.len() {
                    *s = trimmed.to_owned();
                }
            }
        }
        sentences
    }

    /// Phase 1: replace each abbreviation with `{{ABBREV_n}}`.
    ///
    /// The reference builds `new RegExp('(' + abbrevs.join('|') + ')', 'gi')`
    /// from escaped literals. Two properties of that are reproduced by hand
    /// rather than by `regex`: alternation is **ordered** (the first listed
    /// abbreviation that matches at a position wins, even if a later one would
    /// match more text), and `/i` uses the reference language canonicalisation rather than
    /// Unicode case folding.
    fn mask_abbreviations<'a>(
        &self,
        text: &'a str,
        counter: &mut usize,
        map: &mut Placeholders,
    ) -> Cow<'a, str> {
        if self.abbreviations.is_empty() {
            return Cow::Borrowed(text);
        }
        let mut out = String::new();
        let mut copied = 0;
        let mut i = 0;
        loop {
            let hit = self
                .abbreviations
                .iter()
                .find_map(|a| ci_match_at(text, i, a));
            if let Some(len) = hit {
                out.push_str(&text[copied..i]);
                let code = placeholder("ABBREV", counter);
                out.push_str(&code);
                map.push((code, text[i..i + len].to_owned()));
                copied = i + len;
                if len > 0 {
                    i = copied;
                    continue;
                }
                // A zero-length match: the reference language emits the replacement and then
                // steps one character, so the character itself is still copied.
            }
            if i >= text.len() {
                break;
            }
            i += text[i..].chars().next().map_or(1, char::len_utf8);
        }
        if map.is_empty() {
            return Cow::Borrowed(text);
        }
        out.push_str(&text[copied..]);
        Cow::Owned(out)
    }
}

/// `generateUniqueCode(base, index)` — `{{BASE_N}}`, with a shared counter.
///
/// The braces exist so a short placeholder cannot be found inside a longer one.
/// Input text containing a literal `{{DELIM_0}}` collides with the scheme and is
/// mangled; any "safer" scheme would change the output.
fn placeholder(base: &str, counter: &mut usize) -> String {
    let code = format!("{{{{{base}_{counter}}}}}");
    *counter += 1;
    code
}

/// Replaces every match of `re` with a fresh placeholder, recording the original
/// text in `map`.
fn mask<'a>(
    text: &'a str,
    re: &Regex,
    base: &str,
    counter: &mut usize,
    map: &mut Placeholders,
) -> Cow<'a, str> {
    let mut matches = re.find_iter(text).peekable();
    if matches.peek().is_none() {
        return Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(text.len());
    let mut copied = 0;
    for m in matches {
        out.push_str(&text[copied..m.start()]);
        let code = placeholder(base, counter);
        out.push_str(&code);
        map.push((code, m.as_str().to_owned()));
        copied = m.end();
    }
    out.push_str(&text[copied..]);
    Cow::Owned(out)
}

/// Splits on the delimiter placeholders, keeping each one attached to the
/// sentence it ends.
///
/// The reference splits with a capture group, which interleaves the delimiters
/// into the parts array, then pairs `parts[i]` with `parts[i + 1]`. The trailing
/// part is always emitted — including when it is empty — which is where the
/// trailing empty sentence of `"Trailing space. "` comes from.
/// The fixed prefix every delimiter placeholder starts with.
const DELIM_PREFIX: &str = "{{DELIM_";

fn split_on_delimiters(text: &str, delimiters: &Placeholders) -> Vec<String> {
    if delimiters.is_empty() {
        return vec![text.to_owned()];
    }
    // Searching for each of the D codes separately would cost O(D²) substring
    // searches on a document with D sentences — quadratic, and the dominant cost
    // of the whole tokenizer at document scale. Every code shares a fixed prefix,
    // so one forward scan for that prefix finds all of them; the candidate is
    // then confirmed against the map, which is what makes a literal
    // `{{DELIM_9}}` typed by the user *not* a split point unless the counter
    // happens to have reached 9.
    let codes: HashSet<&str> = delimiters.iter().map(|(c, _)| c.as_str()).collect();
    let mut out = Vec::new();
    let mut from = 0;
    let mut search = 0;
    while let Some(rel) = text[search..].find(DELIM_PREFIX) {
        let at = search + rel;
        let after = at + DELIM_PREFIX.len();
        let digits = text[after..].bytes().take_while(u8::is_ascii_digit).count();
        let end = after + digits + 2;
        if digits > 0 && text[after + digits..].starts_with("}}") && codes.contains(&text[at..end])
        {
            out.push(text[from..end].to_owned());
            from = end;
            search = end;
        } else {
            search = at + 1;
        }
    }
    out.push(text[from..].to_owned());
    out
}

/// Every `{{…}}` sequence in `s`, whether or not it is a real placeholder.
///
/// Used only to answer "does this string still contain that code". False
/// positives are harmless (one wasted map lookup); false negatives would skip a
/// substitution the reference performs, so the scan is written to have none:
///
/// * it starts a candidate at **every** `{{`, not every other one. Overlapping
///   openers are real — an empty abbreviation inserts `{{ABBREV_n}}` between
///   every pair of characters, so an input containing `{{DELIM_0}}` becomes
///   `{{{ABBREV_10}}{{{ABBREV_11}}DELIM_0}}`, in which the genuine code starts
///   one byte *inside* the first `{{`;
/// * a placeholder's interior has no braces, so the first `}}` at or after an
///   opener is exactly that placeholder's terminator.
///
/// The closing-brace cursor only moves forward, which keeps the whole scan
/// linear even on input that is mostly braces.
///
/// A sentence normally holds one placeholder — its own delimiter — so the result
/// is a one-element list and a linear membership test beats hashing.
fn present_codes(s: &str) -> Vec<&str> {
    let bytes = s.as_bytes();
    let mut found: Vec<&str> = Vec::new();
    let mut close = 0usize;
    let mut i = 0usize;
    while i + 1 < bytes.len() {
        if bytes[i] == b'{' && bytes[i + 1] == b'{' {
            if close < i {
                close = i;
            }
            while close + 1 < bytes.len() && !(bytes[close] == b'}' && bytes[close + 1] == b'}') {
                close += 1;
            }
            if close + 1 >= bytes.len() {
                break;
            }
            let code = &s[i..close + 2];
            if !found.contains(&code) {
                found.push(code);
            }
        }
        i += 1;
    }
    found
}

/// Maps each code in `map` to its position, for [`unmask`]'s own use.
///
/// Built once per document (`split` computes this outside the per-sentence
/// loop) rather than once per sentence, which is exactly the cost this
/// exists to hoist out — see [`unmask`]'s own doc comment.
fn code_positions(map: &Placeholders) -> HashMap<&str, usize> {
    map.iter()
        .enumerate()
        .map(|(i, (code, _))| (code.as_str(), i))
        .collect()
}

/// Reverses one placeholder map over `s`, in insertion order, exactly once.
///
/// See the module documentation for why a single ordered pass — rather than a
/// fixpoint — is the behaviour under test.
///
/// # Why the present-code list is recomputed rather than the map re-scanned
///
/// The reference runs one full `String#replace` per map entry per sentence,
/// which is O(sentences × placeholders) — quadratic in a document, because both
/// counts grow with its length. Every one of those passes is a no-op unless the
/// sentence actually holds that code, so this version answers the same question
/// from a list built in a single scan.
///
/// The list is rebuilt after each substitution rather than computed once, because
/// a substitution can *introduce* codes: a URI's stored text may contain an
/// abbreviation placeholder, and `$'` can splice in arbitrary text from
/// elsewhere in the sentence. Rebuilding keeps the behaviour identical to the
/// reference's repeated scanning while costing one pass per substitution that
/// actually happened — of which there are typically one or two per sentence.
///
/// # Why only a *frontier* of positions is ever visited, not every entry
///
/// An earlier version of this function still looped over every entry in
/// `map`, in order, skipping the ones the present-code list ruled out —
/// correct, but each sentence still paid to *visit* every one of a whole
/// document's placeholders (`docs/PERFORMANCE_GAPS.md` entry 23: an
/// `O(sentences × placeholders)` cost this crate's own algorithm does not
/// need to pay, since `split`'s placeholder maps only ever grow with
/// document length).
///
/// A short-lived version of this rewrite visited *only* the positions whose
/// code was present in `s` *before* any substitution ran, reasoning that a
/// later phase's stored "original" text can only ever contain an *earlier*
/// phase's code (a URI's captured span can hold `{{ABBREV_n}}`, never the
/// reverse — see `nested_placeholders_are_left_unresolved`), so nothing a
/// substitution reveals could matter beyond what the initial scan already
/// found. That argument covers genuine cross-phase swallowing, but not a
/// phase's own captured *text* coincidentally spelling out the `{{BASE_n}}`
/// syntax for an entry that has not been created yet — a caller can pass
/// `with_abbreviations(["{{ABBREV_1}}", "Y."])`, and the first
/// abbreviation's stored original text then *forward*-references the
/// second's code, which lives at a *higher* `map` position and, if that
/// second abbreviation's own real occurrence falls in a different sentence,
/// is otherwise absent from this sentence's initial scan entirely. A full
/// sweep over `map` still resolves it (position 1 has not been "passed" yet
/// when position 0's substitution reveals it); a fixed initial-only position
/// set does not, and silently diverges from the reference's single-pass
/// semantics instead of reproducing them.
///
/// So the set of positions to visit cannot be fixed up front — it has to be
/// allowed to grow, exactly when a substitution reveals a code whose
/// position is still *ahead* of the position that revealed it (never behind:
/// a position the sweep has already passed is exactly what "single pass, not
/// a fixpoint" means is left unresolved, matching the reference). `frontier`
/// is that growable set: seeded from the initial scan, popped in ascending
/// order like a full sweep would visit them, and replenished after every
/// substitution with whatever newly-present code's position lies beyond the
/// one just visited. Because insertion is restricted to positions strictly
/// greater than the one just popped, the popped sequence is always strictly
/// increasing — the same order, and the same stopping point for any given
/// reintroduction, as a full `0..map.len()` sweep — while a document with no
/// forward references (the overwhelmingly common case) never grows the
/// frontier past what the initial scan already found, keeping the original
/// performance win intact. Verified directly, not just reasoned about,
/// against a reference implementation that still visits every `map` entry,
/// in `tests::unmask_matches_a_full_scan_on_randomized_documents` and
/// `tests::unmask_resolves_a_forward_referencing_abbreviation_across_a_
/// sentence_boundary` (the forward-reference shape above, which the
/// fixed-position-set version failed).
fn unmask(s: &str, map: &Placeholders, index: &HashMap<&str, usize>) -> String {
    // Cheap rejections: an empty map has nothing to reverse, and a sentence with
    // no `{{` cannot contain a placeholder. Between them these cover every
    // sentence of a document with no abbreviations, URIs or numbers in it.
    if map.is_empty() || !s.contains("{{") {
        return s.to_owned();
    }
    let mut cur = s.to_owned();
    let mut present: Vec<String> = present_codes(&cur).into_iter().map(str::to_owned).collect();
    let mut frontier: BTreeSet<usize> = present
        .iter()
        .filter_map(|c| index.get(c.as_str()).copied())
        .collect();

    while let Some(pos) = frontier.pop_first() {
        if present.is_empty() {
            break;
        }
        let (code, original) = &map[pos];
        if !present.iter().any(|p| p == code) {
            continue;
        }
        cur = replace_all_pattern(&cur, code, original);
        present = present_codes(&cur).into_iter().map(str::to_owned).collect();
        for c in &present {
            if let Some(&later) = index.get(c.as_str()) {
                if later > pos {
                    frontier.insert(later);
                }
            }
        }
    }
    cur
}

/// `subject.replace(new RegExp(escaped, 'g'), template)` for a literal needle.
///
/// The template is untrusted captured text, so `$$`, `$&`, ``$` `` and `$'` are
/// expanded per the reference language, against the subject as it stands at the start of this
/// pass.
fn replace_all_pattern(subject: &str, needle: &str, template: &str) -> String {
    let mut out = String::with_capacity(subject.len());
    let mut from = 0;
    while let Some(rel) = subject[from..].find(needle) {
        let at = from + rel;
        out.push_str(&subject[from..at]);
        out.push_str(&get_substitution(needle, at, subject, template));
        from = at + needle.len();
    }
    out.push_str(&subject[from..]);
    out
}

impl Tokenize for SentenceTokenizer {
    type Token<'a> = String;

    /// Sentence splitting is a whole-text operation — the placeholder maps must
    /// be complete before any sentence can be unmasked — so the work happens
    /// eagerly and the iterator walks the finished list.
    fn tokens(&self, text: &str) -> impl Iterator<Item = String> {
        self.split(text).into_iter()
    }
}

impl verbora_core::Tokenizer for SentenceTokenizer {
    fn tokenize_into(&self, text: &str, out: &mut Vec<String>) {
        out.extend(self.split(text));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn st(s: &str) -> Vec<String> {
        SentenceTokenizer::new().tokenize(s)
    }

    #[test]
    fn boundary_inputs() {
        assert_eq!(st(""), Vec::<String>::new());
        assert_eq!(st("   "), [""]);
        assert_eq!(st("\n\n"), [""]);
        assert_eq!(st("Trailing space. "), ["Trailing space.", ""]);
    }

    #[test]
    fn basic_splitting() {
        assert_eq!(st("Hi."), ["Hi."]);
        assert_eq!(st("Hi"), ["Hi"]);
        assert_eq!(st("."), ["."]);
        assert_eq!(st("..."), ["..."]);
        assert_eq!(st("!!!"), ["!!!"]);
        assert_eq!(st("a...b"), ["a...", "b"]);
        assert_eq!(st("A? B! C… D."), ["A?", "B!", "C…", "D."]);
        assert_eq!(st("A.  B."), ["A.", "B."]);
        assert_eq!(st("x.)y. z."), ["x.)", "y.", "z."]);
        assert_eq!(
            st("He said \"Hi.\" Then left."),
            ["He said \"Hi.\"", "Then left."]
        );
    }

    #[test]
    fn numbers_and_uris_are_protected() {
        assert_eq!(
            st("Numbers like 3.14 and 1,000.50 should not split."),
            ["Numbers like 3.14 and 1,000.50 should not split."]
        );
        // Four or more digits never match the number pattern, and there is no
        // delimiter, so this is still one sentence.
        assert_eq!(st("Number 12345 end."), ["Number 12345 end."]);
        // `\S+` swallows the trailing period.
        assert_eq!(
            st("Visit www.foo.com. Then go."),
            ["Visit www.foo.com. Then go."]
        );
        assert_eq!(st("mailto:a@b.co. Next."), ["mailto:a@b.co.", "Next."]);
    }

    #[test]
    fn abbreviations_are_case_insensitive() {
        let t = SentenceTokenizer::with_abbreviations(["Inc."]);
        assert_eq!(
            t.tokenize("INC. uppercase abbrev test. Yes."),
            ["INC. uppercase abbrev test.", "Yes."]
        );
    }

    #[test]
    fn nested_placeholders_are_left_unresolved() {
        // Abbreviations are masked before URIs, so the URI's stored text contains
        // `{{ABBREV_0}}` — and the single ordered unmasking pass never gets back
        // to it. A fixpoint implementation would "fix" this and fail parity.
        let t = SentenceTokenizer::with_abbreviations(["i.e.", "Inc."]);
        assert_eq!(
            t.tokenize("See https://a.b/i.e./x now. Done."),
            ["See https://a.b/{{ABBREV_0}}/x now.", "Done."]
        );
    }

    #[test]
    fn literal_placeholders_in_the_input_collide() {
        assert_eq!(
            st("Contains {{DELIM_0}} literal. Next."),
            ["Contains .", "literal.", "Next."]
        );
    }

    #[test]
    fn dollar_sequences_in_a_url_are_expanded() {
        // `$&` expands to the matched placeholder text, so the URL rewrites itself.
        assert_eq!(
            st("Go to https://a.b/?q=$&z now. Done."),
            ["Go to https://a.b/?q={{URI_0}}z now.", "Done."]
        );
    }

    #[test]
    fn trimming_can_be_disabled() {
        let t = SentenceTokenizer::new().trimming(false);
        assert_eq!(
            t.tokenize("This is a sentence. This is another sentence."),
            ["This is a sentence.", " This is another sentence."]
        );
    }

    #[test]
    fn the_new_alias_is_the_same_type() {
        let a: SentenceTokenizer = SentenceTokenizerNew::new();
        assert_eq!(a, SentenceTokenizer::new());
    }

    // -----------------------------------------------------------------------
    // `unmask`'s index-based rewrite vs. a full-scan reference
    // -----------------------------------------------------------------------

    /// `unmask` before the `docs/PERFORMANCE_GAPS.md` entry 23 rewrite: a
    /// plain forward scan over every entry in `map`, skipping ones the
    /// present-code list rules out. Kept only as the reference the
    /// differential test below checks the real implementation against.
    fn unmask_naive(s: &str, map: &Placeholders) -> String {
        if map.is_empty() || !s.contains("{{") {
            return s.to_owned();
        }
        let mut cur = s.to_owned();
        let mut present: Vec<String> = present_codes(&cur).into_iter().map(str::to_owned).collect();
        for (code, original) in map {
            if present.is_empty() {
                break;
            }
            if !present.iter().any(|p| p == code) {
                continue;
            }
            cur = replace_all_pattern(&cur, code, original);
            present = present_codes(&cur).into_iter().map(str::to_owned).collect();
        }
        cur
    }

    /// `SentenceTokenizer::split`, with `unmask_naive` wired in instead of
    /// the indexed rewrite — otherwise byte-for-byte the same pipeline.
    fn split_naive(t: &SentenceTokenizer, text: &str) -> Vec<String> {
        let mut counter = 0usize;
        let mut replacements: Placeholders = Vec::new();
        let mut delimiters: Placeholders = Vec::new();

        let phase1 = t.mask_abbreviations(text, &mut counter, &mut replacements);
        let phase2 = mask(&phase1, &URI, "URI", &mut counter, &mut replacements);
        let phase3 = mask(&phase2, &NUMBER, "NUMBER", &mut counter, &mut replacements);
        let phase4 = mask(&phase3, &DELIMITER, "DELIM", &mut counter, &mut delimiters);

        let mut sentences: Vec<String> = split_on_delimiters(&phase4, &delimiters)
            .into_iter()
            .map(|s| {
                let unmasked = unmask_naive(&s, &replacements);
                unmask_naive(&unmasked, &delimiters)
            })
            .collect();

        verbora_core::trim_edge_empties(&mut sentences);
        if t.trim_sentences {
            for s in &mut sentences {
                let trimmed = trim_units(s);
                if trimmed.len() != s.len() {
                    *s = trimmed.to_owned();
                }
            }
        }
        sentences
    }

    /// A tiny, dependency-free PRNG — this crate has no `rand` dependency
    /// and does not need one just for this.
    struct Xorshift64(u64);

    impl Xorshift64 {
        fn next_u64(&mut self) -> u64 {
            let mut x = self.0;
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            self.0 = x;
            x
        }

        fn next_range(&mut self, bound: usize) -> usize {
            (self.next_u64() % bound as u64) as usize
        }

        fn choose<'a, T>(&mut self, items: &'a [T]) -> &'a T {
            &items[self.next_range(items.len())]
        }
    }

    /// Builds a random document out of pieces deliberately chosen to stress
    /// every reintroduction path `unmask`'s own doc comment reasons about:
    /// abbreviations, URIs (including ones built to *contain* an
    /// abbreviation's literal text, the documented nested-placeholder
    /// shape), numbers, `$`-sequence-bearing URIs, literal `{{...}}` text,
    /// and plain sentences, joined with real sentence delimiters.
    fn random_document(rng: &mut Xorshift64) -> String {
        const SENTENCE_PIECES: &[&str] = &[
            "This is a plain sentence",
            "Contact us at i.e. the office",
            "See https://a.b/i.e./x now",
            "Visit https://example.com/path now",
            "Go to https://a.b/?q=$&z now",
            "Go to https://a.b/?q=$'z now",
            "The price is 3.14 dollars",
            "Call Inc. at 1,234.50 today",
            "Contains {{DELIM_0}} literal text",
            "Dr. Smith works at the Inc. office",
            "Email a@b.co for details",
            "A short one",
        ];
        const DELIMS: &[&str] = &[". ", "! ", "? ", "... ", ".\" ", "?) "];

        let count = 1 + rng.next_range(6);
        let mut doc = String::new();
        for _ in 0..count {
            doc.push_str(rng.choose(SENTENCE_PIECES));
            doc.push_str(rng.choose(DELIMS));
        }
        doc
    }

    #[test]
    fn unmask_matches_a_full_scan_on_randomized_documents() {
        let mut rng = Xorshift64(0xFEED_C0DE_BEEF_0001);
        let t = SentenceTokenizer::with_abbreviations(["i.e.", "Inc.", "Dr."]);

        for round in 0..500 {
            let doc = random_document(&mut rng);
            let fast = t.tokenize(&doc);
            let naive = split_naive(&t, &doc);
            assert_eq!(fast, naive, "round {round}: document {doc:?}");
        }
    }

    #[test]
    fn unmask_resolves_a_forward_referencing_abbreviation_across_a_sentence_boundary() {
        // A caller-supplied abbreviation can contain `{{` / `}}` — nothing
        // stops it. Here the first abbreviation's literal text is exactly
        // the placeholder syntax `unmask` will later assign to the *second*
        // abbreviation, which does not get matched until later in the
        // document — so `{{ABBREV_0}}`'s stored original text
        // *forward*-references `{{ABBREV_1}}`, a position that comes after
        // it in `replacements`. The second abbreviation's own real
        // occurrence ("QQQ") lands in a different sentence, so the sentence
        // holding `{{ABBREV_0}}` never has `{{ABBREV_1}}` present at the
        // very start — only after `{{ABBREV_0}}` is substituted. A full
        // sweep over `replacements` still resolves it (position 1 has not
        // been passed yet); a version of `unmask` that only ever visits the
        // positions present at the very start does not, and used to leave
        // `{{ABBREV_1}}` unresolved in the output instead.
        let t = SentenceTokenizer::with_abbreviations(["{{ABBREV_1}}", "QQQ"]);
        assert_eq!(t.tokenize("{{ABBREV_1}} Z! QQQ W."), ["QQQ Z!", "QQQ W."]);
    }

    #[test]
    fn unmask_matches_a_full_scan_with_no_abbreviations_configured() {
        // No abbreviations means `replacements` only ever holds URI/NUMBER
        // codes -- a different, still real shape (no cross-phase
        // reintroduction possible at all) worth its own sweep.
        let mut rng = Xorshift64(0x0BAD_F00D_CAFE_0002);
        let t = SentenceTokenizer::new();

        for round in 0..200 {
            let doc = random_document(&mut rng);
            let fast = t.tokenize(&doc);
            let naive = split_naive(&t, &doc);
            assert_eq!(fast, naive, "round {round}: document {doc:?}");
        }
    }
}
